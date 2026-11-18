//! Closed maintenance reviews for the transition from MediaV1 to MediaV2.
use crate::{DomainError, Result, media_execution::{exact, hash}, projects};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MEDIA_V2_CAPABILITIES: [&str; 10] = [
    "access.v2",
    "cleanup.v2",
    "cleanup_batch.v1",
    "receipts.v1",
    "generation_activation.v1",
    "migration.v1",
    "adoption_guard.v1",
    "generation_promotion.v1",
    "media_execution.v1",
    "media_execution.v2",
];

pub fn exact_capabilities(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual.iter().map(String::as_str).collect::<BTreeSet<_>>() == expected.iter().copied().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnablementKind {
    MediaInitializationV2,
    GenerationPromotionV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    MediaV1Initialization,
    MediaV1Promotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetProfileV2 {
    MediaV2,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnablementReviewV2 {
    pub kind: EnablementKind,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnablementSubmitV2 {
    pub plan: EnablementPlanV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceMediaV1 {
    pub origin: SourceOrigin,
    pub origin_operation_id: String,
    pub origin_plan_sha256: String,
    pub origin_receipt_sha256: String,
    pub generation: u32,
    pub activation_id: String,
    pub activation_sequence: u32,
    pub activation_prefix_sha256: String,
    pub store_version: u32,
    #[schemars(length(min = 9, max = 9), extend("uniqueItems" = true))]
    pub capabilities: Vec<String>,
    pub guard_revision: String,
    pub records_sha256: String,
    pub records: u32,
    pub bytes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnablementPlanV2 {
    #[schemars(extend("const" = 2))]
    pub schema_version: u32,
    pub kind: EnablementKind,
    pub id: String,
    pub hash: String,
    pub root_id: String,
    pub source: SourceMediaV1,
    pub rollback_generation: u32,
    pub target_generation: u32,
    pub target_profile: TargetProfileV2,
    #[schemars(length(min = 10, max = 10), extend("uniqueItems" = true))]
    pub required_capabilities: Vec<String>,
    pub estimated_additional_bytes: String,
    pub application_build: String,
    pub created_at: String,
    pub expires_at: String,
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_PROMOTION_PLAN", message)
}

impl EnablementPlanV2 {
    pub fn digest(&self) -> Result<String> {
        let source = &self.source;
        for value in [&self.id, &self.root_id, &source.origin_operation_id, &source.activation_id] {
            projects::validate_id(value)?;
        }
        for value in [&self.application_build, &source.origin_plan_sha256, &source.origin_receipt_sha256, &source.activation_prefix_sha256, &source.records_sha256] {
            hash(value)?;
        }
        let guard = exact(&source.guard_revision)?;
        let bytes = exact(&source.bytes)?;
        let estimated = exact(&self.estimated_additional_bytes)?;
        if self.schema_version != 2 || source.store_version != 3
            || !exact_capabilities(&source.capabilities, super::promotion::MEDIA_V1_CAPABILITIES)
            || !exact_capabilities(&self.required_capabilities, &MEDIA_V2_CAPABILITIES)
            || source.generation == 0 || source.generation.checked_add(1) != Some(self.rollback_generation)
            || self.rollback_generation.checked_add(1) != Some(self.target_generation)
            || self.target_generation > super::MAX_GENERATIONS
            || !(1..=64).contains(&source.activation_sequence)
            || !(1..=49000).contains(&source.records) || !(1..=super::MAX_MIGRATION_BYTES).contains(&bytes)
            || bytes.checked_mul(3).and_then(|value| value.checked_add(64 * 1024 * 1024)) != Some(estimated)
            || (self.kind == EnablementKind::MediaInitializationV2) != (guard == 0)
            || exact(&self.created_at)?.checked_add(300) != Some(exact(&self.expires_at)?)
        {
            return Err(invalid("Mixed generation review differs from the exact source, target or guard policy"));
        }
        let mut semantic = self.clone();
        semantic.hash.clear();
        semantic.id.clear();
        semantic.created_at.clear();
        semantic.expires_at.clear();
        semantic.source.capabilities.sort();
        semantic.required_capabilities.sort();
        let encoded = serde_json::to_vec(&semantic).map_err(|_| invalid("Cannot encode mixed generation review"))?;
        if encoded.len() > 32 * 1024 {
            return Err(invalid("Mixed generation review exceeds the 32 KiB budget"));
        }
        let mut framed = match self.kind {
            EnablementKind::MediaInitializationV2 => b"NumeraWiz.MediaInitializationPlan.v2\0".to_vec(),
            EnablementKind::GenerationPromotionV2 => b"NumeraWiz.GenerationPromotionPlan.v2\0".to_vec(),
        };
        framed.extend(encoded);
        Ok(projects::hash(&framed))
    }

    pub fn validate_hash(&self) -> Result<()> {
        hash(&self.hash)?;
        if self.digest()? != self.hash {
            return Err(invalid("Mixed generation review hash mismatch"));
        }
        Ok(())
    }
}
