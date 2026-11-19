//! Review a source-clock packet interval through the shared copy plan.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    packet_cut::{self, ChapterPolicy, CutInput},
    records::Asset,
    time::{Rational, TimeRange},
};

pub(super) struct CutForm {
    start: String,
    end: String,
}

impl Default for CutForm {
    fn default() -> Self {
        Self {
            start: "0".into(),
            end: "1".into(),
        }
    }
}

fn seconds(text: &str) -> Result<Rational, String> {
    let text = text.trim();
    let (num, den) = text.split_once('/').unwrap_or((text, "1"));
    Rational::new(
        num.parse()
            .map_err(|_| "Use an integer or fraction for the packet-cut time")?,
        den.parse()
            .map_err(|_| "Use a positive denominator for the packet-cut time")?,
    )
    .map_err(|error| error.message)
}

impl WizApp {
    pub(super) fn packet_cut_panel(&mut self, ui: &mut egui::Ui, source: &Asset) {
        if !source
            .probe
            .format
            .split(',')
            .any(|format| format == "matroska")
        {
            return;
        }
        let idle = self.request_idle();
        let mut changed = false;
        let mut review = false;
        ui.collapsing("Cut at original packet boundaries", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Original start (seconds)");
                    changed |= ui.add(egui::TextEdit::singleline(&mut self.packet_cut.start).desired_width(74.0)).changed();
                    ui.label("End (exclusive)");
                    changed |= ui.add(egui::TextEdit::singleline(&mut self.packet_cut.end).desired_width(74.0)).changed();
                });
                ui.small("Use integers or fractions and choose an original keyframe for the start. This command drops chapters.");
                review = ui.add_enabled(!self.output.trim().is_empty(), egui::Button::new("Review packet cut")).clicked();
            });
        });
        if changed || review {
            self.changed();
        }
        if !review {
            return;
        }
        let range = (|| {
            let range = TimeRange {
                start: seconds(&self.packet_cut.start)?,
                end: seconds(&self.packet_cut.end)?,
            };
            packet_cut::validate(range).map_err(|error| error.message)?;
            Ok::<_, String>(range)
        })();
        match range {
            Ok(range) => self.query(
                "media.cut",
                serde_json::json!(CutInput {
                    asset_id: source.id.clone(),
                    output: self.output.clone(),
                    range,
                    chapters: ChapterPolicy::Drop,
                }),
                Action::Plan(self.generation),
                ui.ctx(),
            ),
            Err(error) => self.error = Some(error),
        }
    }
}
