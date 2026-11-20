//! The append review shares the ordinary plan and execution controls.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    commands,
    mux::StreamKind,
    records::Asset,
    track_append::{
        Addition, AppendInput, Conversion, MAX_ADDED_INPUTS, MAX_ADDED_TRACKS, Metadata, Removal,
        ReplaceInput, Selection,
    },
};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct TrackAppendUi {
    sources: Vec<String>,
    explicit: String,
    remove: BTreeSet<u32>,
    strip_global: bool,
}

impl WizApp {
    pub(super) fn track_append_panel(&mut self, ui: &mut egui::Ui, primary: &Asset) {
        if !primary
            .probe
            .format
            .split(',')
            .any(|format| format == "matroska")
        {
            return;
        }
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Add or replace audio and subtitles", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.collapsing("Remove original tracks", |ui| {
                    for stream in primary.probe.streams.iter().filter(|stream| matches!(stream.kind, StreamKind::Audio | StreamKind::Subtitle)) {
                        let mut selected = self.track_append.remove.contains(&stream.index);
                        let text = format!("Remove {:?} stream {}", stream.kind, stream.index);
                        if ui.add_enabled(selected || self.track_append.remove.len() < MAX_ADDED_TRACKS,
                            egui::Checkbox::new(&mut selected, text)).changed() {
                            changed = true;
                            if selected {
                                self.track_append.remove.insert(stream.index);
                            } else {
                                self.track_append.remove.remove(&stream.index);
                            }
                        }
                    }
                });
                changed |= ui.checkbox(&mut self.track_append.strip_global, "Strip global metadata").changed();
                ui.label("Choose registered sources in the order they should be appended.");
                egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                    for asset in self.assets.iter().filter(|asset| asset.id != primary.id) {
                        let order = self.track_append.sources.iter().position(|id| id == &asset.id);
                        let mut selected = order.is_some();
                        let label = order.map_or_else(|| asset.display_name.clone(), |index| format!("{}. {}", index + 1, asset.display_name));
                        if ui.add_enabled(selected || self.track_append.sources.len() < MAX_ADDED_INPUTS,
                            egui::Checkbox::new(&mut selected, label)).changed() {
                            if let Some(index) = order {
                                self.track_append.sources.remove(index);
                            } else {
                                self.track_append.sources.push(asset.id.clone());
                            }
                            changed = true;
                        }
                    }
                });
                ui.collapsing("Explicit track selectors", |ui| {
                    ui.label("Paste an additions array to select individual streams and set their tags. Leave it empty to use the checked sources.");
                    changed |= ui.add(egui::TextEdit::multiline(&mut self.track_append.explicit).code_editor().desired_rows(4)).changed();
                });
                review = ui.add_enabled(
                    !self.output.trim().is_empty()
                      && (!self.track_append.sources.is_empty() || !self.track_append.explicit.trim().is_empty()
                          || !self.track_append.remove.is_empty() || self.track_append.strip_global),
                    egui::Button::new("Review track changes"),
                ).clicked();
            });
        });
        if changed || review {
            self.changed();
        }
        if !review {
            return;
        }
        let additions = if self.track_append.explicit.trim().is_empty() {
            self.track_append
                .sources
                .iter()
                .map(|asset| Addition {
                    asset_id: asset.clone(),
                    selector: Selection::All {},
                })
                .collect()
        } else {
            match commands::decode_json(self.track_append.explicit.as_bytes())
                .and_then(commands::parse)
            {
                Ok(additions) => additions,
                Err(error) => {
                    self.error = Some(error.message);
                    return;
                }
            }
        };
        let (command, params) =
            if self.track_append.remove.is_empty() && !self.track_append.strip_global {
                (
                    "streams.add",
                    serde_json::json!(AppendInput {
                        target: primary.id.clone(),
                        additions,
                        output: self.output.clone(),
                        conversion: Conversion::Forbid,
                    }),
                )
            } else {
                (
                    "streams.replace",
                    serde_json::json!(ReplaceInput {
                        target: primary.id.clone(),
                        additions,
                        remove: self
                            .track_append
                            .remove
                            .iter()
                            .map(|index| Removal::OriginalIndex { index: *index })
                            .collect(),
                        metadata: if self.track_append.strip_global {
                            Metadata::StripGlobal
                        } else {
                            Metadata::Preserve
                        },
                        output: self.output.clone(),
                        conversion: Conversion::Forbid,
                    }),
                )
            };
        self.query(command, params, Action::Plan(self.generation), ui.ctx());
    }
}
