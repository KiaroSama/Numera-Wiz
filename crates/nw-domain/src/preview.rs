//! A bounded decoded frame from an explicitly selected registered source stream.
use crate::{
    projects::{EntityId, ProjectId, Revision},
    time::Rational,
};
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineFrameInput {
    pub sequence_id: EntityId,
    pub revision: Revision,
    #[serde(deserialize_with = "crate::media::read_rational")]
    pub at: Rational,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FrameOrigin {
    Gap {},
    SolidColor {
        clip_id: EntityId,
    },
    Registered {
        clip_id: EntityId,
        asset_id: String,
        source_sha256: String,
        stream_index: u32,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineFrame {
    pub project_id: ProjectId,
    pub sequence_id: EntityId,
    pub revision: Revision,
    pub at: Rational,
    pub origin: FrameOrigin,
    pub width: usize,
    pub height: usize,
    #[schemars(length(min = 76800, max = 76800))]
    pub rgba_base64: String,
}
