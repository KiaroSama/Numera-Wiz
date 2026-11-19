//! Exact frame schedules for a bounded picture-only retime.
use crate::{DomainError, Result, time::{Rational, TimeRange}};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_FRAMES: usize = 64;
pub const MAX_REVERSE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetimeInput {
    pub asset_id: String,
    pub output: String,
    pub video_stream: u32,
    pub range: TimeRange,
    #[schemars(range(min = 10,max = 800))]
    pub tempo_hundredths: u16,
    pub reverse: bool,
    pub audio: AudioPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioPolicy {
    Drop,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retime {
    pub video_stream: u32,
    pub range: TimeRange,
    pub tempo_hundredths: u16,
    pub reverse: bool,
    #[schemars(length(min = 1, max = 64))]
    pub input_clock_ms: Vec<u32>,
}

pub fn milliseconds(value: Rational) -> Result<u32> {
    let scaled = i128::from(value.numerator()) * 1000;
    let denominator = i128::from(value.denominator());
    if scaled % denominator != 0 {
        return Err(invalid("Select exact nonnegative millisecond frame boundaries"));
    }
    u32::try_from(scaled / denominator).map_err(|_| invalid("Frame time is outside the supported clock range"))
}

impl Retime {
    pub fn output_clock(&self) -> Result<Vec<u32>> {
        self.range.validate()?;
        let start = milliseconds(self.range.start)?;
        let end = milliseconds(self.range.end)?;
        if !(1..=MAX_FRAMES).contains(&self.input_clock_ms.len())
            || !(10..=800).contains(&self.tempo_hundredths)
            || self.input_clock_ms[0] != start
            || *self.input_clock_ms.last().expect("Frame list is nonempty") >= end
            || self.input_clock_ms.windows(2).any(|frames| frames[0] >= frames[1])
        {
            return Err(invalid("Review a valid source frame range and a tempo from 0.10× to 8×"));
        }
        let mut clock: Vec<_> = if self.reverse {
            std::iter::once(end).chain(self.input_clock_ms[1..].iter().rev().copied())
                .map(|boundary| end - boundary).collect()
        } else {
            self.input_clock_ms.iter().map(|pts| pts - start).collect()
        };
        for pts in &mut clock {
            let scaled = u64::from(*pts) * 100;
            let tempo = u64::from(self.tempo_hundredths);
            if scaled % tempo != 0 {
                return Err(invalid("The requested frame timing does not fit the exact Matroska millisecond clock"));
            }
            *pts = u32::try_from(scaled / tempo).map_err(|_| invalid("Output clock is outside the supported range"))?;
        }
        Ok(clock)
    }
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_TIME", message)
}
