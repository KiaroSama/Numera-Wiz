//! Actual media, durable owner and official MCP transport integration, hosted CI only.
#[path = "../../../test-support/media.rs"]
mod fixture;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nw_domain::{
    commands::Request,
    mux::{StreamKind, StreamTagPatch, TagPatch},
};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{CallToolRequestParams, ProtocolVersion},
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};

#[path = "support/owner.rs"]
mod owner;
use owner::Cli;

macro_rules! call {
    ($client:expr, $id:expr, $params:expr, $context:expr) => {{
        let args = json!({"params":$params,"context":$context});
        let result = $client.call_tool(CallToolRequestParams::new($id).with_arguments(args.as_object().unwrap().clone())).await.unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["structuredContent"]["ok"], true, "{value}");
        value["structuredContent"]["result"].clone()
    }};
}
#[path = "support/lost_ack.rs"]
mod lost_ack;

#[tokio::test]
#[ignore = "Uses the CI-built CPU profile, actual owner processes and SDK clients"]
async fn cli_and_both_official_mcp_transports_share_real_verified_jobs() {
    nw_platform::init_logging();
    let media = fixture::generate().await;
    let cli = Cli {
        data: media.directory.path().join("data"),
        components: media.bin.clone(),
        request_timeout: Duration::from_secs(40),
    };
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"]
        .as_str()
        .unwrap()
        .to_owned();
    let (created, status) = cli
        .human(&[
            "access",
            "profiles",
            "create",
            "--name",
            "human-setup",
            "--read-root",
            media.directory.path().to_str().unwrap(),
            "--write-root",
            media.directory.path().to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{created}");
    assert_eq!(created["result"]["mode"], "operate");
    assert_eq!(created["result"]["owned_object_access"], true);
    assert!(
        created["result"]["allowed_command_ids"]
            .as_array()
            .unwrap()
            .iter()
            .all(|id| !id.as_str().unwrap().starts_with("access.profiles")
                && !id.as_str().unwrap().starts_with("access.tokens"))
    );
    let (duplicate, status) = cli
        .human(&[
            "access",
            "profiles",
            "create",
            "--name",
            "human-setup",
            "--json",
        ])
        .await;
    assert_eq!(status, 4);
    assert_eq!(duplicate["error"]["code"], "OBJECT_CONFLICT");
    let grants_file = media.directory.path().join("auth/replacement-grants.json");
    let (inspected, status) = cli
        .human(&[
            "access",
            "profiles",
            "inspect",
            "--name",
            "human-setup",
            "--grants-out",
            grants_file.to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0);
    assert_eq!(
        inspected["result"]["principal"],
        created["result"]["principal"]
    );
    let (denied, status) = cli
        .human(&[
            "--access-profile",
            "human-setup",
            "access",
            "profiles",
            "inspect",
            "--name",
            "human-setup",
            "--json",
        ])
        .await;
    assert_eq!(status, 5);
    assert_eq!(
        denied["error"]["code"], "PERMISSION_DENIED",
        "Scoped CLI never borrows its native bootstrap's owner authority"
    );
    let mut grants: Value = serde_json::from_slice(
        &nw_platform::security::read_private(&grants_file, 1024 * 1024).unwrap(),
    )
    .unwrap();
    grants["owned_object_access"] = json!(false);
    std::fs::write(&grants_file, serde_json::to_vec(&grants).unwrap()).unwrap();
    let (updated, status) = cli
        .human(&[
            "access",
            "profiles",
            "update",
            "--name",
            "human-setup",
            "--expected-revision",
            "1",
            "--grants-file",
            grants_file.to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{updated}");
    assert_eq!(updated["result"]["revision"], 2);
    assert_eq!(updated["result"]["owned_object_access"], false);
    assert_eq!(
        updated["result"]["principal"],
        created["result"]["principal"]
    );
    let registered = cli
        .mutation(
            &scope,
            "register-cli",
            "assets.register",
            json!({"path":media.source}),
        )
        .await;
    let (invalid, exit) = cli
        .raw_with_exit(&Request::query("media.probe", json!({"path":true})))
        .await;
    assert_eq!(invalid["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(
        exit, 2,
        "A typed CLI validation failure retains parseable JSON and the stable usage exit"
    );
    let mut conflicting = Request::query("assets.register", json!({"path":"different-source.mkv"}));
    conflicting.replay_scope = Some(scope.clone());
    conflicting.idempotency_key = Some("register-cli".into());
    let (conflict, exit) = cli.raw_with_exit(&conflicting).await;
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_CONFLICT");
    assert_eq!(
        exit, 4,
        "A durable conflict is distinguishable from a media execution failure"
    );
    assert_eq!(registered["ok"], true, "{registered}");
    let asset = registered["result"].clone();
    let report = cli
        .query(
            "streams.clean.report",
            json!({"asset_ids":[asset["id"]],"intent":fixture::rules()}),
        )
        .await;
    assert_eq!(report["resolved_files"], 1);
    assert_eq!(
        report["files"][0]["selected_original_indices"],
        json!([0, 1, 2])
    );
    assert_eq!(report["unique_streams"].as_array().unwrap().len(), 3);
    let output = media.directory.path().join("cli-copy.mkv");
    let plan = cli
        .query(
            "streams.clean",
            json!({"asset_id":asset["id"],"output":output,"intent":fixture::rules()}),
        )
        .await;
    let submit = json!({"plan_id":plan["id"],"plan_hash":plan["hash"]});
    let accepted = cli
        .mutation(&scope, "submit-cli", "jobs.submit", submit.clone())
        .await;
    assert_eq!(accepted["ok"], true, "{accepted}");
    let id = accepted["result"]["job_id"].as_str().unwrap();
    let job = cli.completed(id).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["file_times"]["policy"], "preserve_best_effort");
    assert_eq!(job["file_times"]["preserved"], true);
    assert_eq!(
        std::fs::read(&media.source).unwrap(),
        std::fs::read(&output).unwrap()
    );
    let artifact_id = format!("job-{id}");
    let manifest = cli
        .query("artifacts.manifest", json!({"artifact_id":artifact_id}))
        .await;
    assert_eq!(manifest["sha256"], job["artifact"]["sha256"]);
    assert_eq!(manifest["suggested_name"], "cli-copy.mkv");
    assert!(
        !manifest
            .to_string()
            .contains(media.directory.path().to_str().unwrap())
    );
    let saved_path = media.directory.path().join("client-saved-copy.mkv");
    let (saved, exit) = cli
        .human(&[
            "artifacts",
            "save",
            "--artifact-id",
            &artifact_id,
            "--expected-sha256",
            manifest["sha256"].as_str().unwrap(),
            "--out",
            saved_path.to_str().unwrap(),
        ])
        .await;
    assert_eq!(exit, 0, "{saved}");
    assert_eq!(saved["result"]["sha256"], manifest["sha256"]);
    assert_eq!(
        saved["result"]["encoded_byte_count"],
        manifest["encoded_byte_count"]
    );
    assert_eq!(
        std::fs::read(&saved_path).unwrap(),
        std::fs::read(&output).unwrap()
    );
    let (collision, exit) = cli
        .human(&[
            "artifacts",
            "save",
            "--artifact-id",
            &artifact_id,
            "--expected-sha256",
            manifest["sha256"].as_str().unwrap(),
            "--out",
            saved_path.to_str().unwrap(),
        ])
        .await;
    assert_ne!(exit, 0);
    assert_eq!(collision["error"]["code"], "OUTPUT_COLLISION");
    assert_eq!(
        std::fs::read(&saved_path).unwrap(),
        std::fs::read(&output).unwrap()
    );
    let wrong_path = media.directory.path().join("wrong-client-digest.mkv");
    let (changed, exit) = cli
        .human(&[
            "artifacts",
            "save",
            "--artifact-id",
            &artifact_id,
            "--expected-sha256",
            &"0".repeat(64),
            "--out",
            wrong_path.to_str().unwrap(),
        ])
        .await;
    assert_ne!(exit, 0);
    assert_eq!(changed["error"]["code"], "ARTIFACT_CHANGED");
    assert!(!wrong_path.exists());
    let denied_path = media.directory.path().join("foreign-client-copy.mkv");
    let (denied, exit) = cli
        .human(&[
            "--access-profile",
            "human-setup",
            "artifacts",
            "save",
            "--artifact-id",
            &artifact_id,
            "--expected-sha256",
            manifest["sha256"].as_str().unwrap(),
            "--out",
            denied_path.to_str().unwrap(),
        ])
        .await;
    assert_ne!(exit, 0);
    assert_eq!(denied["error"]["code"], "OBJECT_NOT_FOUND");
    assert!(!denied_path.exists());
    let chunk = cli.query("artifacts.read_chunk", json!({"artifact_id":artifact_id,"expected_sha256":manifest["sha256"],"offset":"3","length":29})).await;
    assert_eq!(
        STANDARD
            .decode(chunk["data_base64"].as_str().unwrap())
            .unwrap(),
        std::fs::read(&output).unwrap()[3..32]
    );
    let replay = cli
        .mutation(&scope, "submit-cli", "jobs.submit", submit)
        .await;
    assert_eq!(accepted["receipt_id"], replay["receipt_id"]);
    assert_eq!(accepted["result"]["job_id"], replay["result"]["job_id"]);
    assert_eq!(
        cli.query("jobs.list", json!({})).await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let profile = json!({"name":"operator","expected_revision":null,"enabled":true,"mode":"operate","owned_object_access":true,"allowed_command_ids":["commands.list","commands.schema","access.replay_scope.create","media.probe","assets.register","streams.clean","streams.clean.report","plans.inspect","jobs.submit","jobs.inspect","jobs.list","jobs.cancel","artifacts.list","artifacts.manifest","artifacts.read_chunk"],"object_grants":[],"read_roots":[media.directory.path()],"write_roots":[media.directory.path()]});
    assert_eq!(
        cli.mutation(
            &scope,
            "create-operator",
            "access.profiles.put",
            profile.clone()
        )
        .await["ok"],
        true
    );
    let mut command = cli.command();
    command.args(["bridge", "--access-profile", "operator"]);
    let stdio = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let observed = call!(
        stdio,
        "media.probe",
        json!({"path":media.source}),
        json!({})
    );
    assert_eq!(observed["streams"].as_array().unwrap().len(), 3);
    // A path grant does not authorize an object created by another principal.
    let denied = stdio
        .call_tool(
            CallToolRequestParams::new("jobs.inspect")
                .with_arguments(rmcp::object!({"params":{"id":id}})),
        )
        .await
        .unwrap();
    let denied = serde_json::to_value(denied).unwrap();
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        "OBJECT_NOT_FOUND"
    );
    let absent = stdio
        .call_tool(
            CallToolRequestParams::new("jobs.inspect")
                .with_arguments(rmcp::object!({"params":{"id":"absent-job"}})),
        )
        .await
        .unwrap();
    assert_eq!(
        denied["structuredContent"]["error"],
        serde_json::to_value(absent).unwrap()["structuredContent"]["error"],
        "Official MCP must not disclose unauthorized object existence"
    );
    let operator_scope =
        call!(stdio, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
    let operator_asset = call!(
        stdio,
        "assets.register",
        json!({"path":media.source}),
        json!({"replay_scope":operator_scope,"idempotency_key":"register-stdio"})
    );
    let report = call!(
        stdio,
        "streams.clean.report",
        json!({"asset_ids":[operator_asset["id"]],"intent":fixture::rules()}),
        json!({})
    );
    assert_eq!(report["resolved_files"], 1);
    assert_eq!(
        report["files"][0]["selected_original_indices"],
        json!([0, 1, 2])
    );
    let mut rules = fixture::rules();
    rules.audio_order = vec![2, 1];
    rules.stream_tag_patches = vec![StreamTagPatch {
        kind: StreamKind::Audio,
        source_indices: vec![2],
        original_languages: vec![],
        language: TagPatch::Keep,
        title: TagPatch::Set {
            value: "SDK stdio first".into(),
        },
    }];
    let output = media.directory.path().join("stdio-remux.mkv");
    let plan = call!(
        stdio,
        "streams.clean",
        json!({"asset_id":operator_asset["id"],"output":output,"intent":rules,"file_times":"require_reviewed"}),
        json!({})
    );
    let accepted = call!(
        stdio,
        "jobs.submit",
        json!({"plan_id":plan["id"],"plan_hash":plan["hash"]}),
        json!({"replay_scope":operator_scope,"idempotency_key":"submit-stdio"})
    );
    let job = cli.completed(accepted["job_id"].as_str().unwrap()).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["file_times"]["policy"], "require_reviewed");
    assert_eq!(job["file_times"]["preserved"], true);
    assert_eq!(job["file_times"]["desired"], job["file_times"]["observed"]);
    let observed = fixture::probe(&media.bin, &output).await;
    assert_eq!(observed["streams"][1]["tags"]["language"], "fas");
    assert_eq!(observed["streams"][1]["tags"]["title"], "SDK stdio first");

    let stdio_artifact_id = format!("job-{}", accepted["job_id"].as_str().unwrap());
    let stdio_manifest = call!(
        stdio,
        "artifacts.manifest",
        json!({"artifact_id":stdio_artifact_id}),
        json!({})
    );
    let resource = stdio
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            stdio_manifest["manifest_uri"].as_str().unwrap(),
        ))
        .await
        .unwrap();
    let resource = serde_json::to_value(resource).unwrap();
    assert_eq!(resource["contents"][0]["mimeType"], "application/json");
    let contents: Value =
        serde_json::from_str(resource["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(contents["sha256"], job["artifact"]["sha256"]);
    assert_eq!(contents["suggested_name"], "stdio-remux.mkv");
    let chunk = call!(
        stdio,
        "artifacts.read_chunk",
        json!({"artifact_id":stdio_artifact_id,"expected_sha256":contents["sha256"],"offset":"131067","length":31}),
        json!({})
    );
    assert_eq!(
        STANDARD
            .decode(chunk["data_base64"].as_str().unwrap())
            .unwrap(),
        std::fs::read(&output).unwrap()[131067..131098]
    );
    // Delegation is an explicit object action; it never borrows the creator's authority.
    let delegated_output = media.directory.path().join("delegated-copy.mkv");
    let delegated_plan = cli.query("streams.clean", json!({"asset_id":registered["result"]["id"],"output":delegated_output,"intent":fixture::rules()})).await;
    let equivalent = call!(
        stdio,
        "streams.clean",
        json!({"asset_id":operator_asset["id"],"output":delegated_output,"intent":fixture::rules()}),
        json!({})
    );
    assert_ne!(equivalent["id"], delegated_plan["id"]);
    assert_eq!(equivalent["schema_version"], 2);
    assert_eq!(
        equivalent["hash"], delegated_plan["hash"],
        "Equivalent native-file requirements must not hash creation principal, profile revision, registration ID or review timestamp"
    );
    assert_eq!(
        equivalent["required_rights"],
        delegated_plan["required_rights"]
    );
    let mut delegated_policy = profile.clone();
    delegated_policy["expected_revision"] = json!(1);
    delegated_policy["object_grants"] = json!([
        {"kind":"plan","id":delegated_plan["id"],"actions":["read","submit"]},
        {"kind":"job","id":id,"actions":["read"]}
    ]);
    assert_eq!(
        cli.mutation(
            &scope,
            "delegate-plan-and-job",
            "access.profiles.put",
            delegated_policy.clone()
        )
        .await["ok"],
        true
    );
    let inspected = call!(
        stdio,
        "plans.inspect",
        json!({"id":delegated_plan["id"]}),
        json!({})
    );
    assert_eq!(inspected["hash"], delegated_plan["hash"]);
    assert!(
        inspected["argv_template"].is_null(),
        "Scoped views omit private invocation internals"
    );
    let shared_job = call!(stdio, "jobs.inspect", json!({"id":id}), json!({}));
    assert!(shared_job["prepared_stages"].is_null());
    assert!(shared_job["plan"]["ffmpeg"].get("path").is_none());
    assert!(shared_job["plan"]["ffmpeg"]["sha256"].is_string());
    let control = stdio.call_tool(CallToolRequestParams::new("jobs.cancel").with_arguments(json!({"params":{"id":id},"context":{"replay_scope":operator_scope,"idempotency_key":"denied-shared-control"}}).as_object().unwrap().clone())).await.unwrap();
    assert_eq!(
        serde_json::to_value(control).unwrap()["structuredContent"]["error"]["code"],
        "OBJECT_NOT_FOUND"
    );
    let delegated_job = call!(
        stdio,
        "jobs.submit",
        json!({"plan_id":delegated_plan["id"],"plan_hash":delegated_plan["hash"]}),
        json!({"replay_scope":operator_scope,"idempotency_key":"delegated-submit"})
    );
    assert_eq!(
        cli.completed(delegated_job["job_id"].as_str().unwrap())
            .await["state"],
        "succeeded"
    );
    assert_eq!(
        std::fs::read(&media.source).unwrap(),
        std::fs::read(&delegated_output).unwrap()
    );
    delegated_policy["expected_revision"] = json!(2);
    delegated_policy["owned_object_access"] = json!(false);
    delegated_policy["object_grants"][0]["actions"] = json!(["read"]);
    assert_eq!(
        cli.mutation(
            &scope,
            "withdraw-inherited-and-submit",
            "access.profiles.put",
            delegated_policy
        )
        .await["ok"],
        true
    );
    let visible = call!(stdio, "jobs.list", json!({}), json!({}));
    assert_eq!(
        visible["total_count"], 1,
        "Only the explicit read grant survives withdrawal of inherited creator access"
    );
    assert_eq!(visible["items"][0]["id"], id);
    let replay = stdio.call_tool(CallToolRequestParams::new("assets.register").with_arguments(json!({"params":{"path":media.source},"context":{"replay_scope":operator_scope,"idempotency_key":"register-stdio"}}).as_object().unwrap().clone())).await.unwrap();
    let replay = serde_json::to_value(replay).unwrap();
    assert_eq!(
        replay["structuredContent"]["error"]["code"],
        "RECEIPT_RESULT_RESTRICTED"
    );
    assert!(
        replay["structuredContent"]["result"]
            .get("identity")
            .is_none()
    );
    let mut restored = profile.clone();
    restored["expected_revision"] = json!(3);
    assert_eq!(
        cli.mutation(
            &scope,
            "restore-operator-for-http",
            "access.profiles.put",
            restored
        )
        .await["ok"],
        true
    );
    stdio.cancel().await.unwrap();

    let mut modern_command = cli.command();
    modern_command.args(["bridge", "--access-profile", "operator"]);
    let modern_stdio = ()
        .serve_with_lifecycle(
            TokioChildProcess::new(modern_command).unwrap(),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    let modern_manifest = modern_stdio
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            stdio_manifest["manifest_uri"].as_str().unwrap(),
        ))
        .await
        .unwrap();
    let modern_manifest = serde_json::to_value(modern_manifest).unwrap();
    assert_eq!(
        modern_manifest["contents"][0]["mimeType"],
        "application/json"
    );
    let modern_chunk = call!(
        modern_stdio,
        "artifacts.read_chunk",
        json!({"artifact_id":stdio_artifact_id,"expected_sha256":stdio_manifest["sha256"],"offset":"0","length":29}),
        json!({})
    );
    assert_eq!(
        STANDARD
            .decode(modern_chunk["data_base64"].as_str().unwrap())
            .unwrap(),
        std::fs::read(&output).unwrap()[..29]
    );
    modern_stdio.cancel().await.unwrap();

    let bind = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = bind.local_addr().unwrap().port();
    drop(bind);
    let token_file = media.directory.path().join("auth/http-token");
    let evidence =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/owner-evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let server_log = evidence.join("http-controller-stderr.log");
    let mut server = cli
        .command()
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--access-profile",
            "operator",
            "--token-file",
        ])
        .arg(&token_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&server_log).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        if token_file.exists() {
            break;
        }
        if let Some(status) = server.try_wait().unwrap() {
            panic!(
                "HTTP controller exited before protected credential export: {status}; {}",
                std::fs::read_to_string(&server_log).unwrap()
            );
        }
        assert!(
            started.elapsed() < Duration::from_secs(45),
            "HTTP controller startup deadline exceeded; {}",
            std::fs::read_to_string(&server_log).unwrap()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let token =
        String::from_utf8(nw_platform::security::read_private(&token_file, 256).unwrap()).unwrap();
    let issued_file = media.directory.path().join("auth/revocation-token");
    let selected_port = port.to_string();
    let issue_args = [
        "access",
        "tokens",
        "issue",
        "--for-access-profile",
        "operator",
        "--port",
        &selected_port,
        "--out",
        issued_file.to_str().unwrap(),
        "--json",
    ];
    let (issued, status) = cli.human(&issue_args).await;
    assert_eq!(status, 0, "{issued}");
    let issued_bearer =
        String::from_utf8(nw_platform::security::read_private(&issued_file, 256).unwrap()).unwrap();
    assert!(
        !issued.to_string().contains(&issued_bearer),
        "Token stdout contains metadata only"
    );
    let (collision, status) = cli.human(&issue_args).await;
    assert_eq!(status, 4);
    assert_eq!(collision["error"]["code"], "OUTPUT_COLLISION");
    assert_eq!(
        nw_platform::security::read_private(&issued_file, 256).unwrap(),
        issued_bearer.as_bytes()
    );
    let direct = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let endpoint = format!("http://127.0.0.1:{port}/api/v1/commands");
    let response = direct
        .post(&endpoint)
        .bearer_auth(&issued_bearer)
        .json(&Request::query("commands.list", json!({})))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
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
    assert_eq!(revoked["result"]["revoked"], true);
    assert_eq!(
        direct
            .post(&endpoint)
            .bearer_auth(&issued_bearer)
            .json(&Request::query("commands.list", json!({})))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    for (ordinal, lifecycle) in [
        ClientLifecycleMode::Initialize,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    ]
    .into_iter()
    .enumerate()
    {
        let transport = StreamableHttpClientTransport::with_client(
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
                .auth_header(&token),
        );
        let http = ().serve_with_lifecycle(transport, lifecycle).await.unwrap();
        let probed = call!(http, "media.probe", json!({"path":media.source}), json!({}));
        assert_eq!(probed["streams"].as_array().unwrap().len(), 3);
        assert_eq!(probed["streams"][1]["tags"]["language"], "eng");
        let scope = call!(http, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let asset = call!(
            http,
            "assets.register",
            json!({"path":media.source}),
            json!({"replay_scope":scope,"idempotency_key":format!("register-http-{ordinal}")})
        );
        let report = call!(
            http,
            "streams.clean.report",
            json!({"asset_ids":[asset["id"]],"intent":fixture::rules()}),
            json!({})
        );
        assert_eq!(report["resolved_files"], 1);
        assert_eq!(
            report["files"][0]["selected_original_indices"],
            json!([0, 1, 2])
        );
        let output = media.directory.path().join(format!("http-{ordinal}.mkv"));
        let plan = call!(
            http,
            "streams.clean",
            json!({"asset_id":asset["id"],"output":output,"intent":fixture::rules()}),
            json!({})
        );
        let submit = json!({"plan_id":plan["id"],"plan_hash":plan["hash"]});
        let identity =
            json!({"replay_scope":scope,"idempotency_key":format!("submit-http-{ordinal}")});
        let accepted = call!(http, "jobs.submit", submit.clone(), identity.clone());
        let repeated = call!(http, "jobs.submit", submit, identity);
        assert_eq!(accepted["job_id"], repeated["job_id"]);
        let job = cli.completed(accepted["job_id"].as_str().unwrap()).await;
        assert_eq!(job["state"], "succeeded", "{job}");
        assert_eq!(
            std::fs::read(&media.source).unwrap(),
            std::fs::read(output).unwrap()
        );
        let artifact_id = format!("job-{}", accepted["job_id"].as_str().unwrap());
        let manifest = call!(
            http,
            "artifacts.manifest",
            json!({"artifact_id":artifact_id}),
            json!({})
        );
        assert_eq!(manifest["suggested_name"], format!("http-{ordinal}.mkv"));
        let resource = http
            .read_resource(rmcp::model::ReadResourceRequestParams::new(
                manifest["manifest_uri"].as_str().unwrap(),
            ))
            .await
            .unwrap();
        let resource = serde_json::to_value(resource).unwrap();
        assert_eq!(resource["contents"][0]["mimeType"], "application/json");
        let chunk = call!(
            http,
            "artifacts.read_chunk",
            json!({"artifact_id":artifact_id,"expected_sha256":manifest["sha256"],"offset":"0","length":31}),
            json!({})
        );
        assert_eq!(
            STANDARD
                .decode(chunk["data_base64"].as_str().unwrap())
                .unwrap(),
            std::fs::read(&media.source).unwrap()[..31]
        );
        http.cancel().await.unwrap();
    }

    // Browser intake bytes use their own bounded route; canonical registration/job
    // acceptance still uses the same official SDK and durable receipt contracts.
    let browser_token_file = media.directory.path().join("auth/browser-token");
    let (issued, status) = cli
        .human(&[
            "access",
            "tokens",
            "issue",
            "--for-access-profile",
            "browser",
            "--port",
            &port.to_string(),
            "--out",
            browser_token_file.to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{issued}");
    let browser_bearer =
        String::from_utf8(nw_platform::security::read_private(&browser_token_file, 256).unwrap())
            .unwrap();
    assert!(
        !issued.to_string().contains(&browser_bearer),
        "Token issuance must never print a bearer"
    );
    let bearer = browser_bearer.as_str();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let endpoint = format!("http://127.0.0.1:{port}");
    let fixture_bytes = std::fs::read(&media.source).unwrap();
    let ticket: Value = client
        .post(format!("{endpoint}/api/v1/uploads"))
        .bearer_auth(bearer)
        .json(
            &json!({"display_name":"Browser original.mkv","bytes":fixture_bytes.len().to_string()}),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let completed: Value = client
        .put(format!(
            "{endpoint}/api/v1/uploads/{}",
            ticket["upload_id"].as_str().unwrap()
        ))
        .bearer_auth(bearer)
        .header("Content-Type", "application/octet-stream")
        .body(fixture_bytes.clone())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(completed["upload_id"], ticket["upload_id"]);
    let transport = StreamableHttpClientTransport::with_client(
        client.clone(),
        StreamableHttpClientTransportConfig::with_uri(format!("{endpoint}/mcp"))
            .auth_header(bearer),
    );
    let browser = ().serve(transport).await.unwrap();
    let browser_scope =
        call!(browser, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
    let identity =
        json!({"replay_scope":browser_scope,"idempotency_key":"register-browser-intake"});
    let asset = call!(
        browser,
        "assets.register",
        json!({"upload_id":ticket["upload_id"]}),
        identity.clone()
    );
    let retry = call!(
        browser,
        "assets.register",
        json!({"upload_id":ticket["upload_id"]}),
        identity
    );
    assert_eq!(asset["id"], retry["id"]);
    assert_eq!(asset["display_name"], "Browser original.mkv");
    let denied: Value = client
        .post(format!("{endpoint}/api/v1/commands"))
        .bearer_auth(&token)
        .json(&Request::query(
            "media.probe",
            json!({"path":asset["identity"]["path"]}),
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["error"]["code"], "PERMISSION_DENIED");
    let destination = call!(
        browser,
        "assets.output_suggest",
        json!({"id":asset["id"]}),
        json!({})
    );
    let plan = call!(
        browser,
        "streams.clean",
        json!({"asset_id":asset["id"],"output":destination["output"],"intent":fixture::rules()}),
        json!({})
    );
    let accepted = call!(
        browser,
        "jobs.submit",
        json!({"plan_id":plan["id"],"plan_hash":plan["hash"]}),
        json!({"replay_scope":browser_scope,"idempotency_key":"submit-browser-intake"})
    );
    let job = cli.completed(accepted["job_id"].as_str().unwrap()).await;
    assert_eq!(job["state"], "succeeded");
    assert_eq!(
        std::fs::read(destination["output"].as_str().unwrap()).unwrap(),
        fixture_bytes
    );
    // The browser profile has write-only exports. Its job purpose still admits exact output reads.
    let artifact_id = format!("job-{}", accepted["job_id"].as_str().unwrap());
    let manifest = call!(
        browser,
        "artifacts.manifest",
        json!({"artifact_id":artifact_id}),
        json!({})
    );
    assert!(!manifest.to_string().contains(cli.data.to_str().unwrap()));
    let bytes_url = format!("{endpoint}{}", manifest["bytes_route"].as_str().unwrap());
    let etag = format!("\"{}\"", manifest["sha256"].as_str().unwrap());
    let response = client
        .get(&bytes_url)
        .bearer_auth(bearer)
        .header("If-Match", &etag)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["etag"], etag);
    assert_eq!(response.bytes().await.unwrap().as_ref(), fixture_bytes);
    for (range, start, end) in [
        ("bytes=3-19", 3, 20),
        ("bytes=-13", fixture_bytes.len() - 13, fixture_bytes.len()),
        ("bytes=131067-131097", 131067, 131098),
    ] {
        let response = client
            .get(&bytes_url)
            .bearer_auth(bearer)
            .header("Range", range)
            .header("If-Match", &etag)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.bytes().await.unwrap().as_ref(),
            &fixture_bytes[start..end]
        );
    }
    assert_eq!(
        client
            .get(&bytes_url)
            .bearer_auth(bearer)
            .header("Range", "bytes=0-1,4-5")
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE
    );
    assert_eq!(
        client
            .get(&bytes_url)
            .bearer_auth(bearer)
            .header("If-Match", format!("\"{}\"", "0".repeat(64)))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        client
            .get(&bytes_url)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND,
        "A copied output route supplies no object authority"
    );
    let original = std::path::Path::new(destination["output"].as_str().unwrap());
    std::fs::rename(original, original.with_extension("retained-original")).unwrap();
    std::fs::write(original, &fixture_bytes).unwrap();
    assert_eq!(
        client
            .get(&bytes_url)
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::PRECONDITION_FAILED,
        "Identical replacement bytes are not the published physical object"
    );
    browser.cancel().await.unwrap();
    let status = cli.query("logs.status", json!({})).await;
    assert_eq!(status["configured"], true);
    assert_eq!(status["writer_healthy"], true);
    assert_eq!(status["retained_file_limit"], 5);
    let logs = std::path::Path::new(status["directory"].as_str().unwrap());
    let mut events = 0;
    for entry in std::fs::read_dir(logs).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            let bytes = nw_platform::security::read_private(&path, 2 * 1024 * 1024 + 1024).unwrap();
            let text = String::from_utf8(bytes.clone()).unwrap();
            for value in [&token, &issued_bearer, &browser_bearer] {
                assert!(
                    !text.contains(value.as_str()),
                    "Protected logs must not contain a transport bearer"
                );
            }
            for line in bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let row = nw_domain::commands::decode_json(line).unwrap();
                assert!(
                    ["INFO", "WARNING", "ERROR", "DEBUG"].contains(&row["level"].as_str().unwrap())
                );
                assert!(row["timestamp"].is_string());
                if row.get("fields").is_some() {
                    events += 1;
                }
            }
        }
    }
    assert!(
        events > 0,
        "Actual detached-owner work must produce protected structured events"
    );
    server.kill().await.unwrap();
    server.wait().await.unwrap();
    lost_ack::verify(&media).await;
}
