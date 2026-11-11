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
        let read = self
            .service
            .store
            .project_read(self.project.as_str())
            .unwrap();
        let before = read.raw_entity(&entity.id).unwrap();
        let request = self.move_request(0, 0, "install-future-fixture");
        let prepared = ProjectCommit {
            header: read.header().clone(),
            expected_header: Some(read.header_raw().into()),
            patch: ProjectPatch {
                changes: vec![EntityChange {
                    id: entity.id.clone(),
                    before,
                    after: Some(entity.bytes().unwrap()),
                }],
            },
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
    async fn expect_blocked(&self, clip: usize, key: &str, failures: &mut Vec<String>) {
        let before = self
            .service
            .store
            .project_read(self.project.as_str())
            .unwrap()
            .header_raw()
            .to_owned();
        let entity_before = self.raw(&self.clips[clip]);
        let history_before = self.history_hash();
        let request = self.move_request(clip, 1, key);
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
        if response.ok || !recognized || response.receipt_id.is_some() || !unchanged || !no_receipt
        {
            failures.push(format!(
                "{key}: result={code}, unchanged={unchanged}, no_receipt={no_receipt}"
            ));
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
