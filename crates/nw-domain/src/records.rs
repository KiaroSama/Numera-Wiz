//! Versioned execution facts shared by the store, engine and clients.
use crate::mux::{CleanResolution, Stream};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FileIdentity {
    pub path: String,
    pub bytes: String,
    pub sha256: String,
}
/// Supplemental native object identity. Content digest remains mandatory.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeObjectIdentity {
    pub schema_version: u32,
    pub kind: NativeObjectKind,
    pub volume: String,
    pub object: String,
    pub created_ns: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeObjectKind {
    UnixDeviceInode,
    WindowsFileId128,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaProbe {
    pub streams: Vec<Stream>,
    #[serde(default)]
    pub stream_details: BTreeMap<u32, serde_json::Value>,
    pub format: String,
    pub global_tags: BTreeMap<String, String>,
    pub chapters: serde_json::Value,
    pub verification_scope: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Asset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<NativeObjectIdentity>,
    #[serde(default)]
    pub display_name: String,
    pub id: String,
    pub creator: String,
    pub identity: FileIdentity,
    pub probe: MediaProbe,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_limits: Option<crate::execution::CleanupLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_times: Option<crate::file_times::FileTimePlan>,
    #[serde(
        default = "legacy_plan_version",
        skip_serializing_if = "is_legacy_plan"
    )]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_rights: Option<crate::plans::RequiredRights>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_audit: Option<crate::plans::CreationAudit>,
    pub id: String,
    pub creator: String,
    pub hash: String,
    pub source: Asset,
    pub output: String,
    pub ffmpeg: FileIdentity,
    pub ffprobe: FileIdentity,
    pub resolution: CleanResolution,
    pub argv_template: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
    pub verification_scope: String,
}
fn legacy_plan_version() -> u32 {
    1
}
fn is_legacy_plan(version: &u32) -> bool {
    *version == 1
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Verifying,
    Publishing,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    Skipped,
}
impl JobState {
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted | Self::Skipped
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Job {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remux_progress: Option<crate::execution::RemuxProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::execution::StageProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_times: Option<crate::file_times::FileTimeOutcome>,
    pub id: String,
    pub creator: String,
    pub plan: Plan,
    pub state: JobState,
    pub prepared_argv: Option<Vec<String>>,
    #[serde(default)]
    pub prepared_stages: Vec<Vec<String>>,
    #[serde(default)]
    pub owned_staging: Option<StagingIntent>,
    pub artifact: Option<FileIdentity>,
    pub verification: Option<String>,
    pub error: Option<crate::DomainError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StagingIntent {
    pub job_id: String,
    pub directory: String,
    pub payload: String,
}
