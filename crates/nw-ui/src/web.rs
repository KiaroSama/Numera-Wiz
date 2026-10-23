//! Single-threaded browser presentation. Credentials are sent only to the paired origin.
use crate::{CommandClient, Reply};
use nw_domain::commands::{self, Request};
use std::{sync::Arc,cell::RefCell};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

struct BrowserClient {
    origin: String,
    bearer: String,
}
impl CommandClient for BrowserClient {
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
            130000,
        )
        .map_err(js_error)?;
    let _deadline = Deadline {
        window: window.clone(),
        timer,
        controller: controller.clone(),
        _callback: callback,
    };
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_redirect(web_sys::RequestRedirect::Error);
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_mode(web_sys::RequestMode::SameOrigin);
    init.set_signal(Some(&controller.signal()));
    let body = js_sys::Uint8Array::from(bytes.as_slice());
    init.set_body(body.as_ref());
    let request = web_sys::Request::new_with_str_and_init(&format!("{origin}{route}"), &init)
        .map_err(js_error)?;
    request
        .headers()
        .set("Content-Type", "application/json")
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
    let reader: web_sys::ReadableStreamDefaultReader =
        body.get_reader().dyn_into().map_err(|error:js_sys::Object|js_error(error.into()))?;
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
struct BrowserLease {window:web_sys::Window,interval:i32,_callback:Closure<dyn FnMut()>,origin:String,bearer:String,id:String}
impl Drop for BrowserLease {
    fn drop(&mut self){
        self.window.clear_interval_with_handle(self.interval);
        let init=web_sys::RequestInit::new();init.set_method("POST");init.set_keepalive(true);init.set_redirect(web_sys::RequestRedirect::Error);init.set_credentials(web_sys::RequestCredentials::Omit);init.set_mode(web_sys::RequestMode::SameOrigin);
        init.set_body(&JsValue::from_str(&serde_json::json!({"id":self.id,"release":true}).to_string()));
        if let Ok(request)=web_sys::Request::new_with_str_and_init(&format!("{}/api/v1/lease",self.origin),&init){let _=request.headers().set("Content-Type","application/json");let _=request.headers().set("Authorization",&format!("Bearer {}",self.bearer));let _=self.window.fetch_with_request(&request);}
    }
}
async fn paired_token(origin:&str,supplied:String)->Result<String,String>{
    let window=web_sys::window().ok_or("Window unavailable")?;
    let hash=if supplied.starts_with("#pair="){supplied.clone()}else{window.location().hash().map_err(js_error)?};
    let bearer=if let Some(proof)=hash.strip_prefix("#pair="){
        let path=format!("{}{}",window.location().pathname().map_err(js_error)?,window.location().search().map_err(js_error)?);
        window.history().map_err(js_error)?.replace_state_with_url(&JsValue::NULL,"",Some(&path)).map_err(js_error)?;
        let paired=fetch_json(origin,"/api/v1/pairing/exchange",None,serde_json::to_vec(&serde_json::json!({"proof":proof})).map_err(|error|error.to_string())?).await?;
        paired["bearer"].as_str().ok_or("Pairing bearer missing")?.to_owned()
    }else{window.session_storage().map_err(js_error)?.and_then(|storage|storage.get_item("numera.session").ok().flatten()).ok_or("NW_PAIRING_REQUIRED: open this workspace from the local application")?};
    let health=fetch_json(origin,"/api/v1/session",Some(&bearer),b"{}".to_vec()).await?;
    if health["registry"]!=commands::registry_fingerprint() || health["ui_source"].as_str()!=option_env!("NW_UI_BUILD_SHA"){return Err("NW_BUILD_MISMATCH: reload the matching owner UI assets".into());}
    if let Ok(Some(storage))=window.session_storage(){let _=storage.set_item("numera.session",&bearer);}
    Ok(bearer)
}
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
    lease:RefCell<Option<BrowserLease>>,
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
            lease:RefCell::new(None),
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
        let bearer=paired_token(&endpoint,bearer).await.map_err(|error|JsValue::from_str(&error))?;
        let mut nonce = [0u8; 16];
        window
            .crypto()?
            .get_random_values_with_u8_array(&mut nonce)?;
        let session = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        fetch_json(&endpoint,"/api/v1/lease",Some(&bearer),serde_json::to_vec(&serde_json::json!({"id":session})).map_err(|error|JsValue::from_str(&error.to_string()))?).await.map_err(|error|JsValue::from_str(&error))?;
        let heartbeat_origin=endpoint.clone();let heartbeat_token=bearer.clone();let heartbeat_id=session.clone();
        let callback=Closure::new(move||{let origin=heartbeat_origin.clone();let token=heartbeat_token.clone();let id=heartbeat_id.clone();wasm_bindgen_futures::spawn_local(async move{let _=fetch_json(&origin,"/api/v1/lease",Some(&token),serde_json::to_vec(&serde_json::json!({"id":id})).unwrap_or_default()).await;});});
        let interval=window.set_interval_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(),15000)?;
        *self.lease.borrow_mut()=Some(BrowserLease{window:window.clone(),interval,_callback:callback,origin:endpoint.clone(),bearer:bearer.clone(),id:session.clone()});
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
