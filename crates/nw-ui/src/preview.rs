//! Shared native/browser display of an authenticated registered-source frame.
use super::{Action, WizApp};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use eframe::egui;
use nw_domain::{mux::StreamKind, preview, records::Asset};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct SourcePreview {
    stream: Option<u32>,
    frame: u32,
    texture: Option<egui::TextureHandle>,
    #[cfg(target_arch = "wasm32")]
    audio_stream: Option<u32>,
}

impl WizApp {
    pub(super) fn preview_source_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let mut videos = asset
            .probe
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Video && !stream.attached_picture());
        let Some(first) = videos.next() else {
            return;
        };
        let idle = self.request_idle();
        let still_image = asset.probe.is_still_image();
        let mut request = None;
        #[cfg(target_arch = "wasm32")]
        let mut playback = None;
        ui.collapsing("Source frame preview", |ui| {
            let state = &mut self.source_preview;
            let stream = state.stream.get_or_insert(first.index);
            let mut changed = false;
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if still_image {
                        *stream = first.index;
                        state.frame = 0;
                        ui.label("Still image");
                    } else {
                        egui::ComboBox::from_id_salt("source-preview-stream")
                            .selected_text(format!("Video stream {stream}"))
                            .show_ui(ui, |ui| {
                                for video in std::iter::once(first).chain(videos) {
                                    changed |= ui
                                        .selectable_value(
                                            stream,
                                            video.index,
                                            format!("Video stream {}", video.index),
                                        )
                                        .changed();
                                }
                            });
                        ui.label("Frame index");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut state.frame)
                                    .range(0..=preview::MAX_FRAME),
                            )
                            .changed();
                    }
                    if ui.button("Decode frame").clicked() {
                        request = Some((*stream, state.frame));
                    }
                });
                #[cfg(target_arch = "wasm32")]
                if !still_image && crate::web::source_segment::supported() {
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("source-playback-audio")
                            .selected_text(state.audio_stream.map_or_else(
                                || "No sound".to_owned(),
                                |index| format!("Audio stream {index}"),
                            ))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut state.audio_stream, None, "No sound");
                                for audio in asset.probe.streams.iter().filter(|stream| {
                                    stream.kind == StreamKind::Audio
                                        && asset
                                            .probe
                                            .stream_details
                                            .get(&stream.index)
                                            .is_some_and(|detail| {
                                                detail["channel_layout"] == "stereo"
                                                    && detail["sample_rate"] == "48000"
                                            })
                                }) {
                                    ui.selectable_value(
                                        &mut state.audio_stream,
                                        Some(audio.index),
                                        format!("Audio stream {}", audio.index),
                                    );
                                }
                            });
                        if ui.button("Open two-second video preview").clicked() {
                            playback = Some((*stream, state.audio_stream));
                        }
                    });
                }
            });
            if changed || request.is_some() {
                state.texture = None;
            }
            if let Some(texture) = &state.texture {
                let width = ui.available_width().clamp(1.0, (preview::WIDTH * 2) as f32);
                let height = width * preview::HEIGHT as f32 / preview::WIDTH as f32;
                ui.image((texture.id(), egui::vec2(width, height)));
            }
        });
        if let Some((stream, frame)) = request {
            self.query(
                "media.preview_frame",
                json!({"asset_id":asset.id,"stream_index":stream,"frame":frame}),
                Action::SourcePreview,
                ui.ctx(),
            );
        }
        #[cfg(target_arch = "wasm32")]
        if let Some((video_stream, audio_stream)) = playback {
            self.client.close_source_segment();
            self.query(
                "previews.source_segment",
                json!({"asset_id":asset.id,"video_stream":video_stream,"audio_stream":audio_stream}),
                Action::SourceSegment,
                ui.ctx(),
            );
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn accept_source_segment(
        &mut self,
        result: Value,
        request: &nw_domain::commands::Request,
    ) -> Result<(), String> {
        let reply: preview::SourceSegment =
            serde_json::from_value(result).map_err(|error| error.to_string())?;
        let selected = self.selected_asset().ok_or("Select a source to play")?;
        let expected: preview::SourceSegmentInput = serde_json::from_value(request.params.clone())
            .map_err(|error| format!("Invalid source preview request: {error}"))?;
        if reply.selection != expected
            || reply.selection.asset_id != selected.id
            || reply.source_sha256 != selected.identity.sha256
            || Some(reply.selection.video_stream) != self.source_preview.stream
            || reply.selection.audio_stream != self.source_preview.audio_stream
        {
            return Err("The preview does not match the selected video and audio streams".into());
        }
        self.client.open_source_segment(reply);
        Ok(())
    }

    pub(super) fn accept_source_preview(
        &mut self,
        result: Value,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let reply: preview::Frame =
            serde_json::from_value(result).map_err(|error| error.to_string())?;
        let asset = self
            .selected_asset()
            .ok_or("No source selected for preview")?;
        if reply.asset_id != asset.id
            || reply.source_sha256 != asset.identity.sha256
            || reply.width != preview::WIDTH
            || reply.height != preview::HEIGHT
            || Some(reply.stream_index) != self.source_preview.stream
            || reply.frame != self.source_preview.frame
            || reply.rgba_base64.len() != preview::RGBA_BYTES.div_ceil(3) * 4
        {
            return Err("The preview does not match the selected source and frame".into());
        }
        let pixels = STANDARD
            .decode(&reply.rgba_base64)
            .map_err(|_| "Invalid preview pixel encoding")?;
        if pixels.len() != preview::RGBA_BYTES {
            return Err("The decoded preview has an invalid byte count".into());
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([reply.width, reply.height], &pixels);
        self.source_preview.texture =
            Some(ctx.load_texture("source-frame-preview", image, egui::TextureOptions::LINEAR));
        Ok(())
    }
}
