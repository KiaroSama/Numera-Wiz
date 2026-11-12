//! Native adapters reconnect to the selected owner without lending owner authority to an agent.
mod download;
use anyhow::{Context as _, Result};
use nw_domain::commands::{Request, Response};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};
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
    credential: Arc<Mutex<Option<(String, String)>>>,
    binding: Arc<Mutex<SessionBinding>>,
    build: String,
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
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(root)?;
        let exe = std::env::current_exe()?;
        let identity_started = std::time::Instant::now();
        let identity = tokio::task::spawn_blocking(move || {
            nw_platform::files::identity(&exe, &tokio_util::sync::CancellationToken::new())
        })
        .await??;
        tracing::info!(elapsed_ms=identity_started.elapsed().as_millis(), executable_bytes=%identity.bytes, "Executable identity read completed");
        let build = identity.sha256;
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
            credential: Arc::new(Mutex::new(None)),
            binding: Arc::new(Mutex::new(SessionBinding::default())),
            build,
        })
    }
    pub async fn attach(&self) -> Result<OwnerRecord> {
        if let Some(record) = self.cached.lock().await.clone() {
            return Ok(record);
        }
        let started = std::time::Instant::now();
        let runtime_root = self.root.join("runtime");
        let runtime_root = tokio::task::spawn_blocking(move || {
            nw_platform::security::private_directory(&runtime_root)
        })
        .await??;
        let runtime = runtime_root.join("owner.json");
        let mut bootstrap_gate = None;
        let mut spawned = false;
        loop {
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
        let Some(profile) = &self.profile else {
            return Ok(record.bearer.clone());
        };
        let mut cached = self.credential.lock().await;
        if let Some((epoch, token)) = &*cached
            && epoch == &record.epoch
        {
            return Ok(token.clone());
        }
        let expected_principal = self.binding.lock().await.principal.clone();
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
            .context("Scoped credential was not issued")?
            .to_owned();
        if value["profile"].as_str() != Some(profile.as_str()) {
            return Err(nw_domain::DomainError::new(
                "OWNER_IDENTITY_CONFLICT",
                "Scoped bootstrap names a different profile",
            )
            .into());
        }
        self.binding
            .lock()
            .await
            .bind_principal(value["principal"].as_str().unwrap_or(""))?;
        *cached = Some((record.epoch.clone(), token.clone()));
        Ok(token)
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
                    *self.cached.lock().await = None;
                    *self.credential.lock().await = None;
                    continue;
                }
                Err(error) => return Err(error),
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
            *self.cached.lock().await = None;
            *self.credential.lock().await = None;
        }
        Err(nw_domain::DomainError::new(
            "TRANSPORT_ERROR",
            "Owner reconnect failed; original durable request identity was retained",
        )
        .into())
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
                })
                .await
                .expect("MOCK bounded startup reader exceeded its outer watchdog");
            });
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
