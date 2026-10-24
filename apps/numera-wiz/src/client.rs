//! Native adapters reconnect to the selected owner without lending owner authority to an agent.
mod download;
use anyhow::{Context as _, Result};
use nw_domain::commands::{Request, Response};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::sync::Mutex;

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
                && (!state_root.try_exists()? || nw_platform::files::RootLock::acquire(&state_root).is_ok())
                && (!lock_root.try_exists()? || nw_platform::files::RootLock::acquire(&lock_root).is_ok());
            if may_spawn && !spawned {
                let mut command = tokio::process::Command::new(std::env::current_exe()?);
                command.arg("--data-dir").arg(&self.root);
                if let Some(components) = &self.components {
                    command.arg("--components-dir").arg(components);
                }
                command
                    .arg("__owner")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(false);
                #[cfg(unix)]
                command.process_group(0);
                #[cfg(windows)]
                command.creation_flags(0x00000008 | 0x00000200);
                let mut child = command
                    .spawn()
                    .context("Cannot start the protected owner")?;
                tracing::info!(process_id = child.id(), "Protected owner child spawned");
                tokio::spawn(async move {
                    match child.wait().await {
                        Ok(status) if status.success() => {
                            tracing::debug!("Protected owner child exited normally")
                        }
                        Ok(status) => tracing::warn!(
                            exit_code = status.code(),
                            "Protected owner child exited with an error"
                        ),
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
    }
}
