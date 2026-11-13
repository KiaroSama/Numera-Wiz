//! One fixed dispatcher consumes accepted durable work without one waiting task per job.
use super::*;
use nw_domain::media_execution::{MediaJobState, StoredJob};

pub(super) fn process_lifetime_uncertain(error: &DomainError) -> bool {
    matches!(
        error.code.as_str(),
        "TREE_TERMINATION_UNCERTAIN"
            | "PROCESS_WAIT_FAILED"
            | "PROCESS_CAPTURE_FAILED"
            | "PROCESS_LAUNCH_FAILED"
    )
}

fn media_failure_state(state: MediaJobState, error: &DomainError) -> MediaJobState {
    if process_lifetime_uncertain(error) {
        MediaJobState::Failed
    } else if error.code == "CANCELLED" || state == MediaJobState::Cancelling {
        MediaJobState::Cancelled
    } else {
        MediaJobState::Failed
    }
}

impl Service {
    pub(super) fn quarantine_execution(&self) {
        if !self.execution_quarantined.swap(true, Ordering::AcqRel) {
            // Keep one job-wide reservation after the ordinary Pending guard is
            // dropped. Startup reconstructs it from the exact retained error.
            self.pending.fetch_add(1, Ordering::AcqRel);
            if let Ok(permit) = self.heavy.clone().try_acquire_owned() {
                permit.forget();
            }
            self.heavy.close();
        }
    }

    pub(super) fn resume_dispatcher(self: &Arc<Self>) -> Result<()> {
        if !self.dispatcher_ready.load(Ordering::Acquire)
            || self.execution_quarantined.load(Ordering::Acquire)
        {
            return Ok(());
        }
        if self.store.pending_migration()?.is_some() || self.store.pending_media_maintenance()? {
            return Ok(());
        }
        let receiver = self
            .dispatcher_receiver
            .lock()
            .map_err(|_| lock_error())?
            .take();
        let Some(receiver) = receiver else {
            return Ok(());
        };
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
        if self.execution_quarantined.load(Ordering::Acquire) {
            *self.dispatcher_receiver.lock().map_err(|_| lock_error())? = Some(receiver);
            return Ok(());
        }
        Self::start_dispatcher(self, receiver);
        self.store.visit_executions(|job| {
            if job.queued() {
                self.schedule(job.id().to_owned())?;
            }
            Ok(())
        })
    }
    pub(super) fn start_dispatcher(
        service: &Arc<Self>,
        mut receiver: tokio::sync::mpsc::Receiver<String>,
    ) {
        let weak = Arc::downgrade(service);
        tokio::spawn(async move {
            while let Some(id) = receiver.recv().await {
                let Some(service) = weak.upgrade() else {
                    break;
                };
                let _pending = Pending(service.pending.clone());
                let token = match service.cancellations.lock() {
                    Ok(tokens) => tokens.get(&id).cloned(),
                    Err(_) => None,
                };
                let Some(token) = token else {
                    continue;
                };
                let permit = match service.heavy.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                let inspect = service.clone();
                let job_id = id.clone();
                let kind =
                    tokio::task::spawn_blocking(move || inspect.store.inspect_execution(&job_id))
                        .await;
                match kind {
                    Ok(Ok(StoredJob::Legacy(_))) => service.run_legacy_scheduled(id, token).await,
                    Ok(Ok(StoredJob::MediaV1(_))) => {
                        let outcome = service.run_media_job(&id, token).await;
                        let uncertain = outcome.as_ref().is_err_and(process_lifetime_uncertain);
                        if uncertain {
                            service.quarantine_execution();
                            permit.forget();
                        }
                        let finish = service.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            let _guard = finish.mutation.lock().map_err(|_| lock_error())?;
                            let StoredJob::MediaV1(job) = finish.store.inspect_execution(&id)?
                            else {
                                return Err(DomainError::new(
                                    "INVALID_RECORD",
                                    "Media dispatcher identity changed",
                                ));
                            };
                            if job.state == MediaJobState::Publishing && !uncertain {
                                finish.reconcile_media(&job)?;
                            } else if !job.state.terminal() {
                                finish.store.update_media_job(&id, |job| {
                                    let error = outcome.err().unwrap_or_else(|| {
                                        DomainError::new(
                                            "VERIFICATION_REQUIRED",
                                            "Media engine ended without a verified publication",
                                        )
                                    });
                                    job.state = media_failure_state(job.state, &error);
                                    job.error = Some(error);
                                    Ok(())
                                })?;
                            }
                            finish
                                .cancellations
                                .lock()
                                .map_err(|_| lock_error())?
                                .remove(&id);
                            let _ = finish.events.send(id);
                            Ok::<_, DomainError>(())
                        })
                        .await;
                        if uncertain {
                            break;
                        }
                    }
                    Ok(Err(error)) => {
                        if let Ok(mut tokens) = service.cancellations.lock() {
                            tokens.remove(&id);
                        }
                        tracing::error!(code=%error.code, job_id=%id, "Retained queued job failed closed during dispatch");
                    }
                    Err(error) => {
                        if let Ok(mut tokens) = service.cancellations.lock() {
                            tokens.remove(&id);
                        }
                        tracing::error!(job_id=%id, %error, "Job dispatcher worker failed");
                    }
                }
            }
        });
    }
}

#[cfg(test)]
pub(super) async fn exercise_uncertain_cancellation() {
    for code in ["TREE_TERMINATION_UNCERTAIN", "PROCESS_WAIT_FAILED", "PROCESS_CAPTURE_FAILED", "PROCESS_LAUNCH_FAILED"] {
        let error = DomainError::new(code, "deterministic supervisor lifetime result");
        assert!(process_lifetime_uncertain(&error));
        assert_eq!(media_failure_state(MediaJobState::Cancelling, &error), MediaJobState::Failed);
        assert_eq!(error.code, code, "The supervisor's exact error is retained");
    }
    for code in ["CANCELLED", "PROCESS_FAILED"] {
        let error = DomainError::new(code, "confirmed supervised completion");
        assert!(!process_lifetime_uncertain(&error));
        assert_eq!(media_failure_state(MediaJobState::Cancelling, &error), MediaJobState::Cancelled);
    }
    let temporary = tempfile::tempdir().unwrap();
    let service = Service::open(temporary.path().to_owned(), None).await.unwrap();
    let scope = service.store.issue_scope(&service.owner().principal, service.clock.now().unwrap()).unwrap();
    let permit = service.heavy.clone().acquire_owned().await.unwrap();
    service.pending.fetch_add(1, Ordering::AcqRel);
    let running = Pending(service.pending.clone());
    service.quarantine_execution();
    permit.forget();
    drop(running);
    service.quarantine_execution();
    assert_eq!(service.pending.load(Ordering::Acquire), 1);
    assert_eq!(service.heavy.available_permits(), 0);
    assert!(service.heavy.clone().try_acquire_owned().is_err());
    let discovery = service.dispatch(service.owner(), Request::query("commands.list", json!({}))).await;
    assert!(discovery.ok, "{:?}", discovery.error);
    let inspection = service.dispatch(service.owner(), Request::query("jobs.list", json!({}))).await;
    assert!(inspection.ok, "{:?}", inspection.error);
    let maintenance = service.dispatch(service.owner(), Request::query("maintenance.plan", json!({}))).await;
    assert_eq!(maintenance.error.unwrap().code, "OWNER_BUSY");
    let mut submit = Request::query("jobs.submit", json!({"plan_id":Uuid::new_v4().to_string()}));
    submit.replay_scope = Some(scope);
    submit.idempotency_key = Some("quarantined-fresh-work".into());
    let refused = service.dispatch(service.owner(), submit).await;
    assert_eq!(refused.error.unwrap().code, "RESOURCE_LIMIT");
}
