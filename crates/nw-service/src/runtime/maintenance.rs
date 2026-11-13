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
        if [
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
        let expected: Vec<u16> = (1..=18).collect();
        let mut batch = crate::schema_validation::Batch::for_schemas(17, &expected, vec![schema]);
        batch.whole(1, 0, encoded, true);
        for id in 2..=18 {
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
    }
}
