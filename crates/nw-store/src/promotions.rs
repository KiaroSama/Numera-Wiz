//! Genuine compatible-source review; no journal, floor, guard or receipt is written.
use crate::{RECORDS, Store, adoption::Guard, copies, decode, generations::RootLease, storage};
use nw_domain::{
    DomainError, Result,
    maintenance::{
        MigrationPhase,
        promotion::{
            GenerationPromotionPlan, MEDIA_V1_CAPABILITIES, PromotionKind, RollbackCondition,
            SourceGuard, TargetProfile,
        },
    },
};

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("PROMOTION_UNQUALIFIED", message)
}
impl Store {
    /// The caller holds exclusive maintenance admission and has drained accepted work.
    /// Close/reopen may change allocator bytes; no logical source record is written.
    pub fn prepare_media_promotion(
        &self,
        lease: &RootLease,
        application_build: &str,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<GenerationPromotionPlan> {
        if self.pending_migration()?.is_some() {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "Accepted migration recovery must finish before promotion review",
            ));
        }
        if self.store_version()? != 3 {
            return Err(invalid(
                "Use the supported legacy schema2-to3 migration before this review",
            ));
        }
        let selection = lease.selection()?;
        let prefix = lease.compatible_prefix()?;
        let guard: Guard = {
            let read = self.db.begin_read().map_err(storage)?;
            let table = read.open_table(RECORDS).map_err(storage)?;
            let value = table
                .get("meta/rollback-guard")
                .map_err(storage)?
                .ok_or_else(|| invalid("A missing adoption guard is unknown, never zero"))?;
            decode(value.value())?
        };
        if guard.validate()? == 0 {
            return Err(invalid(
                "This review requires prior adoption; zero-adoption initialization remains in development",
            ));
        }
        if selection.activation_id.as_deref() != Some(prefix.activation_id.as_str())
            || prefix.root_id != self.root_id()
            || guard.activation_id != prefix.activation_id
            || guard.migration_id != prefix.operation_id
        {
            return Err(invalid(
                "Source guard and selected compatible prefix provenance differ",
            ));
        }
        let migration = self.inspect_migration(&guard.migration_id)?;
        migration.plan.validate_hash()?;
        if migration.schema_version != 1
            || migration.id != guard.migration_id
            || migration.plan.id != guard.migration_id
            || !matches!(migration.phase, MigrationPhase::Succeeded)
            || migration.plan.root_id != self.root_id()
            || migration.plan.target_generation != selection.generation
            || migration.activation_id.as_deref() != Some(prefix.activation_id.as_str())
        {
            return Err(invalid(
                "The original compatible migration is not a completed trusted ancestor",
            ));
        }
        self.validate_migration_records()?;
        self.close_generation(lease)?;
        let result = (|| {
            let facts = lease.inspect_selected()?;
            if facts.store_version != 3
                || facts.root_id != self.root_id()
                || facts.record_count > 49000
                || facts.required_capabilities.iter().any(|capability| {
                    !crate::generations::CURRENT_CAPABILITIES.contains(&capability.as_str())
                })
            {
                return Err(invalid(
                    "Source floor, identity or raw record headroom is not eligible",
                ));
            }
            let rollback_generation = selection
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("Generation identity overflow"))?;
            let target_generation = rollback_generation
                .checked_add(1)
                .ok_or_else(|| invalid("Generation identity overflow"))?;
            if target_generation > nw_domain::maintenance::MAX_GENERATIONS {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Promotion exceeds the existing generation limit",
                ));
            }
            // Two independent copies plus conservative COW headroom; no space is reserved.
            let source_bytes: u64 = facts.store_bytes.parse().map_err(storage)?;
            let estimated_additional_bytes = source_bytes
                .checked_mul(3)
                .and_then(|bytes| bytes.checked_add(64 * 1024 * 1024))
                .ok_or_else(|| invalid("Generation space estimate overflow"))?;
            copies::require_copy_space(lease.state(), source_bytes, 3)?;
            let now = clock()?;
            let mut plan = GenerationPromotionPlan {
                schema_version: 1,
                kind: PromotionKind::GenerationPromotionV1,
                id: uuid::Uuid::new_v4().to_string(),
                hash: String::new(),
                root_id: self.root_id().into(),
                source_generation: selection.generation,
                source_activation_id: prefix.activation_id,
                source_activation_sequence: prefix.sequence,
                source_prefix_sha256: prefix.sha256,
                source_store_version: facts.store_version,
                source_capabilities: facts.required_capabilities,
                source_records_sha256: facts.records_sha256,
                source_records: facts.record_count,
                source_bytes: facts.store_bytes,
                estimated_additional_bytes: estimated_additional_bytes.to_string(),
                source_guard: SourceGuard {
                    migration_id: guard.migration_id,
                    activation_id: guard.activation_id,
                    revision: guard.revision,
                },
                rollback_generation,
                target_generation,
                target_profile: TargetProfile::MediaV1,
                required_capabilities: MEDIA_V1_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                rollback_condition: RollbackCondition::StoppedRootRestorationBeforeAdoption,
                application_build: application_build.into(),
                created_at: now.to_string(),
                expires_at: now
                    .checked_add(300)
                    .ok_or_else(|| invalid("Review clock overflow"))?
                    .to_string(),
            };
            plan.hash = plan.digest()?;
            Ok(plan)
        })();
        let reopened = self.reopen_current_generation(lease);
        match (result, reopened) {
            (Ok(plan), Ok(())) => Ok(plan),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}
