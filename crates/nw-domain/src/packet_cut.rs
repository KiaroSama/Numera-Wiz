//! An absolute packet-clock interval for verified stream copy.
use crate::{
    DomainError, Result,
    time::{Rational, TimeRange},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutInput {
    pub asset_id: String,
    pub output: String,
    pub range: TimeRange,
    pub chapters: ChapterPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChapterPolicy {
    Drop,
}

pub fn ffmpeg_time(value: Rational) -> Result<String> {
    let scaled = i128::from(value.numerator()) * 1_000_000;
    let denominator = i128::from(value.denominator());
    if scaled < 0 || scaled % denominator != 0 || scaled / denominator > i128::from(i64::MAX) {
        return Err(DomainError::new(
            "INVALID_TIME",
            "Packet-cut endpoints must be exact nonnegative FFmpeg microseconds",
        ));
    }
    let us = scaled / denominator;
    Ok(format!("{}.{:06}", us / 1_000_000, us % 1_000_000))
}

pub fn validate(range: TimeRange) -> Result<()> {
    range.validate()?;
    ffmpeg_time(range.start)?;
    ffmpeg_time(range.end)?;
    Ok(())
}
