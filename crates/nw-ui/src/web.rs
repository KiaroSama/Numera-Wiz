//! Single-threaded browser presentation. Credentials are sent only to the paired origin.
mod download;
use crate::{CommandClient, Reply, SaveReply};
use nw_domain::commands::{self, Request};
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

struct BrowserClient {
    origin: String,
    bearer: String,
}
impl CommandClient for BrowserClient {
    fn choose_media(&self, reply: Reply) {
        let origin = self.origin.clone();
        let bearer = self.bearer.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match choose_file().await {
                Ok(file) => upload_file(&origin, &bearer, file).await,
                Err(error) => Err(error),
            };
            reply(result.map(|params| commands::Response::success("intake", params)));
        });
    }
    fn upload_dropped(&self, file: web_sys::File, reply: Reply) {
        let origin = self.origin.clone();
        let bearer = self.bearer.clone();
        wasm_bindgen_futures::spawn_local(async move {
            reply(
                upload_file(&origin, &bearer, file)
                    .await
                    .map(|params| commands::Response::success("intake", params)),
            );
        });
    }
    fn save_artifact(
        &self,
        manifest: nw_domain::artifacts::ArtifactManifest,
        destination: Option<String>,
        control: Arc<nw_domain::download::SaveControl>,
        reply: SaveReply,
    ) {
        if destination.is_some() {
            drop(nw_domain::download::SaveSession(control));
            reply(Err(
                "Browser saves require a browser-selected file handle".into()
            ));
            return;
        }
        // Start the picker synchronously while the human activation is still live.
        let picker = download::select_destination(&manifest);
        let origin = self.origin.clone();
        let bearer = self.bearer.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _session = nw_domain::download::SaveSession(control.clone());
            let result = match picker {
                Ok(picker) => download::save(&origin, &bearer, manifest, picker, &control).await,
                Err(error) => Err(error),
            };
            reply(result);
        });
    }
    fn send(&self, request: Request, reply: Reply) {
        let origin = self.origin.clone();
        let bearer = self.bearer.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let id = request.request_id.clone();
            let result = match serde_json::to_vec(&request) {
                Ok(bytes) => fetch_json(&origin, "/api/v1/commands", Some(&bearer), bytes)
                    .await
                    .and_then(|value| commands::parse(value).map_err(|error| error.to_string())),
                Err(error) => Err(error.to_string()),
            };
            let result = result.and_then(|response: commands::Response| {
                if response.request_id == id {
                    Ok(response)
                } else {
                    Err("Response correlation does not match the admitted request".into())
                }
            });
            reply(result);
        });
    }
}
fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| "Browser operation failed".into())
}
struct Deadline {
    window: web_sys::Window,
    timer: i32,
    controller: web_sys::AbortController,
    _callback: Closure<dyn FnMut()>,
}
impl Drop for Deadline {
    fn drop(&mut self) {
        self.window.clear_timeout_with_handle(self.timer);
        self.controller.abort();
    }
}
pub async fn fetch_json(
    origin: &str,
    route: &str,
    bearer: Option<&str>,
    bytes: Vec<u8>,
) -> Result<serde_json::Value, String> {
    let body = js_sys::Uint8Array::from(bytes.as_slice());
    fetch_value(
        origin,
        route,
        bearer,
        "POST",
        body.as_ref(),
        "application/json",
        130000,
    )
    .await
}
async fn fetch_value(
    origin: &str,
    route: &str,
    bearer: Option<&str>,
    method: &str,
    body: &JsValue,
    content_type: &str,
    timeout: i32,
) -> Result<serde_json::Value, String> {
    let window = web_sys::window().ok_or("Browser window unavailable")?;
    if window.location().origin().map_err(js_error)? != origin {
        return Err("Foreign service origins are refused".into());
    }
    let controller = web_sys::AbortController::new().map_err(js_error)?;
    let cancel = controller.clone();
    let callback = Closure::new(move || cancel.abort());
    let timer = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            timeout,
        )
        .map_err(js_error)?;
    let _deadline = Deadline {
        window: window.clone(),
        timer,
        controller: controller.clone(),
        _callback: callback,
    };
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    init.set_redirect(web_sys::RequestRedirect::Error);
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_mode(web_sys::RequestMode::SameOrigin);
    init.set_signal(Some(&controller.signal()));
    init.set_body(body);
    let request = web_sys::Request::new_with_str_and_init(&format!("{origin}{route}"), &init)
        .map_err(js_error)?;
    request
        .headers()
        .set("Content-Type", content_type)
        .map_err(js_error)?;
    if let Some(bearer) = bearer {
        request
            .headers()
            .set("Authorization", &format!("Bearer {bearer}"))
            .map_err(js_error)?;
    }
    let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;
    if !response.ok() {
        return Err(format!(
            "HTTP {}. Reconnect or pair again from the local application.",
            response.status()
        ));
    }
    let body = response.body().ok_or("Response stream missing")?;
    let reader: web_sys::ReadableStreamDefaultReader = body
        .get_reader()
        .dyn_into()
        .map_err(|error: js_sys::Object| js_error(error.into()))?;
    let mut bytes = Vec::new();
    loop {
        let frame = JsFuture::from(reader.read()).await.map_err(js_error)?;
        if js_sys::Reflect::get(&frame, &JsValue::from_str("done"))
            .map_err(js_error)?
            .as_bool()
            == Some(true)
        {
            break;
        }
        let chunk = js_sys::Uint8Array::new(
            &js_sys::Reflect::get(&frame, &JsValue::from_str("value")).map_err(js_error)?,
        );
        if bytes.len().saturating_add(chunk.length() as usize) > commands::MAX_RESULT_BYTES {
            controller.abort();
            return Err("Response exceeds its bounded projection".into());
        }
        bytes.extend_from_slice(&chunk.to_vec());
    }
    commands::decode_json(&bytes).map_err(|error| error.to_string())
}
struct BrowserLease {
    window: web_sys::Window,
    interval: i32,
    _callback: Closure<dyn FnMut()>,
    origin: String,
    bearer: String,
    id: String,
}
impl Drop for BrowserLease {
    fn drop(&mut self) {
        self.window.clear_interval_with_handle(self.interval);
        let init = web_sys::RequestInit::new();
        init.set_method("POST");
        let _ = js_sys::Reflect::set(
            init.as_ref(),
            &JsValue::from_str("keepalive"),
            &JsValue::TRUE,
        );
        init.set_redirect(web_sys::RequestRedirect::Error);
        init.set_credentials(web_sys::RequestCredentials::Omit);
        init.set_mode(web_sys::RequestMode::SameOrigin);
        init.set_body(&JsValue::from_str(
            &serde_json::json!({"id":self.id,"release":true}).to_string(),
        ));
        if let Ok(request) =
            web_sys::Request::new_with_str_and_init(&format!("{}/api/v1/lease", self.origin), &init)
        {
            let _ = request.headers().set("Content-Type", "application/json");
            let _ = request
                .headers()
                .set("Authorization", &format!("Bearer {}", self.bearer));
            let _ = self.window.fetch_with_request(&request);
        }
    }
}
async fn paired_token(origin: &str, supplied: String) -> Result<String, String> {
    let window = web_sys::window().ok_or("Window unavailable")?;
    let hash = if supplied.starts_with("#pair=") {
        supplied.clone()
    } else {
        window.location().hash().map_err(js_error)?
    };
    let bearer = if let Some(proof) = hash.strip_prefix("#pair=") {
        let path = format!(
            "{}{}",
            window.location().pathname().map_err(js_error)?,
            window.location().search().map_err(js_error)?
        );
        window
            .history()
            .map_err(js_error)?
            .replace_state_with_url(&JsValue::NULL, "", Some(&path))
            .map_err(js_error)?;
        let paired = fetch_json(
            origin,
            "/api/v1/pairing/exchange",
            None,
            serde_json::to_vec(&serde_json::json!({"proof":proof}))
                .map_err(|error| error.to_string())?,
        )
        .await?;
        paired["bearer"]
            .as_str()
            .ok_or("Pairing bearer missing")?
            .to_owned()
    } else {
        window
            .session_storage()
            .map_err(js_error)?
            .and_then(|storage| storage.get_item("numera.session").ok().flatten())
            .ok_or("NW_PAIRING_REQUIRED: open this workspace from the local application")?
    };
    let health = fetch_json(origin, "/api/v1/session", Some(&bearer), b"{}".to_vec()).await?;
    if health["registry"] != commands::registry_fingerprint()
        || health["ui_source"].as_str() != option_env!("NW_UI_BUILD_SHA")
    {
        return Err("NW_BUILD_MISMATCH: reload the matching owner UI assets".into());
    }
    if let Ok(Some(storage)) = window.session_storage() {
        let _ = storage.set_item("numera.session", &bearer);
    }
    Ok(bearer)
}
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
    lease: RefCell<Option<BrowserLease>>,
}
impl Default for WebHandle {
    fn default() -> Self {
        Self::new()
    }
}
#[wasm_bindgen]
impl WebHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            runner: eframe::WebRunner::new(),
            lease: RefCell::new(None),
        }
    }
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
        endpoint: String,
        bearer: String,
    ) -> Result<(), JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("Window unavailable"))?;
        if window.location().origin()? != endpoint {
            return Err(JsValue::from_str(
                "The browser client requires its exact paired origin",
            ));
        }
        let bearer = paired_token(&endpoint, bearer)
            .await
            .map_err(|error| JsValue::from_str(&error))?;
        let mut nonce = [0u8; 16];
        window
            .crypto()?
            .get_random_values_with_u8_array(&mut nonce)?;
        let session = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        fetch_json(
            &endpoint,
            "/api/v1/lease",
            Some(&bearer),
            serde_json::to_vec(&serde_json::json!({"id":session}))
                .map_err(|error| JsValue::from_str(&error.to_string()))?,
        )
        .await
        .map_err(|error| JsValue::from_str(&error))?;
        let heartbeat_origin = endpoint.clone();
        let heartbeat_token = bearer.clone();
        let heartbeat_id = session.clone();
        let callback = Closure::new(move || {
            let origin = heartbeat_origin.clone();
            let token = heartbeat_token.clone();
            let id = heartbeat_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = fetch_json(
                    &origin,
                    "/api/v1/lease",
                    Some(&token),
                    serde_json::to_vec(&serde_json::json!({"id":id})).unwrap_or_default(),
                )
                .await;
            });
        });
        let interval = window.set_interval_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            15000,
        )?;
        *self.lease.borrow_mut() = Some(BrowserLease {
            window: window.clone(),
            interval,
            _callback: callback,
            origin: endpoint.clone(),
            bearer: bearer.clone(),
            id: session.clone(),
        });
        let client: Arc<dyn CommandClient> = Arc::new(BrowserClient {
            origin: endpoint,
            bearer,
        });
        self.runner
            .start(
                canvas,
                eframe::WebOptions {
                    renderer: eframe::Renderer::Glow,
                    ..Default::default()
                },
                Box::new(move |cc| Ok(Box::new(crate::WizApp::new(cc, client, session)))),
            )
            .await
    }
    pub fn destroy(&self) {
        self.lease.borrow_mut().take();
        self.runner.destroy();
    }
}

struct FileSelection {
    input: web_sys::HtmlInputElement,
    _changed: Closure<dyn FnMut()>,
    cancelled: Closure<dyn FnMut()>,
}
impl Drop for FileSelection {
    fn drop(&mut self) {
        self.input.set_onchange(None);
        let _ = self
            .input
            .remove_event_listener_with_callback("cancel", self.cancelled.as_ref().unchecked_ref());
        self.input.remove();
    }
}
async fn choose_file() -> Result<web_sys::File, String> {
    use std::rc::Rc;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or("Document unavailable")?;
    let input: web_sys::HtmlInputElement = document
        .create_element("input")
        .map_err(js_error)?
        .dyn_into()
        .map_err(|error: web_sys::Element| js_error(error.into()))?;
    input.set_type("file");
    input.set_id("media-file-input");
    input.set_hidden(true);
    input.set_multiple(false);
    let resolver = Rc::new(RefCell::new(None::<js_sys::Function>));
    let receiver = resolver.clone();
    let promise = js_sys::Promise::new(&mut move |resolve, _reject| {
        *receiver.borrow_mut() = Some(resolve);
    });
    let selected = input.clone();
    let changed_resolver = resolver.clone();
    let changed = Closure::new(move || {
        if let Some(resolve) = changed_resolver.borrow().as_ref() {
            let file = selected
                .files()
                .and_then(|files| files.get(0))
                .map(JsValue::from)
                .unwrap_or(JsValue::NULL);
            let _ = resolve.call1(&JsValue::NULL, &file);
        }
    });
    let cancelled = Closure::new(move || {
        if let Some(resolve) = resolver.borrow().as_ref() {
            let _ = resolve.call1(&JsValue::NULL, &JsValue::NULL);
        }
    });
    input.set_onchange(Some(changed.as_ref().unchecked_ref()));
    input
        .add_event_listener_with_callback("cancel", cancelled.as_ref().unchecked_ref())
        .map_err(js_error)?;
    document
        .body()
        .ok_or("Document body unavailable")?
        .append_child(&input)
        .map_err(js_error)?;
    let picker = FileSelection {
        input,
        _changed: changed,
        cancelled,
    };
    picker.input.click();
    let value = JsFuture::from(promise).await.map_err(js_error)?;
    value
        .dyn_into()
        .map_err(|_| "File selection cancelled".into())
}
async fn upload_file(
    origin: &str,
    bearer: &str,
    file: web_sys::File,
) -> Result<serde_json::Value, String> {
    let size = file.size();
    if !size.is_finite() || size <= 0.0 || size > 1073741824.0 || size.fract() != 0.0 {
        return Err("Select a nonempty file of at most 1 GiB".into());
    }
    let input = serde_json::json!({"display_name":file.name(),"bytes":format!("{size:.0}")});
    let ticket = fetch_json(
        origin,
        "/api/v1/uploads",
        Some(bearer),
        serde_json::to_vec(&input).map_err(|error| error.to_string())?,
    )
    .await?;
    let id = ticket["upload_id"]
        .as_str()
        .filter(|id| {
            id.len() == 36
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        })
        .ok_or("Upload handle missing")?;
    let completed = fetch_value(
        origin,
        &format!("/api/v1/uploads/{id}"),
        Some(bearer),
        "PUT",
        file.as_ref(),
        "application/octet-stream",
        900000,
    )
    .await?;
    if completed["upload_id"] != id {
        return Err("Intake completion does not match the admitted handle".into());
    }
    Ok(serde_json::json!({"upload_id":id}))
}
