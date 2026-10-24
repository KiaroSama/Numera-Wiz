//! Hosted actual owner publication and larger-than-memory HTTP transfer evidence.
#![cfg(target_os = "linux")]
#[allow(
    dead_code,
    reason = "Shared media fixture contains additional independent media oracles"
)]
#[path = "../../../test-support/media.rs"]
mod fixture;
#[path = "support/owner.rs"]
mod owner;
use owner::Cli;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

#[path = "support/evidence.rs"]
mod evidence;
use evidence::RetainLogs;

async fn owner_pid(root: &Path) -> u64 {
    let record: Value = serde_json::from_slice(
        &nw_platform::security::read_private(&root.join("runtime/owner.json"), 16384).unwrap(),
    )
    .unwrap();
    for _ in 0..50 {
        let bytes = nw_platform::security::read_private(
            &root.join("logs/owner.jsonl"),
            2 * 1024 * 1024 + 1024,
        )
        .unwrap();
        for line in bytes.split(|b| *b == b'\n') {
            if let Ok(row) = serde_json::from_slice::<Value>(line)
                && row["root_id"] == record["root_id"]
                && row["owner_epoch"] == record["epoch"]
                && let Some(pid) = row["process_id"].as_u64()
            {
                return pid;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Matching protected owner process identity was not logged");
}
fn rss_high_kib(pid: u64) -> u64 {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
fn retained_publication_objects(workspace: &Path, output: &Path) -> bool {
    let paths: Vec<_> = std::fs::read_dir(workspace)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.len() == 2
        && paths
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "payload.mkv"))
        && paths.iter().any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(".numera-proof-"))
                .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok())
        })
        && paths
            .iter()
            .all(|path| nw_platform::files::same_object(path, output).unwrap_or(false))
}
async fn cleaned_index(workspace: &Path, output: &Path) {
    let started = Instant::now();
    loop {
        if retained_publication_objects(workspace, output) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Disconnected stream retained its temporary block index"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
async fn incomplete_after_revocation(
    mut response: reqwest::Response,
    first: usize,
    total: u64,
) -> u64 {
    let mut delivered = first as u64;
    let mut interrupted = false;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match response.chunk().await {
                Ok(Some(bytes)) => {
                    delivered += bytes.len() as u64;
                }
                Ok(None) => break,
                Err(_) => {
                    interrupted = true;
                    break;
                }
            }
        }
    })
    .await
    .expect("Revoked stream did not terminate");
    assert!(
        interrupted && delivered < total,
        "Revocation must end the body before its advertised complete length; already delivered bytes remain delivered"
    );
    delivered
}
#[tokio::test]
#[ignore = "Hosted real media owner and authenticated large HTTP transfer"]
async fn large_published_output_streams_with_bounded_owner_memory_and_revocation() {
    nw_platform::init_logging();
    tokio::time::timeout(Duration::from_secs(900), journey())
        .await
        .expect("Large owner transfer journey deadline");
}
async fn journey() {
    let media = fixture::generate().await;
    let source = media.directory.path().join("large-source.mkv");
    // The pinned Matroska muxer writes an actual reserved Cues/Void region with bounded ffio_fill.
    // It creates a valid large container, without inflating test duration or collecting media bytes in RAM.
    let args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        media.source.to_str().unwrap(),
        "-map",
        "0",
        "-c",
        "copy",
        "-reserve_index_space",
        "268435456",
        "-f",
        "matroska",
        source.to_str().unwrap(),
    ]
    .map(str::to_owned)
    .to_vec();
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &args,
        Duration::from_secs(60),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let expected = nw_platform::files::identity(&source, &CancellationToken::new()).unwrap();
    let total = expected.bytes.parse::<u64>().unwrap();
    assert!(total > 256 * 1024 * 1024);
    assert_eq!(
        fixture::probe(&media.bin, &source).await["streams"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let cli = Cli {
        data: media.directory.path().join("owner-data"),
        components: media.bin.clone(),
        request_timeout: Duration::from_secs(150),
    };
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/owner-evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let _logs = RetainLogs {
        source: cli.data.join("logs"),
        destination: evidence.join("large-owner-logs"),
    };
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"]
        .as_str()
        .unwrap()
        .to_owned();
    let asset = cli
        .mutation(
            &scope,
            "register-large",
            "assets.register",
            json!({"path":source}),
        )
        .await;
    assert_eq!(asset["ok"], true, "{asset}");
    let output = media.directory.path().join("large-output.mkv");
    let plan = cli
        .query(
            "streams.clean",
            json!({"asset_id":asset["result"]["id"],"output":output,"intent":fixture::rules()}),
        )
        .await;
    let accepted = cli
        .mutation(
            &scope,
            "submit-large",
            "jobs.submit",
            json!({"plan_id":plan["id"],"plan_hash":plan["hash"]}),
        )
        .await;
    assert_eq!(accepted["ok"], true, "{accepted}");
    let id = accepted["result"]["job_id"].as_str().unwrap();
    let job = cli.completed(id).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["artifact"]["sha256"], expected.sha256);
    let artifact_id = format!("job-{id}");
    let workspace = std::fs::read_dir(media.directory.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".numera-job-")
        })
        .unwrap();
    assert!(
        retained_publication_objects(&workspace, &output),
        "Only the recorded payload and proof remain, both identifying the published verified object"
    );
    let profile = json!({"name":"download-reader","expected_revision":null,"enabled":true,"mode":"operate","owned_object_access":false,"allowed_command_ids":["artifacts.list","artifacts.manifest","artifacts.read_chunk"],"object_grants":[{"kind":"job","id":id,"actions":["read"]}],"read_roots":[],"write_roots":[]});
    assert_eq!(
        cli.mutation(
            &scope,
            "create-download-reader",
            "access.profiles.put",
            profile.clone()
        )
        .await["ok"],
        true
    );
    // A fresh CLI adapter with only this job's read grant saves through its private
    // authenticated byte route; it has no general read/write root permission.
    let client_output = media.directory.path().join("large-client-copy.mkv");
    let mut saving = cli
        .command()
        .args([
            "--access-profile",
            "download-reader",
            "artifacts",
            "save",
            "--artifact-id",
            &artifact_id,
            "--expected-sha256",
            &expected.sha256,
            "--out",
        ])
        .arg(&client_output)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(evidence.join("large-client-copy.log")).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client_pid = saving.id().unwrap() as u64;
    let mut client_copy_peak_kib = 0u64;
    let started = Instant::now();
    loop {
        if let Ok(status) = std::fs::read_to_string(format!("/proc/{client_pid}/status"))
            && let Some(high) = status.lines().find_map(|line| {
                line.strip_prefix("VmHWM:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        {
            client_copy_peak_kib = client_copy_peak_kib.max(high);
        }
        if saving.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(100),
            "Actual bounded client copy did not finish"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let client_copy_elapsed_ms = started.elapsed().as_millis().to_string();
    let saved = saving.wait_with_output().await.unwrap();
    assert!(
        saved.status.success(),
        "Actual scoped client artifact save failed"
    );
    let receipt: Value = serde_json::from_slice(&saved.stdout).unwrap();
    assert_eq!(receipt["ok"], true, "{receipt}");
    assert_eq!(receipt["result"]["sha256"], expected.sha256);
    assert_eq!(receipt["result"]["encoded_byte_count"], total.to_string());
    let client_identity =
        nw_platform::files::identity(&client_output, &CancellationToken::new()).unwrap();
    assert_eq!(client_identity.sha256, expected.sha256);
    assert_eq!(client_identity.bytes, total.to_string());
    assert!(
        client_copy_peak_kib > 0 && client_copy_peak_kib < 128 * 1024,
        "Actual client exceeded its selected 128 MiB RSS ceiling: {client_copy_peak_kib} KiB"
    );
    assert!(
        !std::fs::read_dir(media.directory.path())
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".numera-wiz-save-")
            })
    );
    cleaned_index(&workspace, &output).await;
    let client_checkpoint = json!({
        "schema_version":1, "status":"full_client_copy_passed",
        "source":std::env::var("GITHUB_SHA").ok(), "artifact_id":artifact_id,
        "bytes":client_identity.bytes, "sha256":client_identity.sha256,
        "client_peak_kib":client_copy_peak_kib,"selected_client_ceiling_kib":131072,
        "elapsed_ms":client_copy_elapsed_ms,
        "profile":"job read only; no general filesystem roots",
        "not_qualified":["later cancellation/revocation assertions", "other platforms/packages", "aggregate R20"]
    });
    std::fs::write(
        evidence.join("large-client-copy.json"),
        serde_json::to_vec_pretty(&client_checkpoint).unwrap(),
    )
    .unwrap();
    println!("NW_CLIENT_COPY_REPORT:{client_checkpoint}");
    let cancelled_output = media.directory.path().join("cancelled-client-copy.mkv");
    let mut cancelling = cli
        .command()
        .args([
            "--access-profile",
            "download-reader",
            "artifacts",
            "save",
            "--artifact-id",
            &artifact_id,
            "--expected-sha256",
            &expected.sha256,
            "--out",
        ])
        .arg(&cancelled_output)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(evidence.join("cancelled-client-copy.log")).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let cancel_pid = cancelling.id().unwrap();
    let started = Instant::now();
    loop {
        let partial = std::fs::read_dir(media.directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".numera-wiz-save-")
            })
            .filter_map(|path| std::fs::read_dir(path).ok())
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .any(|meta| meta.is_file() && meta.len() > 0 && meta.len() < total);
        if partial {
            break;
        }
        assert!(
            cancelling.try_wait().unwrap().is_none(),
            "Client finished before the cancellation boundary was observed"
        );
        if started.elapsed() >= Duration::from_secs(100) {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            std::fs::File::open(evidence.join("cancelled-client-copy.log"))
                .unwrap()
                .take(64 * 1024)
                .read_to_end(&mut bytes)
                .unwrap();
            let phases: Vec<String> = String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|line| {
                    line.contains("artifact.client_copy")
                        || line.contains("Executable identity read completed")
                        || line.contains("Protected owner attachment completed")
                })
                .take(16)
                .map(|line| line.chars().take(4096).collect())
                .collect();
            println!(
                "NW_CLIENT_COPY_PREPARATION:{}",
                json!({
                    "elapsed_ms":started.elapsed().as_millis().to_string(), "phases":phases,
                    "scope":"Bounded safe preflight diagnostics; no headers, credentials or response bodies"
                })
            );
            panic!(
                "Client did not begin a partial staged transfer within the bounded preparation deadline"
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let client_copy_cancel_setup_ms = started.elapsed().as_millis().to_string();
    assert!(
        tokio::process::Command::new("/bin/kill")
            .args(["-INT", &cancel_pid.to_string()])
            .status()
            .await
            .unwrap()
            .success()
    );
    let cancelled = tokio::time::timeout(Duration::from_secs(20), cancelling.wait_with_output())
        .await
        .expect("Client cancellation did not drain")
        .unwrap();
    assert!(!cancelled.status.success());
    let cancelled_receipt: Value = serde_json::from_slice(&cancelled.stdout).unwrap();
    assert_eq!(cancelled_receipt["error"]["code"], "CANCELLED");
    assert!(!cancelled_output.exists());
    assert!(
        !std::fs::read_dir(media.directory.path())
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".numera-wiz-save-")
            })
    );
    cleaned_index(&workspace, &output).await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let endpoint = format!("http://127.0.0.1:{port}");
    let token_file = media.directory.path().join("auth/controller-token");
    std::fs::create_dir_all(&evidence).unwrap();
    let mut controller = cli
        .command()
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--access-profile",
            "download-reader",
            "--token-file",
        ])
        .arg(&token_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(evidence.join("large-http-controller.log")).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !token_file.exists() {
        assert!(started.elapsed() < Duration::from_secs(45));
        assert!(controller.try_wait().unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let token =
        String::from_utf8(nw_platform::security::read_private(&token_file, 256).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(130))
        .build()
        .unwrap();
    let url = format!("{endpoint}/api/v1/artifacts/{artifact_id}/bytes");
    let pid = owner_pid(&cli.data).await;
    let runtime_record: Value = serde_json::from_slice(
        &nw_platform::security::read_private(&cli.data.join("runtime/owner.json"), 16384).unwrap(),
    )
    .unwrap();
    let executable = nw_platform::files::identity(
        Path::new(env!("CARGO_BIN_EXE_numera-wiz")),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(runtime_record["build"], executable.sha256);
    let before = rss_high_kib(pid);
    let mut response = client
        .get(&url)
        .bearer_auth(&token)
        .header("If-Match", format!("\"{}\"", expected.sha256))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(response.content_length(), Some(total));
    let mut hash = Sha256::new();
    let mut received = 0u64;
    while let Some(bytes) = response.chunk().await.unwrap() {
        received += bytes.len() as u64;
        hash.update(&bytes);
    }
    assert_eq!(received, total);
    assert_eq!(hex::encode(hash.finalize()), expected.sha256);
    cleaned_index(&workspace, &output).await;
    let high = rss_high_kib(pid);
    assert!(
        high < 128 * 1024,
        "Actual owner exceeded the selected 128 MiB HTTP transfer RSS ceiling: {high} KiB"
    );

    let mut interrupted = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let first = interrupted.chunk().await.unwrap().unwrap().len();
    let mut revoked = profile.clone();
    revoked["expected_revision"] = json!(1);
    revoked["object_grants"] = json!([]);
    assert_eq!(
        cli.mutation(
            &scope,
            "withdraw-download-right",
            "access.profiles.put",
            revoked.clone()
        )
        .await["ok"],
        true
    );
    let job_revoked_bytes = incomplete_after_revocation(interrupted, first, total).await;
    cleaned_index(&workspace, &output).await;
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let mut restored = profile;
    restored["expected_revision"] = json!(2);
    assert_eq!(
        cli.mutation(
            &scope,
            "restore-download-right",
            "access.profiles.put",
            restored
        )
        .await["ok"],
        true
    );
    let extra_file = media.directory.path().join("auth/stream-token");
    let (issued, status) = cli
        .human(&[
            "access",
            "tokens",
            "issue",
            "--for-access-profile",
            "download-reader",
            "--port",
            &port.to_string(),
            "--out",
            extra_file.to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{issued}");
    let extra =
        String::from_utf8(nw_platform::security::read_private(&extra_file, 256).unwrap()).unwrap();
    let mut interrupted = client
        .get(&url)
        .bearer_auth(&extra)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let first = interrupted.chunk().await.unwrap().unwrap().len();
    let (revoked, status) = cli
        .human(&[
            "access",
            "tokens",
            "revoke",
            "--token-id",
            issued["token_id"].as_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{revoked}");
    let credential_revoked_bytes = incomplete_after_revocation(interrupted, first, total).await;
    cleaned_index(&workspace, &output).await;
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&extra)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let mut cancelled = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert!(!cancelled.chunk().await.unwrap().unwrap().is_empty());
    drop(cancelled);
    cleaned_index(&workspace, &output).await;
    let high = high.max(rss_high_kib(pid));
    assert!(high < 128 * 1024);
    let report = json!({"schema_version":1,"status":"passed","executable":executable,"registry":runtime_record["registry"],"components":{"ffmpeg":plan["ffmpeg"],"ffprobe":plan["ffprobe"]},"job_id":id,"plan_id":plan["id"],"plan_hash":plan["hash"],"artifact_id":artifact_id,"source":std::env::var("GITHUB_SHA").ok(),"artifact_bytes":total.to_string(),"sha256":expected.sha256,"owner_process_id":pid,"client_copy_peak_kib":client_copy_peak_kib,"client_copy_bytes":client_identity.bytes,"client_copy_sha256":client_identity.sha256,"client_copy_profile":"job-read-only; no general filesystem roots","client_copy_cancelled_before_publication":true,"client_copy_elapsed_ms":client_copy_elapsed_ms,"client_copy_cancel_setup_ms":client_copy_cancel_setup_ms,"owner_before_high_kib":before,"owner_high_kib":high,"selected_owner_rss_ceiling_kib":131072,"job_revoked_delivered_bytes":job_revoked_bytes.to_string(),"credential_revoked_delivered_bytes":credential_revoked_bytes.to_string(),"cancelled_index_cleanup":true,"scope":"Actual CI-built Linux headless owner, real FFmpeg-created valid large container, reviewed byte-copy job, authenticated HTTP. This is not complete aggregate R20, other-platform or packaged qualification."});
    let encoded = serde_json::to_vec_pretty(&report).unwrap();
    assert!(
        encoded.len() <= 16 * 1024,
        "Nonsecret hosted HTTP evidence must remain bounded"
    );
    std::fs::write(evidence.join("large-http-transfer.json"), encoded).unwrap();
    println!(
        "NW_ARTIFACT_HTTP_REPORT:{}",
        serde_json::to_string(&report).unwrap()
    );
    controller.kill().await.unwrap();
    controller.wait().await.unwrap();
}
