//! Native adapters reconnect to the selected owner without lending owner authority to an agent.
mod download;
use anyhow::{Context as _, Result};
use nw_domain::commands::{Request, Response};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::Mutex};

const OWNER_STARTUP_FRAME_LIMIT: usize = 512;

// Only the owned __owner child's stdout carries this failure-only private frame.
// These finite fields cannot carry error text or any owner/request identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OwnerStartupStage {
    OwnerLogActivation,
    PostLogActivation,
}
impl OwnerStartupStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::OwnerLogActivation => "owner_log_activation",
            Self::PostLogActivation => "post_log_activation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum OwnerStartupCode {
    OwnerBusy,
    LogFileUnowned,
    OwnerIdentityConflict,
    FilesystemError,
    LogStorageError,
    InsecureDataRoot,
    OtherDomain,
    AdapterError,
}
impl OwnerStartupCode {
    fn from_error(error: &anyhow::Error) -> Self {
        match error.downcast_ref::<nw_domain::DomainError>() {
            Some(error) => match error.code.as_str() {
                "OWNER_BUSY" => Self::OwnerBusy,
                "LOG_FILE_UNOWNED" => Self::LogFileUnowned,
                "OWNER_IDENTITY_CONFLICT" => Self::OwnerIdentityConflict,
                "FILESYSTEM_ERROR" => Self::FilesystemError,
                "LOG_STORAGE_ERROR" => Self::LogStorageError,
                "INSECURE_DATA_ROOT" => Self::InsecureDataRoot,
                _ => Self::OtherDomain,
            },
            None => Self::AdapterError,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::OwnerBusy => "OWNER_BUSY",
            Self::LogFileUnowned => "LOG_FILE_UNOWNED",
            Self::OwnerIdentityConflict => "OWNER_IDENTITY_CONFLICT",
            Self::FilesystemError => "FILESYSTEM_ERROR",
            Self::LogStorageError => "LOG_STORAGE_ERROR",
            Self::InsecureDataRoot => "INSECURE_DATA_ROOT",
            Self::OtherDomain => "OTHER_DOMAIN",
            Self::AdapterError => "ADAPTER_ERROR",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerStartupDiagnostic {
    schema_version: u8,
    stage: OwnerStartupStage,
    code: OwnerStartupCode,
}
impl OwnerStartupDiagnostic {
    fn frame(self) -> Option<Vec<u8>> {
        if self.schema_version != 1 {
            return None;
        }
        let mut bytes = serde_json::to_vec(&self).ok()?;
        bytes.push(b'\n');
        (bytes.len() <= OWNER_STARTUP_FRAME_LIMIT).then_some(bytes)
    }
}

pub(crate) fn emit_owner_startup_failure(stage: OwnerStartupStage, error: &anyhow::Error) {
    let diagnostic = OwnerStartupDiagnostic {
        schema_version: 1,
        stage,
        code: OwnerStartupCode::from_error(error),
    };
    if let Some(bytes) = diagnostic.frame() {
        use std::io::Write;
        let _ = std::io::stdout().lock().write_all(&bytes);
    }
}

enum OwnerStartupObservation {
    Missing,
    Overflow,
    Malformed,
    Record(OwnerStartupDiagnostic),
}
impl OwnerStartupObservation {
    fn from_frame(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::Missing;
        }
        if bytes.len() > OWNER_STARTUP_FRAME_LIMIT {
            return Self::Overflow;
        }
        let Ok(record) = serde_json::from_slice::<OwnerStartupDiagnostic>(bytes) else {
            return Self::Malformed;
        };
        if record.frame().as_deref() != Some(bytes) {
            return Self::Malformed;
        }
        Self::Record(record)
    }
    fn record(&self) -> Option<&OwnerStartupDiagnostic> {
        match self {
            Self::Record(record) => Some(record),
            _ => None,
        }
    }
}

async fn collect_owner_startup_failure(
    stdout: Option<tokio::process::ChildStdout>,
    attach_started: std::time::Instant,
) -> OwnerStartupObservation {
    read_owner_startup_failure(stdout, attach_started).await
}

async fn read_owner_startup_failure<R: tokio::io::AsyncRead + Unpin>(
    stdout: Option<R>,
    attach_started: std::time::Instant,
) -> OwnerStartupObservation {
    let read_started = tokio::time::Instant::now();
    let deadline = tokio::time::Instant::from_std(attach_started + Duration::from_secs(20))
        .min(read_started + Duration::from_millis(50));
    let Some(stdout) = stdout else {
        return OwnerStartupObservation::Missing;
    };
    if read_started >= deadline {
        return OwnerStartupObservation::Missing;
    }
    // The wait has already reaped our child. A descendant retaining the writer
    // cannot keep this read alive or turn a complete-looking prefix into a record.
    let mut reader = stdout.take((OWNER_STARTUP_FRAME_LIMIT + 1) as u64);
    let mut bytes = Vec::with_capacity(OWNER_STARTUP_FRAME_LIMIT + 1);
    match tokio::time::timeout_at(deadline, reader.read_to_end(&mut bytes)).await {
        Ok(Ok(_)) => OwnerStartupObservation::from_frame(&bytes),
        _ => OwnerStartupObservation::Missing,
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OwnerRecord {
    pub canonical_root: String,
    pub root_id: String,
    pub epoch: String,
    pub build: String,
    pub registry: String,
    pub port: u16,
    pub bearer: String,
}
#[derive(Clone)]
pub struct NativeClient {
    pub root: PathBuf,
    pub components: Option<PathBuf>,
    pub profile: Option<String>,
    http: reqwest::Client,
    cached: Arc<Mutex<Option<OwnerRecord>>>,
    credential: Arc<CredentialSession>,
    binding: Arc<Mutex<SessionBinding>>,
    build: String,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClientLifetime {
    LongLived,
    OneShot,
}
struct CredentialSession {
    lifetime: ClientLifetime,
    closed: AtomicBool,
    state: Mutex<CredentialState>,
}
impl CredentialSession {
    fn new(lifetime: ClientLifetime) -> Self {
        Self {
            lifetime,
            closed: AtomicBool::new(false),
            state: Mutex::new(CredentialState::default()),
        }
    }
    fn ensure_open(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(nw_domain::DomainError::new(
                "CLIENT_CLOSED",
                "The one-shot native operation has finished",
            )
            .into());
        }
        Ok(())
    }
    fn close(&self) -> bool {
        self.lifetime == ClientLifetime::OneShot && !self.closed.swap(true, Ordering::AcqRel)
    }
}
#[derive(Clone, PartialEq, Eq)]
struct CredentialIssuer {
    root_id: String,
    port: u16,
    epoch: String,
}
impl CredentialIssuer {
    fn from_owner(record: &OwnerRecord) -> Self {
        Self {
            root_id: record.root_id.clone(),
            port: record.port,
            epoch: record.epoch.clone(),
        }
    }
}
// Never retain the bootstrap owner's bearer in a scoped cleanup candidate.
#[derive(Clone, PartialEq, Eq)]
struct ScopedCredential {
    issuer: CredentialIssuer,
    profile: String,
    principal: String,
    token: String,
}
#[derive(Default)]
struct CredentialState {
    cached: Option<ScopedCredential>,
    owned: Vec<ScopedCredential>,
}
impl CredentialState {
    fn prepare_issuance(
        &mut self,
        issuer: &CredentialIssuer,
        profile: &str,
        principal: Option<&str>,
    ) -> Result<()> {
        // Reject identity changes before pruning entries from a verified old epoch.
        for candidate in &self.owned {
            if candidate.issuer.root_id != issuer.root_id || candidate.profile != profile {
                return Err(nw_domain::DomainError::new(
                    "OWNER_IDENTITY_CONFLICT",
                    "Scoped issuance changed its original root or profile",
                )
                .into());
            }
            if principal != Some(candidate.principal.as_str()) {
                return Err(nw_domain::DomainError::new(
                    "PRINCIPAL_CHANGED",
                    "Scoped issuance changed its original principal",
                )
                .into());
            }
        }
        self.owned
            .retain(|candidate| candidate.issuer.epoch == issuer.epoch);
        if self.owned.len() >= 32 {
            return Err(nw_domain::DomainError::new(
                "RESOURCE_LIMIT",
                "One-shot credential ownership budget reached",
            )
            .into());
        }
        Ok(())
    }
    fn record_issued(&mut self, candidate: ScopedCredential, lifetime: ClientLifetime) {
        if lifetime == ClientLifetime::OneShot && !self.owned.contains(&candidate) {
            self.owned.push(candidate.clone());
        }
        self.cached = Some(candidate);
    }
    fn evict_cached(&mut self) {
        self.cached = None;
    }
    fn take_owned_current_first(&mut self) -> Vec<ScopedCredential> {
        let mut candidates = std::mem::take(&mut self.owned);
        candidates.reverse();
        if let Some(current) = self.cached.take()
            && let Some(index) = candidates
                .iter()
                .position(|candidate| candidate == &current)
        {
            candidates.swap(0, index);
        }
        candidates
    }
}
pub(crate) struct FinishSummary {
    pub code: &'static str,
    pub attempted: usize,
    pub released: usize,
    pub remaining: Option<usize>,
    pub elapsed_ms: u128,
}
#[derive(Default)]
struct SessionBinding {
    root_id: Option<String>,
    principal: Option<String>,
}
impl SessionBinding {
    fn bind_root(&mut self, root: &str) -> Result<()> {
        if self
            .root_id
            .as_deref()
            .is_some_and(|original| original != root)
        {
            return Err(nw_domain::DomainError::new(
                "OWNER_IDENTITY_CONFLICT",
                "The selected native root no longer has this adapter's original data identity",
            )
            .into());
        }
        self.root_id = Some(root.into());
        Ok(())
    }
    fn bind_principal(&mut self, principal: &str) -> Result<()> {
        if principal.is_empty() || principal.len() > 128 {
            return Err(nw_domain::DomainError::new(
                "OWNER_IDENTITY_CONFLICT",
                "Scoped bootstrap omitted its bounded principal identity",
            )
            .into());
        }
        if self
            .principal
            .as_deref()
            .is_some_and(|original| original != principal)
        {
            return Err(nw_domain::DomainError::new(
                "PRINCIPAL_CHANGED",
                "Scoped reconnect cannot change the adapter's original principal",
            )
            .into());
        }
        self.principal = Some(principal.into());
        Ok(())
    }
}
pub fn registry_fingerprint() -> String {
    nw_domain::commands::registry_fingerprint()
}
impl NativeClient {
    pub async fn new(
        root: PathBuf,
        components: Option<PathBuf>,
        profile: Option<String>,
    ) -> Result<Self> {
        Self::new_with_lifetime(root, components, profile, ClientLifetime::LongLived).await
    }
    pub async fn new_one_shot(
        root: PathBuf,
        components: Option<PathBuf>,
        profile: Option<String>,
    ) -> Result<Self> {
        Self::new_with_lifetime(root, components, profile, ClientLifetime::OneShot).await
    }
    async fn new_with_lifetime(
        root: PathBuf,
        components: Option<PathBuf>,
        profile: Option<String>,
        lifetime: ClientLifetime,
    ) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(root)?;
        let exe = std::env::current_exe()?;
        let identity_started = std::time::Instant::now();
        let identity = tokio::task::spawn_blocking(move || {
            nw_platform::files::identity(&exe, &tokio_util::sync::CancellationToken::new())
        })
        .await??;
        tracing::info!(elapsed_ms=identity_started.elapsed().as_millis(), executable_bytes=%identity.bytes, "Executable identity read completed");
        Self::from_identity(root, components, profile, identity.sha256, lifetime)
    }
    fn from_identity(
        root: PathBuf,
        components: Option<PathBuf>,
        profile: Option<String>,
        build: String,
        lifetime: ClientLifetime,
    ) -> Result<Self> {
        Ok(Self {
            root,
            components,
            profile,
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(130))
                .build()?,
            cached: Arc::new(Mutex::new(None)),
            credential: Arc::new(CredentialSession::new(lifetime)),
            binding: Arc::new(Mutex::new(SessionBinding::default())),
            build,
        })
    }
    #[cfg(test)]
    pub(crate) fn for_test_owner(
        record: OwnerRecord,
        profile: Option<String>,
        one_shot: bool,
    ) -> Result<Self> {
        let mut client = Self::from_identity(
            PathBuf::from(&record.canonical_root),
            None,
            profile,
            record.build.clone(),
            if one_shot {
                ClientLifetime::OneShot
            } else {
                ClientLifetime::LongLived
            },
        )?;
        let mut binding = SessionBinding::default();
        binding.bind_root(&record.root_id)?;
        client.binding = Arc::new(Mutex::new(binding));
        client.cached = Arc::new(Mutex::new(Some(record)));
        Ok(client)
    }
    #[cfg(test)]
    pub(crate) async fn has_cached_credential(&self) -> bool {
        self.credential.state.lock().await.cached.is_some()
    }
    pub async fn attach(&self) -> Result<OwnerRecord> {
        self.credential.ensure_open()?;
        if let Some(record) = self.cached.lock().await.clone() {
            self.credential.ensure_open()?;
            return Ok(record);
        }
        self.credential.ensure_open()?;
        let started = std::time::Instant::now();
        let runtime_root = self.root.join("runtime");
        let runtime_root = tokio::task::spawn_blocking(move || {
            nw_platform::security::private_directory(&runtime_root)
        })
        .await??;
        self.credential.ensure_open()?;
        let runtime = runtime_root.join("owner.json");
        let mut bootstrap_gate = None;
        let mut spawned = false;
        loop {
            self.credential.ensure_open()?;
            if runtime.try_exists()? {
                let path = runtime.clone();
                let bytes = tokio::task::spawn_blocking(move || {
                    nw_platform::security::read_private(&path, 16384)
                })
                .await??;
                let record: OwnerRecord = serde_json::from_slice(&bytes)?;
                if record.canonical_root != nw_platform::files::path_text(&self.root)? {
                    return Err(nw_domain::DomainError::new(
                        "OWNER_IDENTITY_CONFLICT",
                        "Runtime record names another root",
                    )
                    .into());
                }
                let endpoint = format!("http://127.0.0.1:{}", record.port);
                let health = self
                    .http
                    .get(format!("{endpoint}/health"))
                    .bearer_auth(&record.bearer)
                    .timeout(Duration::from_millis(750))
                    .send()
                    .await;
                if let Ok(health) = health
                    && health.status().is_success()
                {
                    let identity = bounded_json(health).await?;
                    if identity["epoch"] != record.epoch
                        || identity["root_id"] != record.root_id
                        || identity["build"] != record.build
                        || identity["registry"] != record.registry
                    {
                        return Err(nw_domain::DomainError::new(
                            "OWNER_IDENTITY_CONFLICT",
                            "Protected bootstrap and active owner differ",
                        )
                        .into());
                    }
                    if record.build != self.build || record.registry != registry_fingerprint() {
                        return Err(nw_domain::DomainError::new("OWNER_VERSION_CONFLICT", "Another build owns this root; let its jobs/clients drain before updating").into());
                    }
                    self.binding.lock().await.bind_root(&record.root_id)?;
                    self.credential.ensure_open()?;
                    *self.cached.lock().await = Some(record.clone());
                    tracing::info!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "Protected owner attachment completed"
                    );
                    return Ok(record);
                }
            }
            if bootstrap_gate.is_none() {
                match nw_platform::files::RootLock::acquire(&runtime_root) {
                    Ok(gate) => bootstrap_gate = Some(gate),
                    Err(error) if error.code == "OWNER_BUSY" => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let state_root = self.root.join("state");
            let lock_root = nw_store::generations::active_generation_path(&self.root)?;
            let may_spawn = bootstrap_gate.is_some()
                && (!state_root.try_exists()?
                    || nw_platform::files::RootLock::acquire(&state_root).is_ok())
                && (!lock_root.try_exists()?
                    || nw_platform::files::RootLock::acquire(&lock_root).is_ok());
            if may_spawn && !spawned {
                self.credential.ensure_open()?;
                #[cfg(windows)]
                nw_platform::process::prevent_stdio_inheritance()?;
                let mut command = tokio::process::Command::new(std::env::current_exe()?);
                command.arg("--data-dir").arg(&self.root);
                if let Some(components) = &self.components {
                    command.arg("--components-dir").arg(components);
                }
                command
                    .arg("__owner")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(false);
                #[cfg(unix)]
                command.process_group(0);
                #[cfg(windows)]
                command.creation_flags(0x00000008 | 0x00000200);
                let mut child = command
                    .spawn()
                    .context("Cannot start the protected owner")?;
                let stdout = child.stdout.take();
                tracing::info!(process_id = child.id(), "Protected owner child spawned");
                tokio::spawn(async move {
                    match child.wait().await {
                        Ok(status) if status.success() => {
                            tracing::debug!("Protected owner child exited normally")
                        }
                        Ok(status) => {
                            let diagnostic = collect_owner_startup_failure(stdout, started).await;
                            tracing::warn!(
                                exit_code = status.code(),
                                startup_stage =
                                    diagnostic.record().map(|record| record.stage.as_str()),
                                startup_code =
                                    diagnostic.record().map(|record| record.code.as_str()),
                                diagnostic_missing =
                                    matches!(diagnostic, OwnerStartupObservation::Missing),
                                diagnostic_overflow =
                                    matches!(diagnostic, OwnerStartupObservation::Overflow),
                                diagnostic_malformed =
                                    matches!(diagnostic, OwnerStartupObservation::Malformed),
                                "Protected owner child exited with an error"
                            );
                        }
                        Err(_) => {
                            tracing::warn!("Protected owner child status could not be collected")
                        }
                    }
                });
                // Retain the per-root gate through readiness; never launch another child
                // while this attempt may still be initializing its log/store ownership.
                spawned = true;
            }
            if started.elapsed() > Duration::from_secs(20) {
                return Err(nw_domain::DomainError::new(
                    "OWNER_START_TIMEOUT",
                    "Owner is busy, recovering, or cannot initialize the protected root",
                )
                .into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn scoped_token(&self, record: &OwnerRecord) -> Result<String> {
        self.credential.ensure_open()?;
        let Some(profile) = &self.profile else {
            return Ok(record.bearer.clone());
        };
        let mut state = self.credential.state.lock().await;
        self.credential.ensure_open()?;
        let issuer = CredentialIssuer::from_owner(record);
        let expected_principal = self.binding.lock().await.principal.clone();
        if let Some(cached) = &state.cached
            && cached.issuer == issuer
            && cached.profile == *profile
            && Some(cached.principal.as_str()) == expected_principal.as_deref()
        {
            return Ok(cached.token.clone());
        }
        state.prepare_issuance(&issuer, profile, expected_principal.as_deref())?;
        self.credential.ensure_open()?;
        let response = self
            .http
            .post(format!(
                "http://127.0.0.1:{}/internal/credential",
                record.port
            ))
            .bearer_auth(&record.bearer)
            .json(&serde_json::json!({"profile":profile,"expected_principal":expected_principal}))
            .send()
            .await?;
        let value = bounded_json(response).await?;
        let token = value["bearer"]
            .as_str()
            .filter(|token| !token.is_empty() && token.len() <= 256)
            .context("Scoped credential was not issued")?
            .to_owned();
        if value["profile"].as_str() != Some(profile.as_str()) {
            return Err(nw_domain::DomainError::new(
                "OWNER_IDENTITY_CONFLICT",
                "Scoped bootstrap names a different profile",
            )
            .into());
        }
        let principal = value["principal"].as_str().unwrap_or("");
        self.binding.lock().await.bind_principal(principal)?;
        state.record_issued(
            ScopedCredential {
                issuer,
                profile: profile.clone(),
                principal: principal.to_owned(),
                token: token.clone(),
            },
            self.credential.lifetime,
        );
        self.credential.ensure_open()?;
        Ok(token)
    }
    async fn evict_cached(&self) {
        *self.cached.lock().await = None;
        self.credential.state.lock().await.evict_cached();
    }
    pub async fn request(&self, request: &Request) -> Result<Response> {
        Ok(serde_json::from_value(
            self.scoped_post("api/v1/commands", &serde_json::to_value(request)?)
                .await?,
        )?)
    }
    #[cfg(feature = "desktop")]
    pub async fn client_lease(&self, id: &str, release: bool) -> Result<()> {
        self.scoped_post(
            "api/v1/lease",
            &serde_json::json!({"id":id,"release":release}),
        )
        .await?;
        Ok(())
    }
    async fn scoped_post(
        &self,
        route: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        for attempt in 0..2 {
            let record = self.attach().await?;
            let token = match self.scoped_token(&record).await {
                Ok(token) => token,
                Err(error)
                    if attempt == 0
                        && (error.downcast_ref::<reqwest::Error>().is_some()
                            || error
                                .downcast_ref::<nw_domain::DomainError>()
                                .is_some_and(|error| error.code == "AUTHENTICATION_REQUIRED")) =>
                {
                    self.evict_cached().await;
                    continue;
                }
                Err(error) => {
                    if error
                        .downcast_ref::<nw_domain::DomainError>()
                        .is_some_and(|error| error.code == "AUTHENTICATION_REQUIRED")
                    {
                        self.evict_cached().await;
                    }
                    return Err(error);
                }
            };
            let result = self
                .http
                .post(format!("http://127.0.0.1:{}/{route}", record.port))
                .bearer_auth(token)
                .json(body)
                .send()
                .await;
            match result {
                Ok(response) if response.status().is_success() => {
                    return bounded_json(response).await;
                }
                Ok(response)
                    if (response.status() == reqwest::StatusCode::UNAUTHORIZED
                        || response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE)
                        && attempt == 0 => {}
                Ok(response) => {
                    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                        self.evict_cached().await;
                    }
                    bounded_json(response).await?;
                    return Err(nw_domain::DomainError::new(
                        "TRANSPORT_ERROR",
                        "Unexpected owner response",
                    )
                    .into());
                }
                Err(error) if attempt != 0 => return Err(error.into()),
                Err(_) => {}
            }
            self.evict_cached().await;
        }
        Err(nw_domain::DomainError::new(
            "TRANSPORT_ERROR",
            "Owner reconnect failed; original durable request identity was retained",
        )
        .into())
    }
    pub(crate) async fn finish(&self) -> FinishSummary {
        let started = tokio::time::Instant::now();
        let deadline = started + Duration::from_secs(1);
        let mut summary = FinishSummary {
            code: "complete",
            attempted: 0,
            released: 0,
            remaining: Some(0),
            elapsed_ms: 0,
        };
        if !self.credential.close() {
            return summary;
        }
        summary.remaining = None;
        let result = tokio::time::timeout_at(deadline, async {
            let candidates = self
                .credential
                .state
                .lock()
                .await
                .take_owned_current_first();
            summary.remaining = Some(candidates.len());
            for candidate in candidates {
                summary.attempted += 1;
                match self.release_candidate(candidate, deadline).await {
                    Ok(()) => {
                        summary.released += 1;
                        summary.remaining = summary.remaining.map(|count| count - 1);
                    }
                    Err("deadline") => {
                        summary.code = "deadline";
                        break;
                    }
                    Err(code) if summary.code == "complete" => summary.code = code,
                    Err(_) => {}
                }
            }
        })
        .await;
        if result.is_err() {
            summary.code = "deadline";
        }
        summary.elapsed_ms = started.elapsed().as_millis();
        summary
    }
    async fn release_candidate(
        &self,
        candidate: ScopedCredential,
        deadline: tokio::time::Instant,
    ) -> std::result::Result<(), &'static str> {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or("deadline")?;
        let mut response = self
            .http
            .post(format!(
                "http://127.0.0.1:{}/internal/credential/release",
                candidate.issuer.port
            ))
            .bearer_auth(&candidate.token)
            .timeout(remaining)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "deadline"
                } else {
                    "transport"
                }
            })?;
        if !response.status().is_success() {
            return Err(if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                "authentication"
            } else {
                "denied"
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                "deadline"
            } else {
                "transport"
            }
        })? {
            if bytes.len().saturating_add(chunk.len()) > 64 {
                return Err("acknowledgement");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = nw_domain::commands::decode_json(&bytes).map_err(|_| "acknowledgement")?;
        if value != serde_json::json!({"released":true}) {
            return Err("acknowledgement");
        }
        Ok(())
    }
    pub async fn internal(
        &self,
        route: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if self.profile.is_some() {
            return Err(nw_domain::DomainError::new(
                "PERMISSION_DENIED",
                "Internal owner controls are not available to a scoped adapter",
            )
            .into());
        }
        self.scoped_post(route, &body).await
    }
}
pub async fn bounded_json(mut response: reqwest::Response) -> Result<serde_json::Value> {
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > 256 * 1024 {
            return Err(
                nw_domain::DomainError::new("RESOURCE_LIMIT", "Response exceeds 256 KiB").into(),
            );
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = nw_domain::commands::decode_json(&bytes).map_err(|_| {
        nw_domain::DomainError::new("TRANSPORT_ERROR", "Owner returned invalid bounded JSON")
    })?;
    if !status.is_success() {
        let code = value["error"]["code"].as_str().unwrap_or("TRANSPORT_ERROR");
        let message = value["error"]["message"]
            .as_str()
            .unwrap_or("Owner rejected the request");
        return Err(nw_domain::DomainError::new(code, message).into());
    }
    Ok(value)
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    #[test]
    fn reconnect_keeps_root_and_principal_anchors_after_cache_eviction() {
        let mut binding = SessionBinding::default();
        binding.bind_root("root-original").unwrap();
        binding.bind_principal("principal-original").unwrap();
        binding.bind_root("root-original").unwrap();
        binding.bind_principal("principal-original").unwrap();
        assert!(binding.bind_root("root-replacement").is_err());
        assert!(binding.bind_principal("principal-replacement").is_err());
        assert_eq!(binding.root_id.as_deref(), Some("root-original"));
        assert_eq!(binding.principal.as_deref(), Some("principal-original"));
        credential_lifecycle_bookkeeping();

        // The private startup channel accepts only finite canonical frames, even
        // when an error's code, context or message contains arbitrary content.
        let known_codes = [
            ("OWNER_BUSY", OwnerStartupCode::OwnerBusy),
            ("LOG_FILE_UNOWNED", OwnerStartupCode::LogFileUnowned),
            (
                "OWNER_IDENTITY_CONFLICT",
                OwnerStartupCode::OwnerIdentityConflict,
            ),
            ("FILESYSTEM_ERROR", OwnerStartupCode::FilesystemError),
            ("LOG_STORAGE_ERROR", OwnerStartupCode::LogStorageError),
            ("INSECURE_DATA_ROOT", OwnerStartupCode::InsecureDataRoot),
        ];
        for (code, expected) in known_codes {
            let error = anyhow::Error::from(nw_domain::DomainError::new(
                code,
                "private error message must never enter the frame",
            ))
            .context("private error context must never enter the frame");
            assert_eq!(OwnerStartupCode::from_error(&error), expected);
            assert_eq!(expected.as_str(), code);
        }
        for code in ["UNLISTED_PRIVATE_CODE", "ADAPTER_ERROR", "OTHER_DOMAIN", ""] {
            let error = anyhow::Error::from(nw_domain::DomainError::new(code, "private message"));
            assert_eq!(
                OwnerStartupCode::from_error(&error),
                OwnerStartupCode::OtherDomain
            );
        }
        assert_eq!(
            OwnerStartupCode::from_error(&anyhow::anyhow!("private adapter message")),
            OwnerStartupCode::AdapterError
        );
        for stage in [
            OwnerStartupStage::OwnerLogActivation,
            OwnerStartupStage::PostLogActivation,
        ] {
            for code in [
                OwnerStartupCode::OwnerBusy,
                OwnerStartupCode::LogFileUnowned,
                OwnerStartupCode::OwnerIdentityConflict,
                OwnerStartupCode::FilesystemError,
                OwnerStartupCode::LogStorageError,
                OwnerStartupCode::InsecureDataRoot,
                OwnerStartupCode::OtherDomain,
                OwnerStartupCode::AdapterError,
            ] {
                let record = OwnerStartupDiagnostic {
                    schema_version: 1,
                    stage,
                    code,
                };
                let bytes = record.frame().unwrap();
                assert_eq!(
                    bytes,
                    format!(
                        "{{\"schema_version\":1,\"stage\":\"{}\",\"code\":\"{}\"}}\n",
                        stage.as_str(),
                        code.as_str()
                    )
                    .into_bytes()
                );
                assert!(bytes.len() <= OWNER_STARTUP_FRAME_LIMIT);
                assert_eq!(
                    OwnerStartupObservation::from_frame(&bytes).record(),
                    Some(&record)
                );
            }
        }
        let canonical =
            b"{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n";
        for invalid in [
            "{\"schema_version\":0,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":2,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":256,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"unknown\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"UNLISTED_PRIVATE_CODE\"}\n",
            "{\"schema_version\":1,\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"stage\":\"post_log_activation\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\",\"message\":\"private\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\"}\n",
            "{\"schema_version\":1,\"stage\":null,\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n{}\n",
            "{\"code\":\"OWNER_BUSY\",\"stage\":\"owner_log_activation\",\"schema_version\":1}\n",
            "{ \"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER_BUSY\"}\n",
            "{\"schema_version\":1,\"stage\":\"owner_log_activation\",\"code\":\"OWNER\\u005fBUSY\"}\n",
            "[]\n",
            "not JSON\n",
        ] {
            assert!(matches!(
                OwnerStartupObservation::from_frame(invalid.as_bytes()),
                OwnerStartupObservation::Malformed
            ));
        }
        assert!(matches!(
            OwnerStartupObservation::from_frame(&[0xff, b'\n']),
            OwnerStartupObservation::Malformed
        ));
        assert!(matches!(
            OwnerStartupObservation::from_frame(&canonical[..canonical.len() - 1]),
            OwnerStartupObservation::Malformed
        ));
        for suffix in [b"\n".as_slice(), b" ".as_slice(), b"{}\n".as_slice()] {
            let mut noncanonical = canonical.to_vec();
            noncanonical.extend_from_slice(suffix);
            assert!(matches!(
                OwnerStartupObservation::from_frame(&noncanonical),
                OwnerStartupObservation::Malformed
            ));
        }
        let mut at_limit = canonical.to_vec();
        at_limit.resize(OWNER_STARTUP_FRAME_LIMIT, b' ');
        assert!(matches!(
            OwnerStartupObservation::from_frame(&at_limit),
            OwnerStartupObservation::Malformed
        ));
        at_limit.push(b' ');
        assert_eq!(at_limit.len(), 513);
        assert!(matches!(
            OwnerStartupObservation::from_frame(&at_limit),
            OwnerStartupObservation::Overflow
        ));
        assert!(matches!(
            OwnerStartupObservation::from_frame(&[]),
            OwnerStartupObservation::Missing
        ));

        // MOCK in-process I/O coverage of the production reader; this does not
        // prove actual child reaping or platform standard-handle inheritance.
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let expected = OwnerStartupDiagnostic {
                        schema_version: 1,
                        stage: OwnerStartupStage::OwnerLogActivation,
                        code: OwnerStartupCode::OwnerBusy,
                    };
                    let closed_valid =
                        mock_owner_startup_io(canonical, true, std::time::Instant::now()).await;
                    assert_eq!(closed_valid.record(), Some(&expected));

                    // A valid prefix cannot make a 513-byte stream a record,
                    // even when its writer is still open when the limit is hit.
                    assert!(matches!(
                        mock_owner_startup_io(&at_limit, false, std::time::Instant::now()).await,
                        OwnerStartupObservation::Overflow
                    ));
                    // The writer remains alive across collection. Only the
                    // production 50-ms read deadline can complete this case.
                    assert!(matches!(
                        mock_owner_startup_io(canonical, false, std::time::Instant::now()).await,
                        OwnerStartupObservation::Missing
                    ));
                    assert!(matches!(
                        mock_owner_startup_io(&[], true, std::time::Instant::now()).await,
                        OwnerStartupObservation::Missing
                    ));
                    assert!(matches!(
                        mock_owner_startup_io(b"not JSON\n", true, std::time::Instant::now()).await,
                        OwnerStartupObservation::Malformed
                    ));
                    let expired_attach = std::time::Instant::now()
                        .checked_sub(Duration::from_secs(21))
                        .expect("mock attach start must precede the original horizon");
                    assert!(matches!(
                        mock_owner_startup_io(canonical, true, expired_attach).await,
                        OwnerStartupObservation::Missing
                    ));
                    assert!(matches!(
                        collect_owner_startup_failure(None, std::time::Instant::now()).await,
                        OwnerStartupObservation::Missing
                    ));
                    let client = NativeClient::from_identity(
                        PathBuf::from("unused-lifecycle-lock-fixture"),
                        None,
                        Some("agent".into()),
                        "fixture-build".into(),
                        ClientLifetime::OneShot,
                    )
                    .unwrap();
                    let held = client.credential.state.lock().await;
                    let cleanup = client.finish().await;
                    assert_eq!(cleanup.code, "deadline");
                    assert_eq!(cleanup.attempted, 0);
                    assert_eq!(cleanup.remaining, None);
                    assert!(client.credential.ensure_open().is_err());
                    drop(held);
                    assert_eq!(client.finish().await.attempted, 0);
                    assert!(client.attach().await.is_err());
                })
                .await
                .expect("MOCK bounded startup reader exceeded its outer watchdog");
            });
    }

    fn credential_lifecycle_bookkeeping() {
        let issuer = CredentialIssuer {
            root_id: "root-original".into(),
            port: 1578,
            epoch: "epoch-original".into(),
        };
        let candidate = |ordinal| ScopedCredential {
            issuer: issuer.clone(),
            profile: "agent".into(),
            principal: "principal-original".into(),
            token: format!("local-bookkeeping-{ordinal}"),
        };
        let mut state = CredentialState::default();
        for ordinal in 0..32 {
            state
                .prepare_issuance(&issuer, "agent", Some("principal-original"))
                .unwrap();
            state.record_issued(candidate(ordinal), ClientLifetime::OneShot);
        }
        assert_eq!(state.owned.len(), 32);
        state.record_issued(candidate(31), ClientLifetime::OneShot);
        assert_eq!(state.owned.len(), 32);
        assert!(
            state
                .prepare_issuance(&issuer, "agent", Some("principal-original"))
                .is_err()
        );
        state.evict_cached();
        assert!(state.cached.is_none());
        assert_eq!(state.owned.len(), 32);
        let mut next = issuer.clone();
        next.epoch = "epoch-next".into();
        next.root_id = "root-replacement".into();
        assert!(
            state
                .prepare_issuance(&next, "agent", Some("principal-original"))
                .is_err()
        );
        assert_eq!(state.owned.len(), 32);
        next.root_id = issuer.root_id.clone();
        assert!(
            state
                .prepare_issuance(&next, "agent", Some("principal-replacement"))
                .is_err()
        );
        assert!(
            state
                .prepare_issuance(&next, "another-profile", Some("principal-original"))
                .is_err()
        );
        assert_eq!(state.owned.len(), 32);
        state
            .prepare_issuance(&next, "agent", Some("principal-original"))
            .unwrap();
        assert!(state.owned.is_empty());
        for ordinal in 0..3 {
            state.record_issued(candidate(ordinal), ClientLifetime::OneShot);
        }
        state.record_issued(candidate(0), ClientLifetime::OneShot);
        let drained = state.take_owned_current_first();
        assert_eq!(drained.len(), 3);
        assert!(drained[0] == candidate(0));
        assert!(state.owned.is_empty() && state.cached.is_none());
        assert!(state.take_owned_current_first().is_empty());
        state.record_issued(candidate(0), ClientLifetime::LongLived);
        assert!(state.owned.is_empty());
        assert!(state.cached.is_some());
        let session = CredentialSession::new(ClientLifetime::OneShot);
        assert!(session.ensure_open().is_ok());
        assert!(session.close());
        assert!(!session.close());
        assert!(session.ensure_open().is_err());
        let retained = CredentialSession::new(ClientLifetime::LongLived);
        assert!(!retained.close());
        assert!(retained.ensure_open().is_ok());
    }

    async fn mock_owner_startup_io(
        bytes: &[u8],
        close_writer: bool,
        attach_started: std::time::Instant,
    ) -> OwnerStartupObservation {
        use tokio::io::AsyncWriteExt;

        let (mut writer, reader) = tokio::io::duplex(OWNER_STARTUP_FRAME_LIMIT + 1);
        writer.write_all(bytes).await.unwrap();
        let held_writer = if close_writer {
            drop(writer);
            None
        } else {
            Some(writer)
        };
        let observation = read_owner_startup_failure(Some(reader), attach_started).await;
        // Keep a live writer explicitly owned until after the collector returns.
        drop(held_writer);
        observation
    }
}
