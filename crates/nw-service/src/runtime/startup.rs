//! Explicit application startup may bootstrap a genuinely absent MediaV1 root.
//! Direct Service::open retains the legacy factory behavior used by existing callers.
use super::*;

/// These Service wrapper call sites emit at most40 events per constructor.
/// Store lifecycle stages emit additional, separately bounded diagnostics.
/// Ending a stage records elapsed time only; it does not assert success or readiness.
pub(super) struct StageTiming {
    stage: &'static str,
    started: Option<std::time::Instant>,
}
impl StageTiming {
    pub(super) fn start(enabled: bool, stage: &'static str) -> Self {
        let started = enabled.then(std::time::Instant::now);
        if enabled {
            tracing::info!(stage, "Application startup stage started");
        }
        Self { stage, started }
    }
}
impl Drop for StageTiming {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            tracing::info!(
                stage = self.stage,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Application startup stage ended"
            );
        }
    }
}

impl Service {
    pub async fn open_application(
        root: PathBuf,
        components: Option<(PathBuf, PathBuf)>,
    ) -> Result<Arc<Self>> {
        Self::open_internal(root, components, true).await
    }
    pub(super) fn genesis_profiles(root: &Path) -> Result<Vec<AccessProfile>> {
        let assets = nw_platform::security::private_directory(&root.join("assets"))?;
        let exports = nw_platform::security::private_directory(&root.join("exports"))?;
        Ok(vec![
            AccessProfile {
                schema_version: 2,
                id: "agent".into(),
                name: "agent".into(),
                principal: Uuid::new_v4().to_string(),
                revision: 1,
                enabled: true,
                client_sessions: false,
                mode: ProfileMode::Discovery,
                owned_object_access: false,
                allowed_command_ids: BTreeSet::from([
                    "commands.list".into(),
                    "commands.schema".into(),
                    "access.replay_scope.create".into(),
                ]),
                object_grants: Vec::new(),
                read_roots: Vec::new(),
                write_roots: Vec::new(),
            },
            AccessProfile {
                schema_version: 2,
                id: "browser".into(),
                name: "browser".into(),
                principal: Uuid::new_v4().to_string(),
                revision: 1,
                enabled: true,
                client_sessions: true,
                mode: ProfileMode::Operate,
                owned_object_access: true,
                allowed_command_ids: nw_domain::access::ordinary_commands(),
                object_grants: Vec::new(),
                read_roots: vec![files::path_text(&assets)?],
                write_roots: vec![files::path_text(&exports)?],
            },
        ])
    }
    pub(super) async fn initialize_application_media(self: &Arc<Self>) -> Result<()> {
        let eligible = {
            let _stage = StageTiming::start(true, "Check compatible genesis eligibility");
            self.store.fresh_initialization_origin()?
        };
        if !eligible {
            return Ok(());
        }
        let owner = self.owner();
        let scope_stage = StageTiming::start(true, "Issue canonical genesis replay scope");
        let scope = self
            .dispatch(
                owner.clone(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await;
        drop(scope_stage);
        if !scope.ok {
            return Err(scope.error.unwrap_or_else(|| {
                DomainError::new(
                    "INITIALIZATION_UNQUALIFIED",
                    "Genesis scope issuance failed",
                )
            }));
        }
        let token = scope
            .result
            .as_ref()
            .and_then(|result| result["replay_scope"].as_str())
            .ok_or_else(|| {
                DomainError::new("INITIALIZATION_UNQUALIFIED", "Genesis scope was absent")
            })?
            .to_owned();
        let review_stage = StageTiming::start(true, "Review canonical media initialization");
        let review = self
            .dispatch(
                owner.clone(),
                Request::query("maintenance.initialization.plan", json!({})),
            )
            .await;
        drop(review_stage);
        if !review.ok {
            return Err(review.error.unwrap_or_else(|| {
                DomainError::new(
                    "INITIALIZATION_UNQUALIFIED",
                    "Genesis initialization review failed",
                )
            }));
        }
        let mut accept = Request::query(
            "maintenance.initialization.accept",
            json!({"plan":review.result}),
        );
        accept.request_id = format!("genesis-{}", self.store.root_id());
        accept.replay_scope = Some(token);
        accept.idempotency_key = Some(format!("genesis-initialization-{}", self.store.root_id()));
        let accept_stage = StageTiming::start(true, "Accept canonical media initialization");
        let accepted = self.dispatch(owner, accept).await;
        drop(accept_stage);
        if !accepted.ok {
            return Err(accepted.error.unwrap_or_else(|| {
                DomainError::new(
                    "INITIALIZATION_UNQUALIFIED",
                    "Genesis initialization acceptance failed",
                )
            }));
        }
        let pending = {
            let _stage = StageTiming::start(true, "Confirm initialization is no longer pending");
            self.store.pending_media_maintenance()?
        };
        if pending {
            return Err(DomainError::new(
                "STORE_RECOVERY_REQUIRED",
                "Accepted initialization remains pending before ordinary application startup",
            ));
        }
        Ok(())
    }
}
