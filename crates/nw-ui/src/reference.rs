//! Select a reference and compare explicitly aligned frame windows.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    mux::StreamKind,
    records::Asset,
    reference_metrics::{CompareInput, Comparison, MAX_COMPARISON_FRAMES, Psnr, Window},
};
use serde_json::{Value, json};

pub(super) struct ReferenceUi {
    selected: Option<(String, Window)>,
    frame_count: u32,
    start_frame: u64,
    pub(super) result: Option<Comparison>,
}

impl Default for ReferenceUi {
    fn default() -> Self {
        Self {
            selected: None,
            frame_count: 1,
            start_frame: 0,
            result: None,
        }
    }
}

impl WizApp {
    pub(super) fn accept_reference_result(&mut self, value: Value) -> Result<(), String> {
        let report: Comparison =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        if self
            .reference_metrics
            .selected
            .as_ref()
            .is_some_and(|(_, reference)| reference == &report.reference.selection)
            && self.selected_asset().is_some_and(|asset| {
                asset.id == report.candidate.selection.asset_id
                    && asset.identity.sha256 == report.candidate.source_sha256
            })
        {
            self.reference_metrics.result = Some(report);
        }
        Ok(())
    }

    pub(super) fn reference_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let video = |stream: &nw_domain::mux::Stream| {
            stream.kind == StreamKind::Video && !stream.attached_picture()
        };
        if !asset.probe.streams.iter().any(video) {
            return;
        }
        egui::CollapsingHeader::new("Compare video quality").show(ui, |ui| {
            let ready = self.request_idle();
            if let Some((name, reference)) = &self.reference_metrics.selected {
                ui.label(format!(
                    "Reference · {name} · stream {} · frame {}",
                    reference.stream_index, reference.start_frame
                ));
            }
            ui.add_enabled_ui(ready, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Start frame");
                    let start = ui
                        .add(
                            egui::DragValue::new(&mut self.reference_metrics.start_frame)
                                .range(0..=(i64::MAX as u64 - u64::from(MAX_COMPARISON_FRAMES))),
                        )
                        .changed();
                    ui.label("Frames");
                    let count = ui
                        .add(
                            egui::DragValue::new(&mut self.reference_metrics.frame_count)
                                .range(1..=MAX_COMPARISON_FRAMES),
                        )
                        .changed();
                    if start || count {
                        self.reference_metrics.result = None;
                    }
                    if self.reference_metrics.selected.is_some()
                        && ui.button("Clear reference").clicked()
                    {
                        self.reference_metrics.selected = None;
                        self.reference_metrics.result = None;
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for stream in asset.probe.streams.iter().filter(|stream| video(stream)) {
                            ui.horizontal(|ui| {
                                let selection = Window {
                                    asset_id: asset.id.clone(),
                                    stream_index: stream.index,
                                    start_frame: self.reference_metrics.start_frame,
                                };
                                if ui
                                    .button(format!("Use stream {} as reference", stream.index))
                                    .clicked()
                                {
                                    self.reference_metrics.selected =
                                        Some((asset.display_name.clone(), selection.clone()));
                                    self.reference_metrics.result = None;
                                }
                                if let Some((_, reference)) = &self.reference_metrics.selected
                                    && ui
                                        .button(format!("Compare stream {}", stream.index))
                                        .clicked()
                                {
                                    let input = CompareInput {
                                        reference: reference.clone(),
                                        candidate: selection,
                                        frame_count: self.reference_metrics.frame_count,
                                    };
                                    self.reference_metrics.result = None;
                                    self.query(
                                        "media.analyze.reference",
                                        json!(input),
                                        Action::CompareVideo,
                                        ui.ctx(),
                                    );
                                }
                            });
                        }
                    });
            });
            if let Some(result) = &self.reference_metrics.result {
                ui.label(format!(
                    "Decoded 8-bit RGB · {} × {}",
                    result.width, result.height
                ));
                for row in &result.frames {
                    let psnr = match row.psnr {
                        Psnr::Identical => "Identical RGB".to_owned(),
                        Psnr::Finite { centi_db } => {
                            format!("{:.2} dB", f64::from(centi_db) / 100.0)
                        }
                    };
                    ui.label(format!(
                        "Reference {} · Candidate {} · PSNR {} · SSIM {:.6}",
                        row.reference_frame,
                        row.candidate_frame,
                        psnr,
                        f64::from(row.ssim_millionths) / 1_000_000.0
                    ));
                }
            }
        });
    }
}
