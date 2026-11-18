use super::{Action, WizApp};
use eframe::egui;
use nw_domain::capabilities::Observation;
use serde_json::json;

impl WizApp {
    pub(super) fn capability_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Engine capabilities").show(ui, |ui| {
            let idle = self.request_idle();
            ui.horizontal(|ui| {
                if ui.add_enabled(idle, egui::Button::new("Inspect")).clicked() {
                    self.query(
                        "capabilities.inspect",
                        json!({}),
                        Action::Capabilities,
                        ui.ctx(),
                    );
                }
                if ui.add_enabled(idle, egui::Button::new("Refresh")).clicked() {
                    self.query(
                        "capabilities.refresh",
                        json!({"force":true}),
                        Action::Capabilities,
                        ui.ctx(),
                    );
                }
            });
            let Some(report) = &self.capability_report else {
                return;
            };
            ui.label(format!(
                "CPU FFV1 round trip: {}",
                label(report.cpu_round_trip)
            ));
            ui.label(format!("Hardware: {}", label(report.hardware)));
            ui.label(format!("Component fingerprint: {}", report.fingerprint));
            if let Some(advertised) = &report.advertised {
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (name, values) in [
                            ("Encoders", &advertised.encoders),
                            ("Decoders", &advertised.decoders),
                            ("Filters", &advertised.filters),
                            ("Demuxers", &advertised.demuxers),
                            ("Muxers", &advertised.muxers),
                            ("Pixel formats", &advertised.pixel_formats),
                            ("Sample formats", &advertised.sample_formats),
                            (
                                "Advertised hardware methods",
                                &advertised.hardware_accelerators,
                            ),
                        ] {
                            ui.collapsing(name, |ui| ui.label(values.join(", ")));
                        }
                    });
                if ui
                    .add_enabled(idle, egui::Button::new("Clear cache"))
                    .clicked()
                {
                    let params = json!({"namespace":"components","fingerprint":report.fingerprint});
                    self.query(
                        "capabilities.clear",
                        params,
                        Action::ClearCapabilities,
                        ui.ctx(),
                    );
                }
            }
        });
    }
}

fn label(observation: Observation) -> &'static str {
    match observation {
        Observation::Unknown => "Unknown",
        Observation::Unavailable => "Unavailable",
        Observation::Failed => "Failed",
        Observation::Exercised => "Exercised",
    }
}
