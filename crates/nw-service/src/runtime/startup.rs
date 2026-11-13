//! Explicit application startup may bootstrap a genuinely absent MediaV1 root.
//! Direct Service::open retains the legacy factory behavior used by existing callers.
use super::*;

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
        if !self.store.fresh_initialization_origin()? {
            return Ok(());
        }
        let owner = self.owner();
        let scope = self
            .dispatch(
                owner.clone(),
                Request::query("access.replay_scope.create", json!({})),
            )
            .await;
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
        let review = self
            .dispatch(
                owner.clone(),
                Request::query("maintenance.initialization.plan", json!({})),
            )
            .await;
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
        let accepted = self.dispatch(owner, accept).await;
        if !accepted.ok {
            return Err(accepted.error.unwrap_or_else(|| {
                DomainError::new(
                    "INITIALIZATION_UNQUALIFIED",
                    "Genesis initialization acceptance failed",
                )
            }));
        }
        if self.store.pending_media_maintenance()? {
            return Err(DomainError::new(
                "STORE_RECOVERY_REQUIRED",
                "Accepted initialization remains pending before ordinary application startup",
            ));
        }
        Ok(())
    }
}
