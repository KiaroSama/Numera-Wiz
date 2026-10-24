//! Real SDK retry after accepted owner responses are lost in transport.
use super::{fixture, owner::Cli};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[path = "evidence.rs"]
mod evidence;

const LOST_REQUEST: &str = "lost-ack-submit";

#[derive(Default)]
struct Loss {
    original_body: Option<Vec<u8>>,
    accepted: Option<Value>,
    forwards: usize,
    identical: bool,
}
struct Proxy {
    command: &'static str,
    result_key: &'static str,
    upstream: String,
    http: reqwest::Client,
    loss: Mutex<Loss>,
}
struct StopProxy(CancellationToken);
impl Drop for StopProxy {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn forward(
    State(proxy): State<Arc<Proxy>>,
    request: Request,
) -> Result<Response, StatusCode> {
    let (parts, body) = request.into_parts();
    if !["/health", "/internal/credential", "/api/v1/commands"].contains(&parts.uri.path())
        || parts.uri.query().is_some()
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let bytes = to_bytes(body, 1024 * 1024)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let mut upstream = proxy.http.request(
        parts.method,
        format!("{}{}", proxy.upstream, parts.uri.path()),
    );
    for name in [header::AUTHORIZATION, header::CONTENT_TYPE] {
        if let Some(value) = parts.headers.get(&name) {
            upstream = upstream.header(name, value);
        }
    }
    let mut response = upstream
        .body(bytes.clone())
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = response.status();
    let mut returned = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
    {
        if returned.len().saturating_add(chunk.len()) > 256 * 1024 {
            return Err(StatusCode::BAD_GATEWAY);
        }
        returned.extend_from_slice(&chunk);
    }
    let submitted = if parts.uri.path() == "/api/v1/commands" {
        nw_domain::commands::decode_json(&bytes).ok()
    } else {
        None
    };
    if submitted.as_ref().is_some_and(|request| {
        request["command"]["id"] == proxy.command && request["request_id"] == LOST_REQUEST
    }) {
        let accepted =
            nw_domain::commands::decode_json(&returned).map_err(|_| StatusCode::BAD_GATEWAY)?;
        if !status.is_success()
            || accepted["ok"] != true
            || !accepted["receipt_id"].is_string()
            || !accepted["result"][proxy.result_key].is_string()
        {
            return Err(StatusCode::BAD_GATEWAY);
        }
        let mut loss = proxy.loss.lock().await;
        if loss.forwards >= 4 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        loss.forwards += 1;
        if let (Some(original), Some(previous)) = (&loss.original_body, &loss.accepted) {
            let identical = original.as_slice() == bytes.as_ref()
                && previous["receipt_id"] == accepted["receipt_id"]
                && previous["result"] == accepted["result"];
            loss.identical &= identical;
        } else {
            loss.original_body = Some(bytes.to_vec());
            loss.identical = true;
            loss.accepted = Some(accepted);
        }
        // The real owner accepted/replayed this mutation. Its receipt never reaches the SDK.
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":"TRANSPORT_ERROR","message":"Fixture lost an accepted response"}})),
        )
            .into_response());
    }
    Ok(Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(returned))
        .unwrap())
}

fn runtime(path: &std::path::Path) -> Value {
    serde_json::from_slice(&nw_platform::security::read_private(path, 16384).unwrap()).unwrap()
}
fn scope_claims(scope: &Value) -> Value {
    let encoded = scope["replay_scope"].as_str().unwrap();
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded.split_once('.').unwrap().0)
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
async fn wait_for_idle(root: &std::path::Path) {
    let started = std::time::Instant::now();
    loop {
        match nw_platform::files::RootLock::acquire(&root.join("state/generations/1")) {
            Ok(lock) => {
                drop(lock);
                return;
            }
            Err(error) => assert_eq!(error.code, "OWNER_BUSY"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(35),
            "Lost-response owner did not finish its idle drain"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn verify(media: &fixture::Media, second: Option<&std::path::Path>) {
    tokio::time::timeout(Duration::from_secs(240), journey(media, second))
        .await
        .expect("Actual lost-response SDK restart deadline");
}
async fn journey(media: &fixture::Media, second: Option<&std::path::Path>) {
    let label = if second.is_some() { "lost-batch-ack" } else { "lost-ack" };
    let command_id = if second.is_some() { "batches.submit" } else { "jobs.submit" };
    let result_key = if second.is_some() { "batch_id" } else { "job_id" };
    let cli = Cli {
        data: media.directory.path().join(format!("{label}-data")),
        components: media.bin.clone(),
        request_timeout: Duration::from_secs(40),
    };
    let evidence_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/owner-evidence");
    let _logs = evidence::RetainLogs {
        source: cli.data.join("logs"),
        destination: evidence_root.join(format!("lost-ack-{}", uuid::Uuid::new_v4())),
    };
    let (profile, exit) = cli
        .human(&[
            "access",
            "profiles",
            "create",
            "--name",
            "restart-operator",
            "--read-root",
            media.directory.path().to_str().unwrap(),
            "--write-root",
            media.directory.path().to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(exit, 0, "Lost-response profile: {}", profile["error"]);
    let runtime_path = cli.data.join("runtime/owner.json");
    let original = runtime(&runtime_path);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let proxy = Arc::new(Proxy {
        command: command_id,
        result_key,
        upstream: format!("http://127.0.0.1:{}", original["port"].as_u64().unwrap()),
        http: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap(),
        loss: Mutex::new(Loss::default()),
    });
    let stop = StopProxy(CancellationToken::new());
    let shutdown = stop.0.clone();
    let router = Router::new()
        .fallback(forward)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(proxy.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    // Redirect only the protected transport endpoint; every handler remains the actual owner.
    let mut through_proxy = original.clone();
    through_proxy["port"] = json!(port);
    nw_platform::security::write_private(
        &runtime_path,
        &serde_json::to_vec(&through_proxy).unwrap(),
        true,
    )
    .unwrap();
    let mut command = cli.command();
    command.args(["bridge", "--access-profile", "restart-operator"]);
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let scope = call!(client, "access.replay_scope.create", json!({}), json!({}));
    let first_claims = scope_claims(&scope);
    let asset = call!(
        client,
        "assets.register",
        json!({"path":media.source}),
        json!({"replay_scope":scope["replay_scope"],"idempotency_key":"lost-ack-register"})
    );
    let output = media.directory.path().join(format!("{label}-copy.mkv"));
    let second_output = media.directory.path().join(format!("{label}-second-copy.mkv"));
    let plan = if let Some(second) = second {
        let second_asset = call!(client, "assets.register", json!({"path":second}),
            json!({"replay_scope":scope["replay_scope"],"idempotency_key":"lost-ack-register-second"}));
        call!(client,"streams.clean.batch_plan",json!({
            "items":[{"asset_id":asset["id"],"output":output},{"asset_id":second_asset["id"],"output":second_output}],
            "intent":fixture::rules(),"file_times":"new",
            "execution_limits":nw_domain::execution::CleanupLimits::default(),
            "failure_policy":"reject_all","unresolved_policy":"reject"
        }),json!({}))
    } else {
        call!(client,"streams.clean",
            json!({"asset_id":asset["id"],"output":output,"intent":fixture::rules()}),json!({}))
    };
    let arguments = json!({
        "params":{"plan_id":plan["id"],"plan_hash":plan["hash"]},
        "context":{
            "request_id":LOST_REQUEST,
            "replay_scope":scope["replay_scope"],
            "idempotency_key":"lost-ack-durable-submit"
        }
    });
    let lost = client
        .call_tool(
            CallToolRequestParams::new(command_id)
                .with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let lost = serde_json::to_value(lost).unwrap();
    assert_eq!(lost["structuredContent"]["ok"], false);
    assert_eq!(
        lost["structuredContent"]["error"]["code"],
        "TRANSPORT_ERROR"
    );
    assert_eq!(lost["structuredContent"]["request_id"], LOST_REQUEST);
    assert!(lost["structuredContent"].get("receipt_id").is_none());
    assert!(lost["structuredContent"].get("result").is_none());
    let accepted = {
        let loss = proxy.loss.lock().await;
        assert_eq!(
            loss.forwards, 2,
            "Both automatic attempts lose their response"
        );
        assert!(
            loss.identical,
            "Automatic retry changed request or durable identity"
        );
        loss.accepted.clone().unwrap()
    };
    let expected_jobs = if second.is_some() { 2 } else { 1 };
    let member_ids = if second.is_some() {
        accepted["result"]["members"].as_array().unwrap().iter()
            .map(|member| member["job_id"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
    } else { vec![accepted["result"]["job_id"].as_str().unwrap().to_owned()] };
    assert_eq!(member_ids.len(),expected_jobs);
    for id in &member_ids { assert_eq!(cli.completed(id).await["state"],"succeeded"); }
    if let Some(second) = second {
        assert_eq!(std::fs::read(second).unwrap(),std::fs::read(&second_output).unwrap());
        let group = cli.query("batches.inspect",json!({"id":accepted["result"]["batch_id"]})).await;
        assert_eq!(group["total"],2); assert_eq!(group["succeeded"],2); assert_eq!(group["complete"],true);
    }
    assert_eq!(
        std::fs::read(&media.source).unwrap(),
        std::fs::read(&output).unwrap()
    );
    nw_platform::security::write_private(
        &runtime_path,
        &serde_json::to_vec(&original).unwrap(),
        true,
    )
    .unwrap();
    stop.0.cancel();
    server.await.unwrap();
    wait_for_idle(&cli.data).await;

    // Keep the same SDK/stdio process and the exact original durable command.
    let replayed = client
        .call_tool(
            CallToolRequestParams::new(command_id)
                .with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let replayed = serde_json::to_value(replayed).unwrap();
    let replayed = &replayed["structuredContent"];
    assert_eq!(replayed["ok"], true, "Replay error: {}", replayed["error"]);
    assert_eq!(replayed["request_id"], LOST_REQUEST);
    assert_eq!(replayed["receipt_id"], accepted["receipt_id"]);
    assert_eq!(replayed["result"], accepted["result"]);
    let restarted = runtime(&runtime_path);
    assert_eq!(restarted["root_id"], original["root_id"]);
    assert_ne!(restarted["epoch"], original["epoch"]);
    let next = call!(client, "access.replay_scope.create", json!({}), json!({}));
    let next_claims = scope_claims(&next);
    assert_eq!(next_claims["principal"], first_claims["principal"]);
    assert_eq!(next_claims["root"], first_claims["root"]);
    let jobs = cli.query("jobs.list", json!({})).await;
    assert_eq!(jobs["total_count"],expected_jobs);
    let mut observed = jobs["items"].as_array().unwrap().iter().map(|job| job["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    let mut expected = member_ids.clone(); observed.sort(); expected.sort(); assert_eq!(observed,expected);
    if second.is_some() {
        let batches = cli.query("batches.list",json!({})).await;
        assert_eq!(batches["total_count"],1);
        assert_eq!(batches["items"][0]["id"],accepted["result"]["batch_id"]);
    }
    let bytes = nw_platform::files::identity(&output, &CancellationToken::new()).unwrap();
    std::fs::create_dir_all(&evidence_root).unwrap();
    std::fs::write(
        evidence_root.join(format!("{label}-restart.json")),
        serde_json::to_vec_pretty(&json!({
            "schema_version":1,
            "boundary":"actual_owner_official_sdk_stdio",
            "lost_accepted_responses":2,
            "automatic_retry_request_unchanged":true,
            "same_receipt_after_restart":true,
            "same_job_after_restart":true,
            "durable_job_count":expected_jobs,
            "command":command_id,"accepted_identity":accepted["result"][result_key],
            "member_ids":member_ids,
            "root_and_principal_unchanged":true,
            "owner_epoch_changed":true,
            "output_bytes":bytes.bytes,
            "output_sha256":bytes.sha256,
            "owner_log_only":true
        }))
        .unwrap(),
    )
    .unwrap();
    tracing::info!(lost_responses = 2, output_bytes = %bytes.bytes, output_sha256 = %bytes.sha256, "Actual SDK lost-response restart retained one durable receipt and job");
    client.cancel().await.unwrap();
    wait_for_idle(&cli.data).await;
}
