//! One-use, origin-bound pairing is the only unauthenticated service bootstrap.
use super::*;
#[derive(Clone)]
pub(super) struct OriginGuard {
    pub host: String,
    pub origin: String,
}
pub(super) struct Pairing {
    profile: String,
    origin: String,
    expires: Instant,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    proof: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PairingInput {
    profile: String,
}
pub(super) async fn secure_headers(
    State(origin): State<OriginGuard>,
    request: Request,
    next: Next,
) -> Response {
    if request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        != Some(origin.host.as_str())
    {
        return failure(
            StatusCode::FORBIDDEN,
            "HOST_DENIED",
            "Unexpected exact Host",
        );
    }
    if let Some(value) = request.headers().get(header::ORIGIN)
        && value.to_str().ok() != Some(origin.origin.as_str())
    {
        return failure(
            StatusCode::FORBIDDEN,
            "ORIGIN_DENIED",
            "Unexpected or null Origin",
        );
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "X-Content-Type-Options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert("Content-Security-Policy",axum::http::HeaderValue::from_static("default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; script-src-attr 'none'; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' blob:; media-src 'self' blob:; connect-src 'self'; font-src 'self'; worker-src 'none'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"));
    if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
    }
    response
}
pub(super) async fn create_pairing(
    State(owner): State<Arc<Owner>>,
    Json(input): Json<PairingInput>,
) -> Response {
    if crate::web_assets::ASSETS.is_empty() {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "WEB_ASSETS_MISSING",
            "Use a same-build application with embedded browser assets",
        );
    }
    let Some(port) = *owner.public.lock().await else {
        return failure(
            StatusCode::CONFLICT,
            "PUBLIC_LISTENER_REQUIRED",
            "Enable the explicit public listener first",
        );
    };
    if owner.service.profile_context(&input.profile).is_err() {
        return failure(
            StatusCode::FORBIDDEN,
            "PERMISSION_DENIED",
            "Selected pairing profile is missing or revoked",
        );
    }
    let mut proofs = owner.pairing.lock().await;
    proofs.retain(|_, proof| proof.expires > Instant::now());
    if proofs.len() >= 32 {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "RESOURCE_LIMIT",
            "Pairing proof quota reached",
        );
    }
    let proof = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let origin = format!("http://127.0.0.1:{port}");
    proofs.insert(
        proof.clone(),
        Pairing {
            profile: input.profile,
            origin: origin.clone(),
            expires: Instant::now() + Duration::from_secs(60),
        },
    );
    Json(json!({"url":format!("{origin}/#pair={proof}"),"expires_in_seconds":60})).into_response()
}
async fn exchange(
    State(owner): State<Arc<Owner>>,
    headers: axum::http::HeaderMap,
    Json(input): Json<Exchange>,
) -> Response {
    if input.proof.len() != 64 || !input.proof.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return failure(
            StatusCode::UNAUTHORIZED,
            "PAIRING_INVALID",
            "Pairing proof is invalid",
        );
    }
    let Some(proof) = owner.pairing.lock().await.remove(&input.proof) else {
        return failure(
            StatusCode::UNAUTHORIZED,
            "PAIRING_INVALID",
            "Proof was consumed or expired",
        );
    };
    if proof.expires <= Instant::now()
        || headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            != Some(proof.origin.as_str())
    {
        return failure(
            StatusCode::UNAUTHORIZED,
            "PAIRING_INVALID",
            "Proof expired or origin does not match",
        );
    }
    let service = owner.service.clone();
    let root = service.store.root_id().to_owned();
    let epoch = service.epoch.clone();
    match tokio::task::spawn_blocking(move||{let context=service.profile_context(&proof.profile)?;let bearer=service.credential_for_origin(&proof.profile,Some(proof.origin))?;Ok::<_,nw_domain::DomainError>((context.principal,bearer))}).await {
        Ok(Ok((principal,bearer)))=>Json(json!({"bearer":bearer,"principal":principal,"root_id":root,"epoch":epoch,"expires_in_seconds":28800})).into_response(),
        _=>failure(StatusCode::UNAUTHORIZED,"PAIRING_INVALID","Selected profile is no longer available"),
    }
}
pub(super) fn routes(owner: Arc<Owner>) -> Router {
    Router::new()
        .route("/api/v1/pairing/exchange", post(exchange))
        .layer(DefaultBodyLimit::max(16384))
        .with_state(owner)
}

pub(super) async fn launch(url: &str) -> anyhow::Result<()> {
    use std::process::Stdio;
    #[cfg(target_os = "windows")]
    let mut command = {
        let root = std::env::var_os("SystemRoot")
            .ok_or_else(|| anyhow::anyhow!("Windows browser launcher unavailable"))?;
        let root = std::path::PathBuf::from(root).join("System32");
        let mut command = tokio::process::Command::new(root.join("rundll32.exe"));
        command.arg(format!(
            "{},FileProtocolHandler",
            root.join("url.dll").display()
        ));
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("/usr/bin/open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = tokio::process::Command::new("xdg-open");
    let mut child = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            anyhow::anyhow!("BROWSER_LAUNCH_FAILED: open the workspace from a desktop session")
        })?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    tracing::info!("Opened one-use browser pairing; proof omitted from diagnostics");
    Ok(())
}
