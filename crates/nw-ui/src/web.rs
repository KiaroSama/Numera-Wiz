//! Single-threaded browser presentation. Credentials are sent only to the paired origin.
use crate::{CommandClient, Reply};
use nw_domain::commands::{self, Request};
use std::sync::Arc;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;

struct BrowserClient { origin:String, bearer:String }
impl CommandClient for BrowserClient {
    fn send(&self, request:Request, reply:Reply) {
        let origin = self.origin.clone(); let bearer = self.bearer.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let id = request.request_id.clone();
            let result = match serde_json::to_vec(&request) {
                Ok(bytes) => fetch_json(&origin,"/api/v1/commands",Some(&bearer),bytes).await.and_then(|value|commands::parse(value).map_err(|error|error.to_string())),
                Err(error) => Err(error.to_string()),
            };
            let result = result.and_then(|response:commands::Response| if response.request_id == id {Ok(response)} else {Err("Response correlation does not match the admitted request".into())});
            reply(result);
        });
    }
}
fn js_error(error:JsValue)->String { error.as_string().unwrap_or_else(||"Browser operation failed".into()) }
struct Deadline { window:web_sys::Window, timer:i32, controller:web_sys::AbortController, _callback:Closure<dyn FnMut()> }
impl Drop for Deadline { fn drop(&mut self) {self.window.clear_timeout_with_handle(self.timer);self.controller.abort();} }
pub async fn fetch_json(origin:&str, route:&str, bearer:Option<&str>, bytes:Vec<u8>) -> Result<serde_json::Value,String> {
    let window = web_sys::window().ok_or("Browser window unavailable")?;
    if window.location().origin().map_err(js_error)? != origin {return Err("Foreign service origins are refused".into());}
    let controller = web_sys::AbortController::new().map_err(js_error)?;
    let cancel = controller.clone(); let callback = Closure::new(move ||cancel.abort());
    let timer = window.set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(),130000).map_err(js_error)?;
    let _deadline = Deadline { window:window.clone(),timer,controller:controller.clone(),_callback:callback };
    let init = web_sys::RequestInit::new();init.set_method("POST");init.set_redirect(web_sys::RequestRedirect::Error);init.set_credentials(web_sys::RequestCredentials::Omit);init.set_mode(web_sys::RequestMode::SameOrigin);init.set_signal(Some(&controller.signal()));
    let body = js_sys::Uint8Array::from(bytes.as_slice());init.set_body(body.as_ref());
    let request = web_sys::Request::new_with_str_and_init(&format!("{origin}{route}"),&init).map_err(js_error)?;
    request.headers().set("Content-Type","application/json").map_err(js_error)?;
    if let Some(bearer)=bearer {request.headers().set("Authorization",&format!("Bearer {bearer}")).map_err(js_error)?;}
    let response:web_sys::Response = JsFuture::from(window.fetch_with_request(&request)).await.map_err(js_error)?.dyn_into().map_err(js_error)?;
    if !response.ok() {return Err(format!("HTTP {}. Reconnect or pair again from the local application.",response.status()));}
    let body = response.body().ok_or("Response stream missing")?;
    let reader:web_sys::ReadableStreamDefaultReader = body.get_reader().dyn_into().map_err(js_error)?;
    let mut bytes = Vec::new();
    loop {
        let frame = JsFuture::from(reader.read()).await.map_err(js_error)?;
        if js_sys::Reflect::get(&frame,&JsValue::from_str("done")).map_err(js_error)?.as_bool() == Some(true) {break;}
        let chunk = js_sys::Uint8Array::new(&js_sys::Reflect::get(&frame,&JsValue::from_str("value")).map_err(js_error)?);
        if bytes.len().saturating_add(chunk.length() as usize) > commands::MAX_RESULT_BYTES {controller.abort();return Err("Response exceeds its bounded projection".into());}
        bytes.extend_from_slice(&chunk.to_vec());
    }
    commands::decode_json(&bytes).map_err(|error|error.to_string())
}
#[wasm_bindgen]
pub struct WebHandle { runner:eframe::WebRunner }
impl Default for WebHandle {fn default()->Self{Self::new()}}
#[wasm_bindgen]
impl WebHandle {
    #[wasm_bindgen(constructor)]
    pub fn new()->Self{Self{runner:eframe::WebRunner::new()}}
    pub async fn start(&self,canvas:web_sys::HtmlCanvasElement,endpoint:String,bearer:String)->Result<(),JsValue>{
        let window=web_sys::window().ok_or_else(||JsValue::from_str("Window unavailable"))?;
        if window.location().origin()? != endpoint {return Err(JsValue::from_str("The browser client requires its exact paired origin"));}
        let mut nonce=[0u8;16];window.crypto()?.get_random_values_with_u8_array(&mut nonce)?;
        let session=nonce.iter().map(|byte|format!("{byte:02x}")).collect::<String>();
        let client:Arc<dyn CommandClient>=Arc::new(BrowserClient{origin:endpoint,bearer});
        self.runner.start(canvas,eframe::WebOptions{renderer:eframe::Renderer::Glow,..Default::default()},Box::new(move|cc|Ok(Box::new(crate::WizApp::new(cc,client,session))))).await
    }
    pub fn destroy(&self){self.runner.destroy();}
}
