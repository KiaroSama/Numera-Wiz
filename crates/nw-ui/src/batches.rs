//! Shared native/browser cleanup batch controls; the owner owns all plans and jobs.
use crate::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    batches::{
        BatchPlanReview, BatchResult, BatchSummary, CleanBatchInput, FailurePolicy, ItemInput,
        UnresolvedPolicy,
    },
    views::{AssetSummary, SnapshotPage},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use web_time::Instant;

pub(super) struct Candidate {
    asset_id: String,
    name: String,
    output: String,
    needs_output: bool,
}
pub(super) struct BatchUi {
    pub open: bool,
    pub candidates: Vec<Candidate>,
    generation: u64,
    failure: FailurePolicy,
    unresolved: UnresolvedPolicy,
    plan: Option<Arc<BatchPlanReview>>,
    active: Option<String>,
    result: Option<Arc<BatchResult>>,
    history: Vec<BatchSummary>,
    cursor: Option<String>,
    snapshot: Option<String>,
    last_poll: Instant,
}
impl Default for BatchUi {
    fn default() -> Self {
        Self {
            open: false,
            candidates: Vec::new(),
            generation: 0,
            failure: FailurePolicy::RejectAll,
            unresolved: UnresolvedPolicy::Reject,
            plan: None,
            active: None,
            result: None,
            history: Vec::new(),
            cursor: None,
            snapshot: None,
            last_poll: Instant::now(),
        }
    }
}
impl BatchUi {
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.plan = None;
    }
    pub(super) fn contains(&self, id: &str) -> bool {
        self.candidates.iter().any(|item| item.asset_id == id)
    }
}
impl WizApp {
    pub(super) fn batch_select(&mut self, asset: &AssetSummary, included: bool) {
        if included {
            if self.batch.contains(&asset.id) {
                return;
            }
            if self.batch.candidates.len() >= nw_domain::batches::MAX_BATCH_ITEMS {
                self.error = Some("A batch contains at most 64 explicit candidates; no files were silently omitted.".into());
                return;
            }
            self.batch.candidates.push(Candidate {
                asset_id: asset.id.clone(),
                name: asset.display_name.clone(),
                output: String::new(),
                needs_output: true,
            });
        } else {
            self.batch
                .candidates
                .retain(|item| item.asset_id != asset.id);
        }
        self.batch.invalidate();
    }
    pub(super) fn schedule_batch(&mut self, ctx: &egui::Context) {
        if self.waiting || self.pending.is_some() {
            return;
        }
        if let Some(index) = self
            .batch
            .candidates
            .iter()
            .position(|item| item.needs_output)
        {
            let item = &mut self.batch.candidates[index];
            item.needs_output = false;
            let id = item.asset_id.clone();
            self.query(
                "assets.output_suggest",
                json!({"id":id}),
                Action::BatchOutput(self.batch.generation, index),
                ctx,
            );
            return;
        }
        if self.batch.active.is_some()
            && self
                .batch
                .result
                .as_ref()
                .is_none_or(|result| !result.complete)
        {
            if self.batch.last_poll.elapsed() >= Duration::from_secs(1) {
                self.batch.last_poll = Instant::now();
                self.query(
                    "batches.inspect",
                    json!({"id":self.batch.active}),
                    Action::BatchInspect,
                    ctx,
                );
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
    pub(super) fn batch_history_expired(&mut self) {
        self.batch.history.clear();
        self.batch.cursor = None;
        self.batch.snapshot = None;
    }
    pub(super) fn run_reviewed_batch(&mut self, ctx: &egui::Context) {
        if !self.waiting
            && self.pending.is_none()
            && let Some(plan) = self.batch.plan.clone()
        {
            self.mutate(
                "batches.submit",
                json!({"plan_id":plan.id,"plan_hash":plan.hash}),
                Action::BatchSubmit,
                ctx,
            );
        }
    }
    fn review_batch(&mut self, ctx: &egui::Context) {
        let input = self.normalized_intent().and_then(|intent| {
            let input = CleanBatchInput {
                items: self
                    .batch
                    .candidates
                    .iter()
                    .map(|item| ItemInput {
                        asset_id: item.asset_id.clone(),
                        output: item.output.clone(),
                    })
                    .collect(),
                intent,
                file_times: self.file_times,
                execution_limits: self.execution_limits.clone(),
                failure_policy: self.batch.failure,
                unresolved_policy: self.batch.unresolved,
            };
            input
                .validate()
                .map_err(|error| format!("{}: {}", error.code, error.message))?;
            Ok(input)
        });
        match input {
            Ok(input) => self.query(
                "streams.clean.batch_plan",
                json!(input),
                Action::BatchPlan(self.batch.generation),
                ctx,
            ),
            Err(error) => self.error = Some(error),
        }
    }
    pub(super) fn receive_batch(&mut self, action: Action, result: Value) -> Result<(), String> {
        match action {
            Action::BatchOutput(generation, index) => {
                if let Some(item) = self.batch.candidates.get_mut(index) {
                    if generation == self.batch.generation {
                        if item.output.is_empty() {
                            item.output = result["output"]
                                .as_str()
                                .ok_or("Batch output suggestion missing")?
                                .into();
                        }
                    } else if item.output.is_empty() {
                        item.needs_output = true;
                    }
                }
            }
            Action::BatchPlan(generation) => {
                let plan: BatchPlanReview =
                    serde_json::from_value(result).map_err(|error| error.to_string())?;
                if generation == self.batch.generation {
                    self.batch.plan = Some(Arc::new(plan));
                    self.status = "Batch reviewed. Inspect every candidate before running.".into();
                } else {
                    self.status =
                        "Batch controls changed. Review the current candidates again.".into();
                }
            }
            Action::BatchSubmit => {
                let id = result["batch_id"]
                    .as_str()
                    .ok_or("Accepted batch identity missing")?
                    .to_owned();
                self.status = format!(
                    "Batch accepted: {id}. Closing this window leaves accepted jobs running."
                );
                self.batch.active = Some(id);
                self.batch.plan = None;
                self.batch.result = None;
                self.batch.last_poll = Instant::now() - Duration::from_secs(2);
                self.last_jobs = Instant::now() - Duration::from_secs(2);
            }
            Action::BatchInspect => {
                self.batch.result = Some(Arc::new(
                    serde_json::from_value(result).map_err(|error| error.to_string())?,
                ));
                self.batch.last_poll = Instant::now();
            }
            Action::BatchCancel => {
                self.status = "Batch cancellation requests recorded. Child states confirm when workers have stopped; publishing members may complete.".into();
                self.batch.last_poll = Instant::now() - Duration::from_secs(2);
            }
            Action::BatchHistory => {
                let page: SnapshotPage<BatchSummary> =
                    serde_json::from_value(result).map_err(|error| error.to_string())?;
                self.batch.history = page.items;
                self.batch.cursor = page.next_cursor;
                self.batch.snapshot = Some(page.snapshot_id);
            }
            _ => return Err("Unexpected batch response action".into()),
        }
        Ok(())
    }
    pub(super) fn batch_window(&mut self, ctx: &egui::Context) {
        if !self.batch.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Cleanup batch")
            .default_pos(egui::pos2(24.0,160.0)).default_width(1040.0)
            .default_height(540.0).open(&mut open).show(ctx, |ui| {
                let idle = !self.waiting && self.pending.is_none();
                ui.horizontal(|ui| {
                    if ui.add_enabled(idle && !self.batch.candidates.is_empty()
                        && self.batch.candidates.iter().all(|item| !item.output.is_empty() && !item.needs_output),
                        egui::Button::new("Review batch")).clicked() { self.review_batch(ui.ctx()); }
                    let eligible = self.batch.plan.as_ref().is_some_and(|plan| {
                        (plan.failure_policy == FailurePolicy::ContinueWithRejected || plan.items.iter().all(|item| item.error.is_none()))
                        && (plan.unresolved_policy == UnresolvedPolicy::PerFileNoMatch || plan.unresolved.is_empty())
                    });
                    if ui.add_enabled(idle && eligible,egui::Button::new("Run reviewed batch")).clicked() { self.run_reviewed_batch(ui.ctx()); }
                    if ui.add_enabled(idle,egui::Button::new("Refresh batch history")).clicked() {
                        self.query("batches.list",json!({"replace_snapshot":self.batch.snapshot}),Action::BatchHistory,ui.ctx());
                    }
                    ui.label(format!("{} explicit candidates",self.batch.candidates.len()));
                });
                ui.label("Select files with the media checkboxes. The current stream selection, file-time policy and execution limits apply to every candidate.");
                ui.label(format!("File times: {:?} | Probe: {:?} | Operation: {:?}",self.file_times,self.execution_limits.probe,self.execution_limits.operation));
                let mut changed = false;
                ui.add_enabled_ui(idle,|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Rejected candidates");
                        changed |= ui.selectable_value(&mut self.batch.failure,FailurePolicy::RejectAll,"Refuse the batch").changed();
                        changed |= ui.selectable_value(&mut self.batch.failure,FailurePolicy::ContinueWithRejected,"Keep rejected rows and run accepted files").changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Indices absent from every eligible stream");
                        changed |= ui.selectable_value(&mut self.batch.unresolved,UnresolvedPolicy::Reject,"Refuse submission").changed();
                        changed |= ui.selectable_value(&mut self.batch.unresolved,UnresolvedPolicy::PerFileNoMatch,"Apply each file's explicit no-match policy").changed();
                    });
                    egui::ScrollArea::vertical().id_salt("batch-candidates").max_height(150.0).show(ui,|ui| {
                        for (index,item) in self.batch.candidates.iter_mut().enumerate() {
                            ui.push_id(index,|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}. {}",index+1,item.name));
                                    changed |= ui.add(egui::TextEdit::singleline(&mut item.output)
                                        .char_limit(16384).desired_width(620.0).hint_text("Reviewed engine output path")).changed();
                                });
                            });
                        }
                    });
                });
                if changed { self.batch.invalidate(); }
                if let Some(plan) = self.batch.plan.clone() {
                    ui.separator();
                    ui.label(format!("Reviewed proposal: {}",plan.id));
                    ui.monospace(&plan.hash);
                    if !plan.unresolved.is_empty() {
                        ui.colored_label(egui::Color32::YELLOW,format!("Unresolved original indices: {:?}",plan.unresolved));
                    }
                    egui::ScrollArea::vertical().id_salt("batch-preview").max_height(170.0).show(ui,|ui| {
                        for item in &plan.items {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}. {} | {:?} | {} bytes",item.index+1,item.output_name,item.action,item.source_bytes));
                                if let Some(reference) = &item.plan
                                    && ui.add_enabled(idle,egui::Button::new("Plan details")).clicked() {
                                    self.query("plans.inspect",json!({"id":reference.id}),Action::Inspect,ui.ctx());
                                }
                            });
                            if let Some(error) = &item.error { ui.colored_label(egui::Color32::YELLOW,format!("{}: {}",error.code,error.message.as_deref().unwrap_or("Details require current owner access"))); }
                        }
                    });
                }
                if let Some(result) = self.batch.result.clone() {
                    ui.separator();
                    ui.strong(format!("Batch {}: {}/{} finished",result.id,result.finished,result.total));
                    ui.label(format!("{} succeeded | {} skipped | {} failed | {} cancelled | {} interrupted",
                        result.succeeded,result.skipped,result.failed,result.cancelled,result.interrupted));
                    ui.label(format!("Verified successful inputs: {} bytes | outputs: {} bytes | output minus input: {} bytes",
                        result.successful_video_input_bytes,result.successful_video_output_bytes,result.successful_video_size_delta_bytes));
                    if !result.complete && ui.add_enabled(idle,egui::Button::new("Request batch cancellation")).clicked() {
                        self.mutate("batches.cancel",json!({"id":result.id}),Action::BatchCancel,ui.ctx());
                    }
                    egui::ScrollArea::vertical().id_salt("batch-result").max_height(180.0).show(ui,|ui| {
                        for item in &result.items {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}. {} | {:?}",item.index+1,item.output_name,item.state));
                                if let Some(id) = &item.job_id
                                    && ui.add_enabled(idle,egui::Button::new("Job details")).clicked() {
                                    self.query("jobs.inspect",json!({"id":id}),Action::Inspect,ui.ctx());
                                }
                                if let Some(id) = &item.artifact_id
                                    && ui.add_enabled(idle,egui::Button::new("Output details")).clicked() {
                                    self.query("artifacts.manifest",json!({"artifact_id":id}),Action::ArtifactManifest,ui.ctx());
                                }
                            });
                            if let Some(error) = &item.error { ui.colored_label(egui::Color32::YELLOW,format!("{}: {}",error.code,error.message.as_deref().unwrap_or("Inspect the authorized child job for full details"))); }
                        }
                    });
                }
                if !self.batch.history.is_empty() {
                    ui.collapsing("Accepted batch history",|ui| {
                        let mut selected = None;
                        for item in &self.batch.history {
                            if ui.add_enabled(idle,egui::Button::new(format!("{} | {} candidates",item.id,item.total))).clicked() {
                                selected = Some(item.id.clone());
                            }
                        }
                        if let Some(id) = selected {
                            self.batch.active = Some(id.clone());
                            self.batch.result = None;
                            self.query("batches.inspect",json!({"id":id}),Action::BatchInspect,ui.ctx());
                        }
                        if self.batch.cursor.is_some() && ui.add_enabled(idle,egui::Button::new("Next batch page")).clicked() {
                            self.query("batches.list",json!({"cursor":self.batch.cursor}),Action::BatchHistory,ui.ctx());
                        }
                    });
                }
                if self.waiting { ui.label("Working with the local engine…"); }
                if !self.waiting && let Some(pending) = &self.pending {
                    let request = pending.request.clone();
                    let action = pending.action;
                    if ui.button("Reconnect and retry the same request").clicked() {
                        self.dispatch(request,action,ui.ctx());
                    }
                }

                if let Some(error) = &self.error { ui.colored_label(egui::Color32::YELLOW,error); }
                if !self.inspect.is_empty() { ui.collapsing("Selected plan or job details",|ui| {
                    egui::ScrollArea::vertical().max_height(180.0).show(ui,|ui| {
                        ui.add(egui::TextEdit::multiline(&mut self.inspect).code_editor().interactive(false).desired_width(f32::INFINITY));
                    });
                }); }
            });
        self.batch.open = open;
    }
}
