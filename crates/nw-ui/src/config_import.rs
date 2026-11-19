use super::{Action, WizApp};
use eframe::egui;
use nw_domain::config_import::{CONFIG_BYTES, CompatibilityProfile};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct ConfigImportUi {
    source: String,
    base_directory: String,
    pub(super) report: Option<Value>,
    pub(super) generation: u64,
}

impl WizApp {
    pub(super) fn config_import_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Import FFmWiz settings").show(ui, |ui| {
            ui.label("Configuration directory on the media engine");
            let directory = ui.add(
                egui::TextEdit::singleline(&mut self.config_import.base_directory)
                    .char_limit(4096)
                    .hint_text("Absolute path")
                    .desired_width(f32::INFINITY),
            );
            ui.label("Configuration text");
            let source = ui.add(
                egui::TextEdit::multiline(&mut self.config_import.source)
                    .desired_rows(5)
                    .char_limit(CONFIG_BYTES)
                    .desired_width(f32::INFINITY),
            );
            if directory.changed() || source.changed() {
                self.config_import.report = None;
                self.config_import.generation = self.config_import.generation.wrapping_add(1);
            }
            let ready = self.request_idle()
                && !self.config_import.base_directory.trim().is_empty()
                && !self.config_import.source.trim().is_empty()
                && self.config_import.source.len() <= CONFIG_BYTES;
            if ui
                .add_enabled(ready, egui::Button::new("Import settings"))
                .clicked()
            {
                self.config_import.report = None;
                self.query(
                    "config.import.ffmwiz",
                    json!({
                        "source":self.config_import.source,
                        "base_directory":self.config_import.base_directory.trim(),
                        "compatibility_profile":CompatibilityProfile::FfmwizV1,
                    }),
                    Action::ImportConfig(self.config_import.generation),
                    ui.ctx(),
                );
            }
            if self.config_import.source.len() > CONFIG_BYTES {
                ui.label("Choose configuration text within 64 KiB.");
            }
            let Some(report) = &self.config_import.report else {
                return;
            };
            if let Some(settings) = report["job"]["settings"].as_object() {
                ui.label(format!("Imported settings: {}", settings.len()));
            }
            if let Some(diagnostics) = report["diagnostics"].as_array() {
                egui::ScrollArea::vertical()
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for diagnostic in diagnostics {
                            let field = diagnostic["field"].as_str().unwrap_or("assignment");
                            let line = diagnostic["line"].as_u64().unwrap_or(0);
                            let code = diagnostic["code"]
                                .as_str()
                                .unwrap_or_default()
                                .replace('_', " ");
                            if line > 0 {
                                ui.label(format!("Line {line}, {field}: {code}"));
                            } else {
                                ui.label(format!("{field}: {code}"));
                            }
                        }
                    });
            }
            if ui.button("Copy import report").clicked()
                && let Ok(text) = serde_json::to_string_pretty(report)
            {
                ui.ctx().copy_text(text);
            }
            if let Some(asset) = self.asset_detail.as_ref()
                && ui.add_enabled(
                    self.request_idle() && report["input_valid"] == true,
                    egui::Button::new("Review packet-copy job for the selected source"),
                ).clicked()
            {
                let params = json!({"asset_id":asset.id,"job_document":report["job"]});
                self.changed();
                self.query("jobs.plan", params, Action::Plan(self.generation), ui.ctx());
            }
        });
    }
}
