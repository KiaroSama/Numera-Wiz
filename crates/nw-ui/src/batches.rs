//! Shared native/browser batch controls; the owner owns all plans and jobs.
use crate::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    batches::{
        BatchPlanReview, BatchResult, BatchSummary, CleanBatchInput, CleanFolderBatchInput,
        ExtractBatchInput, ExtractBatchItem, ExtractKind, ExtractSelector, FailurePolicy,
        FolderBatchExclusion, FolderBatchReview, ItemInput, UnresolvedPolicy,
    },
    ingest::FolderReview,
    views::{AssetSummary, SnapshotPage},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use web_time::Instant;

fn deadline_label(policy: nw_domain::execution::DeadlinePolicy) -> String {
    match policy {
        nw_domain::execution::DeadlinePolicy::Finite { seconds } => format!("{seconds} s"),
        nw_domain::execution::DeadlinePolicy::Disabled => "Disabled".into(),
    }
}
fn action_label(action: &Option<nw_domain::mux::CleanAction>) -> &'static str {
    match action {
        Some(nw_domain::mux::CleanAction::CopyUnchanged) => "Copy unchanged",
        Some(nw_domain::mux::CleanAction::Remux) => "Remux",
        Some(nw_domain::mux::CleanAction::Skip) => "Skip",
        None => "Rejected",
    }
}
fn state_label(state: &Option<nw_domain::records::JobState>) -> &'static str {
    use nw_domain::records::JobState;
    match state {
        Some(JobState::Queued) => "Queued",
        Some(JobState::Running) => "Running",
        Some(JobState::Verifying) => "Verifying",
        Some(JobState::Publishing) => "Publishing",
        Some(JobState::Succeeded) => "Succeeded",
        Some(JobState::Failed) => "Failed",
        Some(JobState::Cancelled) => "Cancelled",
        Some(JobState::Interrupted) => "Interrupted",
        Some(JobState::Skipped) => "Skipped",
        None => "Rejected during review",
    }
}

fn plan_can_submit(plan: &BatchPlanReview) -> bool {
    (plan.failure_policy == FailurePolicy::ContinueWithRejected
        || plan.items.iter().all(|item| item.error.is_none()))
        && (plan.unresolved_policy == UnresolvedPolicy::PerFileNoMatch
            || plan.unresolved.is_empty())
}

struct FolderBatchMode {
    snapshot: Arc<FolderReview>,
    requested_output_root: String,
    excluded: Vec<FolderBatchExclusion>,
    encode: bool,
}

pub(super) struct Candidate {
    asset_id: String,
    name: String,
    output: String,
    needs_output: bool,
    extract: ExtractSelector,
}
pub(super) struct BatchUi {
    pub open: bool,
    pub candidates: Vec<Candidate>,
    generation: u64,
    extract: bool,
    failure: FailurePolicy,
    unresolved: UnresolvedPolicy,
    plan: Option<Arc<BatchPlanReview>>,
    folder: Option<FolderBatchMode>,
    packet_split: bool,
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
            extract: false,
            failure: FailurePolicy::RejectAll,
            unresolved: UnresolvedPolicy::Reject,
            plan: None,
            folder: None,
            packet_split: false,
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
        if self.folder.is_none() {
            self.plan = None;
        }
    }
    pub(super) fn is_folder_review(&self) -> bool {
        self.folder.is_some()
    }
    pub(super) fn contains(&self, id: &str) -> bool {
        self.candidates.iter().any(|item| item.asset_id == id)
    }
}
impl WizApp {
    pub(super) fn batch_select(&mut self, asset: &AssetSummary, included: bool) {
        if self.projects.active || self.pending.is_some() || self.batch.is_folder_review() {
            return;
        }
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
                extract: ExtractSelector {
                    kind: ExtractKind::Audio,
                    ordinal: 1,
                },
            });
        } else {
            self.batch
                .candidates
                .retain(|item| item.asset_id != asset.id);
        }
        self.batch.invalidate();
    }
    pub(super) fn schedule_batch(&mut self, ctx: &egui::Context) {
        if !self.cleanup_actions_allowed(ctx) || self.waiting || self.pending.is_some() {
            return;
        }
        if !self.batch.is_folder_review()
            && !self.batch.packet_split
            && let Some(index) = self
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
        if self.cleanup_actions_allowed(ctx)
            && !self.folder.open
            && !self.waiting
            && self.pending.is_none()
            && let Some(plan) = self.batch.plan.clone().filter(|plan| plan_can_submit(plan))
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
        if !self.cleanup_actions_allowed(ctx) || self.folder.open || self.batch.is_folder_review() {
            return;
        }
        if self.batch.extract {
            let input = ExtractBatchInput {
                items: self
                    .batch
                    .candidates
                    .iter()
                    .map(|item| ExtractBatchItem {
                        target: ItemInput {
                            asset_id: item.asset_id.clone(),
                            output: item.output.clone(),
                        },
                        selector: item.extract,
                    })
                    .collect(),
                failure_policy: self.batch.failure,
            };
            match input.validate() {
                Ok(()) => self.query(
                    "streams.extract.batch_plan",
                    json!(input),
                    Action::BatchPlan(self.batch.generation),
                    ctx,
                ),
                Err(error) => self.error = Some(format!("{}: {}", error.code, error.message)),
            }
            return;
        }
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
    pub(super) fn review_folder_batch(
        &mut self,
        snapshot: &FolderReview,
        output_root: &str,
        ctx: &egui::Context,
    ) {
        if !self.cleanup_actions_allowed(ctx) {
            return;
        }
        let input = self.normalized_intent().and_then(|intent| {
            let input = CleanFolderBatchInput {
                snapshot_id: snapshot.id.clone(),
                snapshot_hash: snapshot.hash.clone(),
                output_root: output_root.into(),
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
                "streams.clean.folder_batch_plan",
                json!(input),
                Action::FolderBatchPlan(self.batch.generation),
                ctx,
            ),
            Err(error) => self.error = Some(error),
        }
    }
    pub(super) fn review_folder_encoding(&mut self, settings: nw_domain::avc_export::FolderEncodeInput, ctx: &egui::Context) {
        if self.cleanup_actions_allowed(ctx) {
            self.query("media.encode_folder_plan", json!(settings), Action::FolderBatchPlan(self.batch.generation), ctx);
        }
    }
    pub(super) fn open_folder_batch(
        &mut self,
        generation: u64,
        snapshot: Arc<FolderReview>,
        requested_output_root: String,
        review: FolderBatchReview,
        encode: bool,
    ) {
        if generation != self.batch.generation {
            self.status =
                "Cleanup controls changed while planning. Review the folder again.".into();
            return;
        }
        self.batch.packet_split = false;
        self.batch.folder = Some(FolderBatchMode {
            snapshot,
            requested_output_root,
            excluded: review.excluded,
            encode,
        });
        self.batch.plan = Some(Arc::new(review.plan));
        self.batch.open = true;
        self.close_folder();
        self.status =
            "Folder batch reviewed. Inspect the destinations and exclusions before running.".into();
    }

    pub(super) fn invalidate_packet_split(&mut self) {
        if self.batch.packet_split {
            self.batch.invalidate();
        }
    }

    pub(super) fn review_packet_split(
        &mut self,
        input: nw_domain::packet_cut::SplitInput,
        ctx: &egui::Context,
    ) {
        if self.cleanup_actions_allowed(ctx) && !self.folder.open {
            self.batch.folder = None;
            self.batch.invalidate();
            self.query(
                "media.split_plan",
                json!(input),
                Action::BatchPlan(self.batch.generation),
                ctx,
            );
        }
    }

    pub(super) fn batch_policy_controls(&mut self, ui: &mut egui::Ui, include_unresolved: bool) {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Rejected candidates");
            changed |= ui
                .selectable_value(
                    &mut self.batch.failure,
                    FailurePolicy::RejectAll,
                    "Refuse the batch",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut self.batch.failure,
                    FailurePolicy::ContinueWithRejected,
                    "Keep rejected rows and run accepted files",
                )
                .changed();
        });
        if include_unresolved {
            ui.horizontal(|ui| {
                ui.label("Indices absent from every eligible stream");
                changed |= ui
                    .selectable_value(
                        &mut self.batch.unresolved,
                        UnresolvedPolicy::Reject,
                        "Refuse submission",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut self.batch.unresolved,
                        UnresolvedPolicy::PerFileNoMatch,
                        "Apply each file's explicit no-match policy",
                    )
                    .changed();
            });
        }
        if changed {
            self.batch.invalidate();
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
                    self.batch.packet_split =
                        plan.items.iter().any(|item| item.packet_cut.is_some());
                    self.batch.open |= self.batch.packet_split;
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
    fn explicit_batch_controls(&mut self, ui: &mut egui::Ui, idle: bool) {
        let mut changed = false;
        ui.horizontal(|ui| {
            if self.batch.extract {
                ui.label("Select each track by its one-based ordinal and give it a distinct .mkv path.");
            } else {
                ui.label("Select files with the media checkboxes. The current stream selection, file-time policy and execution limits apply to every candidate.");
            }
            changed |= ui.add_enabled(idle, egui::Checkbox::new(&mut self.batch.extract, "Extract streams")).changed();
        });
        if !self.batch.extract {
            let times = match self.file_times {
                nw_domain::file_times::FileTimeRequest::New => "New timestamps",
                nw_domain::file_times::FileTimeRequest::PreserveBestEffort => {
                    "Preserve timestamps (best effort)"
                }
                nw_domain::file_times::FileTimeRequest::RequireReviewed => {
                    "Require reviewed timestamps"
                }
            };
            ui.label(format!(
                "File times: {times} | Probe limit: {} | Operation limit: {}",
                deadline_label(self.execution_limits.probe),
                deadline_label(self.execution_limits.operation)
            ));
        }
        let mut add = None;
        let mut remove = None;
        let more_outputs = self.batch.candidates.len() < nw_domain::batches::MAX_BATCH_ITEMS;
        ui.add_enabled_ui(idle, |ui| {
            self.batch_policy_controls(ui, !self.batch.extract);
            egui::ScrollArea::vertical()
                .id_salt("batch-candidates")
                .max_height(150.0)
                .show(ui, |ui| {
                    for (index, item) in self.batch.candidates.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            if self.batch.extract {
                                ui.horizontal(|ui| {
                                    let selected = item.extract.kind;
                                    egui::ComboBox::from_id_salt("extract-kind")
                                        .selected_text(format!("{selected:?}"))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut item.extract.kind,
                                                ExtractKind::Video,
                                                "Video",
                                            );
                                            ui.selectable_value(
                                                &mut item.extract.kind,
                                                ExtractKind::Audio,
                                                "Audio",
                                            );
                                            ui.selectable_value(
                                                &mut item.extract.kind,
                                                ExtractKind::Subtitle,
                                                "Subtitle",
                                            );
                                        });
                                    changed |= selected != item.extract.kind;
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut item.extract.ordinal)
                                                .range(1..=256)
                                                .prefix("Track "),
                                        )
                                        .changed();
                                    if ui
                                        .add_enabled(
                                            more_outputs,
                                            egui::Button::new("Add output for this file"),
                                        )
                                        .clicked()
                                    {
                                        add = Some(Candidate {
                                            asset_id: item.asset_id.clone(),
                                            name: item.name.clone(),
                                            output: String::new(),
                                            needs_output: false,
                                            extract: item.extract,
                                        });
                                    }
                                    if ui.button("Remove output").clicked() {
                                        remove = Some(index);
                                    }
                                });
                            }
                            ui.horizontal(|ui| {
                                ui.label(format!("{}. {}", index + 1, item.name));
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(&mut item.output)
                                            .char_limit(16384)
                                            .desired_width(620.0)
                                            .hint_text("Reviewed engine output path"),
                                    )
                                    .changed();
                            });
                        });
                    }
                });
        });
        if let Some(item) = add {
            self.batch.candidates.push(item);
            changed = true;
        }
        if let Some(index) = remove {
            self.batch.candidates.remove(index);
            changed = true;
        }
        if changed {
            self.batch.invalidate();
        }
    }

    pub(super) fn batch_window(&mut self, ctx: &egui::Context) {
        if self.projects.active || !self.batch.open {
            return;
        }
        let mut open = true;
        let folder_mode = self.batch.is_folder_review();
        let folder_encode = self.batch.folder.as_ref().is_some_and(|folder| folder.encode);
        let split_mode = self.batch.packet_split;
        let title = if folder_encode {
            "Folder H.264 export"
        } else if folder_mode {
            "Folder cleanup batch"
        } else if split_mode {
            "Packet split"
        } else if self.batch.extract {
            "Stream extraction batch"
        } else {
            "Cleanup batch"
        };
        egui::Window::new(title).id(egui::Id::unique("Cleanup batch"))
            .default_pos(egui::pos2(24.0,160.0)).default_width(1040.0)
            .default_height(540.0).open(&mut open).show(ctx, |ui| {
                let idle = !self.waiting && self.pending.is_none();
                ui.horizontal(|ui| {
                    if !folder_mode && !split_mode && ui.add_enabled(idle && !self.batch.candidates.is_empty()
                        && self.batch.candidates.iter().all(|item| !item.output.is_empty() && !item.needs_output),
                        egui::Button::new("Review batch")).clicked() { self.review_batch(ui.ctx()); }
                    let eligible = self.batch.plan.as_deref().is_some_and(plan_can_submit);
                    if ui.add_enabled(idle && eligible,egui::Button::new("Run reviewed batch")).clicked() { self.run_reviewed_batch(ui.ctx()); }
                    if ui.add_enabled(idle,egui::Button::new("Refresh batch history")).clicked() {
                        self.query("batches.list",json!({"replace_snapshot":self.batch.snapshot}),Action::BatchHistory,ui.ctx());
                    }
                    if folder_mode || split_mode {
                        if ui.add_enabled(idle, egui::Button::new("Return to explicit candidates")).clicked() {
                            self.batch.folder = None;
                            self.batch.packet_split = false;
                            self.batch.invalidate();
                        }
                    } else {
                        ui.label(format!("{} explicit candidates",self.batch.candidates.len()));
                    }
                });
                if let Some(folder) = &self.batch.folder {
                    ui.label(format!("Retained folder snapshot {}", folder.snapshot.id));
                    ui.monospace(&folder.snapshot.hash);
                    ui.label(format!("Source directory: {}", folder.snapshot.root));
                    ui.label(format!("Requested output directory: {}", folder.requested_output_root));
                    ui.collapsing(format!("{} excluded snapshot rows", folder.excluded.len()), |ui| {
                        egui::ScrollArea::vertical().id_salt("folder-batch-exclusions").max_height(120.0).show(ui, |ui| {
                            for exclusion in &folder.excluded {
                                if let Some(entry) = folder.snapshot.entries.get(exclusion.index as usize) {
                                    ui.label(format!(
                                        "{} · {}",
                                        entry.relative_path,
                                        crate::folders::outcome_label(exclusion.outcome),
                                    ));
                                }
                            }
                        });
                    });
                } else if !split_mode {
                    self.explicit_batch_controls(ui, idle);
                }
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
                                let action = if folder_encode && item.plan.is_some() {
                                    "Encode H.264"
                                } else {
                                    action_label(&item.action)
                                };
                                ui.label(format!("{}. {} | {} | {} bytes",item.index+1,item.output_name,action,item.source_bytes));
                                if let Some(reference) = &item.plan
                                    && ui.add_enabled(idle,egui::Button::new("Plan details")).clicked() {
                                    self.query("plans.inspect",json!({"id":reference.id}),Action::Inspect,ui.ctx());
                                }
                            });
                            if (folder_mode || split_mode) && let Some(output) = &item.output_path {
                                ui.monospace(output);
                            }
                            if let Some(range) = item.packet_cut {
                                ui.label(format!("Original time {}/{} to {}/{} seconds (end exclusive)",
                                    range.start.numerator(), range.start.denominator(), range.end.numerator(), range.end.denominator()));
                            }
                            if let Some(extraction) = item.extraction {
                                ui.label(format!(
                                    "{:?} track {}{}",
                                    extraction.selector.kind,
                                    extraction.selector.ordinal,
                                    extraction.source_index.map_or_else(String::new, |index| format!(" · original stream {index}")),
                                ));
                            }
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
                                ui.label(format!("{}. {} | {}",item.index+1,item.output_name,state_label(&item.state)));
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
