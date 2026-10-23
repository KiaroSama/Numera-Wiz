//! One owner process serves private IPC and the explicitly enabled public listener.
use crate::client::{OwnerRecord, registry_fingerprint};
use anyhow::{Result, Context as _};
use axum::{Router, Json, extract::{State, Request, DefaultBodyLimit}, http::{StatusCode, header}, middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}};
use nw_domain::commands;
use nw_service::{Service, Context};
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::Mutex};
use tokio_util::sync::CancellationToken;

struct Lease { principal: String, expires: Instant }
pub struct Owner {
    service: Arc<Service>,
    record: OwnerRecord,
    leases: Mutex<HashMap<String, Lease>>,
    public: Mutex<Option<u16>>,
    stop: CancellationToken,
}
#[derive(Clone)]
struct Auth { owner: Arc<Owner>, host: String, origin: String, private: bool }
#[derive(Deserialize, Serialize)]
struct LeaseInput { id: String, #[serde(default)] release: bool }
#[derive(Deserialize)]
struct PublicInput { port: u16 }
fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error":{"code":code,"message":message}}))).into_response()
}
async fn authenticate(State(auth): State<Auth>, mut request: Request, next: Next) -> Response {
    if request.headers().get(header::HOST).and_then(|value| value.to_str().ok()) != Some(auth.host.as_str()) { return failure(StatusCode::FORBIDDEN, "HOST_DENIED", "Unexpected exact Host"); }
    if let Some(origin) = request.headers().get(header::ORIGIN) && origin.to_str().ok() != Some(auth.origin.as_str()) { return failure(StatusCode::FORBIDDEN, "ORIGIN_DENIED", "Unexpected or null Origin"); }
    let supplied = request.headers().get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).unwrap_or("").to_owned();
    let context = if auth.private && bool::from(supplied.as_bytes().ct_eq(auth.owner.record.bearer.as_bytes())) { auth.owner.service.owner() }
    else {
        let service = auth.owner.service.clone();
        match tokio::task::spawn_blocking(move || service.authenticate(&supplied)).await {
            Ok(Ok(context)) => context,
            _ => return failure(StatusCode::UNAUTHORIZED, "AUTHENTICATION_REQUIRED", "A current scoped bearer is required"),
        }
    };
    if request.uri().path().starts_with("/internal/") && context.profile.is_some() { return failure(StatusCode::FORBIDDEN, "PERMISSION_DENIED", "Internal controls require native owner authority"); }
    if request.headers().contains_key("Mcp-Session-Id") { return failure(StatusCode::BAD_REQUEST, "INVALID_SESSION", "This endpoint uses official stateless MCP profiles"); }
    if request.method() == axum::http::Method::POST {
        let (parts, body) = request.into_parts();
        let bytes = match axum::body::to_bytes(body, 256 * 1024).await { Ok(bytes) => bytes, Err(_) => return failure(StatusCode::PAYLOAD_TOO_LARGE, "RESOURCE_LIMIT", "Request exceeds 256 KiB") };
        if commands::decode_json(&bytes).is_err() { return failure(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "Invalid or ambiguous JSON request"); }
        request = Request::from_parts(parts, axum::body::Body::from(bytes));
    }
    request.extensions_mut().insert(context);
    match tokio::time::timeout(Duration::from_secs(130), next.run(request)).await { Ok(response) => response, Err(_) => failure(StatusCode::REQUEST_TIMEOUT, "REQUEST_TIMEOUT", "Bounded request deadline exceeded") }
}
async fn dispatch(State(owner): State<Arc<Owner>>, axum::Extension(context): axum::Extension<Context>, Json(request): Json<commands::Request>) -> Response {
    let response = owner.service.dispatch(context, request).await;
    match serde_json::to_vec(&response) {
        Ok(bytes) if bytes.len() <= 256 * 1024 => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        _ => failure(StatusCode::PAYLOAD_TOO_LARGE, "RESOURCE_LIMIT", "Response exceeds its admitted byte budget"),
    }
}
async fn lease(State(owner): State<Arc<Owner>>, axum::Extension(context): axum::Extension<Context>, Json(input): Json<LeaseInput>) -> Response {
    if uuid::Uuid::parse_str(&input.id).is_err() { return failure(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "Client lease ID must be a UUID"); }
    let mut leases = owner.leases.lock().await;
    leases.retain(|_, lease| lease.expires > Instant::now());
    if let Some(lease) = leases.get(&input.id) && lease.principal != context.principal { return failure(StatusCode::FORBIDDEN, "PERMISSION_DENIED", "Another principal owns this lease"); }
    if input.release { leases.remove(&input.id); }
    else {
        if leases.len() >= 64 && !leases.contains_key(&input.id) { return failure(StatusCode::SERVICE_UNAVAILABLE, "RESOURCE_LIMIT", "Client lease budget reached"); }
        leases.insert(input.id, Lease { principal: context.principal, expires: Instant::now() + Duration::from_secs(90) });
    }
    Json(json!({"ok":true,"expires_in_seconds":90})).into_response()
}
async fn health(State(owner): State<Arc<Owner>>) -> Json<Value> {
    Json(json!({"product":"Numera Wiz","epoch":owner.record.epoch,"root_id":owner.record.root_id,"build":owner.record.build,"registry":owner.record.registry}))
}
async fn credential(State(owner): State<Arc<Owner>>, Json(input): Json<nw_domain::access::CredentialInput>) -> Response {
    let service = owner.service.clone();
    match tokio::task::spawn_blocking(move || service.credential(&input.profile)).await {
        Ok(Ok(bearer)) => Json(json!({"bearer":bearer,"expires_in_seconds":28800})).into_response(),
        Ok(Err(error)) => (StatusCode::FORBIDDEN, Json(json!({"error":error}))).into_response(),
        Err(_) => failure(StatusCode::INTERNAL_SERVER_ERROR, "WORKER_FAILED", "Credential setup failed"),
    }
}
fn router(owner: Arc<Owner>, port: u16, private: bool) -> Router {
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://{host}");
    let mut config = StreamableHttpServerConfig::default().with_allowed_hosts([host.clone()]).with_allowed_origins([origin.clone()]).enforce_origin_validation().with_legacy_session_mode(false);
    config.max_request_body_bytes = 256 * 1024;
    let service = owner.service.clone();
    let mcp = StreamableHttpService::new(move || Ok(crate::mcp::Bridge::http(service.clone())), LocalSessionManager::default().into(), config);
    let mut router = Router::new().route("/api/v1/commands", post(dispatch).get(|State(owner): State<Arc<Owner>>, axum::Extension(context): axum::Extension<Context>| async move { Json(owner.service.dispatch(context, commands::Request::query("commands.list", json!({}))).await) })).route("/api/v1/lease", post(lease)).route("/health", get(health)).nest_service("/mcp", mcp);
    if private { router = router.route("/internal/credential", post(credential)).route("/internal/lease", post(lease)).route("/internal/public", post(public)); }
    router.layer(DefaultBodyLimit::max(256 * 1024)).layer(middleware::from_fn_with_state(Auth { owner: owner.clone(), host, origin, private }, authenticate)).with_state(owner)
}
async fn public(State(owner): State<Arc<Owner>>, Json(input): Json<PublicInput>) -> Response {
    if input.port == 0 { return failure(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "Public port must be explicit and nonzero"); }
    let mut active = owner.public.lock().await;
    if let Some(port) = *active {
        return if port == input.port { Json(json!({"endpoint":format!("http://127.0.0.1:{port}")})).into_response() }
        else { failure(StatusCode::CONFLICT, "PUBLIC_LISTENER_CONFLICT", "Another explicit public port is active; close its sessions before changing it") };
    }
    let listener = match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, input.port)).await { Ok(listener) => listener, Err(_) => return failure(StatusCode::CONFLICT, "PORT_IN_USE", "The requested port is busy; it was not silently incremented") };
    *active = Some(input.port);
    let routes = router(owner.clone(), input.port, false);
    let stop = owner.stop.clone();
    tokio::spawn(async move { if let Err(error) = axum::serve(listener, routes).with_graceful_shutdown(stop.cancelled_owned()).await { tracing::error!(%error,"Public listener stopped"); } });
    Json(json!({"endpoint":format!("http://127.0.0.1:{}",input.port)})).into_response()
}
pub async fn run_owner(root: PathBuf, components: Option<PathBuf>) -> Result<()> {
    let binaries = components.map(|path| (path.join(if cfg!(windows) {"ffmpeg.exe"} else {"ffmpeg"}), path.join(if cfg!(windows) {"ffprobe.exe"} else {"ffprobe"})));
    let service = Service::open(root.clone(), binaries).await?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let exe = std::env::current_exe()?;
    let build = tokio::task::spawn_blocking(move || nw_platform::files::identity(&exe, &CancellationToken::new())).await??.sha256;
    let record = OwnerRecord { canonical_root: nw_platform::files::path_text(&root)?, root_id: service.store.root_id().into(), epoch: service.epoch.clone(), build, registry: registry_fingerprint(), port, bearer: format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()) };
    let runtime = nw_platform::security::private_directory(&root.join("runtime"))?;
    nw_platform::security::write_private(&runtime.join("owner.json"), &serde_json::to_vec(&record)?, true)?;
    let owner = Arc::new(Owner { service, record, leases: Mutex::new(HashMap::new()), public: Mutex::new(None), stop: CancellationToken::new() });
    let monitoring = owner.clone();
    tokio::spawn(async move {
        let mut idle = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut leases = monitoring.leases.lock().await;
            leases.retain(|_, lease| lease.expires > Instant::now());
            if monitoring.service.busy() || !leases.is_empty() { idle = Instant::now(); }
            if idle.elapsed() >= Duration::from_secs(15) { monitoring.stop.cancel(); break; }
        }
    });
    tracing::info!(port,epoch=%owner.record.epoch,"Protected owner ready; credential omitted");
    axum::serve(listener, router(owner.clone(), port, true)).with_graceful_shutdown(owner.stop.clone().cancelled_owned()).await?;
    // The writer lock remains held until every route/worker reference has drained.
    tracing::info!("Owner idle grace completed");
    Ok(())
}
pub async fn serve(client: crate::client::NativeClient, port: u16, token_file: PathBuf, profile: String) -> Result<()> {
    let lease = uuid::Uuid::new_v4().to_string();
    client.internal("internal/lease", json!({"id":lease})).await?;
    let endpoint = client.internal("internal/public", json!({"port":port})).await?;
    let issued = client.internal("internal/credential", json!({"profile":profile})).await?;
    let token = issued["bearer"].as_str().context("Scoped bearer missing")?;
    nw_platform::security::write_private(&token_file, token.as_bytes(), false)?;
    tracing::info!(endpoint=%endpoint["endpoint"],"Scoped HTTP API/MCP ready; credential omitted");
    loop {
        tokio::select! { _ = tokio::signal::ctrl_c() => break, _ = tokio::time::sleep(Duration::from_secs(15)) => { client.internal("internal/lease", json!({"id":lease})).await?; } }
    }
    let _ = client.internal("internal/lease", json!({"id":lease,"release":true})).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn actual_http_checks_host_origin_bearer_and_profile() {
        let data = tempfile::tempdir().unwrap();
        let service = Service::open(data.path().to_owned(), None).await.unwrap();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = service.credential("agent").unwrap();
        let record = OwnerRecord { canonical_root: "unit".into(), root_id: service.store.root_id().into(), epoch: service.epoch.clone(), build: "unit".into(), registry: registry_fingerprint(), port, bearer: "private-owner-token".into() };
        let owner = Arc::new(Owner { service, record, leases: Mutex::new(HashMap::new()), public: Mutex::new(None), stop: CancellationToken::new() });
        let routes = router(owner, port, false);
        let task = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap(); });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let endpoint = format!("http://127.0.0.1:{port}");
        assert_eq!(client.get(format!("{endpoint}/health")).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(client.get(format!("{endpoint}/health")).bearer_auth(&token).header("Origin", "null").send().await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(client.get(format!("{endpoint}/health")).bearer_auth(&token).header("Host", "other.invalid").send().await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(client.get(format!("{endpoint}/health")).bearer_auth("private-owner-token").send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        let fixture: commands::Request = serde_json::from_str(include_str!("../tests/fixtures/clean-resolve.json")).unwrap();
        let denied: commands::Response = client.post(format!("{endpoint}/api/v1/commands")).bearer_auth(&token).json(&fixture).send().await.unwrap().json().await.unwrap();
        assert_eq!(denied.error.unwrap().code, "PERMISSION_DENIED");
        assert_eq!(client.post(format!("{endpoint}/api/v1/commands")).bearer_auth(&token).header("Content-Type", "application/json").body(r#"{"params":{"index":0,"index":1}}"#).send().await.unwrap().status(), StatusCode::BAD_REQUEST);
        task.abort();
    }
}
