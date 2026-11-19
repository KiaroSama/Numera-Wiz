//! Normalization proposals carry independent settings for each physical track.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{audio_normalization::{NormalizeInput, Settings, TrackRequest}, mux::StreamKind, records::Asset};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct NormalizeUi {
    tracks: BTreeMap<u32, Settings>,
}

impl WizApp {
    pub(super) fn normalize_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let pcm = |stream: &nw_domain::mux::Stream| stream.kind == StreamKind::Audio && stream.codec == "pcm_s16le";
        if !asset.probe.format.split(',').any(|format| format == "matroska")
            || !asset.probe.streams.iter().any(pcm)
        {
            return;
        }
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Normalize PCM audio loudness", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                for stream in asset.probe.streams.iter().filter(|stream| pcm(stream)) {
                    let mut selected = self.normalize.tracks.contains_key(&stream.index);
                    if ui.add_enabled(selected || self.normalize.tracks.len() < 2,
                        egui::Checkbox::new(&mut selected, format!("Normalize original audio stream {}", stream.index))).changed() {
                        if selected {
                            self.normalize.tracks.insert(stream.index, Settings::default());
                        } else {
                            self.normalize.tracks.remove(&stream.index);
                        }
                        changed = true;
                    }
                    if let Some(settings) = self.normalize.tracks.get_mut(&stream.index) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Target (hundredths of LUFS)");
                            changed |= ui.add(egui::DragValue::new(&mut settings.integrated_centi_lufs).range(-3000..=-1000)).changed();
                            ui.label("Peak limit (hundredths of dBTP)");
                            changed |= ui.add(egui::DragValue::new(&mut settings.true_peak_centi_dbtp).range(-900..=-100)).changed();
                            ui.label("Range limit (hundredths of LU)");
                            changed |= ui.add(egui::DragValue::new(&mut settings.range_centi_lu).range(100..=2000)).changed();
                        });
                    }
                }
                review = ui.add_enabled(!self.output.trim().is_empty() && !self.normalize.tracks.is_empty(),
                    egui::Button::new("Measure and review"),).clicked();
            });
        });
        if changed || review {
            self.changed();
        }
        if review {
            let tracks = self.normalize.tracks.iter().map(|(&stream_index, &settings)| TrackRequest { stream_index, settings }).collect();
            let params = NormalizeInput { asset_id: asset.id.clone(), tracks, output: self.output.clone() };
            self.query("audio.normalize", serde_json::json!(params), Action::Plan(self.generation), ui.ctx());
        }
    }
}
