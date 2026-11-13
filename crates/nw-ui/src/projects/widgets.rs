//! Native and browser share these controls; projections, never optimistic edits, draw the view.
use super::forms::ExactForm;
use super::*;

fn exact(value: Rational) -> String {
    format!("{}/{}", value.numerator(), value.denominator())
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    ui.label(label);
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .char_limit(20)
            .desired_width(width),
    );
    if response.gained_focus() {
        response.scroll_to_me(Some(egui::Align::Center));
    }
}
fn rational(ui: &mut egui::Ui, label: &str, value: &mut ExactForm) {
    ui.horizontal(|ui| {
        number(ui, label, &mut value.num, 70.0);
        ui.label("/");
        number(ui, "", &mut value.den, 70.0);
    });
}
fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
fn page_label<T>(page: &Page<T>) -> String {
    format!(
        "{} rows shown · {} {}",
        page.items.len(),
        page.total,
        if page.accuracy == "exact" {
            "total"
        } else {
            "or more"
        }
    )
}
impl WizApp {
    pub(crate) fn project_workspace(&mut self, ui: &mut egui::Ui) {
        let escape = ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            if self.projects.sequence_form || self.projects.source_form {
                self.projects.sequence_form = false;
                self.projects.source_form = false;
            } else if !ui.ctx().text_edit_focused() && self.request_idle() {
                if self.projects.group.is_some() {
                    self.project_navigate(Navigation::Catalog, ui.ctx());
                } else {
                    self.leave_projects(ui.ctx());
                }
            }
        }
        if self.projects.group.is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.project_catalog_ui(ui));
        } else {
            egui::Panel::bottom("project-status")
                .default_size(96.0)
                .show(ui, |ui| self.project_status(ui));
            egui::Panel::left("project-bins")
                .default_size(250.0)
                .resizable(true)
                .show(ui, |ui| self.project_bins(ui));
            egui::Panel::right("project-details")
                .default_size(350.0)
                .resizable(true)
                .show(ui, |ui| self.project_details(ui));
            egui::CentralPanel::default().show(ui, |ui| self.project_timeline(ui));
        }
        self.project_sequence_window(ui.ctx());
        self.project_source_window(ui.ctx());
    }
    fn project_status(&mut self, ui: &mut egui::Ui) {
        ui.label(if self.waiting {
            "Working with the project owner…"
        } else {
            &self.status
        });
        if let Some(accepted) = &self.projects.acceptance {
            ui.small(accepted);
        }
        if self.projects.stale && self.projects.group.is_some() {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Stale captured view — Refresh before another edit.",
            );
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(237, 143, 112), error);
        }
        if !self.waiting && self.pending.is_some() && ui.button("Retry exact request").clicked() {
            self.retry_pending(ui.ctx());
        }
        if self.projects.terminal_failure && !self.projects.handles.is_empty() {
            ui.small(format!(
                "{} captured handle(s): retirement unconfirmed. Cleanup remains available.",
                self.projects.handles.len()
            ));
        }
        ui.small("Accepted cleanup jobs continue in the background.");
    }
    fn project_catalog_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Projects");
        ui.label("Open a readable project or create a named project.");
        let idle = self.request_idle()
            && self.projects.navigation.is_none()
            && self.projects.staging.is_none();
        ui.add_enabled_ui(idle, |ui| {
            ui.horizontal(|ui| {
                ui.label("Project name");
                let name = ui.add(
                    egui::TextEdit::singleline(&mut self.projects.forms.project_name)
                        .id(egui::Id::new("project-name"))
                        .char_limit(128)
                        .desired_width(320.0),
                );
                if self.projects.focus_catalog {
                    name.request_focus();
                    self.projects.focus_catalog = false;
                }
                if ui.button("Create project").clicked() {
                    self.create_project(ui.ctx());
                }
                if ui.button("Refresh projects").clicked() {
                    self.project_navigate(Navigation::Catalog, ui.ctx());
                }
            });
        });
        ui.separator();
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("project-catalog")
            .max_height((ui.available_height() - 160.0).max(100.0))
            .show(ui, |ui| {
                for project in &self.projects.catalog {
                    ui.group(|ui| {
                        if ui
                            .add_enabled(idle, egui::Button::new(format!("Open {}", project.name)))
                            .clicked()
                        {
                            open = Some(project.id.to_string());
                        }
                        ui.small(format!(
                            "{} · revision {} · schema {} / envelope {}",
                            project.id,
                            project.revision.0,
                            project.schema_major,
                            project.envelope_version
                        ));
                        if matches!(
                            project.compatibility,
                            nw_domain::projects::views::ProjectCompatibility::ReadOnlyUnsupported
                        ) {
                            ui.label("Common header readable; content format unsupported.");
                        }
                    });
                }
                if self.projects.catalog.is_empty() {
                    ui.label("No readable projects in this catalog.");
                }
            });
        if let Some(id) = open {
            self.project_navigate(Navigation::Open(View::new(id)), ui.ctx());
        }
        ui.separator();
        self.project_status(ui);
    }
    fn project_bins(&mut self, ui: &mut egui::Ui) {
        let Some(group) = self.projects.group.clone() else {
            return;
        };
        let idle = self.request_idle();
        let ready = self.project_ready();
        ui.heading(group.summary["name"].as_str().unwrap_or("Project"));
        ui.small(format!("Revision {}", group.key.revision));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("Projects catalog"))
                .clicked()
            {
                self.project_navigate(Navigation::Catalog, ui.ctx());
            }
            if ui.add_enabled(idle, egui::Button::new("Refresh")).clicked() {
                self.project_refresh(ui.ctx());
            }
        });
        ui.separator();
        ui.strong("Bins and sequences");
        ui.small(page_label(&group.bins));
        if ui
            .add_enabled(ready, egui::Button::new("Root bin"))
            .clicked()
        {
            let mut view = group.view.clone();
            view.bin = None;
            self.project_reselect(view, ui.ctx());
        }
        let mut selection = None;
        egui::ScrollArea::vertical()
            .id_salt("project-bin-page")
            .max_height(200.0)
            .show(ui, |ui| {
                for item in &group.bins.items {
                    let active = Some(item.id.as_str()) == group.view.sequence.as_deref();
                    if ui
                        .add_enabled(
                            ready && matches!(item.kind.as_str(), "bin" | "sequence"),
                            egui::Button::new(format!("{} · {}", item.name, item.kind))
                                .selected(active),
                        )
                        .clicked()
                    {
                        selection = Some((item.id.to_string(), item.kind.clone()));
                    }
                    ui.small(item.id.as_str());
                }
            });
        if let Some((id, kind)) = selection {
            let mut view = group.view.clone();
            if kind == "bin" {
                view.bin = Some(id);
            } else {
                view.sequence = Some(id);
                view.track = None;
                view.clip = None;
            }
            self.project_reselect(view, ui.ctx());
        }
        if group.bins.next.is_some()
            && ui
                .add_enabled(ready, egui::Button::new("Next bin page"))
                .clicked()
        {
            self.project_next_page(Phase::NextBin, ui.ctx());
        }
        if ui
            .add_enabled(ready, egui::Button::new("New sequence"))
            .clicked()
        {
            self.projects.source_form = false;
            self.projects.sequence_form = true;
            self.projects.focus_sequence = true;
        }
        ui.separator();
        ui.strong("Tracks");
        ui.small(page_label(&group.tracks));
        let mut selected_track = None;
        egui::ScrollArea::vertical()
            .id_salt("project-track-page")
            .show(ui, |ui| {
                for track in &group.tracks.items {
                    if ui
                        .add_enabled(
                            ready,
                            egui::Button::new(format!("{} · {:?}", track.name, track.kind))
                                .selected(Some(track.id.as_str()) == group.view.track.as_deref()),
                        )
                        .clicked()
                    {
                        selected_track = Some(track.id.to_string());
                    }
                    ui.small(format!("{} · {} clips", track.id, track.clip_count));
                }
            });
        if let Some(track) = selected_track {
            let mut view = group.view.clone();
            view.track = Some(track);
            view.clip = None;
            self.project_reselect(view, ui.ctx());
        }
        if group.tracks.next.is_some()
            && ui
                .add_enabled(ready, egui::Button::new("Next tracks page"))
                .clicked()
        {
            self.project_next_page(Phase::NextTracks, ui.ctx());
        }
    }
    fn project_timeline(&mut self, ui: &mut egui::Ui) {
        let Some(group) = self.projects.group.clone() else {
            return;
        };
        let ready = self.project_ready();
        ui.heading("Sequence");
        if let Some(sequence) = &group.sequence {
            ui.label(format!(
                "{} × {} · {} fps · {} Hz · {}",
                sequence.settings.width,
                sequence.settings.height,
                exact(sequence.settings.frame_rate),
                sequence.settings.sample_rate,
                sequence.settings.audio_layout
            ));
        } else {
            ui.label("Create or select a sequence to work with tracks and clips.");
        }
        if let Some(track) = &group.track {
            ui.label(format!(
                "Selected {:?} track · grid rate {} · {} clips",
                track.kind,
                exact(track.grid),
                track.clip_count
            ));
        }
        ui.add_enabled_ui(ready, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        group.sequence.is_some() && group.track.is_some(),
                        egui::Button::new("Place source…"),
                    )
                    .clicked()
                {
                    self.projects.sequence_form = false;
                    self.projects.source_form = true;
                }
                if ui
                    .add_enabled(
                        group.summary["history"]["can_undo"] == true,
                        egui::Button::new("Undo"),
                    )
                    .clicked()
                {
                    self.mutate_project("history.undo", json!({}), ui.ctx());
                }
                if ui
                    .add_enabled(
                        group.summary["history"]["can_redo"] == true,
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                {
                    self.mutate_project("history.redo", json!({}), ui.ctx());
                }
            });
            ui.collapsing("Visible sequence range (exact seconds)", |ui| {
                rational(ui, "Start", &mut self.projects.forms.viewport_start);
                rational(ui, "End", &mut self.projects.forms.viewport_end);
                if ui.button("Apply visible range").clicked() {
                    match self.projects.forms.range() {
                        Ok(range) => {
                            let mut view = group.view.clone();
                            view.range = range;
                            view.clip = None;
                            self.project_reselect(view, ui.ctx());
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            });
        });
        ui.separator();
        ui.label(format!(
            "Captured range [{}, {}) seconds",
            exact(group.view.range.start),
            exact(group.view.range.end)
        ));
        ui.small(page_label(&group.viewport));
        let start =
            group.view.range.start.numerator() as f64 / group.view.range.start.denominator() as f64;
        let end =
            group.view.range.end.numerator() as f64 / group.view.range.end.denominator() as f64;
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("project-viewport")
            .show(ui, |ui| {
                for clip in &group.viewport.items {
                    let Ok(clip_end) = clip.sequence_start.checked_add(clip.duration) else {
                        ui.label("Invalid projected interval; Refresh required.");
                        continue;
                    };
                    let active = Some(clip.id.as_str()) == group.view.clip.as_deref();
                    let label = format!(
                        "Clip {} · [{}, {})",
                        short(clip.id.as_str()),
                        exact(clip.sequence_start),
                        exact(clip_end)
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(ready, egui::Button::new(&label).selected(active))
                            .clicked()
                        {
                            selected = Some(clip.id.to_string());
                        }
                        // Keep the original row height and inspection target. The
                        // compact seed control is separate from primary inspection.
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let mut seed = self.projects.clip_selection.as_ref().is_some_and(
                                    |selection| {
                                        selection.matches(&group)
                                            && selection.seeds.contains(clip.id.as_str())
                                    },
                                );
                                if ui
                                    .add_enabled(ready, egui::Checkbox::new(&mut seed, "Select"))
                                    .changed()
                                {
                                    self.project_toggle_clip_seed(clip.id.as_str());
                                }
                            },
                        );
                    });
                    ui.small(clip.id.as_str());
                    let (response, painter) = ui.allocate_painter(
                        egui::vec2(ui.available_width(), 34.0),
                        egui::Sense::hover(),
                    );
                    painter.rect_filled(
                        response.rect,
                        egui::CornerRadius::same(2),
                        egui::Color32::from_rgb(14, 14, 14),
                    );
                    let a = clip.sequence_start.numerator() as f64
                        / clip.sequence_start.denominator() as f64;
                    let b = clip_end.numerator() as f64 / clip_end.denominator() as f64;
                    let x = |time: f64| {
                        response.rect.left()
                            + (((time - start) / (end - start)).clamp(0.0, 1.0) as f32)
                                * response.rect.width()
                    };
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(x(a), response.rect.top() + 3.0),
                        egui::pos2(x(b).max(x(a) + 2.0), response.rect.bottom() - 3.0),
                    );
                    painter.rect_filled(
                        rect,
                        egui::CornerRadius::same(3),
                        if active {
                            egui::Color32::from_rgb(47, 107, 223)
                        } else {
                            egui::Color32::from_rgb(66, 86, 115)
                        },
                    );
                    let hit = ui.interact(
                        rect,
                        ui.id().with(("project-clip", clip.id.as_str())),
                        egui::Sense::click(),
                    );
                    if ready && hit.clicked() {
                        selected = Some(clip.id.to_string());
                    }
                    if let Some(track) = &group.track
                        && let Ok(tick) =
                            nw_domain::projects::grid_tick(clip.sequence_start, track.grid)
                    {
                        ui.small(format!("Start tick {tick} at rate {}", exact(track.grid)));
                    }
                }
                if group.viewport.items.is_empty() {
                    ui.label("No clips in this captured track range.");
                }
            });
        if let Some(clip) = selected {
            let mut view = group.view.clone();
            view.clip = Some(clip);
            self.project_reselect(view, ui.ctx());
        }
        if group.viewport.next.is_some()
            && ui
                .add_enabled(ready, egui::Button::new("Next viewport page"))
                .clicked()
        {
            self.project_next_page(Phase::NextViewport, ui.ctx());
        }
    }
    fn project_details(&mut self, ui: &mut egui::Ui) {
        let Some(group) = self.projects.group.clone() else {
            return;
        };
        let ready = self.project_ready();
        ui.heading("Selected detail");
        ui.label(format!("Project revision {}", group.key.revision));
        ui.small(format!(
            "Schema: {}",
            group.summary["compatibility"].as_str().unwrap_or("unknown")
        ));
        ui.small(format!(
            "Writer: {}",
            group.summary["writer_status"].as_str().unwrap_or("unknown")
        ));
        ui.small("Each command is checked against your current access profile.");
        ui.separator();
        let Some(clip) = &group.clip else {
            ui.label("Select a visible clip to inspect exact time and source details.");
            self.project_link_controls(ui, &group);
            return;
        };
        ui.monospace(group.view.clip.as_deref().unwrap_or(""));
        if !group
            .viewport
            .items
            .iter()
            .any(|row| Some(row.id.as_str()) == group.view.clip.as_deref())
        {
            ui.small("Selected clip is outside the current viewport page; these controls retain its exact ID.");
        }
        ui.label(format!("Sequence start: {} s", exact(clip.sequence_start)));
        ui.label(format!("Duration: {} s", exact(clip.duration)));
        ui.label(format!(
            "Source range: [{}, {}) s",
            exact(clip.source_range.start),
            exact(clip.source_range.end)
        ));
        if let Some(source) = &group.source {
            ui.label(format!(
                "Source: {}",
                source["source_kind"].as_str().unwrap_or("opaque")
            ));
            ui.small(source["id"].as_str().unwrap_or(""));
            if let Some(asset) = source["asset_id"].as_str() {
                ui.label(format!("Registered asset: {asset}"));
                ui.label(format!("Physical stream: {}", source["stream_index"]));
            }
        }
        ui.separator();
        ui.add_enabled_ui(ready,|ui| {
            rational(ui,"Move by (seconds)",&mut self.projects.forms.delta);
            if ui.button("Move selected clip").clicked() {
                match self.projects.forms.delta.value() {
                    Ok(delta)=>self.mutate_project("clips.move",json!({"clip_ids":[group.view.clip],"delta":delta,"collision":"reject","link_policy":"expand"}),ui.ctx()),
                    Err(error)=>self.error=Some(error),
                }
            }
            rational(ui,"Split at (sequence seconds)",&mut self.projects.forms.split);
            if ui.button("Split selected clip").clicked() {
                match self.projects.forms.split.value() {
                    Ok(at)=>self.mutate_project("clips.split",json!({"clip_id":group.view.clip,"at_sequence":at,"link_policy":"expand"}),ui.ctx()),
                    Err(error)=>self.error=Some(error),
                }
            }
            if ui.button("Lift selected clip").clicked() { self.mutate_project("clips.remove",json!({"clip_ids":[group.view.clip],"mode":"lift","link_policy":"expand"}),ui.ctx()); }
        });
        self.project_link_controls(ui, &group);
    }
    fn project_link_controls(&mut self, ui: &mut egui::Ui, group: &Group) {
        ui.separator();
        let count = self
            .projects
            .clip_selection
            .as_ref()
            .filter(|selection| selection.matches(group))
            .map_or(0, |selection| selection.seeds.len());
        ui.label(format!("Group selection: {count} / 128 clips"));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.project_ready() && count != 0,
                    egui::Button::new("Clear selection"),
                )
                .clicked()
            {
                self.projects.clip_selection = None;
            }
            if ui
                .add_enabled(
                    self.project_link_params(false).is_some(),
                    egui::Button::new("Link"),
                )
                .clicked()
            {
                self.project_link_selection(false, ui.ctx());
            }
            if ui
                .add_enabled(
                    self.project_link_params(true).is_some(),
                    egui::Button::new("Unlink"),
                )
                .clicked()
            {
                self.project_link_selection(true, ui.ctx());
            }
        });
        ui.small("Select clips beside their rows. Link needs two selections. Unlink uses selections or the inspected linked clip. Existing linked companions are included, even on other pages or tracks.");
        if let Some(clip) = &group.clip {
            if let Some(id) = &clip.link_group_id {
                ui.small(format!("Inspected group: {id}"));
            } else {
                ui.small("Inspected clip has no group recorded.");
            }
        }
    }
    fn project_sequence_window(&mut self, ctx: &egui::Context) {
        if !self.projects.sequence_form {
            return;
        }
        let mut open = true;
        egui::Window::new("New sequence")
            .order(egui::Order::Foreground)
            .collapsible(false)
            .default_pos(egui::pos2(280.0, 100.0))
            .default_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
                let ready = self.project_ready();
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(ready, |ui| {
                            ui.label("Sequence name");
                            let name = ui.add(
                                egui::TextEdit::singleline(&mut self.projects.forms.sequence_name)
                                    .id(egui::Id::new("project-sequence-name"))
                                    .char_limit(128)
                                    .desired_width(f32::INFINITY),
                            );
                            if self.projects.focus_sequence {
                                name.request_focus();
                                self.projects.focus_sequence = false;
                            }
                            ui.horizontal(|ui| {
                                number(ui, "Width", &mut self.projects.forms.width, 90.0);
                                number(ui, "Height", &mut self.projects.forms.height, 90.0);
                            });
                            rational(ui, "Frame rate", &mut self.projects.forms.frame_rate);
                            ui.horizontal(|ui| {
                                number(
                                    ui,
                                    "Sample rate",
                                    &mut self.projects.forms.sample_rate,
                                    100.0,
                                );
                                egui::ComboBox::from_id_salt("project-audio-layout")
                                    .selected_text(&self.projects.forms.audio_layout)
                                    .show_ui(ui, |ui| {
                                        for layout in
                                            ["mono", "stereo", "2.1", "3.0", "4.0", "5.1", "7.1"]
                                        {
                                            ui.selectable_value(
                                                &mut self.projects.forms.audio_layout,
                                                layout.into(),
                                                layout,
                                            );
                                        }
                                    });
                            });
                            ui.label("Video track names — one per line");
                            ui.add(
                                egui::TextEdit::multiline(&mut self.projects.forms.video_tracks)
                                    .desired_rows(3)
                                    .char_limit(8192)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.label("Audio track names — one per line");
                            ui.add(
                                egui::TextEdit::multiline(&mut self.projects.forms.audio_tracks)
                                    .desired_rows(2)
                                    .char_limit(8192)
                                    .desired_width(f32::INFINITY),
                            );
                            if ui.button("Create sequence").clicked() {
                                match self.projects.forms.sequence() {
                                    Ok(params) => {
                                        self.mutate_project("sequences.create", params, ui.ctx())
                                    }
                                    Err(error) => self.error = Some(error),
                                }
                            }
                        });
                        self.project_status(ui);
                    });
            });
        self.projects.sequence_form = open;
    }
    fn project_source_window(&mut self, ctx: &egui::Context) {
        if !self.projects.source_form {
            return;
        }
        let mut open = true;
        egui::Window::new("Place source").order(egui::Order::Foreground).collapsible(false).default_pos(egui::pos2(270.0,90.0)).default_width(650.0).open(&mut open).show(ctx,|ui| {
            let idle=self.request_idle();
            egui::ScrollArea::vertical().id_salt("project-place-source").max_height(560.0).show(ui,|ui| {
                ui.add_enabled_ui(idle,|ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.projects.forms.registered,false,"Finite solid");
                        ui.selectable_value(&mut self.projects.forms.registered,true,"Registered media");
                    });
                    if self.projects.forms.registered {
                        ui.label("Use a source already registered in Cleanup under this session.");
                        if ui.button("Refresh registered sources").clicked() {
                            self.query("assets.list",json!({"limit":PAGE,"replace_snapshot":self.projects.assets_snapshot}),self.project_action(Phase::Assets),ui.ctx());
                        }
                        let mut selected=None;
                        for asset in &self.projects.assets {
                            if ui.button(format!("Inspect source {}",asset.display_name)).clicked() { selected=Some(asset.id.clone()); }
                        }
                        if let Some(id)=selected { self.query("assets.inspect",json!({"id":id}),self.project_action(Phase::Asset),ui.ctx()); }
                        if self.projects.assets_cursor.is_some() && ui.button("Next sources page").clicked() {
                            self.query("assets.list",json!({"limit":PAGE,"cursor":self.projects.assets_cursor}),self.project_action(Phase::Assets),ui.ctx());
                        }
                        if let Some(asset)=self.projects.asset.clone() {
                            ui.label(format!("Source: {}",asset.display_name)); ui.small(&asset.id);
                            ui.label("Physical source stream");
                            for stream in &asset.probe.streams {
                                if matches!(stream.kind,nw_domain::mux::StreamKind::Video|nw_domain::mux::StreamKind::Audio) && !stream.attached_picture() {
                                    ui.selectable_value(&mut self.projects.stream,Some(stream.index),format!("#{} · {:?} · {}",stream.index,stream.kind,stream.codec));
                                }
                            }
                        }
                    } else {
                        ui.label("RGBA — four finite channels between zero and one");
                        ui.add(egui::TextEdit::singleline(&mut self.projects.forms.rgba).char_limit(128).desired_width(320.0));
                        rational(ui,"Finite source duration (seconds)",&mut self.projects.forms.duration);
                    }
                    rational(ui,"Source start (seconds)",&mut self.projects.forms.source_start);
                    rational(ui,"Source end (seconds)",&mut self.projects.forms.source_end);
                    rational(ui,"Place at (sequence seconds)",&mut self.projects.forms.sequence_start);
                });
                let ready=self.project_ready() && self.projects.group.as_ref().is_some_and(|group| group.track.is_some());
                if ui.add_enabled(ready,egui::Button::new("Place on selected track")).clicked() {
                    let params=self.projects.forms.place(self.projects.group.as_ref().expect("Ready group"),self.projects.asset.as_deref(),self.projects.stream);
                    match params { Ok(params)=>self.mutate_project("clips.place",params,ui.ctx()), Err(error)=>self.error=Some(error) }
                }
                self.project_status(ui);
            });
        });
        self.projects.source_form = open;
    }
}
