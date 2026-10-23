//! Native adapters reconnect to the selected owner without lending owner authority to an agent.
use anyhow::{Context as _, Result, bail};
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
    build: String,
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
        let build = tokio::task::spawn_blocking(move || {
            nw_platform::files::identity(&exe, &tokio_util::sync::CancellationToken::new())
        })
        .await??
        .sha256;
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
            build,
        })
    }
    pub async fn attach(&self) -> Result<OwnerRecord> {
        if let Some(record) = self.cached.lock().await.clone() {
            return Ok(record);
        }
        let runtime = self.root.join("runtime/owner.json");
        let started = std::time::Instant::now();
        let mut spawned: Option<std::time::Instant> = None;
        loop {
            if runtime.try_exists()? {
                let path = runtime.clone();
                let bytes = tokio::task::spawn_blocking(move || {
                    nw_platform::security::read_private(&path, 16384)
                })
                .await??;
                let record: OwnerRecord = serde_json::from_slice(&bytes)?;
                if record.canonical_root != nw_platform::files::path_text(&self.root)? {
                    bail!("OWNER_IDENTITY_CONFLICT: runtime record names another root");
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
                    if identity["epoch"] != record.epoch || identity["root_id"] != record.root_id {
                        bail!(
                            "OWNER_IDENTITY_CONFLICT: protected bootstrap and active owner differ"
                        );
                    }
                    if record.build != self.build || record.registry != registry_fingerprint() {
                        bail!(
                            "OWNER_VERSION_CONFLICT: another build owns this root; let its jobs/clients drain before updating"
                        );
                    }
                    *self.cached.lock().await = Some(record.clone());
                    return Ok(record);
                }
            }
            let lock_root = self.root.join("state/generations/1");
            let may_spawn = !lock_root.try_exists()?
                || nw_platform::files::RootLock::acquire(&lock_root).is_ok();
            if may_spawn
                && spawned.is_none_or(|instant| instant.elapsed() >= Duration::from_secs(1))
            {
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
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                spawned = Some(std::time::Instant::now());
            }
            if started.elapsed() > Duration::from_secs(20) {
                bail!(
                    "OWNER_START_TIMEOUT: owner is busy, recovering, or cannot initialize the protected root; no different root or port was selected"
                );
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
        let response = self
            .http
            .post(format!(
                "http://127.0.0.1:{}/internal/credential",
                record.port
            ))
            .bearer_auth(&record.bearer)
            .json(&serde_json::json!({"profile":profile}))
            .send()
            .await?
            .error_for_status()?;
        let value = bounded_json(response).await?;
        let token = value["bearer"]
            .as_str()
            .context("Scoped credential was not issued")?
            .to_owned();
        *cached = Some((record.epoch.clone(), token.clone()));
        Ok(token)
    }
    pub async fn request(&self, request: &Request) -> Result<Response> {
        for attempt in 0..2 {
            let record = self.attach().await?;
            let token = self.scoped_token(&record).await?;
            let result = self
                .http
                .post(format!("http://127.0.0.1:{}/api/v1/commands", record.port))
                .bearer_auth(token)
                .json(request)
                .send()
                .await;
            match result {
                Ok(response) if response.status().is_success() => {
                    return Ok(serde_json::from_value(bounded_json(response).await?)?);
                }
                Ok(response)
                    if (response.status() == reqwest::StatusCode::UNAUTHORIZED
                        || response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE)
                        && attempt == 0 => {}
                Ok(response) => bail!("Owner request returned HTTP {}", response.status()),
                Err(error) if attempt != 0 => return Err(error.into()),
                Err(_) => {}
            }
            *self.cached.lock().await = None;
            *self.credential.lock().await = None;
        }
        bail!("Owner reconnect failed; original durable request identity was retained")
    }
    pub async fn internal(
        &self,
        route: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if self.profile.is_some() {
            bail!("Internal owner controls are not available to an agent adapter");
        }
        let record = self.attach().await?;
        let response = self
            .http
            .post(format!("http://127.0.0.1:{}/{route}", record.port))
            .bearer_auth(record.bearer)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        bounded_json(response).await
    }
}
pub async fn bounded_json(mut response: reqwest::Response) -> Result<serde_json::Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > 256 * 1024 {
            bail!("Response exceeds 256 KiB");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(nw_domain::commands::decode_json(&bytes)?)
}
