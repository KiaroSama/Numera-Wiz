//! Actual prior/current packaged executables, exercised only by required hosted checks.
//! Synthetic cleanup plans are storage/digest fixtures and are never submitted as jobs.
use anyhow::{Context, Result, ensure};
use nw_platform::{files::RootLock, security};
use nw_store::{
    generations::{GenerationLocation, RootLease},
    versions::GenerationFacts,
};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
};

const RECORDS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("versioned_records");
const MAX_BODY: usize = 2 * 1024 * 1024;
const POPULATED_PROJECT_NAMESPACES: &[&str] = &[
    "project",
    "project-entity",
    "project-bin",
    "project-track",
    "project-clip",
    "project-op",
    "project-patch",
    "project-history",
    "project-history-summary",
    "project-reference",
];

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn file_identity(path: &Path) -> Result<Value> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "Identity input must be an ordinary file"
    );
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        length = length
            .checked_add(count as u64)
            .context("File byte count")?;
    }
    Ok(json!({"bytes":length,"sha256":hex::encode(hash.finalize())}))
}
fn read_json(path: &Path, limit: u64) -> Result<Value> {
    ensure!(
        fs::metadata(path)?.len() <= limit,
        "JSON file exceeds fixture bound"
    );
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn text(value: &Value, field: &str) -> Result<String> {
    Ok(value[field]
        .as_str()
        .context("Required string field missing")?
        .to_owned())
}
fn ordered(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, ordered(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(ordered).collect()),
        scalar => scalar,
    }
}
struct Evidence {
    path: PathBuf,
    events: Mutex<Vec<Value>>,
    started: Instant,
}
impl Evidence {
    fn record(&self, mut event: Value) -> Result<()> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| anyhow::anyhow!("Evidence lock failed"))?;
        let elapsed_ms = u64::try_from(self.started.elapsed().as_millis())
            .context("Compatibility elapsed milliseconds exceed the evidence integer range")?;
        event
            .as_object_mut()
            .context("Compatibility evidence event must be an object")?
            .insert("elapsed_ms".into(), json!(elapsed_ms));
        events.push(event);
        fs::write(
            &self.path,
            serde_json::to_vec_pretty(
                &json!({"schema_version":1,"scope":"actual prior/current package storage, replay and retained-copy boundaries; no media or release qualification","events":*events}),
            )?,
        )?;
        Ok(())
    }
}
#[derive(Clone)]
struct Binary {
    path: PathBuf,
    identity: Value,
    source: String,
    label: &'static str,
}
impl Binary {
    fn verify(&self) -> Result<()> {
        ensure!(
            file_identity(&self.path)? == self.identity,
            "Executable changed after identity validation"
        );
        Ok(())
    }
    fn hash(&self) -> Result<&str> {
        self.identity["sha256"].as_str().context("Executable hash")
    }
}
struct Configuration {
    old: Binary,
    new: Binary,
}
fn environment_path(name: &str) -> Result<PathBuf> {
    let path =
        PathBuf::from(std::env::var_os(name).context("Required compatibility path missing")?);
    ensure!(path.is_absolute(), "Compatibility paths must be absolute");
    Ok(path)
}
fn configuration(evidence: &Evidence) -> Result<Configuration> {
    ensure!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
        "Actual compatibility execution requires hosted checks"
    );
    let manifest_path = environment_path("NW_COMPAT_BASELINE_MANIFEST")?;
    let bytes = fs::read(&manifest_path)?;
    ensure!(
        bytes.as_slice()
            == include_bytes!("../../../../ci/project-compatibility-baseline.json").as_slice(),
        "Baseline manifest differs from frozen source"
    );
    let manifest: Value = serde_json::from_slice(&bytes)?;
    let label = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("windows", "x86_64") => "windows-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        _ => anyhow::bail!("Unsupported native compatibility target"),
    };
    let target = manifest["targets"]
        .as_array()
        .context("Baseline targets")?
        .iter()
        .find(|row| row["target"] == label)
        .context("Missing baseline target")?;
    let evidence_dir = environment_path("NW_COMPAT_EVIDENCE_DIR")?;
    let prepared = read_json(&evidence_dir.join("preparation.json"), 65536)?;
    let source = std::env::var("NW_SOURCE_SHA").context("Current source SHA")?;
    ensure!(
        source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid current source SHA"
    );
    ensure!(
        prepared["status"] == "prepared"
            && prepared["target"] == label
            && prepared["source_sha"] == source
            && prepared["baseline_source_sha"] == manifest["source_sha"]
            && prepared["baseline_run"] == manifest["run_id"]
            && prepared["baseline_attempt"] == manifest["run_attempt"]
            && prepared["manifest_sha256"] == sha(&bytes),
        "Required package preparation did not succeed for these sources"
    );
    let provenance = read_json(
        &evidence_dir.join("current-package-provenance.json"),
        8 * 1024 * 1024,
    )?;
    let run = format!(
        "https://github.com/{}/actions/runs/{}",
        std::env::var("GITHUB_REPOSITORY")?,
        std::env::var("GITHUB_RUN_ID")?
    );
    ensure!(
        provenance["source_sha"] == source
            && provenance["target"] == label
            && provenance["run_url"] == run
            && provenance["run_attempt"] == std::env::var("GITHUB_RUN_ATTEMPT")?
            && provenance["native_features"] == json!(["desktop", "embedded-web"]),
        "Package provenance is not this exact hosted composition"
    );
    ensure!(
        provenance["executable"]["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0),
        "Package executable length must be an integer"
    );
    let executable = if cfg!(windows) {
        "numera-wiz.exe"
    } else {
        "numera-wiz"
    };
    let entry = if cfg!(target_os = "macos") {
        "Numera Wiz.app/Contents/MacOS/numera-wiz"
    } else {
        executable
    };
    let matching = provenance["files"]
        .as_array()
        .context("Package file inventory")?
        .iter()
        .filter(|file| file["path"] == entry)
        .collect::<Vec<_>>();
    ensure!(
        matching.len() == 1
            && matching[0]["sha256"] == provenance["executable"]["sha256"]
            && matching[0]["bytes"] == provenance["executable"]["bytes"]
            && prepared["current"] == provenance["executable"],
        "Package executable file identity differs"
    );
    let old = Binary {
        path: environment_path("NW_COMPAT_PREVIOUS_EXE")?,
        identity: json!({"bytes":target["executable_bytes"],"sha256":target["executable_sha256"]}),
        source: text(&manifest, "source_sha")?,
        label: "prior",
    };
    let new = Binary {
        path: environment_path("NW_COMPAT_CURRENT_EXE")?,
        identity: provenance["executable"].clone(),
        source,
        label: "current",
    };
    ensure!(
        old.path != new.path
            && old.identity != new.identity
            && prepared["previous"] == old.identity,
        "Distinct identified executable snapshots are required"
    );
    old.verify()?;
    new.verify()?;
    evidence.record(json!({"phase":"configuration","target":label,"baseline_source":old.source,"baseline_run":manifest["run_id"],"baseline_attempt":manifest["run_attempt"],"prior":old.identity,"current_source":new.source,"current":new.identity,"native_features":provenance["native_features"]}))?;
    Ok(Configuration { old, new })
}

#[derive(Clone, Deserialize, PartialEq, Eq)]
struct OwnerRecord {
    canonical_root: String,
    root_id: String,
    epoch: String,
    build: String,
    registry: String,
    port: u16,
    bearer: String,
}
struct Captured {
    bytes: Vec<u8>,
    total: u64,
}
async fn drain(mut input: impl AsyncRead + Unpin) -> Result<Captured> {
    let mut bytes = Vec::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 8192];
    loop {
        let count = input.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .context("Process output count")?;
        let keep = count.min(MAX_BODY.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
    }
    Ok(Captured { bytes, total })
}
async fn response_bytes(mut response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "Private owner HTTP status failed"
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= MAX_BODY as u64),
        "Owner response exceeds bound"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_BODY,
            "Owner response exceeds bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
struct Owner {
    child: Child,
    stderr: tokio::task::JoinHandle<Result<Captured>>,
    record: OwnerRecord,
    http: reqwest::Client,
    lease: String,
    last_lease_renewal: Option<Instant>,
    binary: Binary,
    last_project_query: Option<tokio::time::Instant>,
}
impl Owner {
    async fn start(
        binary: &Binary,
        root: &Path,
        prior_epochs: &mut BTreeSet<String>,
        root_id: &mut Option<String>,
        evidence: &Evidence,
    ) -> Result<Self> {
        binary.verify()?;
        let mut child = Command::new(&binary.path)
            .arg("--data-dir")
            .arg(root)
            .arg("__owner")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stderr = tokio::spawn(drain(child.stderr.take().context("Owner stderr")?));
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(20))
            .build()?;
        let readiness = tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    evidence.record(json!({"phase":"owner-premature-exit","binary":binary.label,"exit_code":status.code(),"source":binary.source,"expected_build":binary.identity["sha256"]}))?;
                    anyhow::bail!("Controlled owner exited before readiness");
                }
                if let Ok(bytes) = security::read_private(&root.join("runtime/owner.json"), 16384)
                    && let Ok(record) = serde_json::from_slice::<OwnerRecord>(&bytes)
                    && record.build == binary.hash()? && !prior_epochs.contains(&record.epoch)
                {
                    ensure!(fs::canonicalize(&record.canonical_root)? == root && record.port != 0, "Private owner canonical root/port differs");
                    if let Ok(response) = http.get(format!("http://127.0.0.1:{}/health", record.port)).bearer_auth(&record.bearer).send().await
                        && let Ok(health) = response_bytes(response).await
                    {
                        ensure!(health["epoch"] == record.epoch && health["root_id"] == record.root_id && health["build"] == record.build && health["registry"] == record.registry, "Owner health/bootstrap identity differs");
                        if let Some(expected) = root_id.as_ref() { ensure!(*expected == record.root_id, "Root UUID changed across owners"); }
                        return Ok::<_, anyhow::Error>(record);
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await;
        let ready = match readiness {
            Ok(Ok(record)) => record,
            failure => {
                let observed = child.try_wait()?;
                if observed.is_none() {
                    // Cleanup is restricted to this retained child; a killed generation
                    // is never reopened or counted as preservation evidence.
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                }
                if let Ok(Ok(Ok(captured))) =
                    tokio::time::timeout(Duration::from_secs(5), &mut stderr).await
                {
                    let diagnostics = String::from_utf8_lossy(&captured.bytes);
                    let codes = [
                        "STORE_VERSION_UNSUPPORTED",
                        "STORE_RECOVERY_REQUIRED",
                        "OWNER_BUSY",
                        "OWNER_IDENTITY_CONFLICT",
                    ]
                    .into_iter()
                    .filter(|code| diagnostics.contains(*code))
                    .collect::<Vec<_>>();
                    evidence.record(json!({"phase":"owner-start-failed","binary":binary.label,"exit_code":observed.as_ref().and_then(|status| status.code()),"cleanup_killed_owned_child":observed.is_none(),"stderr_bytes":captured.total,"retained_stderr_sha256":sha(&captured.bytes),"recognized_error_codes":codes}))?;
                }
                return Err(match failure {
                    Ok(Err(error)) => error,
                    Err(_) => anyhow::anyhow!("Controlled owner readiness deadline"),
                    Ok(Ok(_)) => unreachable!(),
                });
            }
        };
        ensure!(
            uuid::Uuid::parse_str(&ready.epoch)?.to_string() == ready.epoch
                && uuid::Uuid::parse_str(&ready.root_id)?.to_string() == ready.root_id,
            "Owner epoch/root must be canonical UUIDs"
        );
        prior_epochs.insert(ready.epoch.clone());
        *root_id = Some(ready.root_id.clone());
        let mut owner = Self {
            child,
            stderr,
            record: ready,
            http,
            lease: uuid::Uuid::new_v4().to_string(),
            last_lease_renewal: None,
            binary: binary.clone(),
            last_project_query: None,
        };
        owner.renew().await?;
        let registry = owner
            .command(&query("commands.list", json!({})), evidence)
            .await?;
        let fingerprint = sha(&serde_json::to_vec(&ordered(
            registry["result"]["commands"].clone(),
        ))?);
        ensure!(
            registry["result"]["commands"].is_array() && fingerprint == owner.record.registry,
            "Actual binary registry fingerprint differs from health"
        );
        evidence.record(json!({"phase":"owner-ready","binary":binary.label,"source":binary.source,"canonical_root":root,"root_id":owner.record.root_id,"epoch":owner.record.epoch,"build":owner.record.build,"registry":fingerprint}))?;
        Ok(owner)
    }
    async fn private(&self, route: &str, body: &Value) -> Result<Value> {
        response_bytes(
            self.http
                .post(format!("http://127.0.0.1:{}{route}", self.record.port))
                .bearer_auth(&self.record.bearer)
                .json(body)
                .send()
                .await?,
        )
        .await
    }
    async fn renew(&mut self) -> Result<()> {
        ensure!(
            self.child.try_wait()?.is_none(),
            "Controlled owner exited while leased"
        );
        // Measure from request start, conservatively preceding server acceptance.
        let renewed_at = Instant::now();
        self.private("/internal/lease", &json!({"id":self.lease,"public":false}))
            .await?;
        self.last_lease_renewal = Some(renewed_at);
        Ok(())
    }
    async fn cli_boundary(&mut self, root: &Path) -> Result<u64> {
        ensure!(
            self.child
                .try_wait()
                .map_err(|_| anyhow::anyhow!("Retained owner child status unavailable"))?
                .is_none(),
            "Retained owner child exited before CLI ownership check"
        );
        let bytes = security::read_private(&root.join("runtime/owner.json"), 16384)
            .map_err(|_| anyhow::anyhow!("Protected owner record unavailable"))?;
        let observed: OwnerRecord = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Protected owner record invalid"))?;
        ensure!(
            observed == self.record,
            "Protected owner record changed from the retained original"
        );
        self.health_unchanged()
            .await
            .map_err(|_| anyhow::anyhow!("Retained owner health/identity check failed"))?;
        let lease_age_ms = u64::try_from(
            self.last_lease_renewal
                .context("Controlled owner lease has never been renewed")?
                .elapsed()
                .as_millis(),
        )
        .context("Controlled owner lease age exceeds evidence range")?;
        self.renew()
            .await
            .map_err(|_| anyhow::anyhow!("Retained owner same-lease renewal failed"))?;
        ensure!(
            self.child
                .try_wait()
                .map_err(|_| anyhow::anyhow!("Retained owner child status unavailable"))?
                .is_none(),
            "Retained owner child exited during CLI ownership check"
        );
        Ok(lease_age_ms)
    }
    async fn command(&mut self, request: &Value, evidence: &Evidence) -> Result<Value> {
        self.renew().await?;
        let response = self.private("/api/v1/commands", request).await?;
        if ["projects.inspect", "sequences.inspect"]
            .iter()
            .any(|id| request["command"]["id"] == *id)
        {
            self.last_project_query = Some(tokio::time::Instant::now());
        }
        evidence.record(json!({"phase":"command","binary":self.binary.label,"command":request["command"]["id"],"request_id":request["request_id"],"ok":response["ok"],"error_code":response["error"]["code"],"receipt_id":response["receipt_id"],"response_sha256":sha(&serde_json::to_vec(&response)?)}))?;
        ensure!(
            response["ok"] == true && response["request_id"] == request["request_id"],
            "Actual owner command failed or response identity differs (see sanitized evidence)"
        );
        Ok(response)
    }
    async fn health_unchanged(&mut self) -> Result<()> {
        ensure!(
            self.child.try_wait()?.is_none(),
            "Original owner disappeared during conflicting attachment"
        );
        let health = response_bytes(
            self.http
                .get(format!("http://127.0.0.1:{}/health", self.record.port))
                .bearer_auth(&self.record.bearer)
                .send()
                .await?,
        )
        .await?;
        ensure!(
            health["epoch"] == self.record.epoch
                && health["root_id"] == self.record.root_id
                && health["build"] == self.record.build
                && health["registry"] == self.record.registry,
            "Conflicting attachment changed original owner identity"
        );
        Ok(())
    }
    async fn stop(mut self, evidence: &Evidence) -> Result<()> {
        // Replacement queries retire previous tickets; allow the last 30-second
        // ticket and the one-second reaper to drain while our private lease remains.
        if let Some(last) = self.last_project_query {
            self.renew().await?;
            tokio::time::sleep_until(last + Duration::from_secs(32)).await;
            evidence.record(
                json!({"phase":"project-read-tickets-expired","epoch":self.record.epoch}),
            )?;
        }
        ensure!(
            self.child
                .try_wait()
                .map_err(|_| anyhow::anyhow!("Controlled owner stop-release child status unavailable"))?
                .is_none(),
            "Controlled owner exited before stop-release"
        );
        self.private("/internal/lease", &json!({"id":self.lease,"release":true}))
            .await
            .map_err(|_| anyhow::anyhow!("Controlled owner stop-release request failed"))?;
        let status = tokio::time::timeout(Duration::from_secs(40), self.child.wait())
            .await
            .context("Owner did not gracefully drain after lease release")??;
        ensure!(status.success(), "Owned owner did not exit cleanly");
        let stderr = tokio::time::timeout(Duration::from_secs(5), self.stderr)
            .await
            .context("Owner stderr drain deadline")???;
        evidence.record(json!({"phase":"owner-stopped","binary":self.binary.label,"epoch":self.record.epoch,"exit_code":status.code(),"stderr_bytes":stderr.total,"retained_stderr_sha256":sha(&stderr.bytes)}))?;
        Ok(())
    }
}
fn query(id: &str, params: Value) -> Value {
    json!({"api_version":1,"command":{"id":id,"version":1},"request_id":format!("compat-query-{}", uuid::Uuid::new_v4()),"params":params})
}
fn durable(scope: &str, key: &str, id: &str, params: Value, project: Option<(&str, u64)>) -> Value {
    let mut request = query(id, params);
    request["request_id"] = json!(key);
    request["replay_scope"] = json!(scope);
    request["idempotency_key"] = json!(key);
    if let Some((project, revision)) = project {
        request["project"] = json!({"id":project,"expected_revision":revision.to_string()});
    }
    request
}
fn same_receipt(original: &Value, replay: &Value) -> Result<()> {
    ensure!(
        original["ok"] == true
            && replay["ok"] == true
            && original["receipt_id"].is_string()
            && original["receipt_id"] == replay["receipt_id"]
            && original["request_id"] == replay["request_id"]
            && original["result"] == replay["result"],
        "Durable replay changed original receipt/result"
    );
    Ok(())
}
async fn cli(
    owner: &mut Owner,
    binary: &Binary,
    root: &Path,
    args: &[&str],
    evidence: &Evidence,
    label: &str,
) -> Result<(Value, i32, Captured)> {
    // Healthy native attachment precedes this gate in both packaged clients.
    // Holding it prevents an unexpected owner death from spawning a replacement.
    let _bootstrap_gate = RootLock::acquire(&root.join("runtime"))
        .map_err(|_| anyhow::anyhow!("Typed CLI runtime bootstrap guard unavailable"))?;
    let prehash_lease_age_ms = owner
        .cli_boundary(root)
        .await
        .context("Typed CLI prehash ownership boundary")?;
    binary.verify()?;
    let posthash_lease_age_ms = owner
        .cli_boundary(root)
        .await
        .context("Typed CLI posthash-before-spawn ownership boundary")?;
    let mut child = Command::new(&binary.path)
        .arg("--data-dir")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = tokio::spawn(drain(child.stdout.take().context("CLI stdout")?));
    let stderr = tokio::spawn(drain(child.stderr.take().context("CLI stderr")?));
    let status = tokio::time::timeout(Duration::from_secs(45), child.wait())
        .await
        .context("Actual typed CLI deadline")??;
    let post_cli_lease_age_ms = owner
        .cli_boundary(root)
        .await
        .context("Typed CLI post-CLI ownership boundary")?;
    let output = stdout.await??;
    let error = stderr.await??;
    ensure!(output.total <= MAX_BODY as u64, "CLI output exceeds bound");
    let code = status
        .code()
        .context("CLI must return a synchronous exit code")?;
    let value = serde_json::from_slice(&output.bytes).unwrap_or(Value::Null);
    evidence.record(json!({"phase":"cli","binary":binary.label,"command":label,"exit_code":code,"stdout_bytes":output.total,"stdout_sha256":sha(&output.bytes),"stderr_bytes":error.total,"retained_stderr_sha256":sha(&error.bytes),"receipt_id":value["receipt_id"],"error_code":value["error"]["code"],"checked_owner":{"root_id":owner.record.root_id,"epoch":owner.record.epoch,"build":owner.record.build,"registry":owner.record.registry,"port":owner.record.port,"retained_child_alive":true,"protected_record_unchanged":true,"canonical_root_unchanged":true,"runtime_bootstrap_guard_held":true},"lease_age_ms":{"prehash":prehash_lease_age_ms,"posthash":posthash_lease_age_ms,"post_cli":post_cli_lease_age_ms}}))?;
    Ok((value, code, error))
}

struct Snapshot {
    rows: BTreeMap<String, Vec<u8>>,
    facts: GenerationFacts,
    activation: Option<String>,
}
fn project_key(key: &str) -> bool {
    key.starts_with("project/") || key.starts_with("project-")
}
fn legacy_key(key: &str) -> bool {
    ["plan/", "batch-plan/", "receipt/"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
        || key == "profile/compat-legacy"
}
fn raw_locked(path: &Path, activation: Option<String>) -> Result<Snapshot> {
    let rows = {
        let database = redb::Database::builder()
            .set_cache_size(8 * 1024 * 1024)
            .open_read_only(path.join("store.redb"))?;
        let transaction = database.begin_read()?;
        let table = transaction.open_table(RECORDS)?;
        let mut rows = BTreeMap::new();
        let mut namespaces = BTreeSet::new();
        let mut bytes = 0usize;
        for row in table.iter()? {
            let (key, value) = row?;
            let key = key.value();
            let value = value.value();
            ensure!(
                key.len() <= 512 && value.len() <= 8 * 1024 * 1024 && rows.len() < 50000,
                "Raw fixture key/record/count bound exceeded"
            );
            bytes = bytes
                .checked_add(key.len())
                .and_then(|sum| sum.checked_add(value.len()))
                .context("Raw fixture bytes")?;
            ensure!(
                bytes <= 32 * 1024 * 1024,
                "Raw fixture exceeds total 32MiB snapshot budget"
            );
            namespaces.insert(key.split('/').next().unwrap_or("").to_owned());
            ensure!(
                namespaces.len() <= 128,
                "Raw fixture namespace bound exceeded"
            );
            rows.insert(key.to_owned(), value.to_vec());
        }
        rows
    };
    let facts = nw_store::versions::inspect_closed(path)?;
    ensure!(
        rows.len() == facts.record_count as usize,
        "Independent raw record counts differ"
    );
    let mut hash = Sha256::new();
    hash.update(b"numera-generation-records-v1\0");
    for (key, value) in &rows {
        hash.update((key.len() as u64).to_le_bytes());
        hash.update(key.as_bytes());
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    ensure!(
        hex::encode(hash.finalize()) == facts.records_sha256,
        "Independent full raw fingerprints differ"
    );
    ensure!(
        rows.get("meta/root")
            .is_some_and(|root| root == facts.root_id.as_bytes())
            && rows
                .get("meta/signing-key")
                .is_some_and(|key| key.len() == 32),
        "Raw root/replay identity missing"
    );
    Ok(Snapshot {
        rows,
        facts,
        activation,
    })
}
fn record_snapshot(
    name: &str,
    snapshot: &Snapshot,
    path: &Path,
    evidence: &Evidence,
) -> Result<()> {
    evidence.record(json!({"phase":"closed-checkpoint","checkpoint":name,"selected_path":path,"activation_id":snapshot.activation,"facts":snapshot.facts,"physical_store":file_identity(&path.join("store.redb"))?,"rows":snapshot.rows.iter().map(|(key,value)|json!({"key":key,"bytes":value.len(),"sha256":sha(value)})).collect::<Vec<_>>() }))
}
fn closed(root: &Path, name: &str, evidence: &Evidence) -> Result<Snapshot> {
    let lease = RootLease::acquire(root)?;
    let selection = lease.selection()?;
    let path = fs::canonicalize(lease.path(&selection)?)?;
    ensure!(
        path.starts_with(root),
        "Selected generation escaped fixture root"
    );
    // LegacyOne has no retained legacy guard. With a workspace selected,
    // RootLease already owns generation1; this lock is only the selected target.
    let _selected_lock = RootLock::acquire(&path)?;
    let snapshot = raw_locked(&path, selection.activation_id)?;
    if let Some(version) = selection.store_version {
        ensure!(
            snapshot.facts.store_version == version,
            "Selected raw schema differs from activation"
        );
    }
    record_snapshot(name, &snapshot, &path, evidence)?;
    Ok(snapshot)
}
fn preserved(
    original: &Snapshot,
    current: &Snapshot,
    include: impl Fn(&str) -> bool,
) -> Result<()> {
    ensure!(
        original.rows["meta/root"] == current.rows["meta/root"]
            && original.rows["meta/signing-key"] == current.rows["meta/signing-key"],
        "Protected raw root/replay identity changed"
    );
    for (key, bytes) in original.rows.iter().filter(|(key, _)| include(key)) {
        ensure!(
            current.rows.get(key) == Some(bytes),
            "A previously acknowledged raw row changed or disappeared: {key}"
        );
    }
    Ok(())
}
fn projects_identical(original: &Snapshot, current: &Snapshot) -> Result<()> {
    let old = original
        .rows
        .iter()
        .filter(|(key, _)| project_key(key))
        .collect::<BTreeMap<_, _>>();
    let new = current
        .rows
        .iter()
        .filter(|(key, _)| project_key(key))
        .collect::<BTreeMap<_, _>>();
    ensure!(old == new, "Project key set or exact raw bytes changed");
    preserved(original, current, legacy_key)
}
fn guard(snapshot: &Snapshot, migration: &str, revision: &str) -> Result<Vec<u8>> {
    let bytes = snapshot
        .rows
        .get("meta/rollback-guard")
        .context("Adoption guard missing")?;
    let value: Value = serde_json::from_slice(bytes)?;
    ensure!(
        value["schema_version"] == 1
            && value["migration_id"] == migration
            && value["activation_id"].as_str() == snapshot.activation.as_deref()
            && value["revision"] == revision,
        "Actual adoption guard identity/revision differs"
    );
    Ok(bytes.clone())
}
struct RollbackProof {
    rows: Snapshot,
    store: Value,
    manifest: Vec<u8>,
    complete: Vec<u8>,
}
fn rollback(
    root: &Path,
    plan: &Value,
    original: &Snapshot,
    project: &Snapshot,
    name: &str,
    evidence: &Evidence,
) -> Result<RollbackProof> {
    let migration = text(plan, "id")?;
    ensure!(
        uuid::Uuid::parse_str(&migration)?.to_string() == migration,
        "Migration ID must be canonical UUID"
    );
    let lease = RootLease::acquire(root)?;
    let selection = lease.selection()?;
    ensure!(
        matches!(&selection.location, GenerationLocation::Workspace { operation_id, role: nw_store::generations::GenerationRole::Target } if operation_id == &migration),
        "Activation selected a different migration target"
    );
    let expected = lease
        .state()
        .join("generation-workspaces")
        .join(&migration)
        .join("rollback");
    let path = fs::canonicalize(&expected)?;
    ensure!(
        path == expected && path.starts_with(root),
        "Retained rollback path escaped operation workspace"
    );
    let _lock = RootLock::acquire(&path)?;
    let rows = raw_locked(&path, None)?;
    preserved(original, &rows, legacy_key)?;
    projects_identical(project, &rows)?;
    ensure!(
        rows.facts.store_version == 2
            && plan["source_records_sha256"] == rows.facts.records_sha256
            && Some(rows.facts.record_count as u64) == plan["source_records"].as_u64(),
        "Rollback full raw source differs from prior reviewed preacceptance source"
    );
    let manifest = security::read_private(&path.join("copy.json"), 65536)?;
    let complete = security::read_private(&path.join("complete.json"), 65536)?;
    let manifest_value: Value = serde_json::from_slice(&manifest)?;
    let proof: Value = serde_json::from_slice(&complete)?;
    let store = file_identity(&path.join("store.redb"))?;
    ensure!(
        manifest_value["schema_version"] == 1
            && manifest_value["operation_id"] == migration
            && manifest_value["role"] == "rollback"
            && manifest_value["source"] == serde_json::to_value(&rows.facts)?,
        "Retained rollback manifest facts differ"
    );
    let store_bytes = store["bytes"]
        .as_u64()
        .context("Rollback physical length")?
        .to_string();
    ensure!(
        manifest_value["source_file_sha256"] == store["sha256"]
            && manifest_value["source_file_bytes"] == store_bytes
            && proof["schema_version"] == 1
            && proof["manifest_sha256"] == sha(&manifest)
            && proof["store_sha256"] == store["sha256"]
            && proof["store_bytes"] == store_bytes
            && proof["records_sha256"] == rows.facts.records_sha256,
        "Retained rollback physical copy/completion proof differs"
    );
    record_snapshot(name, &rows, &path, evidence)?;
    evidence.record(json!({"phase":"rollback-proof","checkpoint":name,"migration_id":migration,"store":store,"manifest_sha256":sha(&manifest),"completion_sha256":sha(&complete),"reviewed_source_records_sha256":plan["source_records_sha256"]}))?;
    Ok(RollbackProof {
        rows,
        store,
        manifest,
        complete,
    })
}
fn same_rollback(original: &RollbackProof, current: &RollbackProof) -> Result<()> {
    ensure!(
        original.rows.rows == current.rows.rows
            && original.rows.facts == current.rows.facts
            && original.store == current.store
            && original.manifest == current.manifest
            && original.complete == current.complete,
        "Retained rollback bytes/proofs changed after subsequent owner pass"
    );
    Ok(())
}

// Authored independently from the documented legacy struct order / v2 sorted semantic
// projection. Constants were frozen with Python SHA256, never generated by the tested encoder.
const PLAN_V1_PREIMAGE: &str = r#"{"id":"compat-frozen-plan-v1","creator":"compat-storage-fixture","hash":"","source":{"display_name":"Synthetic storage fixture only","id":"compat-fixture-asset","creator":"compat-storage-fixture","identity":{"path":"/compat-storage-fixture/source.mkv","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"probe":{"streams":[],"stream_details":{"0":{"decimal":"9007199254740993","unknown":{"retain":true}}},"format":"matroska","global_tags":{"fixture":"storage-only"},"chapters":[{"unknown_decimal":"18446744073709551615"}],"verification_scope":"Synthetic storage fixture; no media execution"}},"output":"/compat-storage-fixture/output.mkv","ffmpeg":{"path":"/compat-storage-fixture/ffmpeg","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"ffprobe":{"path":"/compat-storage-fixture/ffprobe","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"resolution":{"action":"copy_unchanged","streams":[],"outcomes":[],"reasons":[],"warnings":[],"preservation":{"additional_video":true,"attached_pictures":true,"attachments":true,"data":true,"unknown":true,"global_tags":true,"stream_tags":true,"chapters":true,"dispositions":true}},"argv_template":[],"created_at":"100","expires_at":"1900","verification_scope":"Synthetic frozen cleanup-plan storage/digest fixture only"}"#;
const PLAN_V1_HASH: &str = "45127cf5fcf5231e31058d5f2b86163688002decc008d16d5d398d3e882c36d7";
const PLAN_V1_RAW: &str = concat!(
    r#"{"id":"compat-frozen-plan-v1","creator":"compat-storage-fixture","hash":"45127cf5fcf5231e31058d5f2b86163688002decc008d16d5d398d3e882c36d7","source":{"display_name":"Synthetic storage fixture only","id":"compat-fixture-asset","creator":"compat-storage-fixture","identity":{"path":"/compat-storage-fixture/source.mkv","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"probe":{"streams":[],"stream_details":{"0":{"decimal":"9007199254740993","unknown":{"retain":true}}},"format":"matroska","global_tags":{"fixture":"storage-only"},"chapters":[{"unknown_decimal":"18446744073709551615"}],"verification_scope":"Synthetic storage fixture; no media execution"}},"output":"/compat-storage-fixture/output.mkv","ffmpeg":{"path":"/compat-storage-fixture/ffmpeg","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"ffprobe":{"path":"/compat-storage-fixture/ffprobe","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"resolution":{"action":"copy_unchanged","streams":[],"outcomes":[],"reasons":[],"warnings":[],"preservation":{"additional_video":true,"attached_pictures":true,"attachments":true,"data":true,"unknown":true,"global_tags":true,"stream_tags":true,"chapters":true,"dispositions":true}},"argv_template":[],"created_at":"100","expires_at":"1900","verification_scope":"Synthetic frozen cleanup-plan storage/digest fixture only"}"#,
    "\n"
);
const PLAN_V1_RAW_SHA: &str = "98c1dadeecd81abc4e4401db6002127dc0dadff865450b27a7b8fc9d004df302";
const PLAN_V2_PREIMAGE: &str = r#"{"argv_template":[],"ffmpeg":{"bytes":"17","path":"/compat-storage-fixture/ffmpeg","sha256":"abababababababababababababababababababababababababababababababab"},"ffprobe":{"bytes":"17","path":"/compat-storage-fixture/ffprobe","sha256":"abababababababababababababababababababababababababababababababab"},"output":"/compat-storage-fixture/output.mkv","required_rights":{"data_root":"","output_create":{"collision_policy":"no_replace","path":"/compat-storage-fixture/output.mkv"},"source_read":{"identity":{"bytes":"17","path":"/compat-storage-fixture/source.mkv","sha256":"abababababababababababababababababababababababababababababababab"},"purpose":{"kind":"native_file"}}},"resolution":{"action":"copy_unchanged","outcomes":[],"preservation":{"additional_video":true,"attached_pictures":true,"attachments":true,"chapters":true,"data":true,"dispositions":true,"global_tags":true,"stream_tags":true,"unknown":true},"reasons":[],"streams":[],"warnings":[]},"schema_version":2,"source":{"identity":{"bytes":"17","path":"/compat-storage-fixture/source.mkv","sha256":"abababababababababababababababababababababababababababababababab"},"probe":{"chapters":[{"unknown_decimal":"18446744073709551615"}],"format":"matroska","global_tags":{"fixture":"storage-only"},"stream_details":{"0":{"decimal":"9007199254740993","unknown":{"retain":true}}},"streams":[],"verification_scope":"Synthetic storage fixture; no media execution"}},"verification_scope":"Synthetic frozen cleanup-plan storage/digest fixture only"}"#;
const PLAN_V2_HASH: &str = "5cbe195fb178cc97d3737a74e0c43514a5df2f7f6a8f2d490362f1e848b132dc";
const PLAN_V2_RAW: &str = concat!(
    r#"{"schema_version":2,"required_rights":{"data_root":"","source_read":{"identity":{"path":"/compat-storage-fixture/source.mkv","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"purpose":{"kind":"native_file"}},"output_create":{"path":"/compat-storage-fixture/output.mkv","collision_policy":"no_replace"}},"creation_audit":{"principal":"compat-storage-fixture","policy_revision":7},"id":"compat-frozen-plan-v2","creator":"compat-storage-fixture","hash":"5cbe195fb178cc97d3737a74e0c43514a5df2f7f6a8f2d490362f1e848b132dc","source":{"display_name":"Synthetic storage fixture only","id":"compat-fixture-asset","creator":"compat-storage-fixture","identity":{"path":"/compat-storage-fixture/source.mkv","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"probe":{"streams":[],"stream_details":{"0":{"decimal":"9007199254740993","unknown":{"retain":true}}},"format":"matroska","global_tags":{"fixture":"storage-only"},"chapters":[{"unknown_decimal":"18446744073709551615"}],"verification_scope":"Synthetic storage fixture; no media execution"}},"output":"/compat-storage-fixture/output.mkv","ffmpeg":{"path":"/compat-storage-fixture/ffmpeg","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"ffprobe":{"path":"/compat-storage-fixture/ffprobe","bytes":"17","sha256":"abababababababababababababababababababababababababababababababab"},"resolution":{"action":"copy_unchanged","streams":[],"outcomes":[],"reasons":[],"warnings":[],"preservation":{"additional_video":true,"attached_pictures":true,"attachments":true,"data":true,"unknown":true,"global_tags":true,"stream_tags":true,"chapters":true,"dispositions":true}},"argv_template":[],"created_at":"100","expires_at":"1900","verification_scope":"Synthetic frozen cleanup-plan storage/digest fixture only"}"#,
    "\n"
);
const PLAN_V2_RAW_SHA: &str = "1465b6d402ada1153c01d7a920c0987fd6bbd7a72bb84254abc2485347f2be28";
fn frozen_plans() -> [(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
); 2] {
    [
        (
            "compat-frozen-plan-v1",
            PLAN_V1_RAW,
            PLAN_V1_RAW_SHA,
            PLAN_V1_PREIMAGE,
            PLAN_V1_HASH,
        ),
        (
            "compat-frozen-plan-v2",
            PLAN_V2_RAW,
            PLAN_V2_RAW_SHA,
            PLAN_V2_PREIMAGE,
            PLAN_V2_HASH,
        ),
    ]
}
fn insert_storage_fixtures(root: &Path, evidence: &Evidence) -> Result<()> {
    let lease = RootLease::acquire(root)?;
    let selection = lease.selection()?;
    ensure!(
        matches!(&selection.location, GenerationLocation::LegacyOne {}),
        "Frozen fixtures require original closed legacy generation"
    );
    let path = lease.path(&selection)?;
    let _lock = RootLock::acquire(&path)?;
    // The only direct raw mutation is these declared storage-only legacy fixtures.
    // All profile/receipt/project/migration facts are produced by actual binaries.
    let database = redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open(path.join("store.redb"))?;
    let write = database.begin_write()?;
    {
        let mut table = write.open_table(RECORDS)?;
        for (id, raw, raw_sha, preimage, hash) in frozen_plans() {
            ensure!(
                sha(preimage.as_bytes()) == hash && sha(raw.as_bytes()) == raw_sha,
                "Independent frozen fixture hash differs"
            );
            let value: Value = serde_json::from_str(raw)?;
            ensure!(
                value["id"] == id && value["hash"] == hash,
                "Frozen fixture embedded identity differs"
            );
            let plan = nw_domain::plans::decode(value)?;
            ensure!(
                nw_domain::plans::digest(&plan)? == hash,
                "Current encoder differs from independently frozen legacy digest"
            );
            let key = format!("plan/{id}");
            ensure!(
                table.get(key.as_str())?.is_none(),
                "Fixture row already exists"
            );
            table.insert(key.as_str(), raw.as_bytes())?;
            evidence.record(json!({"phase":"synthetic-storage-fixture","id":id,"raw_bytes":raw.len(),"raw_sha256":raw_sha,"preimage_bytes":preimage.len(),"plan_hash":hash,"v1_scope":"Independent empty-hash struct-order digest; API v1 only decodes and echoes","media_execution":false}))?;
        }
    }
    write.commit()?;
    Ok(())
}
async fn inspect_plans(owner: &mut Owner, evidence: &Evidence) -> Result<()> {
    for (id, _, _, _, hash) in frozen_plans() {
        let response = owner
            .command(&query("plans.inspect", json!({"id":id})), evidence)
            .await?;
        ensure!(
            response["result"]["id"] == id && response["result"]["hash"] == hash,
            "Actual binary plan shape/echo differs from frozen fixture"
        );
    }
    Ok(())
}

struct Project {
    id: String,
    bin: String,
    sequence: String,
    track: String,
    source: String,
    left: String,
    right: String,
    accepted: Vec<(Value, Value)>,
}
fn retain_response(directory: &Path, response: &Value) -> Result<()> {
    let id = text(response, "request_id")?;
    ensure!(
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "Fixture response filename is not bounded ASCII"
    );
    security::write_private(
        &directory.join(format!("{id}-response.json")),
        &serde_json::to_vec(response)?,
        false,
    )?;
    Ok(())
}
fn retain_request(directory: &Path, request: &Value) -> Result<()> {
    let id = text(request, "request_id")?;
    ensure!(
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "Fixture request filename is not bounded ASCII"
    );
    security::write_private(
        &directory.join(format!("{id}.json")),
        &serde_json::to_vec(request)?,
        false,
    )?;
    Ok(())
}
async fn project_write(
    owner: &mut Owner,
    request: Value,
    expected: u64,
    directory: &Path,
    evidence: &Evidence,
    accepted: &mut Vec<(Value, Value)>,
) -> Result<Value> {
    retain_request(directory, &request)?;
    let response = owner.command(&request, evidence).await?;
    let expected_revision = expected.to_string();
    ensure!(
        response["result"]["revision"].as_str() == Some(expected_revision.as_str())
            && response["receipt_id"].is_string(),
        "Actual project mutation revision/receipt differs"
    );
    ensure!(
        accepted
            .iter()
            .all(|(_, prior)| prior["receipt_id"] != response["receipt_id"]),
        "Fresh project mutation reused an earlier receipt ID"
    );
    if expected > 0 {
        let expected_base_revision = (expected - 1).to_string();
        ensure!(
            response["result"]["base_revision"].as_str() == Some(expected_base_revision.as_str()),
            "Project mutation base revision differs"
        );
    }
    same_receipt(&response, &owner.command(&request, evidence).await?)?;
    retain_response(directory, &response)?;
    accepted.push((request, response.clone()));
    Ok(response["result"].clone())
}
async fn create_project(
    owner: &mut Owner,
    scope: &str,
    directory: &Path,
    evidence: &Evidence,
) -> Result<Project> {
    let mut accepted = Vec::new();
    let create = project_write(
        owner,
        durable(
            scope,
            "compat-project-create",
            "projects.create",
            json!({"name":"Compatibility project"}),
            None,
        ),
        0,
        directory,
        evidence,
        &mut accepted,
    )
    .await?;
    let id = text(&create, "project_id")?;
    let bin = text(&create, "root_bin_id")?;
    let sequence = project_write(owner, durable(scope, "compat-sequence-create", "sequences.create", json!({"name":"Sequence","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"}]}), Some((&id, 0))), 1, directory, evidence, &mut accepted).await?;
    let track = text(&sequence["created"]["tracks"][0], "id")?;
    let sequence = text(&sequence["created"], "sequence_id")?;
    let place = project_write(owner, durable(scope, "compat-clip-place", "clips.place", json!({"sequence_id":sequence,"track_id":track,"source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"4","den":"1"}},"sequence_start":{"num":"0","den":"1"},"collision":"reject"}), Some((&id, 1))), 2, directory, evidence, &mut accepted).await?;
    let source = text(&place["created"], "source_id")?;
    let left = place["created"]["clip_ids"][0]
        .as_str()
        .context("Placed clip ID")?
        .to_owned();
    let split = project_write(
        owner,
        durable(
            scope,
            "compat-clip-split",
            "clips.split",
            json!({"clip_id":left,"at_sequence":{"num":"2","den":"1"},"link_policy":"expand"}),
            Some((&id, 2)),
        ),
        3,
        directory,
        evidence,
        &mut accepted,
    )
    .await?;
    let right = split["created"]["clip_ids"][0]
        .as_str()
        .context("Split right clip ID")?
        .to_owned();
    ensure!(left != right, "Split must create a distinct right clip");
    for (command, key, before, after) in [
        ("history.undo", "compat-undo-split", 3, 4),
        ("history.redo", "compat-redo-split", 4, 5),
    ] {
        project_write(
            owner,
            durable(scope, key, command, json!({}), Some((&id, before))),
            after,
            directory,
            evidence,
            &mut accepted,
        )
        .await?;
    }
    evidence.record(json!({"phase":"actual-project-created","project_id":id,"root_bin_id":bin,"sequence_id":sequence,"track_id":track,"source_id":source,"clip_ids":[left,right],"revision":"5","registered_media":false}))?;
    Ok(Project {
        id,
        bin,
        sequence,
        track,
        source,
        left,
        right,
        accepted,
    })
}
async fn project_projection(
    owner: &mut Owner,
    project: &Project,
    command: &str,
    mut params: Value,
    replacement: &mut Option<String>,
    revision: u64,
    evidence: &Evidence,
) -> Result<Value> {
    if let Some(id) = replacement.take() {
        params["replace_snapshot"] = json!(id);
    }
    let mut request = query(command, params);
    request["project"] = json!({"id":project.id}); // expected_revision is deliberately absent, never null.
    let response = owner.command(&request, evidence).await?;
    let result = response["result"].clone();
    let expected_revision = revision.to_string();
    ensure!(
        result["project_revision"].as_str() == Some(expected_revision.as_str()),
        "Reopened project projection has wrong revision"
    );
    *replacement = Some(text(&result, "snapshot_id")?);
    Ok(result)
}
async fn semantic_project(
    owner: &mut Owner,
    project: &Project,
    revision: u64,
    evidence: &Evidence,
) -> Result<Value> {
    let mut replacement = None;
    let summary = project_projection(
        owner,
        project,
        "projects.inspect",
        json!({"projection":"summary"}),
        &mut replacement,
        revision,
        evidence,
    )
    .await?;
    ensure!(
        summary["project_id"] == project.id
            && summary["root_bin_id"] == project.bin
            && summary["name"] == "Compatibility project"
            && summary["entity_count"] == 6
            && summary["sequence_count"] == 1
            && summary["history"]["can_undo"] == true
            && summary["history"]["can_redo"] == false,
        "Reopened project header/history differs"
    );
    let mut entities = BTreeMap::new();
    for (id, kind) in [
        (&project.bin, "bin"),
        (&project.sequence, "sequence"),
        (&project.track, "video"),
        (&project.source, "source_binding"),
        (&project.left, "clip"),
        (&project.right, "clip"),
    ] {
        let result = project_projection(
            owner,
            project,
            "projects.inspect",
            json!({"projection":"entity","entity_id":id}),
            &mut replacement,
            revision,
            evidence,
        )
        .await?;
        ensure!(
            result["entity"]["id"] == *id && result["entity"]["kind"] == kind,
            "Reopened entity identity/kind differs"
        );
        entities.insert(id.clone(), result["entity"].clone());
    }
    ensure!(
        entities[&project.track]["sequence_id"] == project.sequence,
        "Reopened track belongs to a different sequence"
    );
    let offset = if revision == 6 { 1 } else { 0 };
    ensure!(
        entities[&project.left]["sequence_start"] == json!({"num":offset.to_string(),"den":"1"})
            && entities[&project.right]["sequence_start"]
                == json!({"num":(2 + offset).to_string(),"den":"1"}),
        "Actual split/move positions differ"
    );
    for id in [&project.left, &project.right] {
        ensure!(
            entities[id]["duration"] == json!({"num":"2","den":"1"})
                && entities[id]["source_id"] == project.source,
            "Split source/duration differs"
        );
    }
    ensure!(
        entities[&project.left]["source_range"]
            == json!({"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}})
            && entities[&project.right]["source_range"]
                == json!({"start":{"num":"2","den":"1"},"end":{"num":"4","den":"1"}}),
        "Split source ranges differ"
    );
    ensure!(
        entities[&project.source]["source_kind"] == "solid_color"
            && entities[&project.source]["duration"] == json!({"num":"4","den":"1"})
            && entities[&project.track]["grid"] == json!({"num":"4","den":"1"}),
        "Finite generator or explicit track grid differs"
    );
    let bins = project_projection(
        owner,
        project,
        "projects.inspect",
        json!({"projection":"bin_page","bin_id":project.bin,"limit":128}),
        &mut replacement,
        revision,
        evidence,
    )
    .await?;
    let tracks = project_projection(
        owner,
        project,
        "sequences.inspect",
        json!({"sequence_id":project.sequence,"projection":"track_page","limit":128}),
        &mut replacement,
        revision,
        evidence,
    )
    .await?;
    let viewport = project_projection(owner, project, "sequences.inspect", json!({"sequence_id":project.sequence,"projection":"viewport","track_ids":[project.track],"range":{"start":{"num":"0","den":"1"},"end":{"num":"6","den":"1"}},"limit":128}), &mut replacement, revision, evidence).await?;
    ensure!(
        bins["next_cursor"].is_null()
            && tracks["next_cursor"].is_null()
            && viewport["next_cursor"].is_null(),
        "Finite fixture unexpectedly requires more pages"
    );
    ensure!(
        bins["items"].as_array().is_some_and(|items| items
            .iter()
            .any(|item| item["id"] == project.sequence)
            && items.iter().any(|item| item["id"] == project.source)),
        "Project bin membership missing"
    );
    ensure!(
        tracks["items"]
            .as_array()
            .is_some_and(|items| items.len() == 1
                && items[0]["id"] == project.track
                && items[0]["clip_count"] == 2),
        "Track index projection differs"
    );
    ensure!(
        viewport["items"]
            .as_array()
            .is_some_and(|items| items.len() == 2
                && items.iter().any(|item| item["id"] == project.left)
                && items.iter().any(|item| item["id"] == project.right)),
        "Clip range projection differs"
    );
    let stable = json!({"project_id":project.id,"root_bin_id":project.bin,"revision":revision.to_string(),"history":summary["history"],"entities":entities,"bin_items":bins["items"],"track_items":tracks["items"],"clip_items":viewport["items"]});
    evidence.record(json!({"phase":"semantic-project","binary":owner.binary.label,"project_id":project.id,"revision":revision.to_string(),"stable_projection_sha256":sha(&serde_json::to_vec(&stable)?)}))?;
    Ok(stable)
}
async fn replay_project(owner: &mut Owner, project: &Project, evidence: &Evidence) -> Result<()> {
    for (request, response) in &project.accepted {
        same_receipt(response, &owner.command(request, evidence).await?)?;
    }
    Ok(())
}
async fn legacy_reads(
    owner: &mut Owner,
    original: &Value,
    accepted: &Value,
    evidence: &Evidence,
) -> Result<()> {
    owner
        .command(&query("commands.list", json!({})), evidence)
        .await?;
    let profiles = owner
        .command(&query("access.profiles.list", json!({})), evidence)
        .await?;
    ensure!(
        profiles["result"].as_array().is_some_and(|items| items
            .iter()
            .filter(|profile| profile["name"] == "compat-legacy")
            .count()
            == 1),
        "Actual prior-born profile missing or duplicated"
    );
    inspect_plans(owner, evidence).await?;
    same_receipt(accepted, &owner.command(original, evidence).await?)
}
async fn migration_cli(
    owner: &mut Owner,
    binary: &Binary,
    root: &Path,
    plan_file: &Path,
    scope: &str,
    evidence: &Evidence,
) -> Result<Value> {
    let args = [
        "maintenance",
        "migrate",
        "--plan-file",
        plan_file.to_str().context("Fixture plan path encoding")?,
        "--replay-scope",
        scope,
        "--idempotency-key",
        "compat-old-copy",
        "--request-id",
        "compat-old-copy",
    ];
    let (response, code, _) = cli(
        owner,
        binary,
        root,
        &args,
        evidence,
        "maintenance migrate [retained private plan/scope/identity]",
    )
    .await?;
    ensure!(
        code == 0 && response["ok"] == true && response["receipt_id"].is_string(),
        "Actual typed migration acceptance/replay failed"
    );
    Ok(response)
}

async fn scenario(evidence: &Evidence) -> Result<()> {
    let configuration = configuration(evidence)?;
    let temporary = tempfile::tempdir()?;
    let root = security::private_directory(&temporary.path().join("compatibility-root"))?;
    let root = fs::canonicalize(root)?;
    ensure!(
        !root.starts_with(
            configuration
                .old
                .path
                .parent()
                .context("Prior executable parent")?
        ) && !root.starts_with(
            configuration
                .new
                .path
                .parent()
                .context("Current executable parent")?
        ),
        "Fixture root must be outside executable directories"
    );
    let requests = security::private_directory(&root.join("requests"))?;
    let mut epochs = BTreeSet::new();
    let mut root_id = None;

    evidence.record(json!({"phase":"prior-born-profile"}))?;
    let mut owner = Owner::start(
        &configuration.old,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    let old_registry = owner.record.registry.clone();
    let issued = owner
        .command(&query("access.replay_scope.create", json!({})), evidence)
        .await?;
    let scope = text(&issued["result"], "replay_scope")?;
    let original = durable(
        &scope,
        "compat-old-profile",
        "access.profiles.create",
        json!({"name":"compat-legacy","read_roots":[],"write_roots":[]}),
        None,
    );
    retain_request(&requests, &original)?;
    let accepted = owner.command(&original, evidence).await?;
    retain_response(&requests, &accepted)?;
    same_receipt(&accepted, &owner.command(&original, evidence).await?)?;
    owner.stop(evidence).await?;
    let prior = closed(&root, "prior-before-fixtures", evidence)?;
    ensure!(
        prior.facts.store_version == 2
            && prior.rows.contains_key("profile/compat-legacy")
            && prior.rows.keys().any(|key| key.starts_with("receipt/")),
        "Prior executable did not create schema2 profile/receipt facts"
    );
    insert_storage_fixtures(&root, evidence)?;
    let original_raw = closed(&root, "O-before-any-current-startup", evidence)?;
    preserved(&prior, &original_raw, legacy_key)?;
    for (id, raw, _, _, _) in frozen_plans() {
        ensure!(
            original_raw
                .rows
                .get(&format!("plan/{id}"))
                .is_some_and(|bytes| bytes == raw.as_bytes()),
            "Original frozen fixture bytes differ"
        );
    }
    evidence.record(json!({"phase":"original-coverage","old_receipts":original_raw.rows.keys().filter(|key| key.starts_with("receipt/")).count(),"plans":original_raw.rows.keys().filter(|key| key.starts_with("plan/")).count(),"batch_plans":original_raw.rows.keys().filter(|key| key.starts_with("batch-plan/")).count(),"batch_plan_decoder_exercised":false}))?;

    evidence.record(json!({"phase":"current-actual-project"}))?;
    let mut owner = Owner::start(
        &configuration.new,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    ensure!(
        owner.record.registry != old_registry,
        "Current project registry must differ from prior binary registry"
    );
    legacy_reads(&mut owner, &original, &accepted, evidence).await?;
    let mut project = create_project(&mut owner, &scope, &requests, evidence).await?;
    ensure!(
        project
            .accepted
            .iter()
            .all(|(_, response)| response["receipt_id"] != accepted["receipt_id"]),
        "Project mutation reused prior profile receipt ID"
    );
    let semantic_a = semantic_project(&mut owner, &project, 5, evidence).await?;
    owner.stop(evidence).await?;
    let a = closed(&root, "A-current-project-schema2", evidence)?;
    ensure!(
        a.facts.store_version == 2,
        "Current startup silently changed legacy schema"
    );
    preserved(&original_raw, &a, legacy_key)?;
    for namespace in POPULATED_PROJECT_NAMESPACES {
        ensure!(
            a.facts
                .namespaces
                .get(*namespace)
                .is_some_and(|count| *count > 0),
            "Actual project fixture did not populate {namespace}"
        );
    }
    let dependency_rows = a
        .facts
        .namespaces
        .get("project-dependency")
        .copied()
        .unwrap_or(0);
    ensure!(
        dependency_rows == 0,
        "Unlinked project fixture unexpectedly populated dependency rows"
    );
    evidence.record(json!({"phase":"project-namespace-coverage","checkpoint":"A","project_namespaces_required_nonempty":POPULATED_PROJECT_NAMESPACES,"project_dependency_rows":dependency_rows,"nonempty_dependency_preservation":"unqualified","required_before_full_v1":"Extend actual prior/current preservation through a real public link/import producer"}))?;
    ensure!(
        a.rows
            .keys()
            .filter(|key| key.starts_with("receipt/"))
            .count()
            >= 7,
        "Project durable receipts missing from closed A"
    );

    evidence.record(json!({"phase":"prior-reads-with-current-project"}))?;
    let mut owner = Owner::start(
        &configuration.old,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    ensure!(
        owner.record.registry == old_registry,
        "Prior registry changed across restarts"
    );
    legacy_reads(&mut owner, &original, &accepted, evidence).await?;
    let (_, conflict_code, conflict_stderr) = cli(
        &mut owner,
        &configuration.new,
        &root,
        &["commands", "list"],
        evidence,
        "commands list (deliberate cross-build attachment)",
    )
    .await?;
    ensure!(
        conflict_code == 4
            && String::from_utf8_lossy(&conflict_stderr.bytes).contains("OWNER_VERSION_CONFLICT"),
        "Cross-build owner attachment did not fail with OWNER_VERSION_CONFLICT/4"
    );
    owner.health_unchanged().await?;
    let still_owned: OwnerRecord = serde_json::from_slice(&security::read_private(
        &root.join("runtime/owner.json"),
        16384,
    )?)?;
    ensure!(
        still_owned.epoch == owner.record.epoch
            && still_owned.build == owner.record.build
            && still_owned.root_id == owner.record.root_id,
        "Conflicting client replaced private owner identity"
    );
    owner.stop(evidence).await?;
    let b = closed(&root, "B-prior-read-preservation", evidence)?;
    preserved(&original_raw, &b, legacy_key)?;
    projects_identical(&a, &b)?;

    evidence.record(json!({"phase":"actual-prior-migration"}))?;
    let mut owner = Owner::start(
        &configuration.old,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    // Scope issuance changes clock/quota metadata, so it MUST precede review.
    let issued = owner
        .command(&query("access.replay_scope.create", json!({})), evidence)
        .await?;
    let migration_scope = text(&issued["result"], "replay_scope")?;
    let (reviewed, code, _) = cli(
        &mut owner,
        &configuration.old,
        &root,
        &["maintenance", "plan"],
        evidence,
        "maintenance plan (schema2 prior copy)",
    )
    .await?;
    ensure!(
        code == 0 && reviewed["ok"] == true,
        "Actual prior migration review failed"
    );
    let plan = reviewed["result"].clone();
    ensure!(
        plan["application_build"] == configuration.old.hash()?
            && plan["source_store_version"] == 2
            && plan["target_store_version"] == 3
            && plan["root_id"].as_str() == root_id.as_deref(),
        "Prior migration plan identities/versions differ"
    );
    let plan_file = requests.join("compat-old-migration-plan.json");
    security::write_private(&plan_file, &serde_json::to_vec(&plan)?, false)?;
    let migration_request = durable(
        &migration_scope,
        "compat-old-copy",
        "maintenance.migrate",
        json!({"plan":plan}),
        None,
    );
    retain_request(&requests, &migration_request)?;
    let migration_accepted = migration_cli(
        &mut owner,
        &configuration.old,
        &root,
        &plan_file,
        &migration_scope,
        evidence,
    )
    .await?;
    retain_response(&requests, &migration_accepted)?;
    ensure!(
        migration_accepted["receipt_id"] != accepted["receipt_id"]
            && project
                .accepted
                .iter()
                .all(|(_, response)| response["receipt_id"] != migration_accepted["receipt_id"]),
        "Fresh migration reused an existing business receipt ID"
    );
    ensure!(
        migration_accepted["result"]["state"] == "accepted",
        "Migration submit did not acknowledge acceptance"
    );
    let migration_id = text(&migration_accepted["result"], "migration_id")?;
    ensure!(
        plan["id"] == migration_id,
        "Accepted migration changed reviewed identity"
    );
    same_receipt(
        &migration_accepted,
        &migration_cli(
            &mut owner,
            &configuration.old,
            &root,
            &plan_file,
            &migration_scope,
            evidence,
        )
        .await?,
    )?;
    let (inspected, code, _) = cli(
        &mut owner,
        &configuration.old,
        &root,
        &["maintenance", "inspect", &migration_id],
        evidence,
        "maintenance inspect",
    )
    .await?;
    ensure!(
        code == 0
            && inspected["ok"] == true
            && inspected["result"]["phase"] == "succeeded"
            && inspected["receipt_id"].is_null(),
        "Migration inspect must separately prove completion without inventing a receipt"
    );
    owner.stop(evidence).await?;
    let c = closed(&root, "C-prior-migrated-selected-target", evidence)?;
    ensure!(
        c.facts.store_version == 3 && c.activation.is_some(),
        "Prior migration did not select schema3 target"
    );
    preserved(&original_raw, &c, legacy_key)?;
    projects_identical(&a, &c)?;
    guard(&c, &migration_id, "0")?;
    let rollback_c = rollback(&root, &plan, &original_raw, &a, "C-rollback", evidence)?;

    evidence.record(json!({"phase":"current-schema3-replay-and-adoption"}))?;
    let mut owner = Owner::start(
        &configuration.new,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    legacy_reads(&mut owner, &original, &accepted, evidence).await?;
    replay_project(&mut owner, &project, evidence).await?;
    same_receipt(
        &migration_accepted,
        &owner.command(&migration_request, evidence).await?,
    )?;
    same_receipt(
        &migration_accepted,
        &migration_cli(
            &mut owner,
            &configuration.new,
            &root,
            &plan_file,
            &migration_scope,
            evidence,
        )
        .await?,
    )?;
    let reopened = semantic_project(&mut owner, &project, 5, evidence).await?;
    ensure!(
        reopened == semantic_a,
        "Current schema3 reopen changed project entities/history"
    );
    let edit = durable(
        &scope,
        "compat-adoption-move",
        "clips.move",
        json!({"clip_ids":[project.left,project.right],"delta":{"num":"1","den":"1"},"collision":"reject","link_policy":"expand"}),
        Some((&project.id, 5)),
    );
    project_write(
        &mut owner,
        edit,
        6,
        &requests,
        evidence,
        &mut project.accepted,
    )
    .await?;
    let moved = &project.accepted.last().context("Adoption receipt")?.1;
    ensure!(
        moved["receipt_id"] != migration_accepted["receipt_id"]
            && moved["receipt_id"] != accepted["receipt_id"],
        "Fresh adoption edit reused an old receipt ID"
    );
    let semantic_d = semantic_project(&mut owner, &project, 6, evidence).await?;
    owner.stop(evidence).await?;
    let d = closed(&root, "D-current-business-adoption", evidence)?;
    ensure!(
        d.facts
            .namespaces
            .get("project-dependency")
            .copied()
            .unwrap_or(0)
            == 0,
        "Unlinked project move unexpectedly populated dependency rows"
    );
    preserved(&original_raw, &d, legacy_key)?;
    preserved(&c, &d, |key| key.starts_with("receipt/"))?;
    let guard_d = guard(&d, &migration_id, "1")?;
    let rollback_d = rollback(&root, &plan, &original_raw, &a, "D-rollback", evidence)?;
    same_rollback(&rollback_c, &rollback_d)?;

    evidence.record(json!({"phase":"prior-schema3-reader-and-replay"}))?;
    let mut owner = Owner::start(
        &configuration.old,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    legacy_reads(&mut owner, &original, &accepted, evidence).await?;
    same_receipt(
        &migration_accepted,
        &migration_cli(
            &mut owner,
            &configuration.old,
            &root,
            &plan_file,
            &migration_scope,
            evidence,
        )
        .await?,
    )?;
    let (not_required, code, _) = cli(
        &mut owner,
        &configuration.old,
        &root,
        &["maintenance", "plan"],
        evidence,
        "maintenance plan (schema3 no-migration)",
    )
    .await?;
    ensure!(
        code == 3
            && not_required["ok"] == false
            && not_required["error"]["code"] == "MIGRATION_NOT_REQUIRED",
        "Actual prior schema3 maintenance result differs"
    );
    owner.stop(evidence).await?;
    let e = closed(&root, "E-prior-schema3-preservation", evidence)?;
    preserved(&original_raw, &e, legacy_key)?;
    projects_identical(&d, &e)?;
    ensure!(
        guard(&e, &migration_id, "1")? == guard_d,
        "Prior reader/replay changed adoption bytes"
    );
    let rollback_e = rollback(&root, &plan, &original_raw, &a, "E-rollback", evidence)?;
    same_rollback(&rollback_c, &rollback_e)?;

    evidence.record(json!({"phase":"final-current-reopen"}))?;
    let mut owner = Owner::start(
        &configuration.new,
        &root,
        &mut epochs,
        &mut root_id,
        evidence,
    )
    .await?;
    legacy_reads(&mut owner, &original, &accepted, evidence).await?;
    replay_project(&mut owner, &project, evidence).await?;
    same_receipt(
        &migration_accepted,
        &owner.command(&migration_request, evidence).await?,
    )?;
    ensure!(
        semantic_project(&mut owner, &project, 6, evidence).await? == semantic_d,
        "Final current semantic project differs after prior schema3 reader"
    );
    owner.stop(evidence).await?;
    let final_raw = closed(&root, "final-current-replay-preservation", evidence)?;
    preserved(&original_raw, &final_raw, legacy_key)?;
    projects_identical(&d, &final_raw)?;
    ensure!(
        guard(&final_raw, &migration_id, "1")? == guard_d,
        "Final replay/reopen changed adoption bytes"
    );
    let rollback_final = rollback(&root, &plan, &original_raw, &a, "final-rollback", evidence)?;
    same_rollback(&rollback_c, &rollback_final)?;
    ensure!(
        epochs.len() == 7,
        "Every controlled owner must establish a new epoch"
    );
    evidence.record(json!({"phase":"complete","status":"passed","mode":"additive-preservation","owner_epochs":epochs.len(),"root_id":root_id,"project_id":project.id,"final_revision":"6","migration_id":migration_id,"project_namespaces_required_nonempty":POPULATED_PROJECT_NAMESPACES,"project_dependency_rows":final_raw.facts.namespaces.get("project-dependency").copied().unwrap_or(0),"nonempty_dependency_preservation":"unqualified","required_before_full_v1":"Extend actual prior/current preservation through a real public link/import producer","legacy_v1":"shape/echo plus independently frozen digest and exact raw preservation","legacy_v2":"actual decoder acceptance plus independently frozen semantic digest and exact raw preservation","registered_media":false}))?;
    Ok(())
}
pub async fn run() {
    if std::env::var("NW_COMPAT_REQUIRED").as_deref() != Ok("1") {
        return;
    }
    // This fixed fallback preserves a failure record even if preparation/configuration
    // failed before it could create files. Required=1 is set by the workflow itself.
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/project-compatibility/evidence");
    fs::create_dir_all(&directory).expect("Create sanitized compatibility evidence directory");
    let evidence = Evidence {
        path: directory.join("preservation.json"),
        events: Mutex::new(Vec::new()),
        started: Instant::now(),
    };
    evidence
        .record(json!({"phase":"started","status":"running","deadline_seconds":600}))
        .expect("Write compatibility start evidence");
    let outcome = tokio::time::timeout(Duration::from_secs(600), scenario(&evidence)).await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            evidence.record(json!({"phase":"failed","status":"failed","reason":"Actual required package preservation assertion failed; last sanitized event identifies boundary"})).expect("Write compatibility failure evidence");
            panic!("Actual prior/current package compatibility failed: {error:#}");
        }
        Err(_) => {
            evidence.record(json!({"phase":"failed","status":"failed","reason":"600-second preservation deadline; retained owned children are terminated only for cleanup; no dirty reopen is attempted"})).expect("Write compatibility deadline evidence");
            panic!("Actual prior/current package preservation deadline");
        }
    }
}
