//! Inspect source packet and decoded-frame observations in either client.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    mux::StreamKind,
    records::Asset,
    stream_observations::{Detail, Input, Kind, Report},
    time::Rational,
};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct ObservationUi {
    active: Option<Input>,
    report: Option<Report>,
    stream: Option<u32>,
    first: u32,
    kind: Kind,
}

fn time(value: Option<Rational>) -> String {
    value.map_or_else(
        || "—".into(),
        |at| format!("{}/{} s", at.numerator(), at.denominator()),
    )
}

impl WizApp {
    pub(super) fn observations_panel(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        ui.collapsing("Packets and decoded frames", |ui| {
            let idle = self.request_idle();
            let state = &mut self.observations;
            let mut changed = false;
            let mut scan = false;
            let allowed = |stream: &nw_domain::mux::Stream, kind| {
                !stream.attached_picture()
                    && stream.kind != StreamKind::Attachment
                    && (kind == Kind::Packets
                        || matches!(stream.kind, StreamKind::Video | StreamKind::Audio))
            };
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    changed |= ui
                        .radio_value(&mut state.kind, Kind::Packets, "Packets")
                        .changed();
                    changed |= ui
                        .radio_value(&mut state.kind, Kind::Frames, "Frames")
                        .changed();
                    let streams = asset
                        .probe
                        .streams
                        .iter()
                        .filter(|stream| allowed(stream, state.kind))
                        .collect::<Vec<_>>();
                    if !streams.iter().any(|stream| Some(stream.index) == state.stream) {
                        state.stream = streams.first().map(|stream| stream.index);
                    }
                    let Some(stream) = &mut state.stream else {
                        ui.label("No eligible stream");
                        return;
                    };
                    egui::ComboBox::from_id_salt("observation-stream")
                        .selected_text(format!("Stream {stream}"))
                        .show_ui(ui, |ui| {
                            for candidate in streams {
                                changed |= ui
                                    .selectable_value(
                                        stream,
                                        candidate.index,
                                        format!("{} · {}", candidate.index, candidate.codec),
                                    )
                                    .changed();
                            }
                        });
                    ui.label("First row");
                    changed |= ui.add(egui::DragValue::new(&mut state.first)).changed();
                    scan = ui.button("Scan stream").clicked();
                });
            });
            if changed || scan {
                state.report = None;
            }
            if scan && let Some(stream_index) = state.stream {
                let input = Input {
                    asset_id: asset.id.clone(),
                    stream_index,
                    kind: state.kind,
                    first: state.first,
                    limit: 16,
                };
                state.active = Some(input.clone());
                self.query(
                    "media.analyze.stream",
                    json!(input),
                    Action::StreamObservations,
                    ui.ctx(),
                );
            }
            if let Some(report) = &self.observations.report {
                ui.label(format!("{} observed in the stream", report.observed));
                if report.rows.is_empty() {
                    ui.label("No rows at this position");
                }
                egui::ScrollArea::vertical()
                    .id_salt("observation-rows")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for row in &report.rows {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!(
                                    "{} · PTS {} · DTS {} · duration {}",
                                    row.ordinal,
                                    time(row.pts),
                                    time(row.dts),
                                    time(row.duration),
                                ));
                                match &row.detail {
                                    Detail::Packet { bytes, flags, data_sha256 } => {
                                        ui.label(format!("{bytes} bytes · {flags}"))
                                            .on_hover_text(data_sha256.as_str());
                                    }
                                    Detail::Frame { key_frame, picture_type, width, height, samples_per_channel } => {
                                        if *key_frame {
                                            ui.label("Key frame");
                                        }
                                        if let Some(kind) = picture_type {
                                            ui.label(kind.as_str());
                                        }
                                        if let (Some(width), Some(height)) = (width, height) {
                                            ui.label(format!("{width} × {height}"));
                                        }
                                        if let Some(samples) = samples_per_channel {
                                            ui.label(format!("{samples} samples per channel"));
                                        }
                                    }
                                }
                            });
                        }
                    });
            }
        });
    }

    pub(super) fn accept_stream_observations(&mut self, result: Value) -> Result<(), String> {
        let report: Report = serde_json::from_value(result).map_err(|error| error.to_string())?;
        if self
            .selected_asset()
            .is_some_and(|asset| asset.identity.sha256 == report.source_sha256)
            && self.observations.active.as_ref() == Some(&report.selection)
        {
            self.observations.report = Some(report);
        }
        Ok(())
    }
}
