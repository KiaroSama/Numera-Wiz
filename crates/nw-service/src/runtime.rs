//! The owner is the sole command authority, durable acceptor and scheduler.
mod uploads;
use nw_domain::{
    DomainError, Result,
    access::{AccessProfile, CredentialInput, ExecutionAuthorization, ProfileInput, Publication},
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
}
pub struct Service {
    pub store: Arc<Store>,
    pub epoch: String,
    engine: Option<Engine>,
    root: PathBuf,
    mutation: Mutex<()>,
    credentials: Mutex<()>,
    origins: Mutex<HashMap<String, u64>>,
    requests: Arc<Semaphore>,
    heavy: Arc<Semaphore>,
    pending: Arc<AtomicUsize>,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    events: broadcast::Sender<String>,
}
fn lock_error() -> DomainError {
    DomainError::new("INTERNAL_ERROR", "Owner state lock was poisoned")
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
                    id: "agent".into(),
                    name: "agent".into(),
                    principal: Uuid::new_v4().to_string(),
                    revision: 1,
                    enabled: true,
                    client_sessions: false,
                    commands: BTreeSet::from([
                        "commands.list".into(),
                        "commands.schema".into(),
                        "access.replay_scope.create".into(),
                    ]),
                    objects: BTreeSet::new(),
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
                    id: "browser".into(),
                    name: "browser".into(),
                    principal: Uuid::new_v4().to_string(),
                    revision: 1,
                    enabled: true,
                    client_sessions: true,
                    commands: commands::ACTIVE_COMMANDS
                        .iter()
                        .filter(|id| {
                            !id.starts_with("access.profiles") && **id != "access.credentials.issue"
                        })
                        .map(|id| (*id).into())
                        .collect(),
                    objects: BTreeSet::new(),
                    read_roots: vec![files::path_text(&assets)?],
                    write_roots: vec![files::path_text(&exports)?],
                },
            )?;
        }
        let engine = match components {
            Some((ffmpeg, ffprobe)) => Some(Engine::new(ffmpeg, ffprobe).await?),
            None => None,
        };
        let (events, _) = broadcast::channel(128);
        let service = Arc::new(Self {
            store: Arc::new(store),
            root: selected_root,
            epoch: Uuid::new_v4().to_string(),
            engine,
            mutation: Mutex::new(()),
            credentials: Mutex::new(()),
            origins: Mutex::new(HashMap::new()),
            requests: Arc::new(Semaphore::new(16)),
            heavy: Arc::new(Semaphore::new(1)),
            pending: Arc::new(AtomicUsize::new(0)),
            cancellations: Mutex::new(HashMap::new()),
            events,
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
            && context.principal != creator
            && !profile.objects.contains(id)
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Object rights are independent of path grants",
            ));
        }
        Ok(())
    }
    fn path(&self, context: &Context, path: &Path, write: bool) -> Result<PathBuf> {
        let canonical = if write {
            files::output_path(path)?
        } else {
            files::source_path(path)?
        };
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
        if !write && context.profile.is_some() && canonical.starts_with(self.root.join("assets")) {
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
        let now = nw_platform::epoch_seconds();
        if credential.epoch != self.epoch
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
        let now = nw_platform::epoch_seconds();
        let mut live = 0;
        for (key, credential) in self.store.rows::<Credential>("credential/", 10000)? {
            if credential.epoch != self.epoch
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
                expires: now + 8 * 3600,
                last_active: now,
                audience: self.origin_generation(origin.as_deref())?,
                origin,
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
        match tokio::task::spawn_blocking(move || {
            let _work_permit = permit;
            service.dispatch_sync(context, request, cancellation)
        })
        .await
        {
            Ok(Ok(response)) => response.bounded(),
            Ok(Err(error)) => Response::error(&id, error),
            Err(error) => {
                Response::error(&id, DomainError::new("WORKER_FAILED", error.to_string()))
            }
        }
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
        let policy = self.policy(&context)?;
        let durable = matches!(descriptor.state_class, StateClass::DurableMutation);
        let _guard = if durable || matches!(descriptor.state_class, StateClass::Bootstrap) {
            Some(self.mutation.lock().map_err(|_| lock_error())?)
        } else {
            None
        };
        let now = nw_platform::epoch_seconds();
        if durable && let Some(response) = self.store.replay(&context.principal, &request, now)? {
            return Ok(response);
        }
        if let Some(profile) = &policy
            && !profile.commands.contains(&request.command.id)
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
            "commands.list" | "commands.schema" | "streams.clean.resolve" => {
                return Ok(super::CommandService.dispatch(super::Principal::LocalUser, request));
            }
            "access.replay_scope.create" => {
                let _: Empty = commands::parse(params)?;
                json!({"replay_scope":self.store.issue_scope(&context.principal, now)?,"expires_in_seconds":604800})
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
            "access.profiles.put" => {
                if policy.is_some() {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "Only the native owner can change access profiles",
                    ));
                }
                let input: ProfileInput = commands::parse(params)?;
                let key = named("profile", &input.name)?;
                if input.name == "agent" {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "The default discovery profile is immutable; create a named operating profile",
                    ));
                }
                if input.commands.len() > 128
                    || input.objects.len() > 256
                    || input.read_roots.len() > 32
                    || input.write_roots.len() > 32
                    || input.commands.iter().any(|id| {
                        commands::descriptor(id).is_none()
                            || id.starts_with("access.profiles")
                            || id == "access.credentials.issue"
                    })
                {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Profile rights exceed the admitted bounded policy",
                    ));
                }
                let old: Option<AccessProfile> = self.store.get(&key)?;
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
                    id: input.name.clone(),
                    name: input.name,
                    principal: old
                        .as_ref()
                        .map(|profile| profile.principal.clone())
                        .unwrap_or_else(|| Uuid::new_v4().to_string()),
                    revision: old.as_ref().map_or(1, |profile| profile.revision + 1),
                    enabled: input.enabled,
                    client_sessions: input.client_sessions,
                    commands: input.commands,
                    objects: input.objects,
                    read_roots: roots(input.read_roots)?,
                    write_roots: roots(input.write_roots)?,
                };
                let response = self.store.commit(
                    &context.principal,
                    &request,
                    nw_platform::epoch_seconds(),
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
                let mut asset = runtime.block_on(self.engine()?.register(
                    path,
                    context.principal.clone(),
                    cancellation.clone(),
                ))?;
                asset.display_name = display_name;
                self.path(&context, Path::new(&asset.identity.path), false)?;
                return self.store.commit(
                    &context.principal,
                    &request,
                    nw_platform::epoch_seconds(),
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
                let _: Empty = commands::parse(params)?;
                value(
                    &self
                        .store
                        .list::<Asset>("asset/", 256)?
                        .into_iter()
                        .filter(|asset| self.object(&context, &asset.id, &asset.creator).is_ok())
                        .collect::<Vec<_>>(),
                )?
            }
            "assets.output_suggest" => {
                let input: IdInput = commands::parse(params)?;
                let asset: Asset = self
                    .store
                    .get(&named("asset", &input.id)?)?
                    .ok_or_else(|| DomainError::new("OBJECT_NOT_FOUND", "Asset does not exist"))?;
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
                let extension=Path::new(&asset.identity.path).extension().and_then(|value|value.to_str()).filter(|value|value.len()<=12 && value.bytes().all(|byte|byte.is_ascii_alphanumeric())).unwrap_or("mkv");
                let output = self.path(
                    &context,
                    &parent.join(format!("{stem}-clean-{}.{extension}", Uuid::new_v4().simple())),
                    true,
                )?;
                json!({"output":files::path_text(&output)?})
            }
            "streams.clean" => {
                let input: CleanInput = commands::parse(params)?;
                let asset: Asset = self
                    .store
                    .get(&named("asset", &input.asset_id)?)?
                    .ok_or_else(|| DomainError::new("OBJECT_NOT_FOUND", "Asset does not exist"))?;
                self.object(&context, &asset.id, &asset.creator)?;
                self.path(&context, Path::new(&asset.identity.path), false)?;
                let output = self.path(&context, Path::new(&input.output), true)?;
                let plan =
                    self.engine()?
                        .clean_plan(asset, &output, &input.intent, &context.principal)?;
                self.store.put(&named("plan", &plan.id)?, &plan)?;
                value(&plan)?
            }
            "plans.inspect" => {
                let input: IdInput = commands::parse(params)?;
                let plan: Plan = self
                    .store
                    .get(&named("plan", &input.id)?)?
                    .ok_or_else(|| DomainError::new("OBJECT_NOT_FOUND", "Plan does not exist"))?;
                self.object(&context, &plan.id, &plan.creator)?;
                value(&plan)?
            }
            "jobs.submit" => {
                let input: SubmitInput = commands::parse(params)?;
                let plan: Plan = self
                    .store
                    .get(&named("plan", &input.plan_id)?)?
                    .ok_or_else(|| DomainError::new("OBJECT_NOT_FOUND", "Plan does not exist"))?;
                self.object(&context, &plan.id, &plan.creator)?;
                self.object(&context, &plan.source.id, &plan.source.creator)?;
                if plan.hash != input.plan_hash
                    || plan.creator != context.principal
                    || plan.expires_at.parse::<u64>().unwrap_or(0) <= now
                {
                    return Err(DomainError::new(
                        "INVALID_PLAN",
                        "Plan hash, creator or expiry does not match the reviewed submission",
                    ));
                }
                self.path(&context, Path::new(&plan.source.identity.path), false)?;
                self.path(&context, Path::new(&plan.output), true)?;
                files::revalidate(&plan.source.identity, &cancellation)?;
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
                    artifact: None,
                    verification: None,
                    error: None,
                };
                let authorization = ExecutionAuthorization {
                    principal: context.principal.clone(),
                    profile: context.profile,
                    revision: policy.as_ref().map_or(0, |profile| profile.revision),
                };
                let response = self.store.commit(
                    &context.principal,
                    &request,
                    nw_platform::epoch_seconds(),
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
                self.object(&context, &job.id, &job.creator)?;
                value(&job)?
            }
            "jobs.list" => {
                let _: Empty = commands::parse(params)?;
                value(
                    &self
                        .store
                        .list::<Job>("job/", 256)?
                        .into_iter()
                        .filter(|job| self.object(&context, &job.id, &job.creator).is_ok())
                        .collect::<Vec<_>>(),
                )?
            }
            "jobs.cancel" => {
                let input: IdInput = commands::parse(params)?;
                let mut job = self.store.inspect_job(&input.id)?;
                self.object(&context, &job.id, &job.creator)?;
                if job.state == JobState::Publishing {
                    return Err(DomainError::new(
                        "CANCELLATION_TOO_LATE",
                        "The durable publication fence has committed",
                    ));
                }
                if job.state == JobState::Queued {
                    job.state = JobState::Cancelled;
                }
                let response = self.store.commit(&context.principal, &request, nw_platform::epoch_seconds(), json!({"job_id":job.id,"state":job.state,"cancel_requested":!job.state.terminal()}), &[(named("cancel", &job.id)?, json!(true)), (named("job", &job.id)?, value(&job)?)])?;
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
    fn execution_event(&self, id: &str, event: ExecutionEvent) -> Result<()> {
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        match event {
            ExecutionEvent::Prepared(argv) => {
                self.store.execution_allowed(id)?;
                self.store.update_job(id, |job| {
                    job.prepared_argv = Some(argv);
                    Ok(())
                })?;
            }
            ExecutionEvent::Verifying => {
                self.store.execution_allowed(id)?;
                self.store
                    .transition_job(id, JobState::Running, JobState::Verifying)?;
            }
            ExecutionEvent::Fence(publication) => self.store.publication_fence(&publication)?,
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
                std::fs::remove_file(publication.proof).map_err(files::io_error)?;
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
            json!({"name":"operator","expected_revision":null,"enabled":true,"commands":["commands.list"],"objects":[],"read_roots":[],"write_roots":[]}),
        );
        create.replay_scope = Some(scope.clone());
        create.idempotency_key = Some("stable-create".into());
        let accepted = service.dispatch(owner.clone(), create.clone()).await;
        assert!(accepted.ok, "{:?}", accepted.error);
        let token = service.credential("operator").unwrap();
        let principal = service.authenticate(&token).unwrap().principal;
        let changed = mutate(&service, &owner, &scope, "access.profiles.put", json!({"name":"operator","expected_revision":1,"enabled":false,"commands":[],"objects":[],"read_roots":[],"write_roots":[]})).await;
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
        let asset = Asset {
            display_name: String::new(),
            id: "owned-asset".into(),
            creator: owner.principal.clone(),
            identity: files::identity(&source, &CancellationToken::new()).unwrap(),
            probe: nw_domain::records::MediaProbe {
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
        let profile = json!({"name":"reader","expected_revision":null,"enabled":true,"commands":["assets.list","streams.clean"],"objects":[],"read_roots":[root.path().to_str().unwrap()],"write_roots":[root.path().to_str().unwrap()]});
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
                .unwrap(),
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
        assert_eq!(
            service
                .dispatch(
                    reader.clone(),
                    Request::query("streams.clean", params.clone())
                )
                .await
                .error
                .unwrap()
                .code,
            "PERMISSION_DENIED"
        );
        assert!(mutate(&service, &owner, &scope, "access.profiles.put", json!({"name":"reader","expected_revision":1,"enabled":true,"commands":["streams.clean"],"objects":["owned-asset"],"read_roots":[],"write_roots":[]})).await.ok);
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
}
