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
    assert_eq!(page["items"][0].as_object().unwrap().len(), 6);
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
        catalog["items"][0],
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
        for action in ["cancel", "expire", "revoke", "disable", "revision"] {
            if phase == CatalogPhase::Entry && action != "cancel" {
                continue;
            }
            let reader = f
                .profile("publication", if empty { &[] } else { projects })
                .await;
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
