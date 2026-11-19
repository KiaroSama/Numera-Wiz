//! Actual FFmpeg measurements for a selected registered audio stream.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VolumeInput {
    pub asset_id: String,
    pub stream_index: u32,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decibels {
    Finite { micro_db: i64 },
    Silence,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VolumeMeasurement {
    pub asset_id: String,
    pub source_sha256: String,
    pub stream_index: u32,
    pub peak: Decibels,
    pub rms: Decibels,
    /// Decoded samples per channel. Use a decimal string on the wire.
    pub samples_per_channel: String,
}
