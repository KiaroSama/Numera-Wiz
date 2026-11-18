//! Capture authoritative mixed sequence inputs before any media is opened.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedCapture {
    pub project_id: ProjectId,
    pub project_revision: Revision,
    pub sequence_id: EntityId,
    pub range: TimeRange,
    pub settings: MediaSettings,
    pub header_sha256: String,
    pub header_bytes: usize,
    pub facts: Vec<RawFact>,
    pub tracks: Vec<CapturedTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapturedTrack {
    pub id: EntityId,
    pub kind: TrackKind,
    pub order: u32,
    pub clips: Vec<CapturedOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapturedOccurrence {
    pub id: EntityId,
    pub occurrence_id: EntityId,
    pub source_id: EntityId,
    pub sequence_range: TimeRange,
    pub source_range: TimeRange,
    pub rate: Rational,
    pub source: OccurrenceSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OccurrenceSource {
    SolidColor { rgb: [u8; 3] },
    Registered {
        asset_id: String,
        content_sha256: String,
        probe_sha256: String,
        stream_index: u32,
        media_kind: TrackKind,
    },
}

pub fn visible_occurrence(capture: &MixedCapture, at: Rational) -> Option<&CapturedOccurrence> {
    capture.tracks.iter().rev().filter(|track| track.kind == TrackKind::Video).find_map(|track| {
        track.clips.iter().find(|clip| clip.sequence_range.contains(at))
    })
}

fn occurrence<R: RenderRead>(
    closure: &Closure<'_, R>,
    root: &EntityId,
    track_id: &EntityId,
    track: &Track,
    id: &EntityId,
    range: TimeRange,
    occurrences: &mut BTreeSet<EntityId>,
) -> Result<Option<CapturedOccurrence>> {
    let entity = closure.entity(id)?;
    let clip: Clip = entity.payload("clip")?;
    if clip.sequence_id != track.sequence_id
        || clip.track_id != *track_id
        || entity.owner_id.as_ref() != Some(track_id)
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
    if end <= range.start || clip.sequence_start >= range.end {
        return Err(corrupt("Compiler interval index returned a nonintersecting clip"));
    }
    for point in [clip.sequence_start, end] {
        projects::grid_tick(point, track.grid)?;
    }
    let source = closure.entity(&clip.source_id)?;
    if source.owner_id.as_ref() != Some(root)
        || !entity.references.iter().any(|r| r.role == "source" && r.target == clip.source_id)
    {
        return Err(corrupt("Compiler source ownership/reference differs"));
    }
    if !entity.enabled || entity.semantic_scope == "annotation" {
        return Ok(None);
    }
    if entity.semantic_scope != "entity" || source.semantic_scope != "entity" || !source.enabled {
        return Err(unsupported("Enabled clip requires an enabled entity-scoped source"));
    }
    let source = match source.payload::<SourceBinding>("source_binding")? {
        SourceBinding::SolidColor { rgba, duration } => {
            if track.kind != TrackKind::Video || duration <= Rational::ZERO || clip.source_range.end > duration {
                return Err(corrupt("Color source requires a finite video occurrence"));
            }
            let [red, green, blue, alpha] = rgba;
            if alpha != 1.0 || [red, green, blue].iter().any(|value| !(0.0..=1.0).contains(value)) {
                return Err(DomainError::new("UNSUPPORTED_COLOR_PRECISION", "Mixed media requires opaque normalized sRGB colors"));
            }
            OccurrenceSource::SolidColor { rgb: [red, green, blue].map(|value| (value * 255.0).round() as u8) }
        }
        SourceBinding::Registered { asset_id, content_sha256, probe_sha256, stream_index, media_kind, .. } => {
            if track.kind != media_kind {
                return Err(corrupt("Registered stream kind differs from the owning track"));
            }
            projects::validate_id(&asset_id)?;
            hash_text(&content_sha256)?;
            hash_text(&probe_sha256)?;
            OccurrenceSource::Registered { asset_id, content_sha256, probe_sha256, stream_index, media_kind }
        }
    };
    let sequence_range = TimeRange {
        start: range.start.max(clip.sequence_start),
        end: range.end.min(end),
    };
    let source_time = |point: Rational| {
        clip.source_range.start.checked_add(point.checked_sub(clip.sequence_start)?.checked_mul(clip.rate)?)
    };
    Ok(Some(CapturedOccurrence {
        id: id.clone(),
        occurrence_id: clip.occurrence_id,
        source_id: clip.source_id,
        sequence_range,
        source_range: TimeRange {
            start: source_time(sequence_range.start)?,
            end: source_time(sequence_range.end)?,
        },
        rate: clip.rate,
        source,
    }))
}

pub fn capture_mixed<R: RenderRead>(
    read: &R,
    sequence_id: &EntityId,
    revision: Revision,
    range: TimeRange,
) -> Result<MixedCapture> {
    range.validate()?;
    capture_mixed_range(read, sequence_id, revision, CaptureRange::Interval(range))
}

pub fn capture_mixed_frame<R: RenderRead>(
    read: &R,
    sequence_id: &EntityId,
    revision: Revision,
    at: Rational,
) -> Result<MixedCapture> {
    capture_mixed_range(read, sequence_id, revision, CaptureRange::VideoFrame(at))
}

fn capture_mixed_range<R: RenderRead>(
    read: &R,
    sequence_id: &EntityId,
    revision: Revision,
    selection: CaptureRange,
) -> Result<MixedCapture> {
    let (header, header_raw) = capture_header(read, revision)?;
    let CaptureSequence { mut closure, settings, audio_grid, track_ids, range } =
        capture_sequence(read, &header.root_bin_id, header_raw.len(), sequence_id, selection)?;
    let mut tracks = Vec::new();
    let mut unique_tracks = BTreeSet::new();
    let mut orders = BTreeSet::new();
    let mut in_range = BTreeSet::new();
    for id in track_ids {
        if !unique_tracks.insert(id.clone()) {
            return Err(corrupt("Compiler track index repeats identity"));
        }
        closure.load(&id)?;
        let entity = closure.entity(&id)?;
        let track: Track = entity.payload("track")?;
        if track.sequence_id != *sequence_id || entity.owner_id.as_ref() != Some(sequence_id) || !orders.insert(track.order) {
            return Err(corrupt("Compiler track ownership/order differs"));
        }
        let grid = match track.kind {
            TrackKind::Audio => audio_grid,
            TrackKind::Video => settings.frame_rate,
        };
        if track.grid != grid {
            return Err(corrupt("Track grid differs from sequence settings"));
        }
        if !entity.enabled || entity.semantic_scope == "annotation"
            || (matches!(selection, CaptureRange::VideoFrame(_)) && track.kind != TrackKind::Video)
        {
            continue;
        }
        if entity.semantic_scope != "entity" {
            return Err(unsupported("Compiler requires entity-scoped tracks"));
        }
        let clip_ids = read.clip_ids_for_render(sequence_id, &id, &track, range, MAX_CLIPS)?;
        if clip_ids.len() > MAX_CLIPS - in_range.len() {
            return Err(projects::limit("Compiler range exceeds128 clips"));
        }
        if clip_ids.len() > track.clip_count {
            return Err(corrupt("Compiler interval count exceeds declared track clip count"));
        }
        for clip_id in &clip_ids {
            if !in_range.insert(clip_id.clone()) {
                return Err(corrupt("Compiler interval index repeats identity"));
            }
            closure.queue(clip_id)?;
        }
        tracks.push((id, track, clip_ids));
    }
    drain_and_check_root(&mut closure, header)?;
    let enabled_tracks = tracks.iter().map(|(id, _, _)| id.clone()).collect::<BTreeSet<_>>();
    for entity in closure.entities.values().filter(|entity| entity.kind == "clip" && entity.known()) {
        let clip: Clip = entity.payload("clip")?;
        if clip.sequence_id == *sequence_id && enabled_tracks.contains(&clip.track_id)
            && clip.sequence_start < range.end && clip.end()? > range.start && !in_range.contains(&entity.id)
        {
            return Err(corrupt("Compiler interval index omitted a captured intersecting clip"));
        }
    }
    let mut occurrences = BTreeSet::new();
    let mut captured = Vec::new();
    for (id, track, clip_ids) in tracks {
        let mut clips = Vec::new();
        for clip_id in clip_ids {
            if let Some(value) = occurrence(&closure, &header.root_bin_id, &id, &track, &clip_id, range, &mut occurrences)? {
                clips.push(value);
            }
        }
        clips.sort_by(|left, right| left.sequence_range.start.cmp(&right.sequence_range.start).then(left.id.cmp(&right.id)));
        if clips.windows(2).any(|pair| pair[0].sequence_range.end > pair[1].sequence_range.start) {
            return Err(DomainError::new("UNSUPPORTED_OVERLAP", "Compiler requires nonoverlapping clips within each track"));
        }
        captured.push(CapturedTrack { id, kind: track.kind, order: track.order, clips });
    }
    captured.sort_by_key(|track| track.order);
    closure.facts.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(MixedCapture {
        project_id: header.id.clone(),
        project_revision: revision,
        sequence_id: sequence_id.clone(),
        range,
        settings,
        header_sha256: projects::hash(header_raw.as_bytes()),
        header_bytes: header_raw.len(),
        facts: closure.facts,
        tracks: captured,
    })
}
