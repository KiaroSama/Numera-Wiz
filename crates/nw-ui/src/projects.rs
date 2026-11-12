//! Shared project presentation and bounded immutable projection controller.
mod forms;
mod widgets;
#[cfg(test)]
pub(super) mod tests;
use crate::{Action, Asset, AssetSummary, Request, SnapshotPage, WizApp, egui};
use forms::Forms;
use nw_domain::{
    commands::ProjectRef,
    projects::{Clip, Revision, Sequence, Track, views::{ClipSummary, ProjectItemSummary, ProjectSummary, TrackSummary}},
    time::{Rational, TimeRange},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use web_time::Instant;

const PAGE: usize = 128;
const GROUP_BYTES: usize = 2 * 1024 * 1024;
const TRACKED_HANDLES: usize = 4;
const BINDINGS: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Catalog, Summary, Bin, Sequence, Tracks, Track, Viewport, Clip, Source,
    NextBin, NextTracks, NextViewport, Assets, Asset, Create, Mutation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProjectAction { pub generation: u64, pub phase: Phase }
#[derive(Clone)]
struct View {
    project: String,
    revision: Option<String>,
    bin: Option<String>,
    sequence: Option<String>,
    track: Option<String>,
    clip: Option<String>,
    range: TimeRange,
}
impl View {
    fn new(project: String) -> Self {
        Self { project, revision:None, bin:None, sequence:None, track:None, clip:None,
            range:TimeRange { start:Rational::ZERO, end:Rational::new(8,1).expect("Constant rational") } }
    }
}
#[derive(Clone)]
enum Navigation { Catalog, Open(View), Cleanup }
#[derive(Clone)]
struct GroupKey { generation:u64, project:String, revision:String, snapshot:String, first_seen:Instant }
#[derive(Clone)]
struct Page<T> { items:Vec<T>, next:Option<String>, total:usize, accuracy:String }
impl<T> Page<T> {
    fn empty() -> Self { Self { items:Vec::new(), next:None, total:0, accuracy:"exact".into() } }
    fn parse(page: SnapshotPage<T>) -> Result<Self, String> {
        if page.items.len() > PAGE || page.next_cursor.as_ref().is_some_and(|cursor| cursor.len() > 2048)
            || !["exact","lower_bound"].contains(&page.count_accuracy.as_str()) {
            return Err("Projection page exceeds its count, cursor or count-accuracy contract".into());
        }
        Ok(Self { items:page.items, next:page.next_cursor, total:page.total_count, accuracy:page.count_accuracy })
    }
}
#[derive(Clone)]
struct Group {
    key:GroupKey,
    view:View,
    summary:Value,
    bins:Page<ProjectItemSummary>,
    sequence:Option<Sequence>,
    tracks:Page<TrackSummary>,
    track:Option<Track>,
    viewport:Page<ClipSummary>,
    clip:Option<Clip>,
    source:Option<Value>,
    bytes:usize,
    page_bytes:[usize;3],
}
struct Staging { view:View, group:Option<Group>, phase:Phase }
struct Handle { id:String, project:String, first_seen:Instant, bindings:BTreeSet<String> }
pub(super) struct ProjectsUi {
    pub(super) active:bool,
    generation:u64,
    catalog:Vec<ProjectSummary>,
    navigation:Option<Navigation>,
    staging:Option<Staging>,
    group:Option<Arc<Group>>,
    stale:bool,
    pub(super) terminal_failure:bool,
    handles:Vec<Handle>,
    forms:Forms,
    assets:Vec<AssetSummary>,
    assets_cursor:Option<String>,
    assets_snapshot:Option<String>,
    asset:Option<Arc<Asset>>,
    stream:Option<u32>,
    sequence_form:bool,
    focus_sequence:bool,
    source_form:bool,
    acceptance:Option<String>,
    focus_catalog:bool,
}
impl Default for ProjectsUi {
    fn default() -> Self {
        Self { active:false, generation:0, catalog:Vec::new(), navigation:None, staging:None, group:None,
            stale:false, terminal_failure:false, handles:Vec::new(), forms:Forms::default(), assets:Vec::new(),
            assets_cursor:None, assets_snapshot:None, asset:None, stream:None, sequence_form:false, focus_sequence:false,
            source_form:false, acceptance:None, focus_catalog:false }
    }
}
impl ProjectsUi {
    fn advance(&mut self) -> Result<u64, String> {
        self.generation = self.generation.checked_add(1).ok_or("Project workflow identity exhausted; reopen the application")?;
        Ok(self.generation)
    }
    fn remember_ticket(&mut self, id: &str, project: &str) -> Result<(), String> {
        if id.is_empty() || id.len() > 128 { return Err("Invalid captured ticket identity".into()); }
        // A response already in flight may arrive after local metadata expiry.
        // The retained group key prevents that old response from renewing its lease.
        for group in [self.group.as_deref(),self.staging.as_ref().and_then(|stage| stage.group.as_ref())].into_iter().flatten() {
            if group.key.snapshot == id && group.key.first_seen.elapsed() >= Duration::from_secs(30) {
                return Err("Captured response arrived after local expiry; explicitly Refresh".into());
            }
        }
        if let Some(handle) = self.handles.iter().find(|handle| handle.id == id) {
            return if handle.project == project { Ok(()) } else { Err("Captured ticket changed project identity".into()) };
        }
        if self.handles.len() >= TRACKED_HANDLES { return Err("Four project handles await retirement or expiry; return to the catalog before opening another project".into()); }
        self.handles.push(Handle { id:id.into(), project:project.into(), first_seen:Instant::now(), bindings:BTreeSet::new() });
        Ok(())
    }
    fn prune_handles(&mut self) {
        // Response receipt is an upper bound on server creation time. Waiting a full
        // lease from first observation is conservative; reuse never extends it.
        self.handles.retain(|handle| handle.first_seen.elapsed() < Duration::from_secs(30));
        if self.group.as_ref().is_some_and(|group| !self.handles.iter().any(|handle| handle.id == group.key.snapshot)) {
            self.stale = true;
        }
    }
    fn bind(&mut self, snapshot: &str, command: &str, params: &Value) -> Result<(), String> {
        let mut normalized = params.clone();
        if let Some(fields) = normalized.as_object_mut() {
            for key in ["revision","cursor","replace_snapshot"] { fields.remove(key); }
        }
        let binding = json!({"command":command,"params":normalized}).to_string();
        if binding.len() > 8192 { return Err("Projection query exceeds its bounded binding".into()); }
        let handle = self.handles.iter_mut().find(|handle| handle.id == snapshot).ok_or("Captured view expired; explicitly Refresh")?;
        if handle.first_seen.elapsed() >= Duration::from_secs(30) { return Err("Captured view expired locally; explicitly Refresh".into()); }
        if !handle.bindings.contains(&binding) && handle.bindings.len() >= BINDINGS {
            return Err("This captured view has eight query bindings; explicitly Refresh before another projection".into());
        }
        handle.bindings.insert(binding);
        Ok(())
    }
}
fn text(value: &Value, field: &str, maximum: usize) -> Result<String, String> {
    value[field].as_str().filter(|s| !s.is_empty() && s.len() <= maximum).map(str::to_owned).ok_or_else(|| format!("Missing or invalid {field}"))
}
fn identifier(value: &Value, field: &str) -> Result<String, String> {
    let id = text(value,field,36)?;
    nw_domain::projects::validate_id(&id).map_err(|error| error.message)?;
    Ok(id)
}
fn result_bytes(value: &Value) -> Result<usize, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?.len();
    if bytes > 256 * 1024 { return Err("Owner projection exceeds its response bound".into()); }
    Ok(bytes)
}
fn verify(group: &Group, request: &Request, result: &Value) -> Result<(), String> {
    if request.project.as_ref().map(|project| project.id.as_str()) != Some(group.key.project.as_str())
        || request.params["revision"] != group.key.revision
        || result["project_revision"] != group.key.revision || result["snapshot_id"] != group.key.snapshot
        || result["projection"] != request.params["projection"] || result["schema_version"] != 1 {
        return Err("Project, revision, ticket or projection changed during the captured load; explicitly Refresh".into());
    }
    Ok(())
}
impl WizApp {
    pub(super) fn schedule_projects(&mut self, ctx: &egui::Context) {
        let was_stale=self.projects.stale;
        self.projects.prune_handles();
        if self.projects.active && !was_stale && self.projects.stale && self.request_idle() {
            self.status="Captured project view expired locally. Explicitly Refresh; accepted jobs continue running.".into();
        }
        if self.projects.active && !self.projects.handles.is_empty() { ctx.request_repaint_after(Duration::from_millis(250)); }
    }
    pub(super) fn leave_projects(&mut self, ctx: &egui::Context) { self.project_navigate(Navigation::Cleanup,ctx); }
    fn project_action(&self, phase: Phase) -> Action { Action::Project(ProjectAction { generation:self.projects.generation, phase }) }
    pub(super) fn enter_projects(&mut self, ctx: &egui::Context) {
        if !self.workspace_switch_allowed() { return; }
        self.projects.active = true;
        self.project_navigate(Navigation::Catalog, ctx);
    }
    fn project_navigate(&mut self, target: Navigation, ctx: &egui::Context) {
        if !self.request_idle() { return; }
        self.projects.prune_handles();
        if matches!(target, Navigation::Cleanup) && self.projects.terminal_failure {
            self.projects.active = false;
            self.projects.navigation = None;
            self.projects.staging = None;
            self.status = "Cleanup. Project retirement remains unconfirmed until acknowledgement or local tracking expiry.".into();
            return;
        }
        if matches!(target, Navigation::Open(_)) && self.projects.handles.len() >= TRACKED_HANDLES {
            self.error = Some("Four project handles await retirement or expiry. Return to the catalog to retire them before opening another project.".into());
            return;
        }
        if let Err(error) = self.projects.advance() { self.error=Some(error); return; }
        if matches!(target,Navigation::Catalog|Navigation::Cleanup) {
            self.projects.sequence_form=false; self.projects.source_form=false;
        }
        self.projects.stale = self.projects.group.is_some();
        self.projects.terminal_failure = false;
        self.projects.staging = None;
        self.projects.navigation = Some(target);
        self.project_catalog_request(ctx);
    }
    fn project_catalog_request(&mut self, ctx: &egui::Context) {
        let replacement = self.projects.handles.first().map(|handle| handle.id.clone());
        self.status = if replacement.is_some() { "Retiring the previous captured project view…" } else { "Loading readable projects…" }.into();
        self.query("projects.list", json!({"limit":64,"replace_snapshot":replacement}), self.project_action(Phase::Catalog), ctx);
    }
    fn project_refresh(&mut self, ctx: &egui::Context) {
        if let Some(group) = self.projects.group.clone() {
            let mut view = group.view.clone(); view.revision=None;
            self.project_navigate(Navigation::Open(view),ctx);
        } else { self.project_navigate(Navigation::Catalog,ctx); }
    }
    fn project_reselect(&mut self, mut view: View, ctx: &egui::Context) {
        if let Some(group) = &self.projects.group { view.revision=Some(group.key.revision.clone()); }
        self.project_navigate(Navigation::Open(view),ctx);
    }
    fn project_ready(&self) -> bool {
        self.projects.active && self.request_idle() && self.projects.navigation.is_none() && self.projects.staging.is_none()
            && !self.projects.stale && self.projects.group.is_some()
    }
    fn mutate_project(&mut self, command: &str, params: Value, ctx: &egui::Context) {
        if !self.project_ready() { return; }
        let group = self.projects.group.as_ref().expect("Ready group");
        let reference = ProjectRef { id:group.key.project.clone(), expected_revision:Some(group.key.revision.clone()) };
        let action = self.project_action(Phase::Mutation);
        if self.mutate_with_project(command, params, Some(reference), action, ctx) { self.projects.stale=true; }
    }
    fn create_project(&mut self, ctx: &egui::Context) {
        if !self.projects.active || !self.request_idle() || self.projects.navigation.is_some() || self.projects.staging.is_some() { return; }
        if let Err(error) = nw_domain::projects::label(&self.projects.forms.project_name) { self.error=Some(error.message); return; }
        self.mutate_with_project("projects.create",json!({"name":self.projects.forms.project_name}),None,self.project_action(Phase::Create),ctx);
    }
    pub(super) fn project_request_failed(&mut self, action: Action) {
        if matches!(action, Action::Project(_)) || (matches!(action, Action::Scope) && self.deferred.as_ref().is_some_and(|intent| matches!(intent.action, Action::Project(_)))) {
            self.projects.stale = true;
            self.projects.terminal_failure = true;
            self.projects.staging = None;
            self.projects.navigation = None;
            self.status = "Project view is stale. Explicitly Refresh and review before resubmitting; Return to Cleanup remains available. Retirement is unconfirmed.".into();
        }
    }
    fn project_query(&mut self, phase: Phase, command: &str, mut params: Value, ctx: &egui::Context) -> Result<(), String> {
        let stage = self.projects.staging.as_ref().ok_or("No staged project load")?;
        let project = stage.view.project.clone();
        if let Some(group) = &stage.group {
            params["revision"] = json!(group.key.revision);
            let snapshot = group.key.snapshot.clone();
            self.projects.bind(&snapshot, command, &params)?;
        } else if let Some(revision) = &stage.view.revision { params["revision"] = json!(revision); }
        self.projects.staging.as_mut().expect("Staging retained").phase = phase;
        self.query_with_project(command,params,Some(ProjectRef { id:project, expected_revision:None }),self.project_action(phase),ctx);
        Ok(())
    }
    fn publish_project(&mut self) -> Result<(), String> {
        let stage = self.projects.staging.take().ok_or("No staged project load")?;
        let group = stage.group.ok_or("No captured project summary")?;
        if group.key.generation != self.projects.generation { return Err("Project workflow changed before publication".into()); }
        self.projects.prune_handles();
        if !self.projects.handles.iter().any(|handle| handle.id == group.key.snapshot) { return Err("Captured view expired before publication; explicitly Refresh".into()); }
        self.projects.group = Some(Arc::new(group));
        self.projects.stale = false;
        self.projects.terminal_failure = false;
        self.status = "Captured project view ready. Changes are applied only after the owner's projected result.".into();
        Ok(())
    }
    pub(super) fn receive_project(&mut self, action: ProjectAction, result: Value, request: &Request, receipt: Option<&str>, ctx: &egui::Context) -> Result<(), String> {
        if action.generation != self.projects.generation { return Err("Stale project workflow response ignored".into()); }
        if matches!(action.phase,Phase::Summary|Phase::Bin|Phase::Sequence|Phase::Tracks|Phase::Track|Phase::Viewport|Phase::Clip|Phase::Source|Phase::NextBin|Phase::NextTracks|Phase::NextViewport) {
            let project=&request.project.as_ref().ok_or("Projection request has no captured project")?.id;
            // A non-cursor query at the still-current revision may legally allocate
            // a new ticket after expiry/policy changes. Reject mixed panes below,
            // but retain that acknowledged handle for explicit retirement too.
            self.projects.remember_ticket(&text(&result,"snapshot_id",128)?,project)?;
        }
        let bytes = result_bytes(&result)?;
        match action.phase {
            Phase::Catalog => {
                let page: SnapshotPage<ProjectSummary> = serde_json::from_value(result).map_err(|error| error.to_string())?;
                if page.schema_version != 1 || page.projection != "projects.summary.v1" || page.project_revision.is_some()
                    || page.items.len() > 64 || page.total_count != page.items.len() || page.count_accuracy != "exact" || page.next_cursor.is_some() {
                    return Err("Catalog is not a complete bounded header projection".into());
                }
                if let Some(id) = request.params["replace_snapshot"].as_str() { self.projects.handles.retain(|handle| handle.id != id); }
                self.projects.catalog = page.items;
                if !self.projects.handles.is_empty() { self.project_catalog_request(ctx); return Ok(()); }
                let target = self.projects.navigation.take().ok_or("Catalog navigation target is missing")?;
                match target {
                    Navigation::Cleanup => { self.projects.active=false; self.projects.group=None; self.status="Cleanup. Previous captured project handles were acknowledged by the catalog.".into(); }
                    Navigation::Catalog => { self.projects.group=None; self.projects.stale=false; self.projects.focus_catalog=true; self.status="Readable projects loaded.".into(); }
                    Navigation::Open(view) => {
                        if !self.projects.catalog.iter().any(|item| item.id.as_str() == view.project) { return Err("The selected project is no longer in the readable catalog".into()); }
                        self.projects.staging=Some(Staging { view, group:None, phase:Phase::Summary });
                        self.project_query(Phase::Summary,"projects.inspect",json!({"projection":"summary","limit":PAGE}),ctx)?;
                    }
                }
                return Ok(());
            }
            Phase::Create | Phase::Mutation => {
                let project = identifier(&result,"project_id")?;
                let revision = text(&result,"revision",20)?;
                Revision::parse(&revision).map_err(|error| error.message)?;
                if request.project.as_ref().is_some_and(|reference| reference.id != project) { return Err("Accepted project identity differs from the captured intent".into()); }
                let receipt = receipt.filter(|id| !id.is_empty() && id.len() <= 128).ok_or("Accepted project mutation has no bounded receipt")?;
                self.projects.acceptance=Some(format!("Accepted revision {revision} · receipt {receipt}"));
                let mut view = if action.phase == Phase::Create { View::new(project.clone()) }
                    else { self.projects.group.as_ref().map(|group| group.view.clone()).unwrap_or_else(|| View::new(project.clone())) };
                view.project=project; view.revision=Some(revision);
                if let Some(sequence) = result["created"]["sequence_id"].as_str() { nw_domain::projects::validate_id(sequence).map_err(|e| e.message)?; view.sequence=Some(sequence.into()); view.track=None; view.clip=None; }
                if let Some(clip) = result["created"]["clip_ids"].as_array().and_then(|items| items.first()).and_then(Value::as_str) { nw_domain::projects::validate_id(clip).map_err(|e| e.message)?; view.clip=Some(clip.into()); }
                if request.command.id == "clips.remove" { view.clip=None; }
                if request.command.id == "sequences.create" { self.projects.sequence_form=false; }
                if request.command.id == "clips.place" { self.projects.source_form=false; }
                self.project_navigate(Navigation::Open(view),ctx);
                return Ok(());
            }
            Phase::Assets => {
                let page: SnapshotPage<AssetSummary> = serde_json::from_value(result).map_err(|error| error.to_string())?;
                if page.schema_version != 1 || page.projection != "assets.summary.v1" || page.project_revision.is_some()
                    || page.items.len() > PAGE || page.snapshot_id.is_empty() || page.snapshot_id.len() > 128
                    || page.next_cursor.as_ref().is_some_and(|cursor| cursor.is_empty() || cursor.len() > 256)
                    || !["exact","lower_bound"].contains(&page.count_accuracy.as_str()) {
                    return Err("Invalid source inventory page".into());
                }
                self.projects.assets=page.items; self.projects.assets_cursor=page.next_cursor; self.projects.assets_snapshot=Some(page.snapshot_id);
                return Ok(());
            }
            Phase::Asset => {
                let asset: Asset = serde_json::from_value(result).map_err(|error| error.to_string())?;
                if request.params["id"] != asset.id { return Err("Inspected source identity changed".into()); }
                self.projects.stream=asset.probe.streams.iter().find(|stream| matches!(stream.kind,nw_domain::mux::StreamKind::Video|nw_domain::mux::StreamKind::Audio) && !stream.attached_picture()).map(|stream| stream.index);
                self.projects.asset=Some(Arc::new(asset)); return Ok(());
            }
            _ => {}
        }
        if matches!(action.phase,Phase::NextBin|Phase::NextTracks|Phase::NextViewport) { return self.receive_project_page(action.phase,result,request); }
        let stage = self.projects.staging.as_ref().ok_or("No matching staged project load")?;
        if stage.phase != action.phase { return Err("Out-of-order project phase ignored".into()); }
        if action.phase == Phase::Summary {
            let ticket = text(&result,"snapshot_id",128)?;
            let project = stage.view.project.clone();
            // Track the first handle before validating later summary/detail fields.
            self.projects.remember_ticket(&ticket,&project)?;
            self.projects.bind(&ticket,&request.command.id,&request.params)?;
            let stage = self.projects.staging.as_ref().expect("Staging retained");
            let revision = text(&result,"project_revision",20)?;
            Revision::parse(&revision).map_err(|error| error.message)?;
            if identifier(&result,"project_id")? != project || result["schema_version"] != 1 || result["projection"] != "summary"
                || stage.view.revision.as_ref().is_some_and(|expected| expected != &revision) { return Err("Captured summary identity/revision changed".into()); }
            let root = identifier(&result,"root_bin_id")?;
            nw_domain::projects::label(&text(&result,"name",128)?).map_err(|error| error.message)?;
            let mut view = stage.view.clone(); view.revision=Some(revision.clone());
            if view.bin.is_none() { view.bin=Some(root); }
            let bin = view.bin.clone();
            let first_seen=self.projects.handles.iter().find(|handle| handle.id == ticket).ok_or("Summary handle expired")?.first_seen;
            let group = Group { key:GroupKey { generation:action.generation, project, revision, snapshot:ticket, first_seen }, view,
                summary:result, bins:Page::empty(), sequence:None, tracks:Page::empty(), track:None,
                viewport:Page::empty(), clip:None, source:None, bytes, page_bytes:[0;3] };
            self.projects.staging.as_mut().expect("Staging retained").group=Some(group);
            return self.project_query(Phase::Bin,"projects.inspect",json!({"projection":"bin_page","bin_id":bin,"limit":PAGE}),ctx);
        }
        let group = self.projects.staging.as_mut().and_then(|stage| stage.group.as_mut()).ok_or("Missing staged captured summary")?;
        verify(group,request,&result)?;
        group.bytes = group.bytes.checked_add(bytes).filter(|total| *total <= GROUP_BYTES).ok_or("Captured group exceeds its two MiB cache bound")?;
        let mut next = None;
        match action.phase {
            Phase::Bin => {
                group.page_bytes[0]=bytes;
                group.bins=Page::parse(serde_json::from_value::<SnapshotPage<ProjectItemSummary>>(result).map_err(|error| error.to_string())?)?;
                if group.view.sequence.is_none() { group.view.sequence=group.bins.items.iter().find(|row| row.kind == "sequence").map(|row| row.id.to_string()); }
                if let Some(sequence) = &group.view.sequence { next=Some((Phase::Sequence,"sequences.inspect",json!({"projection":"entity","sequence_id":sequence,"limit":PAGE}))); }
            }
            Phase::Sequence => {
                let entity=&result["entity"];
                if entity["id"] != request.params["sequence_id"] || entity["kind"] != "sequence" { return Err("Sequence detail identity changed".into()); }
                group.sequence=Some(serde_json::from_value(entity.clone()).map_err(|error| error.to_string())?);
                next=Some((Phase::Tracks,"sequences.inspect",json!({"projection":"track_page","sequence_id":group.view.sequence,"limit":PAGE})));
            }
            Phase::Tracks => {
                group.page_bytes[1]=bytes;
                group.tracks=Page::parse(serde_json::from_value::<SnapshotPage<TrackSummary>>(result).map_err(|error| error.to_string())?)?;
                if group.view.track.is_none() { group.view.track=group.tracks.items.first().map(|track| track.id.to_string()); }
                if let Some(track)=&group.view.track { next=Some((Phase::Track,"projects.inspect",json!({"projection":"entity","entity_id":track,"limit":PAGE}))); }
            }
            Phase::Track => {
                let entity=&result["entity"];
                if entity["id"] != request.params["entity_id"] { return Err("Track detail identity changed".into()); }
                let track:Track=serde_json::from_value(entity.clone()).map_err(|error| error.to_string())?;
                if Some(track.sequence_id.as_str()) != group.view.sequence.as_deref() || track.grid <= Rational::ZERO { return Err("Track sequence or rate grid changed".into()); }
                group.track=Some(track);
                next=Some((Phase::Viewport,"sequences.inspect",json!({"projection":"viewport","sequence_id":group.view.sequence,"track_ids":[group.view.track],"range":group.view.range,"limit":PAGE})));
            }
            Phase::Viewport => {
                group.page_bytes[2]=bytes;
                group.viewport=Page::parse(serde_json::from_value::<SnapshotPage<ClipSummary>>(result).map_err(|error| error.to_string())?)?;
                if group.viewport.items.iter().any(|clip| Some(clip.track_id.as_str()) != group.view.track.as_deref() || clip.sequence_start < Rational::ZERO || clip.duration <= Rational::ZERO) { return Err("Viewport contains a different track or invalid exact interval".into()); }
                if group.view.clip.is_some() && !group.viewport.items.iter().any(|clip| Some(clip.id.as_str()) == group.view.clip.as_deref()) { group.view.clip=None; }
                if group.view.clip.is_none() { group.view.clip=group.viewport.items.first().map(|clip| clip.id.to_string()); }
                if let Some(clip)=&group.view.clip { next=Some((Phase::Clip,"projects.inspect",json!({"projection":"entity","entity_id":clip,"limit":PAGE}))); }
            }
            Phase::Clip => {
                let entity=&result["entity"];
                if entity["id"] != request.params["entity_id"] || entity["kind"] != "clip" { return Err("Clip detail identity changed".into()); }
                let clip:Clip=serde_json::from_value(entity.clone()).map_err(|error| error.to_string())?;
                if Some(clip.track_id.as_str()) != group.view.track.as_deref() || Some(clip.sequence_id.as_str()) != group.view.sequence.as_deref() { return Err("Clip selection changed track or sequence".into()); }
                next=Some((Phase::Source,"projects.inspect",json!({"projection":"entity","entity_id":clip.source_id,"limit":PAGE})));
                group.clip=Some(clip);
            }
            Phase::Source => {
                if result["entity"]["id"] != request.params["entity_id"] || result["entity"]["kind"] != "source_binding" { return Err("Source binding detail identity changed".into()); }
                group.source=Some(result["entity"].clone());
            }
            _ => return Err("Unexpected captured projection phase".into()),
        }
        if let Some((phase,command,params))=next { self.project_query(phase,command,params,ctx) } else { self.publish_project() }
    }
    fn project_next_page(&mut self, phase: Phase, ctx: &egui::Context) {
        if !self.project_ready() { return; }
        let group=self.projects.group.as_ref().expect("Ready group");
        let (command,params)=match phase {
            Phase::NextBin => ("projects.inspect",json!({"projection":"bin_page","bin_id":group.view.bin,"limit":PAGE,"cursor":group.bins.next,"revision":group.key.revision})),
            Phase::NextTracks => ("sequences.inspect",json!({"projection":"track_page","sequence_id":group.view.sequence,"limit":PAGE,"cursor":group.tracks.next,"revision":group.key.revision})),
            Phase::NextViewport => ("sequences.inspect",json!({"projection":"viewport","sequence_id":group.view.sequence,"track_ids":[group.view.track],"range":group.view.range,"limit":PAGE,"cursor":group.viewport.next,"revision":group.key.revision})),
            _ => return,
        };
        if !params["cursor"].is_string() { return; }
        let snapshot=group.key.snapshot.clone(); let project=group.key.project.clone();
        if let Err(error)=self.projects.bind(&snapshot,command,&params) { self.error=Some(error); return; }
        self.query_with_project(command,params,Some(ProjectRef { id:project,expected_revision:None }),self.project_action(phase),ctx);
    }
    fn receive_project_page(&mut self, phase: Phase, result: Value, request: &Request) -> Result<(),String> {
        let current=self.projects.group.as_ref().ok_or("No captured group for continuation")?;
        verify(current,request,&result)?;
        let mut group=current.as_ref().clone();
        let part=match phase { Phase::NextBin=>0,Phase::NextTracks=>1,Phase::NextViewport=>2,_=>return Err("Unexpected page continuation".into()) };
        let bytes=result_bytes(&result)?;
        group.bytes=group.bytes.checked_sub(group.page_bytes[part]).and_then(|size| size.checked_add(bytes)).filter(|size| *size <= GROUP_BYTES).ok_or("Continued group exceeds its two MiB cache bound")?;
        group.page_bytes[part]=bytes;
        match phase {
            Phase::NextBin => group.bins=Page::parse(serde_json::from_value(result).map_err(|error| error.to_string())?)?,
            Phase::NextTracks => group.tracks=Page::parse(serde_json::from_value(result).map_err(|error| error.to_string())?)?,
            Phase::NextViewport => {
                group.viewport=Page::parse(serde_json::from_value(result).map_err(|error| error.to_string())?)?;
                if group.viewport.items.iter().any(|clip| Some(clip.track_id.as_str()) != group.view.track.as_deref() || clip.sequence_start < Rational::ZERO || clip.duration <= Rational::ZERO) { return Err("Continued viewport changed track or exact interval".into()); }
                if !group.viewport.items.iter().any(|clip| Some(clip.id.as_str()) == group.view.clip.as_deref()) { group.view.clip=None; group.clip=None; group.source=None; }
            }
            _ => return Err("Unexpected page continuation".into()),
        }
        // Replace the page; never append a cursor history or accumulate a graph.
        self.projects.prune_handles();
        if !self.projects.handles.iter().any(|handle| handle.id == group.key.snapshot) { return Err("Captured page expired before publication; explicitly Refresh".into()); }
        self.projects.group=Some(Arc::new(group)); Ok(())
    }
}
