//! Bounded query parameters; project identity and write revision stay in the envelope.
use super::*;
use crate::time::TimeRange;
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
