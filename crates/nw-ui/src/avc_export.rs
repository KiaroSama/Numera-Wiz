//! CPU AVC export from a registered SDR picture.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    avc_export::{Audio, Container, Crop, Encode, EncodeInput, PictureTransform, Preset, Resize, ResizeAspect, Rotation},
    mux::StreamKind,
    records::Asset,
};

pub(super) struct AvcUi {
    picture: Option<u32>,
    audio: Option<u32>,
    bitrate: u16,
    crf: u8,
    preset: Preset,
    picture_transform: Option<PictureTransform>,
}

impl Default for AvcUi {
    fn default() -> Self {
        Self { picture: None, audio: None, bitrate: 128, crf: 18, preset: Preset::Balanced, picture_transform: None }
    }
}

impl WizApp {
    pub(super) fn avc_export_panel(&mut self, ui: &mut egui::Ui, source: &Asset) {
        let pictures: Vec<_> = source.probe.streams.iter().filter(|item| {
            item.kind == StreamKind::Video && item.codec == "ffv1" && !item.attached_picture()
        }).collect();
        if pictures.is_empty() || !source.probe.format.split(',').any(|format| format == "matroska") {
            return;
        }
        let container = match std::path::Path::new(&self.output).extension().and_then(|value| value.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("mp4") => Some(Container::Mp4),
            Some(ext) if ext.eq_ignore_ascii_case("mkv") => Some(Container::Matroska),
            _ => None,
        };
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Encode H.264 video", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                let chosen = self.avc_export.picture.get_or_insert(pictures[0].index);
                egui::ComboBox::from_id_salt("avc-picture").selected_text(format!("Picture {chosen}"))
                    .show_ui(ui, |ui| {
                        for item in pictures {
                            changed |= ui.selectable_value(chosen,item.index,format!("Picture {}",item.index)).changed();
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Quality CRF");
                    changed |= ui.add(egui::DragValue::new(&mut self.avc_export.crf).range(1..=51)).changed();
                    egui::ComboBox::from_id_salt("avc-preset").selected_text(format!("{:?}",self.avc_export.preset))
                        .show_ui(ui, |ui| {
                            for preset in [Preset::Fast,Preset::Balanced,Preset::Slow] {
                                changed |= ui.selectable_value(&mut self.avc_export.preset,preset,format!("{preset:?}")).changed();
                            }
                        });
                });
                egui::ComboBox::from_id_salt("avc-audio").selected_text(
                    self.avc_export.audio.map_or_else(|| "No audio".to_owned(),|index| format!("Encode audio {index} as AAC"))
                ).show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut self.avc_export.audio,None,"No audio").changed();
                    for item in source.probe.streams.iter().filter(|item| item.kind==StreamKind::Audio && item.codec=="pcm_s16le") {
                        changed |= ui.selectable_value(&mut self.avc_export.audio,Some(item.index),
                            format!("Audio {}",item.index)).changed();
                    }
                });
                if self.avc_export.audio.is_some() {
                    changed |= ui.add(egui::DragValue::new(&mut self.avc_export.bitrate).range(48..=320).suffix(" kbps AAC")).changed();
                }
                let mut transform = self.avc_export.picture_transform.is_some();
                if ui.checkbox(&mut transform, "Transform the picture").changed() {
                    self.avc_export.picture_transform = transform.then(PictureTransform::default);
                    changed = true;
                }
                if let Some(picture) = &mut self.avc_export.picture_transform {
                    let details = source.probe.stream_details.get(chosen);
                    let original = |key, maximum| details.and_then(|value| value[key].as_u64())
                        .map_or(2, |size| ((size.min(maximum) as u32) & !1).max(2));
                    changed |= picture_controls(ui, picture, (original("width",4096), original("height",2160)));
                }
                ui.small("Use a new .mp4 or .mkv destination.");
                review = ui.add_enabled(container.is_some(),egui::Button::new("Review H.264 export")).clicked();
            });
        });
        if changed || review {
            self.changed();
        }
        if review {
            let settings = Encode {
                video_stream: self.avc_export.picture.expect("Registered FFV1 stream"),
                container: container.expect("Reviewed destination extension"),
                crf: self.avc_export.crf,
                preset: self.avc_export.preset,
                audio: self.avc_export.audio.map_or(Audio::Drop {},|stream_index| Audio::Aac {
                    stream_index, bitrate_kbps:self.avc_export.bitrate,
                }),
                picture: self.avc_export.picture_transform,
            };
            let request=EncodeInput { asset_id:source.id.clone(),output:self.output.clone(),settings };
            self.query("media.encode",serde_json::json!(request),Action::Plan(self.generation),ui.ctx());
        }
    }
}

fn picture_controls(ui: &mut egui::Ui, picture: &mut PictureTransform, original: (u32,u32)) -> bool {
    let mut changed = false;
    let mut crop = picture.crop.is_some();
    if ui.checkbox(&mut crop, "Crop").changed() {
        picture.crop = crop.then_some(Crop { width: original.0, height: original.1, x: 0, y: 0 });
        changed = true;
    }
    if let Some(crop) = &mut picture.crop {
        ui.horizontal_wrapped(|ui| {
            changed |= dimension(ui, "X", &mut crop.x, 0..=4094);
            changed |= dimension(ui, "Y", &mut crop.y, 0..=2158);
            changed |= dimension(ui, "Width", &mut crop.width, 2..=4096);
            changed |= dimension(ui, "Height", &mut crop.height, 2..=2160);
        });
    }
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("avc-rotation")
            .selected_text(match picture.rotation {
                Rotation::None => "No rotation",
                Rotation::Clockwise => "90° clockwise",
                Rotation::HalfTurn => "180°",
                Rotation::Counterclockwise => "90° counterclockwise",
            })
            .show_ui(ui, |ui| {
                for (rotation, label) in [(Rotation::None,"No rotation"),(Rotation::Clockwise,"90° clockwise"),
                    (Rotation::HalfTurn,"180°"),(Rotation::Counterclockwise,"90° counterclockwise")] {
                    changed |= ui.selectable_value(&mut picture.rotation, rotation, label).changed();
                }
            });
        changed |= ui.checkbox(&mut picture.flip_horizontal, "Flip horizontally").changed();
        changed |= ui.checkbox(&mut picture.flip_vertical, "Flip vertically").changed();
    });
    let mut resize = picture.resize.is_some();
    if ui.checkbox(&mut resize, "Resize").changed() {
        picture.resize = resize.then_some(Resize { width: 1280, height: 720, aspect: ResizeAspect::PreserveDisplay });
        changed = true;
    }
    if let Some(resize) = &mut picture.resize {
        ui.horizontal_wrapped(|ui| {
            changed |= dimension(ui, "Width", &mut resize.width, 2..=4096);
            changed |= dimension(ui, "Height", &mut resize.height, 2..=2160);
            changed |= ui.radio_value(&mut resize.aspect, ResizeAspect::PreserveDisplay, "Preserve display aspect").changed();
            changed |= ui.radio_value(&mut resize.aspect, ResizeAspect::SquarePixels, "Square pixels").changed();
        });
    }
    changed
}

fn dimension(ui: &mut egui::Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) -> bool {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(range).speed(2.0)).changed()
}
