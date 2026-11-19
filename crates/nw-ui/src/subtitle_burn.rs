//! Review lossless caption burn with explicit source, audio and color choices.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    mux::StreamKind,
    records::Asset,
    subtitle_burn::{Audio, BurnInput, Color, Quality, SubtitleInput},
};
use std::collections::BTreeSet;

pub(super) struct CaptionBurnUi {
    video: Option<u32>,
    embedded: Option<u32>,
    external_id: Option<String>,
    use_external: bool,
    copy_all_audio: bool,
    audio: BTreeSet<u32>,
    tone_map: bool,
    peak_nits: u16,
}
impl Default for CaptionBurnUi {
    fn default() -> Self {
        Self {
            video: None,
            embedded: None,
            external_id: None,
            use_external: false,
            copy_all_audio: true,
            audio: BTreeSet::new(),
            tone_map: false,
            peak_nits: 1000,
        }
    }
}

impl WizApp {
    pub(super) fn subtitle_burn_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let pictures: Vec<_> = asset.probe.streams.iter().filter(|stream| {
            stream.kind == StreamKind::Video && stream.codec == "ffv1" && !stream.attached_picture()
        }).collect();
        if pictures.is_empty() || !asset.probe.format.split(',').any(|format| format == "matroska") {
            return;
        }
        let idle = self.request_idle();
        let state = &mut self.caption_burn;
        let video = state.video.get_or_insert(pictures[0].index);
        let output = &self.output;
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Burn text subtitles into video", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                egui::ComboBox::from_id_salt("caption-picture").selected_text(format!("Picture {video}")).show_ui(ui, |ui| {
                    for stream in pictures {
                        changed |= ui.selectable_value(video, stream.index, format!("Picture {}", stream.index)).changed();
                    }
                });
                changed |= ui.checkbox(&mut state.use_external, "Use a registered SRT file").changed();
                if state.use_external {
                    let title = state.external_id.as_ref().and_then(|id| self.assets.iter().find(|entry| &entry.id == id))
                        .map_or("Select a registered SRT file", |entry| entry.display_name.as_str());
                    egui::ComboBox::from_id_salt("caption-external").selected_text(title).show_ui(ui, |ui| {
                        for entry in self.assets.iter().filter(|entry| entry.display_name.to_ascii_lowercase().ends_with(".srt")) {
                            changed |= ui.selectable_value(&mut state.external_id, Some(entry.id.clone()), &entry.display_name).changed();
                        }
                    });
                } else {
                    egui::ComboBox::from_id_salt("caption-embedded")
                        .selected_text(state.embedded.map_or_else(|| "Select a SubRip track".into(), |index| format!("Subtitle {index}")))
                        .show_ui(ui, |ui| {
                            for stream in asset.probe.streams.iter().filter(|stream| stream.kind == StreamKind::Subtitle && stream.codec == "subrip") {
                                changed |= ui.selectable_value(&mut state.embedded, Some(stream.index), format!("Subtitle {}", stream.index)).changed();
                            }
                        });
                }
                changed |= ui.checkbox(&mut state.copy_all_audio, "Copy every original audio track").changed();
                if !state.copy_all_audio {
                    for stream in asset.probe.streams.iter().filter(|stream| stream.kind == StreamKind::Audio) {
                        let mut selected = state.audio.contains(&stream.index);
                        if ui.checkbox(&mut selected, format!("Copy audio {}", stream.index)).changed() {
                            changed = true;
                            if selected {
                                state.audio.insert(stream.index);
                            } else {
                                state.audio.remove(&stream.index);
                            }
                        }
                    }
                }
                changed |= ui.checkbox(&mut state.tone_map, "Convert PQ or HLG to BT.709 SDR").changed();
                if state.tone_map {
                    changed |= ui.add(egui::DragValue::new(&mut state.peak_nits).range(100..=10000).suffix(" nits")).changed();
                }
                ui.small("Output is Matroska with lossless FFV1 and the bundled DejaVu Sans font.");
                let has_caption = if state.use_external { state.external_id.is_some() } else { state.embedded.is_some() };
                review = ui.add_enabled(has_caption && !output.trim().is_empty(), egui::Button::new("Review subtitle burn")).clicked();
            });
        });
        let request = review.then(|| BurnInput {
            asset_id: asset.id.clone(),
            output: output.clone(),
            video_stream: *video,
            subtitle: if state.use_external {
                SubtitleInput::External { asset_id: state.external_id.clone().expect("Selected registered SRT") }
            } else {
                SubtitleInput::Embedded { stream_index: state.embedded.expect("Selected embedded subtitle") }
            },
            audio: if state.copy_all_audio {
                Audio::CopyAll {}
            } else if state.audio.is_empty() {
                Audio::None {}
            } else {
                Audio::CopySelected { indices: state.audio.iter().copied().collect() }
            },
            color: if state.tone_map { Color::ToneMapBt709 { peak_nits: state.peak_nits } } else { Color::PreserveSdr {} },
            quality: Quality::LosslessFfv1,
        });
        if changed || review {
            self.changed();
        }
        if let Some(request) = request {
            self.query("subtitles.burn", serde_json::json!(request), Action::Plan(self.generation), ui.ctx());
        }
    }
}
