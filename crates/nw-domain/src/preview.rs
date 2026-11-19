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
pub const OBSERVE_COMMAND: &str = "previews.observe";
pub const MAX_INLINE_PNG_BYTES: usize = 96 * 1024;
pub const MAX_OBSERVATION_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_MCP_OBSERVATION_BYTES: usize = 768 * 1024;
pub const PNG_MIME_TYPE: &str = "image/png";

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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObserveInput {
    Still {
        target: ObserveTarget,
        frame_selection: FrameSelection,
        source_policy: SourcePolicy,
        display_profile: DisplayProfile,
        fit: Fit,
        encoding: Encoding,
        delivery: Delivery,
    },
}
impl ObserveInput {
    pub fn source(asset_id: String, stream_index: u32, frame_index: u32) -> Self {
        Self::Still {
            target: ObserveTarget::RegisteredSource {
                asset_id,
                stream_index,
                frame_index,
            },
            frame_selection: FrameSelection::DecodedIndex,
            source_policy: SourcePolicy::OriginalRequired,
            display_profile: DisplayProfile::SourceMonitorRgba8,
            fit: Fit::Contain,
            encoding: Encoding::Png {},
            delivery: Delivery::Inline,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObserveTarget {
    RegisteredSource {
        asset_id: String,
        stream_index: u32,
        #[schemars(range(max = 1000000))]
        frame_index: u32,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FrameSelection {
    DecodedIndex,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourcePolicy {
    OriginalRequired,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayProfile {
    SourceMonitorRgba8,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Contain,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "format", rename_all = "snake_case", deny_unknown_fields)]
pub enum Encoding {
    Png {},
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Inline,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameTiming {
    pub time_base: Rational,
    pub pts: Option<Rational>,
    pub duration: Option<Rational>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationMetadata {
    pub observation_id: String,
    pub target: ObserveTarget,
    pub actual_frame_index: u32,
    pub timing: Option<FrameTiming>,
    pub source_sha256: String,
    pub component_sha256: String,
    pub graph_sha256: String,
    pub frame_selection: FrameSelection,
    pub source_policy: SourcePolicy,
    pub display_profile: DisplayProfile,
    pub fit: Fit,
    pub encoding: Encoding,
    pub delivery: Delivery,
    pub width: usize,
    pub height: usize,
    pub mime_type: String,
    pub encoded_byte_count: usize,
    pub encoded_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InlineObservation {
    pub metadata: ObservationMetadata,
    #[schemars(length(min = 1, max = 131072))]
    pub png_base64: String,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSegmentInput {
    pub asset_id: String,
    pub video_stream: u32,
    pub audio_stream: Option<u32>,
}

pub const SOURCE_SEGMENT_SECONDS: i64 = 2;
pub const MAX_SOURCE_SEGMENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SOURCE_SEGMENT_FRAGMENTS: usize = 8;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SegmentBytes {
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSegment {
    pub segment_id: String,
    pub source_sha256: String,
    pub selection: SourceSegmentInput,
    pub sha256: String,
    #[schemars(range(min = 1, max = 2097152))]
    pub bytes: u32,
    pub mime_type: String,
    pub initialization: SegmentBytes,
    #[schemars(length(min = 1, max = 8))]
    pub fragments: Vec<SegmentBytes>,
    pub expires_in_seconds: u32,
    pub requested_range: crate::time::TimeRange,
}
