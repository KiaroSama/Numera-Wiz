//! Shared native/WASM presentation; no store, service or process dependency.
use eframe::egui;
use nw_domain::commands::Request;
use std::sync::{Arc,Mutex};

pub struct WizApp {
    endpoint:String,
    bearer:String,
    request:String,
    response:String,
    pending:Arc<Mutex<Option<String>>>,
    busy:bool,
}
impl WizApp {
    pub fn new(cc:&eframe::CreationContext<'_>,endpoint:String,bearer:String)->Self{
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self{endpoint,bearer,request:serde_json::to_string_pretty(&Request::query("commands.list",serde_json::json!({}))).expect("request"),response:"Select Discover to inspect the actual development contract.".into(),pending:Arc::new(Mutex::new(None)),busy:false}
    }
    fn send(&mut self,ctx:&egui::Context){
        if self.busy{return;}
        let request=match serde_json::from_str::<Request>(&self.request){Ok(value)=>value,Err(error)=>{self.response=format!("Invalid request: {error}");return;}};
        if let Err(error)=request.validate(){self.response=error.to_string();return;}
        let body=match serde_json::to_vec(&request){Ok(value)=>value,Err(error)=>{self.response=error.to_string();return;}};
        let mut http=ehttp::Request::post(format!("{}/api/v1/commands",self.endpoint),body);
        http.headers.insert("Content-Type","application/json");
        http.headers.insert("Authorization",format!("Bearer {}",self.bearer));
        let pending=self.pending.clone();let context=ctx.clone();self.busy=true;
        ehttp::fetch(http,move|result|{
            let text=match result{Ok(response)=>{
                if response.bytes.len()>256*1024{"Response exceeds the admitted UI byte budget".into()}else{match serde_json::from_slice::<serde_json::Value>(&response.bytes){Ok(value)=>serde_json::to_string_pretty(&value).unwrap_or_default(),Err(_)=>format!("HTTP {}: invalid bounded JSON response",response.status)}}
            },Err(error)=>format!("Connection failed: {error}")};
            if let Ok(mut slot)=pending.lock(){*slot=Some(text);}context.request_repaint();
        });
    }
}
impl eframe::App for WizApp {
    fn logic(&mut self,_ctx:&egui::Context,_frame:&mut eframe::Frame){
        if let Ok(mut slot)=self.pending.lock()&&let Some(response)=slot.take(){self.response=response;self.busy=false;}
    }
    fn ui(&mut self,ui:&mut egui::Ui,_frame:&mut eframe::Frame){
        egui::Panel::top("workspace").show(ui,|ui|{ui.horizontal(|ui|{ui.heading("Numera Wiz");ui.separator();ui.label("Media tools · Development");ui.label("Editing / Audio / Color qualification pending");});});
        egui::Panel::left("project").default_size(230.0).show(ui,|ui|{
            ui.heading("Project / Media");ui.label("Pure stream-selection inspection needs no project or file access.");ui.separator();
            if ui.button("Discover commands").clicked(){self.request=serde_json::to_string_pretty(&Request::query("commands.list",serde_json::json!({}))).expect("request");self.send(ui.ctx());}
            ui.label("Use the shared command schema for explicit streams, selectors and metadata rules. No hidden selection is assumed.");
            ui.separator();ui.label("Native and browser use this same Rust UI.");
        });
        egui::Panel::bottom("progress").default_size(75.0).show(ui,|ui|{ui.label(if self.busy{"Waiting for the local engine…"}else{"Ready"});ui.label("No complete media editor or portable release is claimed by this development view.");});
        egui::CentralPanel::default().show(ui,|ui|{
            ui.heading("Command inspector");ui.label("Canonical request");
            egui::ScrollArea::vertical().id_salt("request").max_height(260.0).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.request).code_editor().desired_rows(12).desired_width(f32::INFINITY));});
            if ui.add_enabled(!self.busy,egui::Button::new("Run query")).clicked(){self.send(ui.ctx());}
            ui.separator();ui.label("Authoritative response");
            egui::ScrollArea::vertical().id_salt("response").show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.response).code_editor().desired_rows(18).desired_width(f32::INFINITY));});
        });
    }
}
#[cfg(target_arch="wasm32")]
mod web {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    pub struct WebHandle{runner:eframe::WebRunner}
    #[wasm_bindgen]
    impl WebHandle {
        #[wasm_bindgen(constructor)]
        pub fn new()->Self{Self{runner:eframe::WebRunner::new()}}
        pub async fn start(&self,canvas:web_sys::HtmlCanvasElement,endpoint:String,bearer:String)->Result<(),JsValue>{
            self.runner.start(canvas,eframe::WebOptions{renderer:eframe::Renderer::Glow,..Default::default()},Box::new(move|cc|Ok(Box::new(super::WizApp::new(cc,endpoint,bearer))))).await
        }
        pub fn destroy(&self){self.runner.destroy();}
    }
}
