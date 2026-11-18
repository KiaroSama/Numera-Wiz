//! A bounded decoded frame from an explicitly selected registered source stream.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 90;
pub const RGBA_BYTES: usize = WIDTH * HEIGHT * 4;
pub const MAX_FRAME: u32 = 1_000_000;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameInput {
    pub asset_id: String,
    pub stream_index: u32,
    #[schemars(range(max = 1000000))]
    pub frame: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub asset_id: String,
    pub source_sha256: String,
    pub stream_index: u32,
    pub frame: u32,
    pub width: usize,
    pub height: usize,
    #[schemars(length(min = 76800, max = 76800))]
    pub rgba_base64: String,
}
