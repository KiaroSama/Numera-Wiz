//! CPU AVC export from a registered SDR picture.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    avc_export::{
        Audio, Bitrate, Container, Crop, Detail, Encode, EncodeInput, PictureLook,
        PictureTransform, Preset, RateMode, Resize, ResizeAspect, Rotation, Strength, VideoCodec,
    },
    mux::StreamKind,
    records::Asset,
    time::Rational,
};

pub(super) struct AvcUi {
    picture: Option<u32>,
    audio: Option<u32>,
    bitrate: u16,
    crf: u8,
    quantizer: u8,
    codec: VideoCodec,
    average_kbps: u32,
    rate_mode: Option<RateMode>,
    preset: Preset,
    picture_transform: Option<PictureTransform>,
}

impl Default for AvcUi {
    fn default() -> Self {
        Self {
            picture: None,
            audio: None,
            bitrate: 128,
            crf: 18,
            quantizer: 4,
            codec: VideoCodec::H264,
            average_kbps: 2000,
            rate_mode: None,
            preset: Preset::Balanced,
            picture_transform: None,
        }
    }
}

impl WizApp {
    pub(super) fn avc_export_panel(&mut self, ui: &mut egui::Ui, source: &Asset) {
        let pictures: Vec<_> = source
            .probe
            .streams
            .iter()
            .filter(|item| {
                item.kind == StreamKind::Video
                    && matches!(
                        item.codec.as_str(),
                        "ffv1" | "h264" | "hevc" | "av1" | "vp9" | "mpeg4"
                    )
                    && !item.attached_picture()
            })
            .collect();
        if pictures.is_empty()
            || !source
                .probe
                .format
                .split(',')
                .any(|format| matches!(format, "matroska" | "mov"))
        {
            return;
        }
        let container = match std::path::Path::new(&self.output)
            .extension()
            .and_then(|value| value.to_str())
        {
            Some(ext) if ext.eq_ignore_ascii_case("mp4") => Some(Container::Mp4),
            Some(ext) if ext.eq_ignore_ascii_case("mkv") => Some(Container::Matroska),
            _ => None,
        };
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Encode video", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    changed |= ui
                        .radio_value(&mut self.avc_export.codec, VideoCodec::H264, "H.264")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.avc_export.codec, VideoCodec::Hevc, "HEVC")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.avc_export.codec, VideoCodec::Av1, "AV1")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.avc_export.codec, VideoCodec::Vp9, "VP9")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.avc_export.codec, VideoCodec::Mpeg4, "MPEG-4")
                        .changed();
                });
                let chosen = self.avc_export.picture.get_or_insert(pictures[0].index);
                egui::ComboBox::from_id_salt("avc-picture")
                    .selected_text(format!("Picture {chosen}"))
                    .show_ui(ui, |ui| {
                        for item in pictures {
                            changed |= ui
                                .selectable_value(
                                    chosen,
                                    item.index,
                                    format!("Picture {}", item.index),
                                )
                                .changed();
                        }
                    });
                let quantized = self.avc_export.codec == VideoCodec::Mpeg4;
                let quality_label = if quantized { "Quantizer" } else { "Quality CRF" };
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("avc-rate-mode")
                        .selected_text(match self.avc_export.rate_mode {
                            None => quality_label,
                            Some(RateMode::QualityVbr) => "Variable bitrate",
                            Some(RateMode::StrictSize) => "Bitrate with peak cap",
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    &mut self.avc_export.rate_mode,
                                    None,
                                    quality_label,
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut self.avc_export.rate_mode,
                                    Some(RateMode::QualityVbr),
                                    "Variable bitrate",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut self.avc_export.rate_mode,
                                    Some(RateMode::StrictSize),
                                    "Bitrate with peak cap",
                                )
                                .changed();
                        });
                    if self.avc_export.rate_mode.is_some() {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.avc_export.average_kbps)
                                    .range(64..=100000)
                                    .suffix(" kbps"),
                            )
                            .changed();
                    } else if quantized {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.avc_export.quantizer).range(1..=31),
                            )
                            .changed();
                    } else {
                        changed |= ui
                            .add(egui::DragValue::new(&mut self.avc_export.crf).range(1..=51))
                            .changed();
                    }
                    egui::ComboBox::from_id_salt("avc-preset")
                        .selected_text(format!("{:?}", self.avc_export.preset))
                        .show_ui(ui, |ui| {
                            for preset in [Preset::Fast, Preset::Balanced, Preset::Slow] {
                                changed |= ui
                                    .selectable_value(
                                        &mut self.avc_export.preset,
                                        preset,
                                        format!("{preset:?}"),
                                    )
                                    .changed();
                            }
                        });
                });
                egui::ComboBox::from_id_salt("avc-audio")
                    .selected_text(self.avc_export.audio.map_or_else(
                        || "No audio".to_owned(),
                        |index| format!("Encode audio {index} as AAC"),
                    ))
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut self.avc_export.audio, None, "No audio")
                            .changed();
                        for item in source.probe.streams.iter().filter(|item| {
                            item.kind == StreamKind::Audio
                                && matches!(item.codec.as_str(), "pcm_s16le" | "aac")
                        }) {
                            changed |= ui
                                .selectable_value(
                                    &mut self.avc_export.audio,
                                    Some(item.index),
                                    format!("Audio {}", item.index),
                                )
                                .changed();
                        }
                    });
                if self.avc_export.audio.is_some() {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.avc_export.bitrate)
                                .range(48..=320)
                                .suffix(" kbps AAC"),
                        )
                        .changed();
                }
                let mut transform = self.avc_export.picture_transform.is_some();
                if ui
                    .checkbox(&mut transform, "Transform the picture")
                    .changed()
                {
                    self.avc_export.picture_transform = transform.then(PictureTransform::default);
                    changed = true;
                }
                if let Some(picture) = &mut self.avc_export.picture_transform {
                    let details = source.probe.stream_details.get(chosen);
                    let original = |key, maximum| {
                        details
                            .and_then(|value| value[key].as_u64())
                            .map_or(2, |size| ((size.min(maximum) as u32) & !1).max(2))
                    };
                    changed |= picture_controls(
                        ui,
                        picture,
                        (original("width", 4096), original("height", 2160)),
                    );
                }
                ui.small("Use a new .mp4 or .mkv destination.");
                review = ui
                    .add_enabled(
                        container.is_some(),
                        egui::Button::new("Review video export"),
                    )
                    .clicked();
            });
        });
        if changed || review {
            self.changed();
        }
        if review {
            let quality = self.avc_export.rate_mode.is_none();
            let quantized = self.avc_export.codec == VideoCodec::Mpeg4;
            let settings = Encode {
                video_stream: self.avc_export.picture.expect("Registered picture stream"),
                container: container.expect("Reviewed destination extension"),
                codec: self.avc_export.codec,
                crf: (quality && !quantized).then_some(self.avc_export.crf),
                quantizer: (quality && quantized).then_some(self.avc_export.quantizer),
                bitrate: self.avc_export.rate_mode.map(|mode| Bitrate {
                    average_kbps: self.avc_export.average_kbps,
                    mode,
                }),
                preset: self.avc_export.preset,
                audio: self
                    .avc_export
                    .audio
                    .map_or(Audio::Drop {}, |stream_index| Audio::Aac {
                        stream_index,
                        bitrate_kbps: self.avc_export.bitrate,
                    }),
                picture: self.avc_export.picture_transform,
            };
            let request = EncodeInput {
                asset_id: source.id.clone(),
                output: self.output.clone(),
                settings,
            };
            self.query(
                "media.encode",
                serde_json::json!(request),
                Action::Plan(self.generation),
                ui.ctx(),
            );
        }
    }
}

fn picture_controls(
    ui: &mut egui::Ui,
    picture: &mut PictureTransform,
    original: (u32, u32),
) -> bool {
    let mut changed = false;
    let mut crop = picture.crop.is_some();
    if ui.checkbox(&mut crop, "Crop").changed() {
        picture.crop = crop.then_some(Crop {
            width: original.0,
            height: original.1,
            x: 0,
            y: 0,
        });
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
                for (rotation, label) in [
                    (Rotation::None, "No rotation"),
                    (Rotation::Clockwise, "90° clockwise"),
                    (Rotation::HalfTurn, "180°"),
                    (Rotation::Counterclockwise, "90° counterclockwise"),
                ] {
                    changed |= ui
                        .selectable_value(&mut picture.rotation, rotation, label)
                        .changed();
                }
            });
        changed |= ui
            .checkbox(&mut picture.flip_horizontal, "Flip horizontally")
            .changed();
        changed |= ui
            .checkbox(&mut picture.flip_vertical, "Flip vertically")
            .changed();
    });
    let mut resize = picture.resize.is_some();
    if ui.checkbox(&mut resize, "Resize").changed() {
        picture.resize = resize.then_some(Resize {
            width: 1280,
            height: 720,
            aspect: ResizeAspect::PreserveDisplay,
        });
        changed = true;
    }
    if let Some(resize) = &mut picture.resize {
        ui.horizontal_wrapped(|ui| {
            changed |= dimension(ui, "Width", &mut resize.width, 2..=4096);
            changed |= dimension(ui, "Height", &mut resize.height, 2..=2160);
            changed |= ui
                .radio_value(
                    &mut resize.aspect,
                    ResizeAspect::PreserveDisplay,
                    "Preserve display aspect",
                )
                .changed();
            changed |= ui
                .radio_value(
                    &mut resize.aspect,
                    ResizeAspect::SquarePixels,
                    "Square pixels",
                )
                .changed();
            changed |= ui
                .radio_value(&mut resize.aspect, ResizeAspect::Fit, "Fit with padding")
                .changed();
        });
    }
    let mut look = picture.look.is_some();
    if ui.checkbox(&mut look, "Adjust color and detail").changed() {
        picture.look = look.then(PictureLook::default);
        changed = true;
    }
    if let Some(look) = &mut picture.look {
        changed |= look_controls(ui, look);
    }
    changed
}

fn look_controls(ui: &mut egui::Ui, look: &mut PictureLook) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= ui.checkbox(&mut look.grayscale, "Grayscale").changed();
        changed |= adjustment(ui, "Brightness", &mut look.brightness, 0.0, -1.0..=1.0);
        changed |= adjustment(ui, "Contrast", &mut look.contrast, 1.0, 0.0..=3.0);
        ui.add_enabled_ui(!look.grayscale, |ui| {
            changed |= adjustment(ui, "Saturation", &mut look.saturation, 1.0, 0.0..=3.0);
        });
        changed |= adjustment(ui, "Gamma", &mut look.gamma, 1.0, 0.1..=10.0);
    });
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("avc-denoise")
            .selected_text(look.denoise.map_or_else(
                || "Denoise off".into(),
                |level| format!("Denoise {level:?}"),
            ))
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut look.denoise, None, "Denoise off")
                    .changed();
                for strength in [Strength::Light, Strength::Medium, Strength::Heavy] {
                    changed |= ui
                        .selectable_value(
                            &mut look.denoise,
                            Some(strength),
                            format!("{strength:?}"),
                        )
                        .changed();
                }
            });
        egui::ComboBox::from_id_salt("avc-detail")
            .selected_text(match look.detail {
                None => "Detail unchanged".into(),
                Some(Detail::Sharpen { strength }) => format!("Sharpen {strength:?}"),
                Some(Detail::Blur { strength }) => format!("Blur {strength:?}"),
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut look.detail, None, "Detail unchanged")
                    .changed();
                for strength in [Strength::Light, Strength::Medium, Strength::Heavy] {
                    changed |= ui
                        .selectable_value(
                            &mut look.detail,
                            Some(Detail::Sharpen { strength }),
                            format!("Sharpen {strength:?}"),
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut look.detail,
                            Some(Detail::Blur { strength }),
                            format!("Blur {strength:?}"),
                        )
                        .changed();
                }
            });
    });
    changed
}

fn adjustment(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<Rational>,
    neutral: f64,
    bounds: std::ops::RangeInclusive<f64>,
) -> bool {
    ui.label(label);
    let mut scale = value.map_or(neutral, |value| {
        value.numerator() as f64 / value.denominator() as f64
    });
    let changed = ui
        .add(egui::DragValue::new(&mut scale).range(bounds).speed(0.01))
        .changed();
    if changed {
        let numerator = (scale * 1000.0).round() as i64;
        *value = if numerator == (neutral * 1000.0) as i64 {
            None
        } else {
            Some(Rational::new(numerator, 1000).expect("Fixed UI precision denominator"))
        };
    }
    changed
}

fn dimension(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
) -> bool {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(range).speed(2.0))
        .changed()
}
