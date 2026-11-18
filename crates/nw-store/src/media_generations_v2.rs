//! Read-side ancestry checks for MediaV2 maintenance.
use crate::{
    RECORDS, Store, copies, decode, encode,
    generations::{self, Activation, GenerationLocation, GenerationRole, RootLease},
    media_generations::{self, Enablement},
    records::record_quota,
    restoration_control, storage,
};
use nw_domain::{
    DomainError, Result,
    commands::{Request, Response},
    maintenance::{
        media_lifecycle::EnablementPhase,
        media_v2::{
            EnablementKind, EnablementPlanV2, EnablementSubmitV2, MEDIA_V2_CAPABILITIES,
            SourceMediaV1, SourceOrigin, TargetProfileV2, exact_capabilities,
        },
        promotion::MEDIA_V1_CAPABILITIES,
    },
};
use nw_platform::files::DirectoryStamp;
use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub(crate) const PENDING_MIXED_ENABLEMENT: &str = "meta/pending-media-v2";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordV2 {
    schema_version: u32,
    creator: String,
    plan: EnablementPlanV2,
    receipt_key: String,
    request_digest: String,
    activation_id: String,
    phase: EnablementPhase,
    accepted_at: String,
    completed_at: Option<String>,
    predecessor_restoration: Option<String>,
}

impl RecordV2 {
    pub(crate) fn operation(&self) -> Enablement {
        let plan = &self.plan;
        let source = &plan.source;
        Enablement {
            id: plan.id.clone(),
            initialization: plan.kind == EnablementKind::MediaInitializationV2,
            mixed_media: true,
            root_id: plan.root_id.clone(),
            receipt_key: self.receipt_key.clone(),
            request_digest: self.request_digest.clone(),
            source_generation: source.generation,
            source_activation_id: source.activation_id.clone(),
            source_operation_id: source.origin_operation_id.clone(),
            source_revision: source.guard_revision.clone(),
            source_sequence: source.activation_sequence,
            source_prefix_sha256: source.activation_prefix_sha256.clone(),
            source_records_sha256: source.records_sha256.clone(),
            source_records: source.records,
            source_bytes: source.bytes.clone(),
            source_capabilities: source.capabilities.clone(),
            target_generation: plan.target_generation,
            activation_id: self.activation_id.clone(),
            phase: self.phase,
            predecessor_restoration: self.predecessor_restoration.clone(),
        }
    }

    pub(crate) fn validate(&self, id: &str, root: &str) -> Result<()> {
        self.plan.validate_hash()?;
        nw_domain::projects::validate_id(&self.activation_id)?;
        if let Some(predecessor) = &self.predecessor_restoration {
            nw_domain::projects::validate_id(predecessor)?;
        }
        let accepted = nw_domain::media_execution::exact(&self.accepted_at)?;
        if self.schema_version != 2
            || self.plan.id != id
            || self.plan.root_id != root
            || self.creator.is_empty()
            || accepted == 0
            || match (&self.phase, &self.completed_at) {
                (EnablementPhase::Accepted, None) => false,
                (EnablementPhase::Succeeded, Some(finished)) => {
                    nw_domain::media_execution::exact(finished)? < accepted
                }
                _ => true,
            }
        {
            return Err(unsupported("Mixed enablement journal identity differs"));
        }
        Ok(())
    }
}

fn record_key(id: &str) -> String {
    format!("generation-media-v2/{id}")
}

fn unsupported(message: &str) -> DomainError {
    DomainError::new("INVALID_GENERATION_PLAN", message)
}

fn transform_matches(actual: &Enablement, expected: &Enablement) -> bool {
    actual.id == expected.id
        && actual.initialization == expected.initialization
        && actual.root_id == expected.root_id
        && actual.receipt_key == expected.receipt_key
        && actual.request_digest == expected.request_digest
        && actual.source_generation == expected.source_generation
        && actual.source_activation_id == expected.source_activation_id
        && actual.source_operation_id == expected.source_operation_id
        && actual.source_revision == expected.source_revision
        && actual.source_sequence == expected.source_sequence
        && actual.source_prefix_sha256 == expected.source_prefix_sha256
        && actual.source_records_sha256 == expected.source_records_sha256
        && actual.source_records == expected.source_records
        && actual.source_bytes == expected.source_bytes
        && actual.source_capabilities == expected.source_capabilities
        && actual.target_generation == expected.target_generation
        && actual.activation_id == expected.activation_id
        && actual.phase == EnablementPhase::Accepted
        && actual.predecessor_restoration == expected.predecessor_restoration
}

impl Store {
    pub(crate) fn mixed_enablement(&self, id: &str) -> Result<Enablement> {
        nw_domain::projects::validate_id(id)?;
        let retained: RecordV2 = self
            .get(&record_key(id))?
            .ok_or_else(DomainError::object_not_found)?;
        retained.validate(id, self.root_id())?;
        Ok(retained.operation())
    }

    pub fn review_media_v2(
        &self,
        lease: &RootLease,
        kind: EnablementKind,
        application_build: &str,
        clock: impl FnOnce() -> Result<u64>,
        cancel: &CancellationToken,
    ) -> Result<EnablementPlanV2> {
        let source = self.prove_media_v1_source(lease, cancel)?;
        let rollback_generation = source
            .generation
            .checked_add(1)
            .ok_or_else(|| unsupported("Rollback generation number is exhausted"))?;
        let target_generation = rollback_generation
            .checked_add(1)
            .ok_or_else(|| unsupported("Target generation number is exhausted"))?;
        let bytes = nw_domain::media_execution::exact(&source.bytes)?;
        let estimated = bytes
            .checked_mul(3)
            .and_then(|bytes| bytes.checked_add(64 * 1024 * 1024))
            .ok_or_else(|| {
                unsupported("Generation copy byte estimate exceeds its integer range")
            })?;
        copies::require_copy_space(lease.state(), bytes, 3)?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Mixed media generation review was cancelled",
            ));
        }
        let now = clock()?;
        let expires_at = now
            .checked_add(300)
            .ok_or_else(|| unsupported("Review expiry exceeds its integer range"))?;
        let mut plan = EnablementPlanV2 {
            schema_version: 2,
            kind,
            id: uuid::Uuid::new_v4().to_string(),
            hash: String::new(),
            root_id: self.root_id().to_owned(),
            source,
            rollback_generation,
            target_generation,
            target_profile: TargetProfileV2::MediaV2,
            required_capabilities: MEDIA_V2_CAPABILITIES
                .iter()
                .map(|value| (*value).into())
                .collect(),
            estimated_additional_bytes: estimated.to_string(),
            application_build: application_build.to_owned(),
            created_at: now.to_string(),
            expires_at: expires_at.to_string(),
        };
        plan.hash = plan.digest()?;
        Ok(plan)
    }

    pub fn accept_media_v2(
        &self,
        lease: &RootLease,
        principal: &str,
        request: &Request,
        build: &str,
        clock: impl Fn() -> Result<u64>,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if request.command.id != "maintenance.media_v2.accept" {
            return Err(unsupported("Mixed enablement acceptance command differs"));
        }
        if let Some(response) = self.replay_with_clock(principal, request, &clock)? {
            return Ok(response);
        }
        let input: EnablementSubmitV2 = nw_domain::commands::parse(request.params.clone())?;
        let plan = input.plan;
        plan.validate_hash()?;
        let current = self.review_media_v2(lease, plan.kind, build, &clock, cancel)?;
        if current.hash != plan.hash
            || !nw_domain::media_execution::review_current(
                &plan.created_at,
                &plan.expires_at,
                clock()?,
            )?
        {
            return Err(unsupported(
                "Mixed enablement source or review lifetime changed",
            ));
        }
        nw_platform::generation_restoration::QualifiedDirectory::open(lease.state())?;
        copies::reserve_restoration_slot(lease, &plan.id)?;
        self.validate_migration_records()?;
        let path = self.generation_path()?;
        self.close_generation(lease)?;
        let prepared = (|| {
            let copy = copies::copy_locked(lease, &plan.id, "rollback", &path, cancel)?;
            if copy.facts.root_id != plan.root_id
                || copy.facts.records_sha256 != plan.source.records_sha256
                || copy.facts.record_count != plan.source.records
                || !exact_capabilities(&copy.facts.required_capabilities, MEDIA_V1_CAPABILITIES)
            {
                return Err(unsupported(
                    "Rollback copy differs from the reviewed native source",
                ));
            }
            media_generations::flush_copy(&copy.path)
        })();
        let reopened = self.reopen_current_generation(lease);
        prepared?;
        reopened?;
        let activation_id = uuid::Uuid::new_v4().to_string();
        let predecessor_restoration = self.terminal_restoration_predecessor()?;
        let (receipt_key, _) = self.replay_key(principal, request, clock()?)?;
        let key = record_key(&plan.id);
        let response = self.commit_composed_with_clock(principal, request, &clock, |table, now| {
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Mixed enablement was cancelled before acceptance"));
            }
            let history = generations::activations(lease.root())?;
            if media_generations::prefix_digest(&history) != plan.source.activation_prefix_sha256
                || media_generations::raw_hash(table)? != (plan.source.records, plan.source.records_sha256.clone())
                || !nw_domain::media_execution::review_current(&plan.created_at, &plan.expires_at, now)?
            {
                return Err(unsupported("Mixed enablement source changed before atomic acceptance"));
            }
            copies::restoration_slot(lease, &plan.id)?;
            record_quota(table, &key)?;
            let record = RecordV2 {
                schema_version: 2,
                creator: principal.to_owned(),
                plan: plan.clone(),
                receipt_key: receipt_key.clone(),
                request_digest: request.structural_digest(),
                activation_id: activation_id.clone(),
                phase: EnablementPhase::Accepted,
                accepted_at: now.to_string(),
                completed_at: None,
                predecessor_restoration: predecessor_restoration.clone(),
            };
            if table.insert(key.as_str(), encode(&record)?.as_slice()).map_err(storage)?.is_some() {
                return Err(unsupported("Mixed enablement identity already exists"));
            }
            table.insert("meta/required-capabilities", encode(&MEDIA_V2_CAPABILITIES)?.as_slice()).map_err(storage)?;
            table.insert(PENDING_MIXED_ENABLEMENT, plan.id.as_bytes()).map_err(storage)?;
            Ok(serde_json::json!({"enablement_id": plan.id, "target_generation":plan.target_generation, "state":"accepted"}))
        })?;
        if let Err(error) = self.recover_media_v2(lease, &clock) {
            tracing::warn!(code=%error.code, "Accepted mixed enablement remains fenced for recovery");
        }
        Ok(response)
    }

    pub(crate) fn recover_media_v2(
        &self,
        lease: &RootLease,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        let (key, mut record) = {
            let read = self.db.begin_read().map_err(storage)?;
            let table = read.open_table(RECORDS).map_err(storage)?;
            let Some(pending) = table.get(PENDING_MIXED_ENABLEMENT).map_err(storage)? else {
                return Ok(());
            };
            let id = std::str::from_utf8(pending.value()).map_err(storage)?;
            nw_domain::projects::validate_id(id)?;
            let key = record_key(id);
            let retained = table
                .get(key.as_str())
                .map_err(storage)?
                .ok_or_else(|| unsupported("Accepted mixed operation is missing"))?;
            let record: RecordV2 = decode(retained.value())?;
            record.validate(id, self.root_id())?;
            if record.phase != EnablementPhase::Accepted {
                return Err(unsupported("Accepted mixed operation identity differs"));
            }
            restoration_control::receipt(
                &table,
                &record.receipt_key,
                &record.request_digest,
                id,
                "enablement_id",
            )?;
            (key, record)
        };
        let operation = record.operation();
        let selection = lease.selection()?;
        if selection.activation_id.as_deref() != Some(record.activation_id.as_str()) {
            let history = generations::activations(lease.root())?;
            if selection.activation_id.as_deref() != Some(operation.source_activation_id.as_str())
                || selection.generation != operation.source_generation
                || media_generations::prefix_digest(&history) != operation.source_prefix_sha256
            {
                return Err(unsupported("Accepted mixed source is no longer selected"));
            }
            let workspaces =
                DirectoryStamp::capture_resolved(&lease.state().join("generation-workspaces"))?;
            let workspace = workspaces.child_directory(record.plan.id.as_ref())?;
            let rollback = workspace.child_directory("rollback".as_ref())?;
            let prior = copies::inspect_completed_copy(&rollback, &CancellationToken::new())?;
            if prior.operation_id != record.plan.id
                || prior.role != "rollback"
                || prior.source.root_id != record.plan.root_id
                || prior.source.record_count != operation.source_records
                || prior.source.records_sha256 != operation.source_records_sha256
                || !exact_capabilities(&prior.source.required_capabilities, MEDIA_V1_CAPABILITIES)
            {
                return Err(unsupported(
                    "Accepted mixed rollback copy differs from the reviewed source",
                ));
            }
            self.close_generation(lease)?;
            let switched = (|| {
                self.prepare_media_target(lease, &operation)?;
                lease.append_activation(Activation {
                    schema_version: 1,
                    sequence: 0,
                    id: record.activation_id.clone(),
                    operation_id: record.plan.id.clone(),
                    root_id: self.root_id().to_owned(),
                    generation: record.plan.target_generation,
                    store_version: 3,
                    location: GenerationLocation::Workspace {
                        operation_id: record.plan.id.clone(),
                        role: GenerationRole::Target,
                    },
                    required_capabilities: record.plan.required_capabilities.clone(),
                    previous_sha256: None,
                })?;
                self.attach_media_selection(lease)
            })();
            if let Err(error) = switched {
                if lease.selection()?.activation_id.as_deref()
                    == Some(record.activation_id.as_str())
                {
                    self.attach_media_selection(lease)?;
                } else {
                    self.reopen_current_generation(lease)?;
                }
                return Err(error);
            }
        }
        media_generations::flush_activation(lease)?;
        let write = self.db.begin_control_write().map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let guard = media_generations::guard(&table)?;
            let marker = format!("generation-transform/{}", record.plan.id);
            let encoded = encode(&operation)?;
            let transformed = table
                .get(marker.as_str())
                .map_err(storage)?
                .is_some_and(|value| value.value() == encoded.as_slice());
            if guard.migration_id != record.plan.id
                || guard.activation_id != record.activation_id
                || guard.validate()? != 0
                || !transformed
            {
                return Err(unsupported("Mixed target guard differs before completion"));
            }
            let now = clock()?;
            crate::check_clock(&table, now)?;
            record.phase = EnablementPhase::Succeeded;
            record.completed_at = Some(now.to_string());
            table
                .insert(key.as_str(), encode(&record)?.as_slice())
                .map_err(storage)?;
            table.remove(PENDING_MIXED_ENABLEMENT).map_err(storage)?;
            table
                .insert("meta/clock", now.to_string().as_bytes())
                .map_err(storage)?;
        }
        write.commit().map_err(storage)
    }

    fn prove_media_v1_source(
        &self,
        lease: &RootLease,
        cancel: &CancellationToken,
    ) -> Result<SourceMediaV1> {
        if self.pending_migration()?.is_some() || self.pending_media_maintenance()? {
            return Err(unsupported(
                "Complete accepted maintenance before reviewing a new generation",
            ));
        }
        let workspaces =
            DirectoryStamp::capture_resolved(&lease.state().join("generation-workspaces"))?;
        let history = generations::activations(lease.root())?;
        let (active, _) = history
            .last()
            .ok_or_else(|| unsupported("MediaV1 source has no selected activation"))?;
        let selection = lease.selection()?;
        let selected = lease.path(&selection)?;
        if active.root_id != self.root_id()
            || selection.activation_id.as_deref() != Some(active.id.as_str())
            || active.location
                != (GenerationLocation::Workspace {
                    operation_id: active.operation_id.clone(),
                    role: GenerationRole::Target,
                })
            || active.store_version != 3
            || !exact_capabilities(&active.required_capabilities, MEDIA_V1_CAPABILITIES)
            || self.generation_path()? != selected
        {
            return Err(unsupported(
                "Selected native generation is outside the exact MediaV1 source profile",
            ));
        }
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let cap_row = table
            .get("meta/required-capabilities")
            .map_err(storage)?
            .ok_or_else(|| unsupported("Selected MediaV1 capabilities are missing"))?;
        let caps: Vec<String> = decode(cap_row.value())?;
        if !exact_capabilities(&caps, MEDIA_V1_CAPABILITIES)
            || table
                .get("meta/schema")
                .map_err(storage)?
                .as_ref()
                .map(|value| value.value())
                != Some(b"3".as_slice())
        {
            return Err(unsupported(
                "Selected store does not hold the exact MediaV1 reader floor",
            ));
        }
        let guard = media_generations::guard(&table)?;
        if guard.migration_id != active.operation_id || guard.activation_id != active.id {
            return Err(unsupported(
                "Selected MediaV1 adoption guard differs from the actual activation",
            ));
        }
        let operation_id = &active.operation_id;
        let initialization_key = format!("generation-initialization/{operation_id}");
        let promotion_key = format!("generation-promotion/{operation_id}");
        let (origin, plan_hash, receipt_field, operation) = match (
            table
                .get(initialization_key.as_str())
                .map_err(storage)?
                .is_some(),
            table
                .get(promotion_key.as_str())
                .map_err(storage)?
                .is_some(),
        ) {
            (true, false) => (
                SourceOrigin::MediaV1Initialization,
                self.inspect_media_initialization(operation_id)?.plan.hash,
                "initialization_id",
                self.enablement(operation_id, true)?,
            ),
            (false, true) => (
                SourceOrigin::MediaV1Promotion,
                self.inspect_media_promotion(operation_id)?.plan.hash,
                "promotion_id",
                self.enablement(operation_id, false)?,
            ),
            _ => {
                return Err(unsupported(
                    "Selected MediaV1 origin record is missing or ambiguous",
                ));
            }
        };
        if operation.phase != EnablementPhase::Succeeded
            || operation.target_generation != active.generation
            || operation.activation_id != active.id
        {
            return Err(unsupported(
                "MediaV1 origin is outside the completed source lineage",
            ));
        }
        if let Some(predecessor) = &operation.predecessor_restoration {
            // RootLease has already verified and bound the retained authority chain.
            let key = format!("restoration-control/{predecessor}");
            let raw = table
                .get(key.as_str())
                .map_err(storage)?
                .ok_or_else(|| unsupported("Origin restoration barrier is missing"))?;
            let barrier: restoration_control::ExposureBarrier = decode(raw.value())?;
            if barrier.schema_version != 1
                || barrier.restoration_id != *predecessor
                || barrier.root_id != active.root_id
                || barrier.source_operation_id != operation.source_operation_id
                || barrier.source_activation_id != operation.source_activation_id
            {
                return Err(unsupported(
                    "Origin restoration barrier differs from its predecessor",
                ));
            }
        }
        let source_sequence = usize::try_from(operation.source_sequence).map_err(storage)?;
        let prefix = history
            .get(..source_sequence)
            .ok_or_else(|| unsupported("MediaV1 predecessor prefix is missing"))?;
        let (predecessor, _) = prefix
            .last()
            .ok_or_else(|| unsupported("MediaV1 predecessor prefix is empty"))?;
        if predecessor.id != operation.source_activation_id
            || predecessor.operation_id != operation.source_operation_id
            || predecessor.generation != operation.source_generation
            || media_generations::prefix_digest(prefix) != operation.source_prefix_sha256
        {
            return Err(unsupported(
                "MediaV1 origin no longer matches its captured predecessor prefix",
            ));
        }
        let transform_key = format!("generation-transform/{operation_id}");
        let transform = table
            .get(transform_key.as_str())
            .map_err(storage)?
            .ok_or_else(|| unsupported("MediaV1 origin transformation is missing"))?;
        if !transform_matches(&decode(transform.value())?, &operation) {
            return Err(unsupported(
                "MediaV1 transformation differs from the completed origin",
            ));
        }
        let receipt = restoration_control::receipt(
            &table,
            &operation.receipt_key,
            &operation.request_digest,
            operation_id,
            receipt_field,
        )?;
        let operation_workspace = workspaces.child_directory(operation_id.as_ref())?;
        let rollback = operation_workspace.child_directory("rollback".as_ref())?;
        let manifest = copies::inspect_completed_copy(&rollback, cancel)?;
        if manifest.role != "rollback"
            || manifest.operation_id != *operation_id
            || manifest.source.root_id != active.root_id
            || manifest.source.record_count != operation.source_records
            || manifest.source.records_sha256 != operation.source_records_sha256
        {
            return Err(unsupported(
                "MediaV1 rollback copy does not prove the original source",
            ));
        }
        operation_workspace.revalidate()?;
        workspaces.revalidate()?;
        let (records, records_sha256) = media_generations::raw_hash(&table)?;
        let stamp =
            nw_platform::files::SourceStamp::capture_resolved(&selected.join("store.redb"))?;
        let bytes = stamp.byte_len()?.to_string();
        lease.revalidate()?;
        Ok(SourceMediaV1 {
            origin,
            origin_operation_id: operation_id.clone(),
            origin_plan_sha256: plan_hash,
            origin_receipt_sha256: nw_domain::projects::hash(&receipt),
            generation: active.generation,
            activation_id: active.id.clone(),
            activation_sequence: active.sequence,
            activation_prefix_sha256: media_generations::prefix_digest(&history),
            store_version: active.store_version,
            capabilities: caps,
            guard_revision: guard.revision,
            records_sha256,
            records,
            bytes,
        })
    }
}
