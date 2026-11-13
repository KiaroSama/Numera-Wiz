//! Bounded query parameters; project identity and write revision stay in the envelope.
use super::*;
use crate::time::TimeRange;
/// Schema-only mirror of the unchanged project creation result.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectCreateResult {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub project_id: ProjectId,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub root_bin_id: EntityId,
    #[schemars(extend("pattern" = "^(0|[1-9][0-9]*)$"))]
    pub revision: Revision,
    pub history: ProjectCreateHistory,
}
/// Project creation exposes only these two history flags.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectCreateHistory {
    pub can_undo: bool,
    pub can_redo: bool,
}
/// Public common-header fields only; catalog order is ascending canonical project ID.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub revision: Revision,
    pub schema_major: u32,
    pub envelope_version: u32,
    pub compatibility: ProjectCompatibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<super::authorization::ProjectAuthorization>")]
    pub authorization: Option<serde_json::Value>,
}
impl ProjectSummary {
    pub fn known_authorization(&self) -> Option<super::authorization::ProjectAuthorization> {
        super::authorization::ProjectAuthorization::from_observation(self.authorization.as_ref())
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCompatibility {
    Supported,
    ReadOnlyUnsupported,
}
fn limit_default() -> usize {
    128
}
fn sort_default() -> String {
    "label".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectInput {
    pub projection: String,
    #[serde(default)]
    pub revision: Option<Revision>,
    #[serde(default)]
    pub replace_snapshot: Option<String>,
    #[serde(default)]
    pub entity_id: Option<EntityId>,
    #[serde(default)]
    pub bin_id: Option<EntityId>,
    #[serde(default = "limit_default")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "sort_default")]
    pub sort: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceInspectInput {
    pub sequence_id: EntityId,
    pub projection: String,
    #[serde(default)]
    pub revision: Option<Revision>,
    #[serde(default)]
    pub replace_snapshot: Option<String>,
    #[serde(default = "limit_default")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub track_ids: Vec<EntityId>,
    #[serde(default)]
    pub range: Option<TimeRange>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectItemSummary {
    pub id: EntityId,
    pub kind: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSummary {
    pub id: EntityId,
    pub kind: TrackKind,
    pub name: String,
    pub order: u32,
    pub clip_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSummary {
    pub id: EntityId,
    pub track_id: EntityId,
    pub source_id: EntityId,
    pub sequence_start: Rational,
    pub duration: Rational,
}
