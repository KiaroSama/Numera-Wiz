//! Plain controls reached through the original real snapshot lifetime test entry.
use super::*;
use nw_domain::commands::ProjectRef;

fn capture(service: &Arc<Service>, context: &Context, project: &str) -> Result<String> {
    let mut request = Request::query("projects.inspect", json!({"projection":"summary"}));
    request.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: None,
    });
    let response = service.dispatch_sync(context.clone(), request, CancellationToken::new())?;
    if !response.ok || response.receipt_id.is_some() {
        return Err(response.error.unwrap_or_else(|| {
            DomainError::new(
                "PRESSURE_CONTROL_FAILED",
                "Capture query produced a receipt",
            )
        }));
    }
    response
        .result
        .and_then(|result| result["snapshot_id"].as_str().map(str::to_owned))
        .ok_or_else(|| DomainError::new("PRESSURE_CONTROL_FAILED", "Capture identity missing"))
}

async fn while_observing<T>(
    service: &Arc<Service>,
    projected: usize,
    observe: impl FnOnce(&Service) -> Result<u64> + Send + 'static,
    during: impl FnOnce() -> T,
) -> (T, Result<()>) {
    let (started, ready) = std::sync::mpsc::channel();
    let (resume, waiting) = std::sync::mpsc::channel();
    let worker = service.clone();
    let task = tokio::task::spawn_blocking(move || {
        worker.project_snapshot_pressure_observing(projected, || {
            started.send(()).map_err(|_| lock_error())?;
            waiting.recv_timeout(Duration::from_secs(2)).map_err(|_| {
                DomainError::new("PRESSURE_CONTROL_FAILED", "Observer not released")
            })?;
            observe(&worker)
        })
    });
    let reached = ready.recv_timeout(Duration::from_secs(2));
    // The original entry uses a current-thread runtime. These synchronous, real dispatcher
    // calls keep the periodic reaper from adding an unrelated contender to this control.
    // Catch a query panic so the controlled observer is released and reaped first.
    let outcome = reached
        .as_ref()
        .ok()
        .map(|_| std::panic::catch_unwind(std::panic::AssertUnwindSafe(during)));
    let released = resume.send(());
    let joined = tokio::time::timeout(Duration::from_secs(5), task).await;
    assert!(
        reached.is_ok(),
        "Pressure observer was not reached: {reached:?}"
    );
    assert!(released.is_ok(), "Pressure observer stopped before release");
    let pressure = joined.expect("Pressure worker did not finish").unwrap();
    match outcome.unwrap() {
        Ok(outcome) => (outcome, pressure),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

pub(super) async fn exercise_pressure_scheduling() {
    let temporary = tempfile::tempdir().unwrap();
    let service = Service::open(temporary.path().join("data"), None)
        .await
        .unwrap();
    let context = service.owner();
    // Initial emptiness must neither observe storage nor overflow a useless reservation.
    service
        .project_snapshot_pressure_observing(usize::MAX, || {
            panic!("Empty pressure observation must not touch storage")
        })
        .unwrap();
    assert_eq!(service.snapshots.lock().unwrap().project_write_estimate, 0);

    let asset = super::super::super::tests::asset("pressure-asset", &context.principal);
    service.store.put("asset/pressure-asset", &asset).unwrap();
    let scope = service
        .store
        .issue_scope_with_clock(&context.principal, || service.clock.now())
        .unwrap();
    let mut create = Request::query("projects.create", json!({"name":"Pressure scheduling"}));
    create.replay_scope = Some(scope);
    create.idempotency_key = Some("pressure-create".into());
    let created = service.dispatch(context.clone(), create).await;
    assert!(created.ok, "{created:?}");
    assert!(created.receipt_id.is_some());
    let project = created.result.unwrap()["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ticket = capture(&service, &context, &project).unwrap();

    // Each reservation is exactly 16 MiB. Both must survive interleaved observations,
    // and equality with the 32 MiB limit must retain the genuine captured read.
    let reservation = 2 * 1024 * 1024 - 8192;
    let ((inventory, concurrent), pressure) = while_observing(
        &service,
        reservation,
        |service| service.store.project_generation_bytes(),
        || {
            // This is dispatch's actual request validation, admission and policy path,
            // under the existing authenticated owner context, with no mock inventory.
            let inventory = service.dispatch_sync(
                context.clone(),
                Request::query("assets.list", json!({"limit":128})),
                CancellationToken::new(),
            );
            let concurrent = if inventory.as_ref().is_ok_and(|response| response.ok) {
                service.project_snapshot_pressure(reservation)
            } else {
                Ok(())
            };
            (inventory, concurrent)
        },
    )
    .await;
    pressure.unwrap();
    concurrent.unwrap();
    let inventory = inventory.unwrap();
    assert!(inventory.ok, "{inventory:?}");
    assert!(inventory.receipt_id.is_none());
    let inventory = inventory.result.unwrap();
    assert_eq!(inventory["projection"], "assets.summary.v1");
    assert_eq!(inventory["total_count"], 1);
    assert_eq!(inventory["count_accuracy"], "exact");
    assert_eq!(
        inventory["items"],
        json!([{"id":"pressure-asset","display_name":"pressure-asset.mkv","bytes":"100","stream_count":0}])
    );
    assert!(inventory["next_cursor"].is_null());
    let observed_bytes = service.store.project_generation_bytes().unwrap();
    {
        let cache = service.snapshots.lock().unwrap();
        assert!(cache.projects.contains_key(&ticket));
        assert_eq!(cache.project_write_estimate, 32 * 1024 * 1024);
        assert!(
            observed_bytes.saturating_sub(cache.projects[&ticket].generation_bytes)
                < 32 * 1024 * 1024
        );
    }
    service.project_snapshot_pressure(1).unwrap();
    assert!(service.snapshots.lock().unwrap().projects.is_empty());
    assert_eq!(service.snapshots.lock().unwrap().project_write_estimate, 0);

    let expired_ticket = capture(&service, &context, &project).unwrap();
    let (expired_during_observation, pressure) = while_observing(
        &service,
        1,
        |service| service.store.project_generation_bytes(),
        || {
            let mut cache = service.snapshots.try_lock().map_err(|_| lock_error())?;
            cache
                .projects
                .get_mut(&expired_ticket)
                .ok_or_else(expired)?
                .created = Instant::now() - LEASE;
            Ok::<_, DomainError>(())
        },
    )
    .await;
    expired_during_observation.unwrap();
    pressure.unwrap();
    assert!(service.snapshots.lock().unwrap().projects.is_empty());
    assert_eq!(service.snapshots.lock().unwrap().project_write_estimate, 0);

    let retired = capture(&service, &context, &project).unwrap();
    let retired_baseline = service.snapshots.lock().unwrap().projects[&retired].generation_bytes;
    // Change the real generation size before the pause; replacing the read during the
    // observation must use its new baseline, rather than the retired capture's minimum.
    service
        .store
        .put(
            "unit/pressure-current-baseline",
            &"g".repeat(8 * 1024 * 1024),
        )
        .unwrap();
    let current_baseline = service.store.project_generation_bytes().unwrap();
    assert!(current_baseline > retired_baseline);
    let (changed, pressure) = while_observing(
        &service,
        reservation,
        // At this controlled equality boundary, the retired minimum would wrongly evict.
        |service| {
            service
                .store
                .project_generation_bytes()
                .map(|bytes| bytes + 32 * 1024 * 1024)
        },
        || {
            service
                .snapshots
                .try_lock()
                .map_err(|_| lock_error())?
                .clear_project_reads();
            let replacement = capture(&service, &context, &project)?;
            service.project_snapshot_pressure(1024)?;
            Ok::<_, DomainError>(replacement)
        },
    )
    .await;
    pressure.unwrap();
    let current = changed.unwrap();
    assert_ne!(current, retired);
    {
        let cache = service.snapshots.lock().unwrap();
        assert!(!cache.projects.contains_key(&retired));
        assert!(cache.projects.contains_key(&current));
        assert_eq!(cache.projects.len(), 1);
        assert_eq!(cache.projects[&current].generation_bytes, current_baseline);
        assert_eq!(cache.project_write_estimate, 1024 * 8 + 64 * 1024);
    }

    // An observation error must remain visible without dropping the retained read or
    // erasing reservations. The same error is unnecessary when the cache starts empty.
    let error = service
        .project_snapshot_pressure_observing(1, || {
            Err(DomainError::new(
                "PRESSURE_OBSERVATION_FAILED",
                "Controlled observation failure",
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "PRESSURE_OBSERVATION_FAILED");
    {
        let cache = service.snapshots.lock().unwrap();
        assert!(cache.projects.contains_key(&current));
        assert_eq!(
            cache.project_write_estimate,
            1024 * 8 + 64 * 1024 + 8 + 64 * 1024
        );
    }
    let baseline = service.snapshots.lock().unwrap().projects[&current].generation_bytes;
    service
        .project_snapshot_pressure_observing(0, || Ok(baseline + 32 * 1024 * 1024))
        .unwrap();
    assert!(
        service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&current)
    );
    service
        .project_snapshot_pressure_observing(0, || Ok(baseline + 32 * 1024 * 1024 + 1))
        .unwrap();
    assert!(service.snapshots.lock().unwrap().projects.is_empty());

    capture(&service, &context, &project).unwrap();
    let (cleared, pressure) = while_observing(
        &service,
        1,
        |_| {
            Err(DomainError::new(
                "PRESSURE_OBSERVATION_FAILED",
                "Controlled observation failure after retirement",
            ))
        },
        || {
            service
                .snapshots
                .try_lock()
                .map_err(|_| lock_error())?
                .clear_project_reads();
            Ok::<_, DomainError>(())
        },
    )
    .await;
    cleared.unwrap();
    assert_eq!(pressure.unwrap_err().code, "PRESSURE_OBSERVATION_FAILED");
    assert!(service.snapshots.lock().unwrap().projects.is_empty());
    assert_eq!(service.snapshots.lock().unwrap().project_write_estimate, 0);
    capture(&service, &context, &project).unwrap();
    service.project_snapshot_pressure(usize::MAX).unwrap();
    assert!(service.snapshots.lock().unwrap().projects.is_empty());
    service.store.close_generation(&service.root_lease).unwrap();
    service
        .store
        .reopen_current_generation(&service.root_lease)
        .unwrap();
}
