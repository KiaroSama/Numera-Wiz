//! Adversarial compiler reads run inside the existing atomic-project Store test entry.
use super::*;
use crate::{
    Store,
    projects::{HistoryAction, ProjectCommit},
};
use nw_domain::{
    commands::{ProjectRef, Request},
    media::{self, RenderInput},
    projects::{
        edit::{AllocatedIds, ProjectEditIntent, ProjectRead, resolve_edit},
        *,
    },
    time::Rational,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
};
use uuid::Uuid;

fn id() -> EntityId {
    EntityId::parse(Uuid::new_v4().to_string()).unwrap()
}
struct Fixture<'a> {
    store: &'a Store,
    project: ProjectId,
    sequence: EntityId,
    track: EntityId,
    scope: String,
    now: u64,
}
impl<'a> Fixture<'a> {
    fn new(store: &'a Store) -> Self {
        let project = ProjectId::parse(Uuid::new_v4().to_string()).unwrap();
        let root = id();
        let scope = store.issue_scope("compiler-capture", 100).unwrap();
        let entity = EntityEnvelope::new(
            &project,
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
        let mut request = Request::query("projects.create", json!({"name":"Compiler capture"}));
        request.replay_scope = Some(scope.clone());
        request.idempotency_key = Some("capture-create".into());
        let prepared = ProjectCommit {
            header: ProjectHeader {
                envelope_version: 1,
                schema_major: 1,
                id: project.clone(),
                creator: "compiler-capture".into(),
                name: "Compiler capture".into(),
                revision: Revision(0),
                root_bin_id: root,
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
                    id: entity.id.clone(),
                    before: None,
                    after: Some(entity.bytes().unwrap()),
                }],
            },
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: HistoryAction::Normal,
            created: json!({}),
            expected_records: Vec::new(),
        };
        assert!(
            store
                .commit_project_with_clock("compiler-capture", &request, prepared, || Ok(101))
                .unwrap()
                .ok
        );
        let mut fixture = Self {
            store,
            project,
            sequence: id(),
            track: id(),
            scope,
            now: 101,
        };
        let created = fixture.apply(
            ProjectEditIntent::Sequence(SequenceInput {
                name: "Video".into(),
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
            }),
            false,
        );
        fixture.sequence =
            EntityId::parse(created["created"]["sequence_id"].as_str().unwrap()).unwrap();
        fixture.track =
            EntityId::parse(created["created"]["tracks"][0]["id"].as_str().unwrap()).unwrap();
        fixture
    }
    fn apply(&mut self, intent: ProjectEditIntent, retained_disabled_metadata: bool) -> Value {
        self.now += 1;
        let (command, params) = match &intent {
            ProjectEditIntent::Sequence(input) => {
                ("sequences.create", serde_json::to_value(input).unwrap())
            }
            ProjectEditIntent::Place(input, _) => {
                ("clips.place", serde_json::to_value(input).unwrap())
            }
            _ => panic!("Unexpected compiler fixture edit"),
        };
        let read = self.store.project_read(self.project.as_str()).unwrap();
        let mut request = Request::query(command, params);
        request.project = Some(ProjectRef {
            id: self.project.to_string(),
            expected_revision: Some(read.header().revision.to_string()),
        });
        request.replay_scope = Some(self.scope.clone());
        request.idempotency_key = Some(format!("capture-edit-{}", self.now));
        let mut resolved = resolve_edit(
            &read,
            intent,
            AllocatedIds((0..8).map(|_| id()).collect::<VecDeque<_>>()),
        )
        .unwrap();
        if retained_disabled_metadata {
            for change in &mut resolved.forward.changes {
                if let Some(raw) = &change.after {
                    let mut entity: EntityEnvelope = serde_json::from_str(raw).unwrap();
                    if entity.kind == "clip" {
                        let mut payload: Value =
                            serde_json::from_str(&entity.payload_json).unwrap();
                        payload["retained_metadata"] = json!("m".repeat(56 * 1024));
                        entity.payload_json = serde_json::to_string(&payload).unwrap();
                        entity.payload_sha256 = domain::hash(entity.payload_json.as_bytes());
                        entity.enabled = false;
                        change.after = Some(entity.bytes().unwrap());
                    }
                }
            }
        }
        let prepared = ProjectCommit {
            header: read.header().clone(),
            expected_header: Some(read.header_raw().into()),
            patch: resolved.forward,
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: HistoryAction::Normal,
            created: resolved.created,
            expected_records: Vec::new(),
        };
        drop(read);
        self.store
            .commit_project_with_clock("compiler-capture", &request, prepared, || Ok(self.now))
            .unwrap()
            .result
            .unwrap()
    }
    fn place(&mut self, start: i64, retained_disabled_metadata: bool) -> EntityId {
        let one = Rational::new(1, 1).unwrap();
        let result = self.apply(
            ProjectEditIntent::Place(
                Box::new(PlaceInput {
                    sequence_id: self.sequence.clone(),
                    track_id: self.track.clone(),
                    source: PlaceSource::SolidColor {
                        rgba: [1.0, 0.0, 0.0, 1.0],
                        duration: one,
                    },
                    source_range: SourceSpan {
                        start: Rational::ZERO,
                        end: one,
                    },
                    sequence_start: Rational::new(start, 1).unwrap(),
                    collision: "reject".into(),
                }),
                SourceBinding::SolidColor {
                    rgba: [1.0, 0.0, 0.0, 1.0],
                    duration: one,
                },
            ),
            retained_disabled_metadata,
        );
        EntityId::parse(result["created"]["clip_ids"][0].as_str().unwrap()).unwrap()
    }
    fn input(&self, duration: i64) -> RenderInput {
        let revision = self
            .store
            .project_read(self.project.as_str())
            .unwrap()
            .header()
            .revision;
        serde_json::from_value(json!({"sequence_id":self.sequence,"revision":revision,"range":{"start":{"num":"0","den":"1"},"end":{"num":duration.to_string(),"den":"1"}},"output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le","audio":"silence","destination":{"kind":"download"}}})).unwrap()
    }
}

struct ObservedRead<'a> {
    read: &'a ProjectReadSnapshot,
    copied: Cell<usize>,
    rejected: Cell<usize>,
    attempted: RefCell<BTreeSet<EntityId>>,
}
impl ProjectRead for ObservedRead<'_> {
    fn header(&self) -> &ProjectHeader {
        self.read.header()
    }
    fn raw_entity(&self, _: &EntityId) -> Result<Option<String>> {
        panic!("Compiler used legacy raw prefetch")
    }
    fn has_dependency_target(&self, id: &EntityId) -> Result<bool> {
        self.read.has_dependency_target(id)
    }
    fn dependents(&self, id: &EntityId, limit: usize) -> Result<Vec<EntityId>> {
        self.read.dependents(id, limit)
    }
    fn intersecting(
        &self,
        _: &EntityId,
        _: &EntityId,
        _: Rational,
        _: Rational,
        _: usize,
    ) -> Result<Vec<EntityEnvelope>> {
        panic!("Compiler used legacy full-vector interval prefetch")
    }
}
impl RenderRead for ObservedRead<'_> {
    fn render_header_raw(&self) -> &str {
        self.read.render_header_raw()
    }
    fn track_ids_for_render(
        &self,
        sequence: &EntityId,
        declared_tracks: usize,
        limit: usize,
    ) -> Result<Vec<EntityId>> {
        self.read
            .track_ids_for_render(sequence, declared_tracks, limit)
    }
    fn clip_ids_for_render(
        &self,
        sequence: &EntityId,
        track_id: &EntityId,
        track: &Track,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<EntityId>> {
        self.read
            .clip_ids_for_render(sequence, track_id, track, range, limit)
    }
    fn render_raw_entity(&self, id: &EntityId, remaining: usize) -> Result<Option<String>> {
        assert!(
            self.attempted.borrow_mut().insert(id.clone()),
            "Raw entity was fetched more than once"
        );
        assert_eq!(
            remaining,
            MAX_RAW_BYTES - self.read.header_raw().len() - self.copied.get()
        );
        let result = self.read.render_raw_entity(id, remaining);
        match &result {
            Ok(Some(raw)) => {
                assert!(raw.len() <= remaining);
                self.copied.set(self.copied.get() + raw.len());
            }
            Err(error) if error.code == "RESOURCE_LIMIT" => {
                self.rejected.set(self.rejected.get() + 1)
            }
            _ => {}
        }
        result
    }
}
fn over_budget_raw_capture() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let mut fixture = Fixture::new(&store);
    let clips: Vec<_> = (0..32).map(|start| fixture.place(start, true)).collect();
    let input = fixture.input(32);
    let read = store.project_read(fixture.project.as_str()).unwrap();
    let table = read.read.open_table(RECORDS).unwrap();
    let total: usize = clips
        .iter()
        .map(|id| {
            let row = table
                .get(entity_key(&fixture.project, id).as_str())
                .unwrap()
                .unwrap();
            assert!(row.value().len() < 192 * 1024);
            row.value().len()
        })
        .sum();
    assert!(total > MAX_RAW_BYTES);
    let first = table
        .get(entity_key(&fixture.project, &clips[0]).as_str())
        .unwrap()
        .unwrap();
    assert_eq!(
        read.render_raw_entity(&clips[0], first.value().len() - 1)
            .unwrap_err()
            .code,
        "RESOURCE_LIMIT"
    );
    let observed = ObservedRead {
        read: &read,
        copied: Cell::new(0),
        rejected: Cell::new(0),
        attempted: RefCell::new(BTreeSet::new()),
    };
    assert_eq!(
        media::capture(&observed, &input).unwrap_err().code,
        "RESOURCE_LIMIT"
    );
    assert_eq!(
        observed.rejected.get(),
        1,
        "Actual raw reader must reject before returning the over-budget String"
    );
    assert!(observed.copied.get() + read.header_raw().len() <= MAX_RAW_BYTES);
    assert!(
        observed.attempted.borrow().len() < clips.len(),
        "Capture must stop before fetching all full indexed rows"
    );
    println!(
        "MEDIA_COMPILER_RAW_ADMISSION: actual32storedDisabledClips total={} copied={} rawAttempts={} rejectedBeforeReturn=1 legacyPrefetch=0",
        total,
        observed.copied.get(),
        observed.attempted.borrow().len()
    );
}
fn replace_raw(store: &Store, project: &ProjectId, id: &EntityId, raw: &str) {
    let write = store.db.begin_write().unwrap();
    {
        let mut table = write.open_table(RECORDS).unwrap();
        table
            .insert(entity_key(project, id).as_str(), raw.as_bytes())
            .unwrap();
    }
    write.commit().unwrap();
}
fn raw_reference_extensions() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let mut fixture = Fixture::new(&store);
    let clip = fixture.place(0, false);
    let input = fixture.input(1);
    let read = store.project_read(fixture.project.as_str()).unwrap();
    let original = read.raw_entity(&clip).unwrap().unwrap();
    let positive = media::capture(&read, &input).unwrap();
    assert_eq!(positive.clips.len(), 1);
    assert_eq!(positive.clips[0].clip_id, clip);
    drop(read);
    let original_value: Value = serde_json::from_str(&original).unwrap();
    for (enabled, scope) in [(true, "entity"), (false, "entity"), (true, "annotation")] {
        let mut changed = original_value.clone();
        changed["enabled"] = json!(enabled);
        changed["semantic_scope"] = json!(scope);
        changed["references"][1]["required_semantics"] = json!("future-transform.v2");
        let raw = serde_json::to_string(&changed).unwrap();
        let legacy: EntityEnvelope = serde_json::from_str(&raw).unwrap();
        legacy.validate().unwrap();
        legacy.semantics().unwrap();
        assert_eq!(legacy.references[1].role, "source");
        assert_eq!(
            serde_json::to_value(&legacy.references[1]).unwrap(),
            json!({"role":"source","target":changed["references"][1]["target"]})
        );
        replace_raw(&store, &fixture.project, &clip, &raw);
        let read = store.project_read(fixture.project.as_str()).unwrap();
        assert_eq!(
            media::capture(&read, &input).unwrap_err().code,
            "PROJECT_READ_ONLY",
            "Raw reference semantics must be rejected for enabled={enabled}, scope={scope}"
        );
    }
    // Duplicate unknown keys survive the legacy decoder too; strict raw references reject them.
    let source_target = original_value["references"][1]["target"].as_str().unwrap();
    let reference = format!("{{\"role\":\"source\",\"target\":\"{source_target}\"}}");
    let duplicate = format!(
        "{{\"role\":\"source\",\"target\":\"{source_target}\",\"required_semantics\":1,\"required_semantics\":2}}"
    );
    let compact = serde_json::to_string(&original_value).unwrap();
    let duplicated_raw = compact.replace(&reference, &duplicate);
    assert_ne!(duplicated_raw, compact);
    assert!(serde_json::from_str::<EntityEnvelope>(&duplicated_raw).is_ok());
    replace_raw(&store, &fixture.project, &clip, &duplicated_raw);
    assert_eq!(
        media::capture(
            &store.project_read(fixture.project.as_str()).unwrap(),
            &input
        )
        .unwrap_err()
        .code,
        "PROJECT_READ_ONLY"
    );
    for (field, wrong_type) in [("role", json!(1)), ("target", json!({"id":source_target}))] {
        let mut changed = original_value.clone();
        changed["references"][1][field] = wrong_type;
        replace_raw(
            &store,
            &fixture.project,
            &clip,
            &serde_json::to_string(&changed).unwrap(),
        );
        assert_eq!(
            media::capture(
                &store.project_read(fixture.project.as_str()).unwrap(),
                &input
            )
            .unwrap_err()
            .code,
            "PROJECT_READ_ONLY"
        );
    }
    replace_raw(&store, &fixture.project, &clip, &original);
    let restored = media::capture(
        &store.project_read(fixture.project.as_str()).unwrap(),
        &input,
    )
    .unwrap();
    assert_eq!(restored.clips.len(), positive.clips.len());
    assert_eq!(
        restored
            .facts
            .iter()
            .map(|fact| (&fact.id, &fact.sha256))
            .collect::<Vec<_>>(),
        positive
            .facts
            .iter()
            .map(|fact| (&fact.id, &fact.sha256))
            .collect::<Vec<_>>()
    );
    println!(
        "MEDIA_COMPILER_RAW_REFERENCES: actualStorePositiveAndRestored=2 unknownAcrossEnabledDisabledAnnotation=3 duplicateUnknown=1 invalidTypes=2 legacyCodecUnchanged"
    );
}
fn mixed_track_capture_preserves_original_clocks_and_rows() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let mut fixture = Fixture::new(&store);
    let time = |numerator, denominator| Rational::new(numerator, denominator).unwrap();
    let result = fixture.apply(
        ProjectEditIntent::Sequence(SequenceInput {
            name: "Mixed tracks".into(),
            settings: SequenceSettings {
                width: 64,
                height: 48,
                frame_rate: time(4, 1),
                sample_rate: 48_000,
                audio_layout: "stereo".into(),
            },
            tracks: [TrackKind::Video, TrackKind::Audio, TrackKind::Video]
                .into_iter()
                .enumerate()
                .map(|(index, kind)| TrackInput {
                    kind,
                    name: format!("Track {index}"),
                })
                .collect(),
        }),
        false,
    );
    let sequence =
        serde_json::from_value::<EntityId>(result["created"]["sequence_id"].clone()).unwrap();
    let expected = [0, 2, 5];
    let mut asset_ids = Vec::new();
    for (index, physical_stream) in expected.into_iter().enumerate() {
        let track =
            serde_json::from_value::<EntityId>(result["created"]["tracks"][index]["id"].clone())
                .unwrap();
        let asset_id = Uuid::new_v4().to_string();
        let kind = if index == 1 {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
        let origin = index as i64 + 1;
        fixture.apply(
            ProjectEditIntent::Place(
                Box::new(PlaceInput {
                    sequence_id: sequence.clone(),
                    track_id: track,
                    source: PlaceSource::Registered {
                        asset_id: asset_id.clone(),
                        stream_index: physical_stream,
                    },
                    source_range: SourceSpan {
                        start: time(origin, 1),
                        end: time(origin + 2, 1),
                    },
                    sequence_start: Rational::ZERO,
                    collision: "reject".into(),
                }),
                SourceBinding::Registered {
                    asset_id: asset_id.clone(),
                    content_sha256: "ab".repeat(32),
                    probe_sha256: "cd".repeat(32),
                    stream_index: physical_stream,
                    media_kind: kind,
                    extent_verification: "pending".into(),
                },
            ),
            false,
        );
        asset_ids.push((asset_id, kind, physical_stream, origin));
    }
    let read = store.project_read(fixture.project.as_str()).unwrap();
    let revision = read.header().revision;
    let range = nw_domain::time::TimeRange {
        start: time(1, 4),
        end: time(3, 4),
    };
    let graph = media::capture_mixed(&read, &sequence, revision, range).unwrap();
    assert_eq!(graph.tracks.len(), 3);
    let frame = media::capture_mixed_frame(&read, &sequence, revision, range.start).unwrap();
    assert_eq!(frame.tracks.len(), 2);
    assert_eq!(frame.range.end, time(1, 2));
    assert_eq!(
        media::visible_occurrence(&frame, range.start).unwrap().id,
        graph.tracks[2].clips[0].id
    );
    assert_eq!(
        graph.header_sha256,
        domain::hash(read.header_raw().as_bytes())
    );
    for (track, (id, kind, index, origin)) in graph.tracks.iter().zip(asset_ids) {
        assert_eq!(track.kind, kind);
        assert_eq!(track.clips.len(), 1);
        let clip = &track.clips[0];
        assert_eq!(clip.sequence_range, range);
        assert_eq!(clip.source_range.start, time(4 * origin + 1, 4));
        assert_eq!(clip.source_range.end, time(4 * origin + 3, 4));
        assert!(
            matches!(&clip.source, media::OccurrenceSource::Registered { asset_id, stream_index, .. } if asset_id == &id && *stream_index == index)
        );
        assert!(graph.facts.iter().any(|fact| fact.id == clip.source_id));
    }
    let schedule = media::compile_mixed_graph(&graph).unwrap();
    assert_eq!(schedule.output_frames, "2");
    assert_eq!(schedule.output_samples, "24000");
    assert_eq!(schedule.video.len(), 1);
    assert_eq!(schedule.video[0].frame_count, "2");
    assert_eq!(
        schedule.video[0].window.as_ref().unwrap().occurrence_id,
        graph.tracks[2].clips[0].occurrence_id
    );
    assert_eq!(schedule.audio.len(), 1);
    assert_eq!(schedule.audio[0].start_sample, "0");
    assert_eq!(schedule.audio[0].sample_count, "24000");
    assert_eq!(
        schedule.audio[0].window.occurrence_id,
        graph.tracks[1].clips[0].occurrence_id
    );

    let mut large = graph.clone();
    large.settings.width = 1920;
    large.settings.height = 1080;
    large.range.end = time(600, 1);
    let mut bindings = large
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .map(|clip| {
            let media::OccurrenceSource::Registered {
                asset_id,
                content_sha256,
                probe_sha256,
                stream_index,
                media_kind,
            } = &clip.source
            else {
                panic!("Expected a registered occurrence");
            };
            media::BoundSource {
                asset_id: asset_id.clone(),
                asset_row_sha256: "ef".repeat(32),
                content_sha256: content_sha256.clone(),
                probe_sha256: probe_sha256.clone(),
                stream_index: *stream_index,
                media_kind: *media_kind,
                codec: if *media_kind == TrackKind::Audio {
                    "pcm_s16le"
                } else {
                    "ffv1"
                }
                .into(),
                time_base: time(1, 1000),
                asset_origin: Rational::ZERO,
                stream_start: Rational::ZERO,
            }
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    let request = serde_json::from_value(json!({
        "contract_kind":"media_v2","sequence_id":sequence,"revision":revision,"range":large.range,
        "output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le","destination":{"kind":"download"}}
    })).unwrap();
    let tools = media::MixedTools {
        ffmpeg: media::ComponentIdentity {
            sha256: "ab".repeat(32),
            bytes: "16".into(),
        },
        ffprobe: media::ComponentIdentity {
            sha256: "cd".repeat(32),
            bytes: "16".into(),
        },
        profile_sha256: "aa".repeat(32),
    };
    let root = Uuid::new_v4().to_string();
    let review = Uuid::new_v4().to_string();
    let mut plan = media::MediaPlanV2::compile(
        &request,
        large,
        bindings,
        tools,
        media::MixedReview {
            root_id: &root,
            application_build: &"11".repeat(32),
            principal: "compiler-capture",
            id: &review,
            now: 100,
        },
    )
    .unwrap();
    plan.validate_hash().unwrap();
    let raw = serde_json::to_string(&plan).unwrap();
    use nw_domain::media_execution::{ReviewedMediaPlan, decode_reviewed_media_plan};
    let ReviewedMediaPlan::V2(decoded) = decode_reviewed_media_plan(&raw).unwrap() else {
        panic!("Mixed review selected the wrong plan codec");
    };
    assert_eq!(decoded.hash, plan.hash);
    for invalid in [
        format!(r#"{{"envelope_vers\u0069on":2,{}"#, &raw[1..]),
        raw.replacen("\"media_v2\"", "\"media_v1\"", 1),
        raw.replacen("\"envelope_version\":2", "\"envelope_version\":null", 1),
    ] {
        assert!(decode_reviewed_media_plan(&invalid).is_err());
    }
    assert!(
        plan.payload
            .resources
            .decoded_output_bytes
            .parse::<u64>()
            .unwrap()
            > u64::from(u32::MAX)
    );
    assert!(plan.payload.graph.video.last().unwrap().window.is_none());
    plan.payload.graph.audio[0].sample_count = "23999".into();
    assert!(
        plan.digest().is_err(),
        "An edited schedule cannot be legitimized by recomputing the digest"
    );

    let mut legacy = fixture.input(1);
    legacy.sequence_id = sequence;
    assert_eq!(
        media::capture(&read, &legacy).unwrap_err().code,
        "UNSUPPORTED_AUDIO_TRACK"
    );
}

pub(super) fn run() {
    over_budget_raw_capture();
    raw_reference_extensions();
    mixed_track_capture_preserves_original_clocks_and_rows();
}
