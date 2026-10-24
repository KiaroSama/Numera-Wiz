//! Bounded inventory projections. Detail records are fetched explicitly by identifier.
use crate::{
    access::ObjectAction,
    mux::CleanResolution,
    records::{Asset, FileIdentity, Job, JobState, Plan},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageInput {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub replace_snapshot: Option<String>,
}
fn default_limit() -> usize {
    128
}
impl Default for PageInput {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            cursor: None,
            replace_snapshot: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotPage<T> {
    pub schema_version: u32,
    pub project_revision: Option<String>,
    pub snapshot_id: String,
    pub snapshot_expires_at: String,
    pub projection: String,
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub event_cursor: Option<String>,
    pub total_count: usize,
    pub count_accuracy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssetSummary {
    pub id: String,
    pub display_name: String,
    pub bytes: String,
    pub stream_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remux_progress: Option<crate::execution::RemuxProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::execution::StageProgress>,
    #[serde(default)]
    pub warning_count: usize,
    pub id: String,
    pub state: JobState,
    pub output_name: String,
    pub error_code: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanReview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_limits: Option<crate::execution::CleanupLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_times: Option<crate::file_times::FileTimePlan>,
    pub schema_version: u32,
    pub required_rights: Option<crate::plans::RequiredRights>,
    pub rights_redacted: bool,
    pub id: String,
    pub hash: String,
    pub source: AssetSummary,
    pub source_sha256: String,
    pub ffmpeg: ComponentSummary,
    pub ffprobe: ComponentSummary,
    pub source_path: Option<String>,
    pub output_name: String,
    pub output_path: Option<String>,
    pub resolution: CleanResolution,
    pub argv_template: Option<Vec<String>>,
    pub created_at: String,
    pub expires_at: String,
    pub verification_scope: String,
    pub policy_revision: u64,
    pub effective_actions: BTreeSet<ObjectAction>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComponentSummary {
    pub file_name: String,
    pub bytes: String,
    pub sha256: String,
}
impl From<&FileIdentity> for ComponentSummary {
    fn from(identity: &FileIdentity) -> Self {
        Self {
            file_name: basename(&identity.path),
            bytes: identity.bytes.clone(),
            sha256: identity.sha256.clone(),
        }
    }
}
impl PlanReview {
    pub fn new(
        plan: &Plan,
        source_visible: bool,
        output_visible: bool,
        owner: bool,
        policy_revision: u64,
        effective_actions: BTreeSet<ObjectAction>,
    ) -> Self {
        Self {
            execution_limits: plan.execution_limits.clone(),
            file_times: plan.file_times.clone(),
            schema_version: plan.schema_version,
            required_rights: (source_visible && output_visible)
                .then(|| plan.required_rights.clone())
                .flatten(),
            rights_redacted: plan.required_rights.is_some() && !(source_visible && output_visible),
            id: plan.id.clone(),
            hash: plan.hash.clone(),
            source: AssetSummary::from(&plan.source),
            source_sha256: plan.source.identity.sha256.clone(),
            ffmpeg: ComponentSummary::from(&plan.ffmpeg),
            ffprobe: ComponentSummary::from(&plan.ffprobe),
            source_path: source_visible.then(|| plan.source.identity.path.clone()),
            output_name: basename(&plan.output),
            output_path: output_visible.then(|| plan.output.clone()),
            resolution: plan.resolution.clone(),
            argv_template: owner.then(|| plan.argv_template.clone()),
            created_at: plan.created_at.clone(),
            expires_at: plan.expires_at.clone(),
            verification_scope: plan.verification_scope.clone(),
            policy_revision,
            effective_actions,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactView {
    pub path: Option<String>,
    pub bytes: String,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorView {
    pub code: String,
    pub message: Option<String>,
    pub retryable: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remux_progress: Option<crate::execution::RemuxProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::execution::StageProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_times: Option<FileTimeOutcomeView>,
    pub id: String,
    pub state: JobState,
    pub plan: PlanReview,
    pub artifact: Option<ArtifactView>,
    pub verification: Option<String>,
    pub error: Option<ErrorView>,
    pub prepared_stages: Option<Vec<Vec<String>>>,
    pub policy_revision: u64,
    pub effective_actions: BTreeSet<ObjectAction>,
}
fn basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or("media")
        .to_owned()
}
impl From<&Asset> for AssetSummary {
    fn from(asset: &Asset) -> Self {
        Self {
            id: asset.id.clone(),
            display_name: if asset.display_name.is_empty() {
                basename(&asset.identity.path)
            } else {
                asset.display_name.clone()
            },
            bytes: asset.identity.bytes.clone(),
            stream_count: asset.probe.streams.len(),
        }
    }
}
impl From<&Job> for JobSummary {
    fn from(job: &Job) -> Self {
        Self {
            progress: job.progress.clone(),
            remux_progress: job.remux_progress.clone(),
            warning_count: job
                .file_times
                .as_ref()
                .map_or(0, |value| value.warnings.len()),
            id: job.id.clone(),
            state: job.state.clone(),
            output_name: basename(&job.plan.output),
            error_code: job.error.as_ref().map(|error| error.code.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileTimeOutcomeView {
    pub policy: crate::file_times::FileTimeRequest,
    pub desired: Option<crate::file_times::FileTimesSnapshot>,
    pub observed: Option<crate::file_times::FileTimesSnapshot>,
    pub preserved: Option<bool>,
    pub warnings: Vec<ErrorView>,
    pub scope: String,
}
impl FileTimeOutcomeView {
    pub fn new(value: crate::file_times::FileTimeOutcome, owner: bool) -> Self {
        Self {
            policy: value.policy,
            desired: value.desired,
            observed: value.observed,
            preserved: value.preserved,
            warnings: value
                .warnings
                .into_iter()
                .map(|error| ErrorView {
                    code: error.code,
                    message: owner.then_some(error.message),
                    retryable: error.retryable,
                })
                .collect(),
            scope: value.scope,
        }
    }
}
