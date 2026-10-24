//! Human controls lower into the same typed cleanup intent as machine adapters.
use crate::{Action, WizApp};
use eframe::egui;
use nw_domain::{
    mux::{CleanIntent, NoMatch, Selector, StreamKind, StreamTagPatch, TagPatch},
    records::Asset,
};
use serde_json::json;

pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[derive(Default)]
pub struct PatchForm {
    kind: usize,
    indices: String,
    languages: String,
    language_mode: usize,
    language: String,
    title_mode: usize,
    title: String,
}
fn indices(text: &str) -> Result<Vec<u32>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(str::trim)
        .map(|value| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(format!("Invalid original stream index: {value}"));
            }
            value
                .parse::<u32>()
                .map_err(|_| format!("Invalid original stream index: {value}"))
        })
        .collect()
}
fn values(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}
fn selector(mode: usize, text: &str) -> Result<Selector, String> {
    Ok(match mode {
        0 => Selector::All,
        1 => Selector::None,
        2 => Selector::Languages {
            values: values(text),
        },
        3 => Selector::SourceIndices {
            values: indices(text)?,
        },
        _ => Selector::TitleContainsAny {
            values: text
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
        },
    })
}
fn selection(
    ui: &mut egui::Ui,
    id: &str,
    mode: &mut usize,
    text: &mut String,
    no_match: &mut NoMatch,
) -> bool {
    let mut changed = false;
    let choices = [
        "All tracks",
        "No tracks",
        "Exact language codes",
        "Original stream indices",
        "Title contains",
    ];
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(choices[*mode])
            .show_ui(ui, |ui| {
                for (index, label) in choices.iter().enumerate() {
                    changed |= ui.selectable_value(mode, index, *label).changed();
                }
            });
        if *mode > 1 {
            changed |=
                ui.add(egui::TextEdit::singleline(text).char_limit(4096).hint_text(
                    if *mode == 3 {
                        "Indices: 0, 2, 5"
                    } else if *mode == 2 {
                        "Codes: eng, fas, und"
                    } else {
                        "Original title contains"
                    },
                ))
                .changed();
        }
    });
    ui.horizontal(|ui| {
        ui.small("When the requested selection matches nothing:");
        egui::ComboBox::from_id_salt((id, "no-match"))
            .selected_text(match no_match {
                NoMatch::SkipFile => "Skip file",
                NoMatch::AllowEmpty => "Allow empty",
                NoMatch::Reject => "Reject",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(no_match, NoMatch::SkipFile, "Skip file")
                    .changed();
                changed |= ui
                    .selectable_value(no_match, NoMatch::AllowEmpty, "Allow empty")
                    .changed();
                changed |= ui
                    .selectable_value(no_match, NoMatch::Reject, "Reject")
                    .changed();
            });
    });
    changed
}
fn tag(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    mode: &mut usize,
    text: &mut String,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(["Keep", "Set", "Clear"][*mode])
            .show_ui(ui, |ui| {
                for (index, label) in ["Keep", "Set", "Clear"].iter().enumerate() {
                    changed |= ui.selectable_value(mode, index, *label).changed();
                }
            });
        if *mode == 1 {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(text)
                        .char_limit(4096)
                        .desired_width(200.0),
                )
                .changed();
        }
    });
    changed
}
fn patch(mode: usize, text: &str) -> TagPatch {
    match mode {
        1 => TagPatch::Set { value: text.into() },
        2 => TagPatch::Clear,
        _ => TagPatch::Keep,
    }
}
impl WizApp {
    fn normalized_intent(&self) -> Result<CleanIntent, String> {
        let mut intent = self.intent.clone();
        intent.audio_selection.selector = selector(self.audio_mode, &self.audio_values)?;
        intent.subtitle_selection.selector = selector(self.subtitle_mode, &self.subtitle_values)?;
        intent.audio_order = indices(&self.audio_order)?;
        intent.subtitle_order = indices(&self.subtitle_order)?;
        intent.stream_tag_patches = self
            .patches
            .iter()
            .map(|form| {
                Ok(StreamTagPatch {
                    kind: match form.kind {
                        0 => StreamKind::Audio,
                        1 => StreamKind::Subtitle,
                        2 => StreamKind::Video,
                        3 => StreamKind::Attachment,
                        4 => StreamKind::Data,
                        _ => StreamKind::Unknown,
                    },
                    source_indices: indices(&form.indices)?,
                    original_languages: values(&form.languages),
                    language: patch(form.language_mode, &form.language),
                    title: patch(form.title_mode, &form.title),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(intent)
    }
    pub(super) fn cleanup(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        ui.strong(if asset.display_name.is_empty() {
            file_name(&asset.identity.path)
        } else {
            &asset.display_name
        });
        egui::ScrollArea::vertical().id_salt("cleanup-form").show(ui,|ui| {
            egui::CollapsingHeader::new("Original stream inventory").default_open(true).show(ui,|ui| {
                let first = self.stream_page.saturating_mul(64);
                egui::Grid::new("original-streams").striped(true).show(ui,|ui| {
                    for label in ["Index","Kind","Codec","Language","Title"] {ui.strong(label);} ui.end_row();
                    for stream in asset.probe.streams.iter().skip(first).take(64) {
                        ui.monospace(stream.index.to_string());ui.label(format!("{:?}",stream.kind));ui.label(&stream.codec);ui.label(stream.tags.get("language").map_or("und",String::as_str));ui.label(stream.tags.get("title").map_or("",String::as_str));ui.end_row();
                    }
                });
                if asset.probe.streams.len() > 64 { ui.horizontal(|ui| {if ui.add_enabled(self.stream_page>0,egui::Button::new("Previous streams")).clicked(){self.stream_page-=1;}ui.label(format!("Page {}",self.stream_page+1));if ui.add_enabled(first+64<asset.probe.streams.len(),egui::Button::new("Next streams")).clicked(){self.stream_page+=1;}}); }
            });
            ui.separator();
            let mut changed = false;
            ui.strong("Audio selection"); changed |= selection(ui,"audio-selection",&mut self.audio_mode,&mut self.audio_values,&mut self.intent.audio_selection.no_match);
            ui.horizontal(|ui| {ui.label("Audio order");changed |= ui.add(egui::TextEdit::singleline(&mut self.audio_order).char_limit(4096).hint_text("Original indices; blank keeps original order")).changed();});
            ui.add_space(8.0);
            ui.strong("Subtitle selection"); changed |= selection(ui,"subtitle-selection",&mut self.subtitle_mode,&mut self.subtitle_values,&mut self.intent.subtitle_selection.no_match);
            ui.horizontal(|ui| {ui.label("Subtitle order");changed |= ui.add(egui::TextEdit::singleline(&mut self.subtitle_order).char_limit(4096).hint_text("Original indices; blank keeps original order")).changed();});
            egui::CollapsingHeader::new("Preservation").default_open(true).show(ui,|ui| {
                ui.horizontal_wrapped(|ui| {
                    changed |= ui.checkbox(&mut self.intent.preservation.additional_video,"Additional video").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.attached_pictures,"Cover pictures").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.attachments,"Attachments").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.data,"Data streams").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.unknown,"Unknown streams").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.global_tags,"Global tags").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.stream_tags,"Stream tags").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.dispositions,"Dispositions").changed();
                    changed |= ui.checkbox(&mut self.intent.preservation.chapters,"Chapters").changed();
                    ui.label("File timestamps");
                    let label = match self.file_times {
                        nw_domain::file_times::FileTimeRequest::New => "New timestamps",
                        nw_domain::file_times::FileTimeRequest::PreserveBestEffort => "Preserve timestamps",
                        nw_domain::file_times::FileTimeRequest::RequireReviewed => "Require reviewed timestamps",
                    };
                    egui::ComboBox::from_id_salt("cleanup-file-times").selected_text(label).show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut self.file_times, nw_domain::file_times::FileTimeRequest::PreserveBestEffort, "Preserve timestamps").changed();
                        changed |= ui.selectable_value(&mut self.file_times, nw_domain::file_times::FileTimeRequest::RequireReviewed, "Require reviewed timestamps").changed();
                        changed |= ui.selectable_value(&mut self.file_times, nw_domain::file_times::FileTimeRequest::New, "New timestamps").changed();
                    }).response.on_hover_text("Preserve source access and modification times where supported. Required mode restores the reviewed values before publication; later file reads can change access time.");
                });
            });
            egui::CollapsingHeader::new("Language and title edits").show(ui,|ui| {
                ui.small("Rules match the original inventory. Later explicit field edits take precedence.");
                let mut remove = None;
                for (index,form) in self.patches.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(format!("Rule {}",index+1));
                            let kinds = ["Audio","Subtitle","Video","Attachment","Data","Unknown"];
                            egui::ComboBox::from_id_salt(("patch-kind",index)).selected_text(kinds[form.kind]).show_ui(ui,|ui| { for (kind, label) in kinds.iter().enumerate() { changed |= ui.selectable_value(&mut form.kind,kind,*label).changed(); } });
                            if ui.small_button("Remove").clicked(){remove=Some(index);}
                        });
                        ui.horizontal(|ui| {ui.label("Original indices");changed |= ui.add(egui::TextEdit::singleline(&mut form.indices).hint_text("Blank means all matching tracks").char_limit(4096)).changed();});
                        ui.horizontal(|ui| {ui.label("Original languages");changed |= ui.add(egui::TextEdit::singleline(&mut form.languages).hint_text("Optional: eng, fas, und").char_limit(4096)).changed();});
                        changed |= tag(ui,("patch-language",index),"Language",&mut form.language_mode,&mut form.language);
                        changed |= tag(ui,("patch-title",index),"Title",&mut form.title_mode,&mut form.title);
                    });
                }
                if let Some(index) = remove {self.patches.remove(index);changed=true;}
                if ui.add_enabled(self.patches.len()<256,egui::Button::new("Add tag rule")).clicked(){self.patches.push(PatchForm::default());changed=true;}
            });
            ui.separator();
            ui.horizontal(|ui| {ui.label("Output");changed |= ui.add(egui::TextEdit::singleline(&mut self.output).desired_width(f32::INFINITY).char_limit(16384)).changed();});
            ui.small("An existing destination is preserved. Changed layouts require an admitted preservation profile for the selected output container.");
            if changed {self.changed();}
            if ui.add_enabled(!self.waiting && self.pending.is_none() && !self.output.trim().is_empty(),egui::Button::new("Review cleanup plan")).clicked() {
                match self.normalized_intent() { Ok(intent) => self.query("streams.clean",json!({"asset_id":asset.id,"output":self.output,"intent":intent,"file_times":self.file_times}),Action::Plan(self.generation),ui.ctx()), Err(error) => self.error=Some(error) }
            }
            if ui.add_enabled(!self.waiting && self.pending.is_none(), egui::Button::new("Selection report")).clicked() {
                match self.normalized_intent() {
                    Ok(intent) => self.query("streams.clean.report", json!({"asset_ids":[asset.id],"intent":intent}), Action::SelectionReport(self.generation), ui.ctx()),
                    Err(error) => self.error = Some(error),
                }
            }
        });
    }
    pub(super) fn plan_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Plan review");
        let Some(plan) = self.plan.clone() else {
            ui.label("Prepare a plan after selecting streams and an output. Running a job requires this review.");
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("plan-review")
            .show(ui, |ui| {
                ui.strong(format!("{:?}", plan.resolution.action));
                ui.label("Source");
                ui.label(
                    plan.source_path
                        .as_deref()
                        .unwrap_or(&plan.source.display_name),
                );
                ui.label("Destination");
                ui.label(plan.output_path.as_deref().unwrap_or(&plan.output_name));
                ui.separator();
                for outcome in &plan.resolution.outcomes {
                    ui.label(match outcome.output_index {
                        Some(index) => {
                            format!("Source {} → output {}", outcome.source_index, index)
                        }
                        None => format!("Source {} removed", outcome.source_index),
                    });
                    ui.small(&outcome.reason);
                }
                for warning in &plan.resolution.warnings {
                    ui.colored_label(egui::Color32::from_rgb(237, 177, 99), warning);
                }
                ui.separator();
                ui.label(&plan.verification_scope);
                egui::CollapsingHeader::new("Invocation and identity").show(ui, |ui| {
                    if let Some(policy) = &plan.file_times {
                        let label = match policy.request() {
                            nw_domain::file_times::FileTimeRequest::New => "New output timestamps",
                            nw_domain::file_times::FileTimeRequest::PreserveBestEffort => {
                                "Preserve source timestamps where supported"
                            }
                            nw_domain::file_times::FileTimeRequest::RequireReviewed => {
                                "Require the reviewed source timestamps"
                            }
                        };
                        ui.small(label);
                    }
                    ui.small(format!("Plan {}", plan.id));
                    ui.monospace(&plan.hash);
                    ui.small(format!("Source SHA-256: {}", plan.source_sha256));
                    ui.small(format!(
                        "{} SHA-256: {}",
                        plan.ffmpeg.file_name, plan.ffmpeg.sha256
                    ));
                    ui.small(format!(
                        "{} SHA-256: {}",
                        plan.ffprobe.file_name, plan.ffprobe.sha256
                    ));
                    for argument in plan.argv_template.iter().flatten() {
                        ui.monospace(argument);
                    }
                });
                if ui
                    .add_enabled(
                        !self.waiting && self.pending.is_none(),
                        egui::Button::new("Run reviewed plan"),
                    )
                    .clicked()
                {
                    self.run_reviewed(ui.ctx());
                }
                ui.small("Ctrl/Cmd+Enter runs the reviewed plan.");
            });
    }
}

impl WizApp {
    pub(super) fn selection_report_window(&mut self, ctx: &egui::Context) {
        let Some(report) = self.selection_report.clone() else {
            return;
        };
        let mut open =
            !ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new("Selection report")
            .open(&mut open)
            .default_width(720.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Files: {} | Ready: {} | Skipped: {} | Rejected: {}",
                    report.files.len(),
                    report.resolved_files,
                    report.skipped_files,
                    report.rejected_files,
                ));
                ui.small(
                    "Source inventory and selection preview. Review an output plan before running.",
                );
                ui.horizontal(|ui| {
                    let unique = ui
                        .selectable_value(&mut self.report_by_index, false, "Unique streams")
                        .clicked();
                    let indices = ui
                        .selectable_value(&mut self.report_by_index, true, "Original indices")
                        .clicked();
                    if unique || indices {
                        self.report_page = 0;
                    }
                });
                let rows = if self.report_by_index {
                    &report.original_index_streams
                } else {
                    &report.unique_streams
                };
                let pages = rows.len().div_ceil(32).max(1);
                self.report_page = self.report_page.min(pages - 1);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.report_page > 0, egui::Button::new("Previous"))
                        .clicked()
                    {
                        self.report_page -= 1;
                    }
                    ui.label(format!("Page {} of {pages}", self.report_page + 1));
                    if ui
                        .add_enabled(self.report_page + 1 < pages, egui::Button::new("Next"))
                        .clicked()
                    {
                        self.report_page += 1;
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt("selection-report-rows")
                    .max_height(360.0)
                    .show(ui, |ui| {
                        egui::Grid::new("selection-report-summary")
                            .striped(true)
                            .show(ui, |ui| {
                                for label in [
                                    "Kind", "Index", "Language", "Codec", "Source", "Retained",
                                    "Title",
                                ] {
                                    ui.strong(label);
                                }
                                ui.end_row();
                                for row in rows.iter().skip(self.report_page * 32).take(32) {
                                    ui.label(if row.attached_picture {
                                        "Cover".into()
                                    } else {
                                        format!("{:?}", row.kind)
                                    });
                                    ui.label(
                                        row.source_index.map_or_else(
                                            || "All".into(),
                                            |index| index.to_string(),
                                        ),
                                    );
                                    ui.label(&row.language);
                                    ui.label(&row.codec);
                                    ui.label(row.source_count.to_string());
                                    ui.label(row.retained_count.to_string());
                                    ui.add(egui::Label::new(&row.title).truncate())
                                        .on_hover_text(&row.title);
                                    ui.end_row();
                                }
                            });
                        ui.separator();
                        for file in &report.files {
                            ui.push_id(&file.asset_id, |ui| {
                                ui.collapsing(&file.display_name, |ui| {
                                    let action = match file.action {
                                        Some(nw_domain::mux::CleanAction::CopyUnchanged) => {
                                            "Unchanged copy"
                                        }
                                        Some(nw_domain::mux::CleanAction::Remux) => {
                                            "Stream or metadata change"
                                        }
                                        Some(nw_domain::mux::CleanAction::Skip) => {
                                            "Skipped by selection policy"
                                        }
                                        None => "Rejected",
                                    };
                                    ui.label(action);
                                    if let (Some(audio), Some(subtitles)) =
                                        (file.matched_audio_count, file.matched_subtitle_count)
                                    {
                                        ui.label(format!(
                                            "Matching audio: {audio}; subtitles: {subtitles}"
                                        ));
                                    }
                                    ui.label(format!(
                                        "Retained original indices: {:?}",
                                        file.selected_original_indices
                                    ));
                                    for warning in &file.warnings {
                                        ui.label(warning);
                                    }
                                    if let Some(error) = &file.error {
                                        ui.label(&error.message);
                                    }
                                });
                            });
                        }
                    });
            });
        if !open {
            self.selection_report = None;
        }
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    #[test]
    fn malformed_index_csv_never_executes_a_valid_subset() {
        assert_eq!(indices("0, 2").unwrap(), vec![0, 2]);
        assert!(indices("  ").unwrap().is_empty());
        for value in ["0,,2", "0,2,", ",0", "-1", "+1", "1.5", "1e3", "4294967296"] {
            assert!(indices(value).is_err(), "{value}");
        }
    }
}
