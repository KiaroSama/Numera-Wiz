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
}

impl WizApp {
    pub(super) fn preview_source_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let mut videos = asset.probe.streams.iter()
            .filter(|stream| stream.kind == StreamKind::Video && !stream.attached_picture());
        let Some(first) = videos.next() else { return; };
        let idle = self.request_idle();
        let mut request = None;
        ui.collapsing("Source frame preview", |ui| {
            let state = &mut self.source_preview;
            let stream = state.stream.get_or_insert(first.index);
            let mut changed = false;
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("source-preview-stream")
                        .selected_text(format!("Video stream {stream}"))
                        .show_ui(ui, |ui| {
                            for video in std::iter::once(first).chain(videos) {
                                changed |= ui.selectable_value(stream, video.index, format!("Video stream {}", video.index)).changed();
                            }
                        });
                    ui.label("Frame index");
                    changed |= ui.add(egui::DragValue::new(&mut state.frame).range(0..=preview::MAX_FRAME)).changed();
                    if ui.button("Decode frame").clicked() {
                        request = Some((*stream, state.frame));
                    }
                });
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
    }

    pub(super) fn accept_source_preview(&mut self, result: Value, ctx: &egui::Context) -> Result<(), String> {
        let reply: preview::Frame = serde_json::from_value(result).map_err(|error| error.to_string())?;
        let asset = self.selected_asset().ok_or("No source selected for preview")?;
        if reply.asset_id != asset.id || reply.source_sha256 != asset.identity.sha256
            || reply.width != preview::WIDTH || reply.height != preview::HEIGHT
            || Some(reply.stream_index) != self.source_preview.stream
            || reply.frame != self.source_preview.frame
            || reply.rgba_base64.len() != preview::RGBA_BYTES.div_ceil(3) * 4
        {
            return Err("The preview does not match the selected source and frame".into());
        }
        let pixels = STANDARD.decode(&reply.rgba_base64).map_err(|_| "Invalid preview pixel encoding")?;
        if pixels.len() != preview::RGBA_BYTES {
            return Err("The decoded preview has an invalid byte count".into());
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([reply.width, reply.height], &pixels);
        self.source_preview.texture = Some(ctx.load_texture("source-frame-preview", image, egui::TextureOptions::LINEAR));
        Ok(())
    }
}
