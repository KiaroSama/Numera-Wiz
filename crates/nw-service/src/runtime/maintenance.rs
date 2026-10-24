//! Owner maintenance uses canonical commands, scopes and durable receipts.
use super::*;
pub(super) enum Admission<'a> {
    Ordinary {
        _lease: std::sync::RwLockReadGuard<'a, ()>,
    },
    Maintenance {
        _lease: std::sync::RwLockWriteGuard<'a, ()>,
    },
}
fn busy() -> DomainError {
    DomainError::new(
        "OWNER_BUSY",
        "Maintenance requires ordinary commands, credentials, transfers and intake to drain",
    )
}
impl Service {
    pub(super) fn ordinary_admission(&self) -> Result<Admission<'_>> {
        let lease = self.maintenance.try_read().map_err(|_| busy())?;
        if self.store.pending_migration()?.is_some() {
            return Err(busy());
        }
        Ok(Admission::Ordinary { _lease: lease })
    }
    pub(super) fn command_admission(&self, id: &str) -> Result<Admission<'_>> {
        if ["maintenance.plan", "maintenance.migrate"].contains(&id) {
            self.maintenance
                .try_write()
                .map(|lease| Admission::Maintenance { _lease: lease })
                .map_err(|_| busy())
        } else if ["maintenance.inspect", "commands.list", "commands.schema"].contains(&id) {
            self.maintenance
                .try_read()
                .map(|lease| Admission::Ordinary { _lease: lease })
                .map_err(|_| busy())
        } else {
            self.ordinary_admission()
        }
    }
    fn maintenance_build(&self, cancel: &CancellationToken) -> Result<String> {
        files::identity(&std::env::current_exe().map_err(files::io_error)?, cancel)
            .map(|value| value.sha256)
    }
    pub(super) fn maintenance_command(
        self: &Arc<Self>,
        context: &Context,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if self.policy(context)?.is_some() {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Generation maintenance requires native owner authority",
            ));
        }
        if request.command.id == "maintenance.inspect" {
            let input: IdInput = commands::parse(request.params.clone())?;
            return Ok(Response::success(
                &request.request_id,
                self.store.inspect_migration(&input.id)?,
            ));
        }
        // The admission write guard excludes new work. Work already accepted by
        // the sole scheduler must finish, or be explicitly cancelled, before review.
        if self.pending.load(Ordering::Acquire) != 0 || self.transfers.available_permits() != 2 {
            return Err(busy());
        }
        let build = self.maintenance_build(cancel)?;
        match request.command.id.as_str() {
            "maintenance.plan" => {
                let _: Empty = commands::parse(request.params.clone())?;
                let plan =
                    self.store
                        .prepare_migration(&self.root_lease, &build, self.clock.now()?)?;
                Ok(Response::success(&request.request_id, plan))
            }
            "maintenance.migrate" => self.store.migrate_generation(
                &self.root_lease,
                &context.principal,
                request,
                &build,
                || self.clock.now(),
                cancel,
            ),
            _ => Err(DomainError::new(
                "UNKNOWN_COMMAND",
                "No maintenance handler",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn actual_service_admission_receipts_owner_lock_and_selected_generation_survive_restart()
    {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().to_owned();
        let service = Service::open(root.clone(), None).await.unwrap();
        match Service::open(root.clone(), None).await {
            Err(error) => assert_eq!(error.code, "OWNER_BUSY"),
            Ok(_) => panic!("A second root owner was admitted"),
        }
        let owner = service.owner();
        let principal = owner.principal.clone();
        let ordinary = service.ordinary_admission().unwrap();
        assert!(service.command_admission("maintenance.plan").is_err());
        drop(ordinary);
        let browser = service.profile_context("browser").unwrap();
        let denied = service
            .dispatch(browser, Request::query("maintenance.plan", json!({})))
            .await;
        assert_eq!(denied.error.unwrap().code, "PERMISSION_DENIED");
        assert!(!nw_domain::access::ordinary_commands().contains("maintenance.migrate"));
        let scope = service
            .dispatch(
                owner.clone(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await;
        let scope = scope.result.unwrap()["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut original = Request::query(
            "access.profiles.create",
            json!({"name":"retained","read_roots":[],"write_roots":[]}),
        );
        original.replay_scope = Some(scope.clone());
        original.idempotency_key = Some("before".into());
        let accepted_before = service.dispatch(owner.clone(), original.clone()).await;
        assert!(accepted_before.ok, "{:?}", accepted_before.error);
        let reviewed = service
            .dispatch(owner.clone(), Request::query("maintenance.plan", json!({})))
            .await;
        assert!(reviewed.ok, "{:?}", reviewed.error);
        let plan: nw_domain::maintenance::MigrationPlan =
            serde_json::from_value(reviewed.result.unwrap()).unwrap();
        let mut migrate = Request::query("maintenance.migrate", json!({"plan":plan}));
        migrate.replay_scope = Some(scope);
        migrate.idempotency_key = Some("one-migration".into());
        let accepted = service.dispatch(owner, migrate.clone()).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        let migration_id = accepted.result.as_ref().unwrap()["migration_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(service.store.store_version().unwrap(), 3);
        assert!(service.store.pending_migration().unwrap().is_none());
        let profile_before: AccessProfile = service.store.get("profile/retained").unwrap().unwrap();
        drop(service);
        let restarted = Service::open(root, None).await.unwrap();
        let owner = restarted.owner();
        assert_eq!(owner.principal, principal);
        assert_eq!(restarted.root_lease.selection().unwrap().generation, 3);
        let activation=restarted.root_lease.selection().unwrap().activation_id.unwrap();
        assert_eq!(restarted.store.adoption_revision(&migration_id,&activation).unwrap(),"0");
        assert_eq!(nw_platform::files::RootLock::acquire(&restarted.root.join("state/generations/1")).err().unwrap().code,"OWNER_BUSY");

        assert_eq!(
            restarted
                .store
                .inspect_migration(&migration_id)
                .unwrap()
                .phase,
            nw_domain::maintenance::MigrationPhase::Succeeded
        );
        assert_eq!(
            restarted.dispatch(owner.clone(), migrate).await.receipt_id,
            accepted.receipt_id
        );
        assert_eq!(
            restarted.dispatch(owner, original).await.receipt_id,
            accepted_before.receipt_id
        );
        let retained: AccessProfile = restarted.store.get("profile/retained").unwrap().unwrap();
        assert_eq!(profile_before.principal, retained.principal);
        assert_eq!(profile_before.revision, retained.revision);
        assert_eq!(restarted.store.adoption_revision(&migration_id,&activation).unwrap(),"0","Read-only reopen and exact receipt replay do not adopt");
        let new_scope=restarted.dispatch(restarted.owner(),Request::query("access.replay_scope.create",json!({}))).await.result.unwrap()["replay_scope"].as_str().unwrap().to_owned();
        let mut fresh=Request::query("access.profiles.create",json!({"name":"new-after-migration","read_roots":[],"write_roots":[]}));
        fresh.replay_scope=Some(new_scope);
        fresh.idempotency_key=Some("new-accepted-business-state".into());
        let accepted=restarted.dispatch(restarted.owner(),fresh).await;
        assert!(accepted.ok,"{:?}",accepted.error);
        assert_eq!(restarted.store.adoption_revision(&migration_id,&activation).unwrap(),"1");

    }
}
