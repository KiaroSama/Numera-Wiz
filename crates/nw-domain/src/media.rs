//! Finite, authoritative project capture and a separately framed media review contract.
mod capture;
mod plan;
use crate::{
    DomainError, Result,
    projects::{self, EntityId, ProjectId, Revision},
    time::{Rational, TimeRange},
};
pub use capture::capture;
pub use plan::{MediaPlanV1, compile};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

pub const MAX_TRACKS: usize = 16;
pub const MAX_CLIPS: usize = 128;
pub const MAX_SEGMENTS: usize = 256;
pub const MAX_RAW_BYTES: usize = 1024 * 1024;
pub const MAX_PLAN_BYTES: usize = 128 * 1024;
pub const FILTERS: &[&str] = &[
    "color", "trim", "setpts", "settb", "concat", "anullsrc", "atrim", "asetpts", "format",
    "aformat",
];
pub const ENCODERS: &[&str] = &["ffv1", "pcm_s16le"];
pub const MUXERS: &[&str] = &["matroska"];

pub trait RenderRead: projects::edit::ProjectRead {
    fn track_ids_for_render(&self, sequence: &EntityId, declared_tracks: usize, limit: usize) -> Result<Vec<EntityId>>;
    fn clip_ids_for_render(&self, sequence: &EntityId, track_id: &EntityId, track: &projects::Track, range: TimeRange, limit: usize) -> Result<Vec<EntityId>>;
    fn render_raw_entity(&self, id: &EntityId, remaining_raw_bytes: usize) -> Result<Option<String>>;
    fn render_header_raw(&self) -> &str;
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderInput {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub sequence_id: EntityId,
    #[schemars(extend("pattern" = "^(0|[1-9][0-9]{0,19})$"))]
    pub revision: Revision,
    #[serde(deserialize_with = "read_range")]
    #[schemars(schema_with = "range_schema")]
    pub range: TimeRange,
    pub output: OutputIntent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputIntent {
    pub container: Container,
    pub video_codec: VideoCodec,
    pub audio_codec: AudioCodec,
    pub audio: AudioIntent,
    pub destination: Destination,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Matroska,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    Ffv1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    PcmS16le,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioIntent {
    Silence,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    Download,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentIdentity {
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub sha256: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub bytes: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaTools {
    pub ffmpeg: ComponentIdentity,
    pub ffprobe: ComponentIdentity,
    #[schemars(schema_with = "filter_schema")]
    pub filters: Vec<String>,
    #[schemars(schema_with = "encoder_schema")]
    pub encoders: Vec<String>,
    #[schemars(schema_with = "muxer_schema")]
    pub muxers: Vec<String>,
}
impl MediaTools {
    pub fn validate(&self) -> Result<()> {
        for identity in [&self.ffmpeg, &self.ffprobe] {
            hash_text(&identity.sha256)?;
            if decimal(&identity.bytes)? == 0 {
                return Err(invalid("Empty media component"));
            }
        }
        for (actual, expected) in [
            (&self.filters, FILTERS),
            (&self.encoders, ENCODERS),
            (&self.muxers, MUXERS),
        ] {
            if actual
                .iter()
                .map(String::as_str)
                .ne(expected.iter().copied())
            {
                return Err(invalid("Media component qualification set differs"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaSettings {
    #[schemars(range(min = 2, max = 1920), extend("multipleOf" = 2))]
    pub width: u32,
    #[schemars(range(min = 2, max = 1080), extend("multipleOf" = 2))]
    pub height: u32,
    #[serde(deserialize_with = "read_rational")]
    #[schemars(schema_with = "rational_schema")]
    pub frame_rate: Rational,
    #[schemars(range(min = 8000, max = 384000))]
    pub sample_rate: u32,
    pub audio_layout: AudioLayout,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioLayout {
    Mono,
    Stereo,
}
impl AudioLayout {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mono => "mono",
            Self::Stereo => "stereo",
        }
    }
}
impl MediaSettings {
    fn validate(&self) -> Result<()> {
        projects::SequenceSettings {
            width: self.width,
            height: self.height,
            frame_rate: self.frame_rate,
            sample_rate: self.sample_rate,
            audio_layout: self.audio_layout.name().into(),
        }
        .validate()?;
        if self.width > 1920
            || self.height > 1080
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
            || self.frame_rate > Rational::new(120, 1)?
            || self.frame_rate.numerator() > i64::from(i32::MAX)
            || self.frame_rate.denominator() > i64::from(i32::MAX)
        {
            return Err(DomainError::new(
                "UNSUPPORTED_MEDIA_SETTINGS",
                "Compiler admits even dimensions at most1920x1080 and representable frame rates at most120fps",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawFact {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub id: EntityId,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub sha256: String,
    #[schemars(range(min = 1, max = 1048576))]
    pub bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub project_id: ProjectId,
    #[schemars(extend("pattern" = "^(0|[1-9][0-9]{0,19})$"))]
    pub project_revision: Revision,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub sequence_id: EntityId,
    #[serde(deserialize_with = "read_range")]
    #[schemars(schema_with = "range_schema")]
    pub range: TimeRange,
    pub settings: MediaSettings,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub header_sha256: String,
    #[schemars(range(min = 1, max = 1048576))]
    pub header_bytes: usize,
    #[schemars(length(min = 1, max = 256))]
    pub facts: Vec<RawFact>,
    #[schemars(length(max = 128))]
    pub clips: Vec<ColorClip>,
    pub output: OutputIntent,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColorClip {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub clip_id: EntityId,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub source_id: EntityId,
    #[serde(deserialize_with = "read_range")]
    #[schemars(schema_with = "range_schema")]
    pub range: TimeRange,
    pub color: CornerColor,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CornerColor {
    Black,
    Red,
    Green,
    Blue,
    Yellow,
    Magenta,
    Cyan,
    White,
}
impl CornerColor {
    pub fn rgb(self) -> &'static str {
        match self {
            Self::Black => "000000",
            Self::Red => "FF0000",
            Self::Green => "00FF00",
            Self::Blue => "0000FF",
            Self::Yellow => "FFFF00",
            Self::Magenta => "FF00FF",
            Self::Cyan => "00FFFF",
            Self::White => "FFFFFF",
        }
    }
}
fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::new("INVALID_PLAN", message)
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))
}
fn hash_text(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("Expected lowercase SHA256"));
    }
    Ok(())
}
fn decimal(value: &str) -> Result<u64> {
    let n = value
        .parse::<u64>()
        .map_err(|_| invalid("Invalid bounded decimal"))?;
    if n.to_string() != value {
        return Err(invalid("Expected canonical decimal"));
    }
    Ok(n)
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RationalWire {
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^(0|-?[1-9][0-9]*)$"))]
    num: String,
    #[schemars(length(min = 1, max = 19), extend("pattern" = "^[1-9][0-9]*$"))]
    den: String,
}
impl RationalWire {
    fn checked<E: serde::de::Error>(self) -> std::result::Result<Rational, E> {
        let num = self.num.parse::<i64>().map_err(E::custom)?;
        let den = self.den.parse::<i64>().map_err(E::custom)?;
        let value = Rational::new(num, den).map_err(E::custom)?;
        if num.to_string() != self.num
            || den.to_string() != self.den
            || value.numerator() != num
            || value.denominator() != den
        {
            return Err(E::custom("Expected canonical reduced rational"));
        }
        Ok(value)
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RangeWire {
    start: RationalWire,
    end: RationalWire,
}
fn read_rational<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Rational, D::Error> {
    RationalWire::deserialize(d)?.checked()
}
fn read_range<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<TimeRange, D::Error> {
    let value = RangeWire::deserialize(d)?;
    Ok(TimeRange {
        start: value.start.checked()?,
        end: value.end.checked()?,
    })
}
fn rational_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<RationalWire>()
}
fn range_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<RangeWire>()
}
fn filter_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"type":"array", "minItems":10, "maxItems":10, "uniqueItems":true, "items":{"type":"string", "enum":FILTERS}})
}
fn encoder_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"type":"array", "minItems":2, "maxItems":2, "uniqueItems":true, "items":{"type":"string", "enum":ENCODERS}})
}
fn muxer_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"type":"array", "minItems":1, "maxItems":1, "uniqueItems":true, "items":{"type":"string", "enum":MUXERS}})
}
