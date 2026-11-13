//! Catalog regressions called by the original inventory test entrypoints.
use super::*;
use nw_domain::commands::ProjectRef;

struct Fixture {
    service: Arc<Service>,
    scopes: Mutex<HashMap<String, String>>,
    root: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        Self {
            service,
            scopes: Mutex::new(HashMap::new()),
            root,
        }
    }
    async fn write(&self, context: &Context, command: &str, params: Value) -> Value {
        self.write_project(context, command, params, None).await
    }
    async fn write_project(
        &self,
        context: &Context,
        command: &str,
        params: Value,
        project: Option<ProjectRef>,
    ) -> Value {
        let retained = self.scopes.lock().unwrap().get(&context.principal).cloned();
        let scope = if let Some(scope) = retained {
            scope
        } else {
            let response = view(
                self.service
                    .dispatch(
                        context.clone(),
                        Request::query("access.replay_scope.create", json!({})),
                    )
                    .await,
            );
            let scope = response["replay_scope"].as_str().unwrap().to_owned();
            self.scopes
                .lock()
                .unwrap()
                .insert(context.principal.clone(), scope.clone());
            scope
        };
        let mut request = Request::query(command, params);
        request.project = project;
        request.replay_scope = Some(scope);
        request.idempotency_key = Some(Uuid::new_v4().to_string());
        let response = self.service.dispatch(context.clone(), request).await;
        assert!(response.ok, "{response:?}");
        assert!(response.receipt_id.is_some());
        response.result.unwrap()
    }
    async fn project(&self, context: &Context, name: &str) -> String {
        self.write(context, "projects.create", json!({"name":name}))
            .await["project_id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn profile(&self, name: &str, ids: &[String]) -> Context {
        let old: Option<AccessProfile> =
            self.service.store.get(&format!("profile/{name}")).unwrap();
        let grants: Vec<_> = ids
            .iter()
            .map(|id| json!({"kind":"project","id":id,"actions":["read"]}))
            .collect();
        self.write(&self.service.owner(), "access.profiles.put", json!({
            "name":name,"expected_revision":old.as_ref().map(|p| p.revision),
            "enabled":true,"mode":"operate","owned_object_access":false,
            "allowed_command_ids":["access.replay_scope.create","projects.list","projects.inspect","assets.list","jobs.list","artifacts.list","batches.list"],
            "object_grants":grants,"read_roots":[self.root.path().to_str().unwrap()],"write_roots":[]
        })).await;
        let context = self.service.profile_context(name).unwrap();
        if let Some(old) = old {
            assert_eq!(context.principal, old.principal);
            assert_eq!(context.profile.as_deref(), Some(old.id.as_str()));
        }
        context
    }
    async fn update(&self, context: &Context, change: impl FnOnce(&mut Value)) {
        let old: AccessProfile = self
            .service
            .store
            .get(&format!("profile/{}", context.profile.as_ref().unwrap()))
            .unwrap()
            .unwrap();
        let mut input = json!({"name":old.id,"expected_revision":old.revision,"enabled":old.enabled,
            "mode":old.mode,"owned_object_access":old.owned_object_access,"client_sessions":old.client_sessions,
            "allowed_command_ids":old.allowed_command_ids,"object_grants":old.object_grants,
            "read_roots":old.read_roots,"write_roots":old.write_roots});
        change(&mut input);
        self.write(&self.service.owner(), "access.profiles.update", input)
            .await;
        let current: AccessProfile = self
            .service
            .store
            .get(&format!("profile/{}", context.profile.as_ref().unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(current.principal, context.principal);
        assert_eq!(Some(&current.id), context.profile.as_ref());
    }
    async fn list(&self, context: &Context, params: Value) -> Response {
        self.service
            .dispatch(context.clone(), Request::query("projects.list", params))
            .await
    }
    async fn ticket(&self, context: &Context, project: &str, replacement: Option<&str>) -> String {
        let mut request = Request::query(
            "projects.inspect",
            json!({"projection":"summary","replace_snapshot":replacement}),
        );
        request.project = Some(ProjectRef {
            id: project.into(),
            expected_revision: None,
        });
        let summary = view(self.service.dispatch(context.clone(), request).await);
        let id = summary["snapshot_id"].as_str().unwrap().to_owned();
        assert!(
            self.service
                .snapshots
                .lock()
                .unwrap()
                .projects
                .contains_key(&id)
        );
        id
    }
    fn count(&self) -> usize {
        self.service.snapshots.lock().unwrap().count()
    }
}
fn view(response: Response) -> Value {
    assert!(response.ok, "{response:?}");
    assert!(response.receipt_id.is_none());
    response.result.unwrap()
}
fn failed(response: Response, code: &str) {
    assert!(!response.ok, "{response:?}");
    assert!(response.receipt_id.is_none());
    assert_eq!(response.error.unwrap().code, code);
}
fn header(id: &str, creator: &str, name: &str) -> Value {
    json!({"id":id,"creator":creator,"name":name,"revision":"18446744073709551615",
        "schema_major":2,"envelope_version":7,"private_path":"/private/never-catalog",
        "future":{"opaque":["not a content projection"]}})
}

pub(super) fn legacy_header(row: &Value) -> Value {
    let current: nw_domain::projects::views::ProjectSummary =
        serde_json::from_value(row.clone()).expect("R2_PUBLIC_HEADER_DECODE");
    assert!(
        current.known_authorization().is_some(),
        "R2_PUBLIC_AUTHORIZATION_UNKNOWN"
    );
    let mut legacy = row.clone();
    assert!(
        legacy
            .as_object_mut()
            .unwrap()
            .remove("authorization")
            .is_some()
    );
    legacy
}

pub(super) async fn headers_and_rights() {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let browser = f.service.profile_context("browser").unwrap();
    let own = f.project(&browser, "Browser project").await;
    let foreign = f.project(&owner, "Private owner project").await;
    // Upgraded profiles are changed only by this explicit owner-reviewed update.
    f.update(&browser, |input| {
        input["allowed_command_ids"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id != "projects.list")
    })
    .await;
    failed(
        f.list(&browser, json!({"limit":64})).await,
        "PERMISSION_DENIED",
    );
    f.update(&browser, |input| {
        let commands = input["allowed_command_ids"].as_array_mut().unwrap();
        if !commands.iter().any(|id| id == "projects.list") {
            commands.push(json!("projects.list"));
        }
    })
    .await;
    let page = view(f.list(&browser, json!({"limit":64})).await);
    assert_eq!(page["total_count"], 1);
    assert_eq!(page["items"][0]["id"], own);
    assert!(!page.to_string().contains(&foreign));
    assert!(!page.to_string().contains(&owner.principal));
    assert_eq!(
        legacy_header(&page["items"][0]).as_object().unwrap().len(),
        6
    );
    let reader = f
        .profile("catalog-reader", std::slice::from_ref(&foreign))
        .await;
    let read = view(f.list(&reader, json!({})).await);
    assert_eq!(read["items"][0]["id"], foreign);
    assert_eq!(read["total_count"], 1);
    assert!(!read.to_string().contains(&own));
    let profile: AccessProfile = f
        .service
        .store
        .get("profile/catalog-reader")
        .unwrap()
        .unwrap();
    f.update(&reader, |input| input["object_grants"] = json!([]))
        .await;
    let updated: AccessProfile = f
        .service
        .store
        .get("profile/catalog-reader")
        .unwrap()
        .unwrap();
    assert_eq!(updated.read_roots, profile.read_roots);
    assert_eq!(updated.write_roots, profile.write_roots);
    assert_eq!(view(f.list(&reader, json!({})).await)["total_count"], 0);
    f.update(&reader, |input| {
        input["allowed_command_ids"] = json!(["access.replay_scope.create", "projects.inspect"])
    })
    .await;
    failed(f.list(&reader, json!({})).await, "PERMISSION_DENIED");
    let mut with_project = Request::query("projects.list", json!({}));
    with_project.project = Some(ProjectRef {
        id: own.clone(),
        expected_revision: None,
    });
    failed(
        f.service.dispatch(owner.clone(), with_project).await,
        "VALIDATION_ERROR",
    );
    let mut with_identity = Request::query("projects.list", json!({}));
    with_identity.replay_scope = Some("not-a-query-field".into());
    with_identity.idempotency_key = Some("not-a-query-field".into());
    failed(
        f.service.dispatch(owner.clone(), with_identity).await,
        "VALIDATION_ERROR",
    );

    // Unsupported common headers remain discoverable without any entities or journal.
    let id = "00000000-0000-4000-8000-000000000000";
    let key = format!("project/{id}");
    let name = "é".repeat(64);
    let raw = header(id, &owner.principal, &name);
    f.service.store.put(&key, &raw).unwrap();
    let catalog = view(f.list(&owner, json!({"limit":64})).await);
    assert_eq!(
        legacy_header(&catalog["items"][0]),
        json!({"id":id,"name":name,"revision":"18446744073709551615","schema_major":2,"envelope_version":7,"compatibility":"read_only_unsupported"})
    );
    assert!(!catalog.to_string().contains("private"));
    assert!(!catalog.to_string().contains("creator"));
    assert!(!catalog.to_string().contains("future"));
    let ids: Vec<_> = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert!(serde_json::to_vec(&catalog).unwrap().len() < MAX_PAGE_BYTES);
    // A malformed unauthorized name must not be projected, counted or disclosed.
    let mut bad = raw.clone();
    bad["name"] = json!({"private":"wrong type"});
    f.service.store.put(&key, &bad).unwrap();
    assert_eq!(view(f.list(&browser, json!({})).await)["total_count"], 1);
    failed(f.list(&owner, json!({})).await, "PROJECT_CORRUPT");
    for (field, invalid) in [
        ("name", json!("")),
        ("name", json!("é".repeat(65))),
        ("name", json!("bad\nname")),
        ("revision", json!("01")),
        ("revision", json!(1)),
        ("revision", json!("18446744073709551616")),
        ("id", json!("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")),
        ("id", json!(foreign)),
        ("schema_major", json!("1")),
        ("envelope_version", json!(-1)),
    ] {
        let mut bad = raw.clone();
        bad[field] = invalid;
        f.service.store.put(&key, &bad).unwrap();
        failed(f.list(&owner, json!({})).await, "PROJECT_CORRUPT");
    }
    let mut bounded_header = raw.clone();
    bounded_header["opaque"] = json!("");
    let padding = 64 * 1024 - serde_json::to_vec(&bounded_header).unwrap().len();
    bounded_header["opaque"] = json!("x".repeat(padding));
    assert_eq!(
        serde_json::to_vec(&bounded_header).unwrap().len(),
        64 * 1024
    );
    f.service.store.put(&key, &bounded_header).unwrap();
    let bounded_page = view(f.list(&owner, json!({"limit":64})).await);
    assert!(serde_json::to_vec(&bounded_page).unwrap().len() < MAX_PAGE_BYTES);
    let mut oversized = bounded_header;
    oversized["opaque"] = json!("x".repeat(padding + 1));
    f.service.store.put(&key, &oversized).unwrap();
    failed(f.list(&owner, json!({})).await, "RESOURCE_LIMIT");
    failed(f.list(&browser, json!({})).await, "RESOURCE_LIMIT");
    f.service.store.put(&key, &raw).unwrap();
    // The finite header ceiling applies before authorization, even to an empty view.
    let empty = f.profile("empty", &[]).await;
    for index in 1..=61 {
        let id = format!("00000000-0000-4000-8000-{index:012}");
        f.service
            .store
            .put(
                &format!("project/{id}"),
                &header(&id, &owner.principal, "Bounded"),
            )
            .unwrap();
    }
    let bounded = view(f.list(&owner, json!({"limit":64})).await);
    assert_eq!(bounded["total_count"], 64);
    assert!(bounded["next_cursor"].is_null());
    assert_eq!(
        view(f.list(&owner, json!({"limit":512})).await)["total_count"],
        64
    );
    assert_eq!(f.count(), 0);
    let overflow = "00000000-0000-4000-8000-000000000062";
    f.service
        .store
        .put(
            &format!("project/{overflow}"),
            &header(overflow, &owner.principal, "Overflow"),
        )
        .unwrap();
    failed(f.list(&empty, json!({"limit":1})).await, "RESOURCE_LIMIT");
    assert_eq!(f.count(), 0);
}

pub(super) async fn retirement_and_quotas() {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let mut projects = Vec::new();
    for index in 0..6 {
        projects.push(f.project(&owner, &format!("Navigation {index}")).await);
    }
    let reader = f.profile("navigation", &projects).await;
    let other = f.profile("other", &projects).await;
    let browser = f.service.profile_context("browser").unwrap();
    let browser_project = f.project(&browser, "Browser ticket").await;
    let browser_ticket = f.ticket(&browser, &browser_project, None).await;
    let ticket = f.ticket(&reader, &projects[0], None).await;
    let edited = f.write_project(&owner, "sequences.create", json!({"name":"After capture",
        "settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
        "tracks":[{"kind":"video","name":"Video"}]}), Some(ProjectRef { id:projects[0].clone(), expected_revision:Some("0".into()) })).await;
    assert_eq!(edited["revision"], "1");
    assert!(
        f.service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&ticket),
        "The retired ticket must really retain the older project revision"
    );
    {
        let cache = f.service.snapshots.lock().unwrap();
        let retained = &cache.projects[&ticket];
        assert!(retained.owned_by(&reader));
        // Predicate-only test, not an authenticated alternate-profile request.
        let same_principal_other_profile = Context {
            principal: reader.principal.clone(),
            profile: Some("other".into()),
        };
        assert!(!retained.owned_by(&same_principal_other_profile));
    }
    for context in [&owner, &other] {
        view(
            f.list(context, json!({"limit":64,"replace_snapshot":ticket}))
                .await,
        );
        assert!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .projects
                .contains_key(&ticket)
        );
    }
    view(
        f.list(
            &owner,
            json!({"limit":64,"replace_snapshot":browser_ticket}),
        )
        .await,
    );
    assert!(
        f.service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&browser_ticket)
    );
    for command in ["assets.list", "jobs.list", "artifacts.list", "batches.list"] {
        view(
            f.service
                .dispatch(
                    reader.clone(),
                    Request::query(command, json!({"limit":64,"replace_snapshot":ticket})),
                )
                .await,
        );
        assert!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .projects
                .contains_key(&ticket),
            "Only the new catalog may retire a project ticket"
        );
    }
    // Old inspect replacement is still restricted to the same project.
    let second = f.ticket(&reader, &projects[1], Some(&ticket)).await;
    assert!(
        f.service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&ticket)
    );
    view(
        f.list(&reader, json!({"limit":64,"replace_snapshot":second}))
            .await,
    );
    f.update(&reader, |input| input["object_grants"] = json!([]))
        .await;
    let released = view(
        f.list(&reader, json!({"limit":64,"replace_snapshot":ticket}))
            .await,
    );
    assert_eq!(released["total_count"], 0);
    assert!(
        !f.service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&ticket)
    );
    let before = f.count();
    for _ in 0..5 {
        view(
            f.list(&reader, json!({"limit":64,"replace_snapshot":ticket}))
                .await,
        );
    }
    assert_eq!(
        f.count(),
        before,
        "Exact unretained retirement retries cannot accumulate catalog leases"
    );
    view(
        f.list(
            &browser,
            json!({"limit":64,"replace_snapshot":browser_ticket}),
        )
        .await,
    );
    let reader = f.profile("navigation", &projects).await;
    let mut previous = None;
    for project in &projects {
        view(
            f.list(&reader, json!({"limit":64,"replace_snapshot":previous}))
                .await,
        );
        if let Some(id) = &previous {
            assert!(
                !f.service
                    .snapshots
                    .lock()
                    .unwrap()
                    .projects
                    .contains_key(id)
            );
        }
        previous = Some(f.ticket(&reader, project, None).await);
        assert_eq!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .principal_count(&reader.principal),
            1
        );
    }
    view(
        f.list(&reader, json!({"limit":64,"replace_snapshot":previous}))
            .await,
    );

    // A real retained asset inventory must never be retired by a catalog request.
    for index in 0..2 {
        let asset = super::tests::asset(&format!("catalog-owned-{index}"), &owner.principal);
        f.service
            .store
            .put(&format!("asset/{}", asset.id), &asset)
            .unwrap();
    }
    let assets = view(
        f.service
            .dispatch(
                owner.clone(),
                Request::query("assets.list", json!({"limit":1})),
            )
            .await,
    );
    view(
        f.list(
            &owner,
            json!({"limit":64,"replace_snapshot":assets["snapshot_id"]}),
        )
        .await,
    );
    assert!(
        f.service
            .snapshots
            .lock()
            .unwrap()
            .entries
            .contains_key(assets["snapshot_id"].as_str().unwrap())
    );
    let catalog = view(f.list(&owner, json!({"limit":1})).await);
    view(
        f.list(
            &owner,
            json!({"limit":64,"replace_snapshot":catalog["snapshot_id"]}),
        )
        .await,
    );
    failed(
        f.list(&owner, json!({"limit":1,"cursor":catalog["next_cursor"]}))
            .await,
        "SNAPSHOT_EXPIRED",
    );
    // The legacy inventory admission remains early, even for an unretained page.
    for _ in 0..3 {
        view(
            f.service
                .dispatch(
                    owner.clone(),
                    Request::query("assets.list", json!({"limit":1})),
                )
                .await,
        );
    }
    failed(
        f.service
            .dispatch(
                owner.clone(),
                Request::query("assets.list", json!({"limit":64})),
            )
            .await,
        "RESOURCE_LIMIT",
    );
    view(f.list(&owner, json!({"limit":64})).await);
    failed(f.list(&owner, json!({"limit":1})).await, "RESOURCE_LIMIT");

    unrelated_inventories().await;
    quota_matrix().await;
}

async fn unrelated_inventories() {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    f.project(&owner, "Catalog navigation").await;
    // Storage-only summary fixtures. These do not execute or qualify media jobs.
    for index in 0..2 {
        let id = format!("inventory-{index}");
        let asset = super::tests::asset(&id, &owner.principal);
        f.service.store.put(&format!("asset/{id}"), &asset).unwrap();
        let plan = Plan {
            execution_limits: None,
            file_times: None,
            schema_version: 1,
            required_rights: None,
            creation_audit: None,
            id: id.clone(),
            creator: owner.principal.clone(),
            hash: "00".repeat(32),
            source: asset.clone(),
            output: format!("/private/{id}.mkv"),
            ffmpeg: asset.identity.clone(),
            ffprobe: asset.identity.clone(),
            resolution: nw_domain::mux::CleanResolution {
                action: nw_domain::mux::CleanAction::CopyUnchanged,
                streams: Vec::new(),
                outcomes: Vec::new(),
                reasons: Vec::new(),
                warnings: Vec::new(),
                preservation: Default::default(),
            },
            argv_template: Vec::new(),
            created_at: "0".into(),
            expires_at: "0".into(),
            verification_scope: "Summary fixture only".into(),
        };
        let job = Job {
            remux_progress: None,
            progress: None,
            file_times: None,
            id: id.clone(),
            creator: owner.principal.clone(),
            plan,
            state: JobState::Succeeded,
            prepared_argv: None,
            prepared_stages: Vec::new(),
            owned_staging: Some(nw_domain::records::StagingIntent {
                job_id: id.clone(),
                directory: "/private/staging".into(),
                payload: "/private/staging/payload".into(),
            }),
            artifact: Some(asset.identity.clone()),
            verification: Some("Summary fixture only".into()),
            error: None,
        };
        f.service.store.put(&format!("job/{id}"), &job).unwrap();
        let publication = Publication {
            read_purpose_version: 1,
            job_id: id.clone(),
            proof: "fixture".into(),
            output: job.plan.output.clone(),
            identity: asset.identity,
            verified: "Summary fixture only".into(),
            published: true,
        };
        f.service
            .store
            .put(&format!("publication/{id}"), &publication)
            .unwrap();
        let batch = nw_domain::batches::Batch {
            schema_version: 1,
            id: id.clone(),
            creator: owner.principal.clone(),
            plan_id: id.clone(),
            plan_hash: "00".repeat(32),
            members: vec![nw_domain::batches::BatchMember {
                index: 0,
                job_id: Some(id.clone()),
            }],
            accepted_at: "0".into(),
        };
        f.service.store.put(&format!("batch/{id}"), &batch).unwrap();
    }
    let mut inventories = Vec::new();
    for command in ["assets.list", "jobs.list", "artifacts.list", "batches.list"] {
        let page = view(
            f.service
                .dispatch(owner.clone(), Request::query(command, json!({"limit":1})))
                .await,
        );
        assert_eq!(page["total_count"], 2);
        assert!(page["next_cursor"].is_string());
        inventories.push((command, page));
    }
    assert_eq!(f.count(), 4);
    for (command, page) in inventories {
        view(
            f.list(
                &owner,
                json!({"limit":64,"replace_snapshot":page["snapshot_id"]}),
            )
            .await,
        );
        assert!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .entries
                .contains_key(page["snapshot_id"].as_str().unwrap())
        );
        view(
            f.service
                .dispatch(
                    owner.clone(),
                    Request::query(command, json!({"limit":1,"cursor":page["next_cursor"]})),
                )
                .await,
        );
        assert_eq!(f.count(), 4);
    }
}

async fn quota_matrix() {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let projects = [
        f.project(&owner, "Quota one").await,
        f.project(&owner, "Quota two").await,
    ];
    let mut contexts = Vec::new();
    let mut tickets = Vec::new();
    let mut cursors = Vec::new();
    for index in 0..8 {
        let context = f.profile(&format!("quota-{index}"), &projects).await;
        let ticket = f.ticket(&context, &projects[0], None).await;
        for _ in 0..3 {
            let page = view(f.list(&context, json!({"limit":1})).await);
            assert!(page["next_cursor"].is_string());
            cursors.push((context.clone(), page["next_cursor"].clone()));
        }
        assert_eq!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .principal_count(&context.principal),
            4
        );
        view(f.list(&context, json!({"limit":64})).await);
        failed(
            f.list(&context, json!({"limit":1,"replace_snapshot":"unknown"}))
                .await,
            "RESOURCE_LIMIT",
        );
        tickets.push(ticket);
        contexts.push(context);
    }
    assert_eq!(f.count(), 32);
    assert!(f.service.snapshots.lock().unwrap().bytes() + MAX_PAGE_BYTES < MAX_GLOBAL_BYTES);
    let empty = f.profile("quota-empty", &[]).await;
    view(f.list(&empty, json!({"limit":1})).await);
    view(f.list(&owner, json!({"limit":64})).await);
    failed(f.list(&owner, json!({"limit":1})).await, "RESOURCE_LIMIT");
    // Unknown and foreign handles have the same ordinary admission outcome.
    for replacement in ["unknown", tickets[1].as_str()] {
        failed(
            f.list(
                &contexts[0],
                json!({"limit":1,"replace_snapshot":replacement}),
            )
            .await,
            "RESOURCE_LIMIT",
        );
        assert_eq!(f.count(), 32);
    }
    let (context, cursor) = &cursors[0];
    view(f.list(context, json!({"limit":1,"cursor":cursor})).await);
    assert_eq!(f.count(), 32, "Continuation consumes no new slot");
    let replacement = view(
        f.list(
            &contexts[0],
            json!({"limit":1,"replace_snapshot":tickets[0]}),
        )
        .await,
    );
    assert_eq!(f.count(), 32);
    assert!(
        !f.service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&tickets[0])
    );
    assert!(
        f.service
            .snapshots
            .lock()
            .unwrap()
            .entries
            .contains_key(replacement["snapshot_id"].as_str().unwrap())
    );
    // Retained paged queries are bounded queries, not durable receipt replays.
    failed(
        f.list(
            &contexts[0],
            json!({"limit":1,"replace_snapshot":tickets[0]}),
        )
        .await,
        "RESOURCE_LIMIT",
    );
    view(
        f.list(
            &contexts[0],
            json!({"limit":64,"replace_snapshot":replacement["snapshot_id"]}),
        )
        .await,
    );
    assert_eq!(f.count(), 31);
    let owner_page = view(f.list(&owner, json!({"limit":1})).await);
    assert!(owner_page["next_cursor"].is_string());
    assert_eq!(f.count(), 32);
    // Revoking Read leaves four genuine retained tickets until retirement/expiry;
    // an empty unretained projection must still pass the principal count ceiling.
    f.update(&contexts[1], |input| input["object_grants"] = json!([]))
        .await;
    assert_eq!(
        view(f.list(&contexts[1], json!({"limit":1})).await)["total_count"],
        0
    );
    assert_eq!(f.count(), 32);
    // Accounting fault fixture on a real retained entry: count exemptions never
    // exempt single/empty pages from the independent shared byte ceiling.
    let id = owner_page["snapshot_id"].as_str().unwrap();
    let original_bytes = {
        let mut cache = f.service.snapshots.lock().unwrap();
        let entry = cache.entries.get_mut(id).unwrap();
        let bytes = entry.bytes;
        entry.bytes = MAX_GLOBAL_BYTES;
        bytes
    };
    failed(f.list(&owner, json!({"limit":64})).await, "RESOURCE_LIMIT");
    failed(f.list(&empty, json!({"limit":1})).await, "RESOURCE_LIMIT");
    f.service
        .snapshots
        .lock()
        .unwrap()
        .entries
        .get_mut(id)
        .unwrap()
        .bytes = original_bytes;
    assert_eq!(f.count(), 32);
}

fn pause(
    service: &Service,
    phase: CatalogPhase,
) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<bool>) {
    let (started, ready) = std::sync::mpsc::channel();
    let (resume, waiting) = std::sync::mpsc::channel();
    *service.catalog_hook.lock().unwrap() = Some(CatalogHook {
        phase,
        started,
        waiting,
    });
    (ready, resume)
}
async fn reached(ready: std::sync::mpsc::Receiver<()>) {
    tokio::task::spawn_blocking(move || {
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("Catalog phase not reached")
    })
    .await
    .unwrap();
}
async fn finish(task: tokio::task::JoinHandle<Response>) -> Response {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("Catalog did not resume")
        .unwrap()
}

pub(super) async fn cursor_and_publication() {
    // The unchanged reaper may block on the cache mutex held by a phase seam.
    // Keep another worker available for real policy updates; do not change the
    // original test attribute or production reaper to accommodate the fixture.
    tokio::task::spawn_blocking(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(cursor_and_publication_cases());
    })
    .await
    .unwrap();
}

async fn cursor_and_publication_cases() {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let projects = [
        f.project(&owner, "Pinned one").await,
        f.project(&owner, "Pinned two").await,
    ];
    let reader = f.profile("cursor", &projects).await;
    let other = f.profile("cursor-other", &projects).await;
    let first = view(f.list(&reader, json!({"limit":1})).await);
    let cursor = first["next_cursor"].as_str().unwrap();
    for (context, params) in [
        (&other, json!({"limit":1,"cursor":cursor})),
        (&owner, json!({"limit":1,"cursor":cursor})),
        (&reader, json!({"limit":2,"cursor":cursor})),
        (&reader, json!({"limit":1,"cursor":format!("{cursor}x")})),
    ] {
        failed(f.list(context, params).await, "SNAPSHOT_EXPIRED");
    }
    for command in ["assets.list", "jobs.list", "artifacts.list", "batches.list"] {
        failed(
            f.service
                .dispatch(
                    reader.clone(),
                    Request::query(command, json!({"limit":1,"cursor":cursor})),
                )
                .await,
            "SNAPSHOT_EXPIRED",
        );
    }
    for params in [
        json!({"limit":0}),
        json!({"limit":513}),
        json!({"limit":1,"cursor":cursor,"replace_snapshot":first["snapshot_id"]}),
        json!({"cursor":"x".repeat(257)}),
        json!({"replace_snapshot":"x".repeat(129)}),
    ] {
        failed(f.list(&reader, params).await, "VALIDATION_ERROR");
    }
    let later = f.project(&owner, "After capture").await;
    let remaining = projects
        .iter()
        .find(|id| first["items"][0]["id"] != id.as_str())
        .unwrap();
    let changed = f.write_project(&owner, "sequences.create", json!({"name":"Pinned header change",
        "settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
        "tracks":[{"kind":"video","name":"Video"}]}), Some(ProjectRef { id:remaining.clone(), expected_revision:Some("0".into()) })).await;
    assert_eq!(changed["revision"], "1");
    let continued = view(f.list(&reader, json!({"limit":1,"cursor":cursor})).await);
    assert_eq!(continued["snapshot_id"], first["snapshot_id"]);
    assert_eq!(continued["total_count"], 2);
    assert_eq!(continued["items"][0]["id"], remaining.as_str());
    assert_eq!(continued["items"][0]["revision"], "0");
    assert!(!continued.to_string().contains(&later));
    let latest = view(f.list(&reader, json!({"limit":64})).await);
    assert_eq!(
        latest["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == remaining.as_str())
            .unwrap()["revision"],
        "1"
    );
    f.update(&reader, |input| {
        input["allowed_command_ids"]
            .as_array_mut()
            .unwrap()
            .push(json!("commands.list"))
    })
    .await;
    failed(
        f.list(&reader, json!({"limit":1,"cursor":cursor})).await,
        "SNAPSHOT_EXPIRED",
    );
    view(
        f.list(
            &reader,
            json!({"limit":64,"replace_snapshot":first["snapshot_id"]}),
        )
        .await,
    );
    let fresh = view(f.list(&reader, json!({"limit":1})).await);
    f.service
        .snapshots
        .lock()
        .unwrap()
        .entries
        .get_mut(fresh["snapshot_id"].as_str().unwrap())
        .unwrap()
        .created = Instant::now() - Duration::from_secs(31);
    failed(
        f.list(&reader, json!({"limit":1,"cursor":fresh["next_cursor"]}))
            .await,
        "SNAPSHOT_EXPIRED",
    );

    // Post-dispatch denial must precede retirement, even with a valid old ticket.
    for discovery in [false, true] {
        let reader = f.profile("cursor", &projects).await;
        let ticket = f.ticket(&reader, &projects[0], None).await;
        let (ready, resume) = pause(&f.service, CatalogPhase::Entry);
        let worker = f.service.clone();
        let context = reader.clone();
        let token = ticket.clone();
        let task = tokio::spawn(async move {
            worker
                .dispatch(
                    context,
                    Request::query(
                        "projects.list",
                        json!({"limit":64,"replace_snapshot":token}),
                    ),
                )
                .await
        });
        reached(ready).await;
        tokio::time::timeout(
            Duration::from_secs(5),
            f.update(&reader, |input| {
                input["allowed_command_ids"] = json!(["access.replay_scope.create"]);
                if discovery {
                    input["mode"] = json!("discovery");
                    input["object_grants"] = json!([]);
                    input["owned_object_access"] = json!(false);
                    input["read_roots"] = json!([]);
                }
            }),
        )
        .await
        .expect("Policy update blocked by query");
        resume.send(false).unwrap();
        failed(finish(task).await, "PERMISSION_DENIED");
        assert!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .projects
                .contains_key(&ticket)
        );
        failed(
            f.list(&reader, json!({"limit":64,"replace_snapshot":ticket}))
                .await,
            "PERMISSION_DENIED",
        );
        assert!(
            f.service
                .snapshots
                .lock()
                .unwrap()
                .projects
                .contains_key(&ticket)
        );
        let reader = f.profile("cursor", &projects).await;
        view(
            f.list(&reader, json!({"limit":64,"replace_snapshot":ticket}))
                .await,
        );
    }
    let reader = f.profile("cursor", &projects).await;
    let ticket = f.ticket(&reader, &projects[0], None).await;
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let (ready, resume) = pause(&f.service, CatalogPhase::Entry);
    let worker = f.service.clone();
    let context = reader.clone();
    let replacement = ticket.clone();
    let task = tokio::spawn(async move {
        worker
            .dispatch_cancel(
                context,
                Request::query(
                    "projects.list",
                    json!({"limit":64,"replace_snapshot":replacement}),
                ),
                token,
            )
            .await
    });
    reached(ready).await;
    cancellation.cancel();
    resume.send(false).unwrap();
    failed(finish(task).await, "CANCELLED");
    assert!(
        f.service
            .snapshots
            .lock()
            .unwrap()
            .projects
            .contains_key(&ticket)
    );
    view(
        f.list(&reader, json!({"limit":64,"replace_snapshot":ticket}))
            .await,
    );
    publication_races(&f, &projects).await;
}

async fn publication_races(f: &Fixture, projects: &[String]) {
    // Empty entry/final, after the last rendered row, and cursor publication.
    for (phase, empty, continuation) in [
        (CatalogPhase::Entry, true, false),
        (CatalogPhase::Fresh, true, false),
        (CatalogPhase::Fresh, false, false),
        (CatalogPhase::Cursor, false, true),
    ] {
        for action in ["cancel", "expire", "revoke", "disable", "revision", "edit"] {
            if phase == CatalogPhase::Entry && action != "cancel" {
                continue;
            }
            if action == "edit" && empty {
                continue;
            }
            let reader = f
                .profile("publication", if empty { &[] } else { projects })
                .await;
            if action == "edit" {
                f.update(&reader, |input| {
                    input["object_grants"] = json!(
                        projects
                            .iter()
                            .map(|id| {
                                json!({"kind":"project","id":id,"actions":["read","edit"]})
                            })
                            .collect::<Vec<_>>()
                    );
                })
                .await;
            }
            let first = if continuation {
                Some(view(f.list(&reader, json!({"limit":1})).await))
            } else {
                None
            };
            let before = f.count();
            let params = first.as_ref().map_or(
                json!({"limit":1}),
                |page| json!({"limit":1,"cursor":page["next_cursor"]}),
            );
            let cancellation = CancellationToken::new();
            let token = cancellation.clone();
            let (ready, resume) = pause(&f.service, phase);
            let worker = f.service.clone();
            let context = reader.clone();
            let task = tokio::spawn(async move {
                worker
                    .dispatch_cancel(context, Request::query("projects.list", params), token)
                    .await
            });
            reached(ready).await;
            match action {
                "cancel" => cancellation.cancel(),
                "revoke" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| {
                        input["allowed_command_ids"] = json!(["access.replay_scope.create"])
                    }),
                )
                .await
                .expect("Revocation blocked by cache mutex"),
                "disable" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| input["enabled"] = json!(false)),
                )
                .await
                .expect("Profile revocation blocked by cache mutex"),
                "revision" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| input["object_grants"] = json!([])),
                )
                .await
                .expect("Policy revision blocked by cache mutex"),
                "edit" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| {
                        input["object_grants"] = json!(
                            projects
                                .iter()
                                .map(|id| { json!({"kind":"project","id":id,"actions":["read"]}) })
                                .collect::<Vec<_>>()
                        );
                    }),
                )
                .await
                .expect("Edit revocation blocked by cache mutex"),
                "expire" => {}
                _ => unreachable!(),
            }
            resume.send(action == "expire").unwrap();
            let expected = match action {
                "cancel" => "CANCELLED",
                "revoke" | "disable" => "PERMISSION_DENIED",
                _ => "SNAPSHOT_EXPIRED",
            };
            failed(finish(task).await, expected);
            assert_eq!(
                f.count(),
                before,
                "Final rejection must not retain a new catalog"
            );
            let reader = f.profile("publication", projects).await;
            if let Some(first) = first {
                view(
                    f.list(
                        &reader,
                        json!({"limit":64,"replace_snapshot":first["snapshot_id"]}),
                    )
                    .await,
                );
            }
        }
    }
}

pub(super) async fn rights_and_command_metadata() {
    let mut conformance = crate::project_schema_batch(
        7,
        &[
            1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 50, 51, 52, 53,
            54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
        ],
    );
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let name = "R2 command metadata";
    let project = f.project(&owner, name).await;
    let reader = f
        .profile("r2-catalog-reader", std::slice::from_ref(&project))
        .await;
    let original = metadata_profile(&f, &reader, &project, json!(["read"]));
    assert!(!original.allows_command("clips.link"));
    assert!(!original.allows_command("clips.unlink"));
    f.update(&reader, |input| {
        let commands = input["allowed_command_ids"].as_array_mut().unwrap();
        commands.push(json!("clips.link"));
        commands.push(json!("clips.unlink"));
    })
    .await;
    let read_only = metadata_profile(&f, &reader, &project, json!(["read"]));
    let mut allowed = original.allowed_command_ids.clone();
    allowed.insert("clips.link".into());
    allowed.insert("clips.unlink".into());
    assert!(read_only.allowed_command_ids == allowed);
    assert!(read_only.revision > original.revision);
    let root = f.root.path().to_str().unwrap();
    let private = [
        owner.principal.as_str(),
        reader.principal.as_str(),
        reader.profile.as_deref().unwrap(),
        root,
    ];
    let first = metadata_catalog(
        conformance.capture(
            1,
            "projects.list",
            f.list(&reader, json!({"limit":64})).await,
        ),
        &project,
        name,
        &private,
    );
    // First intended behavioral RED, after public setup, legacy facts and privacy.
    assert_eq!(
        first["authorization"]["schema_version"].as_u64(),
        Some(1),
        "R2_AUTHORIZATION_SCHEMA_MISSING"
    );

    // UNEXECUTED until the marker boundary above passes in hosted execution.
    assert_command_metadata(&first["authorization"], &read_only, false, false);
    let first_detail = conformance.capture(
        4,
        "projects.inspect",
        metadata_detail(&f, &reader, &project).await,
    );
    assert_command_metadata(
        &view(first_detail)["authorization"],
        &read_only,
        false,
        false,
    );
    f.update(&reader, |input| {
        input["object_grants"] = json!([{"kind":"project","id":project,"actions":["read","edit"]}]);
    })
    .await;
    let editable = metadata_profile(&f, &reader, &project, json!(["read", "edit"]));
    assert!(editable.allowed_command_ids == read_only.allowed_command_ids);
    assert!(editable.revision > read_only.revision);
    let second = metadata_catalog(
        conformance.capture(
            2,
            "projects.list",
            f.list(&reader, json!({"limit":64})).await,
        ),
        &project,
        name,
        &private,
    );
    assert_command_metadata(&second["authorization"], &editable, true, false);
    let second_detail = conformance.capture(
        5,
        "projects.inspect",
        metadata_detail(&f, &reader, &project).await,
    );
    assert_command_metadata(
        &view(second_detail)["authorization"],
        &editable,
        true,
        false,
    );

    // UNEXECUTED until both preceding metadata stages pass.
    f.update(&reader, |input| {
        input["allowed_command_ids"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id != "clips.link");
    })
    .await;
    let unlink_only = metadata_profile(&f, &reader, &project, json!(["read", "edit"]));
    allowed.remove("clips.link");
    assert!(unlink_only.allowed_command_ids == allowed);
    assert!(unlink_only.revision > editable.revision);
    let third = metadata_catalog(
        conformance.capture(
            3,
            "projects.list",
            f.list(&reader, json!({"limit":64})).await,
        ),
        &project,
        name,
        &private,
    );
    assert_command_metadata(&third["authorization"], &unlink_only, true, true);
    let third_detail = conformance.capture(
        6,
        "projects.inspect",
        metadata_detail(&f, &reader, &project).await,
    );
    assert_command_metadata(
        &view(third_detail)["authorization"],
        &unlink_only,
        true,
        true,
    );
    let owner_detail = conformance.capture(
        7,
        "projects.inspect",
        metadata_detail(&f, &owner, &project).await,
    );
    assert_eq!(view(owner_detail)["authorization"]["policy_revision"], "0");

    use nw_domain::projects::views::ProjectSummary;
    let known: ProjectSummary = serde_json::from_value(first.clone()).expect("R2_READER_DECODE");
    assert!(known.known_authorization().is_some());
    for newer in [false, true] {
        let mut copy = first.clone();
        if newer {
            copy["authorization"]["schema_version"] = json!(2);
        } else {
            copy.as_object_mut().unwrap().remove("authorization");
        }
        copy["read_only"] = json!(false);
        let historical: ProjectSummary =
            serde_json::from_value(copy).expect("R2_HISTORICAL_READER");
        assert!(
            historical.known_authorization().is_none(),
            "R2_UNKNOWN_MARKER_GRANTED_RIGHTS"
        );
    }
    conformance.reject_copy(50, 1, |v| {
        v["items"][0]
            .as_object_mut()
            .unwrap()
            .remove("authorization");
    });
    conformance.reject_copy(51, 1, |v| {
        v["items"][0]["authorization"]["schema_version"] = json!(2);
    });
    conformance.reject_copy(52, 1, |v| {
        let action = &mut v["items"][0]["authorization"]["actions"][0];
        action["state"] = json!("unavailable");
        action["reasons"] = json!(["unknown_reason"]);
    });
    conformance.reject_copy(53, 1, |v| {
        v["items"][0]["authorization"]["actions"]
            .as_array_mut()
            .unwrap()
            .swap(0, 2);
    });
    conformance.reject_copy(54, 1, |v| {
        v["items"][0]["authorization"]["actions"][0]["unchecked_gates"] = json!([]);
    });
    conformance.reject_copy(55, 1, |v| {
        v["items"][0]["authorization"]["actions"][0]["state"] = json!("ready");
    });
    conformance.reject_copy(56, 2, |v| {
        v["items"][0]["authorization"]["current_edit"]["state"] = json!("unavailable");
    });
    conformance.reject_copy(57, 1, |v| {
        v["items"][0]["authorization"]["extra_field"] = json!(true);
    });
    conformance.reject_copy(58, 1, |v| {
        v["items"][0]["authorization"]["policy_revision"] = json!(1);
    });
    conformance.reject_copy(59, 1, |v| {
        v["items"][0]["authorization"]["observed_project_revision"] = json!("01");
    });
    conformance.reject_copy(60, 1, |v| {
        v["items"][0]["authorization"]["current_edit"]["reasons"] =
            json!(["project_read_not_granted"]);
    });
    conformance.reject_copy(61, 1, |v| {
        v["items"][0]["authorization"]["actions"]
            .as_array_mut()
            .unwrap()
            .remove(0);
    });
    conformance.reject_copy(62, 1, |v| {
        let action = &mut v["items"][0]["authorization"]["actions"][0];
        action["state"] = json!("unavailable");
        action["reasons"] = json!(["command_not_allowed", "command_not_allowed"]);
    });
    conformance.reject_copy(63, 1, |v| {
        v["items"][0]["authorization"]["current_read"] = json!(true);
    });
    conformance.reject_copy(64, 7, |v| {
        v.as_object_mut().unwrap().remove("authorization");
    });
    metadata_runtime_discovery(&f, &reader, &project).await;
    metadata_size_bounds(&mut conformance).await;
    metadata_writer_observation().await;
    conformance.finish().await;
    // The existing reaper can wait on the snapshot mutex while a publication is paused.
    tokio::task::spawn_blocking(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(metadata_publication_races());
    })
    .await
    .unwrap();
}

async fn metadata_detail(f: &Fixture, context: &Context, project: &str) -> Response {
    let mut request = Request::query("projects.inspect", json!({"projection":"summary"}));
    request.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: None,
    });
    f.service.dispatch(context.clone(), request).await
}

async fn metadata_runtime_discovery(f: &Fixture, reader: &Context, project: &str) {
    let owner = f.service.owner();
    let fingerprint = commands::registry_fingerprint();
    f.update(reader, |input| {
        let allowed = input["allowed_command_ids"].as_array_mut().unwrap();
        for command in ["commands.list", "commands.schema"] {
            if !allowed.iter().any(|value| value == command) {
                allowed.push(json!(command));
            }
        }
    })
    .await;
    let profile = metadata_profile(f, reader, project, json!(["read", "edit"]));
    let expected_revision = profile.revision.to_string();
    let pure = crate::CommandService
        .dispatch(
            crate::Principal::DiscoveryAgent,
            Request::query("commands.list", json!({})),
        )
        .result
        .unwrap();
    assert!(
        pure.get("authorization").is_none(),
        "R2_PURE_DISCOVERY_HAS_LIVE_FACTS"
    );
    let mut results = Vec::new();
    for (kind, context) in [("owner", owner.clone()), ("delegated", reader.clone())] {
        let response = f
            .service
            .dispatch(context, Request::query("commands.list", json!({})))
            .await;
        let bytes = serde_json::to_vec(&response).unwrap().len();
        assert!(
            response.ok && bytes <= commands::MAX_RESULT_BYTES,
            "R2_DISCOVERY_RESPONSE_BOUND"
        );
        eprintln!("R2_SIZE kind=discovery actor={kind} response_bytes={bytes}");
        let listing = view(response);
        assert!(
            listing["commands"] == pure["commands"],
            "R2_DISCOVERY_STATIC_ROWS_CHANGED"
        );
        assert_eq!(listing["authorization"].as_object().unwrap().len(), 3);
        assert_eq!(listing["authorization"]["schema_version"], 1);
        assert!(listing["authorization"].get("current_read").is_none());
        assert!(listing["authorization"].get("current_edit").is_none());
        let visible = listing.to_string();
        for private in [
            reader.principal.as_str(),
            reader.profile.as_deref().unwrap(),
            owner.principal.as_str(),
            f.root.path().to_str().unwrap(),
        ] {
            assert!(!visible.contains(private), "R2_DISCOVERY_PRIVATE_FACT");
        }
        results.push(listing);
    }
    assert_eq!(results[0]["authorization"]["policy_revision"], "0");
    assert!(
        results[1]["authorization"]["policy_revision"].as_str() == Some(expected_revision.as_str())
    );
    let rows = results[0]["commands"].as_array().unwrap();
    let expected: std::collections::BTreeSet<_> =
        crate::schema_validation::COMMANDS.into_iter().collect();
    let family: Vec<_> = rows
        .iter()
        // The version1 runtime observation retains its closed13-command family.
        // Additional tools publish static requirements without widening this codec.
        .filter(|row| row["id"].as_str().is_some_and(|id| expected.contains(id)))
        .collect();
    assert_eq!(family.len(), 13);
    for result in &results {
        assert_eq!(
            result["authorization"]["actions"].as_array().unwrap().len(),
            13
        );
    }
    assert_eq!(
        family
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    for (index, row) in family.iter().enumerate() {
        let command = row["id"].as_str().unwrap();
        let owner_action = &results[0]["authorization"]["actions"][index];
        let action = &results[1]["authorization"]["actions"][index];
        assert_eq!(owner_action["command_id"], command);
        assert_eq!(owner_action["state"], "requires_request_validation");
        assert_eq!(owner_action["reasons"], json!([]));
        assert_eq!(action["command_id"], command);
        assert!(action["unchecked_gates"] == row["authorization_requirements"]["request_gates"]);
        if profile.allows_command(command) {
            assert_eq!(action["state"], "requires_request_validation");
            assert_eq!(action["reasons"], json!([]));
        } else {
            assert_eq!(action["state"], "unavailable");
            assert_eq!(action["reasons"], json!(["command_not_allowed"]));
        }
        if row["authorization_requirements"]["project_action"].is_string() {
            let gates = action["unchecked_gates"].as_array().unwrap();
            assert!(
                gates.contains(&json!("project_context"))
                    && gates.contains(&json!("project_rights"))
            );
        }
    }
    for id in ["projects.create", "projects.list", "clips.link"] {
        let request = Request::query("commands.schema", json!({"id":id}));
        let static_result = crate::CommandService
            .dispatch(crate::Principal::DiscoveryAgent, request.clone())
            .result
            .unwrap();
        let response = f.service.dispatch(reader.clone(), request).await;
        assert!(
            response.ok
                && serde_json::to_vec(&response).unwrap().len() <= commands::MAX_RESULT_BYTES
        );
        assert!(
            response.result.as_ref() == Some(&static_result),
            "R2_SCHEMA_CARRIES_LIVE_AUTHORITY"
        );
        assert!(static_result.get("authorization").is_none());
    }
    f.update(reader, |input| {
        input["allowed_command_ids"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id != "commands.list")
    })
    .await;
    let denied = f
        .service
        .dispatch(reader.clone(), Request::query("commands.list", json!({})))
        .await;
    assert!(denied.result.is_none());
    failed(denied, "PERMISSION_DENIED");
    f.update(reader, |input| input["enabled"] = json!(false))
        .await;
    failed(
        f.service
            .dispatch(
                reader.clone(),
                Request::query("commands.schema", json!({"id":"clips.link"})),
            )
            .await,
        "PERMISSION_DENIED",
    );
    assert_eq!(
        commands::registry_fingerprint(),
        fingerprint,
        "R2_LIVE_POLICY_CHANGED_STATIC_FINGERPRINT"
    );
}

async fn metadata_size_bounds(conformance: &mut crate::schema_validation::Batch) {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let limits = [1, 64, 128, 512];
    for (index, limit) in limits.into_iter().enumerate() {
        let response = f.list(&owner, json!({"limit":limit})).await;
        metadata_size(&response, 0, limit, 0);
        conformance.observe(10 + index as u16, "projects.list", &response);
    }
    let name = "é".repeat(64);
    assert_eq!(name.len(), 128);
    // Public profile grants must refer to real existing projects.
    let mut ids = Vec::new();
    for _ in 0..64 {
        ids.push(f.project(&owner, &name).await);
    }
    let single = f.profile("r2-size-single", &ids[..1]).await;
    let all = f.profile("r2-size-all", &ids).await;
    // Only maximal revision/unknown-header widths use storage-boundary fixtures.
    for reader in [&single, &all] {
        let key = format!("profile/{}", reader.profile.as_deref().unwrap());
        let mut maximal: AccessProfile = f.service.store.get(&key).unwrap().unwrap();
        maximal.revision = u64::MAX;
        f.service.store.put(&key, &maximal).unwrap();
    }
    for id in &ids {
        f.service
            .store
            .put(
                &format!("project/{id}"),
                &header(id, &owner.principal, &name),
            )
            .unwrap();
    }
    for (stage, count, reader) in [(0u16, 1usize, &single), (1, 64, &all)] {
        let mut replacement = Value::Null;
        for (index, limit) in limits.into_iter().enumerate() {
            let response = f
                .list(
                    reader,
                    json!({"limit":limit,"replace_snapshot":replacement}),
                )
                .await;
            metadata_size(&response, count, limit, limit.min(count));
            let page = response.result.as_ref().unwrap();
            assert_eq!(page["total_count"], count);
            for row in page["items"].as_array().unwrap() {
                assert_eq!(row["name"], name);
                let encoded = serde_json::to_vec(row).unwrap();
                let decoded: nw_domain::projects::views::ProjectSummary =
                    serde_json::from_slice(&encoded).unwrap();
                assert_eq!(decoded.name, name);
                let auth = decoded
                    .known_authorization()
                    .expect("R2_BOUNDARY_METADATA_UNKNOWN");
                let maximum = u64::MAX.to_string();
                assert!(auth.policy_revision.as_str() == maximum.as_str());
                assert!(auth.observed_project_revision.as_str() == maximum.as_str());
                assert_eq!(auth.current_read.state, nw_domain::projects::authorization::AvailabilityState::RequiresRequestValidation);
                assert_eq!(
                    auth.current_edit.reasons,
                    [nw_domain::projects::authorization::BlockReason::ProjectEditNotGranted]
                );
                let place = metadata_action(&row["authorization"], "clips.place");
                assert_eq!(
                    place["reasons"],
                    json!([
                        "command_not_allowed",
                        "project_edit_not_granted",
                        "unsupported_project_schema"
                    ])
                );
                assert_eq!(place["unchecked_gates"].as_array().unwrap().len(), 8);
            }
            replacement = page["snapshot_id"].clone();
            conformance.observe(14 + stage * 4 + index as u16, "projects.list", &response);
        }
        assert_eq!(f.count(), 0, "R2_BOUNDARY_SINGLE_PAGE_RETAINED");
    }
    let extra = "00000000-0000-4000-8000-000000000064";
    f.service
        .store
        .put(
            &format!("project/{extra}"),
            &header(extra, &owner.principal, &name),
        )
        .unwrap();
    let overflow = f.list(&all, json!({"limit":512})).await;
    assert!(overflow.result.is_none());
    assert!(serde_json::to_vec(&overflow).unwrap().len() <= commands::MAX_RESULT_BYTES);
    failed(overflow, "RESOURCE_LIMIT");
    assert_eq!(f.count(), 0);
}

fn metadata_size(response: &Response, total: usize, limit: usize, delivered: usize) {
    assert!(
        response.ok && response.error.is_none() && response.receipt_id.is_none(),
        "R2_SIZE_QUERY_FAILED"
    );
    let page = response.result.as_ref().expect("R2_SIZE_RESULT_MISSING");
    assert_eq!(page["items"].as_array().unwrap().len(), delivered);
    let result_bytes = serde_json::to_vec(page).unwrap().len();
    let response_bytes = serde_json::to_vec(response).unwrap().len();
    assert!(
        result_bytes <= MAX_PAGE_BYTES && response_bytes <= commands::MAX_RESULT_BYTES,
        "R2_SIZE_EXCEEDED"
    );
    if total <= limit {
        assert!(
            page.get("next_cursor") == Some(&Value::Null),
            "R2_SIZE_SILENT_PAGINATION"
        );
    }
    eprintln!(
        "R2_SIZE kind=catalog total={total} limit={limit} delivered={delivered} result_bytes={result_bytes} response_bytes={response_bytes}"
    );
}

fn metadata_action<'a>(authorization: &'a Value, command: &str) -> &'a Value {
    authorization["actions"]
        .as_array()
        .expect("R2_ACTIONS_MISSING")
        .iter()
        .find(|action| action["command_id"] == command)
        .expect("R2_ACTION_MISSING")
}

async fn metadata_writer_observation() {
    let f = Fixture::new().await;
    let owner = f.service.owner();
    let project = f.project(&owner, "Writer observation").await;
    let summary = view(metadata_detail(&f, &owner, &project).await);
    let bin = summary["root_bin_id"].as_str().unwrap().to_owned();
    let reader = f.profile("r2-writer", std::slice::from_ref(&project)).await;
    f.update(&reader, |input| {
        input["allowed_command_ids"]
            .as_array_mut()
            .unwrap()
            .extend([json!("clips.link"), json!("clips.unlink")]);
    })
    .await;
    f.service.project_writers.lock().unwrap().clear();
    let held = nw_platform::project_lease::ProjectWriterLease::acquire(
        &nw_domain::projects::ProjectId::parse(&project).unwrap(),
    )
    .unwrap();
    let read_only = view(metadata_detail(&f, &reader, &project).await);
    assert_eq!(read_only["writer_status"], "other_owner_read_only");
    assert_eq!(
        read_only["authorization"]["current_edit"],
        json!({"state":"unavailable","reasons":["project_edit_not_granted"]})
    );
    assert_eq!(
        metadata_action(&read_only["authorization"], "clips.link")["reasons"],
        json!(["project_edit_not_granted", "writer_observed_unavailable"])
    );
    f.update(
        &reader,
        |input| {
            input["object_grants"] =
                json!([{"kind":"project","id":project,"actions":["read","edit"]}])
        },
    )
    .await;
    let editable = view(metadata_detail(&f, &reader, &project).await);
    assert_eq!(
        editable["authorization"]["current_edit"],
        json!({"state":"requires_request_validation","reasons":[]})
    );
    assert_eq!(
        metadata_action(&editable["authorization"], "clips.link")["reasons"],
        json!(["writer_observed_unavailable"])
    );
    let mut request = Request::query(
        "projects.inspect",
        json!({"projection":"entity","entity_id":bin}),
    );
    request.project = Some(ProjectRef {
        id: project.clone(),
        expected_revision: None,
    });
    let entity = view(f.service.dispatch(reader.clone(), request).await);
    assert!(entity.get("writer_status").is_none());
    let unobserved = metadata_action(&entity["authorization"], "clips.link");
    assert_eq!(unobserved["state"], "requires_request_validation");
    assert_eq!(unobserved["reasons"], json!([]));
    assert!(
        unobserved["unchecked_gates"]
            .as_array()
            .unwrap()
            .contains(&json!("writer_lease"))
    );
    drop(held);
    let available = view(metadata_detail(&f, &reader, &project).await);
    assert_eq!(available["writer_status"], "available");
    assert_eq!(
        metadata_action(&available["authorization"], "clips.link")["state"],
        "requires_request_validation"
    );
    assert!(
        metadata_action(&available["authorization"], "clips.link")["unchecked_gates"]
            .as_array()
            .unwrap()
            .contains(&json!("writer_lease"))
    );
}

fn metadata_query(project: &str, params: Value) -> Request {
    let mut request = Request::query("projects.inspect", params);
    request.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: None,
    });
    request
}

async fn metadata_sequence(
    f: &Fixture,
    owner: &Context,
    project: &str,
    revision: &str,
    name: &str,
) {
    let result = f.write_project(owner, "sequences.create", json!({"name":name,
        "settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
        "tracks":[{"kind":"video","name":"Video"}]}),
        Some(ProjectRef { id:project.into(), expected_revision:Some(revision.into()) })).await;
    assert!(result["revision"].is_string());
}

fn metadata_busy(response: &Response) -> bool {
    !response.ok
        && response.result.is_none()
        && response.receipt_id.is_none()
        && response.error.as_ref().is_some_and(|error| {
            error.code == "RESOURCE_LIMIT"
                && error.message == "Snapshot service is busy; retry the query"
        })
}

// Only this concurrent metadata consumer retries the query's explicit mutex-busy
// refusal. Capacity, expiry, cancellation and policy failures are never retried.
// Eight attempts add at most seven10ms delays inside the unchanged race deadlines.
async fn metadata_available(
    service: &Arc<Service>,
    context: Context,
    request: Request,
    cancellation: CancellationToken,
) -> Response {
    assert_eq!(request.command.id, "projects.inspect");
    for attempt in 0..8 {
        let response = service
            .dispatch_cancel(context.clone(), request.clone(), cancellation.clone())
            .await;
        if !metadata_busy(&response) || cancellation.is_cancelled() || attempt == 7 {
            return response;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    unreachable!("The last bounded attempt returns its actual response")
}

async fn metadata_publication_races() {
    // Establish the precise retryable condition using the actual retained mutex;
    // a refused query must not allocate a ticket or mutate retained bindings.
    {
        let f = Fixture::new().await;
        let owner = f.service.owner();
        let project = f.project(&owner, "Snapshot busy consumer").await;
        let request = metadata_query(&project, json!({"projection":"summary"}));
        let response = {
            let held = f.service.snapshots.lock().unwrap();
            let before = held.count();
            let error = f
                .service
                .project_query(&owner, &request, &CancellationToken::new())
                .unwrap_err();
            let response = Response::error(&request.request_id, error);
            assert!(metadata_busy(&response), "{response:?}");
            assert_eq!(held.count(), before);
            response
        };
        view(metadata_available(&f.service, owner, request, CancellationToken::new()).await);
        let mut unrelated = response;
        unrelated.error.as_mut().unwrap().message = "Snapshot capacity exhausted".into();
        assert!(!metadata_busy(&unrelated));
        unrelated.error.as_mut().unwrap().code = "SNAPSHOT_EXPIRED".into();
        assert!(!metadata_busy(&unrelated));
    }
    for phase in [
        CatalogPhase::ProjectFresh,
        CatalogPhase::ProjectReused,
        CatalogPhase::ProjectCursor,
    ] {
        for change in ["cancel", "expire", "read", "edit", "command", "disable"] {
            let f = Fixture::new().await;
            let owner = f.service.owner();
            let project = f.project(&owner, "Coherent metadata").await;
            metadata_sequence(&f, &owner, &project, "0", "A").await;
            metadata_sequence(&f, &owner, &project, "1", "B").await;
            let bin = view(
                metadata_available(
                    &f.service,
                    owner.clone(),
                    metadata_query(&project, json!({"projection":"summary"})),
                    CancellationToken::new(),
                )
                .await,
            )["root_bin_id"]
                .clone();
            let reader = f
                .profile("r2-publication", std::slice::from_ref(&project))
                .await;
            f.update(
                &reader,
                |input| {
                    input["object_grants"] =
                        json!([{"kind":"project","id":project,"actions":["read","edit"]}])
                },
            )
            .await;
            let (ticket, params) = if phase == CatalogPhase::ProjectFresh {
                (None, json!({"projection":"summary"}))
            } else if phase == CatalogPhase::ProjectReused {
                let summary = view(
                    metadata_available(
                        &f.service,
                        reader.clone(),
                        metadata_query(
                            &project,
                            json!({"projection":"summary","replace_snapshot":null}),
                        ),
                        CancellationToken::new(),
                    )
                    .await,
                );
                let ticket = summary["snapshot_id"].as_str().unwrap().to_owned();
                assert!(
                    f.service
                        .snapshots
                        .lock()
                        .unwrap()
                        .projects
                        .contains_key(&ticket)
                );
                metadata_sequence(&f, &owner, &project, "2", "C").await;
                let pinned = view(
                    metadata_available(
                        &f.service,
                        reader.clone(),
                        metadata_query(
                            &project,
                            json!({"projection":"entity","entity_id":bin,"revision":"2"}),
                        ),
                        CancellationToken::new(),
                    )
                    .await,
                );
                assert_eq!(pinned["project_revision"], "2");
                assert_eq!(pinned["authorization"]["observed_project_revision"], "3");
                assert_eq!(pinned["entity"]["member_count"], 2);
                (
                    Some(ticket),
                    json!({"projection":"bin_page","bin_id":bin,"revision":"2","limit":64}),
                )
            } else {
                let first = view(
                    metadata_available(
                        &f.service,
                        reader.clone(),
                        metadata_query(
                            &project,
                            json!({"projection":"bin_page","bin_id":bin,"limit":1}),
                        ),
                        CancellationToken::new(),
                    )
                    .await,
                );
                assert!(first["next_cursor"].is_string());
                metadata_sequence(&f, &owner, &project, "2", "C").await;
                let params = json!({"projection":"bin_page","bin_id":bin,"limit":1,"cursor":first["next_cursor"]});
                let pinned = view(
                    metadata_available(
                        &f.service,
                        reader.clone(),
                        metadata_query(&project, params.clone()),
                        CancellationToken::new(),
                    )
                    .await,
                );
                assert_eq!(pinned["project_revision"], "2");
                assert_eq!(pinned["authorization"]["observed_project_revision"], "3");
                assert_eq!(pinned["total_count"], 2);
                (
                    Some(first["snapshot_id"].as_str().unwrap().to_owned()),
                    params,
                )
            };
            let before = f.count();
            let bindings = ticket
                .as_ref()
                .map(|id| f.service.snapshots.lock().unwrap().projects[id].binding_count());
            let cancellation = CancellationToken::new();
            let token = cancellation.clone();
            let (ready, resume) = pause(&f.service, phase);
            let service = f.service.clone();
            let context = reader.clone();
            let request = metadata_query(&project, params);
            let task =
                tokio::spawn(
                    async move { metadata_available(&service, context, request, token).await },
                );
            reached(ready).await;
            match change {
                "cancel" => cancellation.cancel(),
                "read" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| {
                        // Public policies require Read on every grant; revoke the whole grant.
                        input["object_grants"] = json!([])
                    }),
                )
                .await
                .expect("R2_READ_REVOCATION_BLOCKED"),
                "edit" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| {
                        input["object_grants"] =
                            json!([{"kind":"project","id":project,"actions":["read"]}])
                    }),
                )
                .await
                .expect("R2_EDIT_REVOCATION_BLOCKED"),
                "command" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| {
                        input["allowed_command_ids"]
                            .as_array_mut()
                            .unwrap()
                            .retain(|id| id != "projects.inspect")
                    }),
                )
                .await
                .expect("R2_COMMAND_REVOCATION_BLOCKED"),
                "disable" => tokio::time::timeout(
                    Duration::from_secs(5),
                    f.update(&reader, |input| input["enabled"] = json!(false)),
                )
                .await
                .expect("R2_POLICY_DISABLE_BLOCKED"),
                "expire" => {}
                _ => unreachable!(),
            }
            resume.send(change == "expire").unwrap();
            let response = finish(task).await;
            assert!(response.result.is_none() && response.receipt_id.is_none());
            failed(
                response,
                match change {
                    "cancel" => "CANCELLED",
                    "disable" => "PERMISSION_DENIED",
                    _ => "SNAPSHOT_EXPIRED",
                },
            );
            assert_eq!(f.count(), before, "R2_REJECTED_PUBLICATION_RETAINED_TICKET");
            if let Some(id) = ticket {
                assert_eq!(
                    Some(f.service.snapshots.lock().unwrap().projects[&id].binding_count()),
                    bindings,
                    "R2_REJECTED_PUBLICATION_ADDED_BINDING"
                );
            }
        }
    }
    for change in ["cancel", "command", "disable", "revision"] {
        let f = Fixture::new().await;
        let owner = f.service.owner();
        let project = f.project(&owner, "Discovery publication").await;
        let reader = f
            .profile("r2-discovery-publication", std::slice::from_ref(&project))
            .await;
        f.update(&reader, |input| {
            input["allowed_command_ids"]
                .as_array_mut()
                .unwrap()
                .push(json!("commands.list"))
        })
        .await;
        let (ready, resume) = pause(&f.service, CatalogPhase::DiscoveryPublication);
        let token = CancellationToken::new();
        let cancellation = token.clone();
        let service = f.service.clone();
        let context = reader.clone();
        let task = tokio::spawn(async move {
            service
                .dispatch_cancel(context, Request::query("commands.list", json!({})), token)
                .await
        });
        reached(ready).await;
        match change {
            "cancel" => cancellation.cancel(),
            "command" => tokio::time::timeout(
                Duration::from_secs(5),
                f.update(&reader, |input| {
                    input["allowed_command_ids"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|id| id != "commands.list")
                }),
            )
            .await
            .expect("R2_DISCOVERY_REVOKE_BLOCKED"),
            "disable" => tokio::time::timeout(
                Duration::from_secs(5),
                f.update(&reader, |input| input["enabled"] = json!(false)),
            )
            .await
            .expect("R2_DISCOVERY_DISABLE_BLOCKED"),
            "revision" => tokio::time::timeout(
                Duration::from_secs(5),
                f.update(&reader, |input| input["owned_object_access"] = json!(true)),
            )
            .await
            .expect("R2_DISCOVERY_REVISION_BLOCKED"),
            _ => unreachable!(),
        }
        resume.send(false).unwrap();
        let response = finish(task).await;
        assert!(response.result.is_none());
        failed(
            response,
            if change == "cancel" {
                "CANCELLED"
            } else {
                "PERMISSION_DENIED"
            },
        );
    }
}

fn metadata_profile(f: &Fixture, reader: &Context, project: &str, actions: Value) -> AccessProfile {
    let profile: AccessProfile = f
        .service
        .store
        .get(&format!("profile/{}", reader.profile.as_ref().unwrap()))
        .unwrap()
        .unwrap();
    assert!(
        profile.principal == reader.principal,
        "R2_PRINCIPAL_CHANGED"
    );
    assert!(
        Some(&profile.id) == reader.profile.as_ref(),
        "R2_PROFILE_CHANGED"
    );
    assert!(!profile.owned_object_access);
    assert!(
        serde_json::to_value(&profile.object_grants).unwrap()
            == json!([{"kind":"project","id":project,"actions":actions}]),
        "R2_EXPLICIT_PROJECT_GRANTS_CHANGED"
    );
    profile
}

fn metadata_catalog(response: Response, project: &str, name: &str, private: &[&str]) -> Value {
    assert!(response.ok, "R2_CATALOG_NOT_OK");
    assert!(response.error.is_none(), "R2_CATALOG_ERROR");
    assert!(response.receipt_id.is_none(), "R2_CATALOG_RECEIPT");
    let page = response.result.expect("R2_CATALOG_RESULT_MISSING");
    assert!(page["schema_version"] == 1, "R2_CATALOG_SCHEMA_CHANGED");
    assert!(
        page["projection"] == "projects.summary.v1",
        "R2_CATALOG_PROJECTION_CHANGED"
    );
    assert!(
        page.get("project_revision") == Some(&Value::Null),
        "R2_CATALOG_REVISION_CHANGED"
    );
    assert!(page["total_count"] == 1, "R2_CATALOG_COUNT_CHANGED");
    assert!(
        page["count_accuracy"] == "exact",
        "R2_CATALOG_ACCURACY_CHANGED"
    );
    assert!(
        page.get("next_cursor") == Some(&Value::Null),
        "R2_CATALOG_CURSOR_PRESENT"
    );
    let rows = page["items"].as_array().expect("R2_CATALOG_ITEMS_MISSING");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    let mut legacy = row.clone();
    let _ = legacy
        .as_object_mut()
        .expect("R2_CATALOG_ROW_NOT_OBJECT")
        .remove("authorization");
    assert!(
        legacy
            == json!({"id":project,"name":name,"revision":"0","schema_major":1,
            "envelope_version":1,"compatibility":"supported"}),
        "R2_LEGACY_HEADER_CHANGED"
    );
    let serialized = page.to_string();
    for forbidden in private.iter().copied().chain([
        "creator",
        "principal",
        "profile",
        "read_roots",
        "write_roots",
        "private_path",
        "source_identity",
        "\"path\"",
        "\"paths\"",
        "\"root\"",
        "\"roots\"",
        "\"private\"",
        "\"payload\"",
        "future",
    ]) {
        let encoded = serde_json::to_string(forbidden).unwrap();
        assert!(
            !serialized.contains(forbidden) && !serialized.contains(&encoded[1..encoded.len() - 1]),
            "R2_CATALOG_PRIVACY_LEAK"
        );
    }
    row.clone()
}

fn assert_command_metadata(auth: &Value, profile: &AccessProfile, edit: bool, link_removed: bool) {
    let fields = auth.as_object().expect("R2_AUTHORIZATION_NOT_OBJECT");
    assert_eq!(fields.len(), 6);
    assert!(
        auth["schema_version"] == 1,
        "R2_AUTHORIZATION_SCHEMA_CHANGED"
    );
    let expected_policy_revision = profile.revision.to_string();
    assert!(
        auth["policy_revision"].as_str() == Some(expected_policy_revision.as_str()),
        "R2_POLICY_REVISION_CHANGED"
    );
    assert!(
        auth["observed_project_revision"] == "0",
        "R2_OBSERVED_REVISION_CHANGED"
    );
    assert!(
        auth["current_read"] == json!({"state":"requires_request_validation","reasons":[]}),
        "R2_CURRENT_READ_CHANGED"
    );
    assert!(
        auth["current_edit"]
            == if edit {
                json!({"state":"requires_request_validation","reasons":[]})
            } else {
                json!({"state":"unavailable","reasons":["project_edit_not_granted"]})
            },
        "R2_CURRENT_EDIT_CHANGED"
    );
    let commands = [
        "projects.inspect",
        "sequences.create",
        "sequences.inspect",
        "clips.place",
        "clips.move",
        "clips.link",
        "clips.unlink",
        "clips.split",
        "clips.remove",
        "history.undo",
        "history.redo",
    ];
    let reasons = [
        "command_not_allowed",
        "project_read_not_granted",
        "project_edit_not_granted",
        "unsupported_project_schema",
        "writer_observed_unavailable",
    ];
    let gates = [
        "current_policy",
        "project_context",
        "project_rights",
        "request_parameters",
        "expected_revision",
        "writer_lease",
        "affected_scope",
        "history_state",
        "source_object_rights",
        "source_path_and_identity",
    ];
    let actions = auth["actions"].as_array().expect("R2_ACTIONS_NOT_ARRAY");
    assert_eq!(actions.len(), commands.len());
    for (action, command) in actions.iter().zip(commands) {
        assert_eq!(action.as_object().expect("R2_ACTION_NOT_OBJECT").len(), 4);
        assert!(action["command_id"] == command, "R2_ACTION_ORDER_CHANGED");
        assert_finite_metadata_labels(&action["reasons"], &reasons);
        assert_finite_metadata_labels(&action["unchecked_gates"], &gates);
        let blocked = !action["reasons"].as_array().unwrap().is_empty();
        assert!(
            action["state"]
                == if blocked {
                    "unavailable"
                } else {
                    "requires_request_validation"
                },
            "R2_ACTION_STATE_CHANGED"
        );
        let unchecked = action["unchecked_gates"].as_array().unwrap();
        assert!(unchecked.contains(&json!("current_policy")));
        assert!(unchecked.contains(&json!("request_parameters")));
        if command == "clips.link" || command == "clips.unlink" {
            let expected_reasons = if !edit {
                json!(["project_edit_not_granted"])
            } else if command == "clips.link" && link_removed {
                json!(["command_not_allowed"])
            } else {
                json!([])
            };
            assert!(
                action["reasons"] == expected_reasons,
                "R2_LINK_UNLINK_REASONS_CHANGED"
            );
            assert!(
                action["unchecked_gates"]
                    == json!([
                        "current_policy",
                        "project_context",
                        "request_parameters",
                        "expected_revision",
                        "writer_lease",
                        "affected_scope"
                    ]),
                "R2_LINK_UNLINK_GATES_CHANGED"
            );
        }
    }
}

fn assert_finite_metadata_labels(value: &Value, vocabulary: &[&str]) {
    let labels = value.as_array().expect("R2_LABELS_NOT_ARRAY");
    assert!(labels.len() <= vocabulary.len());
    let positions: Vec<_> = labels
        .iter()
        .map(|label| {
            vocabulary
                .iter()
                .position(|known| label.as_str() == Some(*known))
                .expect("R2_UNKNOWN_METADATA_LABEL")
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
