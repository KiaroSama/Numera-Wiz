//! Shared actual-input journey. Adapters send OS/WebDriver input; queries only observe.
use nw_domain::commands::{ProjectRef, Request, Response};
use serde_json::{Value, json};
use std::{future::Future, time::Instant};

pub const OBSERVER_PREFIX: &str = "project-observer-";
const PAGE: usize = 128;
const PROJECT_COMMANDS: [&str; 11] = [
    "projects.list",
    "projects.create",
    "projects.inspect",
    "sequences.create",
    "sequences.inspect",
    "clips.place",
    "clips.move",
    "clips.split",
    "clips.remove",
    "history.undo",
    "history.redo",
];
pub fn project_commands() -> &'static [&'static str] {
    &PROJECT_COMMANDS
}
pub fn project_mutation(command: &str) -> bool {
    matches!(
        command,
        "projects.create"
            | "sequences.create"
            | "clips.place"
            | "clips.move"
            | "clips.split"
            | "clips.remove"
            | "history.undo"
            | "history.redo"
    )
}
#[derive(Clone, Copy)]
pub enum Key {
    Tab,
    Space,
    Escape,
    CtrlEnter,
}
#[derive(Clone, Copy)]
pub enum Policy {
    OlderCommands,
    UpgradeCommands,
    ReadOnlyProject,
    EditProject,
    RestoreSources,
}
pub struct Observation {
    pub request_id: String,
    pub project: Option<String>,
    pub revision: Option<String>,
    pub expected_revision: Option<String>,
    pub code: String,
    pub receipt: Option<String>,
}
impl Observation {
    pub fn evidence(&self, command: &str) -> Value {
        json!({"command":command,"request_id":self.request_id,"project":self.project,
            "revision":self.revision,"expected_revision":self.expected_revision,
            "code":self.code,"receipt":self.receipt})
    }
}
pub trait Driver {
    fn browser(&self) -> bool;
    fn metadata(&self) -> Value;
    fn mark(&mut self) -> impl Future<Output = ()>;
    fn click(&mut self, x: u32, y: u32) -> impl Future<Output = ()>;
    fn key(&mut self, key: Key) -> impl Future<Output = ()>;
    fn replace(&mut self, text: &str) -> impl Future<Output = ()>;
    fn screenshot(&mut self, name: &str) -> impl Future<Output = ()>;
    fn observed(&mut self, command: &str) -> impl Future<Output = Observation>;
    fn loaded(&mut self, reads: usize) -> impl Future<Output = ()>;
    fn no_cleanup_submission(&mut self) -> impl Future<Output = ()>;
    fn request(&mut self, request: Request, external: bool) -> impl Future<Output = Response>;
    fn policy(&mut self, phase: Policy, project: Option<&str>) -> impl Future<Output = Value>;
    fn reopen(&mut self) -> impl Future<Output = Value>;
    fn write(&self, name: &str, value: &Value);
}
pub fn request(command: &str, params: Value, project: Option<&str>) -> Request {
    let mut request = Request::query(command, params);
    request.request_id = format!("{OBSERVER_PREFIX}{}", uuid::Uuid::new_v4());
    request.project = project.map(|id| ProjectRef {
        id: id.into(),
        expected_revision: None,
    });
    request
}
async fn read<D: Driver>(
    driver: &mut D,
    command: &str,
    params: Value,
    project: Option<&str>,
) -> Value {
    let response = driver
        .request(request(command, params, project), false)
        .await;
    assert!(
        response.ok,
        "Independent project observation failed: {:?}",
        response.error
    );
    assert!(response.receipt_id.is_none());
    response.result.unwrap()
}
fn id(value: &Value) -> String {
    value
        .as_str()
        .expect("Canonical string identity")
        .to_owned()
}
fn rational(number: i64) -> Value {
    json!({"num":number.to_string(),"den":"1"})
}
fn range(start: i64, end: i64) -> Value {
    json!({"start":rational(start),"end":rational(end)})
}
fn ids(page: &Value) -> Vec<String> {
    let mut ids: Vec<_> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| id(&row["id"]))
        .collect();
    ids.sort();
    ids
}
struct Graph {
    project: String,
    revision: String,
    bin: String,
    sequence: String,
    solid_track: String,
    source_track: String,
    left: String,
    right: String,
}
impl Graph {
    fn accept(&mut self, observation: &Observation) {
        assert_eq!(observation.code, "OK");
        assert!(
            observation
                .receipt
                .as_ref()
                .is_some_and(|receipt| !receipt.is_empty())
        );
        assert_eq!(observation.project.as_deref(), Some(self.project.as_str()));
        if let Some(expected) = &observation.expected_revision {
            assert_eq!(expected, &self.revision);
        }
        let next = observation
            .revision
            .as_ref()
            .expect("Accepted decimal revision");
        assert_eq!(
            next,
            &(self.revision.parse::<u64>().unwrap() + 1).to_string()
        );
        self.revision = next.clone();
    }
}
async fn summary<D: Driver>(driver: &mut D, graph: &Graph) -> Value {
    let value = read(
        driver,
        "projects.inspect",
        json!({"projection":"summary","limit":PAGE,"revision":graph.revision}),
        Some(&graph.project),
    )
    .await;
    assert_eq!(value["project_id"], graph.project);
    assert_eq!(value["project_revision"], graph.revision);
    value
}
async fn entity<D: Driver>(driver: &mut D, graph: &Graph, entity: &str) -> Value {
    let value = read(
        driver,
        "projects.inspect",
        json!({"projection":"entity","entity_id":entity,"limit":PAGE,"revision":graph.revision}),
        Some(&graph.project),
    )
    .await;
    assert_eq!(value["project_revision"], graph.revision);
    assert_eq!(value["entity"]["id"], entity);
    value["entity"].clone()
}
async fn viewport<D: Driver>(driver: &mut D, graph: &Graph, track: &str) -> Value {
    let value = read(driver, "sequences.inspect", json!({"projection":"viewport","sequence_id":graph.sequence,"track_ids":[track],"range":range(0,8),"limit":PAGE,"revision":graph.revision}), Some(&graph.project)).await;
    assert_eq!(value["project_revision"], graph.revision);
    assert!(value["next_cursor"].is_null());
    value
}
fn clip_at(clip: &Value, start: i64, end: i64, source_start: i64, source_end: i64) {
    let typed: nw_domain::projects::Clip = serde_json::from_value(clip.clone()).unwrap();
    assert_eq!(
        typed.sequence_start,
        nw_domain::time::Rational::new(start, 1).unwrap()
    );
    assert_eq!(
        typed.duration,
        nw_domain::time::Rational::new(end - start, 1).unwrap()
    );
    assert_eq!(clip["sequence_start"], rational(start));
    assert_eq!(clip["duration"], rational(end - start));
    assert_eq!(clip["source_range"], range(source_start, source_end));
    assert_eq!(clip["rate"], rational(1));
}
async fn activate<D: Driver>(
    driver: &mut D,
    x: u32,
    y: u32,
    command: &str,
    reads: usize,
) -> Observation {
    driver.mark().await;
    driver.click(x, y).await;
    let observation = driver.observed(command).await;
    assert_eq!(observation.code, "OK", "{command}");
    if reads != 0 {
        driver.loaded(reads).await;
    }
    observation
}
async fn tab<D: Driver>(driver: &mut D, count: usize) {
    for _ in 0..count {
        driver.key(Key::Tab).await;
    }
}
async fn field<D: Driver>(driver: &mut D, text: &str) {
    driver.key(Key::Tab).await;
    driver.replace(text).await;
}
async fn refresh<D: Driver>(driver: &mut D, reads: usize) {
    activate(driver, 175, 80, "projects.list", reads).await;
}
async fn select_clip<D: Driver>(driver: &mut D, row: usize) {
    // Source-derived shared panel rows; hosted screenshots qualify these input targets.
    driver.mark().await;
    driver.click(410, 184 + row as u32 * 94).await;
    driver.observed("projects.list").await;
    driver.loaded(8).await;
}
async fn select_source_track<D: Driver>(driver: &mut D) {
    driver.mark().await;
    driver.click(85, 288).await;
    driver.observed("projects.list").await;
    driver.loaded(6).await;
}
async fn catalog_open<D: Driver>(driver: &mut D, project: &str, name: &str) {
    let catalog = read(driver, "projects.list", json!({"limit":64}), None).await;
    assert_eq!(catalog["items"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["items"][0]["name"], name);
    assert_eq!(catalog["items"][0]["id"], project);
    driver.screenshot("reopened-catalog").await;
    // Fresh catalog focus starts at the name field: Create, Refresh, then visible Open.
    driver.mark().await;
    tab(driver, 3).await;
    driver.key(Key::Space).await;
    driver.observed("projects.list").await;
    driver.loaded(8).await;
}
async fn source_picker<D: Driver>(driver: &mut D, expected_assets: &[String]) -> (String, u32) {
    driver.click(318, 100).await;
    driver.click(420, 134).await;
    driver.mark().await;
    driver.click(380, 176).await;
    assert_eq!(driver.observed("assets.list").await.code, "OK");
    let assets = read(driver, "assets.list", json!({"limit":PAGE}), None).await;
    assert_eq!(ids(&assets), expected_assets);
    let rows = assets["items"].as_array().unwrap();
    let row = rows
        .iter()
        .position(|asset| asset["display_name"] == "a-registered.mkv")
        .expect("Retained real folder source in the current inventory order");
    let asset_id = id(&rows[row]["id"]);
    driver.mark().await;
    driver.click(390, 197 + row as u32 * 21).await;
    assert_eq!(driver.observed("assets.inspect").await.code, "OK");
    let asset = read(driver, "assets.inspect", json!({"id":asset_id}), None).await;
    let streams = asset["probe"]["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[0]["kind"], "video");
    let physical = streams[0]["index"].as_u64().unwrap() as u32;
    // First real video stream follows the three inspected-source rows.
    driver.click(390, 319).await;
    driver.screenshot("registered-source-picker").await;
    (asset_id, physical)
}
async fn place_registered<D: Driver>(driver: &mut D) -> Observation {
    driver.click(390, 319).await;
    tab(driver, 3).await;
    driver.replace("0").await;
    for value in ["1", "1", "1", "0", "1"] {
        field(driver, value).await;
    }
    driver.screenshot("registered-source-form").await;
    driver.mark().await;
    driver.key(Key::Tab).await;
    driver.key(Key::Space).await;
    driver.observed("clips.place").await
}
async fn external_move<D: Driver>(driver: &mut D, graph: &Graph) -> Response {
    let scope = driver
        .request(request("access.replay_scope.create", json!({}), None), true)
        .await;
    assert!(scope.ok);
    let mut edit = request(
        "clips.move",
        json!({"clip_ids":[graph.left],"delta":rational(1),"collision":"reject","link_policy":"expand"}),
        Some(&graph.project),
    );
    edit.project.as_mut().unwrap().expected_revision = Some(graph.revision.clone());
    edit.replay_scope = Some(id(&scope.result.unwrap()["replay_scope"]));
    edit.idempotency_key = Some(edit.request_id.clone());
    let result = driver.request(edit, true).await;
    assert!(
        result.ok,
        "Authorized external setup failed: {:?}",
        result.error
    );
    result
}
pub async fn run<D: Driver>(driver: &mut D, name: &str) {
    let started = Instant::now();
    let mut evidence = Vec::new();
    let assets = ids(&read(driver, "assets.list", json!({"limit":PAGE}), None).await);
    let job_page = read(driver, "jobs.list", json!({"limit":PAGE}), None).await;
    let jobs = ids(&job_page);
    for row in job_page["items"].as_array().unwrap() {
        let job: nw_domain::views::JobSummary = serde_json::from_value(row.clone()).unwrap();
        assert!(
            job.state.terminal(),
            "Policy fixture starts only after all old jobs are terminal"
        );
    }
    assert_eq!(assets.len(), 3);
    assert_eq!(jobs.len(), 3);
    // Both retained185 captures show the genuine Review control at this point.
    // Keep a valid reviewed Cleanup plan so the Projects shortcut guard is meaningful.
    let review = activate(driver, 330, 570, "streams.clean", 0).await;
    evidence.push(review.evidence("streams.clean"));
    driver.screenshot("retained-cleanup-plan").await;
    if driver.browser() {
        evidence.push(driver.policy(Policy::OlderCommands, None).await);
    }
    driver.mark().await;
    // Verified shared navigation target in retained185 native/browser Cleanup captures.
    driver.click(358, 13).await;
    let opening = driver.observed("projects.list").await;
    if driver.browser() {
        assert_eq!(opening.code, "PERMISSION_DENIED");
        evidence.push(opening.evidence("projects.list"));
        evidence.push(driver.policy(Policy::UpgradeCommands, None).await);
        driver.mark().await;
        driver.click(220, 82).await;
        tab(driver, 2).await;
        driver.key(Key::Space).await;
        assert_eq!(driver.observed("projects.list").await.code, "OK");
    } else {
        assert_eq!(opening.code, "OK");
    }
    driver.screenshot("catalog").await;
    driver.click(220, 82).await;
    driver.replace(name).await;
    driver.mark().await;
    driver.key(Key::CtrlEnter).await;
    driver.screenshot("text-focus-shortcut-blocked").await;
    driver.no_cleanup_submission().await;
    assert_eq!(
        ids(&read(driver, "jobs.list", json!({"limit":PAGE}), None).await),
        jobs
    );
    driver.mark().await;
    driver.key(Key::Tab).await;
    driver.key(Key::Space).await;
    let created = driver.observed("projects.create").await;
    assert_eq!(created.code, "OK");
    assert_eq!(created.revision.as_deref(), Some("0"));
    driver.loaded(2).await;
    let mut graph = Graph {
        project: created.project.clone().unwrap(),
        revision: "0".into(),
        bin: String::new(),
        sequence: String::new(),
        solid_track: String::new(),
        source_track: String::new(),
        left: String::new(),
        right: String::new(),
    };
    evidence.push(created.evidence("projects.create"));
    graph.bin = id(&summary(driver, &graph).await["root_bin_id"]);
    driver.click(65, 169).await;
    driver.replace("Sequence C").await;
    for value in ["1920", "1080", "25", "1", "48000"] {
        field(driver, value).await;
    }
    tab(driver, 2).await;
    driver.replace("Solid C\nSource C").await;
    field(driver, "Audio C").await;
    driver.screenshot("sequence-form").await;
    driver.mark().await;
    driver.key(Key::Tab).await;
    driver.key(Key::Space).await;
    let sequence = driver.observed("sequences.create").await;
    graph.accept(&sequence);
    evidence.push(sequence.evidence("sequences.create"));
    driver.loaded(6).await;
    let bins = read(
        driver,
        "projects.inspect",
        json!({"projection":"bin_page","bin_id":graph.bin,"limit":PAGE,"revision":graph.revision}),
        Some(&graph.project),
    )
    .await;
    graph.sequence = id(&bins["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "Sequence C")
        .unwrap()["id"]);
    let sequence = read(driver,"sequences.inspect",json!({"projection":"entity","sequence_id":graph.sequence,"limit":PAGE,"revision":graph.revision}),Some(&graph.project)).await;
    assert_eq!(sequence["entity"]["settings"]["frame_rate"], rational(25));
    assert_eq!(sequence["entity"]["settings"]["sample_rate"], 48000);
    assert_eq!(sequence["entity"]["settings"]["audio_layout"], "stereo");
    let tracks = read(driver,"sequences.inspect",json!({"projection":"track_page","sequence_id":graph.sequence,"limit":PAGE,"revision":graph.revision}),Some(&graph.project)).await;
    assert_eq!(tracks["items"].as_array().unwrap().len(), 3);
    for (name, kind) in [
        ("Solid C", "video"),
        ("Source C", "video"),
        ("Audio C", "audio"),
    ] {
        let track = tracks["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        assert_eq!(track["kind"], kind);
        if name == "Solid C" {
            graph.solid_track = id(&track["id"]);
        }
        if name == "Source C" {
            graph.source_track = id(&track["id"]);
        }
    }
    driver.screenshot("sequence").await;
    driver.click(318, 100).await;
    driver.click(450, 176).await;
    driver.replace("0.2, 0.4, 0.8, 1").await;
    for value in ["4", "1", "0", "1", "4", "1", "0", "1"] {
        field(driver, value).await;
    }
    driver.screenshot("solid-form").await;
    driver.mark().await;
    driver.key(Key::Tab).await;
    driver.key(Key::Space).await;
    let placed = driver.observed("clips.place").await;
    graph.accept(&placed);
    evidence.push(placed.evidence("clips.place"));
    driver.loaded(8).await;
    let page = viewport(driver, &graph, &graph.solid_track).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    graph.left = id(&page["items"][0]["id"]);
    select_clip(driver, 0).await;
    let initial = entity(driver, &graph, &graph.left).await;
    clip_at(&initial, 0, 4, 0, 4);
    let source_id = id(&initial["source_id"]);
    assert_eq!(
        entity(driver, &graph, &source_id).await["source_kind"],
        "solid_color"
    );
    driver.screenshot("selected-solid-before-move").await;
    driver.mark().await;
    driver.click(1240, 307).await;
    driver.replace("2").await;
    field(driver, "1").await;
    driver.screenshot("move-fields-before-submit").await;
    // Keep the whole first edit attempt inside the same observer window.
    driver.click(1170, 329).await;
    let moved = driver.observed("clips.move").await;
    assert_eq!(moved.code, "OK", "clips.move");
    driver.loaded(8).await;
    graph.accept(&moved);
    evidence.push(moved.evidence("clips.move"));
    clip_at(&entity(driver, &graph, &graph.left).await, 2, 6, 0, 4);
    driver.click(1270, 350).await;
    driver.replace("4").await;
    field(driver, "1").await;
    let split = activate(driver, 1170, 372, "clips.split", 8).await;
    graph.accept(&split);
    evidence.push(split.evidence("clips.split"));
    let split_page = viewport(driver, &graph, &graph.solid_track).await;
    assert_eq!(split_page["items"].as_array().unwrap().len(), 2);
    graph.right = id(&split_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] != graph.left)
        .unwrap()["id"]);
    select_clip(driver, 1).await;
    let right = entity(driver, &graph, &graph.right).await;
    clip_at(&right, 4, 6, 2, 4);
    assert_eq!(right["source_id"], source_id);
    select_clip(driver, 0).await;
    clip_at(&entity(driver, &graph, &graph.left).await, 2, 4, 0, 2);
    select_clip(driver, 1).await;
    let lifted = activate(driver, 1170, 393, "clips.remove", 8).await;
    graph.accept(&lifted);
    evidence.push(lifted.evidence("clips.remove"));
    assert_eq!(
        ids(&viewport(driver, &graph, &graph.solid_track).await),
        vec![graph.left.clone()]
    );
    let undo = activate(driver, 390, 100, "history.undo", 8).await;
    graph.accept(&undo);
    evidence.push(undo.evidence("history.undo"));
    select_clip(driver, 1).await;
    assert_eq!(
        entity(driver, &graph, &graph.right).await,
        right,
        "Undo restores the SAME right ID and exact entity/source ranges"
    );
    let redo = activate(driver, 435, 100, "history.redo", 8).await;
    graph.accept(&redo);
    evidence.push(redo.evidence("history.redo"));
    assert_eq!(
        ids(&viewport(driver, &graph, &graph.solid_track).await),
        vec![graph.left.clone()]
    );
    select_clip(driver, 0).await;
    driver.screenshot("edited-timeline").await;
    driver.mark().await;
    driver.key(Key::CtrlEnter).await;
    driver.screenshot("project-focus-shortcut-blocked").await;
    driver.no_cleanup_submission().await;
    assert_eq!(
        ids(&read(driver, "jobs.list", json!({"limit":PAGE}), None).await),
        jobs
    );
    let (asset, physical) = source_picker(driver, &assets).await;
    driver.key(Key::Escape).await;
    let mut left_start = 2;
    if driver.browser() {
        evidence.push(
            driver
                .policy(Policy::ReadOnlyProject, Some(&graph.project))
                .await,
        );
        refresh(driver, 8).await;
        let readable = summary(driver, &graph).await;
        assert_eq!(readable["compatibility"], "supported");
        assert_eq!(readable["writer_status"], "this_owner");
        assert_eq!(readable["read_only"], false);
        driver.mark().await;
        driver.click(1170, 329).await;
        let denied = driver.observed("clips.move").await;
        assert_eq!(denied.code, "OBJECT_NOT_FOUND");
        evidence.push(denied.evidence("clips.move"));
        evidence.push(
            driver
                .policy(Policy::EditProject, Some(&graph.project))
                .await,
        );
        refresh(driver, 8).await;
        let admitted = activate(driver, 1170, 329, "clips.move", 8).await;
        graph.accept(&admitted);
        evidence.push(admitted.evidence("clips.move"));
        left_start = 4;
        clip_at(&entity(driver, &graph, &graph.left).await, 4, 6, 0, 2);
    }
    select_source_track(driver).await;
    if driver.browser() {
        driver.click(318, 100).await;
        let denied = place_registered(driver).await;
        assert_eq!(denied.code, "OBJECT_NOT_FOUND");
        evidence.push(denied.evidence("clips.place"));
        driver.key(Key::Escape).await;
        evidence.push(
            driver
                .policy(Policy::RestoreSources, Some(&graph.project))
                .await,
        );
        refresh(driver, 6).await;
    }
    let inspected = source_picker(driver, &assets).await;
    assert_eq!(inspected, (asset.clone(), physical));
    let registered = place_registered(driver).await;
    graph.accept(&registered);
    evidence.push(registered.evidence("clips.place"));
    driver.loaded(8).await;
    let registered_page = viewport(driver, &graph, &graph.source_track).await;
    assert_eq!(registered_page["items"].as_array().unwrap().len(), 1);
    let registered_id = id(&registered_page["items"][0]["id"]);
    let registered_clip = entity(driver, &graph, &registered_id).await;
    clip_at(&registered_clip, 0, 1, 0, 1);
    let binding = entity(driver, &graph, &id(&registered_clip["source_id"])).await;
    assert_eq!(binding["source_kind"], "registered");
    assert_eq!(binding["asset_id"], asset);
    assert_eq!(binding["stream_index"], physical);
    evidence.push(json!({"registered_clip":registered_clip,"source_binding":binding}));
    // Select the original solid track through its real row before observing/editing it.
    driver.mark().await;
    driver.click(85, 256).await;
    driver.observed("projects.list").await;
    driver.loaded(8).await;
    select_clip(driver, 0).await;
    let captured_revision = graph.revision.clone();
    let competing = external_move(driver, &graph).await;
    let competing_revision = id(&competing.result.as_ref().unwrap()["revision"]);
    driver.mark().await;
    driver.click(1170, 329).await;
    let stale = driver.observed("clips.move").await;
    assert_eq!(stale.code, "STALE_REVISION");
    assert!(stale.receipt.is_none());
    driver.screenshot("stale").await;
    evidence.push(json!({"external_fixture":"separate authorized canonical editor, never a claimed UI action","captured_ui_revision":captured_revision,"external_revision":competing_revision,"external_receipt":competing.receipt_id,"ui_rejection":stale.evidence("clips.move")}));
    graph.revision = competing_revision;
    refresh(driver, 8).await;
    clip_at(
        &entity(driver, &graph, &graph.left).await,
        left_start + 1,
        left_start + 3,
        0,
        2,
    );
    driver.click(1240, 307).await;
    driver.replace("2").await;
    field(driver, "1").await;
    let deliberate = activate(driver, 1170, 329, "clips.move", 8).await;
    graph.accept(&deliberate);
    evidence.push(deliberate.evidence("clips.move"));
    clip_at(
        &entity(driver, &graph, &graph.left).await,
        left_start + 3,
        left_start + 5,
        0,
        2,
    );
    driver.screenshot("refreshed-deliberate-edit").await;
    activate(driver, 70, 80, "projects.list", 0).await;
    catalog_open(driver, &graph.project, name).await;
    clip_at(
        &entity(driver, &graph, &graph.left).await,
        left_start + 3,
        left_start + 5,
        0,
        2,
    );
    // Acknowledge retirement before frontend/page teardown; observers do not retire tickets.
    activate(driver, 70, 80, "projects.list", 0).await;
    evidence.push(driver.reopen().await);
    driver.mark().await;
    driver.click(358, 13).await;
    assert_eq!(driver.observed("projects.list").await.code, "OK");
    catalog_open(driver, &graph.project, name).await;
    clip_at(
        &entity(driver, &graph, &graph.left).await,
        left_start + 3,
        left_start + 5,
        0,
        2,
    );
    let final_solid = entity(driver, &graph, &graph.left).await;
    driver.screenshot("reopened-project").await;
    select_source_track_after_reopen(driver).await;
    assert_eq!(
        ids(&viewport(driver, &graph, &graph.source_track).await),
        vec![registered_id.clone()]
    );
    let reopened = entity(driver, &graph, &registered_id).await;
    assert_eq!(reopened, registered_clip);
    driver.screenshot("reopened-registered-track").await;
    assert_eq!(
        ids(&read(driver, "assets.list", json!({"limit":PAGE}), None).await),
        assets
    );
    assert_eq!(
        ids(&read(driver, "jobs.list", json!({"limit":PAGE}), None).await),
        jobs
    );
    let final_summary = summary(driver, &graph).await;
    evidence.push(json!({"initial_solid":initial,"split_right_restored_by_undo":right,"final_solid":final_solid,"final_history":final_summary["history"]}));
    assert!(evidence.len() <= 64);
    driver.write("journey",&json!({"schema_version":1,"identity":driver.metadata(),"elapsed_ms":started.elapsed().as_millis().to_string(),"project_id":graph.project,"revision":graph.revision,"sequence_id":graph.sequence,"solid_track":graph.solid_track,"source_track":graph.source_track,"left_clip":graph.left,"right_clip":graph.right,"registered_clip":registered_id,"asset_id":asset,"physical_stream":physical,"assets":assets,"jobs":jobs,"producer_job_count":3,"observations":evidence,"oracle":"Actual pointer/keyboard controls; independent canonical queries reuse the selected UI bindings; external editor and owner policy updates are explicitly labeled fixture setup","not_qualified":["transport-loss replay","OS IME","Windows/macOS human input","GPU","decoded preview/export","full v1"]}));
}
async fn select_source_track_after_reopen<D: Driver>(driver: &mut D) {
    driver.mark().await;
    driver.click(85, 288).await;
    driver.observed("projects.list").await;
    driver.loaded(8).await;
}
