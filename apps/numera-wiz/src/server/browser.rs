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
    struct Fixture {
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
        let routes = super::super::router(owner.clone(), port, false, owner.stop.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, routes).await.unwrap();
        });
        Fixture {
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
    #[tokio::test]
    #[ignore = "Actual Chrome runtime test runs only in the hosted browser job"]
    async fn packaged_browser_boots_shared_wasm_under_the_enforced_policy() {
        use base64::Engine as _;
        assert!(std::env::var_os("NW_FIXTURE_COMPONENTS").is_some(),"The actual UI media journey requires the matching CI-built fixture components");
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
        let session=driver(&client,&base,reqwest::Method::POST,"/session",json!({"capabilities":{"alwaysMatch":{"browserName":"chrome","goog:chromeOptions":{"args":["--headless=new","--no-sandbox","--enable-unsafe-swiftshader","--use-angle=swiftshader","--window-size=1440,1000"]}}}})).await;
        let id = session["sessionId"].as_str().expect("WebDriver session id");
        let route = format!("/session/{id}");
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
            driver(&client,&base,reqwest::Method::POST,&format!("{route}/actions"),json!({"actions":[{"type":"pointer","id":"mouse","parameters":{"pointerType":"mouse"},"actions":[{"type":"pointerMove","duration":150,"origin":"viewport","x":1180,"y":409},{"type":"pointerDown","button":0},{"type":"pause","duration":150},{"type":"pointerUp","button":0}]}]})).await;
            let start=Instant::now();let job=loop {let jobs=fixture.owner.service.store.list::<nw_domain::records::Job>("job/",128).unwrap();if let Some(job)=jobs.into_iter().next() && job.state.terminal(){break job;}assert!(start.elapsed()<Duration::from_secs(30),"Shared Run control did not produce an observed terminal job");tokio::time::sleep(Duration::from_millis(100)).await;};
            assert_eq!(job.state,nw_domain::records::JobState::Succeeded);let artifact=job.artifact.as_ref().expect("A verified published artifact is required");assert_eq!(std::fs::read(&artifact.path).unwrap(),std::fs::read(&media.source).unwrap());
            std::fs::write(directory.join("media-output.json"),serde_json::to_vec_pretty(&json!({"job_id":job.id,"plan_hash":job.plan.hash,"state":job.state,"artifact":artifact,"verification":job.verification,"oracle":"Full independent source/output byte equality after actual shared UI file selection, review and Run"})).unwrap()).unwrap();
            tokio::time::sleep(Duration::from_millis(1200)).await;let screenshot=driver(&client,&base,reqwest::Method::GET,&format!("{route}/screenshot"),Value::Null).await;std::fs::write(directory.join("completed-workspace.png"),base64::engine::general_purpose::STANDARD.decode(screenshot.as_str().unwrap()).unwrap()).unwrap();

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
    }
}
