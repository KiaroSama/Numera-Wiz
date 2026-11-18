//! Deterministic original-window scheduling for opaque mixed sequences.
use super::*;
use projects::TrackKind;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OriginalWindow {
    pub occurrence_id: EntityId,
    pub range: TimeRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoSpan {
    pub start_frame: String,
    pub frame_count: String,
    /// An empty window emits opaque black for the corresponding output interval.
    pub window: Option<OriginalWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioSpan {
    pub start_sample: String,
    pub sample_count: String,
    pub window: OriginalWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedGraph {
    pub output_frames: String,
    pub output_samples: String,
    pub video: Vec<VideoSpan>,
    /// Uncovered samples are zero. Simultaneous inputs mix in capture track order.
    pub audio: Vec<AudioSpan>,
}

fn validate_capture(capture: &MixedCapture) -> Result<()> {
    capture.range.validate()?;
    capture.settings.validate()?;
    let duration = capture.range.end.checked_sub(capture.range.start)?;
    if duration > Rational::new(i64::from(crate::media_execution::MEDIA_RENDER_TIMEOUT_SECONDS), 1)? {
        return Err(projects::limit("Mixed export range exceeds the 600-second profile"));
    }
    hash_text(&capture.header_sha256)?;
    if capture.header_bytes == 0
        || capture.header_bytes > MAX_RAW_BYTES
        || capture.facts.is_empty()
        || capture.facts.len() > projects::MAX_CLOSURE
        || capture.tracks.len() > MAX_TRACKS
    {
        return Err(projects::limit("Mixed capture exceeds its bounded header, facts or tracks"));
    }
    let mut facts = BTreeSet::new();
    let mut captured_bytes = capture.header_bytes;
    for fact in &capture.facts {
        hash_text(&fact.sha256)?;
        if fact.bytes == 0 || fact.bytes > MAX_RAW_BYTES - captured_bytes || !facts.insert(&fact.id) {
            return Err(invalid("Mixed capture has repeated or out-of-budget entity facts"));
        }
        captured_bytes += fact.bytes;
    }
    if !facts.contains(&capture.sequence_id)
        || capture.facts.windows(2).any(|pair| pair[0].id >= pair[1].id)
    {
        return Err(invalid("Mixed capture facts are missing or unordered"));
    }
    let mut seen = BTreeSet::new();
    let mut clip_ids = BTreeSet::new();
    let mut occurrence_ids = BTreeSet::new();
    let mut last_order = None;
    let audio_grid = Rational::new(i64::from(capture.settings.sample_rate), 1)?;
    for track in &capture.tracks {
        if !facts.contains(&track.id) || !seen.insert(&track.id) || last_order.is_some_and(|order| order >= track.order)
            || track.clips.len() > MAX_CLIPS - clip_ids.len()
        {
            return Err(invalid("Mixed capture tracks differ from the captured identities or bounds"));
        }
        last_order = Some(track.order);
        let grid = match track.kind {
            TrackKind::Video => capture.settings.frame_rate,
            TrackKind::Audio => audio_grid,
        };
        let mut previous_end = capture.range.start;
        for clip in &track.clips {
            clip.sequence_range.validate()?;
            clip.source_range.validate()?;
            if !facts.contains(&clip.id) || !facts.contains(&clip.source_id) || !clip_ids.insert(&clip.id)
                || !occurrence_ids.insert(&clip.occurrence_id) || clip.sequence_range.start < previous_end
                || clip.sequence_range.end > capture.range.end || clip.rate <= Rational::ZERO
                || clip.source_range.end.checked_sub(clip.source_range.start)?
                    != clip.sequence_range.end.checked_sub(clip.sequence_range.start)?.checked_mul(clip.rate)?
            {
                return Err(invalid("Mixed occurrence identity or rational map differs from the capture"));
            }
            match &clip.source {
                OccurrenceSource::SolidColor { .. } if track.kind != TrackKind::Video => {
                    return Err(invalid("Color sources require a video track"));
                }
                OccurrenceSource::Registered { media_kind, .. } if *media_kind != track.kind => {
                    return Err(invalid("Registered source kind differs from its track"));
                }
                _ => {}
            }
            projects::grid_tick(clip.sequence_range.start, grid)?;
            projects::grid_tick(clip.sequence_range.end, grid)?;
            previous_end = clip.sequence_range.end;
        }
    }
    for point in [capture.range.start, capture.range.end] {
        projects::grid_tick(point, capture.settings.frame_rate)?;
        projects::grid_tick(point, audio_grid)?;
    }
    Ok(())
}

fn offset(point: Rational, origin: Rational, grid: Rational) -> Result<u64> {
    projects::grid_tick(point.checked_sub(origin)?, grid)
}

fn original_window(clip: &CapturedOccurrence, range: TimeRange) -> Result<OriginalWindow> {
    let original = |point: Rational| {
        clip.source_range.start.checked_add(
            point.checked_sub(clip.sequence_range.start)?.checked_mul(clip.rate)?,
        )
    };
    Ok(OriginalWindow {
        occurrence_id: clip.occurrence_id.clone(),
        range: TimeRange {
            start: original(range.start)?,
            end: original(range.end)?,
        },
    })
}

fn video_spans(capture: &MixedCapture) -> Result<Vec<VideoSpan>> {
    let grid = capture.settings.frame_rate;
    let mut boundaries = BTreeSet::from([capture.range.start, capture.range.end]);
    for clip in capture.tracks.iter().filter(|track| track.kind == TrackKind::Video).flat_map(|track| &track.clips) {
        boundaries.extend([clip.sequence_range.start, clip.sequence_range.end]);
    }
    let mut visible: Vec<(Option<&CapturedOccurrence>, TimeRange)> = Vec::new();
    for (start, end) in boundaries.iter().zip(boundaries.iter().skip(1)) {
        let active = visible_occurrence(capture, *start);
        if let Some((previous, range)) = visible.last_mut()
            && previous.map(|clip| &clip.occurrence_id) == active.map(|clip| &clip.occurrence_id)
        {
            range.end = *end;
        } else {
            visible.push((active, TimeRange { start: *start, end: *end }));
        }
    }
    if visible.len() > MAX_SEGMENTS {
        return Err(projects::limit("Mixed sequence exceeds the video span budget"));
    }
    visible.into_iter().map(|(clip, range)| {
        Ok(VideoSpan {
            start_frame: offset(range.start, capture.range.start, grid)?.to_string(),
            frame_count: offset(range.end, range.start, grid)?.to_string(),
            window: clip.map(|clip| original_window(clip, range)).transpose()?,
        })
    }).collect()
}

/// Compile an actual capture into frame/sample output ranges before execution planning.
pub fn compile_mixed_graph(capture: &MixedCapture) -> Result<MixedGraph> {
    validate_capture(capture)?;
    let video = video_spans(capture)?;
    let audio_grid = Rational::new(i64::from(capture.settings.sample_rate), 1)?;
    let audio = capture.tracks.iter().filter(|track| track.kind == TrackKind::Audio)
        .flat_map(|track| &track.clips)
        .map(|clip| {
            let range = clip.sequence_range;
            Ok(AudioSpan {
                start_sample: offset(range.start, capture.range.start, audio_grid)?.to_string(),
                sample_count: offset(range.end, range.start, audio_grid)?.to_string(),
                window: original_window(clip, range)?,
            })
        }).collect::<Result<_>>()?;
    Ok(MixedGraph {
        output_frames: offset(capture.range.end, capture.range.start, capture.settings.frame_rate)?.to_string(),
        output_samples: offset(capture.range.end, capture.range.start, audio_grid)?.to_string(),
        video,
        audio,
    })
}
