//! Hosted, authenticated metadata capture; deliberately not a toolchain builder.
mod cases;
mod evidence;
mod inner;
mod metadata;
mod process;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CONFIG: &str = include_str!("../../components/linux-toolchain-capture.json");
const IMAGE: &str = "docker.io/library/ubuntu@sha256:78d4744ead1f5b19b04d0df558ab182a5300903900755e515b2ca4d10eb2044d";

pub fn run(root: &Path) -> Result<()> {
    // Validate the complete argument vector before filesystem or native activity.
    let arguments: Vec<_> = std::env::args().skip(2).collect();
    match arguments.as_slice() {
        [selection] if selection == "--self-check" => return cases::run(),
        [mode, seconds]
            if matches!(
                mode.as_str(),
                "--container-supervisor" | "--container-worker"
            ) =>
        {
            let seconds = metadata::decimal(seconds)?;
            ensure!(
                (15..=240).contains(&seconds),
                "invalid private child deadline"
            );
            ensure!(
                std::env::var("NW_TOOLCHAIN_INNER").as_deref() == Ok("1")
                    && std::env::consts::OS == "linux"
                    && std::env::consts::ARCH == "x86_64",
                "private container mode unavailable"
            );
            ensure!(
                std::env::current_exe()? == Path::new("/capture-xtask")
                    && Path::new("/.dockerenv").is_file(),
                "private mode requires the copied executable inside its Docker container"
            );
            let config: Value = serde_json::from_str(CONFIG)?;
            ensure!(
                fs::read("/capture-config.json")? == CONFIG.as_bytes(),
                "copied capture configuration mismatch"
            );
            if mode == "--container-worker" {
                return inner::worker(&config, seconds);
            }
            return supervise_inner(seconds);
        }
        [] => {}
        _ => bail!("capture-linux-toolchain accepts only --self-check or no public options"),
    }
    ensure!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::consts::OS == "linux"
            && std::env::consts::ARCH == "x86_64",
        "actual capture is restricted to hosted Linux x64 CI"
    );
    capture_host(root)
}

fn supervise_inner(seconds: u64) -> Result<()> {
    let mut command = process::command("/capture-xtask");
    command.env("NW_TOOLCHAIN_INNER", "1").args([
        "capture-linux-toolchain",
        "--container-worker",
        &seconds.to_string(),
    ]);
    let result = process::run(
        &mut command,
        Instant::now() + Duration::from_secs(seconds),
        seconds,
        None,
    );
    match result {
        Ok(output) => output.success(),
        Err(error) => Err(error).context(
            "private Rust supervisor terminated worker; incomplete evidence is not a lock",
        ),
    }
}

struct Host {
    output: PathBuf,
    files: Vec<Value>,
    total: u64,
    chunks: usize,
    deadline: Instant,
    diagnostic_failures: Vec<String>,
    sequence: usize,
}

impl Host {
    fn begin_file(&mut self, logical: &str, size: u64, hash: Option<&str>) -> Result<usize> {
        ensure!(self.files.len() < 500, "host evidence file count exceeded");
        metadata::safe_path(logical)?;
        ensure!(
            !self.files.iter().any(|file| file["logical_path"]
                .as_str()
                .is_some_and(|path| path.eq_ignore_ascii_case(logical))),
            "duplicate host evidence path"
        );
        ensure!(
            size <= process::FILE_LIMIT,
            "host evidence file exceeds limit"
        );
        process::check_count(self.total, size, evidence::RAW_LIMIT)?;
        let index = self.files.len();
        self.files.push(json!({"logical_path":logical,"bytes":0,"sha256":metadata::digest(b""),"expected_bytes":size,"expected_sha256":hash,"capture_status":"incomplete","chunks":[]}));
        Ok(index)
    }
    fn append_chunk_with(
        &mut self,
        index: usize,
        path: &str,
        bytes: &[u8],
        hash: &mut Sha256,
        writer: impl FnOnce(&mut fs::File, &[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        ensure!(
            self.chunks < 500 && bytes.len() as u64 <= evidence::CHUNK_LIMIT,
            "host transport file bound exceeded"
        );
        process::check_count(
            self.files[index]["bytes"].as_u64().context("prefix size")?,
            bytes.len() as u64,
            self.files[index]["expected_bytes"]
                .as_u64()
                .context("expected size")?,
        )?;
        self.total = process::check_count(self.total, bytes.len() as u64, evidence::RAW_LIMIT)?;
        self.chunks += 1;
        // The full physical reservation is live before any create/write. The
        // logical row already exists and remains visible on every error exit.
        let outcome = evidence::write_chunk_with(&self.output.join(path), path, bytes, writer);
        match outcome.chunk {
            Some(chunk) => {
                let size = chunk["bytes"].as_u64().context("owned chunk size")?;
                self.total = self
                    .total
                    .checked_sub(bytes.len() as u64 - size)
                    .context("host chunk reservation reconciliation")?;
                let row = &mut self.files[index];
                row["bytes"] = json!(process::check_count(
                    row["bytes"].as_u64().context("retained prefix bytes")?,
                    size,
                    process::FILE_LIMIT
                )?);
                if row["sha256"].is_string()
                    && chunk["sha256"].as_str()
                        == Some(metadata::digest(&bytes[..size as usize]).as_str())
                {
                    hash.update(&bytes[..size as usize]);
                    row["sha256"] = json!(hex::encode(hash.clone().finalize()));
                } else {
                    row["sha256"] = Value::Null;
                    row["integrity"] = json!("UNKNOWN_reserved_upper_bound");
                }
                row["chunks"]
                    .as_array_mut()
                    .context("owned prefix chunks")?
                    .push(chunk);
            }
            None => {
                self.total = self
                    .total
                    .checked_sub(bytes.len() as u64)
                    .context("absent host chunk bytes")?;
                self.chunks -= 1;
            }
        }
        if let Some(error) = outcome.error {
            bail!("{error}");
        }
        Ok(())
    }
    fn complete_file(&mut self, index: usize) -> Result<()> {
        let row = &mut self.files[index];
        ensure!(
            row["bytes"] == row["expected_bytes"] && row["sha256"] == row["expected_sha256"],
            "whole retained file identity mismatch"
        );
        row["capture_status"] = json!("complete");
        Ok(())
    }
    fn record_with(
        &mut self,
        logical: &str,
        bytes: &[u8],
        mut writer: impl FnMut(&mut fs::File, &[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        let index = self.begin_file(logical, bytes.len() as u64, Some(&metadata::digest(bytes)))?;
        let mut hash = Sha256::new();
        for bytes in bytes.chunks(evidence::CHUNK_LIMIT as usize) {
            let path = format!("raw/host-{:04}", self.sequence);
            self.sequence = self
                .sequence
                .checked_add(1)
                .context("host filename sequence overflow")?;
            self.append_chunk_with(index, &path, bytes, &mut hash, &mut writer)?;
        }
        self.complete_file(index)
    }
    fn record(&mut self, logical: &str, bytes: &[u8]) -> Result<()> {
        self.record_with(logical, bytes, |file, bytes| file.write_all(bytes))
    }
    fn docker(&mut self, label: &str, arguments: &[&str], seconds: u64) -> Result<process::Output> {
        let mut command = process::command("/usr/bin/docker");
        command.args(arguments);
        let output = process::run(&mut command, self.deadline, seconds, None)?;
        self.record(&format!("host/commands/{label}.stdout"), &output.stdout)?;
        self.record(&format!("host/commands/{label}.stderr"), &output.stderr)?;
        self.record(
            &format!("host/commands/{label}.status"),
            format!("{}\n", output.status).as_bytes(),
        )?;
        Ok(output)
    }
    fn copy_out(&mut self, container: &str, relative: &str, limit: u64) -> Result<Vec<u8>> {
        metadata::safe_path(relative)?;
        let name = Path::new(relative)
            .file_name()
            .and_then(|n| n.to_str())
            .context("copy evidence basename")?;
        let source = format!("{container}:/capture/out/{relative}");
        let mut command = process::command("/usr/bin/docker");
        command.args(["cp", &source, "-"]);
        let output = process::run(&mut command, self.deadline, 10, None)?;
        if !output.stderr.is_empty() {
            self.record(&format!("host/copy-errors/{relative}"), &output.stderr)?;
        }
        output.success()?;
        metadata::single_tar(&output.stdout, name, limit)
    }
    fn identity(&mut self, label: &str) -> Result<Value> {
        let mut result = json!({});
        for (name, program, arguments) in [
            ("uid", "/usr/bin/id", vec!["-u"]),
            ("gid", "/usr/bin/id", vec!["-g"]),
            ("kernel", "/usr/bin/uname", vec!["-a"]),
            ("utc", "/usr/bin/date", vec!["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        ] {
            let output = process::run(
                process::command(program).args(arguments),
                self.deadline,
                3,
                None,
            )?;
            output.success()?;
            result[name] = json!(output.text()?.trim_end());
        }
        let mut os = Vec::new();
        fs::File::open("/etc/os-release")?
            .take(64 * 1024 + 1)
            .read_to_end(&mut os)?;
        ensure!(os.len() <= 64 * 1024, "host OS metadata too large");
        result["os_release"] = json!(std::str::from_utf8(&os)?);
        self.record(
            &format!("host/{label}.json"),
            &evidence::canonical(&result)?,
        )?;
        Ok(result)
    }
}

fn capture_host(root: &Path) -> Result<()> {
    let started = Instant::now();
    let end = started + Duration::from_secs(300);
    let root = fs::canonicalize(root)?;
    let target = root.join("target");
    if target.exists() {
        ensure!(
            fs::symlink_metadata(&target)?.is_dir()
                && !fs::symlink_metadata(&target)?.file_type().is_symlink(),
            "target must be an owned real directory"
        );
    } else {
        fs::create_dir(&target)?;
    }
    let output = target.join("toolchain-capture");
    fs::create_dir(&output)
        .context("capture requires a fresh target/toolchain-capture directory")?;
    fs::create_dir(output.join("report"))?;
    fs::create_dir(output.join("raw"))?;
    let config: Value = serde_json::from_str(CONFIG)?;
    let mut host = Host {
        output: output.clone(),
        files: Vec::new(),
        total: 0,
        chunks: 0,
        deadline: end - Duration::from_secs(22),
        diagnostic_failures: Vec::new(),
        sequence: 0,
    };
    let mut report = json!({"schema_version":1,"complete_snapshot_lock":false,"lock_scope":"snapshot_package_metadata_only_not_external_runtime_or_standard_profile","source_head":null,"snapshot":metadata::SNAPSHOT,"base":IMAGE,"platform":"linux/amd64","roots":config["roots"],"policy":config["policy"],"limits":config["limits"],"external_build_tool_inputs":config["external_build_tool_inputs"],"runner":{"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"github_actions":true},"unresolved_reasons":[],"cleanup":{"status":"not_created"},"execution":{"install":false,"compiler":false,"ffmpeg":false,"source_bootstrap":false,"package_payloads":"not_downloaded"}});
    for name in [
        "RUNNER_NAME",
        "RUNNER_OS",
        "RUNNER_ARCH",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_JOB",
    ] {
        if let Ok(value) = std::env::var(name) {
            ensure!(value.len() <= 1024, "oversized runner identity");
            report["runner"][name] = json!(value);
        }
    }
    let nonce = format!(
        "nw-toolchain-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let mut cleanup_target = None;
    let result = capture(
        &root,
        &config,
        &nonce,
        &mut host,
        &mut report,
        &mut cleanup_target,
    );
    if let Err(error) = &result {
        report["unresolved_reasons"]
            .as_array_mut()
            .context("unresolved array")?
            .push(json!(format!("{error:#}")));
    }
    host.deadline = end - Duration::from_secs(2);
    if let Some(container) = cleanup_target {
        match cleanup(&mut host, &container, &nonce) {
            Ok(()) => {
                report["cleanup"] = json!({"status":"exact_private_container_removed_and_absence_observed","container":container})
            }
            Err(error) => {
                report["cleanup"] = json!({"status":"unproven","container":container});
                report["unresolved_reasons"]
                    .as_array_mut()
                    .context("unresolved array")?
                    .push(json!(format!("private container cleanup: {error:#}")));
            }
        }
    }
    match host.identity("after") {
        Ok(identity) => {
            for name in ["uid", "gid", "kernel", "os_release"] {
                if report["host_before"][name] != identity[name] {
                    report["unresolved_reasons"]
                        .as_array_mut()
                        .context("unresolved array")?
                        .push(json!(format!("host identity changed: {name}")));
                }
            }
            report["host_after"] = identity;
        }
        Err(error) => report["unresolved_reasons"]
            .as_array_mut()
            .context("unresolved array")?
            .push(json!(format!("host identity after: {error:#}"))),
    }
    // An inspect after cleanup does not execute the image or alter the host user.
    match host.docker("image-after", &["image", "inspect", IMAGE], 3) {
        Ok(after) if after.status.success() && after.failure.is_none() => {
            if let Ok(identity) = nw_domain::commands::decode_json(&after.stdout) {
                if report["image_before"] != identity {
                    report["unresolved_reasons"]
                        .as_array_mut()
                        .context("unresolved array")?
                        .push(json!(
                            "image identity changed or unavailable before capture"
                        ));
                }
                report["image_after"] = identity;
            } else {
                report["unresolved_reasons"]
                    .as_array_mut()
                    .context("unresolved array")?
                    .push(json!("malformed final image inspect"));
            }
        }
        _ => report["unresolved_reasons"]
            .as_array_mut()
            .context("unresolved array")?
            .push(json!("final image identity unavailable")),
    }
    for error in &host.diagnostic_failures {
        report["unresolved_reasons"]
            .as_array_mut()
            .context("unresolved array")?
            .push(json!(error));
    }
    let mut complete = result.is_ok()
        && report["unresolved_reasons"]
            .as_array()
            .context("unresolved array")?
            .is_empty()
        && report["metadata_capture"]["complete_snapshot_lock"] == true;
    report["complete_snapshot_lock"] = json!(complete);
    report["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    let report_size = evidence::canonical(&report)?.len();
    let manifest_estimate = evidence::canonical(&json!({"files":host.files}))?.len();
    if report_size
        .checked_add(manifest_estimate)
        .and_then(|n| n.checked_add(2048))
        .context("report bound overflow")? as u64
        > evidence::SMALL_LIMIT
    {
        report["complete_snapshot_lock"] = json!(false);
        report["unresolved_reasons"]
            .as_array_mut()
            .context("unresolved array")?
            .push(json!(
                "combined report exceeds 8MiB; no truncated report can be accepted as a lock"
            ));
        let oversized = evidence::canonical(&report)?;
        let retained = host
            .record("host/oversized-failed-report.json", &oversized)
            .is_ok();
        report["metadata_capture"] = json!({"complete_snapshot_lock":false,"status":"combined_report_8MiB_limit_exceeded","full_failed_report_sha256":metadata::digest(&oversized),"retained_as_raw":retained});
        report["complete_snapshot_lock"] = json!(false);
        complete = false;
    }
    let manifest = json!({"schema_version":1,"encoding":"ordered_concatenation_of_sha256_checked_chunks","complete":host.files.iter().all(|row| row["capture_status"] == "complete"),"accounting":"UNKNOWN chunks retain their full reserved upper bound","raw_bytes":host.total,"raw_files":host.files.len(),"transport_files":host.chunks,"files":host.files});
    let manifest_bytes = evidence::canonical(&manifest)?;
    report["raw_manifest"] = json!({"path":"raw-manifest.json","bytes":manifest_bytes.len(),"sha256":metadata::digest(&manifest_bytes)});
    if (evidence::canonical(&report)?.len() as u64)
        .checked_add(manifest_bytes.len() as u64)
        .and_then(|n| n.checked_add(512))
        .context("final small report overflow")?
        > evidence::SMALL_LIMIT
    {
        report = json!({"schema_version":1,"complete_snapshot_lock":false,"source_head":report["source_head"],"capture_utc":report["capture_utc"],"snapshot":metadata::SNAPSHOT,"base":IMAGE,"platform":"linux/amd64","cleanup":report["cleanup"],"raw_manifest":report["raw_manifest"],"oversized_failed_report_identity":report["metadata_capture"],"unresolved_reasons":["full report exceeded the combined 8MiB bound; this failed summary is not a lock"],"execution":{"install":false,"compiler":false,"ffmpeg":false,"source_bootstrap":false,"package_payloads":"not_downloaded"}});
        complete = false;
    }
    evidence::write_small(&output.join("report"), &report, &manifest)?;
    ensure!(
        complete,
        "Linux metadata capture unresolved; inspect target/toolchain-capture/report/report.json and retained raw artifact"
    );
    Ok(())
}

fn capture(
    root: &Path,
    _config: &Value,
    nonce: &str,
    host: &mut Host,
    report: &mut Value,
    cleanup_target: &mut Option<String>,
) -> Result<()> {
    host.record(
        "host/capture-started.json",
        b"{\"metadata_only\":true,\"container_evidence\":\"pending\"}\n",
    )?;
    ensure!(
        fs::read(root.join("components/linux-toolchain-capture.json"))? == CONFIG.as_bytes(),
        "source and compiled capture config differ"
    );
    let mut git = process::command("/usr/bin/git");
    git.current_dir(root).args(["rev-parse", "HEAD"]);
    let source = process::run(&mut git, host.deadline, 3, None)?;
    source.success()?;
    let source = source.text()?.trim();
    ensure!(
        source.len() == 40 && source.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid source HEAD"
    );
    report["source_head"] = json!(source);
    report["host_before"] = host.identity("before")?;
    report["capture_utc"] = report["host_before"]["utc"].clone();
    host.record("host/selected-config.json", CONFIG.as_bytes())?;
    let versions = host.docker("docker-version", &["version", "--format", "{{json .}}"], 5)?;
    versions.success()?;
    report["docker_versions"] = nw_domain::commands::decode_json(&versions.stdout)?;
    let pull = host.docker("pull", &["pull", "--platform", "linux/amd64", IMAGE], 100);
    let pull = match pull {
        Ok(output) => output,
        Err(error) => {
            report["pull"] = json!({"client":"failed_or_timed_out_within_100_seconds","daemon_owned_shared_pull_cancellation":"UNKNOWN","claim":"capture_failed_no_completed_cleanup_or_lock_proof","ruling":"do_not_cancel_shared_daemon_image_operations"});
            return Err(error).context("bounded pull failed; daemon cancellation UNKNOWN");
        }
    };
    report["pull"] = json!({"status":pull.status.to_string(),"supervision_failure":pull.failure,"daemon_owned_shared_pull_cancellation":if pull.failure.is_some() || !pull.status.success() { "UNKNOWN" } else { "not_requested" }});
    pull.success().context("pull did not complete successfully; any daemon-owned shared activity remains UNKNOWN and is not cancelled")?;
    let inspect = host.docker("image-before", &["image", "inspect", IMAGE], 5)?;
    inspect.success()?;
    let image = nw_domain::commands::decode_json(&inspect.stdout)?;
    ensure!(
        image.as_array().is_some_and(|a| a.len() == 1)
            && image[0]["Os"] == "linux"
            && image[0]["Architecture"] == "amd64",
        "actual image platform mismatch"
    );
    ensure!(
        image[0]["RepoDigests"]
            .as_array()
            .context("image repository digests")?
            .iter()
            .any(
                |digest| digest.as_str().is_some_and(|value| value.ends_with(
                    "@sha256:78d4744ead1f5b19b04d0df558ab182a5300903900755e515b2ca4d10eb2044d"
                ))
            ),
        "actual image digest mismatch"
    );
    report["image_before"] = image;
    let remaining = host
        .deadline
        .saturating_duration_since(Instant::now())
        .as_secs();
    let worker_seconds = remaining.saturating_sub(40).min(230);
    ensure!(
        worker_seconds >= 15,
        "insufficient budget for supervised private capture"
    );
    let seconds = worker_seconds.to_string();
    let label = format!("nw.toolchain.capture={nonce}");
    *cleanup_target = Some(nonce.to_string());
    let created = host.docker(
        "create",
        &[
            "create",
            "--pull=never",
            "--platform",
            "linux/amd64",
            "--name",
            nonce,
            "--label",
            &label,
            "--memory",
            "1536m",
            "--memory-swap",
            "1536m",
            "--cpus",
            "2",
            "--pids-limit",
            "128",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--user",
            "0:0",
            "--init",
            "--stop-timeout",
            "1",
            "--env",
            "NW_TOOLCHAIN_INNER=1",
            "--env",
            "LC_ALL=C",
            "--env",
            "PATH=/usr/sbin:/usr/bin:/sbin:/bin",
            "--entrypoint",
            "/capture-xtask",
            IMAGE,
            "capture-linux-toolchain",
            "--container-supervisor",
            &seconds,
        ],
        10,
    )?;
    created.success()?;
    let container = created.text()?.trim();
    ensure!(
        container.len() == 64 && container.bytes().all(|b| b.is_ascii_hexdigit()),
        "malformed created container identity"
    );
    *cleanup_target = Some(container.to_string());
    report["container_id"] = json!(container);
    let executable = std::env::current_exe()?;
    let (executable_size, executable_hash) = evidence::hash_file(&executable, evidence::RAW_LIMIT)?;
    report["copied_executable"] = json!({"bytes":executable_size,"sha256":executable_hash,"abi":"must_be_proven_by_actual_container_worker_execution"});
    for (source, destination, label) in [
        (executable, "/capture-xtask", "copy-executable"),
        (
            root.join("components/linux-toolchain-capture.json"),
            "/capture-config.json",
            "copy-config",
        ),
    ] {
        let source = source.to_str().context("UTF8 copy input")?;
        let destination = format!("{container}:{destination}");
        host.docker(label, &["cp", source, &destination], 10)?
            .success()?;
    }
    let mut ca_probe = process::command("/usr/bin/docker");
    ca_probe.args([
        "cp",
        &format!("{container}:/etc/ssl/certs/ca-certificates.crt"),
        "-",
    ]);
    let base_ca = process::run(&mut ca_probe, host.deadline, 5, None)?;
    ensure!(base_ca.failure.is_none(), "base CA inspection incomplete");
    let has_base_ca = base_ca.status.success();
    if has_base_ca {
        let bytes = metadata::single_tar(&base_ca.stdout, "ca-certificates.crt", 4 * 1024 * 1024)?;
        report["base_transport_ca"] = json!({"bytes":bytes.len(),"sha256":metadata::digest(&bytes),"provenance":"actual_selected_base_before_start"});
    } else {
        host.record("host/base-ca-probe.stderr", &base_ca.stderr)?;
        let message = String::from_utf8_lossy(&base_ca.stderr);
        ensure!(
            message.contains("Could not find the file") || message.contains("no such file"),
            "base transport CA absence not established"
        );
    }
    let ca = Path::new("/etc/ssl/certs/ca-certificates.crt");
    if !has_base_ca && ca.is_file() {
        let (size, hash) = evidence::hash_file(ca, 4 * 1024 * 1024)?;
        report["optional_host_transport_ca"] = json!({"path":ca,"bytes":size,"sha256":hash,"authority":"transport_only_if_base_CA_absent_not_archive_signing_trust"});
        host.docker(
            "copy-optional-transport-ca",
            &[
                "cp",
                "/etc/ssl/certs/ca-certificates.crt",
                &format!("{container}:/capture-host-ca.crt"),
            ],
            5,
        )?
        .success()?;
    }
    let run = host.docker(
        "start-attached",
        &["start", "--attach", container],
        worker_seconds + 5,
    );
    let state = host.docker(
        "container-state",
        &["inspect", "--format", "{{json .State}}", container],
        4,
    )?;
    state.success()?;
    let state = nw_domain::commands::decode_json(&state.stdout)?;
    if state["Running"] == true {
        host.docker("stop-timed-out-private-worker", &["kill", container], 4)?
            .success()?;
        let state = host.docker(
            "container-state-after-kill",
            &["inspect", "--format", "{{json .State}}", container],
            3,
        )?;
        state.success()?;
        ensure!(
            nw_domain::commands::decode_json(&state.stdout)?["Running"] == false,
            "private worker stop not confirmed"
        );
    }
    // Retrieve a failed worker's completed diagnostic report too. No exit code can
    // turn a partial inventory into a lock, and no package installation is run.
    let inner_bytes = host.copy_out(container, "report.json", evidence::SMALL_LIMIT)?;
    let manifest_bytes = host.copy_out(container, "raw-manifest.json", evidence::SMALL_LIMIT)?;
    receive_raw_with(host, report, &inner_bytes, &manifest_bytes, |host, path| {
        host.copy_out(container, path, evidence::CHUNK_LIMIT)
    })?;
    let run = run?;
    run.success()?;
    ensure!(
        report["metadata_capture"]["complete_snapshot_lock"] == true
            && report["metadata_capture"]["unresolved_reasons"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "container capture has unresolved required evidence"
    );
    if report["metadata_capture"]["trust"]["tls"]["input"]["provenance"]
        == "host_actual_/etc/ssl/certs/ca-certificates.crt_transport_only_not_archive_signing_trust"
    {
        ensure!(
            report["metadata_capture"]["trust"]["tls"]["input"]["identity"]["sha256"]
                == report["optional_host_transport_ca"]["sha256"],
            "host transport CA provenance/hash mismatch"
        );
    } else {
        ensure!(
            report["metadata_capture"]["trust"]["tls"]["input"]["provenance"] == "selected_base"
                && report["metadata_capture"]["trust"]["tls"]["input"]["identity"]["sha256"]
                    == report["base_transport_ca"]["sha256"],
            "base transport CA provenance/hash mismatch"
        );
    }
    Ok(())
}

fn receive_raw_with(
    host: &mut Host,
    report: &mut Value,
    inner_bytes: &[u8],
    manifest_bytes: &[u8],
    fetch: impl FnMut(&mut Host, &str) -> Result<Vec<u8>>,
) -> Result<()> {
    let inner_report = evidence::parse_canonical(inner_bytes)?;
    let manifest = evidence::parse_canonical(manifest_bytes)?;
    ensure!(
        inner_report["schema_version"] == 1
            && inner_report["raw_manifest"]["sha256"].as_str()
                == Some(metadata::digest(manifest_bytes).as_str())
            && inner_report["raw_manifest"]["bytes"].as_u64() == Some(manifest_bytes.len() as u64),
        "inner raw manifest identity mismatch"
    );
    report["metadata_capture"] = inner_report;
    host.record("host/container-raw-manifest.json", manifest_bytes)?;
    transfer_raw_with(host, &manifest, fetch)
}

fn transfer_raw_with(
    host: &mut Host,
    manifest: &Value,
    mut fetch: impl FnMut(&mut Host, &str) -> Result<Vec<u8>>,
) -> Result<()> {
    ensure!(manifest["schema_version"] == 1, "unsupported raw manifest");
    let files = manifest["files"].as_array().context("raw files")?;
    ensure!(
        files.len() <= 500 && manifest["raw_files"].as_u64() == Some(files.len() as u64),
        "raw file count mismatch"
    );
    let mut seen = BTreeSet::new();
    for file in &host.files {
        metadata::unique_path(
            &mut seen,
            file["logical_path"].as_str().context("host logical path")?,
        )?;
    }
    let mut chunks_seen = BTreeSet::new();
    let mut raw_total = 0_u64;
    let mut physical = 0_u64;
    let mut incomplete = manifest["complete"] == false;
    for entry in files {
        ensure!(
            Instant::now() < host.deadline,
            "raw transfer deadline exhausted"
        );
        let logical = entry["logical_path"].as_str().context("raw logical path")?;
        metadata::unique_path(&mut seen, logical)?;
        let expected_size = entry["bytes"].as_u64().context("raw size")?;
        ensure!(
            expected_size <= process::FILE_LIMIT,
            "raw file exceeds 128MiB"
        );
        let expected_hash = match entry.get("sha256") {
            Some(Value::String(hash)) => {
                metadata::sha256(hash)?;
                Some(hash.as_str())
            }
            Some(Value::Null) => {
                ensure!(
                    manifest["complete"] == false && entry["capture_status"] == "incomplete",
                    "UNKNOWN identity requires an explicitly failed manifest and logical entry"
                );
                ensure!(
                    entry["expected_bytes"]
                        .as_u64()
                        .is_some_and(|size| size >= expected_size && size <= process::FILE_LIMIT),
                    "UNKNOWN original size bound"
                );
                metadata::sha256(
                    entry["expected_sha256"]
                        .as_str()
                        .context("UNKNOWN original SHA256")?,
                )?;
                None
            }
            _ => bail!("raw SHA256 missing or invalid"),
        };
        let chunks = entry["chunks"].as_array().context("raw chunks")?;
        ensure!(chunks.len() <= 500, "raw chunk count exceeded");
        // UNKNOWN source bytes are never host files. Admit only the declared
        // independently verifiable portion, with per-chunk checks again below.
        let admission = if expected_hash.is_some() {
            expected_size
        } else {
            chunks
                .iter()
                .filter(|chunk| chunk["sha256"].is_string())
                .try_fold(0_u64, |total, chunk| {
                    process::check_count(
                        total,
                        chunk["bytes"].as_u64().context("known chunk size")?,
                        expected_size,
                    )
                })?
        };
        let index = host.begin_file(logical, admission, expected_hash)?;
        host.files[index]["source_manifest_bytes"] = json!(expected_size);
        if expected_hash.is_none() {
            host.files[index]["source_integrity"] = json!("UNKNOWN");
            incomplete = true;
        }
        host.files[index]["source_expected_bytes"] = entry
            .get("expected_bytes")
            .unwrap_or(&entry["bytes"])
            .clone();
        host.files[index]["source_expected_sha256"] = entry
            .get("expected_sha256")
            .unwrap_or(&entry["sha256"])
            .clone();
        let mut hash = Sha256::new();
        let mut size = 0;
        let mut unknown = Vec::new();
        for chunk in chunks {
            let path = chunk["path"].as_str().context("raw chunk path")?;
            metadata::unique_path(&mut chunks_seen, path)?;
            let tail = path
                .strip_prefix("raw/chunk-")
                .context("unexpected raw chunk namespace")?;
            ensure!(
                tail.len() == 4 && tail.bytes().all(|b| b.is_ascii_digit()),
                "unexpected raw chunk name"
            );
            physical = physical.checked_add(1).context("chunk count overflow")?;
            ensure!(physical <= 500, "raw transport file count exceeded");
            let chunk_size = chunk["bytes"].as_u64().context("chunk size")?;
            ensure!(
                chunk_size <= evidence::CHUNK_LIMIT,
                "invalid chunk byte bound"
            );
            let next_size = process::check_count(size, chunk_size, expected_size)?;
            if chunk.get("sha256") == Some(&Value::Null) {
                ensure!(
                    expected_hash.is_none()
                        && chunk["integrity"] == "UNKNOWN"
                        && chunk["reserved_upper_bound"] == true,
                    "unsupported UNKNOWN chunk representation"
                );
                let mut skipped = chunk.clone();
                skipped["export_status"] = json!("not_exported_source_identity_UNKNOWN");
                unknown.push(skipped);
                host.files[index]["unexported_chunks"] = json!(&unknown);
                size = next_size;
                incomplete = true;
                continue;
            }
            let chunk_hash = chunk["sha256"].as_str().context("chunk hash")?;
            metadata::sha256(chunk_hash)?;
            ensure!(host.chunks < 500, "combined transport file count exceeded");
            process::check_count(host.total, chunk_size, evidence::RAW_LIMIT)?;
            let bytes = fetch(host, path)?;
            metadata::check_bytes(&bytes, chunk_size, chunk_hash)?;
            host.append_chunk_with(index, path, &bytes, &mut hash, |file, bytes| {
                file.write_all(bytes)
            })?;
            size = next_size;
        }
        ensure!(
            size == expected_size,
            "raw file declared chunk size mismatch"
        );
        raw_total = process::check_count(raw_total, size, evidence::RAW_LIMIT)?;
        if let Some(expected_hash) = expected_hash {
            ensure!(
                hex::encode(hash.finalize()) == expected_hash,
                "complete raw file hash mismatch"
            );
            host.complete_file(index)?;
        }
        if entry["capture_status"] == "incomplete" {
            host.files[index]["capture_status"] = json!("incomplete");
            incomplete = true;
        }
    }
    ensure!(
        manifest["raw_bytes"].as_u64() == Some(raw_total)
            && manifest["transport_files"].as_u64() == Some(physical),
        "raw manifest aggregate mismatch"
    );
    ensure!(
        !incomplete,
        "raw source manifest incomplete; available verified chunks retained, UNKNOWN portions explicitly unexported"
    );
    Ok(())
}

fn cleanup_command(
    host: &mut Host,
    label: &str,
    arguments: &[&str],
    seconds: u64,
) -> Result<process::Output> {
    let mut command = process::command("/usr/bin/docker");
    command.args(arguments);
    let output = process::run(&mut command, host.deadline, seconds, None)?;
    // Diagnostic storage failure must never prevent removal of our live container.
    for (stream, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        if let Err(error) = host.record(&format!("host/cleanup/{label}.{stream}"), bytes) {
            host.diagnostic_failures
                .push(format!("cleanup diagnostics incomplete: {error:#}"));
        }
    }
    Ok(output)
}

fn cleanup(host: &mut Host, target: &str, nonce: &str) -> Result<()> {
    let exact_created_id =
        target.len() == 64 && target.bytes().all(|byte| byte.is_ascii_hexdigit());
    let inspected = cleanup_command(host, "inspect", &["inspect", target], 4);
    match inspected {
        Ok(output) if output.failure.is_none() && output.status.success() => {
            let metadata = nw_domain::commands::decode_json(&output.stdout)?;
            ensure!(
                metadata.as_array().is_some_and(|a| a.len() == 1)
                    && metadata[0]["Config"]["Labels"]["nw.toolchain.capture"] == nonce,
                "refuse cleanup of a container not owned by this attempt"
            );
        }
        Ok(output)
            if output.failure.is_none()
                && String::from_utf8_lossy(&output.stderr).contains("No such object") =>
        {
            ensure!(
                exact_created_id,
                "create response unresolved; immediate name absence cannot prove daemon request completion or cleanup"
            );
            return Ok(());
        }
        // A successfully returned immutable create ID already establishes which
        // exact container we own. An inspect failure must not skip its removal.
        _ if exact_created_id => {}
        _ => bail!("create/ownership inspection unresolved; exact private cleanup UNKNOWN"),
    }
    cleanup_command(host, "remove", &["rm", "--force", "--volumes", target], 8)?.success()?;
    let absent = cleanup_command(host, "absence", &["inspect", target], 3)?;
    ensure!(
        absent.failure.is_none()
            && !absent.status.success()
            && String::from_utf8_lossy(&absent.stderr).contains("No such object"),
        "private container removal not confirmed"
    );
    Ok(())
}
