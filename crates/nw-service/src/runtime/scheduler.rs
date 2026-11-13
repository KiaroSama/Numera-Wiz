//! One fixed dispatcher consumes accepted durable work without one waiting task per job.
use super::*;
use nw_domain::media_execution::{MediaJobState, StoredJob};

impl Service {
    pub(super) fn resume_dispatcher(self: &Arc<Self>) -> Result<()> {
        if !self.dispatcher_ready.load(Ordering::Acquire) { return Ok(()); }
        if self.store.pending_migration()?.is_some() || self.store.pending_media_maintenance()? {
            return Ok(());
        }
        let receiver = self.dispatcher_receiver.lock().map_err(|_| lock_error())?.take();
        let Some(receiver) = receiver else { return Ok(()); };
        // No await separates taking the receiver from restoring it or handing it
        // to the fixed dispatcher. Caller cancellation cannot drop this ownership.
        let recovered = (|| {
            let _guard = self.mutation.lock().map_err(|_| lock_error())?;
            self.recover_uploads()?;
            self.recover()
        })();
        if let Err(error) = recovered {
            *self.dispatcher_receiver.lock().map_err(|_| lock_error())? = Some(receiver);
            return Err(error);
        }
        Self::start_dispatcher(self, receiver);
        self.store.visit_executions(|job| {
            if job.queued() { self.schedule(job.id().to_owned())?; }
            Ok(())
        })
    }
    pub(super) fn start_dispatcher(service: &Arc<Self>, mut receiver: tokio::sync::mpsc::Receiver<String>) {
        let weak = Arc::downgrade(service);
        tokio::spawn(async move {
            while let Some(id) = receiver.recv().await {
                let Some(service) = weak.upgrade() else { break; };
                let _pending = Pending(service.pending.clone());
                let token = match service.cancellations.lock() {
                    Ok(tokens) => tokens.get(&id).cloned(),
                    Err(_) => None,
                };
                let Some(token) = token else { continue; };
                let _permit = match service.heavy.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                let inspect = service.clone();
                let job_id = id.clone();
                let kind = tokio::task::spawn_blocking(move || inspect.store.inspect_execution(&job_id)).await;
                match kind {
                    Ok(Ok(StoredJob::Legacy(_))) => service.run_legacy_scheduled(id, token).await,
                    Ok(Ok(StoredJob::MediaV1(_))) => {
                        let outcome = service.run_media_job(&id, token).await;
                        let finish = service.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            let _guard = finish.mutation.lock().map_err(|_| lock_error())?;
                            let StoredJob::MediaV1(job) = finish.store.inspect_execution(&id)? else {
                                return Err(DomainError::new("INVALID_RECORD", "Media dispatcher identity changed"));
                            };
                            if job.state == MediaJobState::Publishing {
                                finish.reconcile_media(&job)?;
                            } else if !job.state.terminal() {
                                finish.store.update_media_job(&id, |job| {
                                    let error = outcome.err().unwrap_or_else(|| DomainError::new("VERIFICATION_REQUIRED", "Media engine ended without a verified publication"));
                                    job.state = if error.code == "CANCELLED" || job.state == MediaJobState::Cancelling {
                                        MediaJobState::Cancelled
                                    } else {
                                        MediaJobState::Failed
                                    };
                                    job.error = Some(error);
                                    Ok(())
                                })?;
                            }
                            finish.cancellations.lock().map_err(|_| lock_error())?.remove(&id);
                            let _ = finish.events.send(id);
                            Ok::<_, DomainError>(())
                        }).await;
                    }
                    Ok(Err(error)) => {
                        if let Ok(mut tokens) = service.cancellations.lock() { tokens.remove(&id); }
                        tracing::error!(code=%error.code, job_id=%id, "Retained queued job failed closed during dispatch");
                    }
                    Err(error) => {
                        if let Ok(mut tokens) = service.cancellations.lock() { tokens.remove(&id); }
                        tracing::error!(job_id=%id, %error, "Job dispatcher worker failed");
                    }
                }
            }
        });
    }
}
