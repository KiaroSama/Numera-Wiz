//! Reported component capabilities and the outcomes of explicitly exercised routes.
use crate::media::ComponentIdentity;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScope {
    #[default]
    Components,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectInput {
    #[serde(default)]
    pub scope: CapabilityScope,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshInput {
    #[serde(default)]
    pub devices: bool,
    #[serde(default = "include_components")]
    pub components: bool,
    #[serde(default)]
    pub force: bool,
}

fn include_components() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearInput {
    pub namespace: CapabilityScope,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Unknown,
    Unavailable,
    Failed,
    Exercised,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdvertisedComponents {
    pub encoders: Vec<String>,
    pub decoders: Vec<String>,
    pub filters: Vec<String>,
    pub demuxers: Vec<String>,
    pub muxers: Vec<String>,
    pub pixel_formats: Vec<String>,
    pub sample_formats: Vec<String>,
    pub hardware_accelerators: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityReport {
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub fingerprint: String,
    pub ffmpeg: ComponentIdentity,
    pub ffprobe: ComponentIdentity,
    pub advertised: Option<AdvertisedComponents>,
    pub cpu_round_trip: Observation,
    pub hardware: Observation,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClearResult {
    pub cleared: bool,
}
