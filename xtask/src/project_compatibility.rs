//! Hosted preparation for the actual immutable previous/current package test.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn digest(path: &Path) -> Result<Value> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "Executable must be an ordinary file"
    );
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes = bytes
            .checked_add(count as u64)
            .context("Executable byte count")?;
    }
    Ok(json!({"bytes":bytes,"sha256":hex::encode(hash.finalize())}))
}
fn json_file(path: &Path) -> Result<Value> {
    ensure!(
        fs::metadata(path)?.len() <= 8 * 1024 * 1024,
        "Preparation JSON exceeds bound"
    );
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn write(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn resolved_destination(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "Compatibility paths must be absolute");
    let mut existing = path.to_owned();
    let mut suffix = Vec::new();
    while !existing.try_exists()? {
        ensure!(suffix.len() < 64, "Compatibility path depth exceeds bound");
        suffix.push(
            existing
                .file_name()
                .context("Compatibility path ancestor")?
                .to_owned(),
        );
        ensure!(
            existing.pop(),
            "Compatibility path has no existing ancestor"
        );
    }
    // Canonicalize the existing ancestor even before current/ exists. This
    // also reconciles Windows drive paths with canonical verbatim prefixes.
    let mut resolved = fs::canonicalize(existing)?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}
fn configured(name: &str, expected: &Path) -> Result<PathBuf> {
    let path =
        PathBuf::from(std::env::var_os(name).context("Required compatibility path missing")?);
    let resolved = resolved_destination(&path)?;
    ensure!(
        resolved == resolved_destination(expected)?,
        "Compatibility path differs from fixed workspace location"
    );
    Ok(resolved)
}
fn drain(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut saved = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let keep = count.min((2 * 1024 * 1024usize).saturating_sub(saved.len()));
        saved.extend_from_slice(&buffer[..keep]);
    }
    Ok(saved)
}
// gh is installed on the selected hosted images; its token remains in GH_TOKEN.
// No shell, token argument, raw stderr or credential-bearing response is logged.
fn api(endpoint: &str) -> Result<Value> {
    let mut child = Command::new("gh")
        .args(["api", "--method", "GET", endpoint])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().context("Metadata stdout")?;
    let stderr = child.stderr.take().context("Metadata stderr")?;
    let out = std::thread::spawn(move || drain(stdout));
    let err = std::thread::spawn(move || drain(stderr));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() > Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let bytes = out
        .join()
        .map_err(|_| anyhow::anyhow!("Metadata reader stopped"))??;
    let _private_diagnostics = err
        .join()
        .map_err(|_| anyhow::anyhow!("Metadata reader stopped"))??;
    ensure!(
        status.is_some_and(|status| status.success()),
        "Immutable baseline metadata unavailable or request failed"
    );
    ensure!(
        bytes.len() < 2 * 1024 * 1024,
        "Baseline metadata exceeds bound"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn baseline_metadata(manifest: &Value, target: &Value, evidence: &Path) -> Result<()> {
    let repository = manifest["repository"]
        .as_str()
        .context("Baseline repository")?;
    ensure!(
        repository == "KiaroSama/Numera-Wiz",
        "Unexpected baseline repository"
    );
    let id = manifest["run_id"].as_u64().context("Baseline run")?;
    let attempt = manifest["run_attempt"]
        .as_u64()
        .context("Baseline attempt")?;
    let run = api(&format!(
        "repos/{repository}/actions/runs/{id}/attempts/{attempt}"
    ))?;
    ensure!(
        run["id"] == id && run["run_attempt"] == attempt,
        "Baseline run/attempt differs"
    );
    ensure!(
        run["path"] == manifest["workflow_path"]
            && run["status"] == "completed"
            && run["conclusion"] == "success",
        "Baseline workflow did not complete successfully"
    );
    // A PR event may name a synthetic merge; its declared head must be exact.
    let source = &manifest["source_sha"];
    let source_matches = run["head_sha"] == *source
        || run["pull_requests"]
            .as_array()
            .is_some_and(|prs| prs.iter().any(|pr| pr["head"]["sha"] == *source));
    ensure!(source_matches, "Baseline source identity differs");
    let jobs = api(&format!(
        "repos/{repository}/actions/runs/{id}/attempts/{attempt}/jobs?per_page=100"
    ))?;
    let rows = jobs["jobs"].as_array().context("Baseline jobs")?;
    ensure!(
        jobs["total_count"] == 13 && rows.len() == 13,
        "Baseline requires all thirteen expanded jobs"
    );
    ensure!(
        rows.iter().all(|job| job["status"] == "completed"
            && job["conclusion"] == "success"
            && job["run_id"] == id),
        "Baseline jobs are incomplete or failed"
    );
    let expected = [
        "dependency-lock",
        "formatting",
        "web",
        "browser",
        "native-ui",
        "checks (ubuntu-24.04)",
        "checks (windows-2025)",
        "checks (macos-15)",
        "checks (macos-15-intel)",
        "media",
        "owner-batch",
        "owner-transports",
        "artifact-http",
    ];
    ensure!(
        expected
            .iter()
            .all(|name| rows.iter().filter(|job| job["name"] == *name).count() == 1),
        "Baseline job inventory differs"
    );
    let artifact_id = target["native_artifact_id"]
        .as_u64()
        .context("Native artifact ID")?;
    let artifact = api(&format!(
        "repos/{repository}/actions/artifacts/{artifact_id}"
    ))?;
    ensure!(
        artifact["id"] == artifact_id
            && artifact["name"] == target["native_artifact"]
            && artifact["expired"] == false
            && artifact["workflow_run"]["id"] == id,
        "Immutable native artifact expired or changed identity"
    );
    write(
        &evidence.join("baseline-run.json"),
        &json!({
            "id":id,"attempt":attempt,"source_sha":source,"event_head_sha":run["head_sha"],"workflow":run["path"],"conclusion":run["conclusion"],
            "jobs":rows.iter().map(|job| json!({"id":job["id"],"name":job["name"],"status":job["status"],"conclusion":job["conclusion"],"head_sha":job["head_sha"]})).collect::<Vec<_>>(),
            "native_artifact":{"id":artifact_id,"name":artifact["name"],"expired":artifact["expired"]}
        }),
    )
}
fn inner(root: &Path, evidence: &Path) -> Result<Value> {
    ensure!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("NW_COMPAT_REQUIRED").as_deref() == Ok("1"),
        "Compatibility preparation belongs in required hosted checks"
    );
    let source = std::env::var("NW_SOURCE_SHA").context("Current source SHA")?;
    ensure!(
        source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid current source identity"
    );
    let label = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("windows", "x86_64") => "windows-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        _ => bail!("Unsupported compatibility target"),
    };
    let executable = if cfg!(windows) {
        "numera-wiz.exe"
    } else {
        "numera-wiz"
    };
    let workspace = root.join("target/project-compatibility");
    let previous = configured(
        "NW_COMPAT_PREVIOUS_EXE",
        &workspace.join("previous").join(executable),
    )?;
    let current = configured(
        "NW_COMPAT_CURRENT_EXE",
        &workspace.join("current").join(executable),
    )?;
    let manifest_path = configured(
        "NW_COMPAT_BASELINE_MANIFEST",
        &root.join("ci/project-compatibility-baseline.json"),
    )?;
    let manifest = json_file(&manifest_path)?;
    let targets = manifest["targets"]
        .as_array()
        .context("Baseline target inventory")?;
    let matches = targets
        .iter()
        .filter(|entry| entry["target"] == label)
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "Baseline target must be unique");
    let target = matches[0];
    baseline_metadata(&manifest, target, evidence)?;
    let previous_identity = digest(&previous)?;
    ensure!(
        previous_identity["bytes"] == target["executable_bytes"]
            && previous_identity["sha256"] == target["executable_sha256"],
        "Downloaded prior executable differs from independently frozen identity"
    );
    let name = format!(
        "numera-wiz-{}-{label}-{}-provenance.json",
        env!("CARGO_PKG_VERSION"),
        &source[..12]
    );
    let provenance_path = root.join("target/development-packages").join(name);
    let provenance = json_file(&provenance_path)?;
    let attempt = std::env::var("GITHUB_RUN_ATTEMPT")?;
    let run_id = std::env::var("GITHUB_RUN_ID")?;
    let repository = std::env::var("GITHUB_REPOSITORY")?;
    let run_url = format!("https://github.com/{repository}/actions/runs/{run_id}");
    ensure!(
        provenance["source_sha"] == source
            && provenance["target"] == label
            && provenance["run_attempt"] == attempt
            && provenance["run_url"] == run_url,
        "Current package provenance source/target/run differs"
    );
    ensure!(
        provenance["native_features"] == json!(["desktop", "embedded-web"]),
        "Current package feature composition differs"
    );
    let packaged_identity = &provenance["executable"];
    ensure!(
        packaged_identity["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0),
        "Package executable length must be an integer"
    );
    let packaged_path = if cfg!(target_os = "macos") {
        "Numera Wiz.app/Contents/MacOS/numera-wiz"
    } else {
        executable
    };
    let files = provenance["files"]
        .as_array()
        .context("Package file inventory")?;
    let entries = files
        .iter()
        .filter(|entry| entry["path"] == packaged_path)
        .collect::<Vec<_>>();
    ensure!(
        entries.len() == 1
            && entries[0]["bytes"] == packaged_identity["bytes"]
            && entries[0]["sha256"] == packaged_identity["sha256"],
        "Package executable file entry differs"
    );
    let built = root.join("target/debug").join(executable);
    ensure!(
        digest(&built)? == *packaged_identity,
        "Current executable no longer matches native package"
    );
    fs::create_dir_all(current.parent().context("Current executable parent")?)?;
    fs::copy(&built, &current)?;
    let current_identity = digest(&current)?;
    ensure!(
        current_identity == *packaged_identity && digest(&built)? == current_identity,
        "Current executable snapshot changed while copying"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
    }
    ensure!(
        digest(&previous)? == previous_identity && digest(&current)? == current_identity,
        "Verified executable changed during preparation"
    );
    write(
        &evidence.join("current-package-provenance.json"),
        &provenance,
    )?;
    Ok(
        json!({"schema_version":1,"status":"prepared","mode":"additive-preservation","target":label,"source_sha":source,"baseline_source_sha":manifest["source_sha"],"baseline_run":manifest["run_id"],"baseline_attempt":manifest["run_attempt"],"run_url":run_url,"run_attempt":attempt,"previous":previous_identity,"current":current_identity,"manifest_sha256":hex::encode(Sha256::digest(fs::read(&manifest_path)?)),"native_features":provenance["native_features"]}),
    )
}
pub fn prepare(root: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    let evidence = root.join("target/project-compatibility/evidence");
    fs::create_dir_all(&evidence)?;
    write(
        &evidence.join("preparation.json"),
        &json!({"schema_version":1,"status":"started"}),
    )?;
    let result =
        configured("NW_COMPAT_EVIDENCE_DIR", &evidence).and_then(|_| inner(&root, &evidence));
    match result {
        Ok(record) => write(&evidence.join("preparation.json"), &record),
        Err(error) => {
            write(
                &evidence.join("preparation.json"),
                &json!({"schema_version":1,"status":"failed","reason":"Required baseline metadata, immutable executable or current package identity could not be verified"}),
            )?;
            Err(error)
        }
    }
}
