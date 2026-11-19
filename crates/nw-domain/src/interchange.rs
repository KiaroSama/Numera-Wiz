//! Bounded interchange documents for committed project tracks.
mod import;
mod otio;
use crate::{
    DomainError, Result,
    media::{MixedCapture, OccurrenceSource},
    projects::{EntityId, ProjectId, Revision, TrackKind, grid_tick},
    time::{Rational, TimeRange},
};
pub use import::{EdlImportInput, ImportedEdl, MAX_DOCUMENT_BYTES, parse_cmx3600};
pub use otio::{
    ImportedOtio, MAX_OTIO_BYTES, OtioExport, OtioImportInput, OtioSource, export_otio, parse_otio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Clone, Copy)]
pub enum CutTrack {
    Edl(Rational),
    Otio { kind: TrackKind, range: TimeRange },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdlInput {
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    pub revision: Revision,
    pub range: TimeRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdlReel {
    pub name: String,
    pub asset_id: String,
    pub stream_index: u32,
    pub content_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdlExport {
    pub project_id: ProjectId,
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    pub revision: Revision,
    pub range: TimeRange,
    pub frames_per_second: u32,
    pub document: String,
    pub document_sha256: String,
    pub reels: Vec<EdlReel>,
}

fn unsupported(message: &'static str) -> DomainError {
    DomainError::new("EDL_UNREPRESENTABLE", message)
}

fn timecode(time: Rational, rate: Rational) -> Result<String> {
    let fps = u64::try_from(rate.numerator()).map_err(|_| unsupported("Invalid EDL rate"))?;
    let frames = grid_tick(time, rate)
        .map_err(|_| unsupported("EDL boundaries must lie on the selected integer frame grid"))?;
    let seconds = frames / fps;
    if seconds >= 86400 {
        return Err(unsupported("CMX timecode must fit within 24 hours"));
    }
    Ok(format!(
        "{:02}:{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        frames % fps
    ))
}

fn append_cut(
    document: &mut String,
    event: usize,
    reel: &str,
    source: TimeRange,
    record: TimeRange,
    rate: Rational,
) -> Result<()> {
    writeln!(
        document,
        "{event:03}  {reel:8} V     C        {} {} {} {}",
        timecode(source.start, rate)?,
        timecode(source.end, rate)?,
        timecode(record.start, rate)?,
        timecode(record.end, rate)?,
    )
    .map_err(|_| DomainError::new("SERIALIZATION_FAILED", "EDL document serialization failed"))
}

fn append_black(
    document: &mut String,
    event: usize,
    range: TimeRange,
    rate: Rational,
) -> Result<()> {
    append_cut(
        document,
        event,
        "BL",
        TimeRange {
            start: Rational::ZERO,
            end: range.end.checked_sub(range.start)?,
        },
        range,
        rate,
    )
}

pub fn export_cmx3600(capture: &MixedCapture, track_id: &EntityId) -> Result<EdlExport> {
    let rate = capture.settings.frame_rate;
    if rate.denominator() != 1 || !matches!(rate.numerator(), 24 | 25 | 30) {
        return Err(unsupported("CMX export admits integer 24, 25 or 30 fps"));
    }
    capture.range.validate()?;
    timecode(capture.range.start, rate)?;
    timecode(capture.range.end, rate)?;
    let track = capture
        .tracks
        .iter()
        .find(|track| track.id == *track_id && track.kind == TrackKind::Video)
        .ok_or_else(|| unsupported("Select an enabled picture track in the captured sequence"))?;
    if track.clips.len() > crate::media::MAX_CLIPS {
        return Err(crate::projects::limit("EDL exceeds the bounded clip count"));
    }
    let mut document = String::from("TITLE: NUMERA WIZ\nFCM: NON-DROP FRAME\n\n");
    let mut reels: Vec<EdlReel> = Vec::new();
    let mut aliases = BTreeMap::new();
    let mut cursor = capture.range.start;
    let mut event = 1;
    for clip in &track.clips {
        let range = clip.sequence_range;
        if range.start < cursor
            || range.end > capture.range.end
            || range.end <= range.start
            || clip.rate != Rational::new(1, 1)?
            || clip.source_range.end.checked_sub(clip.source_range.start)?
                != range.end.checked_sub(range.start)?
        {
            return Err(unsupported(
                "The selected track requires ordered unit-rate cuts inside the captured range",
            ));
        }
        if cursor < range.start {
            append_black(
                &mut document,
                event,
                TimeRange {
                    start: cursor,
                    end: range.start,
                },
                rate,
            )?;
            event += 1;
        }
        match &clip.source {
            OccurrenceSource::SolidColor { rgb: [0, 0, 0] } => {
                append_black(&mut document, event, range, rate)?
            }
            OccurrenceSource::Registered {
                asset_id,
                stream_index,
                content_sha256,
                media_kind: TrackKind::Video,
                ..
            } => {
                let index = *aliases.entry((asset_id, *stream_index)).or_insert_with(|| {
                    let index = reels.len();
                    reels.push(EdlReel {
                        name: format!("R{:07}", index + 1),
                        asset_id: asset_id.clone(),
                        stream_index: *stream_index,
                        content_sha256: content_sha256.clone(),
                    });
                    index
                });
                let reel = &reels[index];
                if reel.content_sha256 != *content_sha256 {
                    return Err(unsupported(
                        "Occurrences disagree on the registered source identity",
                    ));
                }
                append_cut(
                    &mut document,
                    event,
                    &reel.name,
                    clip.source_range,
                    range,
                    rate,
                )?;
            }
            _ => {
                return Err(unsupported(
                    "CMX cut export requires registered picture or opaque black",
                ));
            }
        }
        event += 1;
        cursor = range.end;
    }
    if cursor < capture.range.end {
        append_black(
            &mut document,
            event,
            TimeRange {
                start: cursor,
                end: capture.range.end,
            },
            rate,
        )?;
    }
    Ok(EdlExport {
        project_id: capture.project_id.clone(),
        sequence_id: capture.sequence_id.clone(),
        track_id: track_id.clone(),
        revision: capture.project_revision,
        range: capture.range,
        frames_per_second: rate.numerator() as u32,
        document_sha256: crate::projects::hash(document.as_bytes()),
        document,
        reels,
    })
}
