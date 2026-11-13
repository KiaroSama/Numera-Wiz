//! Explicit generation enablement/restoration contracts, separate from migration v1.
use super::{MAX_GENERATIONS, MAX_MIGRATION_BYTES};
use crate::{
    DomainError, Result,
    media_execution::{exact, hash},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnablementPhase {
    Accepted,
    Succeeded,
    Restored,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromotionSubmitInput {
    pub plan: super::promotion::GenerationPromotionPlan,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationPromotionRecord {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub receipt_key: String,
    pub request_digest: String,
    pub plan: super::promotion::GenerationPromotionPlan,
    pub activation_id: String,
    pub predecessor_restoration: Option<String>,
    pub phase: EnablementPhase,
    pub accepted_at: String,
    pub completed_at: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InitializationKind {
    MediaInitializationV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InitializationOrigin {
    CompatibleGenesisV1,
    MigratedZeroRevision,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaInitializationPlan {
    pub schema_version: u32,
    pub kind: InitializationKind,
    pub id: String,
    pub hash: String,
    pub root_id: String,
    pub source_generation: u32,
    pub source_activation_id: String,
    pub source_activation_sequence: u32,
    pub source_prefix_sha256: String,
    pub source_records_sha256: String,
    pub source_records: u32,
    pub source_bytes: String,
    pub source_capabilities: Vec<String>,
    pub source_operation_id: String,
    pub source_guard_revision: String,
    pub origin: InitializationOrigin,
    pub rollback_generation: u32,
    pub target_generation: u32,
    pub required_capabilities: Vec<String>,
    pub estimated_additional_bytes: String,
    pub application_build: String,
    pub created_at: String,
    pub expires_at: String,
}
impl MediaInitializationPlan {
    pub fn digest(&self) -> Result<String> {
        for id in [
            &self.id,
            &self.root_id,
            &self.source_activation_id,
            &self.source_operation_id,
        ] {
            crate::projects::validate_id(id)?;
        }
        for digest in [
            &self.source_prefix_sha256,
            &self.source_records_sha256,
            &self.application_build,
        ] {
            hash(digest)?;
        }
        if self.schema_version != 1
            || self.source_guard_revision != "0"
            || self.source_generation == 0
            || self.source_generation.checked_add(1) != Some(self.rollback_generation)
            || self.rollback_generation.checked_add(1) != Some(self.target_generation)
            || self.target_generation > MAX_GENERATIONS
            || !(1..=64).contains(&self.source_activation_sequence)
            || !(1..=49000).contains(&self.source_records)
            || !(1..=MAX_MIGRATION_BYTES).contains(&exact(&self.source_bytes)?)
            || exact(&self.source_bytes)?
                .checked_mul(3)
                .and_then(|n| n.checked_add(64 * 1024 * 1024))
                != Some(exact(&self.estimated_additional_bytes)?)
            || exact(&self.created_at)?.checked_add(300) != Some(exact(&self.expires_at)?)
            || !same_caps(
                &self.source_capabilities,
                &super::promotion::MEDIA_V1_CAPABILITIES[..7],
            )
            || !same_caps(
                &self.required_capabilities,
                super::promotion::MEDIA_V1_CAPABILITIES,
            )
        {
            return Err(invalid(
                "Initialization review eligibility or finite bounds differ",
            ));
        }
        let mut semantic = self.clone();
        semantic.id.clear();
        semantic.hash.clear();
        semantic.created_at.clear();
        semantic.expires_at.clear();
        semantic.source_capabilities.sort();
        semantic.required_capabilities.sort();
        digest(b"NumeraWiz.MediaInitializationPlan.v1\0", &semantic)
    }
    pub fn validate_hash(&self) -> Result<()> {
        hash(&self.hash)?;
        if self.hash != self.digest()? {
            return Err(invalid("Initialization review digest differs"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitializationSubmitInput {
    pub plan: MediaInitializationPlan,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaInitializationRecord {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub receipt_key: String,
    pub request_digest: String,
    pub plan: MediaInitializationPlan,
    pub activation_id: String,
    pub predecessor_restoration: Option<String>,
    pub phase: EnablementPhase,
    pub accepted_at: String,
    pub completed_at: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RestorationKind {
    GenerationPromotionRestorationV1,
    InitializationRestorationV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationRestorationPlan {
    pub schema_version: u32,
    pub kind: RestorationKind,
    pub id: String,
    pub hash: String,
    pub enablement_id: String,
    pub root_id: String,
    pub source_generation: u32,
    pub source_activation_id: String,
    pub source_operation_id: String,
    pub source_guard_revision: String,
    pub prefix_count: u32,
    pub prefix_sha256: String,
    pub fenced_activation_id: String,
    pub history_sha256: String,
    pub history_count: u32,
    pub source_records_sha256: String,
    pub promoted_records_sha256: String,
    pub source_bytes: String,
    pub promoted_bytes: String,
    pub estimated_additional_bytes: String,
    pub application_build: String,
    pub created_at: String,
    pub expires_at: String,
}
impl GenerationRestorationPlan {
    pub fn digest(&self) -> Result<String> {
        for id in [
            &self.id,
            &self.enablement_id,
            &self.root_id,
            &self.source_activation_id,
            &self.source_operation_id,
            &self.fenced_activation_id,
        ] {
            crate::projects::validate_id(id)?;
        }
        for value in [
            &self.prefix_sha256,
            &self.history_sha256,
            &self.source_records_sha256,
            &self.promoted_records_sha256,
            &self.application_build,
        ] {
            hash(value)?;
        }
        let revision = exact(&self.source_guard_revision)?;
        let source = exact(&self.source_bytes)?;
        let promoted = exact(&self.promoted_bytes)?;
        if self.schema_version != 1
            || self.source_generation == 0
            || self.source_generation > MAX_GENERATIONS
            || self.prefix_count == 0
            || self.history_count <= self.prefix_count
            || self.history_count > 64
            || source == 0
            || source > MAX_MIGRATION_BYTES
            || promoted == 0
            || promoted > MAX_MIGRATION_BYTES
            || source
                .checked_add(promoted)
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(64 * 1024 * 1024))
                != Some(exact(&self.estimated_additional_bytes)?)
            || exact(&self.created_at)?.checked_add(300) != Some(exact(&self.expires_at)?)
            || match self.kind {
                RestorationKind::GenerationPromotionRestorationV1 => revision == 0,
                RestorationKind::InitializationRestorationV1 => revision != 0,
            }
        {
            return Err(invalid(
                "Restoration review eligibility or finite bounds differ",
            ));
        }
        let mut semantic = self.clone();
        semantic.id.clear();
        semantic.hash.clear();
        semantic.created_at.clear();
        semantic.expires_at.clear();
        digest(
            match self.kind {
                RestorationKind::GenerationPromotionRestorationV1 => {
                    b"NumeraWiz.GenerationPromotionRestoration.v1\0"
                }
                RestorationKind::InitializationRestorationV1 => {
                    b"NumeraWiz.InitializationRestoration.v1\0"
                }
            },
            &semantic,
        )
    }
    pub fn validate_hash(&self) -> Result<()> {
        hash(&self.hash)?;
        if self.hash != self.digest()? {
            return Err(invalid("Restoration review digest differs"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestorationSubmitInput {
    pub plan: GenerationRestorationPlan,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationRestorationRecord {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub receipt_key: String,
    pub request_digest: String,
    pub plan: GenerationRestorationPlan,
    pub accepted_at: String,
}
fn same_caps(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
            == expected.iter().copied().collect()
}
fn digest(prefix: &[u8], value: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))?;
    if encoded.len() > 32768 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Generation review exceeds32KiB",
        ));
    }
    let mut bytes = prefix.to_vec();
    bytes.extend(encoded);
    Ok(crate::projects::hash(&bytes))
}
fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::new("INVALID_GENERATION_PLAN", message)
}
