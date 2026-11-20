//! Review a source-clock packet interval through the shared copy plan.
use super::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    packet_cut::{self, ChapterPolicy, CutInput, SplitInput, SplitOutput},
    records::Asset,
    time::{Rational, TimeRange},
};

pub(super) struct CutForm {
    start: String,
    end: String,
    parts: Vec<SplitFields>,
}

#[derive(Default)]
struct SplitFields {
    start: String,
    end: String,
    output: String,
}

impl Default for CutForm {
    fn default() -> Self {
        Self {
            start: "0".into(),
            end: "1".into(),
            parts: vec![
                SplitFields {
                    start: "0".into(),
                    end: "1".into(),
                    ..Default::default()
                },
                SplitFields {
                    start: "1".into(),
                    end: "2".into(),
                    ..Default::default()
                },
            ],
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
    fn packet_split_panel(&mut self, ui: &mut egui::Ui, source: &Asset, idle: bool) {
        let mut review = false;
        let mut changed = false;
        ui.collapsing("Split into separate packet-copy files", |ui| {
            ui.add_enabled_ui(idle, |ui| {
                ui.small("Enter source times as integers or fractions. Each start must be an original keyframe. Chapters are dropped.");
                let mut remove = None;
                let removable = self.packet_cut.parts.len() > 2;
                for (index, part) in self.packet_cut.parts.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Start");
                            changed |= ui.add(egui::TextEdit::singleline(&mut part.start).desired_width(64.0)).changed();
                            ui.label("End (exclusive)");
                            changed |= ui.add(egui::TextEdit::singleline(&mut part.end).desired_width(64.0)).changed();
                            ui.label("Output .mkv");
                            changed |= ui.text_edit_singleline(&mut part.output).changed();
                            if ui.add_enabled(removable, egui::Button::new("Remove")).clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                }
                if let Some(index) = remove {
                    self.packet_cut.parts.remove(index);
                    changed = true;
                }
                if ui.add_enabled(self.packet_cut.parts.len() < 16, egui::Button::new("Add part")).clicked() {
                    let start = self.packet_cut.parts.last().map_or(String::new(), |part| part.end.clone());
                    self.packet_cut.parts.push(SplitFields { start, ..Default::default() });
                    changed = true;
                }
                review = ui.add_enabled(self.packet_cut.parts.iter().all(|part| !part.output.trim().is_empty()),
                    egui::Button::new("Review packet split")).clicked();
            });
        });
        if changed {
            self.invalidate_packet_split();
        }
        if review {
            let input = (|| {
                let outputs = self
                    .packet_cut
                    .parts
                    .iter()
                    .map(|part| {
                        Ok::<_, String>(SplitOutput {
                            output: part.output.trim().to_owned(),
                            range: TimeRange {
                                start: seconds(&part.start)?,
                                end: seconds(&part.end)?,
                            },
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let input = SplitInput {
                    asset_id: source.id.clone(),
                    outputs,
                    chapters: ChapterPolicy::Drop,
                };
                input.validate().map_err(|error| error.message)?;
                Ok::<_, String>(input)
            })();
            match input {
                Ok(input) => self.review_packet_split(input, ui.ctx()),
                Err(error) => self.error = Some(error),
            }
        }
    }

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
        self.packet_split_panel(ui, source, idle);
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
