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
        let settings = Encode { video_stream: picture.index, container: self.container, crf: self.crf, preset: self.preset, audio };
        settings.validate()?;
        Ok(settings)
    }
}
