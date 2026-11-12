//! Result-only schema composition for the existing project producers.
//!
//! These documents describe projections, never decode accepted results or stored payloads.
//! In particular, a known payload is decoded by version/kind even when its semantics are
//! read-only. Its typed projection may contain values that a new edit would reject.
use serde_json::{Map, Value, json};

fn object(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let properties: Map<String, Value> = fields
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    let required: Vec<_> = properties.keys().cloned().collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn extend(mut base: Value, fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let properties = base["properties"].as_object_mut().expect("object schema");
    properties.extend(
        fields
            .into_iter()
            .map(|(name, schema)| (name.into(), schema)),
    );
    let required: Vec<_> = properties.keys().cloned().collect();
    base["required"] = json!(required);
    base
}

fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/$defs/{name}")})
}

fn string() -> Value {
    json!({"type":"string"})
}

fn boolean() -> Value {
    json!({"type":"boolean"})
}

fn count() -> Value {
    json!({"type":"integer","minimum":0})
}

fn uint32() -> Value {
    json!({"type":"integer","minimum":0,"maximum":4294967295u64})
}

fn enumeration(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}

fn array(items: Value, maximum: usize) -> Value {
    json!({"type":"array","items":items,"maxItems":maximum})
}

fn nullable(schema: Value) -> Value {
    json!({"anyOf":[schema,{"type":"null"}]})
}

fn definitions() -> Map<String, Value> {
    Map::from_iter([
        (
            "Id".into(),
            json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"}),
        ),
        (
            "Revision".into(),
            json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$","description":"Canonical decimal u64; the numeric bound remains a runtime invariant."}),
        ),
    ])
}

fn document(mut root: Value, definitions: Map<String, Value>) -> Value {
    root["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    root["$defs"] = json!(definitions);
    root
}

fn history() -> Value {
    object([
        ("can_undo", boolean()),
        ("can_redo", boolean()),
        ("retained_operations", count()),
        ("retained_bytes", count()),
        ("history_floor", reference("Revision")),
        ("journal_floor", reference("Revision")),
    ])
}

/// Header-only catalog: the store admits at most 64 headers, irrespective of page syntax.
pub fn catalog() -> Value {
    let mut defs = definitions();
    defs.insert(
        "ProjectSummary".into(),
        object([
            ("id", reference("Id")),
            ("name", string()),
            ("revision", reference("Revision")),
            ("schema_major", uint32()),
            ("envelope_version", uint32()),
            (
                "compatibility",
                enumeration(&["supported", "read_only_unsupported"]),
            ),
        ]),
    );
    let root = object([
        ("schema_version", json!({"const":1})),
        ("project_revision", json!({"type":"null"})),
        ("snapshot_id", reference("Id")),
        ("snapshot_expires_at", reference("Revision")),
        ("projection", json!({"const":"projects.summary.v1"})),
        ("items", array(reference("ProjectSummary"), 64)),
        ("next_cursor", nullable(string())),
        ("event_cursor", json!({"type":"null"})),
        (
            "total_count",
            json!({"type":"integer","minimum":0,"maximum":64}),
        ),
        ("count_accuracy", json!({"const":"exact"})),
    ]);
    document(root, defs)
}

fn entity_common(kind: Value, current: bool) -> Value {
    object([
        ("id", reference("Id")),
        ("kind", kind),
        (
            "kind_version",
            if current {
                json!({"const":1})
            } else {
                uint32()
            },
        ),
        (
            "envelope_version",
            if current {
                json!({"const":1})
            } else {
                uint32()
            },
        ),
        ("owner_id", nullable(reference("Id"))),
        ("enabled", boolean()),
        ("references", array(reference("EntityReference"), 128)),
        ("semantic_scope", string()),
        ("name", string()),
        (
            "payload_sha256",
            json!({"type":"string","pattern":"^[0-9a-f]{64}$"}),
        ),
        (
            "compatibility",
            enumeration(&[
                "supported_or_optional_annotation",
                "read_only_affected_scope",
            ]),
        ),
    ])
}

fn opaque_entity(sequence_only: bool) -> Value {
    let mut schema = extend(
        entity_common(
            if sequence_only {
                json!({"const":"sequence"})
            } else {
                string()
            },
            false,
        ),
        [("payload_json", string())],
    );
    // A current known codec is selected before semantic compatibility is considered.
    // Unknown kinds and unsupported versions retain the original payload as a STRING.
    schema["not"] = json!({"properties":{
        "kind":{"enum":["bin","sequence","track","clip","source_binding"]},
        "kind_version":{"const":1},"envelope_version":{"const":1}
    },"required":["kind","kind_version","envelope_version"]});
    schema
}

fn entity_definitions(defs: &mut Map<String, Value>) {
    defs.insert(
        "Rational".into(),
        object([
            (
                "num",
                json!({"type":"string","pattern":"^(0|-?[1-9][0-9]*)$"}),
            ),
            ("den", json!({"type":"string","pattern":"^[1-9][0-9]*$"})),
        ]),
    );
    defs.insert(
        "SourceSpan".into(),
        object([
            ("start", reference("Rational")),
            ("end", reference("Rational")),
        ]),
    );
    defs.insert(
        "EntityReference".into(),
        object([("role", string()), ("target", reference("Id"))]),
    );
    defs.insert(
        "SequenceSettings".into(),
        object([
            ("width", uint32()),
            ("height", uint32()),
            ("frame_rate", reference("Rational")),
            ("sample_rate", uint32()),
            ("audio_layout", string()),
        ]),
    );
    defs.insert(
        "BinEntity".into(),
        extend(
            entity_common(json!({"const":"bin"}), true),
            [("member_count", count()), ("locked", boolean())],
        ),
    );
    defs.insert(
        "SequenceEntity".into(),
        extend(
            entity_common(json!({"const":"sequence"}), true),
            [
                ("settings", reference("SequenceSettings")),
                ("track_count", count()),
                ("locked", boolean()),
            ],
        ),
    );
    // Track.kind replaces the envelope's "track" discriminator during flattening.
    defs.insert(
        "TrackEntity".into(),
        extend(
            entity_common(enumeration(&["video", "audio"]), true),
            [
                ("sequence_id", reference("Id")),
                ("order", uint32()),
                ("grid", reference("Rational")),
                ("clip_count", count()),
                ("locked", boolean()),
            ],
        ),
    );
    defs.insert(
        "ClipEntity".into(),
        extend(
            entity_common(json!({"const":"clip"}), true),
            [
                ("sequence_id", reference("Id")),
                ("track_id", reference("Id")),
                ("source_id", reference("Id")),
                ("occurrence_id", reference("Id")),
                ("source_range", reference("SourceSpan")),
                ("sequence_start", reference("Rational")),
                ("duration", reference("Rational")),
                ("rate", reference("Rational")),
                ("link_group_id", nullable(reference("Id"))),
                ("locked", boolean()),
            ],
        ),
    );
    defs.insert(
        "SolidSourceEntity".into(),
        extend(
            entity_common(json!({"const":"source_binding"}), true),
            [
                ("source_kind", json!({"const":"solid_color"})),
                (
                    "rgba",
                    json!({"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4}),
                ),
                ("duration", reference("Rational")),
            ],
        ),
    );
    defs.insert(
        "RegisteredSourceEntity".into(),
        extend(
            entity_common(json!({"const":"source_binding"}), true),
            [
                ("source_kind", json!({"const":"registered"})),
                ("asset_id", string()),
                ("content_sha256", string()),
                ("probe_sha256", string()),
                ("stream_index", uint32()),
                ("media_kind", enumeration(&["video", "audio"])),
                ("extent_verification", string()),
            ],
        ),
    );
    defs.insert("OpaqueEntity".into(), opaque_entity(false));
    defs.insert("OpaqueSequenceEntity".into(), opaque_entity(true));
    defs.insert(
        "Entity".into(),
        json!({"anyOf":[reference("BinEntity"),reference("SequenceEntity"),
            reference("TrackEntity"),reference("ClipEntity"),reference("SolidSourceEntity"),
            reference("RegisteredSourceEntity"),reference("OpaqueEntity")]}),
    );
    defs.insert(
        "SequenceProjectionEntity".into(),
        json!({"anyOf":[reference("SequenceEntity"),reference("OpaqueSequenceEntity")]}),
    );
}

fn detail(projection: &str) -> Value {
    object([
        ("schema_version", json!({"const":1})),
        ("project_revision", reference("Revision")),
        ("snapshot_id", reference("Id")),
        ("snapshot_expires_at", reference("Revision")),
        ("projection", json!({"const":projection})),
        ("event_cursor", json!({"type":"null"})),
    ])
}

fn collection(projection: &str, item: &str, accuracy: Value) -> Value {
    extend(
        detail(projection),
        [
            ("items", array(reference(item), 512)),
            ("next_cursor", nullable(string())),
            ("total_count", count()),
            ("count_accuracy", accuracy),
        ],
    )
}

pub fn inspect(sequence_only: bool) -> Value {
    let mut defs = definitions();
    entity_definitions(&mut defs);
    defs.insert("History".into(), history());
    let entity = extend(
        detail("entity"),
        [(
            "entity",
            reference(if sequence_only {
                "SequenceProjectionEntity"
            } else {
                "Entity"
            }),
        )],
    );
    let branches = if sequence_only {
        defs.insert(
            "TrackSummary".into(),
            object([
                ("id", reference("Id")),
                ("kind", enumeration(&["video", "audio"])),
                ("name", string()),
                ("order", uint32()),
                ("clip_count", count()),
            ]),
        );
        defs.insert(
            "ClipSummary".into(),
            object([
                ("id", reference("Id")),
                ("track_id", reference("Id")),
                ("source_id", reference("Id")),
                ("sequence_start", reference("Rational")),
                ("duration", reference("Rational")),
            ]),
        );
        vec![
            entity,
            collection("track_page", "TrackSummary", json!({"const":"exact"})),
            collection(
                "viewport",
                "ClipSummary",
                enumeration(&["exact", "lower_bound"]),
            ),
        ]
    } else {
        defs.insert(
            "ProjectItemSummary".into(),
            object([
                ("id", reference("Id")),
                ("kind", string()),
                ("name", string()),
            ]),
        );
        let summary = extend(
            detail("summary"),
            [
                ("project_id", reference("Id")),
                ("root_bin_id", reference("Id")),
                ("name", string()),
                ("entity_count", count()),
                ("sequence_count", count()),
                ("schema_major", uint32()),
                ("envelope_version", uint32()),
                (
                    "compatibility",
                    enumeration(&["supported", "read_only_unsupported"]),
                ),
                ("history", reference("History")),
                (
                    "writer_status",
                    enumeration(&[
                        "this_owner",
                        "available",
                        "other_owner_read_only",
                        "writer_lease_unavailable_read_only",
                    ]),
                ),
                ("read_only", boolean()),
            ],
        );
        vec![
            summary,
            entity,
            collection("bin_page", "ProjectItemSummary", json!({"const":"exact"})),
        ]
    };
    document(json!({"oneOf":branches}), defs)
}

/// Shapes of the actual edit-created sections; group identifiers are opaque, not entities.
#[derive(Clone, Copy)]
pub enum EditResult {
    Sequence,
    Place,
    Move,
    Link,
    Unlink,
    Split,
    Remove,
    History,
}

pub fn edit(kind: EditResult) -> Value {
    let mut defs = definitions();
    defs.insert("History".into(), history());
    let ids = || array(reference("Id"), super::MAX_CHANGES);
    let created = match kind {
        EditResult::Sequence => object([
            ("sequence_id", reference("Id")),
            (
                "tracks",
                json!({"type":"array","minItems":1,"maxItems":64,"items":object([
                    ("id",reference("Id")),("kind",enumeration(&["video","audio"])),
                ])}),
            ),
        ]),
        EditResult::Place => object([
            ("source_id", reference("Id")),
            (
                "clip_ids",
                json!({"type":"array","items":reference("Id"),"minItems":1,"maxItems":1}),
            ),
        ]),
        EditResult::Link => {
            defs.insert("OpaqueGroup".into(), json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$","description":"Opaque link-group token; not an entity identifier."}));
            let mut clips = ids();
            clips["minItems"] = json!(2);
            clips["uniqueItems"] = json!(true);
            clips["description"] =
                json!("Complete resolved clip closure in ascending canonical ID order.");
            object([
                ("link_group_id", reference("OpaqueGroup")),
                ("clip_ids", clips),
            ])
        }
        EditResult::Unlink => {
            let mut clips = ids();
            clips["minItems"] = json!(1);
            clips["uniqueItems"] = json!(true);
            clips["description"] =
                json!("Complete resolved clip closure in ascending canonical ID order.");
            object([("clip_ids", clips)])
        }
        EditResult::Split => object([("clip_ids", ids()), ("no_change", boolean())]),
        EditResult::Move | EditResult::Remove | EditResult::History => object([]),
    };
    defs.insert("Created".into(), created);
    let common = object([
        ("project_id", reference("Id")),
        ("base_revision", reference("Revision")),
        ("revision", reference("Revision")),
        ("operation_id", reference("Id")),
        ("created", reference("Created")),
        ("changed_ids", ids()),
        ("history", reference("History")),
    ]);
    let can_noop = matches!(
        kind,
        EditResult::Move | EditResult::Link | EditResult::Unlink | EditResult::Split
    );
    let mut changed = common.clone();
    changed["properties"]["changed_ids"]["minItems"] = json!(1);
    if matches!(kind, EditResult::Split) {
        changed["properties"]["created"] = object([
            ("clip_ids", {
                let mut clips = ids();
                clips["minItems"] = json!(1);
                clips
            }),
            ("no_change", json!({"const":false})),
        ]);
    }
    let root = if can_noop {
        let mut noop = extend(common, [("no_change", json!({"const":true}))]);
        noop["properties"]["changed_ids"]["maxItems"] = json!(0);
        if matches!(kind, EditResult::Split) {
            noop["properties"]["created"] = object([
                (
                    "clip_ids",
                    json!({"type":"array","items":reference("Id"),"maxItems":0}),
                ),
                ("no_change", json!({"const":true})),
            ]);
        }
        json!({"oneOf":[changed,noop]})
    } else {
        changed
    };
    document(root, defs)
}
