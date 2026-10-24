//! Real two-source cleanup batches through the typed CLI and official SDK transports.
//! Hosted CI only: no subprocess, media or application execution belongs in local authoring.
#[path = "../../../test-support/media.rs"]
mod fixture;
#[path = "support/owner.rs"]
mod owner;
use nw_domain::{execution::CleanupLimits, mux::Selector};
use owner::Cli;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio_util::sync::CancellationToken;

macro_rules! sdk {
    ($client:expr, $id:expr, $params:expr, $context:expr) => {{
        let arguments = json!({"params": $params, "context": $context});
        let reply = $client.call_tool(CallToolRequestParams::new($id)
            .with_arguments(arguments.as_object().unwrap().clone())).await.unwrap();
        serde_json::to_value(reply).unwrap()["structuredContent"].clone()
    }};
}
macro_rules! call {
    ($client:expr, $id:expr, $params:expr, $context:expr) => {{
        let reply = sdk!($client, $id, $params, $context);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["result"].clone()
    }};
}
macro_rules! finished {
    ($client:expr, $id:expr) => {{
        let started = std::time::Instant::now();
        loop {
            let result = call!($client, "batches.inspect", json!({"id":$id}), json!({}));
            if result["complete"] == true { break result; }
            assert!(started.elapsed() < Duration::from_secs(30), "{result}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }};
}
#[path = "support/lost_ack.rs"]
mod lost_ack;
fn input(
    assets: &[Value],
    outputs: &[std::path::PathBuf],
    intent: &nw_domain::mux::CleanIntent,
    failure: &str,
) -> Value {
    json!({
        "items": assets.iter().zip(outputs).map(|(asset,output)| json!({"asset_id":asset["id"],"output":output})).collect::<Vec<_>>(),
        "intent":intent, "file_times":"new", "execution_limits":CleanupLimits::default(),
        "failure_policy":failure, "unresolved_policy":"reject"
    })
}
async fn packets(bin: &Path, path: &Path) -> BTreeMap<u64, Vec<Value>> {
    let mut args = [
        "-v",
        "error",
        "-show_packets",
        "-show_data_hash",
        "sha256",
        "-show_entries",
        "packet=stream_index,pts,dts,duration,size,flags,data_hash:packet_side_data",
        "-of",
        "json",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(path.to_str().unwrap().into());
    let result = nw_platform::process::run(
        &bin.join("ffprobe"),
        &args,
        Duration::from_secs(30),
        4 * 1024 * 1024,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    let mut grouped = BTreeMap::new();
    for packet in value["packets"].as_array().unwrap() {
        grouped
            .entry(packet["stream_index"].as_u64().unwrap())
            .or_insert_with(Vec::new)
            .push(packet.clone());
    }
    grouped
}
#[tokio::test]
#[ignore = "Actual hosted FFmpeg, durable owner, typed CLI and official SDK batch journeys"]
async fn real_batches_preserve_members_retries_partial_results_and_confirmed_cancellation() {
    let media = fixture::generate().await;
    let second = media.directory.path().join("second-é.mkv");
    let args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        media.source.to_str().unwrap(),
        "-map",
        "0:0",
        "-map",
        "0:1",
        "-c",
        "copy",
        "-f",
        "matroska",
        second.to_str().unwrap(),
    ]
    .map(str::to_owned);
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &args,
        Duration::from_secs(30),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let cli = Cli {
        data: media.directory.path().join("data"),
        components: media.bin.clone(),
        request_timeout: Duration::from_secs(60),
    };
    let (scope, status) = cli
        .human(&["access", "replay-scope", "create", "--json"])
        .await;
    assert_eq!(status, 0, "{scope}");
    let scope = scope["result"]["replay_scope"].as_str().unwrap().to_owned();
    let mut assets = Vec::new();
    for (ordinal, path) in [&media.source, &second].into_iter().enumerate() {
        let reply = cli
            .mutation(
                &scope,
                &format!("register-{ordinal}"),
                "assets.register",
                json!({"path":path,"probe_timeout":{"mode":"finite","seconds":30}}),
            )
            .await;
        assert_eq!(reply["ok"], true, "{reply}");
        assets.push(reply["result"].clone());
    }
    let (profile, status) = cli
        .human(&[
            "access",
            "profiles",
            "create",
            "--name",
            "batch-operator",
            "--read-root",
            media.directory.path().to_str().unwrap(),
            "--write-root",
            media.directory.path().to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{profile}");
    let mut command = cli.command();
    command.args(["bridge", "--access-profile", "batch-operator"]);
    let stdio = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let sdk_scope =
        call!(stdio, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
    let mut sdk_assets = Vec::new();
    for (ordinal, path) in [&media.source, &second].into_iter().enumerate() {
        sdk_assets.push(call!(
            stdio,
            "assets.register",
            json!({"path":path}),
            json!({"replay_scope":sdk_scope,"idempotency_key":format!("register-{ordinal}")})
        ));
    }
    let mut intent = fixture::rules();
    intent.audio_selection.selector = Selector::Languages {
        values: vec!["eng".into()],
    };
    let outputs = [
        media.directory.path().join("cli-remux.mkv"),
        media.directory.path().join("cli-copy.mkv"),
    ];
    let document = media.directory.path().join("batch.json");
    std::fs::write(
        &document,
        serde_json::to_vec(&input(&assets, &outputs, &intent, "reject_all")).unwrap(),
    )
    .unwrap();
    let (review, status) = cli
        .human(&["batches", "plan", "--file", document.to_str().unwrap()])
        .await;
    assert_eq!(status, 0, "{review}");
    let plan = &review["result"];
    assert_eq!(plan["items"][0]["action"], "remux");
    assert_eq!(plan["items"][1]["action"], "copy_unchanged");
    let sdk_plan = call!(
        stdio,
        "streams.clean.batch_plan",
        input(&sdk_assets, &outputs, &intent, "reject_all"),
        json!({})
    );
    assert_eq!(
        plan["hash"], sdk_plan["hash"],
        "Equivalent execution retains one hash across registration, creator and completed probe audit"
    );
    let submission = [
        "batches",
        "submit",
        "--plan",
        plan["id"].as_str().unwrap(),
        "--plan-hash",
        plan["hash"].as_str().unwrap(),
        "--replay-scope",
        &scope,
        "--idempotency-key",
        "first-batch",
    ];
    let (accepted, status) = cli.human(&submission).await;
    assert_eq!(status, 0, "{accepted}");
    let (replayed, status) = cli.human(&submission).await;
    assert_eq!(status, 0, "{replayed}");
    assert_eq!(accepted["receipt_id"], replayed["receipt_id"]);
    assert_eq!(accepted["result"], replayed["result"]);
    let batch_id = accepted["result"]["batch_id"].as_str().unwrap();
    let (completed, status) = cli.human(&["batches", "wait", batch_id]).await;
    assert_eq!(status, 0, "{completed}");
    assert_eq!(completed["result"]["finished"], 2);
    assert_eq!(completed["result"]["succeeded"], 2);
    let child = cli
        .completed(completed["result"]["items"][0]["job_id"].as_str().unwrap())
        .await;
    assert_eq!(child["state"], "succeeded");
    assert_eq!(
        child["artifact"]["bytes"],
        completed["result"]["items"][0]["output_bytes"]
    );

    assert_eq!(completed["result"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&outputs[1]).unwrap()
    );
    let observed = fixture::probe(&media.bin, &outputs[0]).await;
    assert_eq!(observed["streams"].as_array().unwrap().len(), 2);
    assert_eq!(observed["streams"][1]["tags"]["language"], "eng");
    let original_packets = packets(&media.bin, &media.source).await;
    let retained_packets = packets(&media.bin, &outputs[0]).await;
    assert_eq!(original_packets.get(&0), retained_packets.get(&0));
    assert_eq!(original_packets.get(&1), retained_packets.get(&1));
    assert!(!retained_packets.contains_key(&2));

    // An operator cannot borrow the native owner's retry namespace.
    let denied = sdk!(
        stdio,
        "batches.submit",
        json!({"plan_id":sdk_plan["id"],"plan_hash":sdk_plan["hash"]}),
        json!({"replay_scope":scope,"idempotency_key":"first-batch"})
    );
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["error"]["code"], "REPLAY_SCOPE_INVALID");

    let sdk_outputs = [
        media.directory.path().join("sdk-remux.mkv"),
        media.directory.path().join("sdk-copy.mkv"),
    ];
    let sdk_plan = call!(
        stdio,
        "streams.clean.batch_plan",
        input(&sdk_assets, &sdk_outputs, &intent, "reject_all"),
        json!({})
    );
    let submit_params = json!({"plan_id":sdk_plan["id"],"plan_hash":sdk_plan["hash"]});
    let submit_context = json!({"replay_scope":sdk_scope,"idempotency_key":"sdk-batch"});
    let accepted_sdk = sdk!(
        stdio,
        "batches.submit",
        submit_params.clone(),
        submit_context.clone()
    );
    assert_eq!(accepted_sdk["ok"], true, "{accepted_sdk}");
    let retry_sdk = sdk!(stdio, "batches.submit", submit_params, submit_context);
    assert_eq!(accepted_sdk["receipt_id"], retry_sdk["receipt_id"]);
    assert_eq!(accepted_sdk["result"], retry_sdk["result"]);
    let completed_sdk = finished!(stdio, accepted_sdk["result"]["batch_id"]);
    assert_eq!(completed_sdk["succeeded"], 2);
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&sdk_outputs[1]).unwrap()
    );

    // A reviewed rejected candidate is retained and requires explicit continuation.
    let partial_outputs = [
        media.directory.path().join("partial.mkv"),
        media.directory.path().join("unsupported.mp4"),
    ];
    let rejected = cli
        .query(
            "streams.clean.batch_plan",
            input(&assets, &partial_outputs, &intent, "reject_all"),
        )
        .await;
    assert!(
        rejected["items"][1]["error"]["code"].is_string(),
        "{rejected}"
    );
    let refusal = cli
        .mutation(
            &scope,
            "rejected-batch",
            "batches.submit",
            json!({"plan_id":rejected["id"],"plan_hash":rejected["hash"]}),
        )
        .await;
    assert_eq!(refusal["ok"], false);
    assert_eq!(refusal["error"]["code"], "BATCH_REJECTED");
    let partial = cli
        .query(
            "streams.clean.batch_plan",
            input(&assets, &partial_outputs, &intent, "continue_with_rejected"),
        )
        .await;
    let accepted_partial = cli
        .mutation(
            &scope,
            "partial-batch",
            "batches.submit",
            json!({"plan_id":partial["id"],"plan_hash":partial["hash"]}),
        )
        .await;
    assert_eq!(accepted_partial["ok"], true, "{accepted_partial}");
    let (partial_result, status) = cli
        .human(&[
            "batches",
            "wait",
            accepted_partial["result"]["batch_id"].as_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 9, "{partial_result}");
    assert_eq!(partial_result["error"]["code"], "PARTIAL_BATCH_FAILURE");
    assert_eq!(partial_result["result"]["finished"], 2);
    assert_eq!(partial_result["result"]["succeeded"], 1);
    assert_eq!(partial_result["result"]["failed"], 1);
    assert_eq!(
        partial_result["result"]["successful_video_input_bytes"],
        std::fs::metadata(&media.source).unwrap().len().to_string()
    );
    assert!(partial_result["result"]["items"][1]["job_id"].is_null());
    assert!(!partial_outputs[1].exists());

    let mut skip_intent = intent.clone();
    skip_intent.audio_selection.selector = Selector::Languages {
        values: vec!["zzz".into()],
    };
    let skip_outputs = [
        media.directory.path().join("skip-a.mkv"),
        media.directory.path().join("skip-b.mkv"),
    ];
    let skip = call!(
        stdio,
        "streams.clean.batch_plan",
        input(&sdk_assets, &skip_outputs, &skip_intent, "reject_all"),
        json!({})
    );
    assert!(
        skip["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["action"] == "skip")
    );
    let skip = call!(
        stdio,
        "batches.submit",
        json!({"plan_id":skip["id"],"plan_hash":skip["hash"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"skip-batch"})
    );
    let skipped = finished!(stdio, skip["batch_id"]);
    assert_eq!(skipped["skipped"], 2);
    assert_eq!(skipped["succeeded"], 0);
    assert_eq!(skipped["successful_video_input_bytes"], "0");
    assert_eq!(skipped["successful_video_output_bytes"], "0");
    assert!(skip_outputs.iter().all(|path| !path.exists()));

    // Real publication coordination holds the first worker before the fence.
    // The second remains queued under the same serial utility scheduler.
    let cancel_outputs = [
        media.directory.path().join("cancel-a.mkv"),
        media.directory.path().join("cancel-b.mkv"),
    ];
    let stamp =
        nw_platform::files::DirectoryStamp::capture_resolved(media.directory.path()).unwrap();
    let claims = nw_platform::claims::DestinationClaims::try_acquire(vec![
        nw_platform::claims::Destination::reviewed(
            &cancel_outputs[0],
            &stamp.object_identity().unwrap(),
        )
        .unwrap(),
    ])
    .unwrap();
    let cancel_plan = call!(
        stdio,
        "streams.clean.batch_plan",
        input(
            &sdk_assets,
            &cancel_outputs,
            &fixture::rules(),
            "reject_all"
        ),
        json!({})
    );
    let cancel_batch = call!(
        stdio,
        "batches.submit",
        json!({"plan_id":cancel_plan["id"],"plan_hash":cancel_plan["hash"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"cancel-batch"})
    );
    let cancellation = call!(
        stdio,
        "batches.cancel",
        json!({"id":cancel_batch["batch_id"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"cancel-members"})
    );
    assert!(
        cancellation["members"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["cancel_requested"] == true)
    );
    let cancelled = finished!(stdio, cancel_batch["batch_id"]);
    assert_eq!(cancelled["cancelled"], 2, "{cancelled}");
    assert_eq!(cancelled["succeeded"], 0);
    assert!(cancel_outputs.iter().all(|path| !path.exists()));
    assert!(
        cancelled["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["artifact_id"].is_null())
    );
    drop(claims);
    let page = call!(stdio, "batches.list", json!({"limit":1}), json!({}));
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_string(), "{page}");
    let next = call!(
        stdio,
        "batches.list",
        json!({"limit":1,"cursor":page["next_cursor"]}),
        json!({})
    );
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);

    let bind = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = bind.local_addr().unwrap().port();
    drop(bind);
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/owner-evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let token_file = media.directory.path().join("auth/batch-http-token");
    let server_log = evidence.join("batch-http-controller-stderr.log");
    let mut server = cli
        .command()
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--access-profile",
            "batch-operator",
            "--token-file",
        ])
        .arg(&token_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&server_log).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let startup = std::time::Instant::now();
    while !token_file.exists() {
        assert!(
            server.try_wait().unwrap().is_none(),
            "HTTP owner exited before credential export"
        );
        assert!(
            startup.elapsed() < Duration::from_secs(60),
            "HTTP owner startup exceeded bound"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let token =
        String::from_utf8(nw_platform::security::read_private(&token_file, 256).unwrap()).unwrap();
    let transport = StreamableHttpClientTransport::with_client(
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(&token),
    );
    let http = ().serve(transport).await.unwrap();
    let http_scope =
        call!(http, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
    let http_outputs = [
        media.directory.path().join("http-remux.mkv"),
        media.directory.path().join("http-copy.mkv"),
    ];
    let http_plan = call!(
        http,
        "streams.clean.batch_plan",
        input(&sdk_assets, &http_outputs, &intent, "reject_all"),
        json!({})
    );
    let http_batch = call!(
        http,
        "batches.submit",
        json!({"plan_id":http_plan["id"],"plan_hash":http_plan["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"http-batch"})
    );
    let http_result = finished!(http, http_batch["batch_id"]);
    assert_eq!(http_result["succeeded"], 2);
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&http_outputs[1]).unwrap()
    );
    http.cancel().await.unwrap();
    server.start_kill().unwrap();
    server.wait().await.unwrap();

    let mut command = cli.command();
    command.arg("bridge");
    let agent = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let denied = sdk!(agent, "batches.list", json!({}), json!({}));
    assert_eq!(denied["error"]["code"], "PERMISSION_DENIED");
    agent.cancel().await.unwrap();
    stdio.cancel().await.unwrap();
    let report = json!({"source_commit":env!("GITHUB_SHA"),"case":"two_source_cleanup_batch",
        "transports":["typed_cli","official_sdk_stdio","official_sdk_streamable_http"],
        "cli_batch":batch_id,"sdk_batch":completed_sdk["id"],"http_batch":http_result["id"],
        "independent_retained_packet_equality":true,"copy_full_byte_equality":true,
        "partial_exit":9,"confirmed_cancelled":cancelled["cancelled"],"skipped":skipped["skipped"],
        "scope":"Explicit candidates only; directory expansion, companions, replacement and GUI batching remain separate qualification"});
    std::fs::write(
        evidence.join("batch-media-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NW_BATCH_MEDIA_REPORT={report}");
    lost_ack::verify(&media, Some(&second)).await;
}
