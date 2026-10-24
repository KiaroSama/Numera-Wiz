//! Bounded inventory projections. Detail records are fetched explicitly by identifier.
use crate::records::{Asset, Job, JobState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
fn default_limit() -> usize { 128 }
impl Default for PageInput {
    fn default() -> Self { Self { limit: default_limit(), cursor: None, replace_snapshot: None } }
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
    pub id: String,
    pub state: JobState,
    pub output_name: String,
    pub error_code: Option<String>,
}
fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or("media").to_owned()
}
impl From<&Asset> for AssetSummary {
    fn from(asset: &Asset) -> Self {
        Self {
            id: asset.id.clone(),
            display_name: if asset.display_name.is_empty() { basename(&asset.identity.path) } else { asset.display_name.clone() },
            bytes: asset.identity.bytes.clone(),
            stream_count: asset.probe.streams.len(),
        }
    }
}
impl From<&Job> for JobSummary {
    fn from(job: &Job) -> Self {
        Self {
            id: job.id.clone(), state: job.state.clone(), output_name: basename(&job.plan.output),
            error_code: job.error.as_ref().map(|error| error.code.clone()),
        }
    }
}
