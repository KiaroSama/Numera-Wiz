//! Explicit data-root generation migration contracts. Native owner authority is required.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MIGRATION_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_STORE_VERSION: u32 = 3;
pub const MAX_GENERATIONS: u32 = 16;
pub const MAX_MIGRATION_RECORDS: u32 = 50000;
pub const MAX_MIGRATION_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const MIGRATION_SPACE_MARGIN_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub schema_version: u32,
    pub id: String,
    pub hash: String,
    pub root_id: String,
    pub source_generation: u32,
    pub source_activation_id: Option<String>,
    pub rollback_generation: u32,
    pub target_generation: u32,
    pub source_store_version: u32,
    pub target_store_version: u32,
    pub source_records_sha256: String,
    pub source_bytes: String,
    pub source_records: u32,
    pub application_build: String,
    pub required_capabilities: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
}
fn exact_u64(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|number| number.to_string() == value)
}
fn digest_value(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
impl MigrationPlan {
    pub fn digest(&self) -> Result<String> {
        if self.schema_version != MIGRATION_SCHEMA_VERSION
            || uuid_text(&self.id).is_err() || uuid_text(&self.root_id).is_err()
            || self.source_generation == 0
            || self.rollback_generation <= self.source_generation
            || self.target_generation != self.rollback_generation.checked_add(1).unwrap_or(0)
            || self.source_activation_id.as_ref().is_some_and(|id| uuid_text(id).is_err())
            || self.target_generation > MAX_GENERATIONS
            || self.source_store_version != 2 || self.target_store_version != CURRENT_STORE_VERSION
            || !digest_value(&self.source_records_sha256) || !digest_value(&self.application_build)
            || !exact_u64(&self.source_bytes) || self.source_bytes.parse::<u64>().unwrap_or(u64::MAX) > MAX_MIGRATION_BYTES
            || self.source_records == 0 || self.source_records > MAX_MIGRATION_RECORDS
            || !exact_u64(&self.created_at) || !exact_u64(&self.expires_at)
            || self.expires_at.parse::<u64>().unwrap_or(0) <= self.created_at.parse::<u64>().unwrap_or(u64::MAX)
            || self.required_capabilities.is_empty() || self.required_capabilities.len() > 64
        {
            return Err(DomainError::new("INVALID_MIGRATION_PLAN", "Migration identity, versions, source facts or bounds are invalid"));
        }
        let mut capabilities = std::collections::BTreeSet::new();
        for capability in &self.required_capabilities {
            if capability.is_empty() || capability.len() > 128
                || !capability.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                || !capabilities.insert(capability)
            {
                return Err(DomainError::new("INVALID_MIGRATION_PLAN", "Migration capabilities must be distinct bounded identifiers"));
            }
        }
        let mut semantic = self.clone();
        semantic.hash.clear();
        semantic.id.clear();
        semantic.created_at.clear();
        semantic.expires_at.clear();
        semantic.required_capabilities.sort();
        let bytes = serde_json::to_vec(&semantic).map_err(|_| DomainError::new("INVALID_MIGRATION_PLAN", "Cannot encode migration plan"))?;
        if bytes.len() > 32 * 1024 {
            return Err(DomainError::new("RESOURCE_LIMIT","Migration plan exceeds32KiB"));
        }
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    pub fn validate_hash(&self) -> Result<()> {
        if self.hash != self.digest()? { return Err(DomainError::new("INVALID_MIGRATION_PLAN", "Migration plan hash differs")); }
        Ok(())
    }
}
fn uuid_text(value: &str) -> Result<()> {
    // UUID syntax is validated without accepting path components or incidental labels.
    let canonical = value.len() == 36 && value.bytes().enumerate().all(|(index,byte)| {
        if [8,13,18,23].contains(&index) { byte == b'-' }
        else { byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) }
    });
    if canonical { Ok(()) } else { Err(DomainError::new("VALIDATION_ERROR","A canonical UUID is required")) }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MigrationSubmitInput { pub plan: MigrationPlan }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Accepted,
    Quiescing,
    Closed,
    Copied,
    Validated,
    Activated,
    Succeeded,
    RollbackAccepted,
    RolledBack,
    Blocked,
}
impl MigrationPhase {
    pub fn terminal(self) -> bool {
        matches!(self,Self::Succeeded|Self::RolledBack|Self::Blocked)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MigrationRecord {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub plan: MigrationPlan,
    pub phase: MigrationPhase,
    pub accepted_at: String,
    pub completed_at: Option<String>,
    pub activation_id: Option<String>,
    pub error: Option<DomainError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RollbackInput {
    pub migration_id: String,
    pub expected_activation_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationSummary {
    pub generation: u32,
    pub store_version: u32,
    pub root_id: String,
    pub retained_bytes: String,
    pub current: bool,
    pub rollback_eligible: bool,
    pub adoption_revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MigrationView {
    pub schema_version: u32,
    pub root_id: String,
    pub active_generation: u32,
    pub store_version: u32,
    pub mode: String,
    pub operation: Option<MigrationRecord>,
    pub generations: Vec<GenerationSummary>,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> MigrationPlan {
        MigrationPlan {
            schema_version:1,id:"01234567-89ab-cdef-0123-456789abcdef".into(),hash:String::new(),
            root_id:"11234567-89ab-cdef-0123-456789abcdef".into(),source_generation:1,source_activation_id:None,
            rollback_generation:2,target_generation:3,source_store_version:2,target_store_version:3,
            source_records_sha256:"ab".repeat(32),source_bytes:"1000".into(),source_records:20,
            application_build:"cd".repeat(32),required_capabilities:vec!["cleanup.v2".into(),"receipts.v1".into()],
            created_at:"100".into(),expires_at:"200".into(),
        }
    }
    #[test]
    fn migration_hash_binds_data_versions_and_rollback_generation_without_creation_noise() {
        let mut plan = plan();
        let original = plan.digest().unwrap();
        plan.id = "21234567-89ab-cdef-0123-456789abcdef".into();
        plan.created_at = "110".into(); plan.expires_at = "210".into();
        assert_eq!(plan.digest().unwrap(),original);
        plan.source_records_sha256 = "ee".repeat(32);
        assert_ne!(plan.digest().unwrap(),original);
        plan.target_generation = 4;
        assert!(plan.digest().is_err(),"Migration cannot point at an unrelated generation");
    }
}
