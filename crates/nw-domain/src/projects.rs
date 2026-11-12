//! Exact, bounded project records. Payload JSON remains opaque until a known codec is needed.
pub mod edit;
mod json;
pub mod views;
use crate::{DomainError, Result, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_ENTITIES: usize = 16384;
pub const MAX_CHANGES: usize = 128;
pub const MAX_INDEX_WRITES: usize = 512;
pub const MAX_PATCH_BYTES: usize = 128 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_CLOSURE: usize = 256;
pub fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::new("VALIDATION_ERROR", message)
}
pub fn limit(message: impl Into<String>) -> DomainError {
    DomainError::new("RESOURCE_LIMIT", message)
}
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn validate_id(value: &str) -> Result<()> {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
    {
        return Err(invalid("Expected a canonical lowercase UUID"));
    }
    Ok(())
}
macro_rules! identifier {
    ($name:ident) => {
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                validate_id(&value)?;
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = DomainError;
            fn try_from(v: String) -> Result<Self> {
                Self::parse(v)
            }
        }
        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
identifier!(ProjectId);
identifier!(EntityId);
identifier!(OperationId);
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
pub struct Revision(pub u64);
impl Revision {
    pub fn parse(value: &str) -> Result<Self> {
        let parsed = value
            .parse::<u64>()
            .map_err(|_| invalid("Invalid decimal revision"))?;
        if parsed.to_string() != value {
            return Err(invalid("Revision must be canonical unsigned decimal"));
        }
        Ok(Self(parsed))
    }
    pub fn next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| limit("Project revision exhausted"))
    }
}
impl TryFrom<String> for Revision {
    type Error = DomainError;
    fn try_from(v: String) -> Result<Self> {
        Self::parse(&v)
    }
}
impl From<Revision> for String {
    fn from(v: Revision) -> String {
        v.0.to_string()
    }
}
impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub start: Rational,
    pub end: Rational,
}
impl SourceSpan {
    pub fn duration(self) -> Result<Rational> {
        if self.end <= self.start {
            return Err(invalid("Source end must exceed start"));
        }
        self.end.checked_sub(self.start)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational,
    pub sample_rate: u32,
    pub audio_layout: String,
}
impl SequenceSettings {
    pub fn validate(&self) -> Result<()> {
        if !(1..=16384).contains(&self.width)
            || !(1..=16384).contains(&self.height)
            || self.frame_rate <= Rational::ZERO
            || self.frame_rate > Rational::new(1000, 1)?
            || !(8000..=384000).contains(&self.sample_rate)
            || !["mono", "stereo", "2.1", "3.0", "4.0", "5.1", "7.1"]
                .contains(&self.audio_layout.as_str())
        {
            return Err(invalid(
                "Invalid explicit video/sample settings or channel layout",
            ));
        }
        Ok(())
    }
}
pub fn label(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(invalid(
            "Name must contain 1-128 UTF-8 bytes without controls",
        ));
    }
    Ok(())
}
pub fn grid_tick(time: Rational, grid: Rational) -> Result<u64> {
    if grid <= Rational::ZERO || time < Rational::ZERO {
        return Err(invalid(
            "Timeline times must be nonnegative on a positive explicit grid",
        ));
    }
    let ticks = time.checked_mul(grid)?;
    if ticks.denominator() != 1 {
        return Err(DomainError::new(
            "OFF_GRID",
            "Time must lie on the explicit track frame/sample grid",
        ));
    }
    ticks
        .numerator()
        .try_into()
        .map_err(|_| invalid("Grid tick exceeds u64"))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHeader {
    pub envelope_version: u32,
    pub schema_major: u32,
    pub id: ProjectId,
    pub creator: String,
    pub name: String,
    pub revision: Revision,
    pub root_bin_id: EntityId,
    pub entity_count: usize,
    pub sequence_count: usize,
    pub history_root: String,
    pub materialized_revision: Revision,
    pub change_digest: String,
    pub compatibility: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Box<RawValue>>,
}
impl ProjectHeader {
    pub fn writable(&self) -> Result<()> {
        if self.envelope_version != 1 || self.schema_major != 1 {
            return Err(DomainError::new(
                "PROJECT_READ_ONLY",
                "Unsupported project schema/envelope; original bytes are retained",
            ));
        }
        if self.revision != self.materialized_revision {
            return Err(DomainError::new(
                "PROJECT_CORRUPT",
                "Committed project checkpoint revision differs",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityReference {
    pub role: String,
    pub target: EntityId,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityEnvelope {
    pub envelope_version: u32,
    pub id: EntityId,
    pub project_id: ProjectId,
    pub kind: String,
    pub kind_version: u32,
    pub owner_id: Option<EntityId>,
    pub enabled: bool,
    pub references: Vec<EntityReference>,
    pub semantic_scope: String,
    pub label: String,
    pub payload_json: String,
    pub payload_sha256: String,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Box<RawValue>>,
}
impl EntityEnvelope {
    pub fn new(
        project: &ProjectId,
        id: EntityId,
        kind: &str,
        owner: Option<EntityId>,
        name: String,
        payload: &impl Serialize,
    ) -> Result<Self> {
        label(&name)?;
        let payload_json = serde_json::to_string(payload).map_err(|e| invalid(e.to_string()))?;
        let mut value = Self {
            envelope_version: 1,
            id,
            project_id: project.clone(),
            kind: kind.into(),
            kind_version: 1,
            owner_id: owner.clone(),
            enabled: true,
            references: Vec::new(),
            semantic_scope: "entity".into(),
            label: name,
            payload_sha256: hash(payload_json.as_bytes()),
            payload_json,
            extensions: BTreeMap::new(),
        };
        if let Some(target) = owner {
            value.references.push(EntityReference {
                role: "ownership".into(),
                target,
            });
        }
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        label(&self.label)?;
        if self.payload_json.len() > MAX_PAYLOAD_BYTES
            || self.references.len() > 128
            || self.extensions.len() > 64
        {
            return Err(limit("Entity payload or reference budget exceeded"));
        }
        json::validate(&self.payload_json)?;
        if hash(self.payload_json.as_bytes()) != self.payload_sha256 {
            return Err(DomainError::new(
                "PROJECT_CORRUPT",
                "Entity payload hash mismatch",
            ));
        }
        Ok(())
    }
    pub fn known(&self) -> bool {
        self.envelope_version == 1
            && self.kind_version == 1
            && ["bin", "sequence", "track", "clip", "source_binding"].contains(&self.kind.as_str())
    }
    pub fn envelope_semantics_supported(&self) -> bool {
        self.envelope_version == 1
            && ["project", "sequence", "track", "entity", "annotation"]
                .contains(&self.semantic_scope.as_str())
            && self.references.iter().all(|r| {
                ["ownership", "semantic_input", "source", "link"].contains(&r.role.as_str())
            })
    }
    pub fn semantics(&self) -> Result<()> {
        if !self.envelope_semantics_supported() {
            return Err(DomainError::new(
                "PROJECT_READ_ONLY",
                "Unsupported envelope reference/scope semantics",
            ));
        }
        if self.enabled && !self.known() && self.semantic_scope != "annotation" {
            return Err(DomainError::new(
                "UNSUPPORTED_SEMANTIC_NODE",
                "Enabled unknown semantic node affects this edit",
            ));
        }
        Ok(())
    }
    pub fn payload<T: DeserializeOwned>(&self, kind: &str) -> Result<T> {
        self.validate()?;
        if self.envelope_version != 1 || self.kind_version != 1 || self.kind != kind {
            return Err(DomainError::new(
                "UNSUPPORTED_SEMANTIC_NODE",
                "A supported entity codec is required",
            ));
        }
        serde_json::from_str(&self.payload_json).map_err(|e| invalid(e.to_string()))
    }
    /// Overlay only known top-level fields; opaque extension values retain their original number/text bytes.
    pub fn update(&mut self, payload: &impl Serialize) -> Result<()> {
        self.validate()?;
        let mut original: BTreeMap<String, Box<RawValue>> =
            serde_json::from_str(&self.payload_json).map_err(|e| invalid(e.to_string()))?;
        let encoded = serde_json::to_string(payload).map_err(|e| invalid(e.to_string()))?;
        let fields: BTreeMap<String, Box<RawValue>> =
            serde_json::from_str(&encoded).map_err(|e| invalid(e.to_string()))?;
        original.extend(fields);
        let mut candidate = self.clone();
        candidate.payload_json =
            serde_json::to_string(&original).map_err(|e| invalid(e.to_string()))?;
        candidate.payload_sha256 = hash(candidate.payload_json.as_bytes());
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn bytes(&self) -> Result<String> {
        let raw = serde_json::to_string(self).map_err(|e| invalid(e.to_string()))?;
        if raw.len() > 192 * 1024 {
            return Err(limit("Entity envelope exceeds its bounded detail section"));
        }
        Ok(raw)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bin {
    pub member_count: usize,
    #[serde(default)]
    pub locked: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub settings: SequenceSettings,
    pub track_count: usize,
    #[serde(default)]
    pub locked: bool,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub sequence_id: EntityId,
    pub kind: TrackKind,
    pub order: u32,
    pub grid: Rational,
    pub clip_count: usize,
    #[serde(default)]
    pub locked: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    pub source_id: EntityId,
    pub occurrence_id: EntityId,
    pub source_range: SourceSpan,
    pub sequence_start: Rational,
    pub duration: Rational,
    pub rate: Rational,
    pub link_group_id: Option<EntityId>,
    #[serde(default)]
    pub locked: bool,
}
impl Clip {
    pub fn end(&self) -> Result<Rational> {
        self.sequence_start.checked_add(self.duration)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceBinding {
    SolidColor {
        rgba: [f64; 4],
        duration: Rational,
    },
    Registered {
        asset_id: String,
        content_sha256: String,
        probe_sha256: String,
        stream_index: u32,
        media_kind: TrackKind,
        extent_verification: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackInput {
    pub kind: TrackKind,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SequenceInput {
    pub name: String,
    pub settings: SequenceSettings,
    pub tracks: Vec<TrackInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaceSource {
    SolidColor { rgba: [f64; 4], duration: Rational },
    Registered { asset_id: String, stream_index: u32 },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceInput {
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    pub source: PlaceSource,
    pub source_range: SourceSpan,
    pub sequence_start: Rational,
    pub collision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveInput {
    pub clip_ids: Vec<EntityId>,
    pub delta: Rational,
    pub collision: String,
    pub link_policy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkPolicy {
    Expand,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkInput {
    #[schemars(length(min = 1, max = 128), extend("uniqueItems" = true))]
    pub clip_ids: Vec<EntityId>,
    pub link_policy: LinkPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitInput {
    pub clip_id: EntityId,
    pub at_sequence: Rational,
    pub link_policy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveInput {
    pub clip_ids: Vec<EntityId>,
    pub mode: String,
    pub link_policy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChange {
    pub id: EntityId,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectPatch {
    pub changes: Vec<EntityChange>,
}
impl ProjectPatch {
    pub fn inverse(&self) -> Self {
        Self {
            changes: self
                .changes
                .iter()
                .map(|c| EntityChange {
                    id: c.id.clone(),
                    before: c.after.clone(),
                    after: c.before.clone(),
                })
                .collect(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.changes.len() > MAX_CHANGES {
            return Err(limit("Edit exceeds 128 entity changes"));
        }
        let bytes =
            serde_json::to_vec(&(self, self.inverse())).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_PATCH_BYTES {
            return Err(limit("Forward and inverse patches exceed 128 KiB"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for change in &self.changes {
            if !seen.insert(&change.id) || change.before == change.after {
                return Err(invalid("Duplicate or empty entity change"));
            }
            for raw in [&change.before, &change.after].into_iter().flatten() {
                let entity: EntityEnvelope =
                    serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
                entity.validate()?;
                if entity.id != change.id {
                    return Err(invalid("Patch entity ID differs"));
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct ResolvedEdit {
    pub base_revision: Revision,
    pub forward: ProjectPatch,
    pub inverse: ProjectPatch,
    pub changed_ids: Vec<EntityId>,
    pub created: serde_json::Value,
    pub source_requirements: Vec<SourceBinding>,
    pub compatibility_report: String,
}
