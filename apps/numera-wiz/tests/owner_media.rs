//! Actual media, durable owner and official MCP transport integration, hosted CI only.
#[path = "../../../test-support/media.rs"]
mod fixture;
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
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

struct Cli {
    data: PathBuf,
    components: PathBuf,
}
impl Cli {
    fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command
            .arg("--data-dir")
            .arg(&self.data)
            .arg("--components-dir")
            .arg(&self.components);
        command
    }
    async fn raw(&self, request: &Request) -> Value {
        let mut child = self
            .command()
            .arg("request")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        input
            .write_all(&serde_json::to_vec(request).unwrap())
            .await
            .unwrap();
        drop(input);
        let output = tokio::time::timeout(Duration::from_secs(40), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("CLI failed: {}", String::from_utf8_lossy(&output.stderr)))
    }
    async fn query(&self, id: &str, params: Value) -> Value {
        let response = self.raw(&Request::query(id, params)).await;
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    }
    async fn mutation(&self, scope: &str, key: &str, id: &str, params: Value) -> Value {
        let mut request = Request::query(id, params);
        request.replay_scope = Some(scope.into());
        request.idempotency_key = Some(key.into());
        self.raw(&request).await
    }
    async fn completed(&self, id: &str) -> Value {
        for _ in 0..100 {
            let job = self.query("jobs.inspect", json!({"id":id})).await;
            if ["succeeded", "failed", "cancelled", "interrupted", "skipped"]
                .contains(&job["state"].as_str().unwrap())
            {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("job completion deadline exceeded");
    }
}
macro_rules! call {
    ($client:expr, $id:expr, $params:expr, $context:expr) => {{
        let args = json!({"params":$params,"context":$context});
        let result = $client.call_tool(CallToolRequestParams::new($id).with_arguments(args.as_object().unwrap().clone())).await.unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["structuredContent"]["ok"], true, "{value}");
        value["structuredContent"]["result"].clone()
    }};
}
#[tokio::test]
#[ignore = "Uses the CI-built CPU profile, actual owner processes and SDK clients"]
async fn cli_and_both_official_mcp_transports_share_real_verified_jobs() {
    nw_platform::init_logging();
    let media = fixture::generate().await;
    let cli = Cli {
        data: media.directory.path().join("data"),
        components: media.bin.clone(),
    };
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"]
        .as_str()
        .unwrap()
        .to_owned();
    let registered = cli
        .mutation(
            &scope,
            "register-cli",
            "assets.register",
            json!({"path":media.source}),
        )
        .await;
    assert_eq!(registered["ok"], true, "{registered}");
    let asset = registered["result"].clone();
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
    assert_eq!(
        std::fs::read(&media.source).unwrap(),
        std::fs::read(&output).unwrap()
    );
    let replay = cli
        .mutation(&scope, "submit-cli", "jobs.submit", submit)
        .await;
    assert_eq!(accepted["receipt_id"], replay["receipt_id"]);
    assert_eq!(accepted["result"]["job_id"], replay["result"]["job_id"]);
    assert_eq!(
        cli.query("jobs.list", json!({}))
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let profile = json!({"name":"operator","expected_revision":null,"enabled":true,"commands":["commands.list","commands.schema","access.replay_scope.create","media.probe","assets.register","streams.clean","plans.inspect","jobs.submit","jobs.inspect","jobs.list","jobs.cancel"],"objects":[],"read_roots":[media.directory.path()],"write_roots":[media.directory.path()]});
    assert_eq!(
        cli.mutation(&scope, "create-operator", "access.profiles.put", profile)
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
    assert_eq!(
        serde_json::to_value(denied).unwrap()["structuredContent"]["error"]["code"],
        "PERMISSION_DENIED"
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
        json!({"asset_id":operator_asset["id"],"output":output,"intent":rules}),
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
    let observed = fixture::probe(&media.bin, &output).await;
    assert_eq!(observed["streams"][1]["tags"]["language"], "fas");
    assert_eq!(observed["streams"][1]["tags"]["title"], "SDK stdio first");
    stdio.cancel().await.unwrap();

    let bind = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = bind.local_addr().unwrap().port();
    drop(bind);
    let token_file = media.directory.path().join("auth/http-token");
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
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if token_file.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let token =
        String::from_utf8(nw_platform::security::read_private(&token_file, 256).unwrap()).unwrap();
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
        http.cancel().await.unwrap();
    }
    server.kill().await.unwrap();
    server.wait().await.unwrap();
}
