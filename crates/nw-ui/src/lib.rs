//! Shared human media workspace; all authoritative behavior is requested from the owner.
mod cleanup;
pub mod client;
pub use client::{CommandClient, Reply};
use eframe::egui;
use nw_domain::{
    commands::{Request, Response},
    mux::{CleanIntent, NoMatch, Preservation, Selection, Selector},
    records::{Asset, Job, Plan},
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
    Register,
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
    assets: Vec<Arc<Asset>>,
    selected: Option<String>,
    source: String,
    output: String,
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
    jobs: Vec<Arc<Job>>,
    last_jobs: Instant,
    stream_page: usize,
    inspect: String,
}
impl WizApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        client: Arc<dyn CommandClient>,
        session: String,
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        // Presentation colors follow the pinned FilmCraft theme; processing stays in the owner.
        visuals.panel_fill = egui::Color32::from_rgb(29, 29, 29);
        visuals.window_fill = visuals.panel_fill;
        visuals.extreme_bg_color = egui::Color32::from_rgb(14, 14, 14);
        visuals.selection.bg_fill = egui::Color32::from_rgb(47, 107, 223);
        cc.egui_ctx.set_visuals(visuals);
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
            selected: None,
            source: String::new(),
            output: String::new(),
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
            last_jobs: Instant::now(),
            stream_page: 0,
            inspect: String::new(),
        };
        app.query("assets.list", json!({}), Action::Assets, &cc.egui_ctx);
        app
    }
    fn selected_asset(&self) -> Option<Arc<Asset>> {
        self.assets
            .iter()
            .find(|asset| self.selected.as_ref() == Some(&asset.id))
            .cloned()
    }
    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.plan = None;
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
                self.status =
                    "Connection interrupted. Retry retains the original request identity.".into();
                return;
            }
        };
        self.pending = None;
        if !response.ok {
            if let Some(error) = response.error {
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
                    let assets: Vec<Asset> =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.assets = assets.into_iter().map(Arc::new).collect();
                }
                Action::Register => {
                    let asset: Asset =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.selected = Some(asset.id.clone());
                    self.output = cleanup::suggest_output(&asset.identity.path);
                    self.assets.retain(|old| old.id != asset.id);
                    self.assets.push(Arc::new(asset));
                    self.stream_page = 0;
                    self.changed();
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
                    let jobs: Vec<Job> =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.jobs = jobs.into_iter().map(Arc::new).collect();
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
        if let Some((id, params, action)) = self.deferred.take() {
            self.mutate(&id, params, action, ctx);
        } else if matches!(
            received.action,
            Action::Submit | Action::Cancel | Action::Assets
        ) {
            self.query("jobs.list", json!({}), Action::Jobs, ctx);
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
            self.query("assets.list", json!({}), Action::Assets, ui.ctx());
        }
        ui.separator();
        let mut select = None;
        egui::ScrollArea::vertical()
            .id_salt("asset-catalog")
            .show(ui, |ui| {
                for asset in &self.assets {
                    let name = cleanup::file_name(&asset.identity.path);
                    if ui
                        .selectable_label(self.selected.as_ref() == Some(&asset.id), name)
                        .on_hover_text(&asset.identity.path)
                        .clicked()
                    {
                        select = Some(asset.clone());
                    }
                    ui.small(format!(
                        "{} streams · {} bytes",
                        asset.probe.streams.len(),
                        asset.identity.bytes
                    ));
                }
                if self.assets.is_empty() {
                    ui.label("Add a file to inspect its streams and prepare cleanup.");
                }
            });
        if let Some(asset) = select {
            self.selected = Some(asset.id.clone());
            self.output = cleanup::suggest_output(&asset.identity.path);
            self.stream_page = 0;
            self.changed();
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
                self.query("jobs.list", json!({}), Action::Jobs, ui.ctx());
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
                        ui.label(cleanup::file_name(&job.plan.output));
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
                    if let Some(error) = &job.error {
                        ui.colored_label(
                            egui::Color32::from_rgb(237, 143, 112),
                            format!("{}: {}", error.code, error.message),
                        );
                    }
                    if let Some(verified) = &job.verification {
                        ui.small(verified);
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
        if !self.waiting
            && self.pending.is_none()
            && self.jobs.iter().any(|job| !job.state.terminal())
        {
            if self.last_jobs.elapsed() >= Duration::from_secs(1) {
                self.query("jobs.list", json!({}), Action::Jobs, ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .find_map(|file| file.path.clone())
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
            ui.horizontal(|ui| { ui.label("Input"); ui.add(egui::TextEdit::singleline(&mut self.source).hint_text("Absolute path granted to this session").desired_width((ui.available_width()-150.0).max(80.0))); if ui.add_enabled(!self.waiting && self.pending.is_none() && !self.source.trim().is_empty(),egui::Button::new("Add media")).clicked() { self.mutate("assets.register",json!({"path":self.source}),Action::Register,ui.ctx()); } });
            if let Some(asset) = self.selected_asset() { self.cleanup(ui,&asset); }
            else { ui.add_space(30.0); ui.label("Drop a native file here or enter an authorized input path."); }
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
