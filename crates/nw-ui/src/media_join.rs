//! Select registered sources in their intended playback order.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    media_join::{Audio, JoinInput, MAX_INPUTS, Metadata, Strategy},
    records::Asset,
};

#[derive(Default)]
pub(super) struct JoinUi {
    following: Vec<String>,
    drop_audio: bool,
    strip_metadata: bool,
}

impl WizApp {
    pub(super) fn media_join_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        if !asset
            .probe
            .format
            .split(',')
            .any(|format| format == "matroska")
        {
            return;
        }
        let idle = self.request_idle();
        let mut review = false;
        let mut changed = false;
        ui.collapsing("Join compatible media in order", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.label(format!("First input · {}", asset.display_name));
                egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                    for candidate in self.assets.iter().filter(|candidate| candidate.id != asset.id) {
                        let order = self.media_join.following.iter().position(|id| id == &candidate.id);
                        let mut included = order.is_some();
                        let title = order.map_or_else(|| candidate.display_name.clone(), |index| {
                            format!("{} · {}", index + 2, candidate.display_name)
                        });
                        if ui.add_enabled(included || self.media_join.following.len() < MAX_INPUTS - 1,
                            egui::Checkbox::new(&mut included, title)).changed()
                        {
                            changed = true;
                            if let Some(index) = order {
                                self.media_join.following.remove(index);
                            } else {
                                self.media_join.following.push(candidate.id.clone());
                            }
                        }
                    }
                });
                changed |= ui.checkbox(&mut self.media_join.drop_audio, "Drop audio tracks").changed();
                changed |= ui.checkbox(&mut self.media_join.strip_metadata, "Strip metadata").changed();
                ui.small("Sources play in the order selected. The join copies compatible packets into a new Matroska file.");
                review = ui.add_enabled(!self.media_join.following.is_empty() && !self.output.trim().is_empty(),
                    egui::Button::new("Review packet join")).clicked();
            });
        });
        if review || changed {
            self.changed();
        }
        if review {
            let request = JoinInput {
                inputs_in_order: std::iter::once(asset.id.clone())
                    .chain(self.media_join.following.iter().cloned())
                    .collect(),
                output: self.output.clone(),
                audio: if self.media_join.drop_audio {
                    Audio::Drop
                } else {
                    Audio::CopyAll
                },
                metadata: if self.media_join.strip_metadata {
                    Metadata::Strip
                } else {
                    Metadata::FirstInput
                },
                strategy: Strategy::Copy,
            };
            self.query(
                "media.join",
                serde_json::json!(request),
                Action::Plan(self.generation),
                ui.ctx(),
            );
        }
    }
}
