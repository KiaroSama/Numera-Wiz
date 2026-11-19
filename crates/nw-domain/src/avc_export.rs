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
