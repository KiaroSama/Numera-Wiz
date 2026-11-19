//! Picture-only timing and reverse controls for registered short clips.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    mux::StreamKind, records::Asset, time::{Rational, TimeRange},
    video_retime::{AudioPolicy, RetimeInput},
};

pub(super) struct RetimeUi {
    video: Option<u32>,
    from_ms: u32,
    to_ms: u32,
    tempo: u16,
    reverse: bool,
}

impl Default for RetimeUi {
    fn default() -> Self {
        Self { video: None, from_ms: 0, to_ms: 1000, tempo: 100, reverse: false }
    }
}

impl WizApp {
    pub(super) fn video_retime_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let videos: Vec<_> = asset.probe.streams.iter().filter(|stream| {
            stream.kind == StreamKind::Video && stream.codec == "ffv1" && !stream.attached_picture()
        }).collect();
        if videos.is_empty() || !asset.probe.format.split(',').any(|format| format == "matroska") {
            return;
        }
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Change picture speed or reverse", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                let video = self.video_retime.video.get_or_insert(videos[0].index);
                egui::ComboBox::from_id_salt("video-retime-track").selected_text(format!("Picture {video}"))
                    .show_ui(ui, |ui| {
                        for stream in videos {
                            changed |= ui.selectable_value(video, stream.index, format!("Picture {}", stream.index)).changed();
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Start frame time");
                    changed |= ui.add(egui::DragValue::new(&mut self.video_retime.from_ms).suffix(" ms")).changed();
                    ui.label("End time (exclusive)");
                    changed |= ui.add(egui::DragValue::new(&mut self.video_retime.to_ms).suffix(" ms")).changed();
                    changed |= ui.add(egui::DragValue::new(&mut self.video_retime.tempo).range(10..=800).suffix("% tempo")).changed();
                    changed |= ui.checkbox(&mut self.video_retime.reverse, "Reverse").changed();
                });
                ui.small("This lossless Matroska profile accepts clips up to 64 frames and drops audio and chapters.");
                review = ui.add_enabled(self.video_retime.from_ms < self.video_retime.to_ms && !self.output.trim().is_empty(),
                    egui::Button::new("Review picture retime")).clicked();
            });
        });
        if changed || review {
            self.changed();
        }
        if review {
            let range = TimeRange {
                start: Rational::new(i64::from(self.video_retime.from_ms),1000).expect("Bounded millisecond field"),
                end: Rational::new(i64::from(self.video_retime.to_ms),1000).expect("Bounded millisecond field"),
            };
            let request = RetimeInput { asset_id: asset.id.clone(), output: self.output.clone(),
                video_stream: self.video_retime.video.expect("Registered picture selected"),
                range, tempo_hundredths: self.video_retime.tempo, reverse: self.video_retime.reverse, audio: AudioPolicy::Drop };
            self.query("media.retime", serde_json::json!(request), Action::Plan(self.generation), ui.ctx());
        }
    }
}
