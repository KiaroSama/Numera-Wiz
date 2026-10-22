use anyhow::{Result,bail};
#[cfg(not(windows))] use anyhow::Context;
use axum::{Router,Json,extract::{State,DefaultBodyLimit,Request},http::{StatusCode,header},middleware::{self,Next},response::{IntoResponse,Response},routing::{get,post}};
use nw_domain::commands;
use nw_service::{CommandService,Principal};
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig,StreamableHttpService,session::local::LocalSessionManager};
use std::{path::PathBuf,sync::Arc,time::{Duration,Instant}};
#[cfg(not(windows))] use std::io::Write;
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener,sync::Semaphore};

#[derive(Clone)]
struct Auth{bearer:String,host:String,origin:String,principal:Principal,expires:Instant,slots:Arc<Semaphore>}
fn random_bearer()->String{format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple())}
async fn authenticate(State(auth):State<Auth>,mut request:Request,next:Next)->Response {
    if request.headers().get(header::HOST).and_then(|v|v.to_str().ok())!=Some(auth.host.as_str()){return (StatusCode::FORBIDDEN,"Unexpected Host").into_response();}
    if let Some(origin)=request.headers().get(header::ORIGIN)&&origin.to_str().ok()!=Some(auth.origin.as_str()){return (StatusCode::FORBIDDEN,"Unexpected Origin").into_response();}
    if Instant::now()>=auth.expires{return (StatusCode::UNAUTHORIZED,"Session expired").into_response();}
    let supplied=request.headers().get(header::AUTHORIZATION).and_then(|v|v.to_str().ok()).and_then(|s|s.strip_prefix("Bearer ")).unwrap_or("");
    if !bool::from(supplied.as_bytes().ct_eq(auth.bearer.as_bytes())){return (StatusCode::UNAUTHORIZED,"Bearer authentication required").into_response();}
    if request.headers().contains_key("Mcp-Session-Id"){return (StatusCode::BAD_REQUEST,"This development endpoint uses stateless MCP; session IDs are not authorization").into_response();}
    let _permit=match auth.slots.clone().try_acquire_owned(){Ok(value)=>value,Err(_)=>return (StatusCode::SERVICE_UNAVAILABLE,"Request capacity exceeded").into_response()};
    request.extensions_mut().insert(auth.principal);
    match tokio::time::timeout(Duration::from_secs(20),next.run(request)).await{Ok(response)=>response,Err(_)=>(StatusCode::REQUEST_TIMEOUT,"Request deadline exceeded").into_response()}
}
async fn dispatch(axum::Extension(principal):axum::Extension<Principal>,Json(request):Json<commands::Request>)->Response {
    let result=CommandService.dispatch(principal,request);
    match serde_json::to_vec(&result){Ok(bytes) if bytes.len()<=256*1024=>([(header::CONTENT_TYPE,"application/json")],bytes).into_response(),Ok(_)=>(StatusCode::PAYLOAD_TOO_LARGE,"Result exceeds the admitted response budget").into_response(),Err(_)=>(StatusCode::INTERNAL_SERVER_ERROR,"Serialization failed").into_response()}
}
fn router(auth:Auth)->Router {
    let mut config=StreamableHttpServerConfig::default().with_allowed_hosts([auth.host.clone()]).with_allowed_origins([auth.origin.clone()]).enforce_origin_validation().with_legacy_session_mode(false);
    config.max_request_body_bytes=256*1024;
    let mcp=StreamableHttpService::new(||Ok(crate::mcp::Bridge::http()),LocalSessionManager::default().into(),config);
    Router::new().route("/api/v1/commands",post(dispatch).get(|axum::Extension(principal):axum::Extension<Principal>|async move{Json(CommandService.dispatch(principal,commands::Request::query("commands.list",serde_json::json!({}))))}))
        .route("/health",get(||async{Json(serde_json::json!({"product":"Numera Wiz","scope":"development_only"}))}))
        .nest_service("/mcp",mcp)
        .layer(DefaultBodyLimit::max(256*1024))
        .layer(middleware::from_fn_with_state(auth,authenticate))
}
fn auth(port:u16,principal:Principal)->Auth{Auth{bearer:random_bearer(),host:format!("127.0.0.1:{port}"),origin:format!("http://127.0.0.1:{port}"),principal,expires:Instant::now()+Duration::from_secs(8*3600),slots:Arc::new(Semaphore::new(16))}}
#[cfg(windows)]
pub async fn serve(_port:u16,_token_file:PathBuf)->Result<()> {bail!("WINDOWS_ACL_QUALIFICATION_PENDING: direct HTTP token-file creation requires the user-restricted ACL adapter; use stdio discovery in this development build")}
#[cfg(not(windows))]
pub async fn serve(port:u16,token_file:PathBuf)->Result<()> {
    if port==0{bail!("Public port must be explicit and nonzero; default is 1578");}
    let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,port)).await.with_context(||format!("Cannot bind 127.0.0.1:{port}; the port is not silently changed"))?;
    let state=auth(port,Principal::DiscoveryAgent);
    let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    #[cfg(not(windows))]
    {
        let mut file=options.open(&token_file).context("Token file must be new; existing files are never overwritten")?;
        file.write_all(state.bearer.as_bytes())?;file.sync_all()?;
        tracing::info!(port,token_file=%token_file.display(),"Loopback discovery API/MCP ready; bearer omitted from logs");
        tracing::warn!("WARNING: Browser pairing and operating access profiles are not yet qualified; this endpoint is discovery-only");
        axum::serve(listener,router(state)).with_graceful_shutdown(async{let _=tokio::signal::ctrl_c().await;}).await?;
        Ok(())
    }
}
#[cfg(feature="desktop")]
pub async fn native_session()->Result<(String,String,tokio::task::JoinHandle<()>)>{
    let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await?;let port=listener.local_addr()?.port();let state=auth(port,Principal::LocalUser);let bearer=state.bearer.clone();
    let task=tokio::spawn(async move{if let Err(error)=axum::serve(listener,router(state)).await{tracing::error!(%error,"Native development query service stopped");}});
    Ok((format!("http://127.0.0.1:{port}"),bearer,task))
}
