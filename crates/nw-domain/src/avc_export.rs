//! Reviewable CPU video export without untyped encoder options.
use crate::{DomainError, Result, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EncodeInput {
    pub asset_id: String,
    pub output: String,
    pub settings: Encode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Encode {
    pub video_stream: u32,
    pub container: Container,
    #[serde(default, skip_serializing_if = "VideoCodec::is_h264")]
    pub codec: VideoCodec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 51))]
    pub crf: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<Bitrate>,
    pub preset: Preset,
    pub audio: Audio,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<PictureTransform>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    Av1,
}

impl VideoCodec {
    fn is_h264(&self) -> bool {
        *self == Self::H264
    }

    pub fn encoder(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::Hevc => "libx265",
            Self::Av1 => "libsvtav1",
        }
    }

    pub fn probe_name(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bitrate {
    #[schemars(range(min = 64, max = 100000))]
    pub average_kbps: u32,
    pub mode: RateMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RateMode {
    #[default]
    QualityVbr,
    StrictSize,
}

impl Bitrate {
    pub fn limits(self) -> (u32, u32) {
        let scale = match self.mode {
            RateMode::QualityVbr => 2,
            RateMode::StrictSize => 1,
        };
        let peak = self.average_kbps.saturating_mul(scale);
        (peak, peak.saturating_mul(2))
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PictureTransform {
    #[serde(default)]
    pub crop: Option<Crop>,
    #[serde(default)]
    pub rotation: Rotation,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default)]
    pub resize: Option<Resize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<PictureLook>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PictureLook {
    #[serde(default)]
    pub grayscale: bool,
    #[serde(default)]
    pub brightness: Option<Rational>,
    #[serde(default)]
    pub contrast: Option<Rational>,
    #[serde(default)]
    pub saturation: Option<Rational>,
    #[serde(default)]
    pub gamma: Option<Rational>,
    #[serde(default)]
    pub denoise: Option<Strength>,
    #[serde(default)]
    pub detail: Option<Detail>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Light,
    Medium,
    Heavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Detail {
    Sharpen { strength: Strength },
    Blur { strength: Strength },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    None,
    Clockwise,
    HalfTurn,
    Counterclockwise,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    #[schemars(range(min = 2, max = 4096), extend("multipleOf" = 2))]
    pub width: u32,
    #[schemars(range(min = 2, max = 2160), extend("multipleOf" = 2))]
    pub height: u32,
    #[schemars(range(max = 4094), extend("multipleOf" = 2))]
    pub x: u32,
    #[schemars(range(max = 2158), extend("multipleOf" = 2))]
    pub y: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resize {
    #[schemars(range(min = 2, max = 4096), extend("multipleOf" = 2))]
    pub width: u32,
    #[schemars(range(min = 2, max = 2160), extend("multipleOf" = 2))]
    pub height: u32,
    pub aspect: ResizeAspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResizeAspect {
    PreserveDisplay,
    SquarePixels,
    Fit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Matroska,
    Mp4,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Fast,
    Balanced,
    Slow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Audio {
    Drop {},
    Aac {
        stream_index: u32,
        #[schemars(range(min = 48, max = 320))]
        bitrate_kbps: u16,
    },
}

impl Encode {
    pub fn validate(&self) -> Result<()> {
        match (self.crf, self.bitrate) {
            (Some(crf), None) if (1..=51).contains(&crf) => {}
            (None, Some(bitrate)) if (64..=100000).contains(&bitrate.average_kbps) => {}
            _ => {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "Choose one CRF from 1 to 51 or an average bitrate from 64 to 100000 kbps",
                ));
            }
        }
        if let Audio::Aac { bitrate_kbps, .. } = self.audio
            && !(48..=320).contains(&bitrate_kbps)
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Choose an AAC bitrate from 48 to 320 kbps",
            ));
        }
        if let Some(picture) = self.picture {
            let even = |value: u32, minimum, maximum| {
                (minimum..=maximum).contains(&value) && value.is_multiple_of(2)
            };
            if picture.crop.is_some_and(|crop| {
                !even(crop.width, 2, 4096)
                    || !even(crop.height, 2, 2160)
                    || !even(crop.x, 0, 4094)
                    || !even(crop.y, 0, 2158)
            }) || picture
                .resize
                .is_some_and(|resize| !even(resize.width, 2, 4096) || !even(resize.height, 2, 2160))
            {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "Picture coordinates and dimensions require even 4:2:0 alignment",
                ));
            }
            if let Some(look) = picture.look {
                for (field, value, lower, upper) in [
                    (
                        "brightness",
                        look.brightness,
                        Rational::new(-1, 1)?,
                        Rational::new(1, 1)?,
                    ),
                    (
                        "contrast",
                        look.contrast,
                        Rational::ZERO,
                        Rational::new(3, 1)?,
                    ),
                    (
                        "saturation",
                        look.saturation,
                        Rational::ZERO,
                        Rational::new(3, 1)?,
                    ),
                    (
                        "gamma",
                        look.gamma,
                        Rational::new(1, 10)?,
                        Rational::new(10, 1)?,
                    ),
                ] {
                    if value.is_some_and(|value| !(lower..=upper).contains(&value)) {
                        return Err(DomainError::new(
                            "VALIDATION_ERROR",
                            "Color adjustment is outside its admitted range",
                        )
                        .field(&format!("settings.picture.look.{field}")));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderEncodeInput {
    pub snapshot_id: String,
    pub snapshot_hash: String,
    pub output_root: String,
    pub container: Container,
    #[serde(default, skip_serializing_if = "VideoCodec::is_h264")]
    pub codec: VideoCodec,
    #[schemars(range(min = 1, max = 51))]
    pub crf: u8,
    pub preset: Preset,
    pub audio: FolderAudio,
    pub failure_policy: crate::batches::FailurePolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FolderAudio {
    Drop {},
    Aac {
        #[schemars(range(min = 1, max = 32))]
        ordinal: u32,
        #[schemars(range(min = 48, max = 320))]
        bitrate_kbps: u16,
    },
}

impl FolderEncodeInput {
    pub fn settings_for(&self, streams: &[crate::mux::Stream]) -> Result<Encode> {
        use crate::mux::StreamKind;
        let picture = streams
            .iter()
            .find(|stream| stream.kind == StreamKind::Video && !stream.attached_picture())
            .ok_or_else(|| {
                DomainError::new(
                    "STREAM_NOT_FOUND",
                    "The folder member has no original picture",
                )
            })?;
        let audio = match self.audio {
            FolderAudio::Drop {} => Audio::Drop {},
            FolderAudio::Aac {
                ordinal,
                bitrate_kbps,
            } => {
                let index = (1..=32)
                    .contains(&ordinal)
                    .then_some(ordinal.saturating_sub(1) as usize)
                    .ok_or_else(|| {
                        DomainError::new(
                            "VALIDATION_ERROR",
                            "Choose an audio track ordinal from 1 to 32",
                        )
                    })?;
                let mut audio: Vec<_> = streams
                    .iter()
                    .filter(|stream| stream.kind == StreamKind::Audio)
                    .collect();
                audio.sort_unstable_by_key(|stream| stream.index);
                let track = audio.get(index).ok_or_else(|| {
                    DomainError::new(
                        "STREAM_NOT_FOUND",
                        "This folder member has no audio at the selected ordinal",
                    )
                })?;
                Audio::Aac {
                    stream_index: track.index,
                    bitrate_kbps,
                }
            }
        };
        let settings = Encode {
            video_stream: picture.index,
            container: self.container,
            codec: self.codec,
            crf: Some(self.crf),
            bitrate: None,
            preset: self.preset,
            audio,
            picture: None,
        };
        settings.validate()?;
        Ok(settings)
    }
}
