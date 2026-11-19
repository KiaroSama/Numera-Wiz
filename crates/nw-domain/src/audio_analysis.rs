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

pub const MAX_TRACKS: usize = 16;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackSelection {
    pub asset_id: String,
    #[schemars(length(min = 2, max = 16))]
    pub stream_indices: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioSignature {
    pub stream_index: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_layout: Option<String>,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub decoded_pcm_sha256: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioTrackReport {
    pub asset_id: String,
    pub source_sha256: String,
    pub tracks: Vec<AudioSignature>,
    /// Groups of original stream indices with the same decoded signal and declared format.
    pub duplicate_groups: Vec<Vec<u32>>,
}
