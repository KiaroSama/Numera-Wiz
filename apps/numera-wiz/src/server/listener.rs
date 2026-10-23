//! Public sessions own their listener independently of background/private work.
use super::*;
pub(super) struct PublicListener {pub port:u16,pub stop:CancellationToken,pub last_used:Instant}
#[derive(Clone)]
pub(super) struct PublicRoute(pub bool);
impl Owner {
    async fn reap_public(&self,public_busy:bool)->anyhow::Result<()> {
        let mut active=self.public.lock().await;
        if let Some(listener)=active.as_mut(){
            if public_busy {listener.last_used=Instant::now();}
            else if listener.last_used.elapsed()>=Duration::from_secs(15){
                let listener=active.take().expect("selected listener");listener.stop.cancel();
                self.service.advance_origin(&format!("http://127.0.0.1:{}",listener.port))?;
                self.pairing.lock().await.clear();
                tracing::info!(port=listener.port,"Public listener drained independently of private owner work");
            }
        }Ok(())
    }
    pub(super) async fn monitor(self:Arc<Self>){
        let mut idle=Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut leases=self.leases.lock().await;
            leases.retain(|_,lease|lease.expires>Instant::now() && self.service.client_lease_allowed(&Context{principal:lease.principal.clone(),profile:lease.profile.clone()}).is_ok());
            let public_busy=self.public_active.load(Ordering::Acquire)!=0 || leases.values().any(|lease|lease.public);
            // Admission takes this same lease guard before observing listener shutdown.
            if let Err(error)=self.reap_public(public_busy).await{tracing::error!(%error,"Public listener drain failed");self.stop.cancel();break;}
            if self.service.busy() || self.active.load(Ordering::Acquire)!=0 || !leases.is_empty(){idle=Instant::now();}
            if idle.elapsed()>=Duration::from_secs(15){self.stop.cancel();break;}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn private_editor_lease_does_not_keep_a_public_listener_or_its_old_tokens_live(){
        let root=tempfile::tempdir().unwrap();let service=Service::open(root.path().to_owned(),None).await.unwrap();
        let origin="http://127.0.0.1:1578";service.advance_origin(origin).unwrap();let old=service.credential_for_origin("browser",Some(origin.into())).unwrap();assert!(service.authenticate_at(&old,Some(origin),true).is_ok());
        let stop=CancellationToken::new();let owner=Owner{record:OwnerRecord{canonical_root:"test".into(),root_id:service.store.root_id().into(),epoch:service.epoch.clone(),build:"test".into(),registry:registry_fingerprint(),port:1,bearer:"private-test".into()},service:service.clone(),leases:Mutex::new(HashMap::from([("private-editor".into(),Lease{principal:service.owner().principal,profile:None,public:false,expires:Instant::now()+Duration::from_secs(90)})])),public:Mutex::new(Some(PublicListener{port:1578,stop:stop.clone(),last_used:Instant::now()-Duration::from_secs(16)})),public_active:Arc::new(AtomicUsize::new(0)),stop:CancellationToken::new(),active:Arc::new(AtomicUsize::new(0)),slots:Arc::new(Semaphore::new(32)),pairing:Mutex::new(HashMap::new())};
        owner.reap_public(true).await.unwrap();assert!(!stop.is_cancelled());owner.public.lock().await.as_mut().unwrap().last_used=Instant::now()-Duration::from_secs(16);
        owner.reap_public(false).await.unwrap();assert!(stop.is_cancelled());assert!(owner.public.lock().await.is_none());assert!(!owner.stop.is_cancelled());assert_eq!(owner.leases.lock().await.len(),1);assert!(service.authenticate_at(&old,Some(origin),true).is_err());
        service.advance_origin(origin).unwrap();let renewed=service.credential_for_origin("browser",Some(origin.into())).unwrap();assert!(service.authenticate_at(&renewed,Some(origin),true).is_ok());assert!(service.authenticate_at(&old,Some(origin),true).is_err());
    }
}
