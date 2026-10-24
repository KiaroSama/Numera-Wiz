//! The owner is the sole command authority, durable acceptor and scheduler.
mod artifacts;
pub use artifacts::ArtifactTransfer;
mod clock;
mod read_model;
mod uploads;
use nw_domain::{
    DomainError, Result,
    access::{
        AccessProfile, CredentialInput, ExecutionAuthorization, ObjectAction, ObjectKind,
        ProfileInput, ProfileMode, Publication,
    },
    commands::{
        self, CleanInput, Empty, IdInput, PathInput, Request, Response, StateClass, SubmitInput,
    },
    records::{Asset, Job, JobState, Plan},
};
use nw_engine::{Engine, ExecutionEvent, Observer};
use nw_platform::files;
use nw_store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Semaphore, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub principal: String,
    pub profile: Option<String>,
}
#[cfg(test)]
type PreflightHook = Arc<dyn Fn() + Send + Sync>;
#[derive(Serialize, Deserialize)]
struct Credential {
    context: Context,
    epoch: String,
    expires: u64,
    #[serde(default)]
    last_active: u64,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    audience: u64,
    #[serde(default)]
    revoked: bool,
}
pub struct Service {
    pub store: Arc<Store>,
    pub epoch: String,
    engine: Option<Engine>,
    root: PathBuf,
    code_roots: Vec<PathBuf>,
    mutation: Mutex<()>,
    credentials: Mutex<()>,
    snapshots: Mutex<read_model::Snapshots>,
    clock: clock::SessionClock,
    origins: Mutex<HashMap<String, u64>>,
    requests: Arc<Semaphore>,
    heavy: Arc<Semaphore>,
    transfers: Arc<Semaphore>,
    pending: Arc<AtomicUsize>,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    events: broadcast::Sender<String>,
    #[cfg(test)]
    preflight_hook: Mutex<Option<PreflightHook>>,
}
fn lock_error() -> DomainError {
    DomainError::new("INTERNAL_ERROR", "Owner state lock was poisoned")
}
fn staging_path(path: &Path) -> bool {
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        [".numera-job-", ".numera-proof-", ".numera-owned-"]
            .iter()
            .any(|prefix| name.starts_with(*prefix))
    })
}
fn staging_denied() -> DomainError {
    DomainError::new(
        "PATH_DENIED",
        "Engine-owned staging and proof namespaces are not ordinary media paths",
    )
}
fn value(input: &impl Serialize) -> Result<Value> {
    serde_json::to_value(input)
        .map_err(|error| DomainError::new("SERIALIZATION_FAILED", error.to_string()))
}
fn named(prefix: &str, id: &str) -> Result<String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(DomainError::new(
            "VALIDATION_ERROR",
            "Invalid bounded object identifier",
        ));
    }
    Ok(format!("{prefix}/{id}"))
}
impl Service {
    pub async fn open(root: PathBuf, components: Option<(PathBuf, PathBuf)>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&root).map_err(files::io_error)?;
        let root = std::fs::canonicalize(root).map_err(files::io_error)?;
        let selected_root = root.clone();
        let store = tokio::task::spawn_blocking(move || {
            let protected =
                nw_platform::security::private_directory(&root.join("state/generations/1"))?;
            Store::open(&protected)
        })
        .await
        .map_err(|error| DomainError::new("WORKER_FAILED", error.to_string()))??;
        if store.get::<AccessProfile>("profile/agent")?.is_none() {
            store.put(
                "profile/agent",
                &AccessProfile {
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
                    read_roots: vec![],
                    write_roots: vec![],
                },
            )?;
        }
        if store.get::<AccessProfile>("profile/browser")?.is_none() {
            let assets = nw_platform::security::private_directory(&selected_root.join("assets"))?;
            let exports = nw_platform::security::private_directory(&selected_root.join("exports"))?;
            store.put(
                "profile/browser",
                &AccessProfile {
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
            )?;
        }
        let engine = match components {
            Some((ffmpeg, ffprobe)) => Some(Engine::new(ffmpeg, ffprobe).await?),
            None => None,
        };
        let executable = std::env::current_exe().map_err(files::io_error)?;
        let mut code_roots = vec![
            std::fs::canonicalize(executable.parent().ok_or_else(|| {
                DomainError::new("COMPONENT_UNAVAILABLE", "Application directory is required")
            })?)
            .map_err(files::io_error)?,
        ];
        if let Some(engine) = &engine {
            for component in [&engine.ffmpeg, &engine.ffprobe] {
                code_roots.push(
                    Path::new(&component.path)
                        .parent()
                        .ok_or_else(|| {
                            DomainError::new(
                                "COMPONENT_UNAVAILABLE",
                                "Component directory is required",
                            )
                        })?
                        .to_owned(),
                );
            }
        }
        code_roots.sort();
        code_roots.dedup();
        let (events, _) = broadcast::channel(128);
        let service = Arc::new(Self {
            store: Arc::new(store),
            root: selected_root,
            code_roots,
            epoch: Uuid::new_v4().to_string(),
            engine,
            mutation: Mutex::new(()),
            credentials: Mutex::new(()),
            snapshots: Mutex::new(read_model::Snapshots::default()),
            clock: clock::SessionClock::default(),
            origins: Mutex::new(HashMap::new()),
            requests: Arc::new(Semaphore::new(16)),
            heavy: Arc::new(Semaphore::new(1)),
            transfers: Arc::new(Semaphore::new(2)),
            pending: Arc::new(AtomicUsize::new(0)),
            cancellations: Mutex::new(HashMap::new()),
            events,
            #[cfg(test)]
            preflight_hook: Mutex::new(None),
        });
        let recovering = service.clone();
        tokio::task::spawn_blocking(move || {
            recovering.recover_uploads()?;
            recovering.recover()
        })
        .await
        .map_err(|error| DomainError::new("RECOVERY_FAILED", error.to_string()))??;
        for job in service.store.list::<Job>("job/", 10000)? {
            if job.state == JobState::Queued {
                service.schedule(job.id)?;
            }
        }
        Ok(service)
    }
    pub fn owner(&self) -> Context {
        Context {
            principal: format!("owner-{}", self.store.root_id()),
            profile: None,
        }
    }
    #[cfg(test)]
    fn preflight_started(&self) {
        let hook = self.preflight_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
    fn refresh_acceptance(
        &self,
        context: &Context,
        request: &Request,
        cancellation: &CancellationToken,
        _guard: &std::sync::MutexGuard<'_, ()>,
    ) -> Result<Option<Response>> {
        let policy = self.policy(context)?;
        if let Some(response) = self
            .store
            .replay_with_clock(&context.principal, request, || self.clock.now())?
        {
            return self.replay_view(context, request, response).map(Some);
        }
        if policy
            .as_ref()
            .is_some_and(|profile| !profile.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Current command authority was revoked during preflight",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Request cancelled before fresh acceptance",
            ));
        }
        Ok(None)
    }
    pub fn client_lease_allowed(&self, context: &Context) -> Result<()> {
        if let Some(profile) = self.policy(context)?
            && !profile.client_sessions
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "This profile does not admit editor-client leases",
            ));
        }
        Ok(())
    }
    pub fn busy(&self) -> bool {
        self.pending.load(Ordering::Acquire) != 0 || self.requests.available_permits() < 16
    }
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }
    pub fn profile_context(&self, name: &str) -> Result<Context> {
        let profile: AccessProfile =
            self.store.get(&named("profile", name)?)?.ok_or_else(|| {
                DomainError::new(
                    "PROFILE_NOT_FOUND",
                    "Explicit access profile does not exist",
                )
            })?;
        if !profile.enabled {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Access profile is revoked",
            ));
        }
        Ok(Context {
            principal: profile.principal,
            profile: Some(profile.id),
        })
    }
    fn policy(&self, context: &Context) -> Result<Option<AccessProfile>> {
        match &context.profile {
            None if context.principal == self.owner().principal => Ok(None),
            Some(name) => {
                let profile: AccessProfile =
                    self.store.get(&named("profile", name)?)?.ok_or_else(|| {
                        DomainError::new("PERMISSION_DENIED", "Access profile is missing")
                    })?;
                if !profile.enabled || profile.principal != context.principal {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Access profile is revoked or mismatched",
                    ));
                }
                Ok(Some(profile))
            }
            None => Err(DomainError::new(
                "PERMISSION_DENIED",
                "Owner authority cannot be selected by a caller",
            )),
        }
    }
    fn object(&self, context: &Context, id: &str, creator: &str) -> Result<()> {
        if let Some(profile) = self.policy(context)?
            && !profile.allows_asset(creator)
        {
            tracing::warn!(principal=%context.principal, object_id=id, creator, policy_revision=profile.revision, "Creator-scoped asset access denied");
            return Err(DomainError::object_not_found());
        }
        Ok(())
    }
    fn object_action(
        &self,
        context: &Context,
        kind: ObjectKind,
        id: &str,
        creator: &str,
        action: ObjectAction,
    ) -> Result<()> {
        if let Some(profile) = self.policy(context)?
            && !profile.allows_object(kind, id, creator, action)
        {
            tracing::warn!(principal=%context.principal, object_id=id, creator, kind=?kind, action=?action, policy_revision=profile.revision, "Object action denied under current policy");
            return Err(DomainError::object_not_found());
        }
        Ok(())
    }
    fn read_plan(&self, context: &Context, id: &str, action: ObjectAction) -> Result<Plan> {
        let raw: Value = self
            .store
            .get(&named("plan", id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        let creator = raw
            .get("creator")
            .and_then(Value::as_str)
            .ok_or_else(DomainError::object_not_found)?;
        self.object_action(context, ObjectKind::Plan, id, creator, action)?;
        let plan = nw_domain::plans::decode(raw)?;
        if plan.id != id {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Stored plan identity does not match its key",
            ));
        }
        Ok(plan)
    }
    fn plan_review(&self, context: &Context, plan: &Plan) -> Result<nw_domain::views::PlanReview> {
        let policy = self.policy(context)?;
        let source_visible = policy.as_ref().is_none_or(|profile| {
            profile
                .read_roots
                .iter()
                .any(|root| Path::new(&plan.source.identity.path).starts_with(root))
        });
        let output_visible = policy.as_ref().is_none_or(|profile| {
            profile
                .write_roots
                .iter()
                .chain(&profile.read_roots)
                .any(|root| Path::new(&plan.output).starts_with(root))
        });
        let actions = [ObjectAction::Read, ObjectAction::Submit]
            .into_iter()
            .filter(|action| {
                policy.as_ref().is_none_or(|profile| {
                    profile.allows_object(ObjectKind::Plan, &plan.id, &plan.creator, *action)
                })
            })
            .collect();
        Ok(nw_domain::views::PlanReview::new(
            plan,
            source_visible,
            output_visible,
            policy.is_none(),
            policy.as_ref().map_or(0, |profile| profile.revision),
            actions,
        ))
    }
    fn job_view(&self, context: &Context, job: &Job) -> Result<nw_domain::views::JobView> {
        use nw_domain::views::{ArtifactView, ErrorView, JobView};
        let policy = self.policy(context)?;
        let owner = policy.is_none();
        let actions = [ObjectAction::Read, ObjectAction::Control]
            .into_iter()
            .filter(|action| {
                policy.as_ref().is_none_or(|profile| {
                    profile.allows_object(ObjectKind::Job, &job.id, &job.creator, *action)
                })
            })
            .collect();
        Ok(JobView {
            id: job.id.clone(),
            state: job.state.clone(),
            plan: self.plan_review(context, &job.plan)?,
            artifact: job.artifact.as_ref().map(|artifact| ArtifactView {
                path: policy
                    .as_ref()
                    .is_none_or(|profile| {
                        profile
                            .read_roots
                            .iter()
                            .any(|root| Path::new(&artifact.path).starts_with(root))
                    })
                    .then(|| artifact.path.clone()),
                bytes: artifact.bytes.clone(),
                sha256: artifact.sha256.clone(),
            }),
            verification: job.verification.clone(),
            error: job.error.as_ref().map(|error| ErrorView {
                code: error.code.clone(),
                message: owner.then(|| error.message.clone()),
                retryable: error.retryable,
            }),
            prepared_stages: owner.then(|| job.prepared_stages.clone()),
            policy_revision: policy.as_ref().map_or(0, |profile| profile.revision),
            effective_actions: actions,
        })
    }
    fn replay_view(
        &self,
        context: &Context,
        request: &Request,
        response: Response,
    ) -> Result<Response> {
        let Some(profile) = self.policy(context)? else {
            return Ok(response);
        };
        let readable = match request.command.id.as_str() {
            "assets.register" => {
                let asset: Asset = commands::parse(response.result.clone().ok_or_else(|| {
                    DomainError::new("STORAGE_ERROR", "Retained asset receipt has no result")
                })?)?;
                profile.allows_asset(&asset.creator)
                    && profile
                        .read_roots
                        .iter()
                        .any(|root| Path::new(&asset.identity.path).starts_with(root))
            }
            _ => true,
        };
        if readable {
            return Ok(response);
        }
        let digest = hex::encode(Sha256::digest(
            serde_json::to_vec(&response.result)
                .map_err(|error| DomainError::new("SERIALIZATION_FAILED", error.to_string()))?,
        ));
        Ok(Response {
            request_id: request.request_id.clone(),
            ok: false,
            receipt_id: response.receipt_id.clone(),
            result: Some(
                json!({"receipt_id":response.receipt_id,"completion_state":"completed","result_digest":digest}),
            ),
            error: Some(DomainError::new(
                "RECEIPT_RESULT_RESTRICTED",
                "Original work completed; protected result fields are no longer readable under current grants",
            )),
        })
    }
    fn path(&self, context: &Context, path: &Path, write: bool) -> Result<PathBuf> {
        self.path_with_purpose(context, path, write, None)
    }
    fn plan_source_path(&self, context: &Context, plan: &Plan) -> Result<PathBuf> {
        let rights = nw_domain::plans::validate_requirements(plan)?;
        if rights.data_root != self.store.root_id() {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Reviewed requirements belong to another data root",
            ));
        }
        let managed = Path::new(&plan.source.identity.path).starts_with(self.root.join("assets"));
        match &rights.source_read.purpose {
            nw_domain::plans::SourcePurpose::NativeFile if managed => {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Managed input needs its creator-bound read purpose",
                ));
            }
            nw_domain::plans::SourcePurpose::CreatorBoundManaged { principal } => {
                if !managed || principal != &context.principal {
                    return Err(DomainError::new(
                        "INVALID_PLAN",
                        "This creator-bound input requires a newly reviewed plan",
                    ));
                }
                if context.profile.is_some() {
                    self.managed_read(context, Path::new(&plan.source.identity.path))?;
                }
            }
            _ => {}
        }
        self.root_authority(context, Path::new(&plan.source.identity.path), false)?;
        self.path_with_purpose(
            context,
            Path::new(&plan.source.identity.path),
            false,
            Some(&plan.source.identity),
        )
    }
    fn root_authority(&self, context: &Context, canonical: &Path, write: bool) -> Result<()> {
        if let Some(profile) = self.policy(context)? {
            let roots = if write {
                &profile.write_roots
            } else {
                &profile.read_roots
            };
            if !roots
                .iter()
                .any(|root| canonical.starts_with(Path::new(root)))
            {
                return Err(DomainError::new(
                    "PATH_DENIED",
                    "Current profile has no grant for this native path",
                ));
            }
        }
        Ok(())
    }
    fn path_with_purpose(
        &self,
        context: &Context,
        path: &Path,
        write: bool,
        purpose: Option<&nw_domain::records::FileIdentity>,
    ) -> Result<PathBuf> {
        if staging_path(path) {
            return Err(staging_denied());
        }
        let canonical = if write {
            files::output_path(path)?
        } else {
            files::source_path(path)?
        };
        if staging_path(&canonical) {
            return Err(staging_denied());
        }
        if ["state", "runtime", "logs", "ingest"]
            .iter()
            .any(|name| canonical.starts_with(self.root.join(name)))
            || (write && canonical.starts_with(self.root.join("assets")))
        {
            return Err(DomainError::new(
                "PATH_DENIED",
                "Protected owner namespaces and managed objects are not media output targets",
            ));
        }
        if write
            && !canonical.starts_with(self.root.join("exports"))
            && self
                .code_roots
                .iter()
                .any(|root| canonical.starts_with(root))
        {
            return Err(DomainError::new(
                "PATH_DENIED",
                "Application and component folders cannot receive media exports. Choose another destination.",
            ));
        }
        if purpose.is_some_and(|identity| write || canonical != Path::new(&identity.path)) {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Captured input purpose does not match its canonical dependency",
            ));
        }
        if !write
            && purpose.is_none()
            && context.profile.is_some()
            && canonical.starts_with(self.root.join("assets"))
        {
            self.managed_read(context, &canonical)?;
        }
        if let Some(profile) = self.policy(context)? {
            let roots = if write {
                profile.write_roots
            } else {
                profile.read_roots
            };
            if !roots
                .iter()
                .any(|root| canonical.starts_with(Path::new(root)))
            {
                return Err(DomainError::new(
                    "PATH_DENIED",
                    "Current profile has no grant for this native path",
                ));
            }
        }
        Ok(canonical)
    }
    fn engine(&self) -> Result<&Engine> {
        self.engine.as_ref().ok_or_else(|| {
            DomainError::new(
                "COMPONENT_UNAVAILABLE",
                "No admitted FFmpeg/ffprobe component profile is installed",
            )
        })
    }
    pub fn authenticate(&self, token: &str) -> Result<Context> {
        self.authenticate_at(token, None, true)
    }
    pub fn authenticate_at(
        &self,
        token: &str,
        origin: Option<&str>,
        active: bool,
    ) -> Result<Context> {
        let _guard = self.credentials.lock().map_err(|_| lock_error())?;
        let now = self.clock.now()?;
        if token.len() > 256 {
            return Err(DomainError::new(
                "AUTHENTICATION_REQUIRED",
                "Invalid credential",
            ));
        }
        let key = format!(
            "credential/{}",
            hex::encode(Sha256::digest(token.as_bytes()))
        );
        let mut credential: Credential = self.store.get(&key)?.ok_or_else(|| {
            DomainError::new(
                "AUTHENTICATION_REQUIRED",
                "Credential is missing or revoked",
            )
        })?;
        if credential.revoked
            || credential.epoch != self.epoch
            || now >= credential.expires
            || now.saturating_sub(credential.last_active) >= 7200
            || credential.origin.as_deref() != origin
            || credential.audience != self.origin_generation(origin)?
        {
            return Err(DomainError::new(
                "AUTHENTICATION_REQUIRED",
                "Credential expired or owner epoch changed",
            ));
        }
        self.policy(&credential.context)?;
        if active && now > credential.last_active {
            credential.last_active = now;
            self.store.put(&key, &credential)?;
        }
        Ok(credential.context)
    }
    fn origin_generation(&self, origin: Option<&str>) -> Result<u64> {
        let generations = self.origins.lock().map_err(|_| lock_error())?;
        Ok(origin
            .and_then(|origin| generations.get(origin))
            .copied()
            .unwrap_or(0))
    }
    pub fn advance_origin(&self, origin: &str) -> Result<()> {
        let mut generations = self.origins.lock().map_err(|_| lock_error())?;
        if generations.len() >= 128 && !generations.contains_key(origin) {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Public audience transition quota reached; restart after work drains",
            ));
        }
        let generation = generations.entry(origin.into()).or_default();
        *generation = generation
            .checked_add(1)
            .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Audience generation exhausted"))?;
        Ok(())
    }
    pub fn credential(&self, name: &str) -> Result<String> {
        self.credential_for_origin(name, None)
    }
    pub fn credential_for_origin(&self, name: &str, origin: Option<String>) -> Result<String> {
        let _guard = self.credentials.lock().map_err(|_| lock_error())?;
        let context = self.profile_context(name)?;
        let now = self.clock.now()?;
        let mut live = 0;
        for (key, credential) in self.store.rows::<Credential>("credential/", 10000)? {
            if credential.revoked
                || credential.epoch != self.epoch
                || credential.expires <= now
                || now.saturating_sub(credential.last_active) >= 7200
                || credential.audience != self.origin_generation(credential.origin.as_deref())?
            {
                self.store.remove(&key)?;
            } else if credential.context.principal == context.principal {
                live += 1;
            }
        }
        if live >= 32 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Profile credential quota reached",
            ));
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let key = format!(
            "credential/{}",
            hex::encode(Sha256::digest(token.as_bytes()))
        );
        self.store.put(
            &key,
            &Credential {
                context,
                epoch: self.epoch.clone(),
                expires: now.checked_add(8 * 3600).ok_or_else(|| {
                    DomainError::new("RESOURCE_LIMIT", "Credential clock range exhausted")
                })?,
                last_active: now,
                audience: self.origin_generation(origin.as_deref())?,
                origin,
                revoked: false,
            },
        )?;
        Ok(token)
    }
    pub async fn dispatch(self: &Arc<Self>, context: Context, request: Request) -> Response {
        self.dispatch_cancel(context, request, CancellationToken::new())
            .await
    }
    pub async fn dispatch_cancel(
        self: &Arc<Self>,
        context: Context,
        request: Request,
        cancellation: CancellationToken,
    ) -> Response {
        let id = request.request_id.clone();
        let operation = request.command.id.clone();
        tracing::debug!(request_id=%id, command=%operation, principal=%context.principal, "command.received");
        let permit = match self.requests.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return Response::error(
                    &id,
                    DomainError::new("RESOURCE_LIMIT", "Owner request capacity reached"),
                );
            }
        };
        let _cancel_on_disconnect = cancellation.clone().drop_guard();
        let service = self.clone();
        let response = match tokio::task::spawn_blocking(move || {
            let _work_permit = permit;
            service.dispatch_sync(context, request, cancellation)
        })
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => Response::error(&id, error),
            Err(error) => {
                Response::error(&id, DomainError::new("WORKER_FAILED", error.to_string()))
            }
        }
        .bounded();
        tracing::info!(request_id=%id, command=%operation, ok=response.ok, code=response.error.as_ref().map_or("OK", |error| error.code.as_str()), receipt_id=response.receipt_id.as_deref().unwrap_or(""), "command.completed");
        response
    }
    fn dispatch_sync(
        self: &Arc<Self>,
        context: Context,
        request: Request,
        cancellation: CancellationToken,
    ) -> Result<Response> {
        request.validate()?;
        let descriptor = commands::descriptor(&request.command.id)
            .ok_or_else(|| DomainError::new("UNKNOWN_COMMAND", "Unregistered command"))?;
        let durable = matches!(descriptor.state_class, StateClass::DurableMutation);
        let guard = if durable || matches!(descriptor.state_class, StateClass::Bootstrap) {
            Some(self.mutation.lock().map_err(|_| lock_error())?)
        } else {
            None
        };
        let policy = self.policy(&context)?;
        let now = self.clock.now()?;
        if durable
            && let Some(response) =
                self.store
                    .replay_with_clock(&context.principal, &request, || self.clock.now())?
        {
            return self.replay_view(&context, &request, response);
        }
        if let Some(profile) = &policy
            && !profile.allows_command(&request.command.id)
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Command is outside the explicit profile allowlist",
            ));
        }
        if !durable && (request.idempotency_key.is_some() || request.replay_scope.is_some()) {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Query/bootstrap does not accept durable mutation identity",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Request cancelled before new acceptance",
            ));
        }
        let runtime = tokio::runtime::Handle::current();
        let params = request.params.clone();
        let result = match request.command.id.as_str() {
            "artifacts.list" => {
                self.artifact_inventory(&context, commands::parse(params)?, &cancellation)?
            }
            "artifacts.manifest" => {
                let input: nw_domain::artifacts::ManifestInput = commands::parse(params)?;
                value(&self.artifact_manifest(&context, &input.artifact_id, &cancellation)?)?
            }
            "artifacts.read_chunk" => {
                let input: nw_domain::artifacts::ChunkInput = commands::parse(params)?;
                value(&self.artifact_chunk(context.clone(), input, cancellation.clone())?)?
            }
            "logs.status" => {
                let _: Empty = commands::parse(params)?;
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Protected log status requires local-owner authority",
                    ));
                }
                value(&nw_platform::logging::status())?
            }
            "commands.list" | "commands.schema" | "streams.clean.resolve" => {
                return Ok(super::CommandService.dispatch(super::Principal::LocalUser, request));
            }
            "access.replay_scope.create" => {
                let _: Empty = commands::parse(params)?;
                json!({"replay_scope":self.store.issue_scope_with_clock(&context.principal, || self.clock.now())?,"expires_in_seconds":604800})
            }
            "access.credentials.issue" => {
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Credential issuance requires native owner authority",
                    ));
                }
                let input: CredentialInput = commands::parse(params)?;
                json!({"bearer":self.credential_for_origin(&input.profile, input.port.map(|port| format!("http://127.0.0.1:{port}")))?,"profile":input.profile,"expires_in_seconds":28800})
            }
            "access.tokens.issue" => {
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Token issuance requires native owner authority",
                    ));
                }
                let input: nw_domain::access::TokenIssueInput = commands::parse(params)?;
                if input.port == 0 {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Select an explicit nonzero listener port",
                    ));
                }
                let bearer = self.credential_for_origin(
                    &input.for_access_profile,
                    Some(format!("http://127.0.0.1:{}", input.port)),
                )?;
                let token_id = hex::encode(Sha256::digest(bearer.as_bytes()));
                let credential: Credential = self
                    .store
                    .get(&format!("credential/{token_id}"))?
                    .ok_or_else(|| {
                        DomainError::new("STORAGE_ERROR", "Issued credential was not retained")
                    })?;
                json!({"token_id":token_id,"principal":credential.context.principal,"expires_at":credential.expires.to_string(),"bearer":bearer})
            }
            "access.tokens.revoke" => {
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Token revocation requires native owner authority",
                    ));
                }
                let input: nw_domain::access::TokenRevokeInput = commands::parse(params)?;
                if input.token_id.len() != 64
                    || !input
                        .token_id
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Token ID must be the lowercase SHA-256 identifier",
                    ));
                }
                let _credential_guard = self.credentials.lock().map_err(|_| lock_error())?;
                let key = format!("credential/{}", input.token_id);
                let mut credential: Credential = self
                    .store
                    .get(&key)?
                    .ok_or_else(DomainError::object_not_found)?;
                credential.revoked = true;
                return self.store.commit_with_clock(
                    &context.principal,
                    &request,
                    || self.clock.now(),
                    json!({"token_id":input.token_id,"revoked":true}),
                    &[(key, value(&credential)?)],
                );
            }
            "access.profiles.inspect" => {
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Policy inspection requires owner authority",
                    ));
                }
                let input: nw_domain::access::ProfileNameInput = commands::parse(params)?;
                value(
                    &self
                        .store
                        .get::<AccessProfile>(&named("profile", &input.name)?)?
                        .ok_or_else(DomainError::object_not_found)?,
                )?
            }
            "access.profiles.list" => {
                let _: Empty = commands::parse(params)?;
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Policy inspection requires owner authority",
                    ));
                }
                value(&self.store.list::<AccessProfile>("profile/", 128)?)?
            }
            "access.profiles.put" | "access.profiles.create" | "access.profiles.update" => {
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Only the native owner can change access profiles",
                    ));
                }
                let input: ProfileInput = if request.command.id == "access.profiles.create" {
                    let create: nw_domain::access::ProfileCreateInput = commands::parse(params)?;
                    ProfileInput {
                        name: create.name,
                        expected_revision: None,
                        enabled: true,
                        client_sessions: false,
                        mode: ProfileMode::Operate,
                        owned_object_access: true,
                        allowed_command_ids: nw_domain::access::ordinary_commands(),
                        object_grants: Vec::new(),
                        read_roots: create.read_roots,
                        write_roots: create.write_roots,
                    }
                } else {
                    commands::parse(params)?
                };
                if request.command.id == "access.profiles.update"
                    && input.expected_revision.is_none()
                {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Policy update requires an exact expected revision",
                    ));
                }
                let key = named("profile", &input.name)?;
                if input.name == "agent" {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "The default discovery profile is immutable; create a named operating profile",
                    ));
                }
                input.validate()?;
                for grant in &input.object_grants {
                    let prefix = match grant.kind {
                        ObjectKind::Project => "project",
                        ObjectKind::Plan => "plan",
                        ObjectKind::Job => "job",
                    };
                    if self
                        .store
                        .get::<Value>(&named(prefix, &grant.id)?)?
                        .is_none()
                    {
                        return Err(DomainError::object_not_found());
                    }
                }
                let old: Option<AccessProfile> = self.store.get(&key)?;
                if request.command.id == "access.profiles.create" && old.is_some() {
                    return Err(DomainError::new(
                        "OBJECT_CONFLICT",
                        "An access profile with that name already exists; inspect and update its exact revision",
                    ));
                }
                if old.as_ref().map(|profile| profile.revision) != input.expected_revision {
                    return Err(DomainError::new(
                        "STALE_REVISION",
                        "Profile revision changed or create/update intent is incorrect",
                    ));
                }
                let roots = |paths: Vec<String>| -> Result<Vec<String>> {
                    paths
                        .into_iter()
                        .map(|path| {
                            let path = std::fs::canonicalize(path).map_err(files::io_error)?;
                            if !path.is_dir() {
                                return Err(DomainError::new(
                                    "PATH_DENIED",
                                    "Grant roots must be existing directories",
                                ));
                            }
                            files::path_text(&path)
                        })
                        .collect()
                };
                let profile = AccessProfile {
                    schema_version: 2,
                    id: input.name.clone(),
                    name: input.name,
                    principal: old
                        .as_ref()
                        .map(|profile| profile.principal.clone())
                        .unwrap_or_else(|| Uuid::new_v4().to_string()),
                    revision: old.as_ref().map_or(Ok(1), |profile| {
                        profile.revision.checked_add(1).ok_or_else(|| {
                            DomainError::new(
                                "RESOURCE_LIMIT",
                                "Access-policy revision range exhausted",
                            )
                        })
                    })?,
                    enabled: input.enabled,
                    client_sessions: input.client_sessions,
                    mode: input.mode,
                    owned_object_access: input.owned_object_access,
                    allowed_command_ids: input.allowed_command_ids,
                    object_grants: input.object_grants,
                    read_roots: roots(input.read_roots)?,
                    write_roots: roots(input.write_roots)?,
                };
                let response = self.store.commit_with_clock(
                    &context.principal,
                    &request,
                    || self.clock.now(),
                    value(&profile)?,
                    &[(key, value(&profile)?)],
                )?;
                if let Err(error) = self.cancel_revoked() {
                    tracing::error!(code=%error.code,"Accepted policy change retained; active authority checks will fail closed");
                }
                return Ok(response);
            }
            "media.probe" => {
                let input: PathInput = commands::parse(params)?;
                let path = self.path(&context, Path::new(&input.path), false)?;
                value(&runtime.block_on(self.engine()?.probe(&path, cancellation.clone()))?)?
            }
            "assets.register" => {
                let input: nw_domain::ingest::RegisterInput = commands::parse(params)?;
                let upload_id = input.upload_id.clone();
                let (path, display_name) = match (input.path, input.upload_id) {
                    (Some(path), None) => {
                        (self.path(&context, Path::new(&path), false)?, String::new())
                    }
                    (None, Some(id)) => self.resolve_upload(&context, &id)?,
                    _ => {
                        return Err(DomainError::new(
                            "VALIDATION_ERROR",
                            "Select exactly one native path or verified upload_id",
                        ));
                    }
                };
                let engine = self.engine()?;
                let selected_path = path.clone();
                drop(guard);
                #[cfg(test)]
                self.preflight_started();
                let prepared = (|| -> Result<_> {
                    let stamp = files::SourceStamp::capture_resolved(&path).map_err(|_| {
                        DomainError::new(
                            "SOURCE_CHANGED",
                            "Admitted source changed before preflight",
                        )
                    })?;
                    let result = runtime.block_on(engine.register(
                        path,
                        context.principal.clone(),
                        cancellation.clone(),
                    ));
                    stamp.revalidate()?;
                    let asset = result?;
                    Ok((asset, stamp))
                })();
                let accept_guard = self.mutation.lock().map_err(|_| lock_error())?;
                if let Some(replay) =
                    self.refresh_acceptance(&context, &request, &cancellation, &accept_guard)?
                {
                    return Ok(replay);
                }
                self.root_authority(&context, &selected_path, false)?;
                if self.path(&context, &selected_path, false)? != selected_path {
                    return Err(DomainError::new(
                        "SOURCE_CHANGED",
                        "Preflight source now resolves to a different native path",
                    ));
                }
                if let Some(id) = upload_id {
                    let (current, _) = self.resolve_upload(&context, &id)?;
                    if current != selected_path {
                        return Err(DomainError::new(
                            "SOURCE_CHANGED",
                            "Managed input changed during preflight",
                        ));
                    }
                }
                let (mut asset, stamp) = prepared?;
                stamp.revalidate()?;
                asset.display_name = display_name;
                if self.path(&context, Path::new(&asset.identity.path), false)?
                    != Path::new(&asset.identity.path)
                {
                    return Err(DomainError::new(
                        "SOURCE_CHANGED",
                        "Registered source now resolves to a different native path",
                    ));
                }
                return self.store.commit_with_clock(
                    &context.principal,
                    &request,
                    || self.clock.now(),
                    value(&asset)?,
                    &[
                        (named("asset", &asset.id)?, value(&asset)?),
                        (
                            format!(
                                "managed-read/{}/{}",
                                hex::encode(Sha256::digest(asset.identity.path.as_bytes())),
                                context.principal
                            ),
                            json!(true),
                        ),
                    ],
                );
            }
            "assets.list" => {
                self.inventory_page(&context, commands::parse(params)?, false, &cancellation)?
            }
            "assets.inspect" => {
                let input: IdInput = commands::parse(params)?;
                let asset: Asset = self
                    .store
                    .get(&named("asset", &input.id)?)?
                    .ok_or_else(DomainError::object_not_found)?;
                self.object(&context, &asset.id, &asset.creator)?;
                self.path(&context, Path::new(&asset.identity.path), false)?;
                value(&asset)?
            }
            "assets.output_suggest" => {
                let input: IdInput = commands::parse(params)?;
                let asset: Asset = self
                    .store
                    .get(&named("asset", &input.id)?)?
                    .ok_or_else(DomainError::object_not_found)?;
                self.object(&context, &asset.id, &asset.creator)?;
                let parent = if let Some(policy) = &policy {
                    policy
                        .write_roots
                        .first()
                        .map(PathBuf::from)
                        .ok_or_else(|| {
                            DomainError::new(
                                "PATH_DENIED",
                                "Select an output grant before planning",
                            )
                        })?
                } else {
                    Path::new(&asset.identity.path)
                        .parent()
                        .ok_or_else(|| {
                            DomainError::new("PATH_DENIED", "Source parent unavailable")
                        })?
                        .to_owned()
                };
                let label = if asset.display_name.is_empty() {
                    &asset.identity.path
                } else {
                    &asset.display_name
                };
                let stem = Path::new(label)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("media")
                    .chars()
                    .filter(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_')
                    .take(80)
                    .collect::<String>();
                let extension = Path::new(&asset.identity.path)
                    .extension()
                    .and_then(|value| value.to_str())
                    .filter(|value| {
                        value.len() <= 12 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    })
                    .unwrap_or("mkv");
                let output = self.path(
                    &context,
                    &parent.join(format!(
                        "{stem}-clean-{}.{extension}",
                        Uuid::new_v4().simple()
                    )),
                    true,
                )?;
                json!({"output":files::path_text(&output)?})
            }
            "streams.clean" => {
                let input: CleanInput = commands::parse(params)?;
                let asset: Asset = self
                    .store
                    .get(&named("asset", &input.asset_id)?)?
                    .ok_or_else(DomainError::object_not_found)?;
                self.object(&context, &asset.id, &asset.creator)?;
                self.path(&context, Path::new(&asset.identity.path), false)?;
                let output = self.path(&context, Path::new(&input.output), true)?;
                let mut plan = self.engine()?.clean_plan_at(
                    asset,
                    &output,
                    &input.intent,
                    &context.principal,
                    now,
                )?;
                let rights = plan.required_rights.as_mut().ok_or_else(|| {
                    DomainError::new("INVALID_PLAN", "Planner omitted execution rights")
                })?;
                rights.data_root = self.store.root_id().into();
                if Path::new(&plan.source.identity.path).starts_with(self.root.join("assets")) {
                    rights.source_read.purpose =
                        nw_domain::plans::SourcePurpose::CreatorBoundManaged {
                            principal: context.principal.clone(),
                        };
                }
                plan.creation_audit
                    .as_mut()
                    .ok_or_else(|| {
                        DomainError::new("INVALID_PLAN", "Planner omitted creation audit")
                    })?
                    .policy_revision = policy.as_ref().map_or(0, |p| p.revision);
                plan.hash = nw_engine::plan_digest(&plan)?;
                let response = Response::success(
                    &request.request_id,
                    value(&self.plan_review(&context, &plan)?)?,
                )
                .bounded();
                if !response.ok {
                    return Ok(response);
                }
                self.store.put(&named("plan", &plan.id)?, &plan)?;
                return Ok(response);
            }
            "plans.inspect" => {
                let input: IdInput = commands::parse(params)?;
                let plan = self.read_plan(&context, &input.id, ObjectAction::Read)?;
                value(&self.plan_review(&context, &plan)?)?
            }
            "jobs.submit" => {
                let input: SubmitInput = commands::parse(params)?;
                let plan = self.read_plan(&context, &input.plan_id, ObjectAction::Submit)?;
                if plan.hash != input.plan_hash
                    || plan.expires_at.parse::<u64>().unwrap_or(0) <= now
                {
                    return Err(DomainError::new(
                        "INVALID_PLAN",
                        "Plan hash or expiry does not match the reviewed submission",
                    ));
                }
                self.plan_source_path(&context, &plan)?;
                self.path(&context, Path::new(&plan.output), true)?;
                files::revalidate_output(&plan.output)?;
                drop(guard);
                #[cfg(test)]
                self.preflight_started();
                let prepared = (|| -> Result<_> {
                    let engine = self.engine()?;
                    if engine.ffmpeg != plan.ffmpeg
                        || engine.ffprobe != plan.ffprobe
                        || nw_engine::plan_digest(&plan)? != plan.hash
                    {
                        return Err(DomainError::new(
                            "INVALID_PLAN",
                            "Frozen plan or selected component identity changed",
                        ));
                    }
                    let source =
                        files::SourceStamp::capture_resolved(Path::new(&plan.source.identity.path))
                            .map_err(|_| {
                                DomainError::new(
                                    "SOURCE_CHANGED",
                                    "Reviewed source changed before preflight",
                                )
                            })?;
                    let ffmpeg = files::SourceStamp::capture_resolved(Path::new(&plan.ffmpeg.path))
                        .map_err(|_| {
                            DomainError::new(
                                "COMPONENT_CHANGED",
                                "Reviewed FFmpeg component changed before preflight",
                            )
                        })?;
                    let ffprobe =
                        files::SourceStamp::capture_resolved(Path::new(&plan.ffprobe.path))
                            .map_err(|_| {
                                DomainError::new(
                                    "COMPONENT_CHANGED",
                                    "Reviewed ffprobe component changed before preflight",
                                )
                            })?;
                    files::revalidate(&plan.source.identity, &cancellation)?;
                    for component in [&plan.ffmpeg, &plan.ffprobe] {
                        files::revalidate(component, &cancellation).map_err(|error| {
                            if error.code == "SOURCE_CHANGED" {
                                DomainError::new(
                                    "COMPONENT_CHANGED",
                                    "Reviewed component bytes changed during preflight",
                                )
                            } else {
                                error
                            }
                        })?;
                    }
                    source.revalidate()?;
                    ffmpeg.revalidate()?;
                    ffprobe.revalidate()?;
                    Ok((source, ffmpeg, ffprobe))
                })();
                let accept_guard = self.mutation.lock().map_err(|_| lock_error())?;
                if let Some(replay) =
                    self.refresh_acceptance(&context, &request, &cancellation, &accept_guard)?
                {
                    return Ok(replay);
                }
                let current = self.read_plan(&context, &input.plan_id, ObjectAction::Submit)?;
                if current.hash != plan.hash
                    || nw_engine::plan_digest(&current)? != plan.hash
                    || plan.expires_at.parse::<u64>().unwrap_or(0) <= self.clock.now()?
                {
                    return Err(DomainError::new(
                        "INVALID_PLAN",
                        "Reviewed plan changed or expired during preflight",
                    ));
                }
                self.plan_source_path(&context, &plan)?;
                self.path(&context, Path::new(&plan.output), true)?;
                files::revalidate_output(&plan.output)?;
                let policy = self.policy(&context)?;
                let (source, ffmpeg, ffprobe) = prepared?;
                source.revalidate()?;
                for component in [ffmpeg, ffprobe] {
                    component.revalidate().map_err(|_| {
                        DomainError::new(
                            "COMPONENT_CHANGED",
                            "Reviewed component identity changed before acceptance",
                        )
                    })?;
                }
                if self.pending.load(Ordering::Acquire) >= 128 {
                    return Err(DomainError::new(
                        "RESOURCE_LIMIT",
                        "Durable job queue budget reached",
                    ));
                }
                let id = Uuid::new_v4().to_string();
                let job = Job {
                    id: id.clone(),
                    creator: context.principal.clone(),
                    plan,
                    state: JobState::Queued,
                    prepared_argv: None,
                    prepared_stages: Vec::new(),
                    owned_staging: None,
                    artifact: None,
                    verification: None,
                    error: None,
                };
                let authorization = ExecutionAuthorization {
                    principal: context.principal.clone(),
                    profile: context.profile,
                    revision: policy.as_ref().map_or(0, |profile| profile.revision),
                };
                let response = self.store.commit_with_clock(
                    &context.principal,
                    &request,
                    || self.clock.now(),
                    json!({"job_id":id,"state":"queued"}),
                    &[
                        (named("job", &id)?, value(&job)?),
                        (named("job-auth", &id)?, value(&authorization)?),
                    ],
                )?;
                if let Err(error) = self.schedule(id) {
                    tracing::error!(code=%error.code,"Job acceptance is durable; restart the owner to resume the retained queued job");
                }
                return Ok(response);
            }
            "jobs.inspect" => {
                let input: IdInput = commands::parse(params)?;
                let job = self.store.inspect_job(&input.id)?;
                self.object_action(
                    &context,
                    ObjectKind::Job,
                    &job.id,
                    &job.creator,
                    ObjectAction::Read,
                )?;
                value(&self.job_view(&context, &job)?)?
            }
            "jobs.list" => {
                self.inventory_page(&context, commands::parse(params)?, true, &cancellation)?
            }
            "jobs.cancel" => {
                let input: IdInput = commands::parse(params)?;
                let mut job = self.store.inspect_job(&input.id)?;
                self.object_action(
                    &context,
                    ObjectKind::Job,
                    &job.id,
                    &job.creator,
                    ObjectAction::Control,
                )?;
                if job.state == JobState::Publishing {
                    return Err(DomainError::new(
                        "CANCELLATION_TOO_LATE",
                        "The durable publication fence has committed",
                    ));
                }
                if job.state == JobState::Queued {
                    job.state = JobState::Cancelled;
                }
                let response = self.store.commit_with_clock(&context.principal, &request, || self.clock.now(), json!({"job_id":job.id,"state":job.state,"cancel_requested":!job.state.terminal()}), &[(named("cancel", &job.id)?, json!(true)), (named("job", &job.id)?, value(&job)?)])?;
                if let Some(token) = self
                    .cancellations
                    .lock()
                    .map_err(|_| lock_error())?
                    .get(&job.id)
                {
                    token.cancel();
                }
                return Ok(response);
            }
            _ => {
                return Err(DomainError::new(
                    "UNKNOWN_COMMAND",
                    "Registered operation has no handler",
                ));
            }
        };
        Ok(Response::success(&request.request_id, result))
    }
    fn schedule(self: &Arc<Self>, id: String) -> Result<()> {
        let token = CancellationToken::new();
        self.cancellations
            .lock()
            .map_err(|_| lock_error())?
            .insert(id.clone(), token.clone());
        self.pending.fetch_add(1, Ordering::AcqRel);
        let service = self.clone();
        tokio::spawn(async move {
            let _pending = Pending(service.pending.clone());
            let _permit = match service.heavy.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return,
            };
            let start = service.clone();
            let job_id = id.clone();
            let started = tokio::task::spawn_blocking(move || {
                let _guard = start.mutation.lock().map_err(|_| lock_error())?;
                let job = start.store.inspect_job(&job_id)?;
                if job.state.terminal() {
                    return Ok(None);
                }
                start.store.execution_allowed(&job_id)?;
                start.validate_job_paths(&job)?;
                start
                    .store
                    .transition_job(&job_id, JobState::Queued, JobState::Running)
                    .map(Some)
            })
            .await;
            let outcome = match started {
                Ok(Ok(Some(job))) => {
                    let observer_service = service.clone();
                    let observer_id = id.clone();
                    let observer: Observer = Arc::new(move |event| {
                        let service = observer_service.clone();
                        let id = observer_id.clone();
                        Box::pin(async move {
                            tokio::task::spawn_blocking(move || service.execution_event(&id, event))
                                .await
                                .map_err(|error| {
                                    DomainError::new("WORKER_FAILED", error.to_string())
                                })?
                        })
                    });
                    match service.engine() {
                        Ok(engine) => engine
                            .execute_observed(&job.plan, token, &id, observer)
                            .await
                            .map(|_| ()),
                        Err(error) => Err(error),
                    }
                }
                Ok(Ok(None)) => Ok(()),
                Ok(Err(error)) => Err(error),
                Err(error) => Err(DomainError::new("WORKER_FAILED", error.to_string())),
            };
            let finish = service.clone();
            let finish_id = id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _guard = finish.mutation.lock().map_err(|_| lock_error())?;
                let job = finish.store.inspect_job(&finish_id)?;
                if !job.state.terminal() {
                    if job.state == JobState::Publishing {
                        finish.reconcile(&job)?;
                    } else {
                        finish.store.update_job(&finish_id, |job| {
                            match outcome {
                                Ok(()) => {
                                    job.state = JobState::Skipped;
                                    job.verification = Some(
                                        "Explicit no-match policy produced no artifact".into(),
                                    );
                                }
                                Err(error) => {
                                    job.state = if error.code == "CANCELLED" {
                                        JobState::Cancelled
                                    } else {
                                        JobState::Failed
                                    };
                                    job.error = Some(error);
                                }
                            }
                            Ok(())
                        })?;
                    }
                }
                finish
                    .cancellations
                    .lock()
                    .map_err(|_| lock_error())?
                    .remove(&finish_id);
                let _ = finish.events.send(finish_id);
                Ok::<_, DomainError>(())
            })
            .await;
        });
        Ok(())
    }
    fn cancel_revoked(&self) -> Result<()> {
        let tokens = self
            .cancellations
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .map(|(id, token)| (id.clone(), token.clone()))
            .collect::<Vec<_>>();
        for (id, token) in tokens {
            if self.store.execution_allowed(&id).is_err() {
                let job = self.store.inspect_job(&id)?;
                if job.state != JobState::Publishing && !job.state.terminal() {
                    token.cancel();
                }
            }
        }
        Ok(())
    }
    fn validate_job_paths(&self, job: &Job) -> Result<()> {
        let authorization: ExecutionAuthorization = self
            .store
            .get(&named("job-auth", &job.id)?)?
            .ok_or_else(|| {
            DomainError::new(
                "PERMISSION_DENIED",
                "Durable execution authority is missing",
            )
        })?;
        let context = Context {
            principal: authorization.principal,
            profile: authorization.profile,
        };
        self.plan_source_path(&context, &job.plan)?;
        self.path(&context, Path::new(&job.plan.output), true)?;
        files::revalidate_output(&job.plan.output)?;
        Ok(())
    }
    fn execution_event(&self, id: &str, event: ExecutionEvent) -> Result<()> {
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        match event {
            ExecutionEvent::StagingIntent(intent) => {
                self.store.execution_allowed(id)?;
                if intent.job_id != id {
                    return Err(DomainError::new(
                        "INVALID_JOB_TRANSITION",
                        "Staging intent must match its accepted job",
                    ));
                }
                self.store.update_job(id, |job| {
                    if job.owned_staging.is_some() {
                        return Err(DomainError::new(
                            "INVALID_JOB_TRANSITION",
                            "An accepted attempt already has staging intent",
                        ));
                    }
                    job.owned_staging = Some(intent);
                    Ok(())
                })?;
            }
            ExecutionEvent::Prepared(argv) => {
                self.store.execution_allowed(id)?;
                self.validate_job_paths(&self.store.inspect_job(id)?)?;
                self.store.update_job(id, |job| {
                    if job.prepared_stages.len() >= 32 {
                        return Err(DomainError::new(
                            "RESOURCE_LIMIT",
                            "Job prepared-stage budget reached",
                        ));
                    }
                    if job.prepared_argv.is_none() {
                        job.prepared_argv = Some(argv.clone());
                    }
                    job.prepared_stages.push(argv);
                    Ok(())
                })?;
            }
            ExecutionEvent::Verifying => {
                self.store.execution_allowed(id)?;
                self.store
                    .transition_job(id, JobState::Running, JobState::Verifying)?;
            }
            ExecutionEvent::Fence(publication) => {
                self.validate_job_paths(&self.store.inspect_job(id)?)?;
                self.store.publication_fence(&publication)?;
            }
            ExecutionEvent::Published(artifact, verification) => {
                self.store.published(id, artifact, verification)?;
            }
        }
        let _ = self.events.send(id.into());
        Ok(())
    }
    fn reconcile(&self, job: &Job) -> Result<()> {
        let publication: Option<Publication> = self.store.get(&named("publication", &job.id)?)?;
        if let Some(publication) = publication
            && Path::new(&publication.output)
                .try_exists()
                .map_err(files::io_error)?
            && Path::new(&publication.proof)
                .try_exists()
                .map_err(files::io_error)?
            && files::same_object(
                Path::new(&publication.proof),
                Path::new(&publication.output),
            )?
        {
            let actual =
                files::identity(Path::new(&publication.output), &CancellationToken::new())?;
            if actual.sha256 == publication.identity.sha256
                && actual.bytes == publication.identity.bytes
            {
                self.store
                    .published(&job.id, actual, publication.verified)?;
                return Ok(());
            }
        }
        self.store.update_job(&job.id, |job| { job.state = JobState::Interrupted; job.error = Some(DomainError::new("PUBLICATION_UNCONFIRMED", "Owned hard-link identity and verified bytes did not prove publication; unrelated outputs were preserved")); Ok(()) })?;
        Ok(())
    }
    fn recover(&self) -> Result<()> {
        for job in self.store.list::<Job>("job/", 10000)? {
            if job.state == JobState::Publishing {
                self.reconcile(&job)?;
            } else if !job.state.terminal() && job.state != JobState::Queued {
                self.store.update_job(&job.id, |job| { job.state = JobState::Interrupted; job.error = Some(DomainError::new("OWNER_INTERRUPTED", "Owner stopped during execution; no PID was reused and no filename was assumed to be an artifact")); Ok(()) })?;
            }
        }
        Ok(())
    }
}
struct Pending(Arc<AtomicUsize>);
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn parameter_errors_obey_the_result_budget_as_well_as_successful_queries() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let request = Request::query("logs.status", json!({("x".repeat(600 * 1024)):true}));
        let encoded = serde_json::to_vec(&request).unwrap();
        assert!(encoded.len() < commands::MAX_COMMAND_BYTES);
        commands::decode_json(&encoded).unwrap();
        let response = service.dispatch(service.owner(), request).await;
        assert!(!response.ok);
        assert_eq!(response.error.as_ref().unwrap().code, "RESOURCE_LIMIT");
        assert!(serde_json::to_vec(&response).unwrap().len() <= commands::MAX_RESULT_BYTES);
    }
    pub(super) async fn preflight_fixture()
    -> (tempfile::TempDir, Arc<Service>, Context, Request, PathBuf) {
        use nw_domain::{mux::*, records::MediaProbe};
        let root = tempfile::tempdir().unwrap();
        let programs = root.path().join("programs");
        std::fs::create_dir(&programs).unwrap();
        let ffmpeg = programs.join("ffmpeg");
        let ffprobe = programs.join("ffprobe");
        // Synthetic components are hashed for plan identity; no media executable is launched.
        std::fs::write(&ffmpeg, b"preflight identity fixture").unwrap();
        std::fs::write(&ffprobe, b"preflight identity fixture").unwrap();
        let service = Service::open(root.path().join("data"), Some((ffmpeg, ffprobe)))
            .await
            .unwrap();
        let canonical_root =
            files::path_text(&std::fs::canonicalize(root.path()).unwrap()).unwrap();
        let profile = AccessProfile {
            schema_version: 2,
            id: "preflight".into(),
            name: "preflight".into(),
            principal: "preflight-principal".into(),
            revision: 1,
            enabled: true,
            client_sessions: false,
            mode: ProfileMode::Operate,
            owned_object_access: true,
            allowed_command_ids: ["jobs.submit".into(), "access.replay_scope.create".into()].into(),
            object_grants: Vec::new(),
            read_roots: vec![canonical_root.clone()],
            write_roots: vec![canonical_root],
        };
        service.store.put("profile/preflight", &profile).unwrap();
        let context = service.profile_context("preflight").unwrap();
        let source = root.path().join("source.mkv");
        std::fs::write(&source, b"synthetic preflight bytes, not a media container").unwrap();
        let asset = Asset {
            id: "preflight-asset".into(),
            creator: context.principal.clone(),
            display_name: "source.mkv".into(),
            identity: files::identity(&source, &CancellationToken::new()).unwrap(),
            probe: MediaProbe {
                stream_details: Default::default(),
                streams: vec![
                    Stream {
                        index: 0,
                        kind: StreamKind::Video,
                        codec: "ffv1".into(),
                        tags: Default::default(),
                        dispositions: Default::default(),
                    },
                    Stream {
                        index: 1,
                        kind: StreamKind::Audio,
                        codec: "pcm_s16le".into(),
                        tags: Default::default(),
                        dispositions: Default::default(),
                    },
                ],
                format: "synthetic-preflight".into(),
                global_tags: Default::default(),
                chapters: json!([]),
                verification_scope: "Synthetic preflight race, not actual media evidence".into(),
            },
        };
        let intent = CleanIntent {
            audio_selection: Selection {
                selector: Selector::All,
                no_match: NoMatch::SkipFile,
            },
            subtitle_selection: Selection {
                selector: Selector::All,
                no_match: NoMatch::AllowEmpty,
            },
            audio_order: Vec::new(),
            subtitle_order: Vec::new(),
            stream_tag_patches: Vec::new(),
            preservation: Preservation::default(),
        };
        let output = root.path().join("output.mkv");
        let mut plan = service
            .engine()
            .unwrap()
            .clean_plan_at(
                asset,
                &output,
                &intent,
                &context.principal,
                service.clock.now().unwrap(),
            )
            .unwrap();
        plan.required_rights.as_mut().unwrap().data_root = service.store.root_id().into();
        plan.hash = nw_engine::plan_digest(&plan).unwrap();
        service
            .store
            .put(&named("plan", &plan.id).unwrap(), &plan)
            .unwrap();
        let mut request = Request::query(
            "jobs.submit",
            json!({"plan_id":plan.id,"plan_hash":plan.hash}),
        );
        request.replay_scope = Some(
            service
                .store
                .issue_scope(&context.principal, service.clock.now().unwrap())
                .unwrap(),
        );
        request.idempotency_key = Some("preflight-race".into());
        (root, service, context, request, output)
    }
    #[tokio::test]
    async fn captured_managed_purpose_cannot_be_delegated_or_borrow_creator_rights() {
        let (root, service, context, request, _) = preflight_fixture().await;
        let mut plan = service
            .store
            .get_plan(request.params["plan_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        let managed = service
            .root
            .join("assets")
            .join("synthetic-managed")
            .join("source.mkv");
        std::fs::create_dir_all(managed.parent().unwrap()).unwrap();
        std::fs::copy(root.path().join("source.mkv"), &managed).unwrap();
        plan.source.identity = files::identity(&managed, &CancellationToken::new()).unwrap();
        let rights = plan.required_rights.as_mut().unwrap();
        rights.source_read.identity = plan.source.identity.clone();
        rights.source_read.purpose = nw_domain::plans::SourcePurpose::CreatorBoundManaged {
            principal: context.principal.clone(),
        };
        plan.hash = nw_engine::plan_digest(&plan).unwrap();
        let purpose = format!(
            "managed-read/{}/{}",
            hex::encode(Sha256::digest(plan.source.identity.path.as_bytes())),
            context.principal
        );
        service.store.put(&purpose, &true).unwrap();
        assert!(service.plan_source_path(&context, &plan).is_ok());
        let mut policy: AccessProfile = service.store.get("profile/preflight").unwrap().unwrap();
        policy.name = "second".into();
        policy.id = "second".into();
        policy.principal = "second-principal".into();
        policy.object_grants = vec![nw_domain::access::ObjectGrant {
            kind: ObjectKind::Plan,
            id: plan.id.clone(),
            actions: [ObjectAction::Read, ObjectAction::Submit].into(),
        }];
        service.store.put("profile/second", &policy).unwrap();
        let second = service.profile_context("second").unwrap();
        assert!(
            service
                .object_action(
                    &second,
                    ObjectKind::Plan,
                    &plan.id,
                    &plan.creator,
                    ObjectAction::Submit
                )
                .is_ok()
        );
        assert_eq!(
            service.plan_source_path(&second, &plan).unwrap_err().code,
            "INVALID_PLAN"
        );
        let mut own: AccessProfile = service.store.get("profile/preflight").unwrap().unwrap();
        own.owned_object_access = false;
        own.revision += 1;
        service.store.put("profile/preflight", &own).unwrap();
        assert_eq!(
            service.plan_source_path(&context, &plan).unwrap_err().code,
            "OBJECT_NOT_FOUND"
        );
        assert!(!Path::new(&plan.output).exists());
    }
    #[tokio::test]
    async fn reviewed_native_rights_revalidate_current_roots_without_rehashing_audit() {
        let (_root, service, context, request, _) = preflight_fixture().await;
        let mut plan = service
            .store
            .get_plan(request.params["plan_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        let hash = plan.hash.clone();
        let mut policy: AccessProfile = service.store.get("profile/preflight").unwrap().unwrap();
        policy.revision += 1;
        policy.allowed_command_ids.insert("commands.list".into());
        service.store.put("profile/preflight", &policy).unwrap();
        assert!(service.plan_source_path(&context, &plan).is_ok());
        assert_eq!(nw_engine::plan_digest(&plan).unwrap(), hash);
        policy.read_roots.clear();
        policy.revision += 1;
        service.store.put("profile/preflight", &policy).unwrap();
        assert_eq!(
            service.plan_source_path(&context, &plan).unwrap_err().code,
            "PATH_DENIED"
        );
        assert_eq!(nw_engine::plan_digest(&plan).unwrap(), hash);
        plan.required_rights.as_mut().unwrap().data_root = "other-root".into();
        assert_eq!(
            service
                .plan_source_path(&service.owner(), &plan)
                .unwrap_err()
                .code,
            "INVALID_PLAN"
        );
        plan.schema_version = 1;
        plan.required_rights = None;
        plan.creation_audit = None;
        assert_eq!(
            service
                .plan_source_path(&service.owner(), &plan)
                .unwrap_err()
                .code,
            "INVALID_PLAN"
        );
    }
    #[tokio::test]
    async fn raw_plan_compatibility_errors_are_not_disclosed_before_object_authority() {
        let (_root, service, context, request, _) = preflight_fixture().await;
        let id = request.params["plan_id"].as_str().unwrap();
        let mut raw: Value = service
            .store
            .get(&named("plan", id).unwrap())
            .unwrap()
            .unwrap();
        raw["schema_version"] = json!(99);
        service
            .store
            .put(&named("plan", id).unwrap(), &raw)
            .unwrap();
        let mut policy: AccessProfile = service.store.get("profile/preflight").unwrap().unwrap();
        policy.owned_object_access = false;
        service.store.put("profile/preflight", &policy).unwrap();
        assert_eq!(
            service
                .read_plan(&context, id, ObjectAction::Read)
                .unwrap_err()
                .code,
            "OBJECT_NOT_FOUND"
        );
        assert_eq!(
            service
                .read_plan(&context, "absent", ObjectAction::Read)
                .unwrap_err()
                .code,
            "OBJECT_NOT_FOUND"
        );
        assert_eq!(
            service
                .read_plan(&service.owner(), id, ObjectAction::Read)
                .unwrap_err()
                .code,
            "INVALID_PLAN"
        );
    }
    #[tokio::test]
    async fn policy_revocation_is_not_held_behind_expensive_preflight() {
        let (_root, service, context, request, output) = preflight_fixture().await;
        let (started, ready) = std::sync::mpsc::channel();
        let (resume, waiting) = std::sync::mpsc::channel();
        let waiting = Mutex::new(waiting);
        *service.preflight_hook.lock().unwrap() = Some(Arc::new(move || {
            started.send(()).unwrap();
            waiting
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }));
        let worker = service.clone();
        let task = tokio::spawn(async move { worker.dispatch(context, request).await });
        tokio::task::spawn_blocking(move || {
            ready
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
        })
        .await
        .unwrap();
        let owner = service.owner();
        let scope = service
            .store
            .issue_scope(&owner.principal, service.clock.now().unwrap())
            .unwrap();
        let changed = tokio::time::timeout(std::time::Duration::from_secs(5), mutate(&service, &owner, &scope, "access.profiles.update", json!({"name":"preflight","expected_revision":1,"enabled":false,"mode":"operate","owned_object_access":true,"allowed_command_ids":[],"object_grants":[],"read_roots":[],"write_roots":[]}))).await;
        resume.send(()).unwrap();
        let response = task.await.unwrap();
        assert!(changed.expect("Revocation was blocked by preflight").ok);
        assert_eq!(response.error.unwrap().code, "PERMISSION_DENIED");
        assert!(service.store.list::<Job>("job/", 4096).unwrap().is_empty());
        assert!(!output.exists());
    }
    #[tokio::test]
    async fn a_receipt_accepted_during_preflight_wins_over_its_later_source_failure() {
        let (root, service, context, request, _output) = preflight_fixture().await;
        let (started, ready) = std::sync::mpsc::channel();
        let (resume, waiting) = std::sync::mpsc::channel();
        let waiting = Mutex::new(waiting);
        let entered = AtomicUsize::new(0);
        *service.preflight_hook.lock().unwrap() = Some(Arc::new(move || {
            if entered.fetch_add(1, Ordering::AcqRel) == 0 {
                started.send(()).unwrap();
                waiting
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
            }
        }));
        let worker = service.clone();
        let effective = context.clone();
        let original = request.clone();
        let first = tokio::spawn(async move { worker.dispatch(effective, original).await });
        tokio::task::spawn_blocking(move || {
            ready
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
        })
        .await
        .unwrap();
        let accepted = service.dispatch(context, request).await;
        assert!(accepted.ok, "{accepted:?}");
        std::fs::remove_file(root.path().join("source.mkv")).unwrap();
        resume.send(()).unwrap();
        let repeated = first.await.unwrap();
        assert!(repeated.ok, "{repeated:?}");
        assert_eq!(accepted.receipt_id, repeated.receipt_id);
        assert_eq!(accepted.result, repeated.result);
        assert_eq!(service.store.list::<Job>("job/", 4096).unwrap().len(), 1);
        let id = accepted.result.unwrap()["job_id"]
            .as_str()
            .unwrap()
            .to_owned();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !service.store.inspect_job(&id).unwrap().state.terminal() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn delegated_metadata_read_hides_native_paths_and_does_not_grant_submit_or_control() {
        use nw_domain::{
            access::ObjectGrant,
            mux::{CleanAction, CleanResolution, Preservation},
            records::{FileIdentity, MediaProbe},
        };
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let reader = service.profile_context("browser").unwrap();
        let owner = service.owner();
        let identity = FileIdentity {
            path: files::path_text(&root.path().join("private/source.mkv")).unwrap(),
            bytes: "100".into(),
            sha256: "00".repeat(32),
        };
        let asset = Asset {
            id: "private-asset".into(),
            creator: owner.principal.clone(),
            display_name: "source.mkv".into(),
            identity: identity.clone(),
            probe: MediaProbe {
                stream_details: Default::default(),
                streams: Vec::new(),
                format: "metadata-view-only".into(),
                global_tags: Default::default(),
                chapters: json!([]),
                verification_scope: "Synthetic metadata view, no media execution".into(),
            },
        };
        let plan = Plan {
            schema_version: 1,
            required_rights: None,
            creation_audit: None,
            id: "private-plan".into(),
            creator: owner.principal.clone(),
            hash: "synthetic-not-executable".into(),
            source: asset,
            output: files::path_text(&root.path().join("private/export.mkv")).unwrap(),
            ffmpeg: identity.clone(),
            ffprobe: identity.clone(),
            resolution: CleanResolution {
                action: CleanAction::CopyUnchanged,
                streams: Vec::new(),
                outcomes: Vec::new(),
                reasons: Vec::new(),
                warnings: Vec::new(),
                preservation: Preservation::default(),
            },
            argv_template: vec![identity.path.clone()],
            created_at: "1".into(),
            expires_at: "2".into(),
            verification_scope: "Synthetic metadata view".into(),
        };
        let job = Job {
            id: "private-job".into(),
            creator: owner.principal.clone(),
            plan: plan.clone(),
            state: JobState::Succeeded,
            prepared_argv: Some(vec![identity.path.clone()]),
            prepared_stages: vec![vec![identity.path.clone()]],
            owned_staging: None,
            artifact: Some(identity.clone()),
            verification: Some("Synthetic metadata view".into()),
            error: None,
        };
        service.store.put("plan/private-plan", &plan).unwrap();
        service.store.put("job/private-job", &job).unwrap();
        let mut policy: AccessProfile = service.store.get("profile/browser").unwrap().unwrap();
        policy.owned_object_access = false;
        policy.object_grants = vec![
            ObjectGrant {
                kind: ObjectKind::Job,
                id: job.id.clone(),
                actions: [ObjectAction::Read].into(),
            },
            ObjectGrant {
                kind: ObjectKind::Plan,
                id: plan.id.clone(),
                actions: [ObjectAction::Read].into(),
            },
        ];
        service.store.put("profile/browser", &policy).unwrap();
        let observed = service
            .dispatch(
                reader.clone(),
                Request::query("jobs.inspect", json!({"id":job.id})),
            )
            .await;
        assert!(observed.ok, "{observed:?}");
        let observed = observed.result.unwrap();
        assert!(observed["prepared_stages"].is_null());
        assert!(observed["artifact"]["path"].is_null());
        assert!(observed["plan"]["source_path"].is_null());
        assert!(observed["plan"]["output_path"].is_null());
        assert!(!observed.to_string().contains(&identity.path));
        let submit = service
            .dispatch(
                reader.clone(),
                Request::query(
                    "jobs.submit",
                    json!({"plan_id":plan.id,"plan_hash":plan.hash}),
                ),
            )
            .await;
        // Durable operations need a replay scope before performing the object lookup.
        assert_eq!(submit.error.unwrap().code, "REPLAY_SCOPE_INVALID");
        let scope = service
            .store
            .issue_scope(&reader.principal, service.clock.now().unwrap())
            .unwrap();
        for (command, params) in [
            (
                "jobs.submit",
                json!({"plan_id":plan.id,"plan_hash":plan.hash}),
            ),
            ("jobs.cancel", json!({"id":job.id})),
        ] {
            let mut request = Request::query(command, params);
            request.replay_scope = Some(scope.clone());
            request.idempotency_key = Some(command.into());
            assert_eq!(
                service
                    .dispatch(reader.clone(), request)
                    .await
                    .error
                    .unwrap()
                    .code,
                "OBJECT_NOT_FOUND"
            );
        }
        policy.object_grants.clear();
        policy.revision += 1;
        service.store.put("profile/browser", &policy).unwrap();
        assert_eq!(
            service
                .dispatch(
                    reader.clone(),
                    Request::query("jobs.inspect", json!({"id":job.id}))
                )
                .await
                .error
                .unwrap()
                .code,
            "OBJECT_NOT_FOUND"
        );
        assert_eq!(
            service
                .dispatch(reader, Request::query("jobs.list", json!({})))
                .await
                .result
                .unwrap()["total_count"],
            0
        );
    }
    #[tokio::test]
    async fn an_oversized_plan_response_does_not_fill_retained_plan_history() {
        use nw_domain::mux::*;
        let root = tempfile::tempdir().unwrap();
        let component = std::env::current_exe().unwrap();
        // This fixture only plans; neither synthetic executable is launched as media software.
        let service = Service::open(
            root.path().join("data"),
            Some((component.clone(), component)),
        )
        .await
        .unwrap();
        let owner = service.owner();
        let source = root.path().join("source.mkv");
        std::fs::write(&source, b"Plan response budget fixture").unwrap();
        let mut streams = vec![Stream {
            index: 0,
            kind: StreamKind::Video,
            codec: "ffv1".into(),
            tags: Default::default(),
            dispositions: Default::default(),
        }];
        streams.extend((1..=64).map(|index| Stream {
            index,
            kind: StreamKind::Audio,
            codec: "pcm_s16le".into(),
            tags: Default::default(),
            dispositions: Default::default(),
        }));
        let asset = Asset {
            id: "plan-budget-asset".into(),
            creator: owner.principal.clone(),
            display_name: "source.mkv".into(),
            identity: files::identity(&source, &CancellationToken::new()).unwrap(),
            probe: nw_domain::records::MediaProbe {
                streams,
                stream_details: Default::default(),
                format: "matroska,webm".into(),
                global_tags: Default::default(),
                chapters: json!([]),
                verification_scope: "Synthetic planner-only fixture".into(),
            },
        };
        service
            .store
            .put("asset/plan-budget-asset", &asset)
            .unwrap();
        let intent = CleanIntent {
            audio_selection: Selection {
                selector: Selector::All,
                no_match: NoMatch::SkipFile,
            },
            subtitle_selection: Selection {
                selector: Selector::All,
                no_match: NoMatch::AllowEmpty,
            },
            audio_order: Vec::new(),
            subtitle_order: Vec::new(),
            preservation: Preservation::default(),
            stream_tag_patches: (1..=64)
                .map(|index| StreamTagPatch {
                    kind: StreamKind::Audio,
                    source_indices: vec![index],
                    original_languages: Vec::new(),
                    language: TagPatch::Keep,
                    title: TagPatch::Set {
                        value: "x".repeat(4000),
                    },
                })
                .collect(),
        };
        let request = Request::query(
            "streams.clean",
            json!({"asset_id":asset.id,"output":root.path().join("output.mkv"),"intent":intent}),
        );
        assert!(serde_json::to_vec(&request).unwrap().len() < commands::MAX_COMMAND_BYTES);
        let response = service.dispatch(owner, request).await;
        assert_eq!(response.error.unwrap().code, "RESOURCE_LIMIT");
        assert!(service.store.list::<Plan>("plan/", 512).unwrap().is_empty());
        assert!(!root.path().join("output.mkv").exists());
    }
    #[tokio::test]
    async fn a_waiting_request_cannot_accept_a_profile_snapshot_from_before_revocation() {
        let directory = tempfile::tempdir().unwrap();
        let service = Service::open(directory.path().to_owned(), None)
            .await
            .unwrap();
        let mut profile = AccessProfile {
            schema_version: 2,
            id: "waiting-reader".into(),
            name: "waiting-reader".into(),
            principal: "waiting-principal".into(),
            revision: 1,
            enabled: true,
            client_sessions: false,
            mode: ProfileMode::Operate,
            owned_object_access: true,
            allowed_command_ids: ["access.replay_scope.create".into()].into(),
            object_grants: Default::default(),
            read_roots: Vec::new(),
            write_roots: Vec::new(),
        };
        service
            .store
            .put("profile/waiting-reader", &profile)
            .unwrap();
        let context = service.profile_context("waiting-reader").unwrap();
        let guard = service.mutation.lock().unwrap();
        let (started, ready) = std::sync::mpsc::channel();
        let request_service = service.clone();
        let runtime = tokio::runtime::Handle::current();
        let worker = std::thread::spawn(move || {
            let _entered = runtime.enter();
            started.send(()).unwrap();
            request_service.dispatch_sync(
                context,
                Request::query("access.replay_scope.create", json!({})),
                CancellationToken::new(),
            )
        });
        ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        profile.enabled = false;
        profile.revision = 2;
        service
            .store
            .put("profile/waiting-reader", &profile)
            .unwrap();
        drop(guard);
        assert_eq!(
            worker.join().unwrap().unwrap_err().code,
            "PERMISSION_DENIED"
        );
    }
    async fn mutate(
        service: &Arc<Service>,
        context: &Context,
        scope: &str,
        id: &str,
        params: Value,
    ) -> Response {
        let mut request = Request::query(id, params);
        request.replay_scope = Some(scope.into());
        request.idempotency_key = Some(Uuid::new_v4().to_string());
        service.dispatch(context.clone(), request).await
    }
    #[tokio::test]
    async fn profile_replay_precedes_current_revision_and_revocation_invalidates_credentials() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().to_owned(), None).await.unwrap();
        let owner = service.owner();
        let scope = service
            .store
            .issue_scope(&owner.principal, nw_platform::epoch_seconds())
            .unwrap();
        let mut create = Request::query(
            "access.profiles.put",
            json!({"name":"operator","expected_revision":null,"enabled":true,"mode":"operate","owned_object_access":true,"allowed_command_ids":["commands.list"],"object_grants":[],"read_roots":[],"write_roots":[]}),
        );
        create.replay_scope = Some(scope.clone());
        create.idempotency_key = Some("stable-create".into());
        let accepted = service.dispatch(owner.clone(), create.clone()).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        let token = service.credential("operator").unwrap();
        let principal = service.authenticate(&token).unwrap().principal;
        let changed = mutate(&service, &owner, &scope, "access.profiles.put", json!({"name":"operator","expected_revision":1,"enabled":false,"mode":"operate","owned_object_access":true,"allowed_command_ids":[],"object_grants":[],"read_roots":[],"write_roots":[]})).await;
        assert!(changed.ok);
        assert!(service.authenticate(&token).is_err());
        let replay = service.dispatch(owner.clone(), create.clone()).await;
        assert_eq!(accepted.receipt_id, replay.receipt_id);
        assert_eq!(replay.result.unwrap()["revision"], 1);
        let current: AccessProfile = service.store.get("profile/operator").unwrap().unwrap();
        assert_eq!(current.revision, 2);
        assert!(!current.enabled);
        assert_eq!(current.principal, principal);
        create.params["enabled"] = json!(false);
        assert_eq!(
            service.dispatch(owner, create).await.error.unwrap().code,
            "IDEMPOTENCY_CONFLICT"
        );
    }
    #[tokio::test]
    async fn file_grants_and_object_rights_are_checked_independently() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        std::fs::write(&source, b"unit fixture").unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let owner = service.owner();
        let mut asset = Asset {
            display_name: String::new(),
            id: "owned-asset".into(),
            creator: owner.principal.clone(),
            identity: files::identity(&source, &CancellationToken::new()).unwrap(),
            probe: nw_domain::records::MediaProbe {
                stream_details: Default::default(),
                streams: vec![],
                format: "unit-only".into(),
                global_tags: Default::default(),
                chapters: json!([]),
                verification_scope: "authorization fixture only".into(),
            },
        };
        service.store.put("asset/owned-asset", &asset).unwrap();
        let scope = service
            .store
            .issue_scope(&owner.principal, nw_platform::epoch_seconds())
            .unwrap();
        let profile = json!({"name":"reader","expected_revision":null,"enabled":true,"mode":"operate","owned_object_access":true,"allowed_command_ids":["assets.list","streams.clean"],"object_grants":[],"read_roots":[root.path().to_str().unwrap()],"write_roots":[root.path().to_str().unwrap()]});
        assert!(
            mutate(&service, &owner, &scope, "access.profiles.put", profile)
                .await
                .ok
        );
        let reader = service.profile_context("reader").unwrap();
        assert_eq!(
            service
                .dispatch(reader.clone(), Request::query("assets.list", json!({})))
                .await
                .result
                .unwrap()["items"],
            json!([])
        );
        let intent = nw_domain::mux::CleanIntent {
            audio_selection: nw_domain::mux::Selection {
                selector: nw_domain::mux::Selector::All,
                no_match: nw_domain::mux::NoMatch::SkipFile,
            },
            subtitle_selection: nw_domain::mux::Selection {
                selector: nw_domain::mux::Selector::All,
                no_match: nw_domain::mux::NoMatch::AllowEmpty,
            },
            audio_order: vec![],
            subtitle_order: vec![],
            stream_tag_patches: vec![],
            preservation: nw_domain::mux::Preservation::default(),
        };
        let params =
            json!({"asset_id":asset.id,"output":root.path().join("out.mkv"),"intent":intent});
        let denied = service
            .dispatch(
                reader.clone(),
                Request::query("streams.clean", params.clone()),
            )
            .await
            .error
            .unwrap();
        assert_eq!(denied.code, "OBJECT_NOT_FOUND");
        let mut missing = params.clone();
        missing["asset_id"] = json!("absent-asset");
        let absent = service
            .dispatch(reader.clone(), Request::query("streams.clean", missing))
            .await
            .error
            .unwrap();
        assert_eq!(
            serde_json::to_value(&denied).unwrap(),
            serde_json::to_value(&absent).unwrap(),
            "An object lookup must not disclose whether an unauthorized ID exists"
        );
        asset.creator = reader.principal.clone();
        service.store.put("asset/owned-asset", &asset).unwrap();
        assert!(mutate(&service, &owner, &scope, "access.profiles.put", json!({"name":"reader","expected_revision":1,"enabled":true,"mode":"operate","owned_object_access":true,"allowed_command_ids":["streams.clean"],"object_grants":[],"read_roots":[],"write_roots":[]})).await.ok);
        assert_eq!(
            service
                .dispatch(reader, Request::query("streams.clean", params))
                .await
                .error
                .unwrap()
                .code,
            "PATH_DENIED"
        );
    }
    #[tokio::test]
    async fn staging_and_proof_namespaces_are_not_general_media_paths() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let owner = service.owner();
        for name in [
            ".numera-job-fixture",
            ".numera-owned-fixture",
            ".NUMERA-PROOF-fixture",
        ] {
            let directory = root.path().join(name);
            std::fs::create_dir(&directory).unwrap();
            let source = directory.join("source.mkv");
            std::fs::write(&source, b"Owned namespace boundary fixture").unwrap();
            assert_eq!(
                service.path(&owner, &source, false).unwrap_err().code,
                "PATH_DENIED"
            );
            assert_eq!(
                service
                    .path(&owner, &directory.join("new.mkv"), true)
                    .unwrap_err()
                    .code,
                "PATH_DENIED"
            );
            assert_eq!(
                service
                    .path(&owner, &directory.join("absent.mkv"), false)
                    .unwrap_err()
                    .code,
                "PATH_DENIED"
            );
        }
        let ordinary = root.path().join("ordinary.mkv");
        std::fs::write(&ordinary, b"Ordinary file boundary fixture").unwrap();
        assert!(service.path(&owner, &ordinary, false).is_ok());
        assert!(
            service
                .path(&owner, &root.path().join("ordinary-output.mkv"), true)
                .is_ok()
        );
    }
    #[tokio::test]
    async fn public_credentials_are_bound_to_origin_and_owner_epoch() {
        let root = tempfile::tempdir().unwrap();
        let first = Service::open(root.path().to_owned(), None).await.unwrap();
        let token = first
            .credential_for_origin("agent", Some("http://127.0.0.1:1578".into()))
            .unwrap();
        let principal = first
            .authenticate_at(&token, Some("http://127.0.0.1:1578"), true)
            .unwrap()
            .principal;
        assert!(first.authenticate(&token).is_err());
        assert!(
            first
                .authenticate_at(&token, Some("http://127.0.0.1:1579"), true)
                .is_err()
        );
        drop(first);
        let second = Service::open(root.path().to_owned(), None).await.unwrap();
        assert_eq!(
            second.profile_context("agent").unwrap().principal,
            principal
        );
        assert!(
            second
                .authenticate_at(&token, Some("http://127.0.0.1:1578"), true)
                .is_err()
        );
    }
    #[tokio::test]
    async fn broad_owner_authority_cannot_publish_media_into_component_namespaces() {
        let root = tempfile::tempdir().unwrap();
        let programs = root.path().join("programs");
        std::fs::create_dir(&programs).unwrap();
        let a = programs.join("ffmpeg");
        let b = programs.join("ffprobe");
        std::fs::write(&a, b"unexecuted path-policy fixture").unwrap();
        std::fs::write(&b, b"unexecuted path-policy fixture").unwrap();
        let service = Service::open(root.path().join("data"), Some((a, b)))
            .await
            .unwrap();
        assert_eq!(
            service
                .path(
                    &service.owner(),
                    &programs.join("untrusted-output.mkv"),
                    true
                )
                .unwrap_err()
                .code,
            "PATH_DENIED"
        );
        assert!(
            service
                .path(
                    &service.owner(),
                    &std::fs::canonicalize(root.path().join("data/exports"))
                        .unwrap()
                        .join("allowed.mkv"),
                    true
                )
                .is_ok()
        );
        assert!(!programs.join("untrusted-output.mkv").exists());
    }
}
