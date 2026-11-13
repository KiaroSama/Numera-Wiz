use super::*;
use projects::{Bin, Clip, EntityEnvelope, Sequence, SourceBinding, Track, TrackKind};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn corrupt(message: &str) -> DomainError { DomainError::new("PROJECT_CORRUPT", message) }
fn unsupported(message: &str) -> DomainError { DomainError::new("UNSUPPORTED_SEMANTIC_NODE", message) }
#[derive(Deserialize)]
struct RawReferenceProjection<'a> {
    #[serde(borrow)]
    references: &'a serde_json::value::RawValue,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictReference<'a> {
    #[serde(borrow)]
    role: std::borrow::Cow<'a, str>,
    #[serde(borrow)]
    target: std::borrow::Cow<'a, str>,
}
struct StrictReferences;
impl<'de> Deserialize<'de> for StrictReferences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ReferencesVisitor;
        impl<'de> serde::de::Visitor<'de> for ReferencesVisitor {
            type Value = StrictReferences;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("at most128 closed role/target references") }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
                let mut count = 0;
                while let Some(reference) = sequence.next_element::<StrictReference<'de>>()? {
                    count += 1;
                    if count > 128 || !["ownership", "semantic_input", "source", "link"].contains(&reference.role.as_ref()) {
                        return Err(serde::de::Error::custom("Unsupported reference count/role"));
                    }
                    projects::validate_id(&reference.target).map_err(serde::de::Error::custom)?;
                }
                Ok(StrictReferences)
            }
        }
        deserializer.deserialize_seq(ReferencesVisitor)
    }
}
fn strict_references(raw: &str) -> Result<()> {
    let projection: RawReferenceProjection<'_> = serde_json::from_str(raw).map_err(|_| DomainError::new("PROJECT_READ_ONLY", "Unsupported raw envelope reference projection"))?;
    serde_json::from_str::<StrictReferences>(projection.references.get()).map_err(|_| DomainError::new("PROJECT_READ_ONLY", "Unsupported raw reference keys/types/roles"))?;
    Ok(())
}
// Legacy payload codecs preserve unknown fields. Rendering admits an explicit closed projection.
fn closed(entity: &EntityEnvelope) -> Result<()> {
    if !entity.enabled || entity.semantic_scope == "annotation" {
        return Ok(());
    }
    if !entity.extensions.is_empty() {
        return Err(unsupported(
            "Compiler does not admit entity envelope extensions",
        ));
    }
    let fields: BTreeMap<String, serde_json::Value> = serde_json::from_str(&entity.payload_json)
        .map_err(|_| corrupt("Invalid compiler payload object"))?;
    let allowed: &[&str] = match entity.kind.as_str() {
        "bin" => &["member_count", "locked"],
        "sequence" => &["settings", "track_count", "locked"],
        "track" => &[
            "sequence_id",
            "kind",
            "order",
            "grid",
            "clip_count",
            "locked",
        ],
        "clip" => &[
            "sequence_id",
            "track_id",
            "source_id",
            "occurrence_id",
            "source_range",
            "sequence_start",
            "duration",
            "rate",
            "link_group_id",
            "locked",
        ],
        "source_binding" => match fields.get("kind").and_then(serde_json::Value::as_str) {
            Some("solid_color") => &["kind", "rgba", "duration", "locked"],
            Some("registered") => &[
                "kind",
                "asset_id",
                "content_sha256",
                "probe_sha256",
                "stream_index",
                "media_kind",
                "extent_verification",
                "locked",
            ],
            _ => return Err(unsupported("Compiler requires a known source binding kind")),
        },
        _ => return Err(unsupported("Compiler requires a known enabled payload")),
    };
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) { return Err(unsupported("Compiler does not admit unknown payload fields")); }
    if let Some(value) = fields.get("locked") && !value.is_boolean() { return Err(corrupt("Invalid lock metadata")); }
    if entity.references.iter().any(|r| r.role == "semantic_input") { return Err(unsupported("Compiler does not yet lower semantic input effects")); }
    let ownership: Vec<_> = entity.references.iter().filter(|r| r.role == "ownership").collect();
    if ownership.len() != usize::from(entity.owner_id.is_some()) || ownership.iter().any(|r| Some(&r.target) != entity.owner_id.as_ref()) { return Err(corrupt("Compiler ownership reference differs from owner identity")); }
    let source = if entity.kind == "clip" { Some(entity.payload::<Clip>("clip")?.source_id) } else { None };
    let sources: Vec<_> = entity.references.iter().filter(|r| r.role == "source").collect();
    if sources.len() != usize::from(source.is_some()) || sources.iter().any(|r| Some(&r.target) != source.as_ref()) { return Err(unsupported("Compiler source references differ from the supported payload")); }
    if entity.kind != "clip" && entity.references.iter().any(|r| r.role == "link") { return Err(unsupported("Link grouping requires clip entities")); }
    Ok(())
}
struct Closure<'a, R> {
    read: &'a R,
    ids: BTreeSet<EntityId>,
    queue: VecDeque<EntityId>,
    entities: BTreeMap<EntityId, EntityEnvelope>,
    facts: Vec<RawFact>,
    bytes: usize,
}
impl<R: RenderRead> Closure<'_, R> {
    fn queue(&mut self, id: &EntityId) -> Result<()> {
        if !self.ids.contains(id) {
            if self.ids.len() >= projects::MAX_CLOSURE {
                return Err(projects::limit(
                    "Compiler semantic closure exceeds256 entities",
                ));
            }
            self.ids.insert(id.clone());
            self.queue.push_back(id.clone());
        }
        Ok(())
    }
    fn dependents(&mut self, id: &EntityId) -> Result<()> {
        let dependents = self.read.dependents(id, projects::MAX_CLOSURE)?;
        if dependents.len() > projects::MAX_CLOSURE {
            return Err(projects::limit(
                "Compiler dependent read exceeds256 entities",
            ));
        }
        let mut unique = BTreeSet::new();
        for dependent in dependents {
            if !unique.insert(dependent.clone()) {
                return Err(corrupt("Dependent index repeats an entity"));
            }
            self.queue(&dependent)?;
        }
        Ok(())
    }
    fn load(&mut self, id: &EntityId) -> Result<()> {
        self.queue(id)?;
        if self.entities.contains_key(id) { return Ok(()); }
        let remaining = MAX_RAW_BYTES.saturating_sub(self.bytes);
        let raw = self.read.render_raw_entity(id, remaining)?.ok_or_else(DomainError::object_not_found)?;
        if raw.len() > remaining { return Err(projects::limit("Compiler raw capture exceeds1MiB")); }
        self.bytes += raw.len();
        strict_references(&raw)?;
        let entity: EntityEnvelope = serde_json::from_str(&raw).map_err(|_| corrupt("Invalid captured entity envelope"))?;
        if entity.id != *id || entity.project_id != self.read.header().id { return Err(corrupt("Compiler entity identity differs from key/project")); }
        entity.validate()?;
        entity.semantics()?;
        closed(&entity)?;
        self.facts.push(RawFact { id: id.clone(), sha256: projects::hash(raw.as_bytes()), bytes: raw.len() });
        self.entities.insert(id.clone(), entity);
        Ok(())
    }
    fn drain(&mut self) -> Result<()> {
        while let Some(id) = self.queue.pop_front() {
            self.load(&id)?;
            let entity = self.entity(&id)?;
            let mut required = Vec::new();
            required.extend(entity.owner_id.iter().cloned());
            required.extend(entity.references.iter().map(|reference| reference.target.clone()));
            let mut group = None;
            if entity.known() {
                match entity.kind.as_str() {
                    "bin" => { entity.payload::<Bin>("bin")?; }
                    "sequence" => { entity.payload::<Sequence>("sequence")?; }
                    "track" => { required.push(entity.payload::<Track>("track")?.sequence_id); }
                    "clip" => {
                        let clip: Clip = entity.payload("clip")?;
                        required.extend([clip.sequence_id, clip.track_id, clip.source_id]);
                        group = clip.link_group_id;
                    }
                    "source_binding" => { entity.payload::<SourceBinding>("source_binding")?; }
                    _ => {}
                }
            }
            for target in required { self.queue(&target)?; }
            if let Some(group) = group { self.dependents(&group)?; }
            self.dependents(&id)?;
        }
        Ok(())
    }
    fn entity(&self, id: &EntityId) -> Result<&EntityEnvelope> { self.entities.get(id).ok_or_else(DomainError::object_not_found) }

}

pub fn capture<R: RenderRead>(read: &R, input: &RenderInput) -> Result<Capture> {
    let header = read.header();
    header.writable()?;
    if header.revision != input.revision {
        return Err(DomainError::new(
            "STALE_REVISION",
            "Render revision differs from committed project",
        ));
    }
    if !header.extensions.is_empty() || header.compatibility != "supported" {
        return Err(unsupported("Compiler requires a supported project header"));
    }
    if header.entity_count > projects::MAX_ENTITIES
        || header.sequence_count == 0
        || header.sequence_count > header.entity_count
    {
        return Err(corrupt("Invalid project sequence/entity counts"));
    }
    let header_raw = read.render_header_raw();
    if header_raw.is_empty() || header_raw.len() > MAX_RAW_BYTES {
        return Err(projects::limit("Compiler header exceeds raw budget"));
    }
    let raw_header: projects::ProjectHeader =
        serde_json::from_str(header_raw).map_err(|_| corrupt("Invalid captured project header"))?;
    if encode(&raw_header)? != encode(header)? {
        return Err(corrupt("Compiler header bytes differ from snapshot header"));
    }
    input.range.validate()?;
    let duration = input.range.end.checked_sub(input.range.start)?;
    if duration > Rational::new(60, 1)? { return Err(projects::limit("Compiler range exceeds60 seconds")); }
    let mut closure = Closure { read, ids: BTreeSet::new(), queue: VecDeque::new(), entities: BTreeMap::new(), facts: Vec::new(), bytes: header_raw.len() };
    closure.queue(&header.root_bin_id)?;
    closure.load(&input.sequence_id)?;
    let sequence_entity = closure.entity(&input.sequence_id)?;
    if !sequence_entity.enabled || sequence_entity.semantic_scope != "entity" || sequence_entity.owner_id.as_ref() != Some(&header.root_bin_id) { return Err(unsupported("Compiler requires an enabled root-owned sequence")); }
    let sequence: Sequence = sequence_entity.payload("sequence")?;
    sequence.settings.validate()?;
    let settings = MediaSettings {
        width: sequence.settings.width,
        height: sequence.settings.height,
        frame_rate: sequence.settings.frame_rate,
        sample_rate: sequence.settings.sample_rate,
        audio_layout: match sequence.settings.audio_layout.as_str() {
            "mono" => AudioLayout::Mono,
            "stereo" => AudioLayout::Stereo,
            _ => {
                return Err(DomainError::new(
                    "UNSUPPORTED_AUDIO_LAYOUT",
                    "Compiler currently admits mono or stereo explicit silence",
                ));
            }
        },
    };
    settings.validate()?;
    let audio_grid = Rational::new(i64::from(settings.sample_rate), 1)?;
    for point in [input.range.start, input.range.end] {
        projects::grid_tick(point, settings.frame_rate)?;
        projects::grid_tick(point, audio_grid)?;
    }
    let track_ids = read.track_ids_for_render(&input.sequence_id, sequence.track_count, MAX_TRACKS)?;
    if track_ids.len() > MAX_TRACKS || track_ids.len() != sequence.track_count { return Err(corrupt("Compiler requires complete bounded sequence tracks")); }
    let mut unique_tracks = BTreeSet::new();
    let mut orders = BTreeSet::new();
    let mut enabled_video = None;
    for id in &track_ids {
        if !unique_tracks.insert(id.clone()) { return Err(corrupt("Compiler track index repeats identity")); }
        closure.load(id)?;
        let entity = closure.entity(id)?;
        let track: Track = entity.payload("track")?;
        if track.sequence_id != input.sequence_id || entity.owner_id.as_ref() != Some(&input.sequence_id) || !orders.insert(track.order) { return Err(corrupt("Compiler track ownership/order differs")); }
        let expected_grid = match track.kind { TrackKind::Video => settings.frame_rate, TrackKind::Audio => audio_grid };
        if track.grid != expected_grid { return Err(corrupt("Track grid differs from sequence settings")); }
        if entity.enabled && entity.semantic_scope != "annotation" {
            if entity.semantic_scope != "entity" {
                return Err(unsupported("Compiler requires entity-scoped tracks"));
            }
            match track.kind {
                TrackKind::Audio => return Err(DomainError::new("UNSUPPORTED_AUDIO_TRACK", "Enabled source audio is not the explicit silence route")),
                TrackKind::Video => { if enabled_video.replace((id.clone(), track)).is_some() { return Err(DomainError::new("UNSUPPORTED_LAYERING", "Compiler currently admits one enabled video track")); } }
            }
        }
    }
    let (video, track) = enabled_video.ok_or_else(|| DomainError::new("VIDEO_REQUIRED", "One enabled video track is required"))?;
    let intersecting = read.clip_ids_for_render(&input.sequence_id, &video, &track, input.range, MAX_CLIPS)?;
    if intersecting.len() > MAX_CLIPS { return Err(projects::limit("Compiler range exceeds128 clips")); }
    if intersecting.len() > track.clip_count { return Err(corrupt("Compiler interval count exceeds declared track clip count")); }
    let mut clip_ids = BTreeSet::new();
    for id in &intersecting {
        if !clip_ids.insert(id.clone()) { return Err(corrupt("Compiler interval index repeats identity")); }
        closure.queue(id)?;
    }
    closure.drain()?;
    let root = closure.entity(&header.root_bin_id)?;
    if root.kind != "bin"
        || root.owner_id.is_some()
        || !root.enabled
        || root.semantic_scope != "entity"
    {
        return Err(unsupported("Compiler requires an enabled root bin"));
    }
    for entity in closure
        .entities
        .values()
        .filter(|entity| entity.enabled && entity.semantic_scope != "annotation")
    {
        for reference in entity.references.iter().filter(|r| r.role == "link") {
            if closure.entity(&reference.target)?.kind != "clip" {
                return Err(corrupt("Compiler clip link target is not a clip"));
            }
        }
    }
    if closure.entities.len() > header.entity_count { return Err(corrupt("Captured entity count exceeds project header")); }
    let mut clips = Vec::new();
    let mut occurrences = BTreeSet::new();
    for id in &intersecting {
        let entity = closure.entity(id)?;
        let clip: Clip = entity.payload("clip")?;
        if clip.sequence_id != input.sequence_id
            || clip.track_id != video
            || entity.owner_id.as_ref() != Some(&video)
            || !occurrences.insert(clip.occurrence_id.clone())
        {
            return Err(corrupt("Compiler clip interval identity/ownership differs"));
        }
        if clip.duration <= Rational::ZERO
            || clip.rate <= Rational::ZERO
            || clip.source_range.start < Rational::ZERO
            || clip.source_range.duration()? != clip.duration.checked_mul(clip.rate)?
        {
            return Err(corrupt("Compiler clip source extent/rate map differs"));
        }
        let end = clip.end()?;
        if end <= input.range.start || clip.sequence_start >= input.range.end {
            return Err(corrupt(
                "Compiler interval index returned a nonintersecting clip",
            ));
        }
        for point in [clip.sequence_start, end] {
            projects::grid_tick(point, settings.frame_rate)?;
            projects::grid_tick(point, audio_grid)?;
        }
        let source_entity = closure.entity(&clip.source_id)?;
        if source_entity.owner_id.as_ref() != Some(&header.root_bin_id)
            || !entity
                .references
                .iter()
                .any(|r| r.role == "source" && r.target == clip.source_id)
        {
            return Err(corrupt("Compiler source ownership/reference differs"));
        }
        if entity
            .references
            .iter()
            .any(|r| r.role == "source" && r.target != clip.source_id)
        {
            return Err(unsupported(
                "Clip has an additional source semantic reference",
            ));
        }
        if !entity.enabled || entity.semantic_scope == "annotation" {
            continue;
        }
        if entity.semantic_scope != "entity"
            || source_entity.semantic_scope != "entity"
            || !source_entity.enabled
        {
            return Err(unsupported(
                "Enabled clip requires an enabled entity-scoped source",
            ));
        }
        let source: SourceBinding = source_entity.payload("source_binding")?;
        let color = match source {
            SourceBinding::Registered { .. } => {
                return Err(DomainError::new(
                    "UNSUPPORTED_REGISTERED_INPUT",
                    "Registered media requires a qualified held-input lowering",
                ));
            }
            SourceBinding::SolidColor { rgba, duration } => {
                if duration <= Rational::ZERO || clip.source_range.end > duration {
                    return Err(corrupt("Color source range exceeds finite source extent"));
                }
                if rgba[3] != 1.0 || rgba[..3].iter().any(|v| *v != 0.0 && *v != 1.0) {
                    return Err(DomainError::new(
                        "UNSUPPORTED_COLOR_PRECISION",
                        "Compiler admits only opaque sRGB corner colors",
                    ));
                }
                match (rgba[0] == 1.0, rgba[1] == 1.0, rgba[2] == 1.0) {
                    (false, false, false) => CornerColor::Black,
                    (true, false, false) => CornerColor::Red,
                    (false, true, false) => CornerColor::Green,
                    (false, false, true) => CornerColor::Blue,
                    (true, true, false) => CornerColor::Yellow,
                    (true, false, true) => CornerColor::Magenta,
                    (false, true, true) => CornerColor::Cyan,
                    (true, true, true) => CornerColor::White,
                }
            }
        };
        clips.push(ColorClip {
            clip_id: entity.id.clone(),
            source_id: clip.source_id,
            range: TimeRange {
                start: clip.sequence_start.max(input.range.start),
                end: end.min(input.range.end),
            },
            color,
        });
    }
    // Cross-check all clips reached through semantic dependents against the complete interval index.
    for entity in closure
        .entities
        .values()
        .filter(|entity| entity.kind == "clip" && entity.known())
    {
        let clip: Clip = entity.payload("clip")?;
        if clip.sequence_id == input.sequence_id
            && clip.track_id == video
            && clip.sequence_start < input.range.end
            && clip.end()? > input.range.start
            && !clip_ids.contains(&entity.id)
        {
            return Err(corrupt(
                "Compiler interval index omitted a captured intersecting clip",
            ));
        }
    }
    clips.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(a.clip_id.cmp(&b.clip_id))
    });
    for pair in clips.windows(2) {
        if pair[0].range.end > pair[1].range.start {
            return Err(DomainError::new(
                "UNSUPPORTED_OVERLAP",
                "Compiler requires nonoverlapping enabled clips",
            ));
        }
    }
    closure.facts.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Capture {
        project_id: header.id.clone(),
        project_revision: header.revision,
        sequence_id: input.sequence_id.clone(),
        range: input.range,
        settings,
        header_sha256: projects::hash(header_raw.as_bytes()),
        header_bytes: header_raw.len(),
        facts: closure.facts,
        clips,
        output: input.output.clone(),
    })
}
