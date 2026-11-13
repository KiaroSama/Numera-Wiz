//! A read-only review is separate from acceptance and never changes a reader floor.
use super::{MAX_GENERATIONS, MAX_MIGRATION_BYTES, digest_value, exact_u64, uuid_text};
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// This versioned target is independent of the frozen legacy migration producer.
pub const MEDIA_V1_CAPABILITIES: &[&str] = &[
    "access.v2",
    "cleanup.v2",
    "cleanup_batch.v1",
    "receipts.v1",
    "generation_activation.v1",
    "migration.v1",
    "adoption_guard.v1",
    "generation_promotion.v1",
    "media_execution.v1",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PromotionKind {
    GenerationPromotionV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetProfile {
    MediaV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RollbackCondition {
    StoppedRootRestorationBeforeAdoption,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceGuard {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub migration_id: String,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub activation_id: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationPromotionPlan {
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    pub kind: PromotionKind,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub id: String,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub hash: String,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub root_id: String,
    pub source_generation: u32,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub source_activation_id: String,
    pub source_activation_sequence: u32,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub source_prefix_sha256: String,
    #[schemars(extend("const" = 3))]
    pub source_store_version: u32,
    #[schemars(length(min = 1, max = 64), extend("uniqueItems" = true))]
    pub source_capabilities: Vec<String>,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub source_records_sha256: String,
    pub source_records: u32,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub source_bytes: String,
    pub source_guard: SourceGuard,
    pub rollback_generation: u32,
    pub target_generation: u32,
    pub target_profile: TargetProfile,
    #[schemars(length(min = 9, max = 9), extend("uniqueItems" = true))]
    pub required_capabilities: Vec<String>,
    pub rollback_condition: RollbackCondition,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub application_build: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^(0|[1-9][0-9]*)$"))]
    pub created_at: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^(0|[1-9][0-9]*)$"))]
    pub expires_at: String,
}
fn invalid() -> DomainError {
    DomainError::new(
        "INVALID_PROMOTION_PLAN",
        "Promotion review identity, source provenance, target profile or bounds are invalid",
    )
}
impl GenerationPromotionPlan {
    pub fn digest(&self) -> Result<String> {
        if self.source_capabilities.len() > 64
            || self.required_capabilities.len() != MEDIA_V1_CAPABILITIES.len()
        {
            return Err(invalid());
        }
        let source_caps: std::collections::BTreeSet<_> = self
            .source_capabilities
            .iter()
            .map(String::as_str)
            .collect();
        let target_caps: std::collections::BTreeSet<_> = self
            .required_capabilities
            .iter()
            .map(String::as_str)
            .collect();
        if self.schema_version != 1
            || [
                &self.id,
                &self.root_id,
                &self.source_activation_id,
                &self.source_guard.migration_id,
                &self.source_guard.activation_id,
            ]
            .into_iter()
            .any(|id| uuid_text(id).is_err())
            || self.source_generation == 0
            || self.source_generation > MAX_GENERATIONS
            || self.rollback_generation != self.source_generation.checked_add(1).unwrap_or(0)
            || self.target_generation != self.rollback_generation.checked_add(1).unwrap_or(0)
            || self.target_generation > MAX_GENERATIONS
            || !(1..=64).contains(&self.source_activation_sequence)
            || self.source_store_version != 3
            || self.source_guard.activation_id != self.source_activation_id
            || !exact_u64(&self.source_guard.revision)
            || self.source_guard.revision.parse::<u64>().unwrap_or(0) == 0
            || !digest_value(&self.source_prefix_sha256)
            || !digest_value(&self.source_records_sha256)
            || !digest_value(&self.application_build)
            || !exact_u64(&self.source_bytes)
            || self.source_bytes.parse::<u64>().unwrap_or(u64::MAX) == 0
            || self.source_bytes.parse::<u64>().unwrap_or(u64::MAX) > MAX_MIGRATION_BYTES
            || !(1..=49000).contains(&self.source_records)
            || source_caps.len() != self.source_capabilities.len()
            || !source_caps.contains("adoption_guard.v1")
            || !source_caps
                .iter()
                .all(|value| MEDIA_V1_CAPABILITIES[..7].contains(value))
            || target_caps.len() != self.required_capabilities.len()
            || target_caps != MEDIA_V1_CAPABILITIES.iter().copied().collect()
            || !exact_u64(&self.created_at)
            || !exact_u64(&self.expires_at)
            || self
                .created_at
                .parse::<u64>()
                .ok()
                .and_then(|now| now.checked_add(300))
                != self.expires_at.parse::<u64>().ok()
        {
            return Err(invalid());
        }
        let mut semantic = self.clone();
        semantic.hash.clear();
        semantic.id.clear();
        semantic.created_at.clear();
        semantic.expires_at.clear();
        semantic.source_capabilities.sort();
        semantic.required_capabilities.sort();
        let encoded = serde_json::to_vec(&semantic).map_err(|_| invalid())?;
        if encoded.len() > 32768 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Promotion review exceeds32KiB",
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"NumeraWiz.GenerationPromotionPlan.v1\0");
        hash.update(encoded);
        Ok(hex::encode(hash.finalize()))
    }
    pub fn validate_hash(&self) -> Result<()> {
        if !digest_value(&self.hash) || self.hash != self.digest()? {
            return Err(invalid());
        }
        Ok(())
    }
}

pub fn result_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(GenerationPromotionPlan)).expect("schema");
    let fields = &mut schema["properties"];
    assert!(fields.is_object(), "promotion schema fields");
    for (name, minimum, maximum) in [
        ("source_generation", 1, 16),
        ("rollback_generation", 2, 16),
        ("target_generation", 3, 16),
        ("source_activation_sequence", 1, 64),
        ("source_records", 1, 49000),
    ] {
        fields[name]["minimum"] = serde_json::json!(minimum);
        fields[name]["maximum"] = serde_json::json!(maximum);
    }
    fields["source_capabilities"]["maxItems"] = serde_json::json!(7);
    fields["source_capabilities"]["items"]["enum"] = serde_json::json!(&MEDIA_V1_CAPABILITIES[..7]);
    fields["required_capabilities"]["const"] = serde_json::json!(MEDIA_V1_CAPABILITIES);
    schema
}
