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
    text.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
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
                    kind: if form.kind == 0 {
                        StreamKind::Audio
                    } else {
                        StreamKind::Subtitle
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
                });
            });
            egui::CollapsingHeader::new("Language and title edits").show(ui,|ui| {
                ui.small("Rules match the original inventory. Later explicit field edits take precedence.");
                let mut remove = None;
                for (index,form) in self.patches.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(format!("Rule {}",index+1));
                            egui::ComboBox::from_id_salt(("patch-kind",index)).selected_text(if form.kind == 0 {"Audio"} else {"Subtitle"}).show_ui(ui,|ui| {changed |= ui.selectable_value(&mut form.kind,0,"Audio").changed();changed |= ui.selectable_value(&mut form.kind,1,"Subtitle").changed();});
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
            ui.small("An existing destination is preserved. Changed stream layouts currently require an admitted Matroska output.");
            if changed {self.changed();}
            if ui.add_enabled(!self.waiting && self.pending.is_none() && !self.output.trim().is_empty(),egui::Button::new("Review cleanup plan")).clicked() {
                match self.normalized_intent() { Ok(intent) => self.query("streams.clean",json!({"asset_id":asset.id,"output":self.output,"intent":intent}),Action::Plan(self.generation),ui.ctx()), Err(error) => self.error=Some(error) }
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
                    ui.small(format!("Plan {}", plan.id));
                    ui.monospace(&plan.hash);
                    ui.small(format!("Source SHA-256: {}", plan.source_sha256));
                    ui.small(format!("{} SHA-256: {}", plan.ffmpeg.file_name, plan.ffmpeg.sha256));
                    ui.small(format!("{} SHA-256: {}", plan.ffprobe.file_name, plan.ffprobe.sha256));
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
