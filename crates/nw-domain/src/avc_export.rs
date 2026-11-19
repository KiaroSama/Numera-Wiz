//! Reviewable CPU AVC export without untyped encoder options.
use crate::{DomainError, Result};
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
    #[schemars(range(min = 1, max = 51))]
    pub crf: u8,
    pub preset: Preset,
    pub audio: Audio,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<PictureTransform>,
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
        if !(1..=51).contains(&self.crf) {
            return Err(DomainError::new("VALIDATION_ERROR", "Choose an AVC CRF from 1 to 51"));
        }
        if let Audio::Aac { bitrate_kbps, .. } = self.audio && !(48..=320).contains(&bitrate_kbps) {
            return Err(DomainError::new("VALIDATION_ERROR", "Choose an AAC bitrate from 48 to 320 kbps"));
        }
        if let Some(picture) = self.picture {
            let even = |value: u32, minimum, maximum| {
                (minimum..=maximum).contains(&value) && value.is_multiple_of(2)
            };
            if picture.crop.is_some_and(|crop| {
                !even(crop.width, 2, 4096) || !even(crop.height, 2, 2160)
                    || !even(crop.x, 0, 4094) || !even(crop.y, 0, 2158)
            }) || picture.resize.is_some_and(|resize| {
                !even(resize.width, 2, 4096) || !even(resize.height, 2, 2160)
            }) {
                return Err(DomainError::new("VALIDATION_ERROR", "Picture coordinates and dimensions require even 4:2:0 alignment"));
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
        let picture = streams.iter().find(|stream| stream.kind == StreamKind::Video && !stream.attached_picture())
            .ok_or_else(|| DomainError::new("STREAM_NOT_FOUND", "The folder member has no original picture"))?;
        let audio = match self.audio {
            FolderAudio::Drop {} => Audio::Drop {},
            FolderAudio::Aac { ordinal, bitrate_kbps } => {
                let index = (1..=32).contains(&ordinal).then_some(ordinal.saturating_sub(1) as usize)
                    .ok_or_else(|| DomainError::new("VALIDATION_ERROR", "Choose an audio track ordinal from 1 to 32"))?;
                let mut audio: Vec<_> = streams.iter().filter(|stream| stream.kind == StreamKind::Audio).collect();
                audio.sort_unstable_by_key(|stream| stream.index);
                let track = audio.get(index)
                    .ok_or_else(|| DomainError::new("STREAM_NOT_FOUND", "This folder member has no audio at the selected ordinal"))?;
                Audio::Aac { stream_index: track.index, bitrate_kbps }
            }
        };
        let settings = Encode { video_stream: picture.index, container: self.container, crf: self.crf, preset: self.preset, audio, picture: None };
        settings.validate()?;
        Ok(settings)
    }
}
