//! Place original WAVE cue clocks in a committed sequence occurrence.
use super::{Clip, EntityId, ProjectId, Revision, invalid};
use crate::{
    Result,
    chapters::{WaveCue, WaveCueInventory},
    time::Rational,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub clip_id: EntityId,
    pub revision: Revision,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub project_id: ProjectId,
    pub sequence_id: EntityId,
    pub clip_id: EntityId,
    pub occurrence_id: EntityId,
    pub revision: Revision,
    pub asset_id: String,
    pub source_sha256: String,
    pub sample_rate_hz: u32,
    #[schemars(length(max = 256))]
    pub cues: Vec<ProjectedCue>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectedCue {
    pub original: WaveCue,
    pub source_start: Rational,
    pub source_end: Rational,
    pub sequence_start: Rational,
    pub sequence_end: Rational,
}

pub fn map_cues(clip: &Clip, source: &WaveCueInventory) -> Result<Vec<ProjectedCue>> {
    if clip.rate <= Rational::ZERO
        || clip.sequence_start < Rational::ZERO
        || clip.duration.checked_mul(clip.rate)? != clip.source_range.duration()?
        || source.sample_rate_hz == 0
    {
        return Err(invalid(
            "The registered WAVE occurrence lacks an exact forward clock",
        ));
    }
    let sample_time = |tick| Rational::new(tick, i64::from(source.sample_rate_hz));
    let reciprocal = Rational::new(clip.rate.denominator(), clip.rate.numerator())?;
    let sequence_time = |at: Rational| {
        clip.sequence_start.checked_add(
            at.checked_sub(clip.source_range.start)?
                .checked_mul(reciprocal)?,
        )
    };
    let mut cues = Vec::new();
    for cue in &source.cues {
        let offset = i64::from(cue.sample_offset);
        let start = sample_time(offset)?;
        let (source_start, source_end) =
            if let Some(count) = cue.sample_count.filter(|count| *count > 0) {
                let end = sample_time(offset + i64::from(count))?;
                let start = start.max(clip.source_range.start);
                let end = end.min(clip.source_range.end);
                if end <= start {
                    continue;
                }
                (start, end)
            } else {
                if start < clip.source_range.start || start >= clip.source_range.end {
                    continue;
                }
                (start, start)
            };
        cues.push(ProjectedCue {
            original: cue.clone(),
            source_start,
            source_end,
            sequence_start: sequence_time(source_start)?,
            sequence_end: sequence_time(source_end)?,
        });
    }
    Ok(cues)
}
