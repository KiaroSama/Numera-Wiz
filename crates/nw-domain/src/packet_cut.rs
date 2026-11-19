//! An absolute packet-clock interval for verified stream copy.
use crate::{
    DomainError, Result,
    time::{Rational, TimeRange},
};
use schemars::JsonSchema;
use std::collections::BTreeSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutInput {
    pub asset_id: String,
    pub output: String,
    pub range: TimeRange,
    pub chapters: ChapterPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitInput {
    pub asset_id: String,
    #[schemars(length(min = 2, max = 16))]
    pub outputs: Vec<SplitOutput>,
    pub chapters: ChapterPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitOutput {
    pub output: String,
    pub range: TimeRange,
}

impl SplitInput {
    pub fn validate(&self) -> Result<()> {
        validate_ranges(self.outputs.iter().map(|item| item.range))?;
        let mut outputs = BTreeSet::new();
        for item in &self.outputs {
            if item.output.is_empty() || item.output.len() > 16384 || !outputs.insert(&item.output) {
                return Err(DomainError::new("VALIDATION_ERROR", "Split outputs require distinct bounded destination paths"));
            }
        }
        Ok(())
    }
}

pub fn validate_ranges(ranges: impl ExactSizeIterator<Item = TimeRange>) -> Result<()> {
    if !(2..=16).contains(&ranges.len()) {
        return Err(DomainError::new("VALIDATION_ERROR", "A packet split requires two to sixteen output intervals"));
    }
    let mut previous_end = Rational::ZERO;
    for range in ranges {
        validate(range)?;
        if range.start < previous_end {
            return Err(DomainError::new("INVALID_TIME", "Split intervals must be in source order and cannot overlap"));
        }
        previous_end = range.end;
    }
    Ok(())
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
