//! A bounded decoded frame from an explicitly selected registered source stream.
use crate::{
    DomainError, Result,
    media::AudioLayout,
    projects::{EntityId, ProjectId, Revision},
    time::{Rational, TimeRange},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 90;
pub const RGBA_BYTES: usize = WIDTH * HEIGHT * 4;
pub const MAX_FRAME: u32 = 1_000_000;
pub const OBSERVE_COMMAND: &str = "previews.observe";
pub const MAX_INLINE_PNG_BYTES: usize = 96 * 1024;
pub const MAX_INLINE_WAVE_BYTES: usize = 96 * 1024;
pub const MAX_OBSERVATION_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_MCP_OBSERVATION_BYTES: usize = 768 * 1024;
pub const PNG_MIME_TYPE: &str = "image/png";
pub const WAVE_MIME_TYPE: &str = "audio/wav";

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    Audio(AudioObservationInput),
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

    pub fn timeline(
        project_id: ProjectId,
        sequence_id: EntityId,
        revision: Revision,
        at: Rational,
    ) -> Self {
        Self::Still {
            target: ObserveTarget::TimelineFrame {
                project_id,
                sequence_id,
                revision,
                at,
            },
            frame_selection: FrameSelection::ExactSequenceGrid,
            source_policy: SourcePolicy::OriginalRequired,
            display_profile: DisplayProfile::TimelineMonitorRgba8,
            fit: Fit::Contain,
            encoding: Encoding::Png {},
            delivery: Delivery::Inline,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioObservationInput {
    pub target: AudioTarget,
    pub range: SampleRange,
    #[schemars(range(min = 8000, max = 192000))]
    pub sample_rate: u32,
    pub channel_layout: AudioLayout,
    pub source_policy: SourcePolicy,
    pub encoding: AudioEncoding,
    pub delivery: Delivery,
}

impl AudioObservationInput {
    pub fn frames(&self) -> Result<u32> {
        let frames = self
            .range
            .end
            .checked_sub(self.range.start)
            .filter(|count| *count != 0)
            .ok_or_else(|| {
                DomainError::new(
                    "INVALID_RANGE",
                    "Choose a nonempty half-open decoded sample interval",
                )
            })?;
        if !(8000..=192000).contains(&self.sample_rate) {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Choose an original rate between 8000 and 192000 Hz",
            ));
        }
        let pcm = u64::from(frames) * 2 * self.channels() as u64;
        if pcm > (MAX_INLINE_WAVE_BYTES - 44) as u64 {
            return Err(DomainError::new(
                "INLINE_BUDGET_EXCEEDED",
                "Choose a shorter sample interval for inline WAVE delivery",
            ));
        }
        Ok(frames)
    }

    pub fn channels(&self) -> usize {
        match self.channel_layout {
            AudioLayout::Mono => 1,
            AudioLayout::Stereo => 2,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioTarget {
    pub asset_id: String,
    pub stream_index: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SampleRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "format", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioEncoding {
    WavePcm16 {},
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
    TimelineFrame {
        project_id: ProjectId,
        sequence_id: EntityId,
        revision: Revision,
        at: Rational,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FrameSelection {
    DecodedIndex,
    ExactSequenceGrid,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourcePolicy {
    OriginalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayProfile {
    SourceMonitorRgba8,
    TimelineMonitorRgba8,
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
pub struct TimelineObservation {
    pub interval: TimeRange,
    pub origin: FrameOrigin,
    pub original_source_pts: Option<Rational>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationMetadata {
    pub observation_id: String,
    pub target: ObserveTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_frame_index: Option<u32>,
    pub timing: Option<FrameTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<TimelineObservation>,
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioObservationMetadata {
    pub observation_id: String,
    pub target: AudioTarget,
    pub range: SampleRange,
    pub sample_rate: u32,
    pub channel_layout: AudioLayout,
    pub frames: u32,
    pub source_sha256: String,
    pub component_sha256: String,
    pub graph_sha256: String,
    pub source_policy: SourcePolicy,
    pub encoding: AudioEncoding,
    pub delivery: Delivery,
    pub mime_type: String,
    pub encoded_byte_count: usize,
    pub encoded_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InlineAudioObservation {
    pub metadata: AudioObservationMetadata,
    #[schemars(length(min = 1, max = 131072))]
    pub audio_base64: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ObservationResult {
    Still(Box<InlineObservation>),
    Audio(Box<InlineAudioObservation>),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MediaObservationMetadata {
    Still(Box<ObservationMetadata>),
    Audio(Box<AudioObservationMetadata>),
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSegmentInput {
    pub asset_id: String,
    pub video_stream: u32,
    pub audio_stream: Option<u32>,
}

pub const PROGRAM_SEGMENT_COMMAND: &str = "previews.program_segment";
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

/// Capture a finite interval shared by the browser preview and export graph.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramSegmentInput {
    pub sequence_id: EntityId,
    pub revision: Revision,
    pub range: TimeRange,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramSegment {
    pub project_id: ProjectId,
    pub selection: ProgramSegmentInput,
    pub capture_sha256: String,
    pub playback: SegmentPlayback,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SegmentPlayback {
    pub segment_id: String,
    pub sha256: String,
    #[schemars(range(min = 1, max = 2097152))]
    pub bytes: u32,
    pub mime_type: String,
    pub initialization: SegmentBytes,
    #[schemars(length(min = 1, max = 8))]
    pub fragments: Vec<SegmentBytes>,
    pub expires_in_seconds: u32,
}

impl From<SourceSegment> for SegmentPlayback {
    fn from(segment: SourceSegment) -> Self {
        Self {
            segment_id: segment.segment_id,
            sha256: segment.sha256,
            bytes: segment.bytes,
            mime_type: segment.mime_type,
            initialization: segment.initialization,
            fragments: segment.fragments,
            expires_in_seconds: segment.expires_in_seconds,
        }
    }
}

/// Return the largest admissible short window starting at an exact monitor time.
pub fn program_window(
    start: Rational,
    frame_rate: Rational,
    sample_rate: u32,
) -> Result<TimeRange> {
    use crate::projects::grid_tick;
    let maximum_rate = Rational::new(60, 1)?;
    let invalid = || {
        DomainError::new(
            "INVALID_RANGE",
            "Program playback needs a start aligned to the video and audio clocks",
        )
    };
    if frame_rate <= Rational::ZERO || frame_rate > maximum_rate {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Program playback supports at most 60 frames per second",
        ));
    }
    let sample_grid = Rational::new(i64::from(sample_rate), 1)?;
    grid_tick(start, frame_rate).map_err(|_| invalid())?;
    grid_tick(start, sample_grid).map_err(|_| invalid())?;
    let counts = frame_rate.checked_mul(Rational::new(SOURCE_SEGMENT_SECONDS, 1)?)?;
    for frames in (1..=counts.numerator() / counts.denominator()).rev() {
        let duration = Rational::new(frames, 1)?.checked_mul(Rational::new(
            frame_rate.denominator(),
            frame_rate.numerator(),
        )?)?;
        let end = start.checked_add(duration)?;
        if grid_tick(end, sample_grid).is_ok() {
            return Ok(TimeRange { start, end });
        }
    }
    Err(invalid())
}
