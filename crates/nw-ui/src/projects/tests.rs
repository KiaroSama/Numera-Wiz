//! Plain cases within the original shared UI controller test; no actual GUI claims.
use super::*;
use crate::{CommandClient, Preservation, Reply, Response, SaveReply};
use std::{collections::VecDeque, sync::Mutex};

const PROJECT: &str = "10000000-0000-4000-8000-000000000001";
const BIN: &str = "10000000-0000-4000-8000-000000000002";
const TICKET: &str = "10000000-0000-4000-8000-000000000003";

#[derive(Default)]
struct MockJsonClient {
    requests: Mutex<VecDeque<(Request, Reply)>>,
}
impl CommandClient for MockJsonClient {
    fn send(&self, request: Request, reply: Reply) {
        self.requests.lock().unwrap().push_back((request, reply));
    }
    fn choose_media(&self, _: Reply) {
        panic!("Controller fixture must not invoke intake");
    }
    fn save_artifact(
        &self,
        _: nw_domain::artifacts::ArtifactManifest,
        _: Option<String>,
        _: Arc<nw_domain::download::SaveControl>,
        _: SaveReply,
    ) {
        panic!("Controller fixture must not invoke artifact delivery");
    }
}
impl MockJsonClient {
    fn pop(&self) -> (Request, Reply) {
        self.requests
            .lock()
            .unwrap()
            .pop_front()
            .expect("Expected canonical request")
    }
    fn complete(&self, result: Value) -> Request {
        let (request, reply) = self.pop();
        reply(Ok(Response::success(&request.request_id, result)));
        request
    }
    fn empty(&self) -> bool {
        self.requests.lock().unwrap().is_empty()
    }
    fn accept(&self, result: Value) -> Request {
        let (request, reply) = self.pop();
        let mut response = Response::success(&request.request_id, result);
        response.receipt_id = Some("fixture-receipt".into());
        reply(Ok(response));
        request
    }
}
fn inventory(projection: &str, items: Value) -> Value {
    let total = items.as_array().unwrap().len();
    json!({"schema_version":1,"project_revision":null,"snapshot_id":"catalog","snapshot_expires_at":"30",
        "projection":projection,"items":items,"next_cursor":null,"event_cursor":null,"total_count":total,"count_accuracy":"exact"})
}
fn catalog() -> Value {
    inventory(
        "projects.summary.v1",
        json!([{"id":PROJECT,"name":"Controller project","revision":"0","schema_major":1,"envelope_version":1,"compatibility":"supported"}]),
    )
}
fn summary(ticket: &str, revision: &str) -> Value {
    json!({"schema_version":1,"project_revision":revision,"snapshot_id":ticket,"snapshot_expires_at":"30","projection":"summary","event_cursor":null,
        "project_id":PROJECT,"root_bin_id":BIN,"name":"Controller project","entity_count":1,"sequence_count":0,
        "schema_major":1,"envelope_version":1,"compatibility":"supported","writer_status":"available","read_only":false,
        "history":{"can_undo":revision != "0","can_redo":false}})
}
fn bin(ticket: &str, revision: &str) -> Value {
    json!({"schema_version":1,"project_revision":revision,"snapshot_id":ticket,"snapshot_expires_at":"30","projection":"bin_page","event_cursor":null,
        "items":[],"next_cursor":null,"total_count":0,"count_accuracy":"exact"})
}
fn scope() -> String {
    format!("eyJpZCI6ImNvbnRyb2xsZXItZml4dHVyZSJ9.{}", "A".repeat(43))
}
fn boot() -> (WizApp, Arc<MockJsonClient>, egui::Context) {
    let ctx = egui::Context::default();
    let port = Arc::new(MockJsonClient::default());
    let mut app = WizApp::with_context(&ctx, port.clone(), "project-controller".into());
    port.complete(inventory("assets.summary.v1", json!([])));
    app.receive(&ctx);
    port.complete(inventory("jobs.summary.v1", json!([])));
    app.receive(&ctx);
    (app, port, ctx)
}
fn ready() -> (WizApp, Arc<MockJsonClient>, egui::Context) {
    let (mut app, port, ctx) = boot();
    app.enter_projects(&ctx);
    assert_eq!(port.complete(catalog()).command.id, "projects.list");
    app.receive(&ctx);
    app.project_navigate(Navigation::Open(View::new(PROJECT.into())), &ctx);
    assert_eq!(port.complete(catalog()).command.id, "projects.list");
    app.receive(&ctx);
    let request = port.complete(summary(TICKET, "0"));
    assert_eq!(request.command.id, "projects.inspect");
    assert_eq!(request.project.unwrap().id, PROJECT);
    app.receive(&ctx);
    assert!(
        app.projects.group.is_none(),
        "Summary alone is not a coherent group"
    );
    assert_eq!(
        app.projects.handles.len(),
        1,
        "First summary handle must be tracked immediately"
    );
    let request = port.complete(bin(TICKET, "0"));
    assert_eq!(request.params["revision"], "0");
    app.receive(&ctx);
    assert_eq!(app.projects.group.as_ref().unwrap().key.snapshot, TICKET);
    assert!(!app.projects.stale && port.empty());
    (app, port, ctx)
}

pub(crate) fn controller_cases() {
    creation_and_terminal_denial();
    captured_scope_and_retry();
    malformed_scope_retains_intent();
    coherent_load_and_retirement();
    changed_ticket_is_tracked();
    expired_staged_publication();
    cleanup_guards();
    focused_button_actions();
    focused_folder_scan();
    bounded_handle_tracking();
    full_projection_and_exact_forms();
    later_page_selection_keeps_exact_target();
    sequence_history_and_external_removal_recover();
    selection_recovery_rejects_other_failures();
    clip_group_selection_and_requests();
}
fn creation_and_terminal_denial() {
    let (mut app, port, ctx) = boot();
    app.enter_projects(&ctx);
    port.complete(catalog());
    app.receive(&ctx);
    app.projects.forms.project_name = "Captured name".into();
    app.create_project(&ctx);
    let captured = app.deferred.as_ref().unwrap().request.clone();
    assert_eq!(captured.params, json!({"name":"Captured name"}));
    assert!(captured.project.is_none() && captured.idempotency_key.is_some());
    app.projects.forms.project_name = "Later form name".into();
    port.complete(json!({"replay_scope":scope()}));
    app.receive(&ctx);
    let created =
        port.accept(json!({"project_id":PROJECT,"root_bin_id":BIN,"revision":"0","created":{}}));
    app.receive(&ctx);
    assert_eq!(created.params, captured.params);
    assert_eq!(created.request_id, captured.request_id);
    assert_eq!(created.command.id, "projects.create");
    assert!(
        app.projects.group.is_none(),
        "Acceptance cannot publish project panes"
    );
    port.complete(catalog());
    app.receive(&ctx);
    port.complete(summary(TICKET, "0"));
    app.receive(&ctx);
    port.complete(bin(TICKET, "0"));
    app.receive(&ctx);
    assert!(app.project_ready());
    app.project_navigate(Navigation::Catalog, &ctx);
    let (request, reply) = port.pop();
    assert_eq!(request.params["replace_snapshot"], TICKET);
    reply(Ok(Response::error(
        &request.request_id,
        nw_domain::DomainError::new("PERMISSION_DENIED", "Catalog permission revoked"),
    )));
    app.receive(&ctx);
    assert!(app.pending.is_none() && app.projects.stale && app.projects.terminal_failure);
    app.leave_projects(&ctx);
    assert!(!app.projects.active && app.projects.handles.len() == 1 && port.empty());
    assert!(app.status.contains("unconfirmed"));
}
fn captured_scope_and_retry() {
    let (mut app, port, ctx) = ready();
    let params = json!({"clip_ids":[BIN],"delta":{"num":"2","den":"1"},"collision":"reject","link_policy":"expand"});
    app.mutate_project("clips.move", params.clone(), &ctx);
    let captured = app.deferred.as_ref().unwrap().request.clone();
    assert_eq!(
        captured
            .project
            .as_ref()
            .unwrap()
            .expected_revision
            .as_deref(),
        Some("0")
    );
    assert!(captured.idempotency_key.is_some());
    // Simulate a newer local observation while bootstrap is delayed. The intent
    // must not reread it; the canonical stale response remains authoritative.
    Arc::make_mut(app.projects.group.as_mut().unwrap())
        .key
        .revision = "9".into();
    let bootstrap = port.complete(json!({"replay_scope":scope()}));
    assert_eq!(bootstrap.command.id, "access.replay_scope.create");
    assert_ne!(bootstrap.request_id, captured.request_id);
    app.receive(&ctx);
    let (sent, reply) = port.pop();
    let mut expected = captured;
    expected.replay_scope = Some(scope());
    assert_eq!(
        serde_json::to_vec(&sent).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    reply(Err("Connection lost after send".into()));
    app.receive(&ctx);
    let pending_bytes = serde_json::to_vec(&app.pending.as_ref().unwrap().request).unwrap();
    app.project_navigate(Navigation::Cleanup, &ctx);
    app.project_navigate(Navigation::Open(View::new(PROJECT.into())), &ctx);
    app.mutate_project("history.redo", json!({}), &ctx);
    assert!(app.projects.active && port.empty());
    app.retry_pending(&ctx);
    let (retry, reply) = port.pop();
    assert_eq!(serde_json::to_vec(&retry).unwrap(), pending_bytes);
    assert_eq!(retry.params, params);
    assert_eq!(
        retry.project.as_ref().unwrap().expected_revision.as_deref(),
        Some("0")
    );
    reply(Ok(Response::error(
        &retry.request_id,
        nw_domain::DomainError::new("STALE_REVISION", "External edit committed"),
    )));
    app.receive(&ctx);
    assert!(app.pending.is_none() && app.projects.stale && port.empty());
    assert!(app.error.as_ref().unwrap().contains("STALE_REVISION"));
    app.project_navigate(Navigation::Cleanup, &ctx);
    assert!(
        !app.projects.active,
        "Canonical terminal failure permits explicit Cleanup"
    );
    assert_eq!(
        app.projects.handles.len(),
        1,
        "Cleanup cannot claim unconfirmed retirement"
    );
}
fn malformed_scope_retains_intent() {
    for invalid in [
        json!({}),
        json!({"replay_scope":12}),
        json!({"replay_scope":""}),
        json!({"replay_scope":"not-a-token"}),
        json!({"replay_scope":"x".repeat(2049)}),
        json!({"replay_scope":format!("AA.{}B","A".repeat(42))}),
    ] {
        let (mut app, port, ctx) = ready();
        app.mutate_project("history.undo", json!({}), &ctx);
        let deferred = serde_json::to_vec(&app.deferred.as_ref().unwrap().request).unwrap();
        let bootstrap = port.complete(invalid);
        app.receive(&ctx);
        assert!(port.empty() && !app.waiting && app.pending.is_some());
        assert!(app.scope.is_none());
        assert_eq!(
            serde_json::to_vec(&app.deferred.as_ref().unwrap().request).unwrap(),
            deferred
        );
        assert_eq!(
            app.pending.as_ref().unwrap().request.request_id,
            bootstrap.request_id
        );
        app.project_navigate(Navigation::Cleanup, &ctx);
        assert!(app.projects.active && port.empty());
        app.retry_pending(&ctx);
        assert_eq!(
            port.complete(json!({"replay_scope":scope()})).request_id,
            bootstrap.request_id
        );
        app.receive(&ctx);
        let (write, reply) = port.pop();
        assert_eq!(write.command.id, "history.undo");
        assert_eq!(
            write.project.as_ref().unwrap().expected_revision.as_deref(),
            Some("0")
        );
        reply(Ok(Response::error(
            &write.request_id,
            nw_domain::DomainError::new("NOTHING_TO_UNDO", "Fixture"),
        )));
        app.receive(&ctx);
        assert!(app.pending.is_none() && app.deferred.is_none());
    }
}
fn coherent_load_and_retirement() {
    let (mut app, port, ctx) = ready();
    app.project_refresh(&ctx);
    let retirement = port.complete(catalog());
    assert_eq!(
        retirement.params,
        json!({"limit":64,"replace_snapshot":TICKET})
    );
    app.receive(&ctx);
    let fresh = "10000000-0000-4000-8000-000000000004";
    let summary_request = port.complete(summary(fresh, "1"));
    app.receive(&ctx);
    assert!(app.projects.stale);
    assert_eq!(
        app.projects.group.as_ref().unwrap().key.revision,
        "0",
        "No partial group publication"
    );
    assert_eq!(app.projects.handles.len(), 1);
    let pending_id = app.pending.as_ref().unwrap().request.request_id.clone();
    *app.inbox.lock().unwrap() = Some(crate::Received {
        request_id: Some(summary_request.request_id.clone()),
        action: Action::Project(ProjectAction {
            generation: 0,
            phase: Phase::Summary,
        }),
        response: Ok(Response::success(
            &summary_request.request_id,
            summary(fresh, "9"),
        )),
    });
    app.receive(&ctx);
    assert!(app.waiting);
    assert_eq!(app.pending.as_ref().unwrap().request.request_id, pending_id);
    port.complete(bin(fresh, "2"));
    app.receive(&ctx);
    assert!(app.projects.stale && app.projects.staging.is_none());
    assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "0");
    assert_eq!(
        app.projects.handles[0].id, fresh,
        "Later load failure retains its first summary handle"
    );
    // A protocol group failure permits an explicit retirement retry.
    app.project_navigate(Navigation::Catalog, &ctx);
    let (request, reply) = port.pop();
    assert_eq!(request.params["replace_snapshot"], fresh);
    reply(Err("Unknown retirement acknowledgement".into()));
    app.receive(&ctx);
    app.project_navigate(Navigation::Cleanup, &ctx);
    assert!(app.projects.active && app.projects.navigation.is_some());
    app.retry_pending(&ctx);
    let retry = port.complete(catalog());
    assert_eq!(
        serde_json::to_vec(&retry).unwrap(),
        serde_json::to_vec(&request).unwrap()
    );
    app.receive(&ctx);
    assert!(app.projects.handles.is_empty() && app.projects.group.is_none());
    assert!(app.projects.active && port.empty());
    app.project_navigate(Navigation::Cleanup, &ctx);
    port.complete(catalog());
    app.receive(&ctx);
    assert!(!app.projects.active);
}
fn reviewed_cleanup() -> (WizApp, Arc<MockJsonClient>, egui::Context) {
    let (mut app, port, ctx) = boot();
    let source = json!({"id":"asset","display_name":"Fixture","bytes":"0","stream_count":0});
    app.plan = Some(Arc::new(serde_json::from_value(json!({"schema_version":1,"required_rights":null,"rights_redacted":false,
        "id":"reviewed","hash":"00","source":source,"source_sha256":"00","ffmpeg":{"file_name":"ffmpeg","bytes":"0","sha256":"00"},"ffprobe":{"file_name":"ffprobe","bytes":"0","sha256":"00"},
        "source_path":null,"output_name":"out","output_path":null,"resolution":{"action":"copy_unchanged","streams":[],"outcomes":[],"reasons":[],"warnings":[],"preservation":Preservation::default()},"argv_template":null,
        "created_at":"0","expires_at":"999","verification_scope":"Controller only","policy_revision":0,"effective_actions":["submit"]})).unwrap()));
    let asset = fixture_asset();
    app.selected = Some(asset.id.clone());
    app.asset_detail = Some(Arc::new(asset.clone()));
    app.output_needed = true;
    app.batch_select(&AssetSummary::from(&asset), true);
    app.receive_batch(Action::BatchPlan(1), json!({"schema_version":1,"id":"batch","hash":"00","created_at":"0","expires_at":"999","failure_policy":"reject_all","unresolved_policy":"reject","unresolved":[],"items":[]})).unwrap();
    app.scope = Some(scope());
    (app, port, ctx)
}
fn widget_frame(ctx: &egui::Context, events: Vec<egui::Event>, draw: impl FnMut(&mut egui::Ui)) {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 861.0),
            )),
            focused: true,
            events,
            ..Default::default()
        },
        draw,
    )
    .drop_without_applying_deltas();
}
fn key_events(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .collect()
}
fn text_frame(ctx: &egui::Context, text: &mut String, events: Vec<egui::Event>, focus: bool) {
    widget_frame(ctx, events, |ui| {
        let response = ui
            .add(egui::TextEdit::singleline(text).id(egui::Id::unique("project-regression-text")));
        if focus {
            response.request_focus();
        }
    });
}
fn exercise_cleanup_actions(app: &mut WizApp, ctx: &egui::Context) {
    app.run_reviewed(ctx);
    app.run_reviewed_batch(ctx);
    app.query("streams.clean", json!({}), Action::Plan(0), ctx);
    app.mutate(
        "assets.register",
        json!({"path":"/must-not-register"}),
        Action::Register,
        ctx,
    );
    app.schedule_output(ctx);
    app.schedule_batch(ctx);
    app.choose_media(ctx);
}
fn cleanup_guards() {
    let (mut app, port, ctx) = reviewed_cleanup();
    app.projects.active = true;
    app.run_reviewed(&ctx);
    app.run_reviewed_batch(&ctx);
    app.query("streams.clean", json!({}), Action::Plan(0), &ctx);
    app.mutate(
        "assets.register",
        json!({"path":"/must-not-register"}),
        Action::Register,
        &ctx,
    );
    app.schedule_output(&ctx);
    app.schedule_batch(&ctx);
    app.choose_media(&ctx);
    assert!(
        port.empty(),
        "Hidden Cleanup actions cannot steal the project slot"
    );
    app.projects.active = false;
    let mut text = String::new();
    text_frame(&ctx, &mut text, Vec::new(), true);
    text_frame(&ctx, &mut text, Vec::new(), false);
    assert!(ctx.egui_wants_keyboard_input());
    assert!(
        ctx.text_edit_focused(),
        "The fixture must focus a rendered TextEdit, not an arbitrary ID"
    );
    app.run_reviewed(&ctx);
    app.run_reviewed_batch(&ctx);
    app.query("streams.clean", json!({}), Action::Plan(0), &ctx);
    app.schedule_output(&ctx);
    app.schedule_batch(&ctx);
    app.choose_media(&ctx);
    assert!(
        port.empty(),
        "Text/IME focus suppresses actual review and submit paths"
    );
    text_frame(
        &ctx,
        &mut text,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: "かな".into(),
            active_range_chars: Some(0..2),
        })],
        false,
    );
    assert_eq!(
        text, "かな",
        "The genuine TextEdit must process its IME preedit"
    );
    assert!(ctx.text_edit_focused());
    exercise_cleanup_actions(&mut app, &ctx);
    assert!(
        port.empty(),
        "Active text composition must not submit or schedule Cleanup work"
    );
    text_frame(
        &ctx,
        &mut text,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("仮名".into()))],
        false,
    );
    assert_eq!(text, "仮名", "Commit must replace the actual composition");
    assert!(ctx.text_edit_focused());
    exercise_cleanup_actions(&mut app, &ctx);
    assert!(
        port.empty(),
        "Committed text still owns focus until it is transferred"
    );
}
fn review_button_frame(
    app: &mut WizApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    focus: bool,
) -> (egui::Rect, bool) {
    let mut rect = egui::Rect::NOTHING;
    let mut clicked = false;
    widget_frame(ctx, events, |ui| {
        let button = ui.button("Run reviewed fixture");
        rect = button.rect;
        if focus {
            button.request_focus();
        }
        if button.clicked() {
            clicked = true;
            app.run_reviewed(ui.ctx());
        }
    });
    (rect, clicked)
}
fn activate_review_button(app: &mut WizApp, ctx: &egui::Context, rect: egui::Rect, pointer: bool) {
    let clicked = if pointer {
        let pos = rect.center();
        let events = |pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let (_, down) = review_button_frame(app, ctx, events(true), false);
        assert!(!down, "A press alone must not activate the normal button");
        review_button_frame(app, ctx, events(false), false).1
    } else {
        review_button_frame(app, ctx, key_events(egui::Key::Space), false).1
    };
    assert!(
        clicked,
        "Real egui button input must activate before checking dispatch"
    );
}
fn focused_button_actions() {
    for pointer in [false, true] {
        let (mut app, port, ctx) = reviewed_cleanup();
        review_button_frame(&mut app, &ctx, Vec::new(), true);
        let (rect, _) = review_button_frame(&mut app, &ctx, Vec::new(), false);
        assert!(
            ctx.egui_wants_keyboard_input(),
            "A focused normal button wants keyboard input too"
        );
        assert!(!ctx.text_edit_focused());
        assert!(app.request_idle());
        app.projects.active = true;
        activate_review_button(&mut app, &ctx, rect, pointer);
        exercise_cleanup_actions(&mut app, &ctx);
        assert!(
            port.empty() && app.request_idle(),
            "Button focus cannot bypass Projects mode"
        );
        app.projects.active = false;
        activate_review_button(&mut app, &ctx, rect, pointer);
        let (request, reply) = port.pop();
        assert_eq!(request.command.id, "jobs.submit");
        assert_eq!(
            request.params,
            json!({"plan_id":"reviewed","plan_hash":"00"})
        );
        assert!(request.project.is_none());
        assert_eq!(request.replay_scope.as_deref(), Some(scope().as_str()));
        assert_eq!(
            request.idempotency_key.as_deref(),
            Some(request.request_id.as_str())
        );
        assert!(
            port.empty(),
            "One input activation must send exactly one canonical request"
        );
        let captured = serde_json::to_vec(&request).unwrap();
        reply(Err(
            "Unknown acceptance after focused button activation".into()
        ));
        app.receive(&ctx);
        assert!(!app.waiting && app.pending.is_some());
        activate_review_button(&mut app, &ctx, rect, pointer);
        exercise_cleanup_actions(&mut app, &ctx);
        assert!(
            port.empty(),
            "A focused button cannot replace unresolved Pending"
        );
        assert_eq!(
            serde_json::to_vec(&app.pending.as_ref().unwrap().request).unwrap(),
            captured
        );
        app.retry_pending(&ctx);
        let (retry, _) = port.pop();
        assert_eq!(serde_json::to_vec(&retry).unwrap(), captured);
        assert!(port.empty());
    }
}
fn folder_frame(app: &mut WizApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    widget_frame(ctx, events, |ui| app.folder_window(ui.ctx()));
}
fn focused_folder_scan() {
    let (mut app, port, ctx) = boot();
    app.scope = Some(scope());
    app.folder.open = true;
    folder_frame(&mut app, &ctx, Vec::new());
    let path_id = egui::Id::unique("folder-intake-path");
    assert!(
        egui::TextEdit::load_state(&ctx, path_id).is_some(),
        "Focus must target the real rendered folder path"
    );
    ctx.memory_mut(|memory| memory.request_focus(path_id));
    let path = "/controller-fixture/authorized-folder";
    folder_frame(&mut app, &ctx, vec![egui::Event::Text(path.into())]);
    assert!(ctx.text_edit_focused());
    for _ in 0..4 {
        folder_frame(&mut app, &ctx, key_events(egui::Key::Tab));
        folder_frame(&mut app, &ctx, Vec::new());
    }
    assert!(ctx.egui_wants_keyboard_input());
    assert!(
        !ctx.text_edit_focused(),
        "Four real Tab events must leave the path/exclusions TextEdits"
    );
    assert!(app.request_idle() && port.empty());
    folder_frame(&mut app, &ctx, key_events(egui::Key::Space));
    let (request, reply) = port.pop();
    assert_eq!(
        request.command.id, "assets.register_folder",
        "The actual focused Scan control must dispatch"
    );
    assert_eq!(request.params["path"], path);
    assert!(request.project.is_none());
    assert_eq!(request.replay_scope.as_deref(), Some(scope().as_str()));
    assert_eq!(
        request.idempotency_key.as_deref(),
        Some(request.request_id.as_str())
    );
    assert!(port.empty());
    folder_frame(&mut app, &ctx, Vec::new());
    assert!(
        port.empty(),
        "A repeated frame must not duplicate an in-flight Scan"
    );
    reply(Ok(Response::error(
        &request.request_id,
        nw_domain::DomainError::new(
            "PERMISSION_DENIED",
            "Fixture response; no filesystem scan runs",
        ),
    )));
    app.receive(&ctx);
    assert!(app.request_idle());
    folder_frame(&mut app, &ctx, Vec::new());
    folder_frame(&mut app, &ctx, Vec::new());
    assert!(
        port.empty(),
        "Restored Scan focus without new input must not resubmit"
    );
}
fn changed_ticket_is_tracked() {
    let (mut app, port, ctx) = ready();
    let first = "30000000-0000-4000-8000-000000000001";
    let second = "30000000-0000-4000-8000-000000000002";
    app.project_refresh(&ctx);
    port.complete(catalog());
    app.receive(&ctx);
    port.complete(summary(first, "0"));
    app.receive(&ctx);
    port.complete(bin(second, "0"));
    app.receive(&ctx);
    assert!(app.projects.stale && app.projects.staging.is_none());
    assert_eq!(app.projects.group.as_ref().unwrap().key.snapshot, TICKET);
    assert_eq!(
        app.projects.handles.len(),
        2,
        "A later acknowledged replacement ticket must not be abandoned"
    );
    app.project_navigate(Navigation::Catalog, &ctx);
    assert_eq!(port.complete(catalog()).params["replace_snapshot"], first);
    app.receive(&ctx);
    assert_eq!(port.complete(catalog()).params["replace_snapshot"], second);
    app.receive(&ctx);
    assert!(app.projects.handles.is_empty() && app.projects.group.is_none() && port.empty());
}
fn expired_staged_publication() {
    for prune_before_reply in [false, true] {
        let (mut app, port, ctx) = ready();
        let ticket = "30000000-0000-4000-8000-000000000003";
        app.project_refresh(&ctx);
        port.complete(catalog());
        app.receive(&ctx);
        port.complete(summary(ticket, "1"));
        app.receive(&ctx);
        let expired = Instant::now() - Duration::from_secs(31);
        app.projects.handles[0].first_seen = expired;
        app.projects
            .staging
            .as_mut()
            .unwrap()
            .group
            .as_mut()
            .unwrap()
            .key
            .first_seen = expired;
        if prune_before_reply {
            app.projects.prune_handles();
        }
        port.complete(bin(ticket, "1"));
        app.receive(&ctx);
        app.projects.prune_handles();
        assert!(
            app.projects.stale && app.projects.staging.is_none() && app.projects.handles.is_empty()
        );
        assert_eq!(
            app.projects.group.as_ref().unwrap().key.revision,
            "0",
            "An expired staged group must not replace the last coherent view or renew a pruned lease"
        );
        assert!(app.pending.is_none() && port.empty());
    }
}
fn fixture_asset() -> Asset {
    serde_json::from_value(json!({"id":"registered-fixture","creator":"fixture-principal","display_name":"Registered source",
        "identity":{"path":"/fixture/source.mkv","bytes":"0","sha256":"00"},
        "probe":{"streams":[{"index":7,"kind":"video","codec":"fixture","tags":{},"dispositions":[]}],
        "format":"controller-only","global_tags":{},"chapters":[],"verification_scope":"No media execution"}})).unwrap()
}
fn projection(ticket: &str, revision: &str, kind: &str, body: Value) -> Value {
    let mut value = json!({"schema_version":1,"project_revision":revision,"snapshot_id":ticket,"snapshot_expires_at":"30","projection":kind,"event_cursor":null});
    value
        .as_object_mut()
        .unwrap()
        .extend(body.as_object().unwrap().clone());
    value
}
fn sequence_summary(ticket: &str, revision: &str, entities: usize) -> Value {
    let mut value = summary(ticket, revision);
    value["entity_count"] = json!(entities);
    value["sequence_count"] = json!(1);
    value
}
fn full_projection_and_exact_forms() {
    const SEQUENCE: &str = "20000000-0000-4000-8000-000000000001";
    const TRACK: &str = "20000000-0000-4000-8000-000000000002";
    const CLIP: &str = "20000000-0000-4000-8000-000000000003";
    const SOURCE: &str = "20000000-0000-4000-8000-000000000004";
    let (mut app, port, ctx) = ready();
    app.scope = Some(scope());
    let defaults = app.projects.forms.sequence().unwrap();
    assert_eq!(
        defaults["settings"]["frame_rate"],
        json!({"num":"25","den":"1"})
    );
    assert!(
        defaults["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|track| track["kind"] == "audio")
    );
    app.projects.forms.video_tracks = "Video".into();
    app.projects.forms.audio_tracks.clear();
    let sequence = app.projects.forms.sequence().unwrap();
    app.mutate_project("sequences.create", sequence.clone(), &ctx);
    let accepted = port
        .accept(json!({"project_id":PROJECT,"revision":"1","created":{"sequence_id":SEQUENCE}}));
    app.receive(&ctx);
    assert_eq!(
        accepted.project.unwrap().expected_revision.as_deref(),
        Some("0")
    );
    assert_eq!(
        app.projects.group.as_ref().unwrap().key.revision,
        "0",
        "Acceptance is not geometry authority"
    );
    port.complete(catalog());
    app.receive(&ctx);
    let ticket = "20000000-0000-4000-8000-000000000005";
    port.complete(sequence_summary(ticket, "1", 3));
    app.receive(&ctx);
    port.complete(projection(ticket,"1","bin_page",json!({"items":[{"id":SEQUENCE,"kind":"sequence","name":"Sequence"}],"next_cursor":null,"total_count":1,"count_accuracy":"exact"})));
    app.receive(&ctx);
    port.complete(projection(ticket,"1","entity",json!({"entity":{"id":SEQUENCE,"kind":"sequence","settings":sequence["settings"],"track_count":1,"locked":false}})));
    app.receive(&ctx);
    port.complete(projection(ticket,"1","track_page",json!({"items":[{"id":TRACK,"kind":"video","name":"Video","order":0,"clip_count":0}],"next_cursor":null,"total_count":1,"count_accuracy":"exact"})));
    app.receive(&ctx);
    port.complete(projection(ticket,"1","entity",json!({"entity":{"id":TRACK,"kind":"video","sequence_id":SEQUENCE,"order":0,"grid":{"num":"25","den":"1"},"clip_count":0,"locked":false}})));
    app.receive(&ctx);
    assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "0");
    port.complete(projection(
        ticket,
        "1",
        "viewport",
        json!({"items":[],"next_cursor":null,"total_count":0,"count_accuracy":"exact"}),
    ));
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap().clone();
    assert_eq!(group.key.revision, "1");
    assert_eq!(
        group.track.as_ref().unwrap().grid,
        Rational::new(25, 1).unwrap()
    );
    let params = app.projects.forms.place(&group, None, None).unwrap();
    assert_eq!(params["source"]["duration"], json!({"num":"4","den":"1"}));
    assert_eq!(
        params["source_range"],
        json!({"start":{"num":"0","den":"1"},"end":{"num":"4","den":"1"}})
    );
    app.mutate_project("clips.place", params, &ctx);
    port.accept(json!({"project_id":PROJECT,"revision":"2","created":{"clip_ids":[CLIP],"source_id":SOURCE}}));
    app.receive(&ctx);
    port.complete(catalog());
    app.receive(&ctx);
    let ticket = "20000000-0000-4000-8000-000000000006";
    port.complete(sequence_summary(ticket, "2", 5));
    app.receive(&ctx);
    port.complete(projection(ticket,"2","bin_page",json!({"items":[{"id":SEQUENCE,"kind":"sequence","name":"Sequence"}],"next_cursor":null,"total_count":1,"count_accuracy":"exact"})));
    app.receive(&ctx);
    port.complete(projection(ticket,"2","entity",json!({"entity":{"id":SEQUENCE,"kind":"sequence","settings":sequence["settings"],"track_count":1,"locked":false}})));
    app.receive(&ctx);
    port.complete(projection(ticket,"2","track_page",json!({"items":[{"id":TRACK,"kind":"video","name":"Video","order":0,"clip_count":1}],"next_cursor":null,"total_count":1,"count_accuracy":"exact"})));
    app.receive(&ctx);
    port.complete(projection(ticket,"2","entity",json!({"entity":{"id":TRACK,"kind":"video","sequence_id":SEQUENCE,"order":0,"grid":{"num":"25","den":"1"},"clip_count":1,"locked":false}})));
    app.receive(&ctx);
    port.complete(projection(ticket,"2","viewport",json!({"items":[{"id":CLIP,"track_id":TRACK,"source_id":SOURCE,"sequence_start":{"num":"0","den":"1"},"duration":{"num":"4","den":"1"}}],"next_cursor":"same-query-next","total_count":1,"count_accuracy":"lower_bound"})));
    app.receive(&ctx);
    port.complete(projection(ticket,"2","entity",json!({"entity":{"id":CLIP,"kind":"clip","sequence_id":SEQUENCE,"track_id":TRACK,"source_id":SOURCE,"occurrence_id":CLIP,"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"4","den":"1"}},"sequence_start":{"num":"0","den":"1"},"duration":{"num":"4","den":"1"},"rate":{"num":"1","den":"1"},"link_group_id":null,"locked":false}})));
    app.receive(&ctx);
    assert_eq!(
        app.projects.group.as_ref().unwrap().key.revision,
        "1",
        "Selected source must agree before publication"
    );
    port.complete(projection(ticket,"2","entity",json!({"entity":{"id":SOURCE,"kind":"source_binding","source_kind":"solid_color","rgba":[0.2,0.4,0.8,1],"duration":{"num":"4","den":"1"}}})));
    app.receive(&ctx);
    assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "2");
    assert_eq!(app.projects.handles[0].bindings.len(), 8);
    app.projects.forms.registered = true;
    let registered = app
        .projects
        .forms
        .place(
            app.projects.group.as_ref().unwrap(),
            Some(&fixture_asset()),
            Some(7),
        )
        .unwrap();
    assert_eq!(
        registered["source"],
        json!({"kind":"registered","asset_id":"registered-fixture","stream_index":7})
    );
    app.project_next_page(Phase::NextViewport, &ctx);
    assert_eq!(
        app.projects.handles[0].bindings.len(),
        8,
        "Cursor continuation must reuse its binding"
    );
    port.complete(projection(
        ticket,
        "2",
        "viewport",
        json!({"items":[],"next_cursor":null,"total_count":1,"count_accuracy":"exact"}),
    ));
    app.receive(&ctx);
    assert!(app.projects.group.as_ref().unwrap().clip.is_none());
    assert!(app.pending.is_none() && port.empty());
}
fn bounded_handle_tracking() {
    let (mut app, port, ctx) = boot();
    app.projects.active = true;
    for index in 0..4 {
        app.projects
            .remember_ticket(&format!("ticket-{index}"), PROJECT)
            .unwrap();
    }
    let first_seen = app.projects.handles[0].first_seen;
    app.projects.remember_ticket("ticket-0", PROJECT).unwrap();
    assert_eq!(app.projects.handles.len(), 4);
    assert_eq!(app.projects.handles[0].first_seen, first_seen);
    assert!(app.projects.remember_ticket("ticket-4", PROJECT).is_err());
    app.project_navigate(Navigation::Open(View::new(PROJECT.into())), &ctx);
    assert!(
        port.empty(),
        "Full unconfirmed metadata blocks more project opens"
    );
    app.projects.terminal_failure = true;
    app.project_navigate(Navigation::Cleanup, &ctx);
    assert!(!app.projects.active && app.projects.handles.len() == 4);
    for handle in &mut app.projects.handles {
        handle.first_seen = Instant::now() - Duration::from_secs(31);
    }
    app.projects.prune_handles();
    assert!(
        app.projects.handles.is_empty() && port.empty(),
        "Local expiry is bookkeeping, never an edit or release bypass"
    );
}

const SELECT_SEQUENCE: &str = "70000000-0000-4000-8000-000000000001";
const SELECT_TRACK: &str = "70000000-0000-4000-8000-000000000002";
const SELECT_SOURCE: &str = "70000000-0000-4000-8000-000000000003";
const OTHER_SEQUENCE: &str = "70000000-0000-4000-8000-000000000004";
const SELECT_TICKET: &str = "80000000-0000-4000-8000-000000000001";
const RELOAD_TICKET: &str = "80000000-0000-4000-8000-000000000002";
const RESTORED_TICKET: &str = "80000000-0000-4000-8000-000000000003";
fn selected_clip_id(index: usize) -> String {
    format!("70000000-0000-4000-8000-{:012}", index + 100)
}
fn selected_summary(ticket: &str, revision: &str, sequences: &[&str], can_redo: bool) -> Value {
    let mut value = summary(ticket, revision);
    value["sequence_count"] = json!(sequences.len());
    value["entity_count"] = json!(1 + sequences.len() * 2);
    value["history"]["can_redo"] = json!(can_redo);
    value
}
fn selected_prefix(
    app: &mut WizApp,
    port: &MockJsonClient,
    ctx: &egui::Context,
    ticket: &str,
    revision: &str,
    sequences: &[&str],
    can_redo: bool,
) -> Request {
    assert_eq!(port.complete(catalog()).command.id, "projects.list");
    app.receive(ctx);
    let request = port.complete(selected_summary(ticket, revision, sequences, can_redo));
    assert_eq!(request.params["projection"], "summary");
    app.receive(ctx);
    let rows: Vec<_> = sequences
        .iter()
        .map(|id| json!({"id":id,"kind":"sequence","name":"Fixture sequence"}))
        .collect();
    port.complete(projection(ticket, revision, "bin_page", json!({"items":rows,"next_cursor":null,"total_count":sequences.len(),"count_accuracy":"exact"})));
    app.receive(ctx);
    request
}
fn selected_tracks(
    app: &mut WizApp,
    port: &MockJsonClient,
    ctx: &egui::Context,
    ticket: &str,
    revision: &str,
    clips: usize,
) {
    let settings = app.projects.forms.sequence().unwrap()["settings"].clone();
    let request = port.complete(projection(ticket, revision, "entity", json!({"entity":{"id":SELECT_SEQUENCE,"kind":"sequence","settings":settings,"track_count":1,"locked":false}})));
    assert_eq!(request.params["sequence_id"], SELECT_SEQUENCE);
    app.receive(ctx);
    port.complete(projection(ticket, revision, "track_page", json!({"items":[{"id":SELECT_TRACK,"kind":"video","name":"Video","order":0,"clip_count":clips}],"next_cursor":null,"total_count":1,"count_accuracy":"exact"})));
    app.receive(ctx);
    let request = port.complete(projection(ticket, revision, "entity", json!({"entity":{"id":SELECT_TRACK,"kind":"video","sequence_id":SELECT_SEQUENCE,"order":0,"grid":{"num":"25","den":"1"},"clip_count":clips,"locked":false}})));
    assert_eq!(request.params["entity_id"], SELECT_TRACK);
    app.receive(ctx);
}
fn selected_page(
    ticket: &str,
    revision: &str,
    start: usize,
    count: usize,
    next: Option<&str>,
) -> Value {
    let rows: Vec<_> = (start..start + count).map(|index| json!({"id":selected_clip_id(index),"track_id":SELECT_TRACK,"source_id":SELECT_SOURCE,"sequence_start":Rational::new(index as i64,25).unwrap(),"duration":{"num":"1","den":"25"}})).collect();
    projection(
        ticket,
        revision,
        "viewport",
        json!({"items":rows,"next_cursor":next,"total_count":start+count,"count_accuracy":if next.is_some(){"lower_bound"}else{"exact"}}),
    )
}
fn selected_clip(ticket: &str, revision: &str, index: usize) -> Value {
    projection(
        ticket,
        revision,
        "entity",
        json!({"entity":{"id":selected_clip_id(index),"kind":"clip","sequence_id":SELECT_SEQUENCE,"track_id":SELECT_TRACK,"source_id":SELECT_SOURCE,"occurrence_id":selected_clip_id(index),"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"25"}},"sequence_start":Rational::new(index as i64,25).unwrap(),"duration":{"num":"1","den":"25"},"rate":{"num":"1","den":"1"},"link_group_id":null,"locked":false}}),
    )
}
fn selected_detail(
    app: &mut WizApp,
    port: &MockJsonClient,
    ctx: &egui::Context,
    ticket: &str,
    revision: &str,
    index: usize,
) {
    let request = port.complete(selected_clip(ticket, revision, index));
    assert_eq!(request.params["entity_id"], selected_clip_id(index));
    app.receive(ctx);
    let request = port.complete(projection(ticket, revision, "entity", json!({"entity":{"id":SELECT_SOURCE,"kind":"source_binding","source_kind":"solid_color","rgba":[0.2,0.4,0.8,1],"duration":{"num":"4","den":"1"}}})));
    assert_eq!(request.params["entity_id"], SELECT_SOURCE);
    app.receive(ctx);
    assert!(app.project_ready() && port.empty());
}
fn selected_sequence_created() -> (WizApp, Arc<MockJsonClient>, egui::Context) {
    let (mut app, port, ctx) = ready();
    app.scope = Some(scope());
    app.projects.forms.video_tracks = "Video".into();
    app.projects.forms.audio_tracks.clear();
    app.mutate_project(
        "sequences.create",
        app.projects.forms.sequence().unwrap(),
        &ctx,
    );
    let request = port.accept(
        json!({"project_id":PROJECT,"revision":"1","created":{"sequence_id":SELECT_SEQUENCE}}),
    );
    assert_eq!(
        request.project.unwrap().expected_revision.as_deref(),
        Some("0")
    );
    app.receive(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        SELECT_TICKET,
        "1",
        &[SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, SELECT_TICKET, "1", 0);
    port.complete(selected_page(SELECT_TICKET, "1", 0, 0, None));
    app.receive(&ctx);
    assert!(app.project_ready() && port.empty());
    (app, port, ctx)
}
fn selected_failure(
    app: &mut WizApp,
    port: &MockJsonClient,
    ctx: &egui::Context,
    code: &str,
) -> Request {
    let (request, reply) = port.pop();
    reply(Ok(Response::error(
        &request.request_id,
        nw_domain::DomainError::new(code, "Canonical selection fixture"),
    )));
    app.receive(ctx);
    request
}
fn later_page_selection_keeps_exact_target() {
    let (mut app, port, ctx) = ready();
    app.scope = Some(scope());
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        SELECT_TICKET,
        "1",
        &[SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, SELECT_TICKET, "1", 129);
    port.complete(selected_page(
        SELECT_TICKET,
        "1",
        0,
        128,
        Some("second-nonempty-page"),
    ));
    app.receive(&ctx);
    selected_detail(&mut app, &port, &ctx, SELECT_TICKET, "1", 0);
    assert_eq!(app.projects.handles[0].bindings.len(), 8);
    app.project_next_page(Phase::NextViewport, &ctx);
    let next = port.complete(selected_page(SELECT_TICKET, "1", 128, 1, None));
    assert_eq!(next.params["cursor"], "second-nonempty-page");
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap();
    assert_eq!(group.viewport.items.len(), 1);
    assert!(group.clip.is_none());
    // The same controller seam used by the clicked viewport row captures its ID.
    let selected = group.viewport.items[0].id.to_string();
    let mut view = group.view.clone();
    view.clip = Some(selected.clone());
    app.project_reselect(view, &ctx);
    assert_eq!(
        app.pending.as_ref().unwrap().request.params["replace_snapshot"],
        SELECT_TICKET
    );
    let reload = selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "1",
        &[SELECT_SEQUENCE],
        false,
    );
    assert_eq!(reload.params["revision"], "1");
    selected_tracks(&mut app, &port, &ctx, RELOAD_TICKET, "1", 129);
    let viewport = port.complete(selected_page(
        RELOAD_TICKET,
        "1",
        0,
        128,
        Some("reloaded-second-page"),
    ));
    assert!(viewport.params.get("cursor").is_none());
    app.receive(&ctx);
    assert_eq!(
        app.projects.group.as_ref().unwrap().key.snapshot,
        SELECT_TICKET,
        "Do not publish partial replacement geometry"
    );
    selected_detail(&mut app, &port, &ctx, RELOAD_TICKET, "1", 128);
    let group = app.projects.group.as_ref().unwrap().clone();
    assert_eq!(group.view.clip.as_deref(), Some(selected.as_str()));
    assert_eq!(
        group.clip.as_ref().unwrap().source_id.as_str(),
        SELECT_SOURCE
    );
    assert_eq!(
        group.clip.as_ref().unwrap().sequence_start,
        Rational::new(128, 25).unwrap()
    );
    assert!(
        !group
            .viewport
            .items
            .iter()
            .any(|clip| clip.id.as_str() == selected.as_str())
    );
    assert_eq!(group.viewport.items.len(), 128);
    assert_eq!(app.projects.handles[0].bindings.len(), 8);
    assert!(group.bytes <= GROUP_BYTES);
    app.mutate_project("clips.move", json!({"clip_ids":[group.view.clip],"delta":{"num":"2","den":"1"},"collision":"reject","link_policy":"expand"}), &ctx);
    let (edit, reply) = port.pop();
    assert_eq!(edit.params["clip_ids"], json!([selected]));
    assert_eq!(
        edit.project.as_ref().unwrap().expected_revision.as_deref(),
        Some("1")
    );
    assert_eq!(
        edit.idempotency_key.as_deref(),
        Some(edit.request_id.as_str())
    );
    let captured = serde_json::to_value(&edit).unwrap();
    reply(Err("Unknown selection edit acceptance".into()));
    app.receive(&ctx);
    assert_eq!(
        serde_json::to_value(&app.pending.as_ref().unwrap().request).unwrap(),
        captured
    );
    assert!(port.empty());
}
fn sequence_history_and_external_removal_recover() {
    let (mut app, port, ctx) = selected_sequence_created();
    app.mutate_project("history.undo", json!({}), &ctx);
    let undo = port.accept(json!({"project_id":PROJECT,"revision":"2","created":{}}));
    assert_eq!(
        undo.project.unwrap().expected_revision.as_deref(),
        Some("1")
    );
    app.receive(&ctx);
    let initial_summary = selected_prefix(&mut app, &port, &ctx, RELOAD_TICKET, "2", &[], true);
    let first_seen = app.projects.handles[0].first_seen;
    let bindings = app.projects.handles[0].bindings.len();
    assert_eq!(
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND").params["sequence_id"],
        SELECT_SEQUENCE
    );
    assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "1");
    let revalidation = port.complete(selected_summary(RELOAD_TICKET, "2", &[], true));
    assert_eq!(revalidation.command.id, initial_summary.command.id);
    assert_eq!(
        revalidation.params, initial_summary.params,
        "Revalidation reuses the exact summary binding"
    );
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap();
    assert!(app.project_ready() && port.empty());
    assert_eq!(group.key.revision, "2");
    assert_eq!(group.summary["history"]["can_redo"], true);
    assert!(
        group.view.sequence.is_none() && group.view.track.is_none() && group.view.clip.is_none()
    );
    assert!(group.sequence.is_none() && group.track.is_none() && group.clip.is_none());
    assert!(group.tracks.items.is_empty() && group.viewport.items.is_empty());
    assert!(app.projects.forms.place(group, None, None).is_err());
    assert!(app.status.contains("unavailable"));
    assert_eq!(app.projects.handles[0].bindings.len(), bindings);
    assert_eq!(app.projects.handles[0].first_seen, first_seen);
    app.mutate_project("history.redo", json!({}), &ctx);
    let redo = port.accept(json!({"project_id":PROJECT,"revision":"3","created":{}}));
    assert_eq!(
        redo.project.unwrap().expected_revision.as_deref(),
        Some("2")
    );
    app.receive(&ctx);
    assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "2");
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RESTORED_TICKET,
        "3",
        &[SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, RESTORED_TICKET, "3", 0);
    port.complete(selected_page(RESTORED_TICKET, "3", 0, 0, None));
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap();
    assert_eq!(group.view.sequence.as_deref(), Some(SELECT_SEQUENCE));
    assert_eq!(group.view.track.as_deref(), Some(SELECT_TRACK));
    assert!(app.project_ready() && port.empty());

    let (mut app, port, ctx) = selected_sequence_created();
    // An external Undo removed the selected sequence; another sequence remains.
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "2",
        &[OTHER_SEQUENCE],
        true,
    );
    assert_eq!(
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND").params["sequence_id"],
        SELECT_SEQUENCE
    );
    port.complete(selected_summary(
        RELOAD_TICKET,
        "2",
        &[OTHER_SEQUENCE],
        true,
    ));
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap();
    assert!(app.project_ready() && port.empty());
    assert_eq!(group.summary["sequence_count"], 1);
    assert_eq!(group.bins.items[0].id.as_str(), OTHER_SEQUENCE);
    assert!(
        group.view.sequence.is_none(),
        "Do not replace a missing human selection with another sequence"
    );
    assert_eq!(group.summary["history"]["can_redo"], true);
    app.project_refresh(&ctx);
    selected_failure(&mut app, &port, &ctx, "PERMISSION_DENIED");
    assert!(app.projects.stale && port.empty());
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RESTORED_TICKET,
        "3",
        &[OTHER_SEQUENCE, SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, RESTORED_TICKET, "3", 0);
    port.complete(selected_page(RESTORED_TICKET, "3", 0, 0, None));
    app.receive(&ctx);
    assert_eq!(
        app.projects
            .group
            .as_ref()
            .unwrap()
            .view
            .sequence
            .as_deref(),
        Some(SELECT_SEQUENCE),
        "Explicit Refresh rechecks the bounded restoration hint, not the first other row"
    );
    assert!(app.project_ready() && port.empty());
}
fn missing_sequence_stage() -> (WizApp, Arc<MockJsonClient>, egui::Context) {
    let (mut app, port, ctx) = selected_sequence_created();
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "2",
        &[OTHER_SEQUENCE],
        true,
    );
    (app, port, ctx)
}
fn selection_recovery_rejects_other_failures() {
    selected_track_and_clip_recovery();
    for code in [
        "PERMISSION_DENIED",
        "SNAPSHOT_EXPIRED",
        "CANCELLED",
        "STALE_REVISION",
    ] {
        let (mut app, port, ctx) = missing_sequence_stage();
        selected_failure(&mut app, &port, &ctx, code);
        assert!(app.projects.stale && app.projects.staging.is_none());
        assert!(app.pending.is_none() && port.empty());
        assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "1");
    }
    let (mut app, port, ctx) = missing_sequence_stage();
    let (request, reply) = port.pop();
    let captured = serde_json::to_value(&request).unwrap();
    reply(Err("Unknown query outcome".into()));
    app.receive(&ctx);
    assert_eq!(
        serde_json::to_value(&app.pending.as_ref().unwrap().request).unwrap(),
        captured
    );
    assert!(port.empty() && app.projects.staging.is_some());
    app.retry_pending(&ctx);
    let (retried, reply) = port.pop();
    assert_eq!(serde_json::to_value(&retried).unwrap(), captured);
    reply(Ok(Response::error(
        &retried.request_id,
        nw_domain::DomainError::object_not_found(),
    )));
    app.receive(&ctx);
    assert_eq!(
        app.pending.as_ref().unwrap().request.params["projection"],
        "summary"
    );
    // A failed revalidation is terminal; it must not start a second recovery.
    selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND");
    assert!(port.empty() && app.pending.is_none() && app.projects.staging.is_none());

    let (mut app, port, ctx) = missing_sequence_stage();
    port.complete(projection(
        RELOAD_TICKET,
        "2",
        "entity",
        json!({"entity":{"id":SELECT_SEQUENCE,"kind":"clip"}}),
    ));
    app.receive(&ctx);
    assert!(port.empty() && app.pending.is_none() && app.projects.staging.is_none());
    for changed in [
        "generation",
        "phase",
        "request_project",
        "request_revision",
        "request_entity",
    ] {
        let (mut app, port, ctx) = missing_sequence_stage();
        match changed {
            "generation" => app.projects.generation += 1,
            "phase" => app.projects.staging.as_mut().unwrap().phase = Phase::Track,
            "request_project" => {
                app.pending
                    .as_mut()
                    .unwrap()
                    .request
                    .project
                    .as_mut()
                    .unwrap()
                    .id = OTHER_SEQUENCE.into()
            }
            "request_revision" => {
                app.pending.as_mut().unwrap().request.params["revision"] = json!("999")
            }
            "request_entity" => {
                app.pending.as_mut().unwrap().request.params["sequence_id"] = json!(OTHER_SEQUENCE)
            }
            _ => unreachable!(),
        }
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND");
        assert!(
            port.empty() && app.pending.is_none() && app.projects.staging.is_none(),
            "{changed} must not authorize recovery"
        );
        assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "1");
    }
    for changed in [
        "project",
        "revision",
        "ticket",
        "projection",
        "history",
        "expiry",
    ] {
        let (mut app, port, ctx) = missing_sequence_stage();
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND");
        let mut response = selected_summary(RELOAD_TICKET, "2", &[OTHER_SEQUENCE], true);
        match changed {
            "project" => response["project_id"] = json!(OTHER_SEQUENCE),
            "revision" => response["project_revision"] = json!("999"),
            "ticket" => response["snapshot_id"] = json!(RESTORED_TICKET),
            "projection" => response["projection"] = json!("entity"),
            "history" => response["history"]["can_redo"] = json!(false),
            "expiry" => {
                let expired = Instant::now() - Duration::from_secs(31);
                app.projects
                    .staging
                    .as_mut()
                    .unwrap()
                    .group
                    .as_mut()
                    .unwrap()
                    .key
                    .first_seen = expired;
                app.projects.handles[0].first_seen = expired;
            }
            _ => unreachable!(),
        }
        port.complete(response);
        app.receive(&ctx);
        assert!(
            app.projects.stale && app.projects.staging.is_none(),
            "{changed} must fail closed"
        );
        assert_eq!(app.projects.group.as_ref().unwrap().key.revision, "1");
        assert!(app.pending.is_none() && port.empty());
    }
}

fn selected_track_and_clip_recovery() {
    let (mut app, port, ctx) = selected_sequence_created();
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "2",
        &[SELECT_SEQUENCE, OTHER_SEQUENCE],
        true,
    );
    let settings = app.projects.forms.sequence().unwrap()["settings"].clone();
    port.complete(projection(RELOAD_TICKET, "2", "entity", json!({"entity":{"id":SELECT_SEQUENCE,"kind":"sequence","settings":settings,"track_count":0,"locked":false}})));
    app.receive(&ctx);
    port.complete(projection(
        RELOAD_TICKET,
        "2",
        "track_page",
        json!({"items":[],"next_cursor":null,"total_count":0,"count_accuracy":"exact"}),
    ));
    app.receive(&ctx);
    let bindings = app.projects.handles[0].bindings.len();
    assert_eq!(
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND").params["entity_id"],
        SELECT_TRACK
    );
    port.complete(selected_summary(
        RELOAD_TICKET,
        "2",
        &[SELECT_SEQUENCE, OTHER_SEQUENCE],
        true,
    ));
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap();
    assert!(app.project_ready() && port.empty());
    assert!(group.sequence.is_some() && group.view.sequence.as_deref() == Some(SELECT_SEQUENCE));
    assert!(group.view.track.is_none() && group.track.is_none() && group.view.clip.is_none());
    assert!(app.projects.forms.place(group, None, None).is_err());
    assert_eq!(app.projects.handles[0].bindings.len(), bindings);
    let mut replacement = group.view.clone();
    replacement.sequence = Some(group.bins.items[1].id.to_string());
    replacement.track = None;
    replacement.clip = None;
    app.project_reselect(replacement, &ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RESTORED_TICKET,
        "2",
        &[SELECT_SEQUENCE, OTHER_SEQUENCE],
        true,
    );
    assert_eq!(
        selected_failure(&mut app, &port, &ctx, "PERMISSION_DENIED").params["sequence_id"],
        OTHER_SEQUENCE,
        "An explicit row choice supersedes the old restoration hint"
    );

    let (mut app, port, ctx) = selected_sequence_created();
    let mut view = app.projects.group.as_ref().unwrap().view.clone();
    view.clip = Some(selected_clip_id(128));
    app.project_reselect(view, &ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "1",
        &[SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, RELOAD_TICKET, "1", 1);
    port.complete(selected_page(RELOAD_TICKET, "1", 0, 1, None));
    app.receive(&ctx);
    let bindings = app.projects.handles[0].bindings.len();
    assert_eq!(
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND").params["entity_id"],
        selected_clip_id(128)
    );
    port.complete(selected_summary(
        RELOAD_TICKET,
        "1",
        &[SELECT_SEQUENCE],
        false,
    ));
    app.receive(&ctx);
    let group = app.projects.group.as_ref().unwrap();
    assert!(app.project_ready() && port.empty());
    assert!(group.sequence.is_some() && group.track.is_some());
    assert!(group.view.clip.is_none() && group.clip.is_none() && group.source.is_none());
    assert_eq!(group.viewport.items[0].id.as_str(), selected_clip_id(0));
    assert_eq!(app.projects.handles[0].bindings.len(), bindings);
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RESTORED_TICKET,
        "2",
        &[SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, RESTORED_TICKET, "2", 2);
    port.complete(selected_page(
        RESTORED_TICKET,
        "2",
        0,
        1,
        Some("restored-target-on-another-page"),
    ));
    app.receive(&ctx);
    selected_detail(&mut app, &port, &ctx, RESTORED_TICKET, "2", 128);
    assert_eq!(
        app.projects.group.as_ref().unwrap().view.clip.as_deref(),
        Some(selected_clip_id(128).as_str())
    );

    let (mut app, port, ctx) = missing_sequence_stage();
    let (request, reply) = port.pop();
    let captured = serde_json::to_value(&request).unwrap();
    reply(Ok(Response::error(
        "unrelated-response",
        nw_domain::DomainError::object_not_found(),
    )));
    app.receive(&ctx);
    assert_eq!(
        serde_json::to_value(&app.pending.as_ref().unwrap().request).unwrap(),
        captured
    );
    assert!(port.empty() && app.projects.staging.is_some());

    let (mut app, port, ctx) = selected_sequence_created();
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "2",
        &[SELECT_SEQUENCE],
        false,
    );
    selected_tracks(&mut app, &port, &ctx, RELOAD_TICKET, "2", 1);
    port.complete(selected_page(RELOAD_TICKET, "2", 0, 1, None));
    app.receive(&ctx);
    port.complete(selected_clip(RELOAD_TICKET, "2", 0));
    app.receive(&ctx);
    assert_eq!(
        selected_failure(&mut app, &port, &ctx, "OBJECT_NOT_FOUND").params["entity_id"],
        SELECT_SOURCE
    );
    assert!(app.projects.stale && app.projects.staging.is_none());
    assert!(
        app.pending.is_none() && port.empty(),
        "A source failure is not proof that the selected clip is absent"
    );
}

// These controller cases use captured projections and the same handlers as the
// visible controls. They do not claim OS/browser interaction qualification.
const GROUP_OTHER_TRACK: &str = "70000000-0000-4000-8000-000000000005";
fn group_selection_ready(count: usize) -> (WizApp, Arc<MockJsonClient>, egui::Context) {
    let (mut app, port, ctx) = ready();
    app.scope = Some(scope());
    app.project_refresh(&ctx);
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        SELECT_TICKET,
        "1",
        &[SELECT_SEQUENCE],
        false,
    );
    if count > PAGE {
        // Both navigation targets come from this canonical captured track page.
        let settings = app.projects.forms.sequence().unwrap()["settings"].clone();
        let request = port.complete(projection(SELECT_TICKET, "1", "entity", json!({"entity":{"id":SELECT_SEQUENCE,"kind":"sequence","settings":settings,"track_count":2,"locked":false}})));
        assert_eq!(request.params["sequence_id"], SELECT_SEQUENCE);
        app.receive(&ctx);
        port.complete(projection(SELECT_TICKET, "1", "track_page", json!({"items":[{"id":SELECT_TRACK,"kind":"video","name":"Video","order":0,"clip_count":count},{"id":GROUP_OTHER_TRACK,"kind":"video","name":"Other video","order":1,"clip_count":0}],"next_cursor":null,"total_count":2,"count_accuracy":"exact"})));
        app.receive(&ctx);
        let request = port.complete(projection(SELECT_TICKET, "1", "entity", json!({"entity":{"id":SELECT_TRACK,"kind":"video","sequence_id":SELECT_SEQUENCE,"order":0,"grid":{"num":"25","den":"1"},"clip_count":count,"locked":false}})));
        assert_eq!(request.params["entity_id"], SELECT_TRACK);
        app.receive(&ctx);
    } else {
        selected_tracks(&mut app, &port, &ctx, SELECT_TICKET, "1", count);
    }
    port.complete(selected_page(
        SELECT_TICKET,
        "1",
        0,
        count.min(PAGE),
        (count > PAGE).then_some("selection-next-page"),
    ));
    app.receive(&ctx);
    selected_detail(&mut app, &port, &ctx, SELECT_TICKET, "1", 0);
    (app, port, ctx)
}
fn clip_group_selection_and_requests() {
    let (mut app, port, ctx) = group_selection_ready(129);
    app.project_toggle_clip_seed(BIN);
    assert!(app.projects.clip_selection.is_none());
    app.project_toggle_clip_seed(&selected_clip_id(0));
    assert!(app.project_link_params(false).is_none());
    app.project_toggle_clip_seed(&selected_clip_id(1));
    let expected =
        json!({"clip_ids":[selected_clip_id(0),selected_clip_id(1)],"link_policy":"expand"});
    assert_eq!(app.project_link_params(false), Some(expected.clone()));
    assert_eq!(app.project_link_params(true), Some(expected.clone()));
    let captured = app.projects.clip_selection.clone();
    app.projects.clip_selection.as_mut().unwrap().sequence = BIN.into();
    assert!(app.project_link_params(false).is_none());
    assert!(app.project_link_params(true).is_none());
    app.project_link_selection(false, &ctx);
    assert!(port.empty(), "Mismatched selection context cannot submit");
    app.projects.clip_selection = captured;
    assert_eq!(
        app.projects.group.as_ref().unwrap().view.clip.as_deref(),
        Some(selected_clip_id(0).as_str())
    );
    app.project_toggle_clip_seed(&selected_clip_id(1));
    assert!(app.project_link_params(false).is_none());
    for index in 1..PAGE {
        app.project_toggle_clip_seed(&selected_clip_id(index));
    }
    assert_eq!(
        app.projects.clip_selection.as_ref().unwrap().seeds.len(),
        PAGE
    );
    app.project_next_page(Phase::NextViewport, &ctx);
    assert!(
        app.project_link_params(false).is_none(),
        "In-flight reads cannot submit topology"
    );
    port.complete(selected_page(SELECT_TICKET, "1", PAGE, 1, None));
    app.receive(&ctx);
    app.project_toggle_clip_seed(&selected_clip_id(PAGE));
    assert_eq!(
        app.projects.clip_selection.as_ref().unwrap().seeds.len(),
        PAGE
    );
    assert!(
        !app.projects
            .clip_selection
            .as_ref()
            .unwrap()
            .seeds
            .contains(&selected_clip_id(PAGE))
    );
    assert!(app.error.as_ref().unwrap().contains("128"));
    assert!(app.projects.group.as_ref().unwrap().clip.is_none());
    assert_eq!(
        app.project_link_params(false).unwrap()["clip_ids"]
            .as_array()
            .unwrap()
            .len(),
        PAGE
    );
    let mut view = app.projects.group.as_ref().unwrap().view.clone();
    view.track = Some(
        app.projects
            .group
            .as_ref()
            .unwrap()
            .tracks
            .items
            .iter()
            .find(|track| Some(track.id.as_str()) != view.track.as_deref())
            .expect("A different track captured in the same sequence")
            .id
            .to_string(),
    );
    assert_eq!(view.track.as_deref(), Some(GROUP_OTHER_TRACK));
    assert_ne!(view.track.as_deref(), Some(SELECT_TRACK));
    app.project_reselect(view, &ctx);
    assert_eq!(
        app.projects.clip_selection.as_ref().unwrap().seeds.len(),
        PAGE,
        "Same revision track navigation retains only captured seeds"
    );
    assert!(app.project_link_params(false).is_none());

    let (mut app, port, ctx) = group_selection_ready(2);
    app.project_toggle_clip_seed(&selected_clip_id(0));
    app.project_toggle_clip_seed(&selected_clip_id(1));
    app.project_link_selection(false, &ctx);
    assert!(app.projects.stale);
    app.project_link_selection(false, &ctx);
    app.project_toggle_clip_seed(&selected_clip_id(0));
    assert_eq!(app.projects.clip_selection.as_ref().unwrap().seeds.len(), 2);
    let (request, reply) = port.pop();
    assert!(port.empty());
    assert_eq!(request.command.id, "clips.link");
    assert_eq!(request.params, expected);
    assert_eq!(request.project.as_ref().unwrap().id, PROJECT);
    assert_eq!(
        request
            .project
            .as_ref()
            .unwrap()
            .expected_revision
            .as_deref(),
        Some("1")
    );
    assert_eq!(request.replay_scope.as_deref(), Some(scope().as_str()));
    assert_eq!(
        request.idempotency_key.as_deref(),
        Some(request.request_id.as_str())
    );
    reply(Ok(Response::error(
        &request.request_id,
        nw_domain::DomainError::new("STALE_REVISION", "Canonical competitor"),
    )));
    app.receive(&ctx);
    assert!(app.projects.stale && app.projects.terminal_failure);
    assert!(
        app.projects
            .group
            .as_ref()
            .unwrap()
            .clip
            .as_ref()
            .unwrap()
            .link_group_id
            .is_none()
    );
    app.project_link_selection(true, &ctx);
    assert!(port.empty());
    app.project_refresh(&ctx);
    assert!(
        app.projects.clip_selection.is_none(),
        "Explicit Refresh discards stale selection"
    );

    let (mut app, port, ctx) = group_selection_ready(2);
    app.project_toggle_clip_seed(&selected_clip_id(0));
    app.project_toggle_clip_seed(&selected_clip_id(1));
    app.project_link_selection(false, &ctx);
    port.accept(json!({"project_id":PROJECT,"revision":"2","created":{"link_group_id":BIN,"clip_ids":[selected_clip_id(1),selected_clip_id(0)]}}));
    app.receive(&ctx);
    assert!(
        app.projects.clip_selection.is_none(),
        "Accepted revision clears seeds before reload"
    );
    assert!(
        app.projects
            .group
            .as_ref()
            .unwrap()
            .clip
            .as_ref()
            .unwrap()
            .link_group_id
            .is_none(),
        "Receipt never edits displayed topology optimistically"
    );
    selected_prefix(
        &mut app,
        &port,
        &ctx,
        RELOAD_TICKET,
        "2",
        &[SELECT_SEQUENCE],
        false,
    );
    assert_eq!(
        app.projects
            .staging
            .as_ref()
            .unwrap()
            .group
            .as_ref()
            .unwrap()
            .view
            .clip
            .as_deref(),
        Some(selected_clip_id(0).as_str()),
        "Expanded member order must not replace the inspected clip"
    );
    selected_tracks(&mut app, &port, &ctx, RELOAD_TICKET, "2", 2);
    port.complete(selected_page(RELOAD_TICKET, "2", 0, 2, None));
    app.receive(&ctx);
    let mut linked = selected_clip(RELOAD_TICKET, "2", 0);
    linked["entity"]["link_group_id"] = json!(BIN);
    linked["entity"]["occurrence_id"] = json!(selected_clip_id(99));
    port.complete(linked);
    app.receive(&ctx);
    port.complete(projection(
        RELOAD_TICKET,
        "2",
        "entity",
        json!({"entity":{"id":SELECT_SOURCE,"kind":"source_binding","source_kind":"solid_color"}}),
    ));
    app.receive(&ctx);
    assert!(app.project_ready());
    let group = app.projects.group.as_ref().unwrap();
    let clip = group.clip.as_ref().unwrap();
    let inspected = group.view.clip.as_deref().unwrap();
    assert_ne!(inspected, clip.occurrence_id.as_str());
    assert_ne!(inspected, clip.source_id.as_str());
    assert_ne!(
        Some(inspected),
        clip.link_group_id.as_ref().map(|id| id.as_str())
    );
    // Invalid controller captures must never guess another payload identity or
    // send a request. Production capture publication already checks correlation.
    for invalid in [
        "missing id",
        "invalid id",
        "missing payload",
        "sequence",
        "track",
    ] {
        let captured = app.projects.group.as_ref().unwrap().clone();
        let group = Arc::make_mut(app.projects.group.as_mut().unwrap());
        match invalid {
            "missing id" => group.view.clip = None,
            "invalid id" => group.view.clip = Some("invalid-entity-id".into()),
            "missing payload" => group.clip = None,
            "sequence" => {
                group.clip.as_mut().unwrap().sequence_id =
                    nw_domain::projects::EntityId::parse(BIN).unwrap();
            }
            "track" => {
                group.clip.as_mut().unwrap().track_id =
                    nw_domain::projects::EntityId::parse(BIN).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(app.project_link_params(true).is_none(), "Refuse {invalid}");
        app.project_link_selection(true, &ctx);
        assert!(port.empty(), "No dispatch for {invalid}");
        app.projects.group = Some(captured);
    }
    app.project_link_selection(true, &ctx);
    let unlink = selected_failure(&mut app, &port, &ctx, "PERMISSION_DENIED");
    assert_eq!(unlink.command.id, "clips.unlink");
    assert_eq!(
        unlink.params,
        json!({"clip_ids":[selected_clip_id(0)],"link_policy":"expand"})
    );
    assert_eq!(
        unlink
            .project
            .as_ref()
            .unwrap()
            .expected_revision
            .as_deref(),
        Some("2")
    );
    assert!(
        app.projects.stale
            && app
                .projects
                .group
                .as_ref()
                .unwrap()
                .clip
                .as_ref()
                .unwrap()
                .link_group_id
                .is_some()
    );
    assert!(port.empty());

    for context in ["project", "sequence", "revision", "leave"] {
        let (mut app, _, ctx) = group_selection_ready(2);
        app.project_toggle_clip_seed(&selected_clip_id(0));
        let mut view = app.projects.group.as_ref().unwrap().view.clone();
        match context {
            "project" => view.project = BIN.into(),
            "sequence" => view.sequence = Some(BIN.into()),
            "revision" => view.revision = Some("2".into()),
            _ => {}
        }
        if context == "leave" {
            app.leave_projects(&ctx);
        } else {
            app.project_navigate(Navigation::Open(view), &ctx);
        }
        assert!(
            app.projects.clip_selection.is_none(),
            "Selection must clear on {context}"
        );
    }
}
