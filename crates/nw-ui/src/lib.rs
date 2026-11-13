//! Shared human media workspace; all authoritative behavior is requested from the owner.
mod batches;
mod cleanup;
pub mod client;
mod folders;
mod jobs;
mod projects;
pub use client::{CommandClient, Reply, SaveReply};
use eframe::egui;
use nw_domain::{
    commands::{ProjectRef, Request, Response},
    mux::{CleanIntent, NoMatch, Preservation, Selection, Selector},
    records::Asset,
    views::{AssetSummary, PlanReview, SnapshotPage},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use web_time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Project(projects::ProjectAction),
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
    ArtifactManifest,
    SelectionReport(u64),
    BatchOutput(u64, usize),
    BatchPlan(u64),
    BatchSubmit,
    BatchInspect,
    BatchCancel,
    BatchHistory,
    FolderRegister,
    FolderInspect,
    FolderOpenAsset,
    FolderBatchAsset,
}
struct Pending {
    request: Request,
    action: Action,
}
struct DeferredIntent {
    request: Request,
    action: Action,
}
struct Received {
    request_id: Option<String>,
    action: Action,
    response: Result<Response, String>,
}
fn valid_scope(result: &Value) -> Option<String> {
    let token = result["replay_scope"].as_str()?;
    if token.len() > 2048 {
        return None;
    }
    let (payload, signature) = token.split_once('.')?;
    let encoded = |part: &str| {
        let alphabet = part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte));
        let tail = part.as_bytes().last().copied().unwrap_or_default();
        alphabet
            && match part.len() % 4 {
                0 => true,
                2 => b"AQgw".contains(&tail),
                3 => b"AEIMQUYcgkosw048".contains(&tail),
                _ => false,
            }
    };
    if payload.len() < 2 || signature.len() != 43 || !encoded(payload) || !encoded(signature) {
        return None;
    }
    // The signed token remains opaque. Only the owner verifies authority/signature.
    Some(token.into())
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
    deferred: Option<DeferredIntent>,
    assets: Vec<AssetSummary>,
    asset_detail: Option<Arc<Asset>>,
    assets_cursor: Option<String>,
    assets_snapshot: Option<String>,
    selected: Option<String>,
    source: String,
    output: String,
    output_needed: bool,
    intent: CleanIntent,
    file_times: nw_domain::file_times::FileTimeRequest,
    execution_limits: nw_domain::execution::CleanupLimits,
    patches: Vec<cleanup::PatchForm>,
    audio_mode: usize,
    subtitle_mode: usize,
    audio_values: String,
    subtitle_values: String,
    audio_order: String,
    subtitle_order: String,
    generation: u64,
    plan: Option<Arc<PlanReview>>,
    jobs: Vec<jobs::JobRow>,
    jobs_cursor: Option<String>,
    jobs_snapshot: Option<String>,
    last_jobs: Instant,
    stream_page: usize,
    inspect: String,
    artifact_manifest: Option<nw_domain::artifacts::ArtifactManifest>,
    save_control: Option<Arc<nw_domain::download::SaveControl>>,
    save_inbox: Arc<Mutex<Option<Result<nw_domain::download::SaveReceipt, String>>>>,
    save_status: String,
    #[cfg(target_arch = "wasm32")]
    focus_artifact_save: bool,
    #[cfg(not(target_arch = "wasm32"))]
    save_destination: String,
    selection_report: Option<Arc<nw_domain::reports::SelectionReport>>,
    report_by_index: bool,
    report_page: usize,
    batch: batches::BatchUi,
    folder: folders::FolderUi,
    projects: projects::ProjectsUi,
}
impl WizApp {
    fn intake_allowed(&self, ctx: &egui::Context) -> bool {
        self.cleanup_actions_allowed(ctx)
            && !self.folder.open
            && !self.batch.open
            && self.artifact_manifest.is_none()
            && self.selection_report.is_none()
    }
    fn choose_media(&mut self, ctx: &egui::Context) {
        if !self.intake_allowed(ctx) {
            return;
        }
        self.waiting = true;
        self.error = None;
        self.status = "Selecting media…".into();
        let inbox = self.inbox.clone();
        let repaint = ctx.clone();
        self.client.choose_media(Box::new(move |response| {
            if let Ok(mut slot) = inbox.lock() {
                *slot = Some(Received {
                    request_id: None,
                    action: Action::Intake,
                    response,
                });
            }
            repaint.request_repaint();
        }));
    }
    fn save_artifact(
        &mut self,
        manifest: nw_domain::artifacts::ArtifactManifest,
        ctx: &egui::Context,
    ) {
        if self.projects.active || !self.request_idle() || self.save_control.is_some() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let destination =
            (!self.save_destination.trim().is_empty()).then(|| self.save_destination.clone());
        #[cfg(target_arch = "wasm32")]
        let destination = None;
        let control = Arc::new(nw_domain::download::SaveControl::default());
        self.save_control = Some(control.clone());
        self.save_status = "Selecting a client destination and verifying its artifact copy…".into();
        let inbox = self.save_inbox.clone();
        let repaint = ctx.clone();
        self.client.save_artifact(
            manifest,
            destination,
            control,
            Box::new(move |result| {
                if let Ok(mut slot) = inbox.lock() {
                    *slot = Some(result);
                }
                repaint.request_repaint();
            }),
        );
    }
    fn save_controls(&mut self, ui: &mut egui::Ui) {
        if let Some(control) = &self.save_control {
            ui.horizontal(|ui| {
                ui.label(format!("Client copy: {} bytes", control.copied()));
                if control.is_cancelling() {
                    ui.label("Cancelling; waiting for active writes and cleanup…");
                } else if ui.button("Cancel client copy").clicked() {
                    self.save_status = if control.cancel() {
                        "Cancellation requested; waiting for active writes and cleanup.".into()
                    } else {
                        "Client publication is already underway; waiting for its result.".into()
                    };
                }
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !self.save_status.is_empty() {
            ui.label(&self.save_status);
        }
    }
    fn run_reviewed(&mut self, ctx: &egui::Context) {
        if self.cleanup_actions_allowed(ctx)
            && !self.folder.open
            && !self.waiting
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
            file_times: nw_domain::file_times::FileTimeRequest::default(),
            execution_limits: nw_domain::execution::CleanupLimits::default(),
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
            artifact_manifest: None,
            save_control: None,
            save_inbox: Arc::new(Mutex::new(None)),
            save_status: String::new(),
            #[cfg(target_arch = "wasm32")]
            focus_artifact_save: false,
            #[cfg(not(target_arch = "wasm32"))]
            save_destination: String::new(),
            selection_report: None,
            report_by_index: false,
            report_page: 0,
            batch: batches::BatchUi::default(),
            folder: folders::FolderUi::default(),
            projects: projects::ProjectsUi::default(),
        };
        app.query("assets.list", json!({}), Action::Assets, ctx);
        app
    }
    fn selected_asset(&self) -> Option<Arc<Asset>> {
        self.asset_detail
            .clone()
            .filter(|asset| self.selected.as_ref() == Some(&asset.id))
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
        if self.cleanup_actions_allowed(ctx)
            && self.output_needed
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
        self.selection_report = None;
        self.batch.invalidate();
    }
    fn refresh_assets(&mut self, ctx: &egui::Context) {
        self.query(
            "assets.list",
            json!({"replace_snapshot":self.assets_snapshot}),
            Action::Assets,
            ctx,
        );
    }
    fn refresh_jobs(&mut self, ctx: &egui::Context) {
        self.query(
            "jobs.list",
            json!({"replace_snapshot":self.jobs_snapshot}),
            Action::Jobs,
            ctx,
        );
    }
    fn dispatch(&mut self, request: Request, action: Action, ctx: &egui::Context) {
        if self.waiting {
            return;
        }
        if let Some(pending) = &self.pending
            && (pending.action != action
                || serde_json::to_vec(&pending.request).ok() != serde_json::to_vec(&request).ok())
        {
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
        let request_id = request.request_id.clone();
        self.client.send(
            request,
            Box::new(move |response| {
                if let Ok(mut slot) = inbox.lock() {
                    *slot = Some(Received {
                        request_id: Some(request_id),
                        action,
                        response,
                    });
                }
                repaint.request_repaint();
            }),
        );
    }
    fn query(&mut self, id: &str, params: Value, action: Action, ctx: &egui::Context) {
        self.query_with_project(id, params, None, action, ctx);
    }
    fn request_idle(&self) -> bool {
        !self.waiting && self.pending.is_none() && self.deferred.is_none()
    }
    fn workspace_switch_allowed(&self) -> bool {
        self.request_idle()
            && !self.folder.open
            && !self.batch.open
            && self.artifact_manifest.is_none()
            && self.selection_report.is_none()
            && self.save_control.is_none()
    }
    fn cleanup_actions_allowed(&self, ctx: &egui::Context) -> bool {
        !self.projects.active && self.request_idle() && !ctx.text_edit_focused()
    }
    fn action_allowed(&self, action: Action, ctx: &egui::Context) -> bool {
        match action {
            Action::Project(_) => self.projects.active,
            Action::Scope => self.deferred.is_some(),
            _ if self.projects.active => false,
            Action::Plan(_)
            | Action::Submit
            | Action::Output(_)
            | Action::SelectionReport(_)
            | Action::BatchOutput(_, _)
            | Action::BatchPlan(_)
            | Action::BatchSubmit
            | Action::Register
            | Action::Intake
            | Action::FolderRegister => !ctx.text_edit_focused(),
            _ => true,
        }
    }
    fn new_request(
        &mut self,
        id: &str,
        params: Value,
        project: Option<ProjectRef>,
    ) -> Option<Request> {
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.error = Some("Request identity exhausted; reopen the application.".into());
            return None;
        };
        self.sequence = sequence;
        let mut request = Request::query(id, params);
        request.request_id = format!("{}-{}", self.session, sequence);
        request.project = project;
        if let Err(error) = request.validate() {
            self.error = Some(format!("{}: {}", error.code, error.message));
            return None;
        }
        Some(request)
    }
    fn query_with_project(
        &mut self,
        id: &str,
        params: Value,
        project: Option<ProjectRef>,
        action: Action,
        ctx: &egui::Context,
    ) {
        if self.waiting
            || self.pending.is_some()
            || !self.action_allowed(action, ctx)
            || (self.deferred.is_some() && !matches!(action, Action::Scope))
        {
            return;
        }
        if let Some(request) = self.new_request(id, params, project) {
            self.dispatch(request, action, ctx);
        }
    }
    fn mutate(&mut self, id: &str, params: Value, action: Action, ctx: &egui::Context) {
        self.mutate_with_project(id, params, None, action, ctx);
    }
    fn mutate_with_project(
        &mut self,
        id: &str,
        params: Value,
        project: Option<ProjectRef>,
        action: Action,
        ctx: &egui::Context,
    ) -> bool {
        if !self.request_idle() || !self.action_allowed(action, ctx) {
            return false;
        }
        let Some(mut request) = self.new_request(id, params, project) else {
            return false;
        };
        // Allocate the complete intent identity before scope bootstrap; no continuation
        // rereads a selected project, form, expected revision or idempotency key.
        request.idempotency_key = Some(request.request_id.clone());
        let Some(scope) = self.scope.clone() else {
            self.deferred = Some(DeferredIntent { request, action });
            self.query("access.replay_scope.create", json!({}), Action::Scope, ctx);
            if self.pending.is_none() {
                self.deferred = None;
                return false;
            }
            return true;
        };
        request.replay_scope = Some(scope);
        self.dispatch(request, action, ctx);
        true
    }
    fn retry_pending(&mut self, ctx: &egui::Context) {
        if !self.waiting
            && let Some(pending) = &self.pending
        {
            let request = pending.request.clone();
            let action = pending.action;
            self.dispatch(request, action, ctx);
        }
    }
    fn receive(&mut self, ctx: &egui::Context) {
        let received = self.inbox.lock().ok().and_then(|mut slot| slot.take());
        let Some(received) = received else {
            return;
        };
        if let Some(id) = &received.request_id {
            if !self.pending.as_ref().is_some_and(|pending| {
                pending.request.request_id == *id && pending.action == received.action
            }) {
                return;
            }
            if received
                .response
                .as_ref()
                .is_ok_and(|response| response.request_id != *id)
            {
                self.waiting = false;
                self.error = Some(
                    "Owner response correlation changed. Retry retains the captured request."
                        .into(),
                );
                return;
            }
        } else if !matches!(received.action, Action::Intake)
            || self.pending.is_some()
            || self.projects.active
        {
            return;
        }
        self.waiting = false;
        let response = match received.response {
            Ok(response) => response,
            Err(error) => {
                self.error = Some(error);
                self.folder_request_failed(received.action);
                self.status = if matches!(received.action, Action::Intake) {
                    "File intake did not finish. Select the file to try again.".into()
                } else {
                    "Connection interrupted. Retry retains the original request identity.".into()
                };
                return;
            }
        };
        if !response.ok && response.error.is_none() {
            self.error =
                Some("Invalid owner error acknowledgement; retry the captured request.".into());
            return;
        }
        if !response.ok {
            let completed = self.pending.take();
            if let (Action::Project(action), Some(error), Some(completed)) =
                (received.action, response.error.as_ref(), completed.as_ref())
            {
                match self.recover_project_selection(action, &completed.request, error, ctx) {
                    Ok(true) => {
                        self.deferred = None;
                        return;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        self.project_request_failed(received.action);
                        self.deferred = None;
                        self.error = Some(format!("Project selection recovery failed: {error}"));
                        return;
                    }
                }
            }
            self.project_request_failed(received.action);
            if let Some(error) = response.error {
                if error.code == "SNAPSHOT_EXPIRED" {
                    match received.action {
                        Action::Assets => {
                            self.assets.clear();
                            self.assets_cursor = None;
                            self.assets_snapshot = None;
                        }
                        Action::BatchHistory => {
                            self.batch_history_expired();
                        }
                        Action::Jobs => {
                            self.jobs.clear();
                            self.jobs_cursor = None;
                            self.jobs_snapshot = None;
                        }
                        _ => {}
                    }
                    self.status =
                        "Inventory snapshot expired. Refresh to load a coherent page.".into();
                }
                if error.code == "REPLAY_SCOPE_EXPIRED" || error.code == "REPLAY_SCOPE_INVALID" {
                    self.scope = None;
                }
                self.error = Some(format!("{}: {}", error.code, error.message));
            }
            self.deferred = None;
            self.folder_request_failed(received.action);
            return;
        }
        if matches!(received.action, Action::Scope) {
            let token = response.result.as_ref().and_then(valid_scope);
            let Some(token) = token.filter(|_| response.receipt_id.is_none()) else {
                self.error=Some("Invalid owner replay scope. The captured intent is retained; retry the same bootstrap request.".into());
                return;
            };
            self.scope = Some(token.clone());
            self.pending = None;
            if let Some(mut intent) = self.deferred.take() {
                intent.request.replay_scope = Some(token);
                self.dispatch(intent.request, intent.action, ctx);
            } else {
                self.error = Some(
                    "Scope arrived without a captured intent; no mutation was dispatched.".into(),
                );
            }
            return;
        }
        let completed = self.pending.take();
        let receipt = response.receipt_id.clone();
        let result = response.result.unwrap_or(Value::Null);
        let parsed: Result<(), String> = (|| {
            match received.action {
                Action::Project(action) => {
                    let request = &completed
                        .as_ref()
                        .ok_or("Project response has no captured request")?
                        .request;
                    self.receive_project(action, result, request, receipt.as_deref(), ctx)?;
                }
                Action::Assets => {
                    let page: SnapshotPage<AssetSummary> =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.assets = page.items;
                    self.assets_cursor = page.next_cursor;
                    self.assets_snapshot = Some(page.snapshot_id);
                }
                Action::Asset => {
                    let asset: Asset =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
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
                    if self.assets.len() < 128 {
                        self.assets.push(AssetSummary::from(&asset));
                    }
                    self.status = "Media registered. Review stream selection and output.".into();
                }
                Action::Scope => {
                    return Err("Scope continuation was not validated".into());
                }
                Action::Plan(generation) => {
                    let plan: PlanReview =
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
                    let page: SnapshotPage<Value> =
                        serde_json::from_value(result).map_err(|error| error.to_string())?;
                    self.jobs = page
                        .items
                        .into_iter()
                        .map(jobs::JobRow::decode)
                        .collect::<Result<_, _>>()?;
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
                Action::ArtifactManifest => {
                    #[cfg(target_arch = "wasm32")]
                    {
                        self.focus_artifact_save = true;
                    }
                    self.artifact_manifest =
                        Some(serde_json::from_value(result).map_err(|error| error.to_string())?);
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.save_destination.clear();
                        ctx.memory_mut(|memory| {
                            memory.request_focus(egui::Id::new("artifact-save-destination"))
                        });
                    }
                }
                Action::SelectionReport(generation) => {
                    if generation == self.generation {
                        self.selection_report = Some(Arc::new(
                            serde_json::from_value(result).map_err(|error| error.to_string())?,
                        ));
                        self.report_page = 0;
                    }
                }
                Action::BatchOutput(_, _)
                | Action::BatchPlan(_)
                | Action::BatchSubmit
                | Action::BatchInspect
                | Action::BatchCancel
                | Action::BatchHistory => {
                    self.receive_batch(received.action, result)?;
                }
                Action::FolderRegister
                | Action::FolderInspect
                | Action::FolderOpenAsset
                | Action::FolderBatchAsset => {
                    self.receive_folder(received.action, result, ctx)?;
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
            self.project_request_failed(received.action);
            if matches!(
                received.action,
                Action::Project(projects::ProjectAction {
                    phase: projects::Phase::Create | projects::Phase::Mutation,
                    ..
                })
            ) {
                self.pending = completed;
                self.projects.terminal_failure = false;
                self.status="Mutation acknowledgement is incomplete. Retry the exact captured intent before navigating.".into();
            }
            return;
        }
        if matches!(received.action, Action::FolderRegister) {
            self.refresh_assets(ctx);
        } else if matches!(
            received.action,
            Action::Submit | Action::Cancel | Action::Assets
        ) {
            self.refresh_jobs(ctx);
        }
    }
    fn catalog(&mut self, ui: &mut egui::Ui) {
        ui.heading("Media");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.waiting && self.pending.is_none(),
                    egui::Button::new("Refresh media"),
                )
                .clicked()
            {
                self.refresh_assets(ui.ctx());
            }
            if ui
                .button(format!("Batch ({})", self.batch.candidates.len()))
                .clicked()
            {
                self.batch.open = true;
            }
        });
        if self.assets_cursor.is_some()
            && ui
                .add_enabled(
                    !self.waiting && self.pending.is_none(),
                    egui::Button::new("Next media page"),
                )
                .clicked()
        {
            self.query(
                "assets.list",
                json!({"cursor":self.assets_cursor}),
                Action::Assets,
                ui.ctx(),
            );
        }
        ui.separator();
        let mut select = None;
        let mut batch_changes = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt("asset-catalog")
            .show(ui, |ui| {
                for asset in &self.assets {
                    let name = asset.display_name.as_str();
                    ui.horizontal(|ui| {
                        let mut included = self.batch.contains(&asset.id);
                        if ui
                            .add_enabled(
                                !self.waiting && self.pending.is_none(),
                                egui::Checkbox::without_text(&mut included),
                            )
                            .on_hover_text("Include in cleanup batch")
                            .changed()
                        {
                            batch_changes.push((asset.clone(), included));
                        }
                        if ui
                            .selectable_label(self.selected.as_ref() == Some(&asset.id), name)
                            .clicked()
                        {
                            select = Some(asset.id.clone());
                        }
                    });
                    ui.small(format!(
                        "{} streams · {} bytes",
                        asset.stream_count, asset.bytes
                    ));
                }
                if self.assets.is_empty() {
                    ui.label("Add a file to inspect its streams and prepare cleanup.");
                }
            });
        for (asset, included) in batch_changes {
            self.batch_select(&asset, included);
        }
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
            if self.jobs_cursor.is_some()
                && ui
                    .add_enabled(
                        !self.waiting && self.pending.is_none(),
                        egui::Button::new("Next jobs page"),
                    )
                    .clicked()
            {
                self.query(
                    "jobs.list",
                    json!({"cursor":self.jobs_cursor}),
                    Action::Jobs,
                    ui.ctx(),
                );
            }
        });
        let mut cancel = None;
        let mut artifact = None;
        let mut inspect_job = None;
        egui::ScrollArea::vertical()
            .id_salt("jobs")
            .max_height(145.0)
            .show(ui, |ui| {
                for job in &self.jobs {
                    ui.horizontal(|ui| {
                        ui.monospace(job.id.chars().take(8).collect::<String>());
                        ui.label(job.state.label());
                        ui.label(&job.output_name);
                        if !job.state.terminal()
                            && let Some(progress) = &job.remux_progress
                        {
                            ui.small(format!(
                                "FFmpeg time: {} us | output: {} bytes",
                                progress
                                    .reported_out_time_us
                                    .as_deref()
                                    .unwrap_or("unknown"),
                                progress.output_bytes.as_deref().unwrap_or("unknown"),
                            ));
                        }
                        if !job.state.terminal()
                            && let Some(progress) = &job.progress
                        {
                            let stage = match progress.stage {
                                nw_domain::execution::ProgressStage::CopyBytes => "Copy",
                                nw_domain::execution::ProgressStage::VerifyStagingBytes => {
                                    "Verify saved bytes"
                                }
                                nw_domain::execution::ProgressStage::VerifySourceBytes => {
                                    "Recheck source"
                                }
                            };
                            ui.small(format!(
                                "{stage}: {} / {} bytes",
                                progress.completed_bytes, progress.total_bytes
                            ));
                        }
                        if job.state.succeeded()
                            && ui
                                .add_enabled(
                                    !self.waiting && self.pending.is_none(),
                                    egui::Button::new("Output details"),
                                )
                                .clicked()
                        {
                            artifact = Some(job.artifact_id.clone());
                        }
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
                        if ui
                            .add_enabled(
                                !self.waiting && self.pending.is_none(),
                                egui::Button::new("Job details"),
                            )
                            .clicked()
                        {
                            inspect_job = Some(job.id.clone());
                        }
                        if job.warning_count > 0 {
                            ui.colored_label(
                                egui::Color32::from_rgb(237, 177, 99),
                                format!("{} warning(s)", job.warning_count),
                            );
                        }
                    });
                    if let Some(error) = &job.error_code {
                        ui.colored_label(egui::Color32::from_rgb(237, 143, 112), error);
                    }
                }
                if self.jobs.is_empty() {
                    ui.label(
                        "Accepted jobs appear here. Closing this window does not cancel them.",
                    );
                }
            });
        if let Some(id) = inspect_job {
            self.query("jobs.inspect", json!({"id":id}), Action::Inspect, ui.ctx());
        }
        if let Some(id) = artifact {
            self.query(
                "artifacts.manifest",
                json!({"artifact_id":id}),
                Action::ArtifactManifest,
                ui.ctx(),
            );
        }
        let mut open = self.artifact_manifest.is_some();
        let mut save = None;
        if !self.folder.open
            && let Some(manifest) = self.artifact_manifest.clone()
        {
            egui::Window::new("Output details").default_pos(egui::pos2(24.0, 260.0))
                .default_width(720.0).open(&mut open).show(ui.ctx(), |ui| {
                ui.label(format!("Verified bytes: {}", manifest.encoded_byte_count));
                ui.label("SHA-256"); ui.monospace(&manifest.sha256);
                ui.label(format!("Job: {}", manifest.job_id));
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.label("Client destination");
                    ui.add(egui::TextEdit::singleline(&mut self.save_destination)
                        .id(egui::Id::new("artifact-save-destination"))
                        .hint_text("Absolute new file path; leave blank for the Save As dialog")
                        .desired_width(f32::INFINITY));
                    ui.small("An existing native destination is preserved.");
                }
                let save_button = ui.add_enabled(self.save_control.is_none(), egui::Button::new("Save verified copy…"));
                #[cfg(target_arch = "wasm32")]
                if self.focus_artifact_save {
                    save_button.request_focus();
                    self.focus_artifact_save = false;
                }
                if save_button.clicked() {
                    save = Some(manifest.clone());
                }
                if ui.button("Copy manifest URI").clicked() { ui.ctx().copy_text(manifest.manifest_uri.clone()); }
                ui.small("Reading this output requires current job access. A copied URI does not grant access.");
            });
        }
        if let Some(manifest) = save {
            self.save_artifact(manifest, ui.ctx());
        }
        if !open {
            self.artifact_manifest = None;
        }
        self.save_controls(ui);
        if !self.folder.open {
            self.selection_report_window(ui.ctx());
        }
        if let Some(id) = cancel {
            self.mutate("jobs.cancel", json!({"id":id}), Action::Cancel, ui.ctx());
        }
    }
}
impl Drop for WizApp {
    fn drop(&mut self) {
        if let Some(control) = &self.save_control {
            control.cancel();
        }
    }
}
impl eframe::App for WizApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let saved = self.save_inbox.lock().ok().and_then(|mut slot| slot.take());
        if let Some(saved) = saved {
            self.save_control = None;
            self.save_status = match saved {
                Ok(receipt) => {
                    let warning = if receipt.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" Warning: {}", receipt.warnings.join(", "))
                    };
                    format!(
                        "Saved {} verified bytes to {}.{}",
                        receipt.encoded_byte_count, receipt.destination, warning
                    )
                }
                Err(error) => format!("Client copy did not finish: {error}"),
            };
        }
        self.receive(ctx);
        self.schedule_projects(ctx);
        self.schedule_output(ctx);
        self.schedule_batch(ctx);
        if !self.projects.active
            && !self.waiting
            && self.pending.is_none()
            && self.jobs.iter().any(|job| !job.state.terminal())
        {
            if self.last_jobs.elapsed() >= Duration::from_secs(1) {
                self.refresh_jobs(ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        #[cfg(target_arch = "wasm32")]
        if self.intake_allowed(ctx)
            && !self.waiting
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
                            request_id: None,
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
            && self.intake_allowed(ctx)
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
        // Consume keyboard input in the input-bearing UI pass, never in timer-only logic ticks.
        if !self.projects.active
            && !ui.ctx().text_edit_focused()
            && !self.folder.open
            && ui.ctx().input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::Enter,
                ))
            })
        {
            if self.batch.open {
                self.run_reviewed_batch(ui.ctx());
            } else {
                self.run_reviewed(ui.ctx());
            }
        }
        if self.folder.open
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.close_folder();
        } else if (self.batch.open || self.artifact_manifest.is_some())
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.batch.open {
                self.batch.open = false;
            } else {
                self.artifact_manifest = None;
            }
        }
        if self.folder.open {
            ui.disable();
        }
        egui::Panel::top("navigation").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Numera Wiz");
                ui.separator();
                ui.strong("Media tools");
                ui.separator();
                ui.label("Stream Cleanup");
                ui.separator();
                if self.projects.active {
                    ui.strong("Projects");
                    if ui
                        .add_enabled(self.request_idle(), egui::Button::new("Return to Cleanup"))
                        .clicked()
                    {
                        self.leave_projects(ui.ctx());
                    }
                } else if ui
                    .add_enabled(
                        self.workspace_switch_allowed(),
                        egui::Button::new("Projects"),
                    )
                    .clicked()
                {
                    self.enter_projects(ui.ctx());
                }
            });
        });
        if self.projects.active {
            self.project_workspace(ui);
            return;
        }
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
                self.choose_media(ui.ctx());
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
            self.folder_controls(ui);
        });
        if !self.folder.open {
            self.batch_window(ui.ctx());
        }
        self.folder_window(ui.ctx());
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
        fn save_artifact(
            &self,
            _manifest: nw_domain::artifacts::ArtifactManifest,
            _destination: Option<String>,
            _control: Arc<nw_domain::download::SaveControl>,
            _reply: SaveReply,
        ) {
            panic!("This controller test does not invoke artifact delivery");
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
            source_object: None,
            id: id.into(),
            creator: "controller-unit".into(),
            display_name: id.into(),
            identity: FileIdentity {
                path: format!("/controller-unit/{id}.mkv"),
                bytes: "0".into(),
                sha256: "00".repeat(32),
            },
            probe: MediaProbe {
                probe_timeout: None,
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
        app.assets = vec![
            AssetSummary::from(a.as_ref()),
            AssetSummary::from(b.as_ref()),
        ];
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
        crate::projects::tests::controller_cases();
        crate::jobs::exercise();
    }
}
