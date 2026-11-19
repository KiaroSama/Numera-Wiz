use super::EdlReel;
use crate::{
    DomainError, Result,
    projects::{EntityId, MAX_CHANGES, PlaceInput, PlaceSource, SourceSpan, hash},
    time::Rational,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024;
// The track and root counters share the same atomic edit with two entities per cut.
const MAX_CUTS: usize = (MAX_CHANGES - 2) / 2;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdlImportInput {
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    pub frames_per_second: u32,
    #[schemars(length(min = 1, max = MAX_DOCUMENT_BYTES))]
    pub document: String,
    #[schemars(length(max = MAX_CUTS))]
    pub reels: Vec<EdlReel>,
}

pub struct ImportedEdl {
    pub frames_per_second: Rational,
    pub document_sha256: String,
    pub cuts: Vec<(PlaceInput, Option<String>)>,
}

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("EDL_INVALID", message)
}

fn timecode(text: &str, fps: u32) -> Result<Rational> {
    let bytes = text.as_bytes();
    if bytes.len() != 11 || [2, 5, 8].iter().any(|index| bytes[*index] != b':') {
        return Err(invalid("Each timecode must use HH:MM:SS:FF"));
    }
    let mut values = [0u32; 4];
    for (value, offset) in values.iter_mut().zip([0, 3, 6, 9]) {
        let digits = &bytes[offset..offset + 2];
        if !digits.iter().all(u8::is_ascii_digit) {
            return Err(invalid("Each timecode must use decimal digits"));
        }
        *value = u32::from(digits[0] - b'0') * 10 + u32::from(digits[1] - b'0');
    }
    let [hours, minutes, seconds, frames] = values;
    if hours >= 24 || minutes >= 60 || seconds >= 60 || frames >= fps {
        return Err(invalid("Timecode is outside the selected non-drop frame grid"));
    }
    Rational::new(i64::from(((hours * 60 + minutes) * 60 + seconds) * fps + frames), i64::from(fps))
}

pub fn parse_cmx3600(input: EdlImportInput) -> Result<ImportedEdl> {
    let fps = input.frames_per_second;
    if !matches!(fps, 24 | 25 | 30) {
        return Err(invalid("CMX import admits integer 24, 25 or 30 fps"));
    }
    if input.document.len() > MAX_DOCUMENT_BYTES || input.reels.len() > MAX_CUTS {
        return Err(crate::projects::limit("EDL input exceeds the bounded edit size"));
    }
    let mut reels = BTreeMap::new();
    for reel in input.reels {
        if reel.name.is_empty() || reel.name.len() > 8 || reel.name == "BL"
            || !reel.name.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
            || reel.content_sha256.len() != 64
            || !reel.content_sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("Invalid reel name or SHA-256 binding"));
        }
        if reels.insert(reel.name.clone(), reel).is_some() {
            return Err(invalid("Duplicate reel binding"));
        }
    }
    let mut lines = input.document.lines().map(str::trim).filter(|line| !line.is_empty());
    let title = lines.next().and_then(|line| line.strip_prefix("TITLE:"));
    if !title.is_some_and(|title| !title.trim().is_empty() && title.len() <= 128 && !title.chars().any(char::is_control))
        || lines.next() != Some("FCM: NON-DROP FRAME")
    {
        return Err(invalid("Expected a CMX title and NON-DROP FRAME mode"));
    }

    let mut cuts = Vec::new();
    let mut used = BTreeSet::new();
    let mut previous_end = None;
    for line in lines {
        if cuts.len() == MAX_CUTS {
            return Err(crate::projects::limit("EDL cut count exceeds the atomic edit limit"));
        }
        let fields = line.split_ascii_whitespace().take(9).collect::<Vec<_>>();
        let [event, alias, "V", "C", source_start, source_end, record_start, record_end] = fields.as_slice() else {
            return Err(invalid("Each EDL event must be one picture cut"));
        };
        if *event != format!("{:03}", cuts.len() + 1) {
            return Err(invalid("EDL events must use consecutive three-digit numbers"));
        }
        let source_range = SourceSpan {
            start: timecode(source_start, fps)?,
            end: timecode(source_end, fps)?,
        };
        let sequence_start = timecode(record_start, fps)?;
        let sequence_end = timecode(record_end, fps)?;
        let duration = sequence_end.checked_sub(sequence_start)?;
        if previous_end.is_some_and(|end| end != sequence_start)
            || duration <= Rational::ZERO || source_range.duration()? != duration
        {
            return Err(invalid("Source and record cuts must be positive, unit-rate and record-contiguous"));
        }
        previous_end = Some(sequence_end);
        let (source, content_sha256) = if *alias == "BL" {
            if source_range.start != Rational::ZERO {
                return Err(invalid("Black events must start at zero in source time"));
            }
            (PlaceSource::SolidColor { rgba: [0., 0., 0., 1.], duration }, None)
        } else {
            let reel = reels.get(*alias).ok_or_else(|| invalid("Every picture reel requires an explicit asset, stream and digest binding"))?;
            used.insert(*alias);
            (
                PlaceSource::Registered { asset_id: reel.asset_id.clone(), stream_index: reel.stream_index },
                Some(reel.content_sha256.clone()),
            )
        };
        cuts.push((PlaceInput {
            sequence_id: input.sequence_id.clone(),
            track_id: input.track_id.clone(),
            source,
            source_range,
            sequence_start,
            collision: "reject".into(),
        }, content_sha256));
    }
    if cuts.is_empty() || used.len() != reels.len() {
        return Err(invalid("EDL requires at least one cut and every reel binding must be used"));
    }
    Ok(ImportedEdl {
        frames_per_second: Rational::new(i64::from(fps), 1)?,
        document_sha256: hash(input.document.as_bytes()),
        cuts,
    })
}
