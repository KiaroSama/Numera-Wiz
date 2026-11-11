//! Shared folder intake uses authorized engine paths and retained owner snapshots.
use crate::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    ingest::{
        CompanionIntakePolicy, FolderEntryKind, FolderEntryOutcome, FolderIntakeLimits, FolderReview,
        RegisterFolderInput,
    },
    records::Asset,
    views::{AssetSummary, ErrorView},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct FolderUi {
    path: String,
    recursive: bool,
    exclusions: String,
    inventory_companions: bool,
    snapshot_id: String,
    review: Option<Arc<FolderReview>>,
}
impl Default for FolderUi {
    fn default() -> Self {
        Self {
            path: String::new(),
            recursive: false,
            exclusions: String::new(),
            inventory_companions: true,
            snapshot_id: String::new(),
            review: None,
        }
    }
}

fn kind_label(kind: FolderEntryKind) -> &'static str {
    match kind {
        FolderEntryKind::Media => "Media candidate",
        FolderEntryKind::Companion => "Companion",
        FolderEntryKind::Directory => "Directory",
        FolderEntryKind::Other => "Other entry",
    }
}
fn outcome_label(outcome: FolderEntryOutcome) -> &'static str {
    match outcome {
        FolderEntryOutcome::Registered => "Registered",
        FolderEntryOutcome::ProbeFailed => "Probe failed",
        FolderEntryOutcome::CompanionInventoried => "Inventoried only",
        FolderEntryOutcome::CompanionExcluded => "Companion excluded",
        FolderEntryOutcome::Excluded => "Excluded",
        FolderEntryOutcome::Rejected => "Rejected",
    }
}
fn limits(ui: &mut egui::Ui, limits: &FolderIntakeLimits) {
    ui.small(format!(
        "Limits: {} manifest rows, {} filesystem entries, {} directories, depth {}.",
        limits.manifest_entries, limits.filesystem_entries, limits.directories, limits.depth,
    ));
    ui.small(format!(
        "Source reads: {} bytes; directory names: {} bytes; {} s before the next entry. An active probe keeps its own deadline.",
        limits.source_read_bytes, limits.directory_name_bytes, limits.seconds_before_next_entry,
    ));
}
fn error(ui: &mut egui::Ui, error: &ErrorView) {
    ui.colored_label(
        egui::Color32::YELLOW,
        format!(
            "{}: {}",
            error.code,
            error
                .message
                .as_deref()
                .unwrap_or("Diagnostic details require current owner access"),
        ),
    );
}
fn deadline_label(policy: nw_domain::execution::DeadlinePolicy) -> String {
    match policy {
        nw_domain::execution::DeadlinePolicy::Finite { seconds } => format!("{seconds} s"),
        nw_domain::execution::DeadlinePolicy::Disabled => "Disabled by owner policy".into(),
    }
}

impl WizApp {
    fn register_folder(&mut self, ctx: &egui::Context) {
        let input = RegisterFolderInput {
            path: self.folder.path.clone(),
            recursive: self.folder.recursive,
            exclude_relative: self
                .folder
                .exclusions
                .lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            companion_policy: if self.folder.inventory_companions {
                CompanionIntakePolicy::Inventory
            } else {
                CompanionIntakePolicy::Exclude
            },
            probe_timeout: Some(self.execution_limits.probe),
        };
        if let Err(error) = input.validate() {
            self.error = Some(format!("{}: {}", error.code, error.message));
            return;
        }
        self.status = "Scanning an authorized engine folder. No cleanup job has started.".into();
        self.mutate(
            "assets.register_folder",
            json!(input),
            Action::FolderRegister,
            ctx,
        );
    }

    pub(super) fn receive_folder(&mut self, action: Action, result: Value) -> Result<(), String> {
        match action {
            Action::FolderRegister | Action::FolderInspect => {
                let review: FolderReview =
                    serde_json::from_value(result).map_err(|error| error.to_string())?;
                let registered = review
                    .entries
                    .iter()
                    .filter(|entry| entry.outcome == FolderEntryOutcome::Registered)
                    .count();
                self.status = format!(
                    "Folder snapshot {} loaded: {registered} registered media; {}. Review every entry before reusing media.",
                    review.id,
                    if review.complete {
                        "enumeration complete"
                    } else {
                        "enumeration incomplete"
                    },
                );
                self.folder.snapshot_id = review.id.clone();
                self.folder.review = Some(Arc::new(review));
            }
            Action::FolderBatchAsset => {
                let asset: Asset =
                    serde_json::from_value(result).map_err(|error| error.to_string())?;
                let summary = AssetSummary::from(&asset);
                self.batch_select(&summary, true);
                self.status = if self.batch.contains(&asset.id) {
                    format!(
                        "Registered media {} selected for batch review; no new scan or cleanup job was started.",
                        asset.id,
                    )
                } else {
                    "The cleanup batch is full. This media was not included; remove a candidate before trying again.".into()
                };
            }
            _ => return Err("Unexpected folder response action".into()),
        }
        Ok(())
    }

    pub(super) fn folder_controls(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Folder intake (authorized engine path)")
            .default_open(false)
            .show(ui, |ui| {
                let height = ui.available_height().clamp(120.0, 420.0);
                egui::ScrollArea::vertical()
                    .id_salt("folder-intake-controls")
                    .max_height(height)
                    .show(ui, |ui| self.folder_body(ui));
            });
    }

    fn folder_body(&mut self, ui: &mut egui::Ui) {
        let idle = !self.waiting && self.pending.is_none();
        ui.label("Enter an absolute folder path on the computer running the local engine. The owner must explicitly grant this session access to that root.");
        ui.label("A browser file picker uploads selected file bytes; it does not grant access to a host folder or reveal its native path. Use Choose media above for file upload.");
        ui.label("This intake registers supported video candidates and records every observed failure or exclusion. Companion files are inventory only; it does not copy them or start cleanup jobs.");
        limits(ui, &FolderIntakeLimits::default());
        ui.small(format!(
            "Probe deadline for a new scan: {}.",
            deadline_label(self.execution_limits.probe),
        ));
        ui.add_enabled_ui(idle, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.folder.path)
                    .char_limit(4096)
                    .desired_width(f32::INFINITY)
                    .hint_text("Absolute owner-granted engine folder path"),
            );
            ui.checkbox(&mut self.folder.recursive, "Include subdirectories");
            ui.checkbox(
                &mut self.folder.inventory_companions,
                "Inventory companion files without copying them",
            );
            ui.label("Exclude relative paths, one per line, using / between components. Blank lines are ignored; names are kept exactly as entered.");
            ui.add(
                egui::TextEdit::multiline(&mut self.folder.exclusions)
                    .char_limit(32768)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .hint_text("Output\nSubfolder/Already processed"),
            );
        });
        if ui
            .add_enabled(
                idle && !self.folder.path.trim().is_empty(),
                egui::Button::new("Scan and register folder"),
            )
            .clicked()
        {
            self.register_folder(ui.ctx());
        }
        ui.small("The current cleanup probe deadline applies to this scan. A completed enumeration can still contain failed probes; inspect the recorded outcomes.");
        ui.separator();
        ui.label("Inspect a retained snapshot by ID to reuse its registered media without scanning again.");
        ui.add_enabled(
            idle,
            egui::TextEdit::singleline(&mut self.folder.snapshot_id)
                .char_limit(128)
                .desired_width(f32::INFINITY)
                .hint_text("Retained folder snapshot ID"),
        );
        if ui
            .add_enabled(
                idle && !self.folder.snapshot_id.trim().is_empty(),
                egui::Button::new("Inspect retained snapshot"),
            )
            .clicked()
        {
            self.query(
                "assets.folder_inspect",
                json!({"id":self.folder.snapshot_id}),
                Action::FolderInspect,
                ui.ctx(),
            );
        }
        if let Some(review) = self.folder.review.clone() {
            self.folder_review(ui, &review, idle);
        }
    }

    fn folder_review(&mut self, ui: &mut egui::Ui, review: &FolderReview, idle: bool) {
        ui.separator();
        ui.strong(if review.complete {
            "Retained snapshot: enumeration complete"
        } else {
            "Retained snapshot: incomplete; additional entries may be missing"
        });
        ui.monospace(format!("Snapshot ID: {}", review.id));
        ui.label(format!("Engine root: {}", review.root));
        ui.small(format!(
            "Captured at {} seconds since Unix epoch; {} manifest rows, {} filesystem entries and {} directories visited.",
            review.created_at, review.entries.len(), review.visited_entries, review.visited_directories,
        ));
        ui.label("Immutable retained snapshot SHA-256:");
        ui.monospace(&review.hash);
        ui.collapsing("Recorded scan settings", |ui| {
            ui.label(if review.input.recursive {
                "Subdirectories included"
            } else {
                "Top-level entries only"
            });
            ui.label(match review.input.companion_policy {
                CompanionIntakePolicy::Inventory => "Companion files inventoried without copying",
                CompanionIntakePolicy::Exclude => "Companion files excluded",
            });
            if let Some(deadline) = review.input.probe_timeout {
                ui.label(format!("Recorded probe deadline: {}", deadline_label(deadline)));
            } else {
                ui.label("Probe deadline used the engine default at registration");
            }
            if review.input.exclude_relative.is_empty() {
                ui.label("No explicit relative exclusions");
            } else {
                for path in &review.input.exclude_relative {
                    ui.monospace(path);
                }
            }
        });
        limits(ui, &review.limits);
        if let Some(scan_error) = &review.scan_error {
            error(ui, scan_error);
        }
        if review.entries.is_empty() {
            ui.label("No entries were retained. Check completeness and any scan error above.");
        }
        egui::ScrollArea::vertical()
            .id_salt("folder-snapshot-entries")
            .max_height(240.0)
            .show(ui, |ui| {
                for entry in &review.entries {
                    ui.push_id(entry.index, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!("{}. {}", entry.index + 1, entry.relative_path));
                            ui.label(kind_label(entry.kind));
                            ui.label(outcome_label(entry.outcome));
                        });
                        if let Some(entry_error) = &entry.error {
                            error(ui, entry_error);
                        }
                        if let Some(identity) = &entry.identity {
                            ui.small(format!("{} bytes; source SHA-256:", identity.bytes));
                            ui.monospace(&identity.sha256);
                        }
                        if let Some(id) = &entry.asset_id {
                            ui.monospace(format!("Registered asset ID: {id}"));
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(idle, egui::Button::new("Open registered media"))
                                    .clicked()
                                {
                                    self.query(
                                        "assets.inspect",
                                        json!({"id":id}),
                                        Action::Asset,
                                        ui.ctx(),
                                    );
                                }
                                if ui
                                    .add_enabled(
                                        idle && !self.batch.contains(id),
                                        egui::Button::new("Include in cleanup batch"),
                                    )
                                    .clicked()
                                {
                                    self.query(
                                        "assets.inspect",
                                        json!({"id":id}),
                                        Action::FolderBatchAsset,
                                        ui.ctx(),
                                    );
                                }
                            });
                        }
                        if entry.outcome == FolderEntryOutcome::CompanionInventoried {
                            ui.small("Inventory only: no registered media asset or copied output was created for this companion.");
                        }
                        ui.separator();
                    });
                }
            });
    }
}
