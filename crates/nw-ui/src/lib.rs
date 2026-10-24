//! Shared human media workspace; all authoritative behavior is requested from the owner.
mod cleanup;
pub mod client;
pub use client::{CommandClient, Reply};
use eframe::egui;
use nw_domain::{
    commands::{Request, Response},
    mux::{CleanIntent, NoMatch, Preservation, Selection, Selector},
    records::{Asset, Plan},
    views::{AssetSummary, JobSummary, SnapshotPage},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use web_time::Instant;

#[derive(Clone, Copy)]
enum Action {
    Assets,
    Asset,
    Register,
    Intake,
    Output(u64),
    Scope,
    Plan(u64),
    Submit,
    Jobs,
    Cancel,
    Inspect,
}
struct Pending {
    request: Request,
    action: Action,
}
struct Received {
    action: Action,
    response: Result<Response, String>,
}
pub struct WizApp {
    client: Arc<dyn CommandClient>,
    session: String,
    sequence: u64,
    inbox: Arc<Mutex<Option<Received>>>,
    pending: Option<Pending>,
    waiting: bool,
    error: Option<String>,
    status: String,
    scope: Option<String>,
    deferred: Option<(String, Value, Action)>,
    assets: Vec<AssetSummary>,
    asset_detail: Option<Arc<Asset>>,
    assets_cursor: Option<String>,
    assets_snapshot: Option<String>,
    selected: Option<String>,
    source: String,
    output: String,
    output_needed: bool,
    intent: CleanIntent,
    patches: Vec<cleanup::PatchForm>,
    audio_mode: usize,
    subtitle_mode: usize,
    audio_values: String,
    subtitle_values: String,
    audio_order: String,
    subtitle_order: String,
    generation: u64,
    plan: Option<Arc<Plan>>,
    jobs: Vec<JobSummary>,
    jobs_cursor: Option<String>,
    jobs_snapshot: Option<String>,
    last_jobs: Instant,
    stream_page: usize,
    inspect: String,
}
impl WizApp {
    fn run_reviewed(&mut self, ctx: &egui::Context) {
        if !self.waiting
            && self.pending.is_none()
            && let Some(plan) = self.plan.clone()
        {
            self.mutate(
                "jobs.submit",
                json!({"plan_id":plan.id,"plan_hash":plan.hash}),
                Action::Submit,
                ctx,
            );
        }
    }
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        client: Arc<dyn CommandClient>,
        session: String,
    ) -> Self {
        Self::with_context(&cc.egui_ctx, client, session)
    }
    fn with_context(ctx: &egui::Context, client: Arc<dyn CommandClient>, session: String) -> Self {
        let mut visuals = egui::Visuals::dark();
        // Presentation colors follow the pinned FilmCraft theme; processing stays in the owner.
        visuals.panel_fill = egui::Color32::from_rgb(29, 29, 29);
        visuals.window_fill = visuals.panel_fill;
        visuals.extreme_bg_color = egui::Color32::from_rgb(14, 14, 14);
        visuals.selection.bg_fill = egui::Color32::from_rgb(47, 107, 223);
        ctx.set_theme(egui::Theme::Dark);
        ctx.set_visuals_of(egui::Theme::Dark, visuals);
        let mut app = Self {
            client,
            session,
            sequence: 0,
            inbox: Arc::new(Mutex::new(None)),
            pending: None,
            waiting: false,
            error: None,
            status: "Ready".into(),
            scope: None,
            deferred: None,
            assets: vec![],
            asset_detail: None,
            assets_cursor: None,
            assets_snapshot: None,
            selected: None,
            source: String::new(),
            output: String::new(),
            output_needed: false,
            intent: CleanIntent {
                audio_selection: Selection {
                    selector: Selector::All,
                    no_match: NoMatch::SkipFile,
                },
                subtitle_selection: Selection {
                    selector: Selector::All,
                    no_match: NoMatch::AllowEmpty,
                },
                audio_order: vec![],
                subtitle_order: vec![],
                stream_tag_patches: vec![],
                preservation: Preservation::default(),
            },
            patches: vec![],
            audio_mode: 0,
            subtitle_mode: 0,
            audio_values: String::new(),
            subtitle_values: String::new(),
            audio_order: String::new(),
            subtitle_order: String::new(),
            generation: 0,
            plan: None,
            jobs: vec![],
            jobs_cursor: None,
            jobs_snapshot: None,
            last_jobs: Instant::now(),
            stream_page: 0,
            inspect: String::new(),
        };
        app.query("assets.list", json!({}), Action::Assets, ctx);
        app
    }
    fn selected_asset(&self) -> Option<Arc<Asset>> {
        self.asset_detail.clone().filter(|asset| self.selected.as_ref() == Some(&asset.id))
    }
    fn select_asset(&mut self, asset: &Asset, ctx: &egui::Context) {
        self.selected = Some(asset.id.clone());
        self.asset_detail = Some(Arc::new(asset.clone()));
        self.output.clear();
        self.output_needed = true;
        self.stream_page = 0;
        self.changed();
        ctx.request_repaint();
    }
    fn schedule_output(&mut self, ctx: &egui::Context) {
        if self.output_needed
            && !self.waiting
            && self.pending.is_none()
            && let Some(asset) = self.selected_asset()
        {
            self.output_needed = false;
            self.query(
                "assets.output_suggest",
                json!({"id":asset.id}),
                Action::Output(self.generation),
                ctx,
            );
        }
    }
    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.plan = None;
    }
    fn refresh_assets(&mut self, ctx: &egui::Context) {
        self.query("assets.list", json!({"replace_snapshot":self.assets_snapshot}), Action::Assets, ctx);
    }
    fn refresh_jobs(&mut self, ctx: &egui::Context) {
        self.query("jobs.list", json!({"replace_snapshot":self.jobs_snapshot}), Action::Jobs, ctx);
    }
    fn dispatch(&mut self, request: Request, action: Action, ctx: &egui::Context) {
        if self.waiting {
            return;
        }
        self.error = None;
        self.waiting = true;
        self.pending = Some(Pending {
            request: request.clone(),
            action,
        });
        let inbox = self.inbox.clone();
        let repaint = ctx.clone();
        self.client.send(
            request,
            Box::new(move |response| {
                if let Ok(mut slot) = inbox.lock() {
                    *slot = Some(Received { action, response });
                }
                repaint.request_repaint();
            }),
        );
    }
    fn query(&mut self, id: &str, params: Value, action: Action, ctx: &egui::Context) {
        if self.waiting || self.pending.is_some() {
            return;
        }
        self.sequence += 1;
        let mut request = Request::query(id, params);
        request.request_id = format!("{}-{}", self.session, self.sequence);
        self.dispatch(request, action, ctx);
    }
    fn mutate(&mut self, id: &str, params: Value, action: Action, ctx: &egui::Context) {
        if self.waiting || self.pending.is_some() {
            return;
        }
        let Some(scope) = self.scope.clone() else {
            self.deferred = Some((id.into(), params, action));
            self.query("access.replay_scope.create", json!({}), Action::Scope, ctx);
            return;
        };
        self.sequence += 1;
        let mut request = Request::query(id, params);
        request.request_id = format!("{}-{}", self.session, self.sequence);
        request.replay_scope = Some(scope);
        request.idempotency_key = Some(format!("{}-{}", self.session, self.sequence));
        self.dispatch(request, action, ctx);
    }
    fn receive(&mut self, ctx: &egui::Context) {
        let received = self.inbox.lock().ok().and_then(|mut slot| slot.take());
        let Some(received) = received else {
            return;
        };
        self.waiting = false;
        let response = match received.response {
            Ok(response) => response,
            Err(error) => {
                self.error = Some(error);
                self.status = if matches!(received.action, Action::Intake) {
                    "File intake did not finish. Select the file to try again.".into()
                } else {
                    "Connection interrupted. Retry retains the original request identity.".into()
                };
                return;
            }
        };
        self.pending = None;
        if !response.ok {
            if let Some(error) = response.error {
                if error.code == "SNAPSHOT_EXPIRED" {
                    match received.action {
                        Action::Assets => { self.assets.clear(); self.assets_cursor = None; self.assets_snapshot = None; }
                        Action::Jobs => { self.jobs.clear(); self.jobs_cursor = None; self.jobs_snapshot = None; }
                        _ => {}
                    }
                    self.status = "Inventory snapshot expired. Refresh to load a coherent page.".into();
                }
                if error.code == "REPLAY_SCOPE_EXPIRED" || error.code == "REPLAY_SCOPE_INVALID" {
                    self.scope = None;
                }
                self.error = Some(format!("{}: {}", error.code, error.message));
            }
            self.deferred = None;
            return;
        }
        let result = response.result.unwrap_or(Value::Null);
        let parsed: Result<(), String> = (|| {
            match received.action {
                Action::Assets => {
                    let page: SnapshotPage<AssetSummary> =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.assets = page.items;
                    self.assets_cursor = page.next_cursor;
                    self.assets_snapshot = Some(page.snapshot_id);
                }
                Action::Asset => {
                    let asset: Asset = serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.select_asset(&asset, ctx);
                }
                Action::Intake => {
                    self.mutate("assets.register", result, Action::Register, ctx);
                }
                Action::Output(generation) => {
                    if generation == self.generation {
                        self.output = result["output"]
                            .as_str()
                            .ok_or("Output suggestion missing")?
                            .into();
                        self.changed();
                    }
                }
                Action::Register => {
                    let asset: Asset =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.select_asset(&asset, ctx);
                    self.assets.retain(|old| old.id != asset.id);
                    if self.assets.len() < 128 { self.assets.push(AssetSummary::from(&asset)); }
                    self.status = "Media registered. Review stream selection and output.".into();
                }
                Action::Scope => {
                    self.scope = Some(
                        result["replay_scope"]
                            .as_str()
                            .ok_or("Replay scope missing")?
                            .into(),
                    );
                }
                Action::Plan(generation) => {
                    let plan: Plan =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    if generation == self.generation {
                        self.plan = Some(Arc::new(plan));
                        self.status = "Plan ready for review. No media job has started.".into();
                    } else {
                        self.status =
                            "Controls changed while planning. Review a fresh plan.".into();
                    }
                }
                Action::Submit => {
                    self.status = format!(
                        "Job accepted: {}",
                        result["job_id"].as_str().unwrap_or("unknown")
                    );
                    self.plan = None;
                    self.last_jobs = Instant::now() - Duration::from_secs(2);
                }
                Action::Jobs => {
                    let page: SnapshotPage<JobSummary> =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.jobs = page.items;
                    self.jobs_cursor = page.next_cursor;
                    self.jobs_snapshot = Some(page.snapshot_id);
                    self.last_jobs = Instant::now();
                }
                Action::Cancel => {
                    self.status =
                        "Cancellation result recorded; publication state remains authoritative."
                            .into();
                    self.last_jobs = Instant::now() - Duration::from_secs(2);
                }
                Action::Inspect => {
                    self.inspect =
                        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?;
                }
            }
            Ok(())
        })();
        if let Err(error) = parsed {
            self.error = Some(format!("Invalid owner response: {error}"));
        }
        if matches!(received.action, Action::Scope)
            && let Some((id, params, action)) = self.deferred.take()
        {
            self.mutate(&id, params, action, ctx);
        } else if matches!(
            received.action,
            Action::Submit | Action::Cancel | Action::Assets
        ) {
            self.refresh_jobs(ctx);
        }
    }
    fn catalog(&mut self, ui: &mut egui::Ui) {
        ui.heading("Media");
        if ui
            .add_enabled(
                !self.waiting && self.pending.is_none(),
                egui::Button::new("Refresh media"),
            )
            .clicked()
        {
            self.refresh_assets(ui.ctx());
        }
        if self.assets_cursor.is_some() && ui.add_enabled(!self.waiting && self.pending.is_none(), egui::Button::new("Next media page")).clicked() {
            self.query("assets.list", json!({"cursor":self.assets_cursor}), Action::Assets, ui.ctx());
        }
        ui.separator();
        let mut select = None;
        egui::ScrollArea::vertical()
            .id_salt("asset-catalog")
            .show(ui, |ui| {
                for asset in &self.assets {
                    let name = asset.display_name.as_str();
                    if ui
                        .selectable_label(self.selected.as_ref() == Some(&asset.id), name)
                        .clicked()
                    {
                        select = Some(asset.id.clone());
                    }
                    ui.small(format!(
                        "{} streams · {} bytes",
                        asset.stream_count,
                        asset.bytes
                    ));
                }
                if self.assets.is_empty() {
                    ui.label("Add a file to inspect its streams and prepare cleanup.");
                }
            });
        if let Some(id) = select {
            self.query("assets.inspect", json!({"id":id}), Action::Asset, ui.ctx());
        }
    }

    fn jobs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Jobs");
            if ui
                .add_enabled(
                    !self.waiting && self.pending.is_none(),
                    egui::Button::new("Refresh jobs"),
                )
                .clicked()
            {
                self.refresh_jobs(ui.ctx());
            }
            if self.jobs_cursor.is_some() && ui.add_enabled(!self.waiting && self.pending.is_none(), egui::Button::new("Next jobs page")).clicked() {
                self.query("jobs.list", json!({"cursor":self.jobs_cursor}), Action::Jobs, ui.ctx());
            }
        });
        let mut cancel = None;
        egui::ScrollArea::vertical()
            .id_salt("jobs")
            .max_height(145.0)
            .show(ui, |ui| {
                for job in &self.jobs {
                    ui.horizontal(|ui| {
                        ui.monospace(job.id.chars().take(8).collect::<String>());
                        ui.label(format!("{:?}", job.state));
                        ui.label(&job.output_name);
                        if !job.state.terminal()
                            && ui
                                .add_enabled(
                                    !self.waiting && self.pending.is_none(),
                                    egui::Button::new("Cancel"),
                                )
                                .clicked()
                        {
                            cancel = Some(job.id.clone());
                        }
                    });
                    if let Some(error) = &job.error_code {
                        ui.colored_label(
                            egui::Color32::from_rgb(237, 143, 112),
                            error,
                        );
                    }
                }
                if self.jobs.is_empty() {
                    ui.label(
                        "Accepted jobs appear here. Closing this window does not cancel them.",
                    );
                }
            });
        if let Some(id) = cancel {
            self.mutate("jobs.cancel", json!({"id":id}), Action::Cancel, ui.ctx());
        }
    }
}
impl eframe::App for WizApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        self.schedule_output(ctx);
        if ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Enter)) {
            self.run_reviewed(ctx);
        }
        if !self.waiting
            && self.pending.is_none()
            && self.jobs.iter().any(|job| !job.state.terminal())
        {
            if self.last_jobs.elapsed() >= Duration::from_secs(1) {
                self.refresh_jobs(ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        #[cfg(target_arch = "wasm32")]
        if !self.waiting
            && self.pending.is_none()
            && let Some(file) = ctx.input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .find_map(|file| file.web_file().cloned())
            })
        {
            self.waiting = true;
            self.error = None;
            self.status = "Uploading media to the local engine…".into();
            let inbox = self.inbox.clone();
            let repaint = ctx.clone();
            self.client.upload_dropped(
                file,
                Box::new(move |response| {
                    if let Ok(mut slot) = inbox.lock() {
                        *slot = Some(Received {
                            action: Action::Intake,
                            response,
                        });
                    }
                    repaint.request_repaint();
                }),
            );
        }
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .find_map(|file| file.path().is_absolute().then(|| file.path().to_owned()))
        });
        if let Some(path) = dropped
            && !self.waiting
            && self.pending.is_none()
        {
            if let Some(path) = path.to_str() {
                self.source = path.into();
                self.mutate(
                    "assets.register",
                    json!({"path":self.source}),
                    Action::Register,
                    ctx,
                );
            } else {
                self.error = Some("This native path needs the lossless path adapter.".into());
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("navigation").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Numera Wiz");
                ui.separator();
                ui.strong("Media tools");
                ui.separator();
                ui.label("Stream Cleanup");
            });
        });
        egui::Panel::bottom("job-queue")
            .default_size(160.0)
            .resizable(true)
            .show(ui, |ui| self.jobs(ui));
        egui::Panel::left("media-panel")
            .default_size(260.0)
            .resizable(true)
            .show(ui, |ui| self.catalog(ui));
        egui::Panel::right("plan-panel")
            .default_size(340.0)
            .resizable(true)
            .show(ui, |ui| self.plan_panel(ui));
        egui::CentralPanel::default().show(ui,|ui| {
            ui.heading("Stream Cleanup");
            ui.label("Inspect the original streams, select what to keep, then review the plan before running it.");
            ui.separator();
            if ui.add_enabled(!self.waiting && self.pending.is_none(),egui::Button::new("Choose media…")).clicked(){
                self.waiting=true;self.error=None;self.status="Selecting media…".into();let inbox=self.inbox.clone();let repaint=ui.ctx().clone();self.client.choose_media(Box::new(move|response|{if let Ok(mut slot)=inbox.lock(){*slot=Some(Received{action:Action::Intake,response});}repaint.request_repaint();}));
            }
            ui.horizontal(|ui| { ui.label("Input"); ui.add(egui::TextEdit::singleline(&mut self.source).hint_text("Absolute path granted to this session").desired_width((ui.available_width()-150.0).max(80.0))); if ui.add_enabled(!self.waiting && self.pending.is_none() && !self.source.trim().is_empty(),egui::Button::new("Add media")).clicked() { self.mutate("assets.register",json!({"path":self.source}),Action::Register,ui.ctx()); } });
            if let Some(asset) = self.selected_asset() { self.cleanup(ui,&asset); }
            else { ui.add_space(30.0); ui.label("Choose a file, drop media here, or enter an authorized engine path."); }
            ui.separator();
            ui.label(if self.waiting { "Working with the local engine…" } else { &self.status });
            if let Some(error) = &self.error { ui.colored_label(egui::Color32::from_rgb(237,143,112),error); }
            if !self.waiting && let Some(pending) = &self.pending {
                let request = pending.request.clone(); let action = pending.action;
                if ui.button("Reconnect and retry the same request").clicked() { self.dispatch(request,action,ui.ctx()); }
            }
            egui::CollapsingHeader::new("Command details").show(ui,|ui| {
                if ui.add_enabled(!self.waiting && self.pending.is_none(),egui::Button::new("Inspect available operations")).clicked() { self.query("commands.list",json!({}),Action::Inspect,ui.ctx()); }
                egui::ScrollArea::vertical().max_height(180.0).show(ui,|ui| { ui.add(egui::TextEdit::multiline(&mut self.inspect).code_editor().interactive(false).desired_width(f32::INFINITY)); });
            });
        });
    }
}
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(test)]
mod tests {
    use super::*;
    use nw_domain::records::{FileIdentity, MediaProbe};
    use std::collections::VecDeque;
    #[derive(Default)]
    struct Port {
        waiting: Mutex<VecDeque<(Request, Reply)>>,
    }
    impl CommandClient for Port {
        fn send(&self, request: Request, reply: Reply) {
            self.waiting.lock().unwrap().push_back((request, reply));
        }
        fn choose_media(&self, _reply: Reply) {
            panic!("This controller test does not invoke the platform file picker");
        }
    }
    impl Port {
        fn complete(&self, result: Value) -> Request {
            let (request, reply) = self
                .waiting
                .lock()
                .unwrap()
                .pop_front()
                .expect("Expected pending owner request");
            reply(Ok(Response::success(&request.request_id, result)));
            request
        }
    }
    fn asset(id: &str) -> Arc<Asset> {
        Arc::new(Asset {
            id: id.into(),
            creator: "controller-unit".into(),
            display_name: id.into(),
            identity: FileIdentity {
                path: format!("/controller-unit/{id}.mkv"),
                bytes: "0".into(),
                sha256: "00".repeat(32),
            },
            probe: MediaProbe {
                stream_details: Default::default(),
                streams: vec![],
                format: "synthetic-controller-unit".into(),
                global_tags: Default::default(),
                chapters: json!([]),
                verification_scope: "No actual media in this controller test".into(),
            },
        })
    }
    #[test]
    fn delayed_suggestion_preserves_the_current_selection_and_manual_output() {
        let ctx = egui::Context::default();
        let port = Arc::new(Port::default());
        let mut app = WizApp::with_context(&ctx, port.clone(), "unit".into());
        port.complete(json!({"schema_version":1,"project_revision":null,"snapshot_id":"assets","snapshot_expires_at":"30","projection":"assets.summary.v1","items":[],"next_cursor":null,"event_cursor":null,"total_count":0,"count_accuracy":"exact"}));
        app.receive(&ctx);
        port.complete(json!({"schema_version":1,"project_revision":null,"snapshot_id":"jobs","snapshot_expires_at":"30","projection":"jobs.summary.v1","items":[],"next_cursor":null,"event_cursor":null,"total_count":0,"count_accuracy":"exact"}));
        app.receive(&ctx);
        let a = asset("a");
        let b = asset("b");
        app.assets = vec![AssetSummary::from(a.as_ref()), AssetSummary::from(b.as_ref())];
        app.select_asset(&a, &ctx);
        app.schedule_output(&ctx);
        app.select_asset(&b, &ctx);
        let first = port.complete(json!({"output":"/controller-unit/a-output.mkv"}));
        assert_eq!(first.params["id"], "a");
        app.receive(&ctx);
        assert!(app.output.is_empty());
        app.schedule_output(&ctx);
        let second = port.complete(json!({"output":"/controller-unit/b-output.mkv"}));
        assert_eq!(second.params["id"], "b");
        app.receive(&ctx);
        assert_eq!(app.output, "/controller-unit/b-output.mkv");
        app.select_asset(&a, &ctx);
        app.schedule_output(&ctx);
        app.output = "/controller-unit/manually-selected.mkv".into();
        app.changed();
        port.complete(json!({"output":"/controller-unit/a-output.mkv"}));
        app.receive(&ctx);
        assert_eq!(app.output, "/controller-unit/manually-selected.mkv");
        assert!(app.plan.is_none());
    }
}
