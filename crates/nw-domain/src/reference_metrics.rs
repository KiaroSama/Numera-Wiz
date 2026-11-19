//! A reference comparison over explicitly aligned decoded video windows.
use crate::{DomainError, Result, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub const MAX_COMPARISON_FRAMES: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub asset_id: String,
    pub stream_index: u32,
    /// The first decoded frame, counted from zero in original presentation order.
    pub start_frame: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareInput {
    pub reference: Window,
    pub candidate: Window,
    #[schemars(range(min = 1, max = 8))]
    pub frame_count: u32,
}

impl CompareInput {
    pub fn frame_ranges(&self) -> Result<[Range<u64>; 2]> {
        let invalid = || {
            DomainError::new("INVALID_RANGE", "Choose one to eight bounded decoded frames per source")
        };
        if !(1..=MAX_COMPARISON_FRAMES).contains(&self.frame_count) {
            return Err(invalid());
        }
        let window = |source: &Window| -> Result<Range<u64>> {
            let end = source.start_frame.checked_add(u64::from(self.frame_count))
                .filter(|end| *end <= i64::MAX as u64).ok_or_else(invalid)?;
            Ok(source.start_frame..end)
        };
        Ok([window(&self.reference)?, window(&self.candidate)?])
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Psnr {
    Finite { centi_db: i32 },
    Identical,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameScores {
    pub reference_frame: u64,
    pub candidate_frame: u64,
    pub offset: Rational,
    pub psnr: Psnr,
    pub ssim_millionths: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleBasis {
    DecodedRgb8,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparedWindow {
    pub selection: Window,
    pub source_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub reference: ComparedWindow,
    pub candidate: ComparedWindow,
    pub sample_basis: SampleBasis,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<FrameScores>,
}
