//! Packet and decoded-frame facts on the exact registered stream clock.
use crate::{DomainError, Result, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub const MAX_ROWS: u32 = 64;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Packets,
    Frames,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub asset_id: String,
    pub stream_index: u32,
    pub kind: Kind,
    /// Zero-based position in the observed packet or decoded-frame order.
    pub first: u32,
    #[schemars(range(min = 1, max = 64))]
    pub limit: u32,
}

impl Input {
    pub fn window(&self) -> Result<Range<u64>> {
        if !(1..=MAX_ROWS).contains(&self.limit) {
            return Err(DomainError::new(
                "INVALID_RANGE",
                "Choose one to 64 observation rows",
            ));
        }
        let first = u64::from(self.first);
        Ok(first..first + u64::from(self.limit))
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Detail {
    Packet {
        bytes: String,
        flags: String,
        data_sha256: String,
    },
    Frame {
        key_frame: bool,
        picture_type: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        samples_per_channel: Option<u32>,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub ordinal: String,
    pub pts: Option<Rational>,
    pub dts: Option<Rational>,
    pub duration: Option<Rational>,
    pub detail: Detail,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub selection: Input,
    pub source_sha256: String,
    pub ffprobe_sha256: String,
    pub time_base: Rational,
    /// Number of rows emitted by ffprobe for the selected stream, as a decimal string.
    pub observed: String,
    pub rows: Vec<Row>,
}
