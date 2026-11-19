//! Sample edit controls share the ordinary source, review and Run workflow.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{audio_edit::{EditInput, Piece}, mux::StreamKind, records::Asset};

pub(super) struct AudioEditUi {
    stream: Option<u32>,
    pieces: Vec<Piece>,
}

fn initial_piece(start_sample: u32) -> Piece {
    Piece { start_sample, end_sample: start_sample.saturating_add(48000), reverse: false, tempo_hundredths: 100 }
}

impl Default for AudioEditUi {
    fn default() -> Self {
        Self { stream: None, pieces: vec![initial_piece(0)] }
    }
}

impl WizApp {
    pub(super) fn audio_edit_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let streams: Vec<_> = asset.probe.streams.iter()
            .filter(|stream| stream.kind == StreamKind::Audio && stream.codec == "pcm_s16le").collect();
        if streams.is_empty() {
            return;
        }
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Edit PCM audio samples", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                let stream = self.audio_edit.stream.get_or_insert(streams[0].index);
                egui::ComboBox::from_id_salt("audio-edit-stream").selected_text(format!("Audio {stream}"))
                    .show_ui(ui, |ui| {
                        for item in streams {
                            changed |= ui.selectable_value(stream, item.index, format!("Audio {}", item.index)).changed();
                        }
                    });
                ui.small("Enter sample positions in the order they should play. End samples are exclusive. Use a new .wav destination.");
                let removable = self.audio_edit.pieces.len() > 1;
                let mut remove = None;
                for (index, piece) in self.audio_edit.pieces.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Start sample");
                            changed |= ui.add(egui::DragValue::new(&mut piece.start_sample)).changed();
                            ui.label("End sample");
                            changed |= ui.add(egui::DragValue::new(&mut piece.end_sample)).changed();
                            changed |= ui.checkbox(&mut piece.reverse, "Reverse").changed();
                            changed |= ui.add(egui::DragValue::new(&mut piece.tempo_hundredths).range(10..=800)
                                .suffix("% tempo")).changed();
                            if ui.add_enabled(removable, egui::Button::new("Remove")).clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                }
                if let Some(index) = remove {
                    self.audio_edit.pieces.remove(index);
                    changed = true;
                }
                if ui.add_enabled(self.audio_edit.pieces.len() < 16, egui::Button::new("Add interval")).clicked() {
                    let start = self.audio_edit.pieces.last().map_or(0, |piece| piece.end_sample);
                    self.audio_edit.pieces.push(initial_piece(start));
                    changed = true;
                }
                review = ui.add_enabled(!self.output.trim().is_empty(), egui::Button::new("Review audio edit")).clicked();
            });
        });
        if review || changed {
            self.changed();
        }
        if review {
            let request = EditInput { asset_id: asset.id.clone(), output: self.output.clone(),
                stream_index: self.audio_edit.stream.expect("Selected audio track"), pieces: self.audio_edit.pieces.clone() };
            self.query("audio.edit", serde_json::json!(request), Action::Plan(self.generation), ui.ctx());
        }
    }
}
