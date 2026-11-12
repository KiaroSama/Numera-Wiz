//! Additional cases run inside the existing service test identities.
use super::*;
use nw_domain::commands::ProjectRef;
use nw_domain::projects::edit::ProjectRead;
async fn write(
    service: &Arc<Service>,
    context: &Context,
    scope: &str,
    project: Option<(&str, &str)>,
    command: &str,
    key: &str,
    params: Value,
) -> Response {
    let mut request = Request::query(command, params);
    request.replay_scope = Some(scope.into());
    request.idempotency_key = Some(key.into());
    request.project = project.map(|(id, revision)| ProjectRef {
        id: id.into(),
        expected_revision: Some(revision.into()),
    });
    service.dispatch(context.clone(), request).await
}
async fn query(
    service: &Arc<Service>,
    context: &Context,
    project: &str,
    command: &str,
    params: Value,
) -> Response {
    let mut request = Request::query(command, params);
    request.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: None,
    });
    service.dispatch(context.clone(), request).await
}
fn good(response: Response) -> Value {
    assert!(response.ok, "{:?}", response.error);
    assert!(response.receipt_id.is_some());
    response.result.unwrap()
}
fn view(response: Response) -> Value {
    assert!(response.ok, "{:?}", response.error);
    assert!(response.receipt_id.is_none());
    response.result.unwrap()
}
fn failed(response: Response, code: &str) {
    assert!(!response.ok);
    assert!(response.receipt_id.is_none());
    assert_eq!(response.error.unwrap().code, code);
}
pub(in crate::runtime) async fn indexed_snapshot_and_edit_cases() {
    let mut conformance = crate::project_schema_batch(2, &[1,2,3,4,5,6,7,8,9,10,20,21,22,23,24,25,26,27,28,29,30,31,32,33,50,51,52,53,54,55,56,57,58,59,60,61]);
    let root = tempfile::tempdir().unwrap();
    let service = Service::open(root.path().join("data"), None).await.unwrap();
    let actor = service.profile_context("browser").unwrap();
    let scope = service
        .store
        .issue_scope_with_clock(&actor.principal, || service.clock.now())
        .unwrap();
    let created = good(conformance.capture(1, "projects.create", 
        write(
            &service,
            &actor,
            &scope,
            None,
            "projects.create",
            "create-indexed",
            json!({"name":"Indexed fixture"}),
        )
        .await));
    let project = created["project_id"].as_str().unwrap();
    let bin = created["root_bin_id"].as_str().unwrap();
    let sequence = good(conformance.capture(2, "sequences.create", write(&service,&actor,&scope,Some((project,"0")),"sequences.create","sequence-indexed",json!({"name":"Sequence","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"}]})).await));
    let sequence_id = sequence["created"]["sequence_id"].as_str().unwrap();
    let track = sequence["created"]["tracks"][0]["id"].as_str().unwrap();
    let mut revision = "1".to_owned();
    let mut clip_ids = Vec::new();
    for index in 0..130 {
        let placed = good(write(&service,&actor,&scope,Some((project,&revision)),"clips.place",&format!("indexed-place-{index}"),json!({"sequence_id":sequence_id,"track_id":track,"source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"1","den":"1"}},"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},"sequence_start":{"num":(index*2).to_string(),"den":"1"},"collision":"reject"})).await);
        revision = placed["revision"].as_str().unwrap().to_owned();
        clip_ids.push(
            placed["created"]["clip_ids"][0]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    let first = view(conformance.capture(3, "projects.inspect", 
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"bin_page","bin_id":bin,"limit":128}),
        )
        .await));
    assert_eq!(first["items"].as_array().unwrap().len(), 128);
    assert_eq!(first["total_count"], 131);
    assert!(serde_json::to_vec(&first).unwrap().len() < 240 * 1024);
    let cursor = first["next_cursor"].as_str().unwrap();
    {
        let indexed = service.store.project_read(project).unwrap();
        let page = indexed
            .bin_page(&EntityId::parse(bin).unwrap(), None, 128, 240 * 1024)
            .unwrap();
        assert!(page.scanned <= 129);
        let late = indexed
            .clip_range(
                &EntityId::parse(sequence_id).unwrap(),
                &[EntityId::parse(track).unwrap()],
                nw_domain::time::TimeRange {
                    start: nw_domain::time::Rational::new(258, 1).unwrap(),
                    end: nw_domain::time::Rational::new(260, 1).unwrap(),
                },
                None,
                128,
                240 * 1024,
            )
            .unwrap();
        assert_eq!(late.items.len(), 1);
        assert!(
            late.scanned <= 5,
            "A late narrow viewport must not scan preceding clips"
        );
    }
    let old_revision = revision.clone();
    let moved = good(conformance.capture(4, "clips.move", write(&service,&actor,&scope,Some((project,&revision)),"clips.move","remote-move",json!({"clip_ids":[clip_ids[0]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"})).await));
    revision = moved["revision"].as_str().unwrap().to_owned();
    let next = view(conformance.capture(5, "projects.inspect", 
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"bin_page","bin_id":bin,"limit":128,"cursor":cursor}),
        )
        .await));
    assert!(next["next_cursor"].is_null(),"SCHEMA_BIN_END_CURSOR");
    assert_eq!(next["project_revision"], old_revision);
    assert_eq!(next["snapshot_id"], first["snapshot_id"]);
    assert_eq!(next["items"].as_array().unwrap().len(), 3);
    let mut all = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(next["items"].as_array().unwrap())
        .map(|i| i["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 131);
    for params in [
        json!({"projection":"bin_page","bin_id":bin,"limit":127,"cursor":cursor}),
        json!({"projection":"bin_page","bin_id":track,"limit":128,"cursor":cursor}),
    ] {
        failed(
            query(&service, &actor, project, "projects.inspect", params).await,
            "SNAPSHOT_EXPIRED",
        );
    }
    failed(
        query(
            &service,
            &service.owner(),
            project,
            "projects.inspect",
            json!({"projection":"bin_page","bin_id":bin,"limit":128,"cursor":cursor}),
        )
        .await,
        "SNAPSHOT_EXPIRED",
    );
    let range = json!({"start":{"num":"0","den":"1"},"end":{"num":"260","den":"1"}});
    let viewport = view(conformance.capture(6, "sequences.inspect", query(&service,&actor,project,"sequences.inspect",json!({"projection":"viewport","sequence_id":sequence_id,"track_ids":[track],"range":range,"limit":128,"revision":old_revision})).await));
    assert_eq!(viewport["snapshot_id"], first["snapshot_id"]);
    assert_eq!(
        viewport["items"][0]["sequence_start"],
        json!({"num":"0","den":"1"})
    );
    assert_eq!(viewport["count_accuracy"], "lower_bound");
    let rest = view(conformance.capture(7, "sequences.inspect", query(&service,&actor,project,"sequences.inspect",json!({"projection":"viewport","sequence_id":sequence_id,"track_ids":[track],"range":range,"limit":128,"cursor":viewport["next_cursor"]})).await));
    assert!(viewport["next_cursor"].is_string(),"SCHEMA_VIEWPORT_CONTINUATION");
    assert!(rest["next_cursor"].is_null(),"SCHEMA_VIEWPORT_END_CURSOR");
    assert_eq!(rest["total_count"], 130);
    assert_eq!(rest["count_accuracy"], "exact");
    failed(query(&service,&actor,project,"sequences.inspect",json!({"projection":"viewport","sequence_id":sequence_id,"track_ids":[track],"range":{"start":{"num":"1","den":"1"},"end":{"num":"260","den":"1"}},"limit":128,"cursor":viewport["next_cursor"]})).await,"SNAPSHOT_EXPIRED");
    let boundary = good(conformance.capture(8, "clips.split", write(&service,&actor,&scope,Some((project,&revision)),"clips.split","boundary-no-change",json!({"clip_id":clip_ids[0],"at_sequence":{"num":"1","den":"4"},"link_policy":"expand"})).await));
    assert_eq!(boundary["revision"], revision);
    assert_eq!(boundary["no_change"], true);
    assert!(
        boundary["created"]["clip_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for (key, delta, code) in [
        ("off-grid", json!({"num":"1","den":"3"}), "OFF_GRID"),
        (
            "negative",
            json!({"num":"-1","den":"1"}),
            "VALIDATION_ERROR",
        ),
        ("collision", json!({"num":"2","den":"1"}), "CLIP_COLLISION"),
    ] {
        failed(write(&service,&actor,&scope,Some((project,&revision)),"clips.move",key,json!({"clip_ids":[clip_ids[0]],"delta":delta,"collision":"reject","link_policy":"expand"})).await,code);
    }
    let held = service.store.project_read(project).unwrap();
    let selected = EntityId::parse(clip_ids[0].clone()).unwrap();
    let mut clip = held.entity(&selected).unwrap();
    let original = clip.bytes().unwrap();
    let mut payload: Clip = clip.payload("clip").unwrap();
    payload.locked = true;
    clip.update(&payload).unwrap();
    service
        .store
        .put(&format!("project-entity/{project}/{selected}"), &clip)
        .unwrap();
    failed(write(&service,&actor,&scope,Some((project,&revision)),"clips.move","locked",json!({"clip_ids":[clip_ids[0]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"})).await,"ENTITY_LOCKED");
    service
        .store
        .put(
            &format!("project-entity/{project}/{selected}"),
            &serde_json::from_str::<EntityEnvelope>(&original).unwrap(),
        )
        .unwrap();
    // A declared unknown dependency blocks its affected branch and survives independent edits byte-for-byte.
    let unknown_id = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
    let project_id = ProjectId::parse(project).unwrap();
    let mut unknown = EntityEnvelope::new(
        &project_id,
        unknown_id.clone(),
        "future_effect",
        Some(selected.clone()),
        "Future effect".into(),
        &json!({}),
    )
    .unwrap();
    unknown.payload_json = r#"{"exact":900719925474099312345678901}"#.into();
    unknown.payload_sha256 = domain::hash(unknown.payload_json.as_bytes());
    unknown.references.push(EntityReference {
        role: "semantic_input".into(),
        target: selected.clone(),
    });
    service
        .store
        .put(&format!("project-entity/{project}/{unknown_id}"), &unknown)
        .unwrap();
    service
        .store
        .put(
            &format!("project-dependency/{project}/{selected}/{unknown_id}"),
            &unknown_id,
        )
        .unwrap();
    let unknown_before = service
        .store
        .project_fact_hash(&format!("project-entity/{project}/{unknown_id}"))
        .unwrap();
    failed(write(&service,&actor,&scope,Some((project,&revision)),"clips.move","unknown-affected",json!({"clip_ids":[clip_ids[0]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"})).await,"UNSUPPORTED_SEMANTIC_NODE");
    let independent = good(write(&service,&actor,&scope,Some((project,&revision)),"clips.move","unknown-independent",json!({"clip_ids":[clip_ids[10]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"})).await);
    revision = independent["revision"].as_str().unwrap().to_owned();
    assert_eq!(
        service
            .store
            .project_fact_hash(&format!("project-entity/{project}/{unknown_id}"))
            .unwrap(),
        unknown_before
    );
    drop(held);
    service.project_snapshot_pressure(8 * 1024 * 1024).unwrap();
    failed(
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"bin_page","bin_id":bin,"limit":128,"cursor":cursor}),
        )
        .await,
        "SNAPSHOT_EXPIRED",
    );
    failed(
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"summary","revision":old_revision}),
        )
        .await,
        "REVISION_NOT_RETAINED",
    );
    let mut latest = view(
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"summary"}),
        )
        .await,
    );
    for _ in 0..8 {
        latest = view(
            query(
                &service,
                &actor,
                project,
                "projects.inspect",
                json!({"projection":"summary","replace_snapshot":latest["snapshot_id"]}),
            )
            .await,
        );
    }
    assert_eq!(latest["project_revision"], revision);
    let mut profile: AccessProfile = service.store.get("profile/browser").unwrap().unwrap();
    profile.revision += 1;
    profile.owned_object_access = false;
    service.store.put("profile/browser", &profile).unwrap();
    failed(
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"summary","revision":revision}),
        )
        .await,
        "OBJECT_NOT_FOUND",
    );
    let mut retry = Request::query("projects.create", json!({"name":"Indexed fixture"}));
    retry.replay_scope = Some(scope);
    retry.idempotency_key = Some("create-indexed".into());
    let restricted = service.dispatch(actor, retry).await;
    assert!(!restricted.ok);
    assert!(restricted.receipt_id.is_some());
    assert_eq!(restricted.error.unwrap().code, "RECEIPT_RESULT_RESTRICTED");
    // Unsupported project major remains inspectable and byte-preserved under rejected writes.
    let mut header = service
        .store
        .project_read(project)
        .unwrap()
        .header()
        .clone();
    header.schema_major = 99;
    service
        .store
        .put(&format!("project/{project}"), &header)
        .unwrap();
    let before = service
        .store
        .project_fact_hash(&format!("project/{project}"))
        .unwrap();
    let owner = service.owner();
    let owner_scope = service
        .store
        .issue_scope_with_clock(&owner.principal, || service.clock.now())
        .unwrap();
    let summary = view(conformance.capture(10,"projects.inspect",
        query(
            &service,
            &owner,
            project,
            "projects.inspect",
            json!({"projection":"summary"}),
        )
        .await));
    assert_eq!(summary["compatibility"], "read_only_unsupported");
    failed(
        write(
            &service,
            &owner,
            &owner_scope,
            Some((project, &revision)),
            "history.undo",
            "unknown-major",
            json!({}),
        )
        .await,
        "PROJECT_READ_ONLY",
    );
    assert_eq!(
        before,
        service
            .store
            .project_fact_hash(&format!("project/{project}"))
            .unwrap()
    );
    // Independent finite fixture follows every original indexed/raw/rights oracle.
    let owner = service.owner();
    let extra_scope = service.store.issue_scope_with_clock(&owner.principal, || service.clock.now()).unwrap();
    let extra = good(conformance.capture(20,"projects.create",write(&service,&owner,&extra_scope,None,"projects.create","schema-small-create",json!({"name":"Schema projections"})).await));
    let extra_id = extra["project_id"].as_str().unwrap();
    let extra_bin = extra["root_bin_id"].as_str().unwrap();
    let small = good(conformance.capture(21,"sequences.create",write(&service,&owner,&extra_scope,Some((extra_id,"0")),"sequences.create","schema-small-sequence",json!({"name":"Both track codecs","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"},{"kind":"audio","name":"Audio"}]})).await));
    let seq = small["created"]["sequence_id"].as_str().unwrap();
    let video = small["created"]["tracks"][0]["id"].as_str().unwrap();
    let audio = small["created"]["tracks"][1]["id"].as_str().unwrap();
    let placed = good(conformance.capture(22,"clips.place",write(&service,&owner,&extra_scope,Some((extra_id,"1")),"clips.place","schema-small-place",json!({"sequence_id":seq,"track_id":video,"source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"1","den":"1"}},"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},"sequence_start":{"num":"0","den":"1"},"collision":"reject"})).await));
    let summary = query(&service,&owner,extra_id,"projects.inspect",json!({"projection":"summary"})).await;
    conformance.observe(9,"projects.inspect",&summary);
    let mut ticket = view(summary)["snapshot_id"].clone();
    for (case,entity) in [(23,extra_bin),(24,seq),(25,video),(26,audio),(27,placed["created"]["clip_ids"][0].as_str().unwrap()),(28,placed["created"]["source_id"].as_str().unwrap())] {
        let response = query(&service,&owner,extra_id,"projects.inspect",json!({"projection":"entity","entity_id":entity,"replace_snapshot":ticket})).await;
        conformance.observe(case,"projects.inspect",&response);
        ticket = view(response)["snapshot_id"].clone();
    }
    let sequence_entity = view(conformance.capture(29,"sequences.inspect",query(&service,&owner,extra_id,"sequences.inspect",json!({"projection":"entity","sequence_id":seq,"replace_snapshot":ticket})).await));
    let tracks_first = view(conformance.capture(30,"sequences.inspect",query(&service,&owner,extra_id,"sequences.inspect",json!({"projection":"track_page","sequence_id":seq,"limit":1,"replace_snapshot":sequence_entity["snapshot_id"]})).await));
    assert!(tracks_first["next_cursor"].is_string());
    let tracks_end = view(conformance.capture(31,"sequences.inspect",query(&service,&owner,extra_id,"sequences.inspect",json!({"projection":"track_page","sequence_id":seq,"limit":1,"cursor":tracks_first["next_cursor"]})).await));
    assert!(tracks_end["next_cursor"].is_null());
    let catalog = view(conformance.capture(32,"projects.list",service.dispatch(owner.clone(),Request::query("projects.list",json!({"limit":1,"replace_snapshot":tracks_end["snapshot_id"]}))).await));
    assert!(catalog["next_cursor"].is_string());
    let catalog_end = view(conformance.capture(33,"projects.list",service.dispatch(owner,Request::query("projects.list",json!({"limit":1,"cursor":catalog["next_cursor"]}))).await));
    assert!(catalog_end["next_cursor"].is_null());
    conformance.reject_copy(50,20,|v| {v.as_object_mut().unwrap().remove("project_id");});
    conformance.reject_copy(51,20,|v| {v["extra"]=json!(true);});
    conformance.reject_copy(52,20,|v| {v["project_id"]=json!("bad-uuid");});
    conformance.reject_copy(53,20,|v| {v["revision"]=json!("01");});
    conformance.reject_copy(54,9,|v| {v["entity_count"]=json!("1");});
    conformance.reject_copy(55,9,|v| {v["read_only"]=json!(0);});
    conformance.reject_copy(56,3,|v| {v["projection"]=json!("not-a-projection");});
    conformance.reject_copy(57,3,|v| {let item=v["items"][0].clone(); v["items"]=json!(vec![item;513]);});
    conformance.reject_copy(58,7,|v| {v.as_object_mut().unwrap().remove("next_cursor");});
    conformance.reject_copy(59,28,|v| {v["entity"]["payload_sha256"]=json!("not-a-hash");});
    conformance.reject_copy(60,27,|v| {v["entity"]["duration"]["den"]=json!("0");});
    conformance.reject_copy(61,9,|v| {v["writer_status"]=json!("unknown");});
    conformance.finish().await;

}

pub(in crate::runtime) async fn held_source_current_grants() {
    let mut conformance = crate::project_schema_batch(3,&[1,2,3,4,5,6]);
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("registered-model-source.bin");
    std::fs::write(&file, b"bounded held-source test content").unwrap();
    let service = Service::open(temporary.path().join("data"), None)
        .await
        .unwrap();
    let actor = service.profile_context("browser").unwrap();
    let mut profile: AccessProfile = service.store.get("profile/browser").unwrap().unwrap();
    // Stored grants use canonical roots, as the profile command does in production.
    let grant_root = std::fs::canonicalize(temporary.path()).unwrap();
    profile.read_roots = vec![files::path_text(&grant_root).unwrap()];
    profile.revision += 1;
    service.store.put("profile/browser", &profile).unwrap();
    let cancel = CancellationToken::new();
    let identity = files::identity(&file, &cancel).unwrap();
    assert!(Path::new(&identity.path).starts_with(&grant_root));
    let asset = Asset {
        id: Uuid::new_v4().to_string(),
        creator: actor.principal.clone(),
        display_name: "Held model source".into(),
        identity,
        source_object: Some(files::source_object(&file).unwrap()),
        probe: nw_domain::records::MediaProbe {
            probe_timeout: None,
            streams: vec![nw_domain::mux::Stream {
                index: 7,
                kind: nw_domain::mux::StreamKind::Video,
                codec: "model-only".into(),
                tags: Default::default(),
                dispositions: Default::default(),
            }],
            stream_details: Default::default(),
            format: "model-only-no-decoder".into(),
            global_tags: Default::default(),
            chapters: json!([]),
            verification_scope:
                "Storage/rights fixture; real registered media is exercised by owner_media".into(),
        },
    };
    service
        .store
        .put(&format!("asset/{}", asset.id), &asset)
        .unwrap();
    let scope = service
        .store
        .issue_scope_with_clock(&actor.principal, || service.clock.now())
        .unwrap();
    let created = good(
        write(
            &service,
            &actor,
            &scope,
            None,
            "projects.create",
            "held-create",
            json!({"name":"Held source"}),
        )
        .await,
    );
    let project = created["project_id"].as_str().unwrap();
    let sequence=good(write(&service,&actor,&scope,Some((project,"0")),"sequences.create","held-sequence",json!({"name":"Sequence","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"}]})).await);
    let params = json!({"sequence_id":sequence["created"]["sequence_id"],"track_id":sequence["created"]["tracks"][0]["id"],"source":{"kind":"registered","asset_id":asset.id,"stream_index":7},"source_range":{"start":{"num":"-1","den":"1"},"end":{"num":"0","den":"1"}},"sequence_start":{"num":"0","den":"1"},"collision":"reject"});
    let weak = Arc::downgrade(&service);
    let preflight_observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_observed = preflight_observed.clone();
    let mut revoked = profile.clone();
    revoked.read_roots.clear();
    revoked.revision += 1;
    *service.preflight_hook.lock().unwrap() = Some(Arc::new(move || {
        let service = weak.upgrade().unwrap();
        service.store.put("profile/browser", &revoked).unwrap();
        hook_observed.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    failed(
        write(
            &service,
            &actor,
            &scope,
            Some((project, "1")),
            "clips.place",
            "held-placement",
            params.clone(),
        )
        .await,
        "PATH_DENIED",
    );
    *service.preflight_hook.lock().unwrap() = None;
    assert!(preflight_observed.load(std::sync::atomic::Ordering::SeqCst));
    profile.revision += 2;
    service.store.put("profile/browser", &profile).unwrap();
    assert_eq!(
        service
            .store
            .project_read(project)
            .unwrap()
            .header()
            .revision,
        Revision(1)
    );
    let accepted = good(conformance.capture(1, "clips.place", 
        write(
            &service,
            &actor,
            &scope,
            Some((project, "1")),
            "clips.place",
            "held-placement",
            params,
        )
        .await));
    let source = accepted["created"]["source_id"].as_str().unwrap();
    let clip = accepted["created"]["clip_ids"][0].as_str().unwrap();
    let binding = view(conformance.capture(2, "projects.inspect", 
        query(
            &service,
            &actor,
            project,
            "projects.inspect",
            json!({"projection":"entity","entity_id":source}),
        )
        .await));
    assert_eq!(binding["entity"]["stream_index"], 7);
    assert_eq!(binding["entity"]["content_sha256"], asset.identity.sha256);
    assert!(!binding.to_string().contains(file.to_str().unwrap()));
    let mut different = Request::query("clips.place", json!({"bad":"changed"}));
    different.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: Some("1".into()),
    });
    different.replay_scope = Some(scope.clone());
    different.idempotency_key = Some("held-placement".into());
    failed(
        service.dispatch(actor.clone(), different).await,
        "IDEMPOTENCY_CONFLICT",
    );
    good(conformance.capture(4,"clips.remove",
        write(
            &service,
            &actor,
            &scope,
            Some((project, "2")),
            "clips.remove",
            "held-remove",
            json!({"clip_ids":[clip],"mode":"lift","link_policy":"expand"}),
        )
        .await));
    std::fs::remove_file(&file).unwrap();
    profile.read_roots.clear();
    profile.revision += 1;
    service.store.put("profile/browser", &profile).unwrap();
    // Undo restores the relationship and ID even when the source file is absent and path rights were withdrawn.
    good(conformance.capture(5,"history.undo",
        write(
            &service,
            &actor,
            &scope,
            Some((project, "3")),
            "history.undo",
            "held-undo",
            json!({}),
        )
        .await));
    let restored=view(conformance.capture(3, "projects.inspect", query(&service,&actor,project,"projects.inspect",json!({"projection":"entity","entity_id":clip,"replace_snapshot":binding["snapshot_id"]})).await));
    assert_eq!(restored["entity"]["source_id"], source);
    assert_eq!(
        restored["entity"]["source_range"]["start"],
        json!({"num":"-1","den":"1"})
    );
    conformance.reject_copy(6,2,|v| {v["entity"]["stream_index"]=json!(4294967296u64);});
    conformance.finish().await;

}
