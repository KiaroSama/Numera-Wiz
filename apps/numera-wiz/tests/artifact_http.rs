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
    let report = json!({"schema_version":1,"status":"passed","executable":executable,"registry":runtime_record["registry"],"components":{"ffmpeg":plan["ffmpeg"],"ffprobe":plan["ffprobe"]},"job_id":id,"plan_id":plan["id"],"plan_hash":plan["hash"],"artifact_id":artifact_id,"source":std::env::var("GITHUB_SHA").ok(),"artifact_bytes":total.to_string(),"sha256":expected.sha256,"owner_process_id":pid,"owner_before_high_kib":before,"owner_high_kib":high,"selected_owner_rss_ceiling_kib":131072,"job_revoked_delivered_bytes":job_revoked_bytes.to_string(),"credential_revoked_delivered_bytes":credential_revoked_bytes.to_string(),"cancelled_index_cleanup":true,"scope":"Actual CI-built Linux headless owner, real FFmpeg-created valid large container, reviewed byte-copy job, authenticated HTTP. This is not complete aggregate R20, other-platform or packaged qualification."});
    let encoded = serde_json::to_vec_pretty(&report).unwrap();
    assert!(
        encoded.len() <= 16 * 1024,
        "Nonsecret hosted HTTP evidence must remain bounded"
    );
    std::fs::write(evidence.join("large-http-transfer.json"), encoded).unwrap();
    println!("NW_ARTIFACT_HTTP_REPORT:{}", serde_json::to_string(&report).unwrap());
    controller.kill().await.unwrap();
    controller.wait().await.unwrap();
}
