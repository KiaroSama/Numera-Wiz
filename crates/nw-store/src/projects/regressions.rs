//! Store review fixtures retain the original transaction-test entrypoint.
use super::*;
use nw_domain::commands::ProjectRef;
use nw_domain::projects::edit::{AllocatedIds, ProjectEditIntent, resolve_edit};
use nw_domain::time::Rational;
use std::collections::VecDeque;

fn creation(project: &ProjectId, root: &EntityId, principal: &str) -> ProjectCommit {
    let entity = EntityEnvelope::new(
        project,
        root.clone(),
        "bin",
        None,
        "Root".into(),
        &Bin {
            member_count: 0,
            locked: false,
        },
    )
    .unwrap();
    ProjectCommit {
        header: ProjectHeader {
            envelope_version: 1,
            schema_major: 1,
            id: project.clone(),
            creator: principal.into(),
            name: "Review fixture".into(),
            revision: Revision(0),
            root_bin_id: root.clone(),
            entity_count: 0,
            sequence_count: 0,
            history_root: format!("project-history/{project}"),
            materialized_revision: Revision(0),
            change_digest: String::new(),
            compatibility: "supported".into(),
            extensions: BTreeMap::new(),
        },
        expected_header: None,
        patch: ProjectPatch {
            changes: vec![EntityChange {
                id: root.clone(),
                before: None,
                after: Some(entity.bytes().unwrap()),
            }],
        },
        operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
        history_action: HistoryAction::Normal,
        created: json!({}),
        expected_records: Vec::new(),
    }
}
fn request(
    command: &str,
    params: Value,
    scope: &str,
    key: &str,
    project: Option<(&ProjectId, Revision)>,
) -> Request {
    let mut request = Request::query(command, params);
    request.replay_scope = Some(scope.into());
    request.idempotency_key = Some(key.into());
    request.project = project.map(|(id, revision)| ProjectRef {
        id: id.to_string(),
        expected_revision: Some(revision.to_string()),
    });
    request
}
fn scoped_digest(store: &Store, project: &ProjectId) -> String {
    let read = store.db.begin_read().unwrap();
    let table = read.open_table(RECORDS).unwrap();
    let mut digest = Sha256::new();
    for row in table.iter().unwrap() {
        let (key, value) = row.unwrap();
        if key.value().contains(project.as_str()) {
            digest.update((key.value().len() as u64).to_le_bytes());
            digest.update(key.value().as_bytes());
            digest.update((value.value().len() as u64).to_le_bytes());
            digest.update(value.value());
        }
    }
    hex::encode(digest.finalize())
}
fn schema3_atomic_failure() {
    let temporary = tempfile::tempdir().unwrap();
    let root = nw_platform::security::private_directory(&temporary.path().join("root")).unwrap();
    let generation =
        nw_platform::security::private_directory(&root.join("state/generations/1")).unwrap();
    let store = Store::open(&generation).unwrap();
    let lease = crate::generations::RootLease::acquire(&root).unwrap();
    let principal = format!("owner-{}", store.root_id());
    let scope = store.issue_scope(&principal, 100).unwrap();
    let plan = store
        .prepare_migration(&lease, &"ab".repeat(32), 101)
        .unwrap();
    let migration_id = plan.id.clone();
    let migrate = request(
        "maintenance.migrate",
        json!({"plan":plan}),
        &scope,
        "review-migration",
        None,
    );
    store
        .migrate_generation(
            &lease,
            &principal,
            &migrate,
            &"ab".repeat(32),
            || Ok(102),
            &tokio_util::sync::CancellationToken::new(),
        )
        .unwrap();
    let activation = lease.selection().unwrap().activation_id.unwrap();
    assert_eq!(
        store.adoption_revision(&migration_id, &activation).unwrap(),
        "0"
    );
    let project = ProjectId::parse(Uuid::new_v4().to_string()).unwrap();
    let bin = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
    let create = request(
        "projects.create",
        json!({"name":"Review fixture"}),
        &scope,
        "schema3-project-fault",
        None,
    );
    let guard_before = store.project_fact_hash("meta/rollback-guard").unwrap();
    let clock_before = store.project_fact_hash("meta/clock").unwrap();
    let rows_before = scoped_digest(&store, &project);
    store
        .project_fail_after_rows
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        store
            .commit_project_with_clock(
                &principal,
                &create,
                creation(&project, &bin, &principal),
                || Ok(103)
            )
            .unwrap_err()
            .code,
        "INJECTED_PROJECT_FAILURE"
    );
    assert_eq!(scoped_digest(&store, &project), rows_before);
    assert_eq!(
        store.project_fact_hash("meta/rollback-guard").unwrap(),
        guard_before
    );
    assert_eq!(store.project_fact_hash("meta/clock").unwrap(), clock_before);
    assert_eq!(
        store.adoption_revision(&migration_id, &activation).unwrap(),
        "0"
    );
    assert!(store.replay(&principal, &create, 103).unwrap().is_none());
    let accepted = store
        .commit_project_with_clock(
            &principal,
            &create,
            creation(&project, &bin, &principal),
            || Ok(103),
        )
        .unwrap();
    assert!(accepted.ok);
    assert_eq!(
        store.adoption_revision(&migration_id, &activation).unwrap(),
        "1"
    );
    let replay = store.replay(&principal, &create, 104).unwrap().unwrap();
    assert_eq!(replay.receipt_id, accepted.receipt_id);
    assert_eq!(
        store.adoption_revision(&migration_id, &activation).unwrap(),
        "1"
    );
}

struct EditFixture<'a> {
    store: &'a Store,
    project: ProjectId,
    principal: String,
    scope: String,
    now: u64,
}
impl<'a> EditFixture<'a> {
    fn new(store: &'a Store) -> Self {
        let principal = "history-cardinality".to_owned();
        let scope = store.issue_scope(&principal, 200).unwrap();
        let project = ProjectId::parse(Uuid::new_v4().to_string()).unwrap();
        let bin = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
        let create = request(
            "projects.create",
            json!({"name":"Review fixture"}),
            &scope,
            "cardinality-create",
            None,
        );
        store
            .commit_project_with_clock(
                &principal,
                &create,
                creation(&project, &bin, &principal),
                || Ok(201),
            )
            .unwrap();
        Self {
            store,
            project,
            principal,
            scope,
            now: 201,
        }
    }
    fn prepare(&mut self, intent: ProjectEditIntent) -> (Request, ProjectCommit) {
        self.now += 1;
        let (command, params) = match &intent {
            ProjectEditIntent::Sequence(input) => {
                ("sequences.create", serde_json::to_value(input).unwrap())
            }
            ProjectEditIntent::Place(input, _) => {
                ("clips.place", serde_json::to_value(input).unwrap())
            }
            ProjectEditIntent::Move(input) => ("clips.move", serde_json::to_value(input).unwrap()),
            _ => panic!("Unexpected review fixture intent"),
        };
        let read = self.store.project_read(self.project.as_str()).unwrap();
        let request = request(
            command,
            params,
            &self.scope,
            &format!("cardinality-edit-{}", self.now),
            Some((&self.project, read.header().revision)),
        );
        let ids = AllocatedIds(
            (0..8)
                .map(|_| EntityId::parse(Uuid::new_v4().to_string()).unwrap())
                .collect::<VecDeque<_>>(),
        );
        let resolved = resolve_edit(&read, intent, ids).unwrap();
        let prepared = ProjectCommit {
            header: read.header().clone(),
            expected_header: Some(read.header_raw().into()),
            patch: resolved.forward,
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: HistoryAction::Normal,
            created: resolved.created,
            expected_records: Vec::new(),
        };
        (request, prepared)
    }
    fn apply(&mut self, intent: ProjectEditIntent) -> Value {
        let (request, prepared) = self.prepare(intent);
        self.store
            .commit_project_with_clock(&self.principal, &request, prepared, || Ok(self.now))
            .unwrap()
            .result
            .unwrap()
    }
    fn undo(&mut self) {
        self.now += 1;
        let read = self.store.project_read(self.project.as_str()).unwrap();
        let history = read.history().unwrap();
        let entry = history.undo.last().unwrap();
        let patch = read.patch(&entry.id).unwrap().inverse;
        let request = request(
            "history.undo",
            json!({}),
            &self.scope,
            &format!("cardinality-undo-{}", self.now),
            Some((&self.project, read.header().revision)),
        );
        let prepared = ProjectCommit {
            header: read.header().clone(),
            expected_header: Some(read.header_raw().into()),
            patch,
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: HistoryAction::Undo(entry.id.clone()),
            created: json!({}),
            expected_records: Vec::new(),
        };
        drop(read);
        assert!(
            self.store
                .commit_project_with_clock(&self.principal, &request, prepared, || Ok(self.now))
                .unwrap()
                .ok
        );
    }
}
fn retained_reference_branch() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let mut fixture = EditFixture::new(&store);
    let sequence = fixture.apply(ProjectEditIntent::Sequence(SequenceInput {
        name: "Cardinality sequence".into(),
        settings: SequenceSettings {
            width: 64,
            height: 48,
            frame_rate: Rational::new(4, 1).unwrap(),
            sample_rate: 48000,
            audio_layout: "stereo".into(),
        },
        tracks: vec![TrackInput {
            kind: TrackKind::Video,
            name: "Video".into(),
        }],
    }));
    let sequence_id =
        EntityId::parse(sequence["created"]["sequence_id"].as_str().unwrap()).unwrap();
    let track_id =
        EntityId::parse(sequence["created"]["tracks"][0]["id"].as_str().unwrap()).unwrap();
    let mut clips = Vec::new();
    let mut sources = Vec::new();
    for index in 0..512i64 {
        let duration = Rational::new(1, 1).unwrap();
        let placed = fixture.apply(ProjectEditIntent::Place(
            Box::new(PlaceInput {
                sequence_id: sequence_id.clone(),
                track_id: track_id.clone(),
                source: PlaceSource::SolidColor {
                    rgba: [0.0, 0.0, 0.0, 1.0],
                    duration,
                },
                source_range: SourceSpan {
                    start: Rational::ZERO,
                    end: duration,
                },
                sequence_start: Rational::new(index * 4, 1).unwrap(),
                collision: "reject".into(),
            }),
            SourceBinding::SolidColor {
                rgba: [0.0, 0.0, 0.0, 1.0],
                duration,
            },
        ));
        clips.push(EntityId::parse(placed["created"]["clip_ids"][0].as_str().unwrap()).unwrap());
        sources.push(EntityId::parse(placed["created"]["source_id"].as_str().unwrap()).unwrap());
    }
    // Every original operation is admitted through the real resolver and receipt transaction.
    for pair in clips.chunks_exact(2) {
        fixture.apply(ProjectEditIntent::Move(MoveInput {
            clip_ids: pair.to_vec(),
            delta: Rational::new(1, 4).unwrap(),
            collision: "reject".into(),
            link_policy: "expand".into(),
        }));
    }
    let history = store
        .project_read(fixture.project.as_str())
        .unwrap()
        .history()
        .unwrap();
    let retained = history.undo.len();
    assert!(retained > 0 && retained <= 256);
    let distinct = history
        .undo
        .iter()
        .flat_map(|entry| entry.source_ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .len();
    // A future fix may evict complete oldest originals earlier to reserve branch headroom.
    // Always undo the entire admitted retained stack, rather than requiring illegal retention.
    for _ in 0..retained {
        fixture.undo();
    }
    let before = store.project_read(fixture.project.as_str()).unwrap();
    assert!(before.history().unwrap().undo.is_empty());
    assert_eq!(before.history().unwrap().redo.len(), retained);
    let revision = before.header().revision;
    let entity_count = before.header().entity_count;
    let source_bytes = sources
        .iter()
        .map(|id| before.raw_entity(id).unwrap().unwrap())
        .collect::<Vec<_>>();
    drop(before);
    let fingerprint = scoped_digest(&store, &fixture.project);
    let clock = store.project_fact_hash("meta/clock").unwrap();
    let intent = MoveInput {
        clip_ids: vec![clips.last().unwrap().clone()],
        delta: Rational::new(1, 4).unwrap(),
        collision: "reject".into(),
        link_policy: "expand".into(),
    };
    let (request, prepared) = fixture.prepare(ProjectEditIntent::Move(intent.clone()));
    let result =
        store.commit_project_with_clock(&fixture.principal, &request, prepared, || Ok(fixture.now));
    if result.is_err() {
        assert_eq!(
            scoped_digest(&store, &fixture.project),
            fingerprint,
            "Rejected branch changed project rows"
        );
        assert_eq!(store.project_fact_hash("meta/clock").unwrap(), clock);
        assert!(
            store
                .replay(&fixture.principal, &request, fixture.now)
                .unwrap()
                .is_none()
        );
        let read = store.project_read(fixture.project.as_str()).unwrap();
        let resolved = resolve_edit(
            &read,
            ProjectEditIntent::Move(intent),
            AllocatedIds(VecDeque::new()),
        )
        .unwrap();
        let retry = ProjectCommit {
            header: read.header().clone(),
            expected_header: Some(read.header_raw().into()),
            patch: resolved.forward,
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: HistoryAction::Normal,
            created: resolved.created,
            expected_records: Vec::new(),
        };
        drop(read);
        assert!(
            store
                .commit_project_with_clock(&fixture.principal, &request, retry, || Ok(fixture.now))
                .is_err()
        );
        assert_eq!(scoped_digest(&store, &fixture.project), fingerprint);
        assert!(
            store
                .replay(&fixture.principal, &request, fixture.now)
                .unwrap()
                .is_none()
        );
    }
    let after = store.project_read(fixture.project.as_str()).unwrap();
    assert_eq!(after.header().entity_count, entity_count);
    for ((source, bytes), clip) in sources.iter().zip(source_bytes).zip(&clips) {
        assert_eq!(
            after.raw_entity(source).unwrap().as_deref(),
            Some(bytes.as_str()),
            "History pruning must preserve every current source binding"
        );
        assert!(after.raw_entity(clip).unwrap().is_some());
        let reference: Value = store
            .get(&format!("project-reference/{}/{source}", fixture.project))
            .unwrap()
            .unwrap();
        assert_eq!(
            reference["live"], 2,
            "Binding and live clip references must survive history pruning"
        );
    }
    match result {
        Ok(response) => {
            assert_eq!(after.header().revision, revision.next().unwrap());
            assert!(after.history().unwrap().redo.is_empty());
            assert_eq!(
                store
                    .replay(&fixture.principal, &request, fixture.now)
                    .unwrap()
                    .unwrap()
                    .receipt_id,
                response.receipt_id
            );
        }
        Err(error) => panic!(
            "I3: tiny branch rejected after 512 sources, 256 admitted two-source moves, {retained} undos and {distinct} retained source identities: {} (atomic/live preservation checks passed)",
            error.code
        ),
    }
}
pub(super) fn run() {
    schema3_atomic_failure();
    retained_reference_branch();
}
