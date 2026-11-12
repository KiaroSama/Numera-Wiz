//! Review regressions use isolated projects and retain the original test entrypoints.
use super::*;
use nw_domain::commands::ProjectRef;
use nw_domain::projects::edit::ProjectRead;

struct Fixture {
    service: Arc<Service>,
    context: Context,
    scope: String,
    project: ProjectId,
    clips: Vec<EntityId>,
    sources: Vec<EntityId>,
    _root: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let context = service.owner();
        let scope = service
            .store
            .issue_scope_with_clock(&context.principal, || service.clock.now())
            .unwrap();
        let mut create = Request::query("projects.create", json!({"name":"Review regression"}));
        create.replay_scope = Some(scope.clone());
        create.idempotency_key = Some("review-project".into());
        let response = service.dispatch(context.clone(), create).await;
        assert!(response.ok, "{:?}", response.error);
        let project =
            ProjectId::parse(response.result.unwrap()["project_id"].as_str().unwrap()).unwrap();
        let mut fixture = Self {
            service,
            context,
            scope,
            project,
            clips: Vec::new(),
            sources: Vec::new(),
            _root: root,
        };
        let mut sequences = Vec::new();
        for number in 0..2 {
            let request = fixture.request("sequences.create", &format!("review-sequence-{number}"), json!({
                "name":format!("Sequence {number}"),
                "settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
                "tracks":[{"kind":"video","name":"First"},{"kind":"video","name":"Second"}]
            }));
            let response = fixture
                .service
                .dispatch(fixture.context.clone(), request)
                .await;
            assert!(response.ok, "{:?}", response.error);
            sequences.push(response.result.unwrap()["created"].clone());
        }
        for (number, sequence, track, start) in [
            (0usize, 0usize, 0usize, 0i64),
            (1, 0, 0, 4),
            (2, 0, 1, 0),
            (3, 1, 0, 0),
        ] {
            let request = fixture.request("clips.place", &format!("review-place-{number}"), json!({
                "sequence_id":sequences[sequence]["sequence_id"],"track_id":sequences[sequence]["tracks"][track]["id"],
                "source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},
                "source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
                "sequence_start":{"num":start.to_string(),"den":"1"},"collision":"reject"
            }));
            let response = fixture
                .service
                .dispatch(fixture.context.clone(), request)
                .await;
            assert!(response.ok, "{:?}", response.error);
            let result = response.result.unwrap();
            fixture
                .clips
                .push(EntityId::parse(result["created"]["clip_ids"][0].as_str().unwrap()).unwrap());
            fixture
                .sources
                .push(EntityId::parse(result["created"]["source_id"].as_str().unwrap()).unwrap());
        }
        fixture
    }
    fn request(&self, command: &str, key: &str, params: Value) -> Request {
        let revision = self
            .service
            .store
            .project_read(self.project.as_str())
            .unwrap()
            .header()
            .revision;
        let mut request = Request::query(command, params);
        request.project = Some(ProjectRef {
            id: self.project.to_string(),
            expected_revision: Some(revision.to_string()),
        });
        request.replay_scope = Some(self.scope.clone());
        request.idempotency_key = Some(key.into());
        request
    }
    fn move_request(&self, clip: usize, delta: i64, key: &str) -> Request {
        self.request("clips.move", key, json!({"clip_ids":[self.clips[clip]],"delta":{"num":delta.to_string(),"den":"4"},"collision":"reject","link_policy":"expand"}))
    }
    fn raw(&self, id: &EntityId) -> String {
        self.service
            .store
            .project_read(self.project.as_str())
            .unwrap()
            .raw_entity(id)
            .unwrap()
            .unwrap()
    }
    fn entity(&self, id: &EntityId) -> EntityEnvelope {
        serde_json::from_str(&self.raw(id)).unwrap()
    }
    fn history_hash(&self) -> String {
        self.service
            .store
            .project_fact_hash(&format!("project-history/{}", self.project))
            .unwrap()
    }
    /// Simulate a retained future producer's envelope using the actual index transaction,
    /// so ancestor-scope regressions exercise index_rows instead of hand-building its answer.
    fn install_future_envelope(&self, entity: EntityEnvelope) {
        self.install_envelopes(vec![entity]);
    }
    fn install_envelopes(&self, entities: Vec<EntityEnvelope>) {
        let read = self
            .service
            .store
            .project_read(self.project.as_str())
            .unwrap();
        let request =
            self.move_request(0, 0, &format!("install-fixture-{}", read.header().revision));
        let changes = entities
            .into_iter()
            .map(|entity| EntityChange {
                before: read.raw_entity(&entity.id).unwrap(),
                after: Some(entity.bytes().unwrap()),
                id: entity.id,
            })
            .collect();
        let prepared = ProjectCommit {
            header: read.header().clone(),
            expected_header: Some(read.header_raw().into()),
            patch: ProjectPatch { changes },
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: HistoryAction::Normal,
            created: json!({}),
            expected_records: Vec::new(),
        };
        drop(read);
        self.service
            .store
            .commit_project_with_clock(&self.context.principal, &request, prepared, || {
                self.service.clock.now()
            })
            .unwrap();
    }
    async fn accept(&self, request: Request) -> Value {
        let response = self.service.dispatch(self.context.clone(), request).await;
        assert!(response.ok, "{:?}", response.error);
        assert!(response.receipt_id.is_some());
        response.result.unwrap()
    }
    async fn reject_unchanged(&self, request: Request, code: &str) {
        let before = self
            .service
            .store
            .project_read(self.project.as_str())
            .unwrap()
            .header_raw()
            .to_owned();
        let history = self.history_hash();
        let response = self
            .service
            .dispatch(self.context.clone(), request.clone())
            .await;
        assert_eq!(response.error.unwrap().code, code);
        assert!(response.receipt_id.is_none());
        assert_eq!(
            self.service
                .store
                .project_read(self.project.as_str())
                .unwrap()
                .header_raw(),
            before
        );
        assert_eq!(self.history_hash(), history);
        assert!(
            self.service
                .store
                .replay_with_clock(&self.context.principal, &request, || self
                    .service
                    .clock
                    .now())
                .unwrap()
                .is_none()
        );
    }
    async fn expect_blocked(&self, clip: usize, key: &str, failures: &mut Vec<String>) {
        for delta in [1, 0] {
            let key = format!("{key}-delta-{delta}");
            let before = self
                .service
                .store
                .project_read(self.project.as_str())
                .unwrap()
                .header_raw()
                .to_owned();
            let entity_before = self.raw(&self.clips[clip]);
            let history_before = self.history_hash();
            let request = self.move_request(clip, delta, &key);
            let response = self
                .service
                .dispatch(self.context.clone(), request.clone())
                .await;
            let code = response
                .error
                .as_ref()
                .map(|e| e.code.as_str())
                .unwrap_or("accepted");
            let recognized = [
                "PROJECT_READ_ONLY",
                "UNSUPPORTED_SEMANTIC_NODE",
                "OBJECT_NOT_FOUND",
                "PROJECT_CORRUPT",
                "VALIDATION_ERROR",
            ]
            .contains(&code);
            let current = self
                .service
                .store
                .project_read(self.project.as_str())
                .unwrap();
            let unchanged = current.header_raw() == before
                && self.raw(&self.clips[clip]) == entity_before
                && self.history_hash() == history_before;
            let no_receipt = self
                .service
                .store
                .replay_with_clock(&self.context.principal, &request, || {
                    self.service.clock.now()
                })
                .unwrap()
                .is_none();
            if response.ok
                || !recognized
                || response.receipt_id.is_some()
                || !unchanged
                || !no_receipt
            {
                failures.push(format!(
                    "{key}: result={code}, unchanged={unchanged}, no_receipt={no_receipt}"
                ));
            }
        }
    }
}

pub(in crate::runtime) async fn compatibility_and_zero_delta() {
    let mut failures = Vec::new();
    for (case, affected, independent, global_block) in [
        ("known_source_unknown_role", 0, 3, true),
        ("known_source_unknown_scope", 0, 3, true),
        ("known_source_unknown_envelope", 0, 3, true),
        ("missing_semantic_input", 0, 3, false),
        ("missing_source_reference", 0, 3, false),
        ("missing_ownership_reference", 0, 3, false),
        ("unknown_track_scope", 1, 3, false),
        ("unknown_sequence_scope", 2, 3, false),
        ("unknown_entity_scope", 0, 1, false),
    ] {
        let fixture = Fixture::new().await;
        let root = fixture
            .service
            .store
            .project_read(fixture.project.as_str())
            .unwrap()
            .header()
            .root_bin_id
            .clone();
        let mut entity = if case.starts_with("known_source") {
            fixture.entity(&fixture.sources[0])
        } else if case.starts_with("missing_") {
            fixture.entity(&fixture.clips[0])
        } else {
            EntityEnvelope::new(
                &fixture.project,
                EntityId::parse(Uuid::new_v4().to_string()).unwrap(),
                "future_effect",
                Some(fixture.clips[0].clone()),
                "Retained future effect".into(),
                &json!({"future":true}),
            )
            .unwrap()
        };
        match case {
            "known_source_unknown_role" => entity.references.push(EntityReference {
                role: "future_required_role".into(),
                target: root,
            }),
            "known_source_unknown_scope" => entity.semantic_scope = "future_scope".into(),
            "known_source_unknown_envelope" => entity.envelope_version = 2,
            "missing_semantic_input"
            | "missing_source_reference"
            | "missing_ownership_reference" => entity.references.push(EntityReference {
                role: match case {
                    "missing_source_reference" => "source",
                    "missing_ownership_reference" => "ownership",
                    _ => "semantic_input",
                }
                .into(),
                target: EntityId::parse(Uuid::new_v4().to_string()).unwrap(),
            }),
            _ => {
                entity.semantic_scope = match case {
                    "unknown_track_scope" => "track",
                    "unknown_sequence_scope" => "sequence",
                    _ => "entity",
                }
                .into();
                entity.references.push(EntityReference {
                    role: "semantic_input".into(),
                    target: fixture.clips[0].clone(),
                });
            }
        }
        let unknown_id = entity.id.clone();
        fixture.install_future_envelope(entity);
        fixture
            .expect_blocked(affected, &format!("{case}-affected"), &mut failures)
            .await;
        let opaque_before = fixture.raw(&unknown_id);
        if global_block {
            fixture
                .expect_blocked(independent, &format!("{case}-global"), &mut failures)
                .await;
        } else {
            let request = fixture.move_request(independent, 1, &format!("{case}-independent"));
            let response = fixture
                .service
                .dispatch(fixture.context.clone(), request)
                .await;
            if !response.ok || fixture.raw(&unknown_id) != opaque_before {
                failures.push(format!(
                    "{case}: independent branch failed or rewrote opaque bytes: {:?}",
                    response.error
                ));
            }
        }
    }
    // A fresh and an undo-restored clip are separate fixtures so one regression cannot mask the other.
    for restored in [false, true] {
        let fixture = Fixture::new().await;
        let original = fixture.raw(&fixture.clips[0]);
        if restored {
            let moved = fixture
                .service
                .dispatch(
                    fixture.context.clone(),
                    fixture.move_request(0, 1, "nonzero-before-undo"),
                )
                .await;
            assert!(moved.ok, "{:?}", moved.error);
            let undo = fixture.request("history.undo", "restore-original-bytes", json!({}));
            let undone = fixture
                .service
                .dispatch(fixture.context.clone(), undo)
                .await;
            assert!(undone.ok, "{:?}", undone.error);
            assert_eq!(fixture.raw(&fixture.clips[0]), original);
            assert!(
                fixture
                    .service
                    .store
                    .project_read(fixture.project.as_str())
                    .unwrap()
                    .history()
                    .unwrap()
                    .summary()["can_redo"]
                    .as_bool()
                    .unwrap()
            );
        }
        let before = fixture
            .service
            .store
            .project_read(fixture.project.as_str())
            .unwrap()
            .header_raw()
            .to_owned();
        let history_before = fixture.history_hash();
        let request = fixture.move_request(
            0,
            0,
            if restored {
                "restored-zero"
            } else {
                "fresh-zero"
            },
        );
        let response = fixture
            .service
            .dispatch(fixture.context.clone(), request.clone())
            .await;
        let retry = fixture
            .service
            .dispatch(fixture.context.clone(), request)
            .await;
        let unchanged = fixture
            .service
            .store
            .project_read(fixture.project.as_str())
            .unwrap()
            .header_raw()
            == before
            && fixture.raw(&fixture.clips[0]) == original
            && fixture.history_hash() == history_before;
        let no_change = response
            .result
            .as_ref()
            .is_some_and(|v| v["no_change"] == true && v["changed_ids"] == json!([]));
        let replayed = response.receipt_id.is_some()
            && response.receipt_id == retry.receipt_id
            && response.result == retry.result;
        if !response.ok || !no_change || !unchanged || !replayed {
            failures.push(format!("zero_delta restored={restored}: ok={}, no_change={no_change}, unchanged={unchanged}, replayed={replayed}",response.ok));
        }
    }
    assert!(
        failures.is_empty(),
        "I2/I4 project regression failures: {failures:#?}"
    );
}

pub(in crate::runtime) async fn linked_edits() {
    let fixture = Fixture::new().await;
    let group = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
    let mut first = fixture.entity(&fixture.clips[0]);
    let mut second = fixture.entity(&fixture.clips[2]);
    for entity in [&mut first, &mut second] {
        let mut clip: Clip = entity.payload("clip").unwrap();
        clip.link_group_id = Some(group.clone());
        entity.update(&clip).unwrap();
    }
    // A one-way edge exercises incoming expansion when selecting its target.
    first.references.push(EntityReference {
        role: "link".into(),
        target: second.id.clone(),
    });
    fixture.install_envelopes(vec![first, second]);
    let mut locked = fixture.entity(&fixture.clips[0]);
    let mut clip: Clip = locked.payload("clip").unwrap();
    clip.locked = true;
    locked.update(&clip).unwrap();
    fixture.install_future_envelope(locked.clone());
    fixture
        .reject_unchanged(fixture.move_request(2, 1, "linked-locked"), "ENTITY_LOCKED")
        .await;
    fixture
        .reject_unchanged(
            fixture.move_request(2, 0, "linked-locked-zero"),
            "ENTITY_LOCKED",
        )
        .await;
    clip.locked = false;
    locked.update(&clip).unwrap();
    fixture.install_future_envelope(locked);
    fixture.reject_unchanged(fixture.request("clips.split","linked-off-grid",json!({"clip_id":fixture.clips[2],"at_sequence":{"num":"1","den":"3"},"link_policy":"expand"})),"OFF_GRID").await;
    let boundary=fixture.accept(fixture.request("clips.split","linked-boundary",json!({"clip_id":fixture.clips[2],"at_sequence":{"num":"0","den":"1"},"link_policy":"expand"}))).await;
    assert_eq!(boundary["no_change"], true);
    let moved = fixture
        .accept(fixture.move_request(2, 1, "linked-incoming-move"))
        .await;
    assert_eq!(moved["changed_ids"].as_array().unwrap().len(), 2);
    for id in [&fixture.clips[0], &fixture.clips[2]] {
        assert_eq!(
            fixture
                .entity(id)
                .payload::<Clip>("clip")
                .unwrap()
                .sequence_start,
            nw_domain::time::Rational::new(1, 4).unwrap()
        );
    }
    let split=fixture.accept(fixture.request("clips.split","linked-split",json!({"clip_id":fixture.clips[2],"at_sequence":{"num":"3","den":"4"},"link_policy":"expand"}))).await;
    let right_ids = split["created"]["clip_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| EntityId::parse(id.as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(right_ids.len(), 2);
    let right_entities = right_ids
        .iter()
        .map(|id| fixture.entity(id))
        .collect::<Vec<_>>();
    let right_group = right_entities[0]
        .payload::<Clip>("clip")
        .unwrap()
        .link_group_id
        .unwrap();
    assert_ne!(right_group, group);
    for right in &right_entities {
        let right_clip: Clip = right.payload("clip").unwrap();
        assert_eq!(right_clip.link_group_id.as_ref(), Some(&right_group));
        let left_id = [&fixture.clips[0], &fixture.clips[2]]
            .into_iter()
            .find(|id| {
                fixture
                    .entity(id)
                    .payload::<Clip>("clip")
                    .unwrap()
                    .source_id
                    == right_clip.source_id
            })
            .unwrap();
        let left: Clip = fixture.entity(left_id).payload("clip").unwrap();
        assert_eq!(left.link_group_id.as_ref(), Some(&group));
        assert_eq!(left.source_range.start, nw_domain::time::Rational::ZERO);
        assert_eq!(left.source_range.end, right_clip.source_range.start);
        assert_eq!(
            right_clip.source_range.end,
            nw_domain::time::Rational::new(1, 1).unwrap()
        );
        assert_eq!(left.end().unwrap(), right_clip.sequence_start);
        assert_ne!(left.occurrence_id, right_clip.occurrence_id);
        for reference in right.references.iter().filter(|r| r.role == "link") {
            assert!(
                right_ids.contains(&reference.target),
                "Right split edge must target the right closure"
            );
        }
    }
    // Remove explicit right edges after proving their remapping: the following
    // move/lift must now expand the shared group index on its own.
    let unchanged_right = right_entities
        .iter()
        .find(|entity| {
            !entity
                .references
                .iter()
                .any(|reference| reference.role == "link")
        })
        .unwrap();
    let unchanged_id = unchanged_right.id.clone();
    let unchanged_bytes = fixture.raw(&unchanged_id);
    // Only the source of the one-way edge changes; its target is already group-only.
    let grouped_only = right_entities
        .into_iter()
        .filter(|entity| {
            entity
                .references
                .iter()
                .any(|reference| reference.role == "link")
        })
        .map(|mut entity| {
            entity
                .references
                .retain(|reference| reference.role != "link");
            entity
        })
        .collect::<Vec<_>>();
    assert_eq!(
        grouped_only.len(),
        1,
        "Exactly the one-way explicit edge must be removed"
    );
    fixture.install_envelopes(grouped_only);
    assert_eq!(fixture.raw(&unchanged_id), unchanged_bytes);
    for id in &right_ids {
        let entity = fixture.entity(id);
        assert!(
            !entity
                .references
                .iter()
                .any(|reference| reference.role == "link")
        );
        assert_eq!(
            entity
                .payload::<Clip>("clip")
                .unwrap()
                .link_group_id
                .as_ref(),
            Some(&right_group)
        );
    }
    let left_bytes = [
        fixture.raw(&fixture.clips[0]),
        fixture.raw(&fixture.clips[2]),
    ];
    let moved=fixture.accept(fixture.request("clips.move","right-group-move",json!({"clip_ids":[right_ids[0]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"}))).await;
    assert_eq!(moved["changed_ids"].as_array().unwrap().len(), 2);
    assert_eq!(
        [
            fixture.raw(&fixture.clips[0]),
            fixture.raw(&fixture.clips[2])
        ],
        left_bytes
    );
    let right_before = right_ids
        .iter()
        .map(|id| fixture.raw(id))
        .collect::<Vec<_>>();
    fixture
        .accept(fixture.request(
            "clips.remove",
            "right-group-lift",
            json!({"clip_ids":[right_ids[0]],"mode":"lift","link_policy":"expand"}),
        ))
        .await;
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    for id in &right_ids {
        assert!(read.raw_entity(id).unwrap().is_none());
    }
    for id in &fixture.sources {
        assert!(read.raw_entity(id).unwrap().is_some());
    }
    drop(read);
    fixture
        .accept(fixture.request("history.undo", "right-group-restore", json!({})))
        .await;
    for (id, before) in right_ids.iter().zip(right_before) {
        assert_eq!(fixture.raw(id), before);
    }

    let different = Fixture::new().await;
    let mut first = different.entity(&different.clips[0]);
    let mut second = different.entity(&different.clips[2]);
    first.references.push(EntityReference {
        role: "link".into(),
        target: second.id.clone(),
    });
    let mut shorter: Clip = second.payload("clip").unwrap();
    shorter.duration = nw_domain::time::Rational::new(1, 2).unwrap();
    shorter.source_range.end = shorter.duration;
    second.update(&shorter).unwrap();
    different.install_envelopes(vec![first, second]);
    let incoming = different
        .accept(different.move_request(2, 1, "incoming-edge-without-group"))
        .await;
    assert_eq!(incoming["changed_ids"].as_array().unwrap().len(), 2);
    different.reject_unchanged(different.request("clips.split","linked-non-interior",json!({"clip_id":different.clips[0],"at_sequence":{"num":"1","den":"1"},"link_policy":"expand"})),"VALIDATION_ERROR").await;
}

/// Public topology must persist a real group and expand a move from one seed.
pub(in crate::runtime) async fn public_link_producer() {
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
        request.request_id = key.into();
        request.replay_scope = Some(scope.into());
        request.idempotency_key = Some(key.into());
        request.project = project.map(|(id, revision)| ProjectRef {
            id: id.into(),
            expected_revision: Some(revision.into()),
        });
        service.dispatch(context.clone(), request).await
    }
    fn accepted(response: Response) -> Value {
        assert!(response.ok, "Public-link producer acceptance: {response:?}");
        assert!(response.error.is_none(), "{response:?}");
        Uuid::parse_str(response.receipt_id.as_deref().expect("Durable receipt")).unwrap();
        response.result.unwrap()
    }
    fn clip_facts(entity: &EntityEnvelope, ignore_start: bool) -> Value {
        let mut value = serde_json::to_value(entity).unwrap();
        let mut payload: Value = serde_json::from_str(&entity.payload_json).unwrap();
        payload.as_object_mut().unwrap().remove("link_group_id");
        if ignore_start {
            payload.as_object_mut().unwrap().remove("sequence_start");
        }
        let object = value.as_object_mut().unwrap();
        object.remove("payload_sha256");
        object.insert("payload_json".into(), payload);
        value
    }

    let root = tempfile::tempdir().unwrap();
    let service = Service::open(root.path().join("data"), None).await.unwrap();
    let context = service.owner();
    let scope_response = service
        .dispatch(
            context.clone(),
            Request::query("access.replay_scope.create", json!({})),
        )
        .await;
    assert!(scope_response.ok, "{scope_response:?}");
    assert!(scope_response.receipt_id.is_none(), "{scope_response:?}");
    let scope = scope_response.result.unwrap()["replay_scope"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = accepted(
        write(
            &service,
            &context,
            &scope,
            None,
            "projects.create",
            "public-link-red-create",
            json!({"name":"Public two-solid link producer"}),
        )
        .await,
    );
    let project = ProjectId::parse(created["project_id"].as_str().unwrap()).unwrap();
    let bin = EntityId::parse(created["root_bin_id"].as_str().unwrap()).unwrap();
    assert_eq!(created["revision"], "0");
    let sequence = accepted(
        write(
            &service,
            &context,
            &scope,
            Some((project.as_str(), "0")),
            "sequences.create",
            "public-link-red-sequence",
            json!({
                "name":"Two video tracks",
                "settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
                "tracks":[{"kind":"video","name":"First solid"},{"kind":"video","name":"Second solid"}]
            }),
        )
        .await,
    );
    assert_eq!(sequence["base_revision"], "0");
    assert_eq!(sequence["revision"], "1");
    let sequence_id =
        EntityId::parse(sequence["created"]["sequence_id"].as_str().unwrap()).unwrap();
    let tracks = sequence["created"]["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 2);
    let track_ids = tracks
        .iter()
        .map(|track| {
            assert_eq!(track["kind"], "video");
            EntityId::parse(track["id"].as_str().unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
    let mut clip_ids = Vec::new();
    let mut source_ids = Vec::new();
    for (index, revision, next_revision, source_range, sequence_start) in [
        (
            0usize,
            "1",
            "2",
            json!({"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}}),
            json!({"num":"0","den":"1"}),
        ),
        (
            1usize,
            "2",
            "3",
            json!({"start":{"num":"1","den":"1"},"end":{"num":"5","den":"2"}}),
            json!({"num":"1","den":"4"}),
        ),
    ] {
        let placed = accepted(
            write(
                &service,
                &context,
                &scope,
                Some((project.as_str(), revision)),
                "clips.place",
                &format!("public-link-red-place-{index}"),
                json!({
                    "sequence_id":sequence_id,"track_id":track_ids[index],
                    "source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},
                    "source_range":source_range,"sequence_start":sequence_start,"collision":"reject"
                }),
            )
            .await,
        );
        assert_eq!(placed["base_revision"], revision);
        assert_eq!(placed["revision"], next_revision);
        assert_eq!(placed["created"]["clip_ids"].as_array().unwrap().len(), 1);
        clip_ids.push(EntityId::parse(placed["created"]["clip_ids"][0].as_str().unwrap()).unwrap());
        source_ids.push(EntityId::parse(placed["created"]["source_id"].as_str().unwrap()).unwrap());
    }
    let mut expected_members = clip_ids.clone();
    expected_members.sort();
    let mut identities = vec![
        project.to_string(),
        bin.to_string(),
        sequence_id.to_string(),
    ];
    identities.extend(
        track_ids
            .iter()
            .chain(&source_ids)
            .chain(&clip_ids)
            .map(ToString::to_string),
    );
    let (before_clips, before_nonclips) = {
        let read = service.store.project_read(project.as_str()).unwrap();
        assert_eq!(read.header().revision, Revision(3));
        assert_eq!(read.header().entity_count, 8);
        assert_eq!(read.header().sequence_count, 1);
        assert_eq!(
            read.entity(&bin)
                .unwrap()
                .payload::<Bin>("bin")
                .unwrap()
                .member_count,
            3
        );
        assert_eq!(
            read.entity(&sequence_id)
                .unwrap()
                .payload::<Sequence>("sequence")
                .unwrap()
                .track_count,
            2
        );
        for track_id in &track_ids {
            let track: Track = read.entity(track_id).unwrap().payload("track").unwrap();
            assert_eq!(track.kind, TrackKind::Video);
            assert_eq!(track.grid, nw_domain::time::Rational::new(4, 1).unwrap());
            assert_eq!(track.clip_count, 1);
            assert_eq!(track.sequence_id, sequence_id);
        }
        let clips = clip_ids
            .iter()
            .map(|id| read.entity(id).unwrap())
            .collect::<Vec<_>>();
        for (index, entity) in clips.iter().enumerate() {
            let clip: Clip = entity.payload("clip").unwrap();
            identities.push(clip.occurrence_id.to_string());
            assert_eq!(clip.link_group_id, None);
            assert!(
                !entity
                    .references
                    .iter()
                    .any(|reference| reference.role == "link")
            );
            assert_eq!(entity.owner_id.as_ref(), Some(&track_ids[index]));
            assert_eq!(clip.sequence_id, sequence_id);
            assert_eq!(clip.track_id, track_ids[index]);
            assert_eq!(clip.source_id, source_ids[index]);
            assert_eq!(entity.references.len(), 2);
            assert!(entity.references.iter().any(
                |reference| reference.role == "source" && reference.target == source_ids[index]
            ));
            assert!(
                entity
                    .references
                    .iter()
                    .any(|reference| reference.role == "ownership"
                        && reference.target == track_ids[index])
            );
            assert_eq!(clip.rate, nw_domain::time::Rational::new(1, 1).unwrap());
            assert!(!clip.locked);
        }
        assert_eq!(
            serde_json::to_value(clips[0].payload::<Clip>("clip").unwrap()).unwrap()["source_range"],
            json!({"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}})
        );
        assert_eq!(
            serde_json::to_value(clips[1].payload::<Clip>("clip").unwrap()).unwrap()["source_range"],
            json!({"start":{"num":"1","den":"1"},"end":{"num":"5","den":"2"}})
        );
        let first: Clip = clips[0].payload("clip").unwrap();
        let second: Clip = clips[1].payload("clip").unwrap();
        assert_eq!(
            first.sequence_start,
            nw_domain::time::Rational::new(0, 1).unwrap()
        );
        assert_eq!(
            first.duration,
            nw_domain::time::Rational::new(2, 1).unwrap()
        );
        assert_eq!(
            first.end().unwrap(),
            nw_domain::time::Rational::new(2, 1).unwrap()
        );
        assert_eq!(
            second.sequence_start,
            nw_domain::time::Rational::new(1, 4).unwrap()
        );
        assert_eq!(
            second.duration,
            nw_domain::time::Rational::new(3, 2).unwrap()
        );
        assert_eq!(
            second.end().unwrap(),
            nw_domain::time::Rational::new(7, 4).unwrap()
        );
        let nonclips = std::iter::once(&bin)
            .chain(std::iter::once(&sequence_id))
            .chain(&track_ids)
            .chain(&source_ids)
            .map(|id| (id.clone(), read.raw_entity(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        (clips, nonclips)
    };
    identities.sort();
    identities.dedup();
    assert_eq!(
        identities.len(),
        11,
        "Project, eight entities and two occurrences must be distinct"
    );

    // Link and Unlink discovery must describe the callable shared producers.
    let listed = service
        .dispatch(context.clone(), Request::query("commands.list", json!({})))
        .await;
    assert!(listed.ok, "{listed:?}");
    assert!(listed.receipt_id.is_none(), "{listed:?}");
    let listing = listed
        .result
        .as_ref()
        .unwrap_or_else(|| panic!("{listed:?}"));
    assert!(listing.is_object(), "{listed:?}");
    assert_eq!(listing["release"], "development_only", "{listed:?}");
    assert_eq!(listing["full_release"], "blocked", "{listed:?}");
    let descriptors = listing["commands"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed:?}"));
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor["id"] == "clips.link")
        .unwrap_or_else(|| panic!("{listed:?}"));
    assert_eq!(descriptor["version"], 1, "{listed:?}");
    assert_eq!(descriptor["state_class"], "durable_mutation", "{listed:?}");
    assert_eq!(
        descriptor["project_context"], "required_write",
        "{listed:?}"
    );
    assert_eq!(
        descriptor["params_schema"]["additionalProperties"], false,
        "{listed:?}"
    );
    let required = descriptor["params_schema"]["required"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed:?}"));
    assert_eq!(required.len(), 2, "{listed:?}");
    assert!(
        required.contains(&json!("clip_ids")) && required.contains(&json!("link_policy")),
        "{listed:?}"
    );
    let clip_schema = &descriptor["params_schema"]["properties"]["clip_ids"];
    assert_eq!(clip_schema["minItems"], 1, "{listed:?}");
    assert_eq!(clip_schema["maxItems"], 128, "{listed:?}");
    assert_eq!(clip_schema["uniqueItems"], true, "{listed:?}");
    assert_eq!(
        descriptor["params_schema"]["$defs"]["LinkPolicy"]["enum"],
        json!(["expand"]),
        "{listed:?}"
    );
    let registered = commands::descriptor("clips.link").unwrap_or_else(|| panic!("{listed:?}"));
    assert_eq!(
        serde_json::to_value(registered).unwrap(),
        *descriptor,
        "{listed:?}"
    );
    assert_eq!(
        commands::registry()
            .iter()
            .filter(|row| row.id == "clips.link")
            .count(),
        1,
        "{listed:?}"
    );
    let schema = service
        .dispatch(
            context.clone(),
            Request::query("commands.schema", json!({"id":"clips.link"})),
        )
        .await;
    assert!(
        schema.ok && schema.error.is_none() && schema.receipt_id.is_none(),
        "{schema:?}"
    );
    assert_eq!(schema.result.as_ref(), Some(descriptor), "{schema:?}");
    {
        let command = "clips.unlink";
        let descriptor = descriptors
            .iter()
            .find(|row| row["id"] == command)
            .unwrap_or_else(|| panic!("{listed:?}"));
        assert_eq!(descriptor["version"], 1, "{listed:?}");
        assert_eq!(descriptor["descriptor_schema_version"], 2, "{listed:?}");
        assert!(descriptor.get("result_schema").is_none(), "{listed:?}");
        assert_eq!(descriptor["state_class"], "durable_mutation", "{listed:?}");
        assert_eq!(
            descriptor["project_context"], "required_write",
            "{listed:?}"
        );
        let params = &descriptor["params_schema"];
        assert_eq!(params["additionalProperties"], false, "{listed:?}");
        let required = params["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{listed:?}"));
        assert_eq!(required.len(), 2, "{listed:?}");
        assert!(
            required.contains(&json!("clip_ids")) && required.contains(&json!("link_policy")),
            "{listed:?}"
        );
        assert_eq!(
            params["properties"]["clip_ids"]["minItems"], 1,
            "{listed:?}"
        );
        assert_eq!(
            params["properties"]["clip_ids"]["maxItems"], 128,
            "{listed:?}"
        );
        assert_eq!(
            params["properties"]["clip_ids"]["uniqueItems"], true,
            "{listed:?}"
        );
        assert_eq!(
            params["$defs"]["LinkPolicy"]["enum"],
            json!(["expand"]),
            "{listed:?}"
        );
        assert!(commands::ACTIVE_COMMANDS.contains(&command), "{listed:?}");
        let registered = commands::descriptor(command).unwrap_or_else(|| panic!("{listed:?}"));
        assert_eq!(
            serde_json::to_value(registered).unwrap(),
            *descriptor,
            "{listed:?}"
        );
        assert_eq!(
            commands::registry()
                .iter()
                .filter(|row| row.id == command)
                .count(),
            1,
            "{listed:?}"
        );
        let schema = service
            .dispatch(
                context.clone(),
                Request::query("commands.schema", json!({"id":command})),
            )
            .await;
        assert!(
            schema.ok && schema.error.is_none() && schema.receipt_id.is_none(),
            "{schema:?}"
        );
        assert_eq!(schema.result.as_ref(), Some(descriptor), "{schema:?}");
    }

    // The prior hosted A failure was this real Link request's UNKNOWN_COMMAND.
    let linked = accepted(
        write(
            &service,
            &context,
            &scope,
            Some((project.as_str(), "3")),
            "clips.link",
            "public-link-red-link",
            json!({"clip_ids":clip_ids,"link_policy":"expand"}),
        )
        .await,
    );
    assert_eq!(linked["project_id"], project.as_str());
    assert_eq!(linked["base_revision"], "3");
    assert_eq!(linked["revision"], "4");
    OperationId::parse(linked["operation_id"].as_str().unwrap()).unwrap();
    assert!(linked.get("no_change").is_none());
    let group = EntityId::parse(linked["created"]["link_group_id"].as_str().unwrap()).unwrap();
    assert!(!identities.contains(&group.to_string()));
    assert_eq!(
        linked["created"],
        json!({"link_group_id":group,"clip_ids":expected_members})
    );
    assert_eq!(linked["changed_ids"], json!(expected_members));
    {
        let read = service.store.project_read(project.as_str()).unwrap();
        assert_eq!(read.header().revision, Revision(4));
        assert_eq!(read.header().entity_count, 8);
        assert!(read.raw_entity(&group).unwrap().is_none());
        assert_eq!(read.dependents(&group, 128).unwrap(), expected_members);
        for before in &before_clips {
            let after = read.entity(&before.id).unwrap();
            let clip: Clip = after.payload("clip").unwrap();
            assert_eq!(clip.link_group_id.as_ref(), Some(&group));
            assert_eq!(clip_facts(&after, false), clip_facts(before, false));
            assert!(
                !after
                    .references
                    .iter()
                    .any(|reference| reference.role == "link")
            );
        }
        for (id, raw) in &before_nonclips {
            assert_eq!(read.raw_entity(id).unwrap().as_ref(), Some(raw));
        }
    }
    let moved = accepted(
        write(
            &service,
            &context,
            &scope,
            Some((project.as_str(), "4")),
            "clips.move",
            "public-link-red-single-seed-move",
            json!({"clip_ids":[clip_ids[0]],"delta":{"num":"1","den":"4"},"collision":"reject","link_policy":"expand"}),
        )
        .await,
    );
    assert_eq!(moved["project_id"], project.as_str());
    assert_eq!(moved["base_revision"], "4");
    assert_eq!(moved["revision"], "5");
    assert_eq!(moved["changed_ids"], json!(expected_members));
    OperationId::parse(moved["operation_id"].as_str().unwrap()).unwrap();
    assert!(moved.get("no_change").is_none());
    let read = service.store.project_read(project.as_str()).unwrap();
    assert_eq!(read.header().revision, Revision(5));
    assert_eq!(read.header().entity_count, 8);
    assert!(read.raw_entity(&group).unwrap().is_none());
    assert_eq!(read.dependents(&group, 128).unwrap(), expected_members);
    for before in &before_clips {
        let after = read.entity(&before.id).unwrap();
        assert_eq!(
            after
                .payload::<Clip>("clip")
                .unwrap()
                .link_group_id
                .as_ref(),
            Some(&group)
        );
        assert_eq!(clip_facts(&after, true), clip_facts(before, true));
    }
    let first: Clip = read.entity(&clip_ids[0]).unwrap().payload("clip").unwrap();
    let second: Clip = read.entity(&clip_ids[1]).unwrap().payload("clip").unwrap();
    assert_eq!(
        first.sequence_start,
        nw_domain::time::Rational::new(1, 4).unwrap()
    );
    assert_eq!(
        first.end().unwrap(),
        nw_domain::time::Rational::new(9, 4).unwrap()
    );
    assert_eq!(
        second.sequence_start,
        nw_domain::time::Rational::new(1, 2).unwrap()
    );
    assert_eq!(
        second.end().unwrap(),
        nw_domain::time::Rational::new(2, 1).unwrap()
    );
    assert_eq!(
        second
            .sequence_start
            .checked_sub(first.sequence_start)
            .unwrap(),
        nw_domain::time::Rational::new(1, 4).unwrap()
    );
    for (id, raw) in &before_nonclips {
        assert_eq!(read.raw_entity(id).unwrap().as_ref(), Some(raw));
    }
    let moved_clips = clip_ids
        .iter()
        .map(|id| read.entity(id).unwrap())
        .collect::<Vec<_>>();
    drop(read);
    public_link_edges().await;

    let before_unlink = unlink_images(&service, &project, &clip_ids, std::slice::from_ref(&group));
    // Actual hosted missing-handler RED reached this public request after every Link oracle.
    let unlinked = accepted(
        write(
            &service,
            &context,
            &scope,
            Some((project.as_str(), "5")),
            "clips.unlink",
            "public-link-red-unlink",
            json!({"clip_ids":[clip_ids[1]],"link_policy":"expand"}),
        )
        .await,
    );
    assert_eq!(unlinked["project_id"], project.as_str());
    assert_eq!(unlinked["base_revision"], "5");
    assert_eq!(unlinked["revision"], "6");
    OperationId::parse(unlinked["operation_id"].as_str().unwrap()).unwrap();
    assert!(unlinked.get("no_change").is_none());
    assert_eq!(unlinked["created"], json!({"clip_ids":expected_members}));
    assert_eq!(unlinked["changed_ids"], json!(expected_members));
    let read = service.store.project_read(project.as_str()).unwrap();
    assert_eq!(read.header().revision, Revision(6));
    assert_eq!(read.header().entity_count, 8);
    assert!(read.raw_entity(&group).unwrap().is_none());
    assert!(read.dependents(&group, 128).unwrap().is_empty());
    for before in &moved_clips {
        let after = read.entity(&before.id).unwrap();
        assert_eq!(after.payload::<Clip>("clip").unwrap().link_group_id, None);
        assert_eq!(clip_facts(&after, false), clip_facts(before, false));
        assert!(
            !after
                .references
                .iter()
                .any(|reference| reference.role == "link")
        );
    }
    for (id, raw) in &before_nonclips {
        assert_eq!(read.raw_entity(id).unwrap().as_ref(), Some(raw));
    }
    drop(read);
    let after_unlink = unlink_images(&service, &project, &clip_ids, std::slice::from_ref(&group));
    let undone = accepted(
        write(
            &service,
            &context,
            &scope,
            Some((project.as_str(), "6")),
            "history.undo",
            "public-unlink-undo",
            json!({}),
        )
        .await,
    );
    assert_eq!(undone["base_revision"], "6");
    assert_eq!(undone["revision"], "7");
    assert_eq!(
        unlink_images(&service, &project, &clip_ids, std::slice::from_ref(&group)),
        before_unlink
    );
    let restored = service.store.project_read(project.as_str()).unwrap();
    assert_eq!(restored.header().entity_count, 8);
    assert_eq!(restored.dependents(&group, 128).unwrap(), expected_members);
    for id in &clip_ids {
        assert_eq!(
            restored
                .entity(id)
                .unwrap()
                .payload::<Clip>("clip")
                .unwrap()
                .link_group_id
                .as_ref(),
            Some(&group)
        );
    }
    drop(restored);
    let redone = accepted(
        write(
            &service,
            &context,
            &scope,
            Some((project.as_str(), "7")),
            "history.redo",
            "public-unlink-redo",
            json!({}),
        )
        .await,
    );
    assert_eq!(redone["base_revision"], "7");
    assert_eq!(redone["revision"], "8");
    assert_eq!(
        unlink_images(&service, &project, &clip_ids, std::slice::from_ref(&group)),
        after_unlink
    );
    assert_eq!(
        service
            .store
            .project_read(project.as_str())
            .unwrap()
            .header()
            .entity_count,
        8
    );
    public_unlink_edges().await;
    linked_split_right_group_collision().await;
}

// Public validation and real-store deterministic allocation models, after A's literal oracles.
async fn public_link_edges() {
    let fixture = Fixture::new().await;
    for (key, seeds) in [
        ("empty", vec![]),
        (
            "duplicate",
            vec![fixture.clips[0].clone(), fixture.clips[0].clone()],
        ),
        ("singleton", vec![fixture.clips[0].clone()]),
        (
            "cross-sequence",
            vec![fixture.clips[0].clone(), fixture.clips[3].clone()],
        ),
    ] {
        fixture
            .reject_unchanged(
                fixture.request(
                    "clips.link",
                    &format!("link-{key}"),
                    json!({"clip_ids":seeds,"link_policy":"expand"}),
                ),
                "VALIDATION_ERROR",
            )
            .await;
    }
    fixture
        .reject_unchanged(
            fixture.request(
                "clips.link",
                "link-too-many",
                json!({"clip_ids":vec![fixture.clips[0].clone();129],"link_policy":"expand"}),
            ),
            "RESOURCE_LIMIT",
        )
        .await;
    for (key, params) in [
        (
            "policy",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[2]],"link_policy":"ignore"}),
        ),
        (
            "field",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[2]],"link_policy":"expand","group":"caller-group"}),
        ),
    ] {
        fixture
            .reject_unchanged(
                fixture.request("clips.link", &format!("link-{key}"), params),
                "VALIDATION_ERROR",
            )
            .await;
    }
    let linked = fixture
        .accept(fixture.request(
            "clips.link",
            "link-model-group",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[2]],"link_policy":"expand"}),
        ))
        .await;
    let group = EntityId::parse(linked["created"]["link_group_id"].as_str().unwrap()).unwrap();
    let mut members = vec![fixture.clips[0].clone(), fixture.clips[2].clone()];
    members.sort();
    assert_eq!(linked["changed_ids"], json!(members));

    // No-op validates a complete group from one seed and preserves project/history bytes.
    let raw = members.iter().map(|id| fixture.raw(id)).collect::<Vec<_>>();
    let history = fixture.history_hash();
    let header = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap()
        .header_raw()
        .to_owned();
    let request = fixture.request(
        "clips.link",
        "link-existing-group",
        json!({"clip_ids":[fixture.clips[2]],"link_policy":"expand"}),
    );
    let response = fixture
        .service
        .dispatch(fixture.context.clone(), request.clone())
        .await;
    assert!(response.ok && response.receipt_id.is_some(), "{response:?}");
    let result = response.result.as_ref().unwrap();
    assert_eq!(result["no_change"], true);
    assert_eq!(result["revision"], linked["revision"]);
    assert_eq!(result["base_revision"], linked["revision"]);
    assert_eq!(result["changed_ids"], json!([]));
    assert_eq!(
        result["created"],
        json!({"link_group_id":group,"clip_ids":members})
    );
    assert_eq!(fixture.history_hash(), history);
    assert_eq!(
        fixture
            .service
            .store
            .project_read(fixture.project.as_str())
            .unwrap()
            .header_raw(),
        header
    );
    assert_eq!(
        members.iter().map(|id| fixture.raw(id)).collect::<Vec<_>>(),
        raw
    );
    let replay = fixture
        .service
        .dispatch(fixture.context.clone(), request)
        .await;
    assert_eq!(replay.receipt_id, response.receipt_id);
    assert_eq!(replay.result, response.result);

    // Add an unlinked partner to clip 1 by public placement; A's project is untouched.
    let clip: Clip = fixture.entity(&fixture.clips[1]).payload("clip").unwrap();
    let placed = fixture
        .accept(fixture.request(
            "clips.place",
            "link-model-extra",
            json!({
                "sequence_id":clip.sequence_id,"track_id":clip.track_id,
                "source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},
                "source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
                "sequence_start":{"num":"6","den":"1"},"collision":"reject"
            }),
        ))
        .await;
    let extra = EntityId::parse(placed["created"]["clip_ids"][0].as_str().unwrap()).unwrap();
    let seeds = vec![fixture.clips[1].clone(), extra.clone()];
    let intent = || {
        ProjectEditIntent::Link(
            commands::parse(json!({"clip_ids":seeds,"link_policy":"expand"})).unwrap(),
        )
    };
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    assert!(read.raw_entity(&group).unwrap().is_none());
    assert_eq!(read.dependents(&group, 128).unwrap(), members);
    assert_eq!(
        read.dependents(&group, 1).unwrap_err().code,
        "RESOURCE_LIMIT"
    );
    assert!(read.has_dependency_target(&group).unwrap());
    let unchanged = resolve_edit(
        &read,
        ProjectEditIntent::Link(
            commands::parse(json!({"clip_ids":[members[0]],"link_policy":"expand"})).unwrap(),
        ),
        AllocatedIds(VecDeque::new()),
    )
    .unwrap();
    assert!(unchanged.forward.changes.is_empty() && unchanged.inverse.changes.is_empty());
    assert_eq!(
        unchanged.created,
        json!({"link_group_id":group,"clip_ids":members})
    );
    let header = read.header_raw().to_owned();
    let before = seeds
        .iter()
        .map(|id| read.raw_entity(id).unwrap().unwrap())
        .collect::<Vec<_>>();
    for candidate in [read.header().root_bin_id.clone(), group.clone()] {
        let error = resolve_edit(&read, intent(), AllocatedIds(VecDeque::from([candidate])))
            .expect_err("Occupied identity must reject the single candidate");
        assert_eq!(error.code, "OBJECT_CONFLICT");
    }
    let free = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
    assert!(!read.has_dependency_target(&free).unwrap());
    assert!(read.raw_entity(&free).unwrap().is_none());
    let resolved = resolve_edit(
        &read,
        intent(),
        AllocatedIds(VecDeque::from([free.clone()])),
    )
    .unwrap();
    let mut sorted_seeds = seeds.clone();
    sorted_seeds.sort();
    assert_eq!(resolved.changed_ids, sorted_seeds);
    assert_eq!(
        resolved.created,
        json!({"link_group_id":free,"clip_ids":sorted_seeds})
    );
    assert_eq!(resolved.forward.changes.len(), 2);
    for change in &resolved.forward.changes {
        let after: EntityEnvelope = serde_json::from_str(change.after.as_ref().unwrap()).unwrap();
        assert_eq!(
            after
                .payload::<Clip>("clip")
                .unwrap()
                .link_group_id
                .as_ref(),
            Some(&free)
        );
        assert_eq!(change.before, read.raw_entity(&change.id).unwrap());
    }
    assert_eq!(read.header_raw(), header);
    assert_eq!(
        seeds
            .iter()
            .map(|id| read.raw_entity(id).unwrap().unwrap())
            .collect::<Vec<_>>(),
        before
    );
    drop(read);

    // Injected MODEL only: a known nonclip semantic_input occupies the same target namespace.
    let occupied = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
    let mut source = fixture.entity(&fixture.sources[3]);
    source.references.push(EntityReference {
        role: "semantic_input".into(),
        target: occupied.clone(),
    });
    fixture.install_future_envelope(source);
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    assert!(read.raw_entity(&occupied).unwrap().is_none());
    assert_eq!(
        read.dependents(&occupied, 128).unwrap(),
        vec![fixture.sources[3].clone()]
    );
    assert!(read.has_dependency_target(&occupied).unwrap());
    assert_eq!(
        resolve_edit(&read, intent(), AllocatedIds(VecDeque::from([occupied])))
            .expect_err("Any dependency role occupies a candidate")
            .code,
        "OBJECT_CONFLICT"
    );
    drop(read);

    // Injected relations exercise strict Link behavior without replacing legacy linked fixtures.
    for relation in [
        "incoming-nonclip",
        "outgoing-nonclip",
        "semantic-input",
        "incoming-clip",
    ] {
        let fixture = Fixture::new().await;
        let (id, target, role) = match relation {
            "incoming-nonclip" => (&fixture.sources[3], &fixture.clips[0], "link"),
            "outgoing-nonclip" => (&fixture.clips[0], &fixture.sources[3], "link"),
            "semantic-input" => (&fixture.sources[3], &fixture.clips[0], "semantic_input"),
            _ => (&fixture.clips[2], &fixture.clips[0], "link"),
        };
        let mut entity = fixture.entity(id);
        entity.references.push(EntityReference {
            role: role.into(),
            target: target.clone(),
        });
        fixture.install_future_envelope(entity);
        let seeds = if relation == "incoming-clip" {
            vec![fixture.clips[0].clone()]
        } else {
            vec![fixture.clips[0].clone(), fixture.clips[2].clone()]
        };
        let request = fixture.request(
            "clips.link",
            relation,
            json!({"clip_ids":seeds,"link_policy":"expand"}),
        );
        if relation.ends_with("nonclip") {
            fixture.reject_unchanged(request, "VALIDATION_ERROR").await;
        } else {
            let result = fixture.accept(request).await;
            let mut expected = vec![fixture.clips[0].clone(), fixture.clips[2].clone()];
            expected.sort();
            assert_eq!(result["changed_ids"], json!(expected));
            assert!(
                fixture
                    .entity(id)
                    .references
                    .iter()
                    .any(|r| r.role == role && &r.target == target)
            );
        }
    }
}

// Metadata/model evidence only: compare exact entity bytes and real derived index values.
fn unlink_images(
    service: &Arc<Service>,
    project: &ProjectId,
    clips: &[EntityId],
    groups: &[EntityId],
) -> Value {
    let read = service.store.project_read(project.as_str()).unwrap();
    let mut entities = BTreeMap::new();
    let mut targets = std::collections::BTreeSet::new();
    targets.extend(groups.iter().cloned());
    let mut keys = Vec::new();
    for id in clips {
        let entity = read.entity(id).unwrap();
        let clip: Clip = entity.payload("clip").unwrap();
        let track: Track = read
            .entity(&clip.track_id)
            .unwrap()
            .payload("track")
            .unwrap();
        let tick = grid_tick(clip.sequence_start, track.grid).unwrap();
        keys.push(format!(
            "project-clip/{project}/{}/{}/{tick:016x}/{id}",
            clip.sequence_id, clip.track_id
        ));
        targets.extend([
            id.clone(),
            clip.source_id.clone(),
            clip.track_id.clone(),
            clip.sequence_id.clone(),
        ]);
    }
    targets.insert(read.header().root_bin_id.clone());
    for id in &targets {
        if let Some(raw) = read.raw_entity(id).unwrap() {
            entities.insert(id.to_string(), raw);
        }
    }
    let mut dependencies = BTreeMap::new();
    for target in &targets {
        let members = read.dependents(target, 256).unwrap();
        for member in &members {
            keys.push(format!("project-dependency/{project}/{target}/{member}"));
        }
        dependencies.insert(target.to_string(), members);
    }
    let indexes = keys
        .into_iter()
        .map(|key| {
            let hash = service.store.project_fact_hash(&key).unwrap();
            (key, hash)
        })
        .collect::<BTreeMap<_, _>>();
    json!({"entities":entities,"dependencies":dependencies,"indexes":indexes})
}

fn unlink_unchanged_facts(fixture: &Fixture, groups: &[EntityId]) -> Value {
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    let history = read.history().unwrap();
    let mut keys = history
        .undo
        .iter()
        .chain(&history.redo)
        .map(|entry| format!("project-patch/{}/{}", fixture.project, entry.id))
        .collect::<Vec<_>>();
    keys.extend(
        fixture
            .sources
            .iter()
            .map(|source| format!("project-reference/{}/{source}", fixture.project)),
    );
    let retained = keys
        .into_iter()
        .map(|key| {
            let hash = fixture.service.store.project_fact_hash(&key).unwrap();
            (key, hash)
        })
        .collect::<BTreeMap<_, _>>();
    json!({"header":read.header_raw(),"history":fixture.history_hash(),"retained":retained,
        "images":unlink_images(&fixture.service, &fixture.project, &fixture.clips, groups)})
}

fn install_unlink_raw_clip(fixture: &Fixture, index: usize) {
    let mut entity = fixture.entity(&fixture.clips[index]);
    entity.payload_json = format!(
        "{{ \"future_number\": 9007199254740993, \"future_escape\":\"\\u0061\", {} }}",
        &entity.payload_json[1..entity.payload_json.len() - 1]
    );
    entity.payload_sha256 = domain::hash(entity.payload_json.as_bytes());
    entity.extensions.insert(
        "future_optional".into(),
        serde_json::from_str("[9007199254740993,\"\\u0062\"]").unwrap(),
    );
    fixture.install_future_envelope(entity);
}

async fn public_unlink_edges() {
    let fixture = Fixture::new().await;
    for (key, params, code) in [
        (
            "empty",
            json!({"clip_ids":[],"link_policy":"expand"}),
            "VALIDATION_ERROR",
        ),
        (
            "duplicate",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[0]],"link_policy":"expand"}),
            "VALIDATION_ERROR",
        ),
        (
            "oversized",
            json!({"clip_ids":vec![fixture.clips[0].clone();129],"link_policy":"expand"}),
            "RESOURCE_LIMIT",
        ),
        (
            "cross-sequence",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[3]],"link_policy":"expand"}),
            "VALIDATION_ERROR",
        ),
        (
            "policy",
            json!({"clip_ids":[fixture.clips[0]],"link_policy":"ignore"}),
            "VALIDATION_ERROR",
        ),
        (
            "missing-policy",
            json!({"clip_ids":[fixture.clips[0]]}),
            "VALIDATION_ERROR",
        ),
        (
            "unknown-field",
            json!({"clip_ids":[fixture.clips[0]],"link_policy":"expand","group":"supplied"}),
            "VALIDATION_ERROR",
        ),
    ] {
        let before = unlink_unchanged_facts(&fixture, &[]);
        fixture
            .reject_unchanged(
                fixture.request("clips.unlink", &format!("unlink-{key}"), params),
                code,
            )
            .await;
        assert_eq!(unlink_unchanged_facts(&fixture, &[]), before);
    }
    public_unlink_mixed().await;
    public_unlink_noop().await;
    public_unlink_relations().await;
}

async fn public_unlink_mixed() {
    let fixture = Fixture::new().await;
    // A/B are grouped by the public producer; C's deliberately noncanonical bytes are admitted.
    install_unlink_raw_clip(&fixture, 0);
    install_unlink_raw_clip(&fixture, 1);
    let c_raw = fixture.raw(&fixture.clips[1]);
    let linked = fixture
        .accept(fixture.request(
            "clips.link",
            "unlink-mixed-link",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[2]],"link_policy":"expand"}),
        ))
        .await;
    let group = EntityId::parse(linked["created"]["link_group_id"].as_str().unwrap()).unwrap();
    let mut members = vec![
        fixture.clips[0].clone(),
        fixture.clips[1].clone(),
        fixture.clips[2].clone(),
    ];
    members.sort();
    let mut changing = vec![fixture.clips[0].clone(), fixture.clips[2].clone()];
    changing.sort();
    let params = json!({"clip_ids":[fixture.clips[0],fixture.clips[1]],"link_policy":"expand"});
    // The untouched member must still receive every compatibility/lock/reference check.
    for (case, code) in [
        ("locked", "ENTITY_LOCKED"),
        ("unsupported", "PROJECT_READ_ONLY"),
        ("required", "OBJECT_NOT_FOUND"),
    ] {
        let mut c: EntityEnvelope = serde_json::from_str(&c_raw).unwrap();
        match case {
            "locked" => {
                let mut clip: Clip = c.payload("clip").unwrap();
                clip.locked = true;
                c.update(&clip).unwrap();
            }
            "unsupported" => c.semantic_scope = "future_scope".into(),
            _ => c.references.push(EntityReference {
                role: "semantic_input".into(),
                target: EntityId::parse(Uuid::new_v4().to_string()).unwrap(),
            }),
        }
        fixture.install_future_envelope(c);
        let before = unlink_unchanged_facts(&fixture, std::slice::from_ref(&group));
        fixture
            .reject_unchanged(
                fixture.request(
                    "clips.unlink",
                    &format!("unlink-mixed-{case}"),
                    params.clone(),
                ),
                code,
            )
            .await;
        assert_eq!(
            unlink_unchanged_facts(&fixture, std::slice::from_ref(&group)),
            before
        );
        fixture.install_future_envelope(serde_json::from_str(&c_raw).unwrap());
        assert_eq!(fixture.raw(&fixture.clips[1]), c_raw);
    }
    let before = unlink_images(
        &fixture.service,
        &fixture.project,
        &fixture.clips,
        std::slice::from_ref(&group),
    );
    let source_raw = fixture
        .sources
        .iter()
        .map(|id| fixture.raw(id))
        .collect::<Vec<_>>();
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    let entity_count = read.header().entity_count;
    let resolved = resolve_edit(
        &read,
        ProjectEditIntent::Unlink(commands::parse(params.clone()).unwrap()),
        AllocatedIds(VecDeque::new()),
    )
    .unwrap();
    assert_eq!(resolved.created, json!({"clip_ids":members}));
    assert_eq!(resolved.changed_ids, changing);
    assert_eq!(resolved.forward.changes.len(), 2);
    assert!(
        !resolved
            .forward
            .changes
            .iter()
            .any(|change| change.id == fixture.clips[1])
    );
    assert_eq!(
        serde_json::to_value(&resolved.inverse).unwrap(),
        serde_json::to_value(resolved.forward.inverse()).unwrap()
    );
    drop(read);
    let result = fixture
        .accept(fixture.request("clips.unlink", "unlink-mixed", params))
        .await;
    assert_eq!(result["created"], json!({"clip_ids":members}));
    assert_eq!(result["changed_ids"], json!(changing));
    assert_eq!(fixture.raw(&fixture.clips[1]), c_raw);
    assert_eq!(
        fixture
            .sources
            .iter()
            .map(|id| fixture.raw(id))
            .collect::<Vec<_>>(),
        source_raw
    );
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    assert_eq!(read.header().entity_count, entity_count);
    assert!(read.raw_entity(&group).unwrap().is_none());
    assert!(read.dependents(&group, 128).unwrap().is_empty());
    for (id, raw) in before["entities"].as_object().unwrap() {
        let id = EntityId::parse(id).unwrap();
        if !changing.contains(&id) {
            assert_eq!(read.raw_entity(&id).unwrap().as_deref(), raw.as_str());
        }
    }
    for id in &changing {
        let old: EntityEnvelope =
            serde_json::from_str(before["entities"][id.to_string()].as_str().unwrap()).unwrap();
        let mut expected: Clip = old.payload("clip").unwrap();
        expected.link_group_id = None;
        let after = read.entity(id).unwrap();
        assert_eq!(
            serde_json::to_value(after.payload::<Clip>("clip").unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&after.references).unwrap(),
            serde_json::to_value(&old.references).unwrap()
        );
        assert_eq!(after.owner_id, old.owner_id);
        if id == &fixture.clips[0] {
            let old_payload: BTreeMap<String, Box<serde_json::value::RawValue>> =
                serde_json::from_str(&old.payload_json).unwrap();
            let new_payload: BTreeMap<String, Box<serde_json::value::RawValue>> =
                serde_json::from_str(&after.payload_json).unwrap();
            for key in ["future_number", "future_escape"] {
                assert_eq!(new_payload[key].get(), old_payload[key].get());
            }
            assert_eq!(
                after.extensions["future_optional"].get(),
                old.extensions["future_optional"].get()
            );
        }
    }
    drop(read);
    let after = unlink_images(
        &fixture.service,
        &fixture.project,
        &fixture.clips,
        std::slice::from_ref(&group),
    );
    let operation = OperationId::parse(result["operation_id"].as_str().unwrap()).unwrap();
    let stored = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap()
        .patch(&operation)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&stored.forward).unwrap(),
        serde_json::to_value(resolved.forward).unwrap()
    );
    fixture
        .accept(fixture.request("history.undo", "unlink-mixed-undo", json!({})))
        .await;
    assert_eq!(
        unlink_images(
            &fixture.service,
            &fixture.project,
            &fixture.clips,
            std::slice::from_ref(&group)
        ),
        before
    );
    fixture
        .accept(fixture.request("history.redo", "unlink-mixed-redo", json!({})))
        .await;
    assert_eq!(
        unlink_images(
            &fixture.service,
            &fixture.project,
            &fixture.clips,
            std::slice::from_ref(&group)
        ),
        after
    );
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    assert_eq!(read.history().unwrap().undo.last().unwrap().id, operation);
    let redone = read.patch(&operation).unwrap();
    assert_eq!(
        serde_json::to_value(redone.forward).unwrap(),
        serde_json::to_value(stored.forward).unwrap()
    );
    assert_eq!(
        serde_json::to_value(redone.inverse).unwrap(),
        serde_json::to_value(stored.inverse).unwrap()
    );
}

async fn public_unlink_noop() {
    for several in [false, true] {
        let fixture = Fixture::new().await;
        install_unlink_raw_clip(&fixture, 1);
        let original = fixture.raw(&fixture.clips[1]);
        fixture
            .accept(fixture.move_request(1, 1, "unlink-noop-move"))
            .await;
        fixture
            .accept(fixture.request("history.undo", "unlink-noop-restore", json!({})))
            .await;
        assert_eq!(fixture.raw(&fixture.clips[1]), original);
        let mut members = if several {
            fixture.clips[..3].to_vec()
        } else {
            vec![fixture.clips[1].clone()]
        };
        members.sort();
        let params = json!({"clip_ids":members,"link_policy":"expand"});
        let read = fixture
            .service
            .store
            .project_read(fixture.project.as_str())
            .unwrap();
        assert_eq!(read.history().unwrap().summary()["can_redo"], true);
        let resolved = resolve_edit(
            &read,
            ProjectEditIntent::Unlink(commands::parse(params.clone()).unwrap()),
            AllocatedIds(VecDeque::new()),
        )
        .unwrap();
        assert!(resolved.forward.changes.is_empty() && resolved.inverse.changes.is_empty());
        assert!(resolved.changed_ids.is_empty());
        assert_eq!(resolved.created, json!({"clip_ids":members}));
        let revision = read.header().revision.to_string();
        drop(read);
        let before = unlink_unchanged_facts(&fixture, &[]);
        let request = fixture.request("clips.unlink", "unlink-raw-noop", params);
        let response = fixture
            .service
            .dispatch(fixture.context.clone(), request.clone())
            .await;
        assert!(response.ok && response.receipt_id.is_some(), "{response:?}");
        let result = response.result.as_ref().unwrap();
        assert_eq!(result["no_change"], true);
        assert_eq!(result["base_revision"], revision);
        assert_eq!(result["revision"], revision);
        assert_eq!(result["changed_ids"], json!([]));
        assert_eq!(result["created"], json!({"clip_ids":members}));
        assert_eq!(result["history"]["can_redo"], true);
        assert_eq!(unlink_unchanged_facts(&fixture, &[]), before);
        let replay = fixture
            .service
            .dispatch(fixture.context.clone(), request)
            .await;
        assert_eq!(replay.receipt_id, response.receipt_id);
        assert_eq!(replay.result, response.result);
        assert_eq!(unlink_unchanged_facts(&fixture, &[]), before);
        for (case, code) in [
            ("locked", "ENTITY_LOCKED"),
            ("unsupported", "PROJECT_READ_ONLY"),
            ("required", "OBJECT_NOT_FOUND"),
        ] {
            let mut c: EntityEnvelope = serde_json::from_str(&original).unwrap();
            match case {
                "locked" => {
                    let mut clip: Clip = c.payload("clip").unwrap();
                    clip.locked = true;
                    c.update(&clip).unwrap();
                }
                "unsupported" => c.semantic_scope = "future_scope".into(),
                _ => c.references.push(EntityReference {
                    role: "semantic_input".into(),
                    target: EntityId::parse(Uuid::new_v4().to_string()).unwrap(),
                }),
            }
            fixture.install_future_envelope(c);
            let before = unlink_unchanged_facts(&fixture, &[]);
            fixture
                .reject_unchanged(
                    fixture.request(
                        "clips.unlink",
                        &format!("unlink-noop-{case}"),
                        json!({"clip_ids":members,"link_policy":"expand"}),
                    ),
                    code,
                )
                .await;
            assert_eq!(unlink_unchanged_facts(&fixture, &[]), before);
            fixture.install_future_envelope(serde_json::from_str(&original).unwrap());
        }
    }
}

async fn public_unlink_relations() {
    // MODEL setup: explicit directed edges use real envelopes and derived transaction indexes.
    for relation in [
        "incoming",
        "outgoing",
        "incoming-nonclip",
        "outgoing-nonclip",
        "semantic-input",
        "multiple-groups",
    ] {
        let fixture = Fixture::new().await;
        let mut groups = Vec::new();
        if relation == "multiple-groups" {
            let clip: Clip = fixture.entity(&fixture.clips[1]).payload("clip").unwrap();
            let placed = fixture
                .accept(fixture.request(
                    "clips.place",
                    "unlink-chain-extra",
                    json!({
                "sequence_id":clip.sequence_id,"track_id":clip.track_id,
                "source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},
                "source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
                "sequence_start":{"num":"6","den":"1"},"collision":"reject"}),
                ))
                .await;
            let extra =
                EntityId::parse(placed["created"]["clip_ids"][0].as_str().unwrap()).unwrap();
            for (key, seeds) in [
                (
                    "first",
                    vec![fixture.clips[0].clone(), fixture.clips[2].clone()],
                ),
                ("second", vec![fixture.clips[1].clone(), extra]),
            ] {
                let linked = fixture
                    .accept(fixture.request(
                        "clips.link",
                        &format!("unlink-chain-{key}"),
                        json!({"clip_ids":seeds,"link_policy":"expand"}),
                    ))
                    .await;
                groups.push(
                    EntityId::parse(linked["created"]["link_group_id"].as_str().unwrap()).unwrap(),
                );
            }
        }
        let (id, target, role) = match relation {
            "incoming-nonclip" => (&fixture.sources[3], &fixture.clips[0], "link"),
            "outgoing-nonclip" => (&fixture.clips[0], &fixture.sources[3], "link"),
            "semantic-input" => (&fixture.sources[3], &fixture.clips[0], "semantic_input"),
            "outgoing" => (&fixture.clips[0], &fixture.clips[2], "link"),
            "multiple-groups" => (&fixture.clips[1], &fixture.clips[2], "link"),
            _ => (&fixture.clips[2], &fixture.clips[0], "link"),
        };
        let mut entity = fixture.entity(id);
        entity.references.push(EntityReference {
            role: role.into(),
            target: target.clone(),
        });
        entity.extensions.insert(
            "future_optional".into(),
            serde_json::from_str("{\"number\":9007199254740993,\"escape\":\"\\u0061\"}").unwrap(),
        );
        fixture.install_future_envelope(entity.clone());
        let mut selected = fixture.clips.clone();
        if relation == "multiple-groups" {
            let read = fixture
                .service
                .store
                .project_read(fixture.project.as_str())
                .unwrap();
            selected.extend(read.dependents(&groups[1], 128).unwrap());
            selected.sort();
            selected.dedup();
        }
        let before = unlink_images(&fixture.service, &fixture.project, &selected, &groups);
        let request = fixture.request(
            "clips.unlink",
            relation,
            json!({"clip_ids":[fixture.clips[0]],"link_policy":"expand"}),
        );
        if relation.ends_with("nonclip") {
            fixture.reject_unchanged(request, "VALIDATION_ERROR").await;
            assert_eq!(
                unlink_images(&fixture.service, &fixture.project, &selected, &groups),
                before
            );
            continue;
        }
        let result = fixture.accept(request).await;
        let mut expected = match relation {
            "semantic-input" => vec![fixture.clips[0].clone()],
            "multiple-groups" => selected
                .iter()
                .filter(|id| *id != &fixture.clips[3])
                .cloned()
                .collect(),
            _ => vec![fixture.clips[0].clone(), fixture.clips[2].clone()],
        };
        expected.sort();
        assert_eq!(result["created"], json!({"clip_ids":expected}));
        if relation == "semantic-input" {
            assert_eq!(result["no_change"], true);
            assert_eq!(result["changed_ids"], json!([]));
            assert_eq!(fixture.raw(id), entity.bytes().unwrap());
            assert_eq!(
                unlink_images(&fixture.service, &fixture.project, &selected, &groups),
                before
            );
            continue;
        }
        let changed = if relation == "multiple-groups" {
            expected.clone()
        } else {
            vec![id.clone()]
        };
        assert_eq!(result["changed_ids"], json!(changed));
        let read = fixture
            .service
            .store
            .project_read(fixture.project.as_str())
            .unwrap();
        for member in &expected {
            let after = read.entity(member).unwrap();
            assert_eq!(after.payload::<Clip>("clip").unwrap().link_group_id, None);
            assert!(
                !after
                    .references
                    .iter()
                    .any(|reference| reference.role == "link")
            );
        }
        for group in &groups {
            assert!(read.raw_entity(group).unwrap().is_none());
            assert!(read.dependents(group, 128).unwrap().is_empty());
        }
        let after_entity = read.entity(id).unwrap();
        assert_eq!(
            after_entity.extensions["future_optional"].get(),
            entity.extensions["future_optional"].get()
        );
        assert_eq!(
            serde_json::to_value(&after_entity.references).unwrap(),
            serde_json::to_value(
                entity
                    .references
                    .iter()
                    .filter(|reference| reference.role != "link")
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        drop(read);
        let after = unlink_images(&fixture.service, &fixture.project, &selected, &groups);
        fixture
            .accept(fixture.request("history.undo", "unlink-edge-undo", json!({})))
            .await;
        assert_eq!(
            unlink_images(&fixture.service, &fixture.project, &selected, &groups),
            before
        );
        fixture
            .accept(fixture.request("history.redo", "unlink-edge-redo", json!({})))
            .await;
        assert_eq!(
            unlink_images(&fixture.service, &fixture.project, &selected, &groups),
            after
        );
    }
}

// Pure resolver collision model, reached only after every public Link/Unlink oracle above.
async fn linked_split_right_group_collision() {
    use nw_domain::time::Rational;
    let fixture = Fixture::new().await;
    let linked = fixture
        .accept(fixture.request(
            "clips.link",
            "split-collision-source-group",
            json!({"clip_ids":[fixture.clips[0],fixture.clips[2]],"link_policy":"expand"}),
        ))
        .await;
    let source_group =
        EntityId::parse(linked["created"]["link_group_id"].as_str().unwrap()).unwrap();
    let occupied_clip: Clip = fixture.entity(&fixture.clips[1]).payload("clip").unwrap();
    let placed = fixture
        .accept(fixture.request(
            "clips.place",
            "split-collision-extra",
            json!({
                "sequence_id":occupied_clip.sequence_id,"track_id":occupied_clip.track_id,
                "source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},
                "source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
                "sequence_start":{"num":"6","den":"1"},"collision":"reject"
            }),
        ))
        .await;
    let extra = EntityId::parse(placed["created"]["clip_ids"][0].as_str().unwrap()).unwrap();
    let extra_source = EntityId::parse(placed["created"]["source_id"].as_str().unwrap()).unwrap();
    let occupied = fixture
        .accept(fixture.request(
            "clips.link",
            "split-collision-occupied-group",
            json!({"clip_ids":[fixture.clips[1],extra],"link_policy":"expand"}),
        ))
        .await;
    let occupied_group =
        EntityId::parse(occupied["created"]["link_group_id"].as_str().unwrap()).unwrap();

    // No setup read survives a public write. This final snapshot is never changed/applied.
    let read = fixture
        .service
        .store
        .project_read(fixture.project.as_str())
        .unwrap();
    let mut source_members = vec![fixture.clips[0].clone(), fixture.clips[2].clone()];
    source_members.sort();
    let mut occupied_members = vec![fixture.clips[1].clone(), extra.clone()];
    occupied_members.sort();
    assert_ne!(source_group, occupied_group);
    assert_eq!(linked["changed_ids"], json!(source_members));
    assert_eq!(occupied["changed_ids"], json!(occupied_members));
    for (group, members) in [
        (&source_group, &source_members),
        (&occupied_group, &occupied_members),
    ] {
        assert!(read.raw_entity(group).unwrap().is_none());
        assert_eq!(read.dependents(group, 128).unwrap(), *members);
        assert!(read.has_dependency_target(group).unwrap());
        assert_eq!(
            read.dependents(group, 1).unwrap_err().code,
            "RESOURCE_LIMIT"
        );
    }

    let zero = Rational::ZERO;
    let one = Rational::new(1, 1).unwrap();
    let half = Rational::new(1, 2).unwrap();
    let four = Rational::new(4, 1).unwrap();
    let primary: Clip = read
        .entity(&fixture.clips[0])
        .unwrap()
        .payload("clip")
        .unwrap();
    let partner: Clip = read
        .entity(&fixture.clips[2])
        .unwrap()
        .payload("clip")
        .unwrap();
    assert_eq!(primary.sequence_id, partner.sequence_id);
    assert_ne!(primary.track_id, partner.track_id);
    let mut clips = fixture.clips.clone();
    clips.push(extra.clone());
    let root_bin = read.header().root_bin_id.clone();
    assert!(
        !read
            .entity(&root_bin)
            .unwrap()
            .payload::<Bin>("bin")
            .unwrap()
            .locked
    );
    let mut entity_ids = std::collections::BTreeSet::from([root_bin.clone()]);
    let mut identities =
        std::collections::BTreeSet::from([source_group.clone(), occupied_group.clone()]);
    let mut sequences = std::collections::BTreeSet::new();
    for (id, start) in [
        (&fixture.clips[0], 0),
        (&fixture.clips[1], 4),
        (&fixture.clips[2], 0),
        (&fixture.clips[3], 0),
        (&extra, 6),
    ] {
        let entity = read.entity(id).unwrap();
        entity.semantics().unwrap();
        let clip: Clip = entity.payload("clip").unwrap();
        let track: Track = read
            .entity(&clip.track_id)
            .unwrap()
            .payload("track")
            .unwrap();
        let sequence: Sequence = read
            .entity(&clip.sequence_id)
            .unwrap()
            .payload("sequence")
            .unwrap();
        assert!(!clip.locked && !track.locked && !sequence.locked);
        assert_eq!(track.sequence_id, clip.sequence_id);
        assert_eq!(track.grid, four);
        assert_eq!(clip.rate, one);
        assert_eq!(clip.sequence_start, Rational::new(start, 1).unwrap());
        assert_eq!(clip.end().unwrap(), Rational::new(start + 1, 1).unwrap());
        assert_eq!(clip.duration, one);
        assert_eq!(clip.source_range.start, zero);
        assert_eq!(clip.source_range.end, one);
        let source = read.entity(&clip.source_id).unwrap();
        source.semantics().unwrap();
        match source.payload::<SourceBinding>("source_binding").unwrap() {
            SourceBinding::SolidColor { duration, .. } => assert_eq!(duration, four),
            other => panic!("Split collision setup requires a solid source: {other:?}"),
        }
        if id == &fixture.clips[0] || id == &fixture.clips[2] {
            assert_eq!(clip.link_group_id.as_ref(), Some(&source_group));
            assert!(clip.sequence_start < half && half < clip.end().unwrap());
            assert_eq!(grid_tick(half, track.grid).unwrap(), 2);
        } else if id == &fixture.clips[1] || id == &extra {
            assert_eq!(clip.sequence_id, primary.sequence_id);
            assert_eq!(clip.track_id, primary.track_id);
            assert_eq!(clip.link_group_id.as_ref(), Some(&occupied_group));
            assert!(clip.sequence_start > one);
        }
        assert!(
            !entity
                .references
                .iter()
                .any(|reference| reference.role == "link")
        );
        entity_ids.extend([
            id.clone(),
            clip.source_id,
            clip.track_id,
            clip.sequence_id.clone(),
        ]);
        identities.insert(clip.occurrence_id);
        if let Some(group) = clip.link_group_id {
            identities.insert(group);
        }
        sequences.insert(clip.sequence_id);
    }
    // Include even unused tracks, then prove the finite fixture list covers every entity.
    for sequence in sequences {
        let page = read.track_page(&sequence, None, 128, 128 * 1024).unwrap();
        assert!(page.next_key.is_none());
        assert_eq!(page.items.len(), 2);
        for track in page.items {
            entity_ids.insert(EntityId::parse(track["id"].as_str().unwrap()).unwrap());
        }
    }
    assert_eq!(entity_ids.len(), read.header().entity_count);
    identities.extend(entity_ids.iter().cloned());
    let fixed = [
        "00000000-0000-4000-8000-000000000101",
        "00000000-0000-4000-8000-000000000102",
        "00000000-0000-4000-8000-000000000103",
        "00000000-0000-4000-8000-000000000104",
        "00000000-0000-4000-8000-000000000105",
    ]
    .map(|id| EntityId::parse(id).unwrap());
    assert_eq!(
        fixed
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5
    );
    for id in &fixed {
        assert!(
            !identities.contains(id),
            "Fixed Split allocation identity is already in use: {id}"
        );
        assert!(read.raw_entity(id).unwrap().is_none());
        assert!(!read.has_dependency_target(id).unwrap());
    }
    let [occ_a, right_a, occ_b, right_b, free_group] = fixed;
    let groups = [source_group.clone(), occupied_group.clone()];
    let snapshot = || {
        let raw = entity_ids
            .iter()
            .map(|id| (id.clone(), read.raw_entity(id).unwrap().unwrap()))
            .collect::<BTreeMap<_, _>>();
        json!({
            "header":read.header_raw(),"raw":raw,
            "retained":unlink_unchanged_facts(&fixture, &groups),
            "images":unlink_images(&fixture.service, &fixture.project, &clips, &groups),
            "extra_source_reference":fixture.service.store.project_fact_hash(&format!("project-reference/{}/{extra_source}", fixture.project)).unwrap()
        })
    };
    let before = snapshot();
    let intent = |at: Rational| {
        ProjectEditIntent::Split(
            commands::parse(json!({
                "clip_id":fixture.clips[0],"at_sequence":at,"link_policy":"expand"
            }))
            .unwrap(),
        )
    };
    for boundary in [zero, one] {
        let unchanged = resolve_edit(&read, intent(boundary), AllocatedIds(VecDeque::new()))
            .expect("Primary boundary must finish without allocating or probing a right group");
        assert!(unchanged.forward.changes.is_empty());
        assert!(unchanged.inverse.changes.is_empty());
        assert!(unchanged.changed_ids.is_empty());
        assert_eq!(unchanged.created, json!({"clip_ids":[],"no_change":true}));
        assert_eq!(snapshot(), before);
    }

    // A is the explicit primary. Exact queue: A occurrence, right group, A right,
    // B occurrence, B right. No exhaustion, injected dependency rows, or retries.
    let allocations = |group: EntityId| {
        AllocatedIds(VecDeque::from([
            occ_a.clone(),
            group,
            right_a.clone(),
            occ_b.clone(),
            right_b.clone(),
        ]))
    };
    let error = resolve_edit(&read, intent(half), allocations(occupied_group.clone()))
        .expect_err("Occupied right group must reject");
    assert_eq!(
        error.code, "OBJECT_CONFLICT",
        "Dependency-occupied right-group model: {error:?}"
    );
    assert_eq!(snapshot(), before);

    // Independent entity occupancy and free-candidate controls remain after the intended RED.
    assert!(read.raw_entity(&root_bin).unwrap().is_some());
    let error = resolve_edit(&read, intent(half), allocations(root_bin))
        .expect_err("Entity-occupied right group must reject");
    assert_eq!(
        error.code, "OBJECT_CONFLICT",
        "Entity-occupied right-group model: {error:?}"
    );
    assert_eq!(snapshot(), before);
    let resolved = resolve_edit(&read, intent(half), allocations(free_group.clone()))
        .expect("A free right group must resolve on valid linked geometry");
    let mut changed = vec![
        fixture.clips[0].clone(),
        fixture.clips[2].clone(),
        right_a.clone(),
        right_b.clone(),
        primary.track_id.clone(),
        partner.track_id.clone(),
    ];
    changed.sort();
    assert_eq!(resolved.changed_ids, changed);
    assert_eq!(resolved.forward.changes.len(), 6);
    assert_eq!(
        resolved.created,
        json!({"clip_ids":[right_a,right_b],"no_change":false})
    );
    assert_eq!(
        serde_json::to_value(&resolved.inverse).unwrap(),
        serde_json::to_value(resolved.forward.inverse()).unwrap()
    );
    for (left_id, right_id, occurrence) in [
        (&fixture.clips[0], &right_a, &occ_a),
        (&fixture.clips[2], &right_b, &occ_b),
    ] {
        let original = read.entity(left_id).unwrap();
        let original_clip: Clip = original.payload("clip").unwrap();
        for right in [false, true] {
            let mut expected = original.clone();
            let mut clip = original_clip.clone();
            clip.duration = half;
            if right {
                expected.id = right_id.clone();
                clip.occurrence_id = occurrence.clone();
                clip.sequence_start = half;
                clip.source_range.start = half;
                clip.link_group_id = Some(free_group.clone());
            } else {
                clip.source_range.end = half;
            }
            expected.update(&clip).unwrap();
            let change = resolved
                .forward
                .changes
                .iter()
                .find(|change| change.id == expected.id)
                .unwrap();
            assert_eq!(change.before, read.raw_entity(&expected.id).unwrap());
            assert_eq!(
                change.after.as_deref(),
                Some(expected.bytes().unwrap().as_str())
            );
            assert_eq!(clip.duration, half);
            assert_eq!(clip.source_range.duration().unwrap(), half);
            assert_eq!(
                clip.link_group_id.as_ref(),
                Some(if right { &free_group } else { &source_group })
            );
        }
        let mut expected = read.entity(&original_clip.track_id).unwrap();
        let mut track: Track = expected.payload("track").unwrap();
        track.clip_count += 1;
        expected.update(&track).unwrap();
        let change = resolved
            .forward
            .changes
            .iter()
            .find(|change| change.id == expected.id)
            .unwrap();
        assert_eq!(change.before, read.raw_entity(&expected.id).unwrap());
        assert_eq!(
            change.after.as_deref(),
            Some(expected.bytes().unwrap().as_str())
        );
    }
    assert!(
        !resolved
            .forward
            .changes
            .iter()
            .any(|change| change.id == free_group)
    );
    assert!(read.raw_entity(&free_group).unwrap().is_none());
    assert!(!read.has_dependency_target(&free_group).unwrap());
    assert_eq!(snapshot(), before);
    // Pure resolution only: no patch application or public Split rejection/rollback claim.
}
