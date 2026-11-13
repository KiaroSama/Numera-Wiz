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
        if self.store.pending_migration()?.is_some() || self.store.pending_media_maintenance()? {
            return Err(busy());
        }
        Ok(Admission::Ordinary { _lease: lease })
    }
    pub(super) fn command_admission(&self, id: &str) -> Result<Admission<'_>> {
        if (id.starts_with("maintenance.promotion.")
            || id.starts_with("maintenance.initialization."))
            && !id.ends_with(".inspect")
            || [
                "maintenance.plan",
                "maintenance.migrate",
                "maintenance.promotion.plan",
            ]
            .contains(&id)
        {
            self.maintenance
                .try_write()
                .map(|lease| Admission::Maintenance { _lease: lease })
                .map_err(|_| busy())
        } else if (id.starts_with("maintenance.") && id.ends_with(".inspect"))
            || ["commands.list", "commands.schema"].contains(&id)
        {
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
        if request.command.id.ends_with(".inspect") {
            let input: IdInput = commands::parse(request.params.clone())?;
            let result = match request.command.id.as_str() {
                "maintenance.promotion.inspect" => {
                    value(&self.store.inspect_media_promotion(&input.id)?)?
                }
                "maintenance.initialization.inspect" => {
                    value(&self.store.inspect_media_initialization(&input.id)?)?
                }
                "maintenance.promotion.restore.inspect"
                | "maintenance.initialization.restore.inspect" => {
                    let mut record = value(&self.store.inspect_media_restoration(&input.id)?)?;
                    record["status"] = json!(self.store.media_restoration_status(&input.id)?);
                    record
                }
                _ => {
                    return Err(DomainError::new(
                        "UNKNOWN_COMMAND",
                        "No maintenance inspection handler",
                    ));
                }
            };
            return Ok(Response::success(&request.request_id, result));
        }
        // The admission write guard excludes new work. Work already accepted by
        // the sole scheduler must finish, or be explicitly cancelled, before review.
        if self.pending.load(Ordering::Acquire) != 0 || self.transfers.available_permits() != 2 {
            return Err(busy());
        }
        let build = self.maintenance_build(cancel)?;
        self.snapshots
            .lock()
            .map_err(|_| lock_error())?
            .clear_project_reads();
        match request.command.id.as_str() {
            "maintenance.plan" => {
                let _: Empty = commands::parse(request.params.clone())?;
                let plan =
                    self.store
                        .prepare_migration(&self.root_lease, &build, self.clock.now()?)?;
                Ok(Response::success(&request.request_id, plan))
            }
            "maintenance.promotion.plan" => {
                let _: Empty = commands::parse(request.params.clone())?;
                self.snapshots
                    .lock()
                    .map_err(|_| lock_error())?
                    .clear_project_reads();
                let plan = self
                    .store
                    .prepare_media_promotion(&self.root_lease, &build, || self.clock.now())?;
                if cancel.is_cancelled() {
                    return Err(DomainError::new(
                        "CANCELLED",
                        "Promotion review cancelled before publication",
                    ));
                }
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
            "maintenance.promotion.accept" => self.store.accept_media_promotion(
                &self.root_lease,
                &context.principal,
                request,
                &build,
                || self.clock.now(),
                cancel,
            ),
            "maintenance.initialization.plan" => {
                let _: Empty = commands::parse(request.params.clone())?;
                Ok(Response::success(
                    &request.request_id,
                    self.store
                        .prepare_media_initialization(&self.root_lease, &build, || {
                            self.clock.now()
                        })?,
                ))
            }
            "maintenance.initialization.accept" => self.store.accept_media_initialization(
                &self.root_lease,
                &context.principal,
                request,
                &build,
                || self.clock.now(),
                cancel,
            ),
            "maintenance.promotion.restore.plan" | "maintenance.initialization.restore.plan" => {
                let input: IdInput = commands::parse(request.params.clone())?;
                Ok(Response::success(
                    &request.request_id,
                    self.store.prepare_media_restoration(
                        &self.root_lease,
                        &input.id,
                        request
                            .command
                            .id
                            .starts_with("maintenance.initialization."),
                        &build,
                        || self.clock.now(),
                    )?,
                ))
            }
            "maintenance.promotion.restore.accept"
            | "maintenance.initialization.restore.accept" => self.store.accept_media_restoration(
                &self.root_lease,
                &context.principal,
                request,
                request
                    .command
                    .id
                    .starts_with("maintenance.initialization."),
                &build,
                || self.clock.now(),
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
        let activation = restarted
            .root_lease
            .selection()
            .unwrap()
            .activation_id
            .unwrap();
        assert_eq!(
            restarted
                .store
                .adoption_revision(&migration_id, &activation)
                .unwrap(),
            "0"
        );
        assert_eq!(
            nw_platform::files::RootLock::acquire(&restarted.root.join("state/generations/1"))
                .err()
                .unwrap()
                .code,
            "OWNER_BUSY"
        );

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
        assert_eq!(
            restarted
                .store
                .adoption_revision(&migration_id, &activation)
                .unwrap(),
            "0",
            "Read-only reopen and exact receipt replay do not adopt"
        );
        let new_scope = restarted
            .dispatch(
                restarted.owner(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await
            .result
            .unwrap()["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut fresh = Request::query(
            "access.profiles.create",
            json!({"name":"new-after-migration","read_roots":[],"write_roots":[]}),
        );
        fresh.replay_scope = Some(new_scope);
        fresh.idempotency_key = Some("new-accepted-business-state".into());
        let accepted = restarted.dispatch(restarted.owner(), fresh).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        assert_eq!(
            restarted
                .store
                .adoption_revision(&migration_id, &activation)
                .unwrap(),
            "1"
        );
        public_media_promotion_review(&restarted, &migration_id, &activation).await;
    }

    async fn public_media_promotion_review(
        service: &Arc<Service>,
        migration_id: &str,
        activation: &str,
    ) {
        // The original public migration/replay/business journey supplies real P and r=1.
        let bootstrap = service
            .dispatch(
                service.owner(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await;
        assert!(bootstrap.ok);
        let scope = bootstrap.result.unwrap()["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let request = Request::query("maintenance.promotion.plan", json!({}));
        let request_id = request.request_id.clone();
        let response = service.dispatch(service.owner(), request).await;
        assert!(
            response.ok,
            "MEDIA_PROMOTION_PUBLIC_REVIEW_MISSING code={:?}",
            response.error.as_ref().map(|error| error.code.as_str())
        );
        assert_eq!(response.request_id, request_id);
        assert!(
            response
                .result
                .as_ref()
                .is_some_and(serde_json::Value::is_object)
        );
        assert!(response.receipt_id.is_none());
        assert_eq!(
            service
                .store
                .adoption_revision(migration_id, activation)
                .unwrap(),
            "1",
            "Review must not adopt media or reset the original source guard"
        );
        let raw = response.result.unwrap();
        let encoded = serde_json::to_string(&raw).unwrap();
        assert!(
            encoded.len() < 32768
                && !encoded.contains("signing_key")
                && !encoded.contains("replay_scope")
                && !encoded.contains("credential")
        );
        assert!(!encoded.contains(service.root.to_string_lossy().as_ref()));
        assert_eq!(raw["kind"], "generation_promotion_v1");
        assert_eq!(raw["target_profile"], "media_v1");
        assert_eq!(
            raw["rollback_condition"],
            "stopped_root_restoration_before_adoption"
        );
        assert_eq!(raw["source_generation"], 3);
        assert_eq!(raw["rollback_generation"], 4);
        assert_eq!(raw["target_generation"], 5);
        assert_eq!(raw["source_activation_sequence"], 1);
        assert_eq!(raw["root_id"], service.store.root_id());
        assert_eq!(raw["source_activation_id"], activation);
        assert_eq!(
            raw["source_guard"],
            json!({"migration_id":migration_id,"activation_id":activation,"revision":"1"})
        );
        assert_eq!(
            raw["required_capabilities"],
            json!([
                "access.v2",
                "cleanup.v2",
                "cleanup_batch.v1",
                "receipts.v1",
                "generation_activation.v1",
                "migration.v1",
                "adoption_guard.v1",
                "generation_promotion.v1",
                "media_execution.v1"
            ])
        );
        let plan: nw_domain::maintenance::promotion::GenerationPromotionPlan =
            commands::parse(raw.clone()).unwrap();
        plan.validate_hash().unwrap();
        let source_bytes = plan.source_bytes.parse::<u64>().unwrap();
        let additional_bytes = plan.estimated_additional_bytes.parse::<u64>().unwrap();
        assert_eq!(additional_bytes, source_bytes * 3 + 67_108_864);
        let mut wrong_estimate = plan.clone();
        wrong_estimate.estimated_additional_bytes = (additional_bytes + 1).to_string();
        assert!(wrong_estimate.digest().is_err());
        service.store.close_generation(&service.root_lease).unwrap();
        let facts = service.root_lease.inspect_selected().unwrap();
        service
            .store
            .reopen_current_generation(&service.root_lease)
            .unwrap();
        assert_eq!(plan.source_records_sha256, facts.records_sha256);
        assert_eq!(plan.source_records, facts.record_count);
        assert_eq!(plan.source_capabilities, facts.required_capabilities);
        assert_eq!(
            raw["source_capabilities"],
            json!([
                "access.v2",
                "cleanup.v2",
                "cleanup_batch.v1",
                "receipts.v1",
                "generation_activation.v1",
                "migration.v1",
                "adoption_guard.v1"
            ])
        );
        let mut noise = plan.clone();
        noise.id = uuid::Uuid::new_v4().to_string();
        noise.created_at = (plan.created_at.parse::<u64>().unwrap() + 1).to_string();
        noise.expires_at = (plan.expires_at.parse::<u64>().unwrap() + 1).to_string();
        noise.required_capabilities.reverse();
        noise.source_capabilities.reverse();
        assert_eq!(noise.digest().unwrap(), plan.hash);
        let mut tampered = plan.clone();
        tampered.source_guard.revision = "2".into();
        assert!(tampered.validate_hash().is_err());
        tampered.source_guard.revision = "0".into();
        assert!(tampered.digest().is_err());
        let mut tampered = plan.clone();
        tampered.required_capabilities.pop();
        assert!(tampered.digest().is_err());
        let mut overflow = plan.clone();
        overflow.source_generation = u32::MAX;
        overflow.rollback_generation = 0;
        overflow.target_generation = 1;
        assert!(overflow.digest().is_err());
        let descriptor = commands::descriptor("maintenance.promotion.plan").unwrap();
        assert!(!nw_domain::access::ordinary_commands().contains("maintenance.promotion.plan"));
        let schema = descriptor.result_schema.unwrap();
        let expected: Vec<u16> = (1..=20).collect();
        let mut batch = crate::schema_validation::Batch::for_schemas(17, &expected, vec![schema]);
        batch.whole(1, 0, encoded, true);
        for id in 2..=20 {
            let mut copy = raw.clone();
            match id {
                2 => copy["schema_version"] = json!(2),
                3 => copy["extra"] = json!(true),
                4 => copy["hash"] = json!("0".repeat(63)),
                5 => copy["root_id"] = json!("../../not-a-root"),
                6 => copy["source_guard"]["revision"] = json!("0"),
                7 => copy["source_guard"] = serde_json::Value::Null,
                8 => copy["source_store_version"] = json!(2),
                9 => copy["target_profile"] = json!("cleanup"),
                10 => {
                    copy["required_capabilities"].as_array_mut().unwrap().pop();
                }
                11 => copy["required_capabilities"][8] = json!("access.v2"),
                12 => copy["required_capabilities"][8] = json!("future.v99"),
                13 => copy["source_generation"] = json!(0),
                14 => copy["source_records"] = json!(49001),
                15 => copy["source_prefix_sha256"] = json!("g".repeat(64)),
                16 => {
                    copy.as_object_mut().unwrap().remove("source_guard");
                }
                17 => copy["rollback_condition"] = json!("automatic"),
                18 => copy["source_capabilities"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("media_execution.v1")),
                19 => copy["estimated_additional_bytes"] = json!("01"),
                20 => {
                    copy.as_object_mut()
                        .unwrap()
                        .remove("estimated_additional_bytes");
                }
                _ => unreachable!(),
            }
            batch.whole(id, 0, serde_json::to_string(&copy).unwrap(), false);
        }
        batch.finish().await;
        let browser = service.profile_context("browser").unwrap();
        let denied = service
            .dispatch(
                browser,
                Request::query("maintenance.promotion.plan", json!({})),
            )
            .await;
        assert_eq!(denied.error.unwrap().code, "PERMISSION_DENIED");
        assert!(denied.receipt_id.is_none());
        assert_eq!(
            service
                .store
                .adoption_revision(migration_id, activation)
                .unwrap(),
            "1"
        );
        assert!(service.store.pending_migration().unwrap().is_none());
        promotion_rejects_legacy_and_unadopted_sources().await;
        let mut accept = Request::query("maintenance.promotion.accept", json!({"plan":raw}));
        accept.replay_scope = Some(scope);
        accept.idempotency_key = Some("actual-media-promotion".into());
        let accepted = service.dispatch(service.owner(), accept).await;
        assert!(
            accepted.ok,
            "MEDIA_PROMOTION_PUBLIC_ACCEPT_MISSING code={:?}",
            accepted.error.as_ref().map(|error| error.code.as_str())
        );
        assert!(accepted.receipt_id.is_some());
        assert_eq!(accepted.result.unwrap()["promotion_id"], plan.id);
        let selected = service.root_lease.selection().unwrap();
        assert_eq!(selected.generation, 5);
        let activation = selected.activation_id.unwrap();
        assert_eq!(
            service
                .store
                .adoption_revision(&plan.id, &activation)
                .unwrap(),
            "0",
            "A promoted target starts its own guard without resetting original source adoption"
        );
        exercise_public_restoration(service, &plan.id, false).await;
        assert_eq!(
            service
                .store
                .adoption_revision(migration_id, &plan.source_activation_id)
                .unwrap(),
            "1"
        );
        let inspected = service
            .dispatch(
                service.owner(),
                Request::query("maintenance.promotion.inspect", json!({"id":plan.id})),
            )
            .await;
        assert!(inspected.ok, "{:?}", inspected.error);
        assert_eq!(inspected.result.unwrap()["phase"], "restored");
    }

    async fn promotion_rejects_legacy_and_unadopted_sources() {
        let temporary = tempfile::tempdir().unwrap();
        let service = Service::open(temporary.path().to_owned(), None)
            .await
            .unwrap();
        let owner = service.owner();
        let refused = service
            .dispatch(
                owner.clone(),
                Request::query("maintenance.promotion.plan", json!({})),
            )
            .await;
        assert_eq!(refused.error.unwrap().code, "PROMOTION_UNQUALIFIED");
        assert_eq!(service.store.store_version().unwrap(), 2);
        assert!(refused.receipt_id.is_none());
        let issued = service
            .dispatch(
                owner.clone(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await;
        assert!(issued.ok);
        let scope = issued.result.unwrap()["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let reviewed = service
            .dispatch(owner.clone(), Request::query("maintenance.plan", json!({})))
            .await;
        assert!(reviewed.ok);
        let mut migrate = Request::query(
            "maintenance.migrate",
            json!({"plan":reviewed.result.unwrap()}),
        );
        migrate.replay_scope = Some(scope);
        migrate.idempotency_key = Some("promotion-zero-source".into());
        let accepted = service.dispatch(owner.clone(), migrate).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        let migration = accepted.result.unwrap()["migration_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let activation = service
            .root_lease
            .selection()
            .unwrap()
            .activation_id
            .unwrap();
        assert_eq!(
            service
                .store
                .adoption_revision(&migration, &activation)
                .unwrap(),
            "0"
        );
        let refused = service
            .dispatch(
                owner,
                Request::query("maintenance.promotion.plan", json!({})),
            )
            .await;
        assert_eq!(refused.error.unwrap().code, "PROMOTION_UNQUALIFIED");
        assert!(refused.receipt_id.is_none());
        assert_eq!(
            service
                .store
                .adoption_revision(&migration, &activation)
                .unwrap(),
            "0"
        );
        assert_eq!(service.store.store_version().unwrap(), 3);
        assert!(service.store.pending_migration().unwrap().is_none());
        let scope = issue_owner_scope(&service).await;
        let review = service
            .dispatch(
                service.owner(),
                Request::query("maintenance.initialization.plan", json!({})),
            )
            .await;
        assert!(review.ok, "{:?}", review.error);
        let reviewed = review.result.unwrap();
        assert_eq!(reviewed["origin"], "migrated_zero_revision");
        assert_eq!(reviewed["source_guard_revision"], "0");
        let id = reviewed["id"].as_str().unwrap().to_owned();
        let mut request = Request::query(
            "maintenance.initialization.accept",
            json!({"plan":reviewed}),
        );
        request.replay_scope = Some(scope);
        request.idempotency_key = Some("actual-r0-initialization".into());
        let accepted = service.dispatch(service.owner(), request.clone()).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        assert_eq!(accepted.result.as_ref().unwrap()["initialization_id"], id);
        service.store.require_media_floor().unwrap();
        exercise_public_restoration(&service, &id, true).await;
        assert_eq!(
            service
                .store
                .adoption_revision(&migration, &activation)
                .unwrap(),
            "0"
        );
        let replay = service.dispatch(service.owner(), request).await;
        assert!(replay.ok, "{:?}", replay.error);
        assert_eq!(replay.receipt_id, accepted.receipt_id);
        assert_eq!(service.root_lease.selection().unwrap().generation, 3);
        assert!(service.store.require_media_floor().is_err());
        drop(service);
        let restarted = Service::open_application(temporary.path().to_owned(), None)
            .await
            .unwrap();
        assert_eq!(restarted.root_lease.selection().unwrap().generation, 3);
        assert_eq!(
            restarted
                .store
                .adoption_revision(&migration, &activation)
                .unwrap(),
            "0"
        );
        drop(restarted);
        exercise_true_fresh_and_disconnected_resume().await;
        super::super::scheduler::exercise_uncertain_cancellation().await;
        exercise_nonfresh_sibling_refusal().await;
    }
    async fn exercise_nonfresh_sibling_refusal() {
        for sibling in ["2", "history", "unknown"] {
            let temporary = tempfile::tempdir().unwrap();
            let root =
                nw_platform::security::private_directory(&temporary.path().join("root")).unwrap();
            let retained = nw_platform::security::private_directory(
                &root.join("state/generations").join(sibling),
            )
            .unwrap();
            let history = retained.join("retained.bin");
            std::fs::write(&history, b"unknown retained generation history").unwrap();
            assert!(Service::open_application(root.clone(), None).await.is_err());
            assert!(!root.join("state/generations/1/store.redb").exists());
            assert_eq!(
                std::fs::read(history).unwrap(),
                b"unknown retained generation history"
            );
        }
    }
    async fn issue_owner_scope(service: &Arc<Service>) -> String {
        let issued = service
            .dispatch(
                service.owner(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await;
        assert!(issued.ok, "{:?}", issued.error);
        issued.result.unwrap()["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    async fn exercise_public_restoration(
        service: &Arc<Service>,
        id: &str,
        initialization: bool,
    ) -> Request {
        let namespace = if initialization {
            "maintenance.initialization"
        } else {
            "maintenance.promotion"
        };
        let scope = issue_owner_scope(service).await;
        let reviewed = service
            .dispatch(
                service.owner(),
                Request::query(&format!("{namespace}.restore.plan"), json!({"id":id})),
            )
            .await;
        assert!(reviewed.ok, "{:?}", reviewed.error);
        let plan = reviewed.result.unwrap();
        let restoration = plan["id"].as_str().unwrap().to_owned();
        let mut request =
            Request::query(&format!("{namespace}.restore.accept"), json!({"plan":plan}));
        request.replay_scope = Some(scope);
        request.idempotency_key = Some("actual-stopped-restoration".into());
        let accepted = service.dispatch(service.owner(), request.clone()).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        assert!(accepted.receipt_id.is_some());
        assert_eq!(
            accepted.result.as_ref().unwrap()["restoration_id"],
            restoration
        );
        assert!(!service.store.pending_media_maintenance().unwrap());
        let inspected = service
            .dispatch(
                service.owner(),
                Request::query(
                    &format!("{namespace}.restore.inspect"),
                    json!({"id":restoration}),
                ),
            )
            .await;
        assert!(inspected.ok, "{:?}", inspected.error);
        assert_eq!(inspected.result.unwrap()["id"], restoration);
        let replay = service.dispatch(service.owner(), request.clone()).await;
        assert!(replay.ok, "{:?}", replay.error);
        assert_eq!(replay.receipt_id, accepted.receipt_id);
        assert!(service.store.require_media_floor().is_err());
        request
    }
    async fn exercise_true_fresh_and_disconnected_resume() {
        let temporary = tempfile::tempdir().unwrap();
        // The private startup gate models the real retained receiver before owner
        // recovery starts; every generation and receipt is canonically produced.
        let service = Service::open_runtime(temporary.path().to_owned(), None, true, true)
            .await
            .unwrap();
        service.store.require_media_floor().unwrap();
        let selected = service.root_lease.selection().unwrap();
        assert_eq!(selected.generation, 3);
        let nw_store::generations::GenerationLocation::Workspace {
            operation_id: initialization,
            ..
        } = selected.location
        else {
            panic!("Fresh media initialization must select its real workspace target");
        };
        let initialized = service
            .store
            .inspect_media_initialization(&initialization)
            .unwrap();
        assert_eq!(
            initialized.plan.origin,
            nw_domain::maintenance::media_lifecycle::InitializationOrigin::CompatibleGenesisV1
        );
        assert_eq!(initialized.plan.source_generation, 1);
        let principal = service.owner().principal;
        let restoration_replay = exercise_public_restoration(&service, &initialization, true).await;
        assert_eq!(service.root_lease.selection().unwrap().generation, 1);
        assert_eq!(
            service
                .store
                .adoption_revision(
                    &initialized.plan.source_operation_id,
                    &initialized.plan.source_activation_id
                )
                .unwrap(),
            "0"
        );
        assert!(!service.store.fresh_initialization_origin().unwrap());
        assert!(service.dispatcher_receiver.lock().unwrap().is_some());
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release = Mutex::new(release_rx);
        *service.preflight_hook.lock().unwrap() = Some(Arc::new(move || {
            if let Some(entered) = entered.lock().unwrap().take() {
                entered.send(()).unwrap();
            }
            release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }));
        service.dispatcher_ready.store(true, Ordering::Release);
        let retained_receipt = service
            .store
            .replay_with_clock(&service.owner().principal, &restoration_replay, || {
                service.clock.now()
            })
            .unwrap()
            .unwrap()
            .receipt_id;
        let caller = service.clone();
        let lost_request = restoration_replay.clone();
        let lost = tokio::spawn(async move { caller.dispatch(caller.owner(), lost_request).await });
        tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
            .await
            .unwrap()
            .unwrap();
        lost.abort();
        assert!(lost.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while service.dispatcher_receiver.lock().unwrap().is_some()
                || service.requests.available_permits() != 16
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        *service.preflight_hook.lock().unwrap() = None;
        assert!(
            !service.dispatcher.is_closed(),
            "Caller loss must not drop the retained receiver"
        );
        let replay = service.dispatch(service.owner(), restoration_replay).await;
        assert!(replay.ok, "{:?}", replay.error);
        assert_eq!(replay.receipt_id, retained_receipt);
        assert_eq!(service.owner().principal, principal);
        drop(service);
        let restarted = Service::open_application(temporary.path().to_owned(), None)
            .await
            .unwrap();
        assert_eq!(restarted.root_lease.selection().unwrap().generation, 1);
        assert!(!restarted.store.fresh_initialization_origin().unwrap());
        assert!(restarted.store.require_media_floor().is_err());
        let scope = issue_owner_scope(&restarted).await;
        let mut business = Request::query(
            "access.profiles.create",
            json!({
                "name":"after-restored-genesis", "read_roots":[], "write_roots":[],
            }),
        );
        business.replay_scope = Some(scope.clone());
        business.idempotency_key = Some("old-business-after-genesis-restoration".into());
        let business = restarted.dispatch(restarted.owner(), business).await;
        assert!(business.ok, "{:?}", business.error);
        assert_eq!(
            restarted
                .store
                .adoption_revision(
                    &initialized.plan.source_operation_id,
                    &initialized.plan.source_activation_id
                )
                .unwrap(),
            "1"
        );
        let review = restarted
            .dispatch(
                restarted.owner(),
                Request::query("maintenance.promotion.plan", json!({})),
            )
            .await;
        assert!(review.ok, "{:?}", review.error);
        let plan = review.result.unwrap();
        assert_eq!(plan["source_generation"], 1);
        assert_eq!(plan["source_guard"]["revision"], "1");
        let promotion = plan["id"].as_str().unwrap().to_owned();
        let mut accept = Request::query("maintenance.promotion.accept", json!({"plan":plan}));
        accept.replay_scope = Some(scope);
        accept.idempotency_key = Some("successor-after-restored-genesis".into());
        let accepted = restarted.dispatch(restarted.owner(), accept).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        assert!(
            restarted
                .store
                .inspect_media_promotion(&promotion)
                .unwrap()
                .predecessor_restoration
                .is_some()
        );
        drop(restarted);
        let successor = Service::open_application(temporary.path().to_owned(), None)
            .await
            .unwrap();
        successor.store.require_media_floor().unwrap();
        assert_eq!(
            successor.root_lease.selection().unwrap().generation,
            3,
            "A historical restoration must not capture a genuinely accepted successor"
        );
        let original_source = temporary.path().join("state/generations/1");
        let held_identity = files::source_object(&original_source.join("store.redb")).unwrap();
        assert_eq!(
            files::RootLock::acquire(&original_source)
                .err()
                .unwrap()
                .code,
            "OWNER_BUSY"
        );
        exercise_public_restoration(&successor, &promotion, false).await;
        assert_eq!(
            files::source_object(&original_source.join("store.redb")).unwrap(),
            held_identity
        );
        assert_eq!(
            files::RootLock::acquire(&original_source)
                .err()
                .unwrap()
                .code,
            "OWNER_BUSY"
        );
        assert_eq!(
            successor
                .store
                .adoption_revision(
                    &initialized.plan.source_operation_id,
                    &initialized.plan.source_activation_id
                )
                .unwrap(),
            "1"
        );
        assert!(
            successor
                .store
                .get::<AccessProfile>("profile/after-restored-genesis")
                .unwrap()
                .is_some()
        );
    }
}
