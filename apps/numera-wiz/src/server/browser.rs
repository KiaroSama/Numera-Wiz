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
    listener_stop: CancellationToken,
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
    let mut response = if request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        != Some(origin.host.as_str())
    {
        failure(
            StatusCode::FORBIDDEN,
            "HOST_DENIED",
            "Unexpected exact Host",
        )
    } else if request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|value| value.to_str().ok() != Some(origin.origin.as_str()))
    {
        failure(
            StatusCode::FORBIDDEN,
            "ORIGIN_DENIED",
            "Unexpected or null Origin",
        )
    } else {
        next.run(request).await
    };
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
    let Some((port, listener_stop)) = owner
        .public
        .lock()
        .await
        .as_ref()
        .map(|listener| (listener.port, listener.stop.clone()))
    else {
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
            listener_stop,
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
    if proof.listener_stop.is_cancelled()
        || proof.expires <= Instant::now()
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
async fn bootstrap_admit(
    State(owner): State<Arc<Owner>>,
    request: Request,
    next: Next,
) -> Response {
    let permit = match owner.slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "RESOURCE_LIMIT",
                "Bootstrap admission capacity reached",
            );
        }
    };
    {
        let _guard = owner.leases.lock().await;
        if owner.stop.is_cancelled() {
            return failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "OWNER_STOPPING",
                "Reconnect to the active owner",
            );
        }
        owner.active.fetch_add(1, Ordering::AcqRel);
        owner.public_active.fetch_add(1, Ordering::AcqRel);
    }
    let work = Work {
        active: owner.active.clone(),
        public: Some(owner.public_active.clone()),
        _permit: permit,
    };
    let expires = tokio::time::Instant::now() + Duration::from_secs(10);
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout_at(expires, axum::body::to_bytes(body, 16384)).await {
        Ok(Ok(bytes)) => bytes,
        _ => {
            return failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "RESOURCE_LIMIT",
                "Pairing body limit or deadline exceeded",
            );
        }
    };
    if commands::decode_json(&bytes).is_err() {
        return failure(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "Invalid or ambiguous pairing JSON",
        );
    }
    let response = match tokio::time::timeout_at(
        expires,
        next.run(Request::from_parts(parts, axum::body::Body::from(bytes))),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return failure(
                StatusCode::REQUEST_TIMEOUT,
                "REQUEST_TIMEOUT",
                "Pairing deadline exceeded",
            );
        }
    };
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        axum::body::Body::new(ActiveBody {
            deadline: Box::pin(tokio::time::sleep_until(expires)),
            finished: false,
            body: Box::pin(body),
            _work: work,
        }),
    )
}
pub(super) fn routes(owner: Arc<Owner>) -> Router {
    Router::new()
        .route("/api/v1/pairing/exchange", post(exchange))
        .layer(DefaultBodyLimit::max(16384))
        .layer(middleware::from_fn_with_state(
            owner.clone(),
            bootstrap_admit,
        ))
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

#[cfg(all(test, feature = "embedded-web"))]
#[allow(
    dead_code,
    reason = "The shared CI fixture also serves deeper media journeys"
)]
#[path = "../../../../test-support/media.rs"]
mod media_fixture;

#[cfg(all(test, feature = "embedded-web"))]
mod tests {
    use super::*;
    struct ObservedReport {
        request_id: String,
        status: u16,
        response: Value,
    }
    type ObservedReports = Arc<Mutex<std::collections::VecDeque<ObservedReport>>>;
    async fn observe_reports(
        State(reports): State<ObservedReports>,
        request: Request,
        next: Next,
    ) -> Response {
        if request.method() != axum::http::Method::POST
            || request.uri().path() != "/api/v1/commands"
        {
            return next.run(request).await;
        }
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, nw_domain::commands::MAX_COMMAND_BYTES)
            .await
            .expect("Observed command request exceeds the production bound");
        let report_id = nw_domain::commands::decode_json(&bytes)
            .ok()
            .and_then(|value| {
                if value["command"]["id"] == "streams.clean.report" {
                    value["request_id"].as_str().map(str::to_owned)
                } else {
                    None
                }
            });
        let request = Request::from_parts(parts, axum::body::Body::from(bytes));
        let response = next.run(request).await;
        let Some(request_id) = report_id else {
            return response;
        };
        // This test-only wire observer records the actual canonical response,
        // then forwards its original status, headers and exact body bytes.
        // It never invokes a business handler or stores request headers/tokens.
        let status = response.status().as_u16();
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, nw_domain::commands::MAX_RESULT_BYTES)
            .await
            .expect("Observed report response exceeds the production bound");
        let result = nw_domain::commands::decode_json(&bytes).expect("Observed report JSON");
        {
            let mut reports = reports.lock().await;
            if reports.len() == 8 {
                reports.pop_front();
            }
            reports.push_back(ObservedReport {
                request_id,
                status,
                response: result,
            });
        }
        Response::from_parts(parts, axum::body::Body::from(bytes))
    }
    struct Fixture {
        observed_reports: ObservedReports,
        owner: Arc<Owner>,
        endpoint: String,
        _root: tempfile::TempDir,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let components = std::env::var_os("NW_FIXTURE_COMPONENTS").map(|path| {
            let path = std::path::PathBuf::from(path);
            (path.join("ffmpeg"), path.join("ffprobe"))
        });
        let service = Service::open(root.path().to_owned(), components)
            .await
            .unwrap();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let owner = Arc::new(Owner {
            record: OwnerRecord {
                canonical_root: "test".into(),
                root_id: service.store.root_id().into(),
                epoch: service.epoch.clone(),
                build: "test".into(),
                registry: registry_fingerprint(),
                port,
                bearer: "native-test-bootstrap".into(),
            },
            service,
            leases: Mutex::new(HashMap::new()),
            public: Mutex::new(Some(PublicListener {
                port,
                stop: CancellationToken::new(),
                last_used: Instant::now(),
            })),
            public_active: Arc::new(AtomicUsize::new(0)),
            stop: CancellationToken::new(),
            active: Arc::new(AtomicUsize::new(0)),
            slots: Arc::new(Semaphore::new(32)),
            pairing: Mutex::new(HashMap::new()),
        });
        let observed_reports = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let routes = super::super::router(owner.clone(), port, false, owner.stop.clone()).layer(
            axum::middleware::from_fn_with_state(observed_reports.clone(), observe_reports),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, routes).await.unwrap();
        });
        Fixture {
            observed_reports,
            owner,
            endpoint: format!("http://127.0.0.1:{port}"),
            _root: root,
            task,
        }
    }
    async fn pairing(fixture: &Fixture) -> String {
        let response = create_pairing(
            State(fixture.owner.clone()),
            Json(PairingInput {
                profile: "browser".into(),
            }),
        )
        .await;
        assert!(response.status().is_success());
        let bytes = axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        value["url"].as_str().unwrap().to_owned()
    }
    #[tokio::test]
    async fn embedded_namespace_pairing_and_leases_enforce_the_actual_http_boundary() {
        let fixture = fixture().await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for asset in crate::web_assets::ASSETS {
            let path = if asset.path == "index.html" {
                "/".into()
            } else {
                format!("/{}", asset.path)
            };
            let response = client
                .get(format!("{}{path}", fixture.endpoint))
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
            assert_eq!(response.headers()["X-Content-Type-Options"], "nosniff");
            assert_eq!(response.headers()[header::CONTENT_TYPE], asset.mime);
            assert!(
                response.headers()["Content-Security-Policy"]
                    .to_str()
                    .unwrap()
                    .contains("script-src 'self' 'wasm-unsafe-eval'")
            );
            assert_eq!(response.bytes().await.unwrap().as_ref(), asset.bytes);
        }
        assert_eq!(
            client
                .get(format!("{}/assets/upload.html", fixture.endpoint))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let url = pairing(&fixture).await;
        let proof = url.split("#pair=").nth(1).unwrap();
        let route = format!("{}/api/v1/pairing/exchange", fixture.endpoint);
        assert_eq!(
            client
                .post(&route)
                .header(header::ORIGIN, "null")
                .json(&json!({"proof":proof}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response = client
            .post(&route)
            .header(header::ORIGIN, &fixture.endpoint)
            .json(&json!({"proof":proof}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let credential: Value = response.json().await.unwrap();
        let bearer = credential["bearer"].as_str().unwrap();
        assert_eq!(
            client
                .post(&route)
                .header(header::ORIGIN, &fixture.endpoint)
                .json(&json!({"proof":proof}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let health: Value = client
            .post(format!("{}/api/v1/session", fixture.endpoint))
            .bearer_auth(bearer)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["ui_source"], crate::web_assets::UI_SOURCE);
        assert_eq!(health["registry"], registry_fingerprint());
        let id = uuid::Uuid::new_v4().to_string();
        let lease = format!("{}/api/v1/lease", fixture.endpoint);
        assert!(
            client
                .post(&lease)
                .bearer_auth(bearer)
                .json(&json!({"id":id}))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
        assert_eq!(fixture.owner.leases.lock().await.len(), 1);
        assert!(
            client
                .post(&lease)
                .bearer_auth(bearer)
                .json(&json!({"id":id,"release":true}))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
        assert!(fixture.owner.leases.lock().await.is_empty());
        let agent = fixture
            .owner
            .service
            .credential_for_origin("agent", Some(fixture.endpoint.clone()))
            .unwrap();
        assert_eq!(
            client
                .post(&lease)
                .bearer_auth(agent)
                .json(&json!({"id":id}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn old_pairing_proof_cannot_reactivate_when_the_same_origin_reopens() {
        let fixture = fixture().await;
        let old = pairing(&fixture).await;
        let proof = old.split("#pair=").nth(1).unwrap().to_owned();
        {
            let mut listener = fixture.owner.public.lock().await;
            let listener = listener.as_mut().unwrap();
            listener.stop.cancel();
            listener.stop = CancellationToken::new();
        }
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{}/api/v1/pairing/exchange", fixture.endpoint))
            .header(header::ORIGIN, &fixture.endpoint)
            .json(&json!({"proof":proof}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    async fn browser_xdo(args: &[&str]) -> std::process::Output {
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("/usr/bin/xdotool")
                .args(args)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("Browser OS input deadline")
        .expect("Browser OS input command")
    }
    async fn browser_input(args: &[&str]) {
        assert!(
            browser_xdo(args).await.status.success(),
            "Browser OS input failed"
        );
    }
    async fn browser_desktop_diagnostic(path: &std::path::Path) {
        use base64::Engine as _;
        use std::io::Read as _;
        let status = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("/usr/bin/import")
                .args(["-window", "root"])
                .arg(path)
                .kill_on_drop(true)
                .status(),
        )
        .await
        .expect("Browser desktop capture deadline")
        .expect("Browser desktop capture");
        assert!(status.success());
        let mut png = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .take(256 * 1024 + 1)
            .read_to_end(&mut png)
            .unwrap();
        if png.len() <= 256 * 1024 {
            println!(
                "NW_BROWSER_DESKTOP_FAILURE_IMAGE:{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            );
        }
        let windows = browser_xdo(&["search", "--onlyvisible", "--name", "."]).await;
        let mut titles = Vec::new();
        for id in String::from_utf8_lossy(&windows.stdout).lines().take(32) {
            if !id.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let title = browser_xdo(&["getwindowname", id]).await;
            if title.status.success() {
                let title: String = String::from_utf8_lossy(&title.stdout)
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect();
                titles.push(title);
            }
        }
        // This isolated display contains only the test browser and OS dialog.
        println!("NW_BROWSER_VISIBLE_TITLES:{}", json!(titles));
    }
    async fn driver(
        client: &reqwest::Client,
        base: &str,
        method: reqwest::Method,
        route: &str,
        body: Value,
    ) -> Value {
        let request = client.request(method, format!("{base}{route}"));
        let request = if body.is_null() {
            request
        } else {
            request.json(&body)
        };
        let response = request.send().await.expect("WebDriver transport failed");
        let status = response.status();
        let result: Value = response.json().await.expect("Invalid WebDriver result");
        assert!(
            status.is_success(),
            "WebDriver request failed with {}",
            result["value"]["error"]
        );
        result["value"].clone()
    }
    async fn browser_click(client: &reqwest::Client, base: &str, route: &str, x: u32, y: u32) {
        driver(client,base,reqwest::Method::POST,&format!("{route}/actions"),
            json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[
                {"type":"pointerMove","duration":150,"origin":"viewport","x":x,"y":y},
                {"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
    }
    #[tokio::test]
    #[ignore = "Actual Chrome runtime test runs only in the hosted browser job"]
    async fn packaged_browser_boots_shared_wasm_under_the_enforced_policy() {
        use base64::Engine as _;
        assert!(
            std::env::var_os("NW_FIXTURE_COMPONENTS").is_some(),
            "The actual UI media journey requires the matching CI-built fixture components"
        );
        use std::process::Stdio;
        let driver_path =
            std::env::var_os("NW_CHROMEDRIVER").expect("CI supplies its identified ChromeDriver");
        let selected = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = selected.local_addr().unwrap().port();
        drop(selected);
        let mut process = tokio::process::Command::new(driver_path)
            .arg(format!("--port={port}"))
            .arg("--allowed-ips=127.0.0.1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let start = Instant::now();
        loop {
            if client.get(format!("{base}/status")).send().await.is_ok() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "ChromeDriver did not start"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let headful = std::env::var_os("NW_BROWSER_SAVE").is_some();
        let mut window_manager = if headful {
            assert!(
                std::env::var_os("DISPLAY").is_some(),
                "Real browser file picking requires the hosted X display"
            );
            Some(
                tokio::process::Command::new("/usr/bin/openbox")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .unwrap(),
            )
        } else {
            None
        };
        let mut chrome_args = vec![
            "--no-sandbox",
            "--enable-unsafe-swiftshader",
            "--use-angle=swiftshader",
            "--window-size=1440,1000",
            "--force-device-scale-factor=1",
        ];
        if !headful {
            chrome_args.push("--headless=new");
        }
        let session=driver(&client,&base,reqwest::Method::POST,"/session",json!({"capabilities":{"alwaysMatch":{"browserName":"chrome","goog:loggingPrefs":{"performance":"ALL"},"goog:chromeOptions":{"args":chrome_args}}}})).await;
        let id = session["sessionId"].as_str().expect("WebDriver session id");
        let route = format!("/session/{id}");
        if headful {
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/goog/cdp/execute"),
                json!({"cmd":"Emulation.setDeviceMetricsOverride","params":{"width":1440,"height":861,"deviceScaleFactor":1,"mobile":false}})).await;
        }
        let fixture = fixture().await;
        driver(
            &client,
            &base,
            reqwest::Method::POST,
            &format!("{route}/url"),
            json!({"url":pairing(&fixture).await}),
        )
        .await;
        let element = driver(
            &client,
            &base,
            reqwest::Method::POST,
            &format!("{route}/element"),
            json!({"using":"css selector","value":"#boot-status"}),
        )
        .await;
        let element = element["element-6066-11e4-a52e-4f735466cecf"]
            .as_str()
            .unwrap();
        let start = Instant::now();
        loop {
            let hidden = driver(
                &client,
                &base,
                reqwest::Method::GET,
                &format!("{route}/element/{element}/property/hidden"),
                Value::Null,
            )
            .await;
            if hidden == true {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "Shared WASM bootstrap did not finish under CSP/MIME enforcement"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let location = driver(
            &client,
            &base,
            reqwest::Method::GET,
            &format!("{route}/url"),
            Value::Null,
        )
        .await;
        assert!(
            !location.as_str().unwrap().contains("#pair="),
            "One-use proof must leave the address before WASM loads"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/browser-evidence");
        std::fs::create_dir_all(&directory).unwrap();
        let screenshot = driver(
            &client,
            &base,
            reqwest::Method::GET,
            &format!("{route}/screenshot"),
            Value::Null,
        )
        .await;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(screenshot.as_str().unwrap())
            .unwrap();
        std::fs::write(directory.join("shared-workspace.png"), bytes).unwrap();
        {
            let media = media_fixture::generate().await;
            driver(
                &client,
                &base,
                reqwest::Method::POST,
                &format!("{route}/goog/cdp/execute"),
                json!({"cmd":"Page.setInterceptFileChooserDialog","params":{"enabled":true}}),
            )
            .await;
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":0,"origin":"viewport","x":320,"y":94},{"type":"pause","duration":150},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            let after_click = driver(
                &client,
                &base,
                reqwest::Method::GET,
                &format!("{route}/screenshot"),
                Value::Null,
            )
            .await;
            std::fs::write(
                directory.join("file-chooser-workspace.png"),
                base64::engine::general_purpose::STANDARD
                    .decode(after_click.as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            let start = Instant::now();
            let input = loop {
                let response = client
                    .post(format!("{base}{route}/element"))
                    .json(&json!({"using":"css selector","value":"#media-file-input"}))
                    .send()
                    .await
                    .unwrap();
                if response.status().is_success() {
                    let result: Value = response.json().await.unwrap();
                    break result["value"]["element-6066-11e4-a52e-4f735466cecf"]
                        .as_str()
                        .unwrap()
                        .to_owned();
                }
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "Shared choose-media control did not create its file input"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            driver(
                &client,
                &base,
                reqwest::Method::POST,
                &format!("{route}/element/{input}/value"),
                json!({"text":media.source.to_str().unwrap()}),
            )
            .await;
            let start = Instant::now();
            let asset = loop {
                let assets = fixture
                    .owner
                    .service
                    .store
                    .list::<nw_domain::records::Asset>("asset/", 128)
                    .unwrap();
                if let Some(asset) = assets.into_iter().next() {
                    break asset;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "Actual UI file selection did not complete upload and canonical registration"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            };
            assert_eq!(
                asset.creator,
                fixture
                    .owner
                    .service
                    .profile_context("browser")
                    .unwrap()
                    .principal
            );
            assert_eq!(
                std::fs::read(&asset.identity.path).unwrap(),
                std::fs::read(&media.source).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
            let screenshot = driver(
                &client,
                &base,
                reqwest::Method::GET,
                &format!("{route}/screenshot"),
                Value::Null,
            )
            .await;
            std::fs::write(
                directory.join("loaded-workspace.png"),
                base64::engine::general_purpose::STANDARD
                    .decode(screenshot.as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            driver(&client, &base, reqwest::Method::POST, &format!("{route}/goog/cdp/execute"), json!({
                "cmd":"Network.enable",
                "params":{"maxTotalBufferSize":1048576,"maxResourceBufferSize":262144,"maxPostDataSize":16384}
            })).await;
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":150,"origin":"viewport","x":331,"y":591},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
            let started = Instant::now();
            let mut report_request = None::<String>;
            let mut report_correlation = None::<String>;
            let mut report_response_polls = 0usize;
            let mut report_cancelled_event = false;
            let mut completed_requests = std::collections::VecDeque::new();
            let mut last_report_input = Instant::now();
            let mut report_input_attempts = 1;
            let report_response = loop {
                // Inspect actual Chrome network events in memory. Never retain
                // raw headers, cookies, pairing proof or unrelated request bodies.
                let events = driver(
                    &client,
                    &base,
                    reqwest::Method::POST,
                    &format!("{route}/se/log"),
                    json!({"type":"performance"}),
                )
                .await;
                for entry in events.as_array().unwrap() {
                    let Ok(event) =
                        serde_json::from_str::<Value>(entry["message"].as_str().unwrap())
                    else {
                        continue;
                    };
                    let message = &event["message"];
                    let params = &message["params"];
                    if message["method"] == "Network.requestWillBeSent"
                        && let Some(post) = params["request"]["postData"].as_str()
                        && let Ok(request) = nw_domain::commands::decode_json(post.as_bytes())
                        && request["command"]["id"] == "streams.clean.report"
                    {
                        assert_ne!(request["request_id"], "local");
                        report_correlation =
                            Some(request["request_id"].as_str().unwrap().to_owned());
                        report_request = Some(params["requestId"].as_str().unwrap().to_owned());
                    }
                    if message["method"] == "Network.loadingFailed"
                        && params["requestId"].as_str() == report_request.as_deref()
                        && params["canceled"] == true
                    {
                        report_cancelled_event = true;
                    }
                    if message["method"] == "Network.loadingFinished"
                        && let Some(id) = params["requestId"].as_str()
                    {
                        completed_requests.push_back(id.to_owned());
                        if completed_requests.len() > 64 {
                            completed_requests.pop_front();
                        }
                    }
                }
                if let Some(correlation) = &report_correlation {
                    report_response_polls += 1;
                    let mut responses = fixture.observed_reports.lock().await;
                    if let Some(index) = responses
                        .iter()
                        .position(|value| &value.request_id == correlation)
                    {
                        let observed = responses.remove(index).unwrap();
                        assert_eq!(observed.status, 200);
                        assert_eq!(
                            observed.response["request_id"].as_str(),
                            Some(correlation.as_str())
                        );
                        break observed.response;
                    }
                }
                if started.elapsed() >= Duration::from_secs(15) {
                    let png = driver(
                        &client,
                        &base,
                        reqwest::Method::GET,
                        &format!("{route}/screenshot"),
                        Value::Null,
                    )
                    .await;
                    let png = png.as_str().unwrap();
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(png)
                        .unwrap();
                    std::fs::write(directory.join("selection-report-input-timeout.png"), &bytes)
                        .unwrap();
                    if bytes.len() <= 256 * 1024 {
                        println!("NW_BROWSER_FAILURE_IMAGE:{png}");
                    }
                    panic!(
                        "Actual browser Selection report response deadline; request_seen={}, pointer_attempts={}, retained_finished_ids={}, response_polls={}, cancelled_event={}",
                        report_request.is_some(),
                        report_input_attempts,
                        completed_requests.len(),
                        report_response_polls,
                        report_cancelled_event
                    );
                }
                // Registration is durable before the form's final async details/
                // output-suggestion reply. Retry only this nonmutating pointer
                // action while no report request is observed; UI guards remain.
                if report_request.is_none() && last_report_input.elapsed() >= Duration::from_secs(1)
                {
                    driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":150,"origin":"viewport","x":331,"y":591},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
                    last_report_input = Instant::now();
                    report_input_attempts += 1;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            assert_eq!(report_response["ok"], true, "{report_response}");
            assert_eq!(report_response["result"]["resolved_files"], 1);
            assert_eq!(
                report_response["result"]["files"][0]["selected_original_indices"],
                json!([0, 1, 2])
            );
            std::fs::write(directory.join("selection-report.json"), serde_json::to_vec_pretty(&json!({
                "actual_ui_response":report_response,
                "scope":"Actual Chrome Selection report mouse input, observed request correlation and bounded canonical HTTP response forwarded unchanged by test-only middleware; no direct handler or mocked UI response"
            })).unwrap()).unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            let report_png = driver(
                &client,
                &base,
                reqwest::Method::GET,
                &format!("{route}/screenshot"),
                Value::Null,
            )
            .await;
            let report_png = report_png.as_str().unwrap();
            let report_bytes = base64::engine::general_purpose::STANDARD
                .decode(report_png)
                .unwrap();
            std::fs::write(directory.join("selection-report.png"), &report_bytes).unwrap();
            if report_bytes.len() <= 256 * 1024 {
                println!("NW_BROWSER_SELECTION_IMAGE:{report_png}");
            }
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"key","id":"keyboard","actions":[{"type":"keyDown","value":"\u{E00C}"},{"type":"keyUp","value":"\u{E00C}"}]}]})).await;
            tokio::time::sleep(Duration::from_millis(150)).await;
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":150,"origin":"viewport","x":331,"y":570},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
            let start = Instant::now();
            loop {
                if !fixture
                    .owner
                    .service
                    .store
                    .list::<nw_domain::records::Plan>("plan/", 128)
                    .unwrap()
                    .is_empty()
                {
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(15),
                    "Shared review control did not create its frozen plan"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            let screenshot = driver(
                &client,
                &base,
                reqwest::Method::GET,
                &format!("{route}/screenshot"),
                Value::Null,
            )
            .await;
            std::fs::write(
                directory.join("reviewed-workspace.png"),
                base64::engine::general_purpose::STANDARD
                    .decode(screenshot.as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"key","id":"keyboard","actions":[{"type":"keyDown","value":"\u{E009}"},{"type":"keyDown","value":"\u{E007}"},{"type":"pause","duration":150},{"type":"keyUp","value":"\u{E007}"},{"type":"keyUp","value":"\u{E009}"}]}]})).await;
            let start = Instant::now();
            let job = loop {
                let jobs = fixture
                    .owner
                    .service
                    .store
                    .list::<nw_domain::records::Job>("job/", 128)
                    .unwrap();
                if let Some(job) = jobs.into_iter().next()
                    && job.state.terminal()
                {
                    break job;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "Shared Run control did not produce an observed terminal job"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            assert_eq!(job.state, nw_domain::records::JobState::Succeeded);
            let artifact = job
                .artifact
                .as_ref()
                .expect("A verified published artifact is required");
            assert_eq!(
                std::fs::read(&artifact.path).unwrap(),
                std::fs::read(&media.source).unwrap()
            );
            std::fs::write(directory.join("media-output.json"),serde_json::to_vec_pretty(&json!({"job_id":job.id,"plan_hash":job.plan.hash,"state":job.state,"artifact":artifact,"verification":job.verification,"oracle":"Full independent source/output byte equality after actual shared UI file selection, review and Run"})).unwrap()).unwrap();
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let screenshot = driver(
                &client,
                &base,
                reqwest::Method::GET,
                &format!("{route}/screenshot"),
                Value::Null,
            )
            .await;
            std::fs::write(
                directory.join("completed-workspace.png"),
                base64::engine::general_purpose::STANDARD
                    .decode(screenshot.as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            if headful {
                // Release file-input interception: the save path must use the genuine OS picker.
                driver(
                    &client,
                    &base,
                    reqwest::Method::POST,
                    &format!("{route}/goog/cdp/execute"),
                    json!({"cmd":"Page.setInterceptFileChooserDialog","params":{"enabled":false}}),
                )
                .await;
                driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":150,"origin":"viewport","x":533,"y":850},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
                tokio::time::sleep(Duration::from_millis(700)).await;
                let manifest_png = driver(
                    &client,
                    &base,
                    reqwest::Method::GET,
                    &format!("{route}/screenshot"),
                    Value::Null,
                )
                .await;
                let manifest_png = manifest_png.as_str().unwrap();
                let manifest_bytes = base64::engine::general_purpose::STANDARD
                    .decode(manifest_png)
                    .unwrap();
                std::fs::write(
                    directory.join("browser-output-details.png"),
                    &manifest_bytes,
                )
                .unwrap();
                if manifest_bytes.len() <= 256 * 1024 {
                    println!("NW_BROWSER_MANIFEST_IMAGE:{manifest_png}");
                }
                // The browser omits the native destination field; Save follows the four metadata rows.
                driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),
                    json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":150,"origin":"viewport","x":100,"y":381},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
                let started = Instant::now();
                let dialog = loop {
                    let found = browser_xdo(&[
                        "search",
                        "--onlyvisible",
                        "--name",
                        "^([Ss]ave|[Ff]ile|[Cc]hoose|Warning: this site can see edits you make)",
                    ])
                    .await;
                    if found.status.success() {
                        let found = String::from_utf8(found.stdout).unwrap();
                        if let Some(window) = found.lines().last() {
                            break window.to_owned();
                        }
                    }
                    if started.elapsed() >= Duration::from_secs(12) {
                        let png = driver(
                            &client,
                            &base,
                            reqwest::Method::GET,
                            &format!("{route}/screenshot"),
                            Value::Null,
                        )
                        .await;
                        let png = png.as_str().unwrap();
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(png)
                            .unwrap();
                        std::fs::write(directory.join("browser-save-picker-timeout.png"), &bytes)
                            .unwrap();
                        if bytes.len() <= 256 * 1024 {
                            println!("NW_BROWSER_SAVE_FAILURE_IMAGE:{png}");
                        }
                        browser_desktop_diagnostic(
                            &directory.join("browser-save-picker-desktop-timeout.png"),
                        )
                        .await;
                        panic!("Actual browser Save control did not open its real OS file picker");
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                };
                let saved_path = directory.join("browser-client-saved-copy.mkv");
                assert!(
                    !saved_path.exists(),
                    "Browser Save As qualification requires a fresh destination"
                );
                let status = tokio::process::Command::new("/usr/bin/import")
                    .args(["-window", &dialog])
                    .arg(directory.join("browser-save-picker.png"))
                    .status()
                    .await
                    .unwrap();
                assert!(status.success());
                browser_input(&["windowactivate", "--sync", &dialog]).await;
                browser_input(&["key", "--clearmodifiers", "ctrl+l"]).await;
                browser_input(&["key", "--clearmodifiers", "ctrl+a"]).await;
                browser_input(&[
                    "type",
                    "--clearmodifiers",
                    "--delay",
                    "1",
                    saved_path.to_str().unwrap(),
                ])
                .await;
                browser_input(&["key", "--clearmodifiers", "Return"]).await;
                tokio::time::sleep(Duration::from_millis(250)).await;
                if browser_xdo(&[
                    "search",
                    "--onlyvisible",
                    "--name",
                    "^([Ss]ave|[Ff]ile|[Cc]hoose|Warning: this site can see edits you make)",
                ])
                .await
                .status
                .success()
                {
                    browser_input(&["key", "--clearmodifiers", "alt+s"]).await;
                }
                let started = Instant::now();
                loop {
                    if saved_path.exists()
                        && let Ok(saved) =
                            nw_platform::files::identity(&saved_path, &CancellationToken::new())
                        && saved.sha256 == artifact.sha256
                        && saved.bytes == artifact.bytes
                    {
                        break;
                    }
                    if started.elapsed() >= Duration::from_secs(20) {
                        let png = driver(
                            &client,
                            &base,
                            reqwest::Method::GET,
                            &format!("{route}/screenshot"),
                            Value::Null,
                        )
                        .await;
                        let png = png.as_str().unwrap();
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(png)
                            .unwrap();
                        std::fs::write(directory.join("browser-save-timeout.png"), &bytes).unwrap();
                        if bytes.len() <= 256 * 1024 {
                            println!("NW_BROWSER_SAVE_FAILURE_IMAGE:{png}");
                        }
                        browser_desktop_diagnostic(
                            &directory.join("browser-save-desktop-timeout.png"),
                        )
                        .await;
                        panic!(
                            "Actual browser picker/stream/close did not produce the selected verified bytes"
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                assert_eq!(
                    std::fs::read(&saved_path).unwrap(),
                    std::fs::read(&media.source).unwrap()
                );
                assert_eq!(
                    fixture
                        .owner
                        .service
                        .store
                        .list::<nw_domain::records::Job>("job/", 128)
                        .unwrap()
                        .len(),
                    1
                );
                tokio::time::sleep(Duration::from_millis(300)).await;
                let png = driver(
                    &client,
                    &base,
                    reqwest::Method::GET,
                    &format!("{route}/screenshot"),
                    Value::Null,
                )
                .await;
                let png = png.as_str().unwrap();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(png)
                    .unwrap();
                std::fs::write(directory.join("browser-client-saved.png"), &bytes).unwrap();
                if bytes.len() <= 256 * 1024 {
                    println!("NW_BROWSER_SAVE_IMAGE:{png}");
                }
                std::fs::write(directory.join("browser-client-save.json"), serde_json::to_vec_pretty(&json!({
                    "artifact_id":format!("job-{}",job.id),"sha256":artifact.sha256,"bytes":artifact.bytes,
                    "oracle":"Actual Output details and Save pointer input, genuine Chrome OS Save picker and independent saved/source byte equality",
                    "producer_job_count":1,
                    "not_qualified":["other browsers/platforms","native OS Save As picker","browser existing-file overwrite/crash guarantees"]
                })).unwrap()).unwrap();
            }

            // The single-producer Save assertions above remain independent.
            // Now drive a two-source batch through the same real WASM controls.
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),
                json!({"actions":[{"type":"key","id":"keyboard","actions":[{"type":"keyDown","value":"\u{E00C}"},{"type":"keyUp","value":"\u{E00C}"}]}]})).await;
            let second_source = media.directory.path().join("browser-second.mkv");
            let ffmpeg_args = ["-hide_banner","-nostdin","-v","error","-i",media.source.to_str().unwrap(),
                "-map","0:0","-map","0:1","-c","copy","-f","matroska",second_source.to_str().unwrap()].map(str::to_owned);
            nw_platform::process::run(&media.bin.join("ffmpeg"),&ffmpeg_args,
                Duration::from_secs(30),4096,CancellationToken::new()).await.unwrap();
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/goog/cdp/execute"),
                json!({"cmd":"Page.setInterceptFileChooserDialog","params":{"enabled":true}})).await;
            browser_click(&client,&base,&route,320,94).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            let selected = driver(&client,&base,reqwest::Method::POST,&format!("{route}/element"),
                json!({"using":"css selector","value":"input[type=file]"})).await;
            let selected = selected["element-6066-11e4-a52e-4f735466cecf"].as_str().unwrap();
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/element/{selected}/value"),
                json!({"text":second_source.to_str().unwrap()})).await;
            let started = Instant::now();
            loop {
                if fixture.owner.service.store.list::<nw_domain::records::Asset>("asset/",128).unwrap().len() == 2 { break; }
                assert!(started.elapsed() < Duration::from_secs(20),"Second browser file intake did not register");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
            browser_click(&client,&base,&route,16,94).await;
            tokio::time::sleep(Duration::from_millis(700)).await;
            browser_click(&client,&base,&route,16,132).await;
            tokio::time::sleep(Duration::from_millis(700)).await;
            browser_click(&client,&base,&route,158,68).await;
            tokio::time::sleep(Duration::from_millis(700)).await;
            let png = driver(&client,&base,reqwest::Method::GET,&format!("{route}/screenshot"),Value::Null).await;
            std::fs::write(directory.join("batch-draft.png"),base64::engine::general_purpose::STANDARD.decode(png.as_str().unwrap()).unwrap()).unwrap();
            browser_click(&client,&base,&route,85,199).await;
            let started = Instant::now();
            let batch_plan = loop {
                let plans = fixture.owner.service.store.list::<nw_domain::batches::BatchPlan>("batch-plan/",128).unwrap();
                if let Some(plan) = plans.into_iter().next() { break plan; }
                if started.elapsed() >= Duration::from_secs(20) {
                    let png = driver(&client,&base,reqwest::Method::GET,&format!("{route}/screenshot"),Value::Null).await;
                    let bytes = base64::engine::general_purpose::STANDARD.decode(png.as_str().unwrap()).unwrap();
                    std::fs::write(directory.join("batch-review-timeout.png"),&bytes).unwrap();
                    if bytes.len() <= 256*1024 { println!("NW_BROWSER_BATCH_FAILURE_IMAGE:{}",png.as_str().unwrap()); }
                    panic!("Actual shared batch Review did not retain its frozen proposal");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            assert_eq!(batch_plan.items.len(),2);
            assert!(batch_plan.items.iter().all(|item| item.error.is_none()));
            assert_eq!(fixture.owner.service.store.list::<nw_domain::records::Job>("job/",128).unwrap().len(),1,
                "Batch review must not create jobs");
            tokio::time::sleep(Duration::from_millis(300)).await;
            browser_click(&client,&base,&route,215,199).await;
            let started = Instant::now();
            let batch = loop {
                let batches = fixture.owner.service.store.list::<nw_domain::batches::Batch>("batch/",128).unwrap();
                if let Some(batch) = batches.into_iter().next() {
                    let terminal = batch.members.iter().all(|member| {
                        member.job_id.as_ref().is_some_and(|id| fixture.owner.service.store.inspect_job(id).unwrap().state.terminal())
                    });
                    if terminal { break batch; }
                }
                if started.elapsed() >= Duration::from_secs(30) {
                    let png = driver(&client,&base,reqwest::Method::GET,&format!("{route}/screenshot"),Value::Null).await;
                    let bytes = base64::engine::general_purpose::STANDARD.decode(png.as_str().unwrap()).unwrap();
                    std::fs::write(directory.join("batch-run-timeout.png"),&bytes).unwrap();
                    if bytes.len() <= 256*1024 { println!("NW_BROWSER_BATCH_FAILURE_IMAGE:{}",png.as_str().unwrap()); }
                    panic!("Actual shared reviewed batch Run did not produce terminal children");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            assert_eq!(batch.members.len(),2);
            for member in &batch.members {
                let child = fixture.owner.service.store.inspect_job(member.job_id.as_ref().unwrap()).unwrap();
                assert_eq!(child.state,nw_domain::records::JobState::Succeeded);
                let artifact = child.artifact.as_ref().unwrap();
                assert_eq!(std::fs::read(&artifact.path).unwrap(),std::fs::read(&child.plan.source.identity.path).unwrap());
            }
            assert_eq!(fixture.owner.service.store.list::<nw_domain::records::Job>("job/",128).unwrap().len(),3);
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let png = driver(&client,&base,reqwest::Method::GET,&format!("{route}/screenshot"),Value::Null).await;
            let bytes = base64::engine::general_purpose::STANDARD.decode(png.as_str().unwrap()).unwrap();
            std::fs::write(directory.join("batch-completed.png"),&bytes).unwrap();
            if bytes.len() <= 256*1024 { println!("NW_BROWSER_BATCH_IMAGE:{}",png.as_str().unwrap()); }
            std::fs::write(directory.join("batch.json"),serde_json::to_vec_pretty(&json!({
                "plan_id":batch_plan.id,"plan_hash":batch_plan.hash,"batch_id":batch.id,
                "members":batch.members,"producer_job_count":3,
                "oracle":"Actual second file intake, two media checkboxes, batch Review and Run pointer input in same-source WASM; independent full bytes for each child",
                "not_qualified":["GUI cancellation","folder expansion","companions","replacement","other browser families"]
            })).unwrap()).unwrap();
        }
        std::fs::write(directory.join("boot.json"),serde_json::to_vec_pretty(&json!({"schema_version":1,"source":crate::web_assets::UI_SOURCE,"registry":registry_fingerprint(),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"runner_image":std::env::var("ImageVersion").ok(),"capabilities":session["capabilities"],"observed":"Same-source WASM startup, one-use pairing, fragment removal, actual UI file intake, frozen plan review and byte-verified published output under real CSP/MIME","not_qualified":["media playback","other browser families","native desktop interaction","full B7-J01"]})).unwrap()).unwrap();
        driver(
            &client,
            &base,
            reqwest::Method::POST,
            &format!("{route}/url"),
            json!({"url":"about:blank"}),
        )
        .await;
        let start = Instant::now();
        while !fixture.owner.leases.lock().await.is_empty() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Page teardown did not release its browser lease"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        driver(&client, &base, reqwest::Method::DELETE, &route, Value::Null).await;
        process.kill().await.unwrap();
        let _ = process.wait().await;
        if let Some(manager) = &mut window_manager {
            let _ = manager.kill().await;
            let _ = manager.wait().await;
        }
    }
}
