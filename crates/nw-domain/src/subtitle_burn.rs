//! Reviewable assumptions for rendering text captions into an original picture.
use crate::{
    DomainError, Result,
    records::{Asset, FileIdentity, NativeObjectIdentity},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BurnInput {
    pub asset_id: String,
    pub output: String,
    pub video_stream: u32,
    pub subtitle: SubtitleInput,
    pub audio: Audio,
    pub color: Color,
    pub quality: Quality,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubtitleInput {
    Embedded { stream_index: u32 },
    External { asset_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Audio {
    None {},
    CopyAll {},
    CopySelected {
        #[schemars(length(min = 1, max = 32))]
        indices: Vec<u32>,
    },
}
impl Audio {
    pub fn validate(&self) -> Result<()> {
        if let Self::CopySelected { indices } = self
            && (!(1..=32).contains(&indices.len())
                || indices.iter().collect::<BTreeSet<_>>().len() != indices.len())
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Select one to thirty-two distinct original audio streams",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Color {
    PreserveSdr {},
    ToneMapBt709 {
        #[schemars(range(min = 100, max = 10000))]
        peak_nits: u16,
    },
}

impl Color {
    pub fn validate(self) -> Result<()> {
        if let Self::ToneMapBt709 { peak_nits } = self
            && !(100..=10000).contains(&peak_nits)
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Choose a reviewed HDR source peak from 100 to 10000 nits",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    LosslessFfv1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubtitleSource {
    Embedded { stream_index: u32 },
    External { source: Box<Asset> },
}
impl SubtitleSource {
    pub fn external(&self) -> Option<&Asset> {
        match self {
            Self::External { source } => Some(source.as_ref()),
            Self::Embedded { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Burn {
    pub video_stream: u32,
    pub subtitle: SubtitleSource,
    pub audio: Audio,
    pub color: Color,
    pub quality: Quality,
    pub font: FileIdentity,
    pub font_object: NativeObjectIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BurnReview {
    pub video_stream: u32,
    pub embedded_subtitle_stream: Option<u32>,
    pub external_subtitle_asset: Option<String>,
    pub audio: Audio,
    pub color: Color,
    pub quality: Quality,
    pub font_sha256: String,
}
