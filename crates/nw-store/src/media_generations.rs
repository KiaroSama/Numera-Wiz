//! Accepted media floor transitions retain the source guard and transform only a distinct target.
use crate::{
    RECORDS, Store,
    adoption::Guard,
    copies, decode, encode,
    generations::{self, Activation, GenerationLocation, GenerationRole, RootLease},
    records::record_quota,
    storage, versions,
};
use nw_domain::{
    DomainError, Result,
    commands::{Request, Response},
    maintenance::{
        media_lifecycle::*,
        promotion::{GenerationPromotionPlan, MEDIA_V1_CAPABILITIES},
    },
};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Enablement {
    pub id: String,
    pub initialization: bool,
    pub root_id: String,
    pub receipt_key: String,
    pub request_digest: String,
    pub source_generation: u32,
    pub source_activation_id: String,
    pub source_operation_id: String,
    pub source_revision: String,
    pub source_sequence: u32,
    pub source_prefix_sha256: String,
    pub source_records_sha256: String,
    pub source_records: u32,
    pub source_bytes: String,
    pub source_capabilities: Vec<String>,
    pub target_generation: u32,
    pub activation_id: String,
    pub phase: EnablementPhase,
    pub predecessor_restoration: Option<String>,
}
impl Enablement {
    pub fn pending_key(&self) -> &'static str {
        if self.initialization {
            "meta/pending-initialization"
        } else {
            "meta/pending-promotion"
        }
    }
    pub fn record_key(&self) -> String {
        format!(
            "{}/{}",
            if self.initialization {
                "generation-initialization"
            } else {
                "generation-promotion"
            },
            self.id
        )
    }
    pub fn source_path(&self, lease: &RootLease) -> Result<std::path::PathBuf> {
        let history = generations::activations(lease.root())?;
        let (activation, _) = history
            .get(self.source_sequence.saturating_sub(1) as usize)
            .ok_or_else(|| invalid("Original compatible prefix is missing"))?;
        if activation.id != self.source_activation_id
            || activation.generation != self.source_generation
        {
            return Err(invalid("Original compatible source identity differs"));
        }
        generations::location_path(lease.root(), &activation.location)
    }
}
pub(crate) fn raw_hash(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
) -> Result<(u32, String)> {
    let mut hash = Sha256::new();
    hash.update(b"numera-generation-records-v1\0");
    let mut count = 0u32;
    for row in table.iter().map_err(storage)? {
        let (key, bytes) = row.map_err(storage)?;
        count += 1;
        if count > versions::MAX_GENERATION_RECORDS
            || key.value().len() > 512
            || bytes.value().len() > crate::MAX_RECORD
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Generation raw facts exceed their inspection bounds",
            ));
        }
        hash.update((key.value().len() as u64).to_le_bytes());
        hash.update(key.value().as_bytes());
        hash.update((bytes.value().len() as u64).to_le_bytes());
        hash.update(bytes.value());
    }
    Ok((count, hex::encode(hash.finalize())))
}
fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::new("INVALID_GENERATION_PLAN", message)
}
pub(crate) fn guard(table: &impl ReadableTable<&'static str, &'static [u8]>) -> Result<Guard> {
    let row = table
        .get("meta/rollback-guard")
        .map_err(storage)?
        .ok_or_else(|| invalid("Missing guard is unknown, never zero"))?;
    let guard: Guard = decode(row.value())?;
    guard.validate()?;
    Ok(guard)
}
pub(crate) fn prefix_digest(history: &[(Activation, String)]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"NumeraWiz.CompatibleActivationPrefix.v1\0");
    for (record, file_hash) in history {
        hash.update(record.sequence.to_le_bytes());
        hash.update(file_hash.as_bytes());
    }
    hex::encode(hash.finalize())
}
impl Store {
    pub fn pending_media_maintenance(&self) -> Result<bool> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        for key in [
            "meta/pending-promotion",
            "meta/pending-initialization",
            "meta/pending-restoration",
        ] {
            if table.get(key).map_err(storage)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn inspect_media_promotion(&self, id: &str) -> Result<GenerationPromotionRecord> {
        nw_domain::projects::validate_id(id)?;
        let mut record: GenerationPromotionRecord = self
            .get(&format!("generation-promotion/{id}"))?
            .ok_or_else(DomainError::object_not_found)?;
        record.plan.validate_hash()?;
        if record.schema_version != 1
            || record.id != id
            || record.plan.id != id
            || record.plan.root_id != self.root_id()
        {
            return Err(invalid("Accepted promotion identity differs"));
        }
        if self.has_restored_enablement(id)? {
            record.phase = EnablementPhase::Restored;
        }
        Ok(record)
    }
    pub fn inspect_media_initialization(&self, id: &str) -> Result<MediaInitializationRecord> {
        nw_domain::projects::validate_id(id)?;
        let mut record: MediaInitializationRecord = self
            .get(&format!("generation-initialization/{id}"))?
            .ok_or_else(DomainError::object_not_found)?;
        record.plan.validate_hash()?;
        if record.schema_version != 1
            || record.id != id
            || record.plan.id != id
            || record.plan.root_id != self.root_id()
        {
            return Err(invalid("Accepted initialization identity differs"));
        }
        if self.has_restored_enablement(id)? {
            record.phase = EnablementPhase::Restored;
        }
        Ok(record)
    }
    pub(crate) fn enablement(&self, id: &str, initialization: bool) -> Result<Enablement> {
        if initialization {
            let record = self.inspect_media_initialization(id)?;
            let p = record.plan;
            Ok(Enablement {
                id: record.id,
                initialization,
                root_id: p.root_id,
                receipt_key: record.receipt_key,
                request_digest: record.request_digest,
                source_generation: p.source_generation,
                source_activation_id: p.source_activation_id,
                source_operation_id: p.source_operation_id,
                source_revision: p.source_guard_revision,
                source_sequence: p.source_activation_sequence,
                source_prefix_sha256: p.source_prefix_sha256,
                source_records_sha256: p.source_records_sha256,
                source_records: p.source_records,
                source_bytes: p.source_bytes,
                source_capabilities: p.source_capabilities,
                target_generation: p.target_generation,
                activation_id: record.activation_id,
                phase: record.phase,
                predecessor_restoration: record.predecessor_restoration,
            })
        } else {
            let record = self.inspect_media_promotion(id)?;
            let p = record.plan;
            Ok(Enablement {
                id: record.id,
                initialization,
                root_id: p.root_id,
                receipt_key: record.receipt_key,
                request_digest: record.request_digest,
                source_generation: p.source_generation,
                source_activation_id: p.source_activation_id,
                source_operation_id: p.source_guard.migration_id,
                source_revision: p.source_guard.revision,
                source_sequence: p.source_activation_sequence,
                source_prefix_sha256: p.source_prefix_sha256,
                source_records_sha256: p.source_records_sha256,
                source_records: p.source_records,
                source_bytes: p.source_bytes,
                source_capabilities: p.source_capabilities,
                target_generation: p.target_generation,
                activation_id: record.activation_id,
                phase: record.phase,
                predecessor_restoration: record.predecessor_restoration,
            })
        }
    }
    pub fn accept_media_promotion(
        &self,
        lease: &RootLease,
        principal: &str,
        request: &Request,
        build: &str,
        clock: impl Fn() -> Result<u64>,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if request.command.id != "maintenance.promotion.accept" {
            return Err(invalid("Promotion acceptance command differs"));
        }
        if let Some(response) = self.replay_with_clock(principal, request, &clock)? {
            return Ok(response);
        }
        let input: PromotionSubmitInput = nw_domain::commands::parse(request.params.clone())?;
        let plan = input.plan;
        plan.validate_hash()?;
        self.prepare_enablement_acceptance(lease, &plan, build, clock()?, cancel)?;
        let activation = uuid::Uuid::new_v4().to_string();
        let (receipt_key, _) = self.replay_key(principal, request, clock()?)?;
        let predecessor = self.terminal_restoration_predecessor()?;
        let response = self.commit_composed_with_clock(principal, request, &clock, |table, now| {
            copies::restoration_slot(lease, &plan.id)?;
            if !nw_domain::media_execution::review_current(&plan.created_at, &plan.expires_at, now)? { return Err(invalid("Promotion review expired")); }
            if raw_hash(table)? != (plan.source_records, plan.source_records_sha256.clone()) {
                return Err(invalid("Full source raw facts changed before atomic promotion acceptance"));
            }
            let selection = lease.selection()?;
            let prefix = lease.compatible_prefix()?;
            let (original, _) = crate::initialization::source_ancestry(table, self.root_id(), &selection, &prefix)?;
            if selection.generation != plan.source_generation || prefix.sha256 != plan.source_prefix_sha256
                || prefix.sequence != plan.source_activation_sequence || prefix.activation_id != plan.source_activation_id
                || original.migration_id != plan.source_guard.migration_id || original.activation_id != plan.source_guard.activation_id
                || original.revision != plan.source_guard.revision || original.validate()? == 0 {
                return Err(invalid("Original source adoption provenance changed"));
            }
            let record = GenerationPromotionRecord {
                schema_version: 1, id: plan.id.clone(), creator: principal.into(), plan: plan.clone(),
                receipt_key: receipt_key.clone(), request_digest: request.structural_digest(),
                activation_id: activation.clone(), predecessor_restoration: predecessor.clone(), phase: EnablementPhase::Accepted,
                accepted_at: now.to_string(), completed_at: None,
            };
            let key = format!("generation-promotion/{}", plan.id);
            record_quota(table, &key)?;
            if table.get(key.as_str()).map_err(storage)?.is_some() { return Err(invalid("Promotion identity already exists")); }
            // Preserve original source guard bytes. Only the independently copied target gets a new epoch.
            table.insert("meta/required-capabilities", encode(&MEDIA_V1_CAPABILITIES)?.as_slice()).map_err(storage)?;
            table.insert("meta/pending-promotion", plan.id.as_bytes()).map_err(storage)?;
            table.insert(key.as_str(), encode(&record)?.as_slice()).map_err(storage)?;
            Ok(serde_json::json!({"promotion_id":plan.id,"target_generation":plan.target_generation,"state":"accepted"}))
        })?;
        if let Err(error) = self.recover_media_enablement(lease, &clock) {
            tracing_error(&error);
        }
        Ok(response)
    }
    fn prepare_enablement_acceptance(
        &self,
        lease: &RootLease,
        plan: &GenerationPromotionPlan,
        build: &str,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if self.pending_migration()?.is_some() || self.pending_media_maintenance()? {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "Accepted generation maintenance must finish first",
            ));
        }
        if plan.root_id != self.root_id()
            || plan.application_build != build
            || !nw_domain::media_execution::review_current(&plan.created_at, &plan.expires_at, now)?
        {
            return Err(invalid("Promotion review root/build/lifetime differs"));
        }
        let selection = lease.selection()?;
        let prefix = lease.compatible_prefix()?;
        if selection.generation != plan.source_generation
            || selection.activation_id.as_deref() != Some(plan.source_activation_id.as_str())
            || prefix.sha256 != plan.source_prefix_sha256
            || prefix.sequence != plan.source_activation_sequence
        {
            return Err(invalid("Reviewed compatible prefix changed"));
        }
        {
            let read = self.db.begin_read().map_err(storage)?;
            let table = read.open_table(RECORDS).map_err(storage)?;
            let (original, _) = crate::initialization::source_ancestry(&table, self.root_id(), &selection, &prefix)?;
            if original.migration_id != plan.source_guard.migration_id
                || original.activation_id != plan.source_guard.activation_id
                || original.revision != plan.source_guard.revision || original.validate()? == 0
            {
                return Err(invalid("Original source adoption provenance changed"));
            }
        }
        copies::reserve_restoration_slot(lease, &plan.id)?;
        self.validate_migration_records()?;
        self.close_generation(lease)?;
        let preparation = (|| {
            let facts = lease.inspect_selected()?;
            if facts.store_version != 3
                || facts.record_count != plan.source_records
                || facts.records_sha256 != plan.source_records_sha256
                || facts.required_capabilities != plan.source_capabilities
                || facts.required_capabilities.len() != generations::CURRENT_CAPABILITIES.len()
                || facts
                    .required_capabilities
                    .iter()
                    .any(|cap| !generations::CURRENT_CAPABILITIES.contains(&cap.as_str()))
            {
                return Err(invalid(
                    "Reviewed source floor or complete raw snapshot changed",
                ));
            }
            copies::require_copy_space(
                lease.state(),
                facts.store_bytes.parse().map_err(storage)?,
                3,
            )?;
            // Qualification precedes acceptance; unsupported local filesystems cannot acquire a floor.
            nw_platform::generation_restoration::QualifiedDirectory::open(lease.state())?;
            let copy = copies::copy_locked(
                lease,
                &plan.id,
                "rollback",
                &self.generation_path()?,
                cancel,
            )?;
            flush_copy(&copy.path)?;
            Ok(())
        })();
        let reopened = self.reopen_current_generation(lease);
        preparation?;
        reopened?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Promotion cancelled before acceptance",
            ));
        }
        Ok(())
    }
    pub fn recover_media_enablement(
        &self,
        lease: &RootLease,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        let pending = {
            let read = self.db.begin_read().map_err(storage)?;
            let table = read.open_table(RECORDS).map_err(storage)?;
            let mut pending = None;
            for (key, initialization) in [
                ("meta/pending-promotion", false),
                ("meta/pending-initialization", true),
            ] {
                if let Some(row) = table.get(key).map_err(storage)? {
                    if pending.is_some() {
                        return Err(invalid("Multiple accepted enablements conflict"));
                    }
                    pending = Some((
                        String::from_utf8(row.value().to_vec()).map_err(storage)?,
                        initialization,
                    ));
                }
            }
            pending
        };
        let Some((id, initialization)) = pending else {
            return Ok(());
        };
        let operation = self.enablement(&id, initialization)?;
        if operation.phase == EnablementPhase::Restored {
            return Err(invalid("A historical restored operation cannot rerun"));
        }
        let selection = lease.selection()?;
        if selection.activation_id.as_deref() != Some(operation.activation_id.as_str()) {
            if selection.activation_id.as_deref() != Some(operation.source_activation_id.as_str())
                || selection.generation != operation.source_generation
            {
                return Err(invalid("Accepted enablement source is no longer selected"));
            }
            self.close_generation(lease)?;
            let switched = (|| {
                self.prepare_media_target(lease, &operation)?;
                lease.append_activation(Activation {
                    schema_version: 1,
                    sequence: 0,
                    id: operation.activation_id.clone(),
                    operation_id: operation.id.clone(),
                    root_id: self.root_id().into(),
                    generation: operation.target_generation,
                    store_version: 3,
                    location: GenerationLocation::Workspace {
                        operation_id: operation.id.clone(),
                        role: GenerationRole::Target,
                    },
                    required_capabilities: MEDIA_V1_CAPABILITIES
                        .iter()
                        .map(|value| (*value).into())
                        .collect(),
                    previous_sha256: None,
                })?;
                self.attach_media_selection(lease)
            })();
            if let Err(error) = switched {
                if lease.selection()?.activation_id.as_deref()
                    == Some(operation.activation_id.as_str())
                {
                    self.attach_media_selection(lease)?;
                } else {
                    self.reopen_current_generation(lease)?;
                }
                return Err(error);
            }
        }
        flush_activation(lease)?;
        let write = self.db.begin_control_write().map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let target_guard = guard(&table)?;
            if target_guard.migration_id != id
                || target_guard.activation_id != operation.activation_id
                || target_guard.validate()? != 0
            {
                return Err(invalid("Target enablement guard differs before completion"));
            }
            let now = clock()?;
            crate::check_clock(&table, now)?;
            let bytes = if initialization {
                let mut record = self.inspect_media_initialization(&id)?;
                record.phase = EnablementPhase::Succeeded;
                record.completed_at = Some(now.to_string());
                encode(&record)?
            } else {
                let mut record = self.inspect_media_promotion(&id)?;
                record.phase = EnablementPhase::Succeeded;
                record.completed_at = Some(now.to_string());
                encode(&record)?
            };
            table
                .insert(operation.record_key().as_str(), bytes.as_slice())
                .map_err(storage)?;
            table.remove(operation.pending_key()).map_err(storage)?;
            table
                .insert("meta/clock", now.to_string().as_bytes())
                .map_err(storage)?;
        }
        write.commit().map_err(storage)
    }
    fn prepare_media_target(&self, lease: &RootLease, operation: &Enablement) -> Result<()> {
        let source = self.generation_path()?;
        let target = copies::workspace(lease, &operation.id)?.join("target");
        let marker = format!("generation-transform/{}", operation.id);
        if !target.join("store.redb").try_exists().map_err(storage)? {
            copies::copy_locked(
                lease,
                &operation.id,
                "target",
                &source,
                &CancellationToken::new(),
            )?;
        }
        let manifest: copies::CopyManifest = decode(&nw_platform::security::read_private(
            &target.join("copy.json"),
            65536,
        )?)?;
        let source_facts = versions::inspect_closed(&source)?;
        if manifest.schema_version != 1
            || manifest.operation_id != operation.id
            || manifest.role != "target"
            || manifest.source.root_id != operation.root_id
            || manifest.source.records_sha256 != source_facts.records_sha256
            || manifest.source.record_count != source_facts.record_count
        {
            return Err(invalid(
                "Target has no accepted immutable source-copy provenance",
            ));
        }
        // Inspect a dirty target through the existing owned recovery probe before
        // allowing the original file to be repaired. Only either complete state
        // of our one transaction is eligible; neither path recopies an existing file.
        self.db.retain_existing_lock(&target)?;
        let facts = versions::preflight_existing_facts(&target)?;
        let transformed_hash = transformed_target_hash(&source, operation)?;
        let transformed = facts.records_sha256 == transformed_hash
            && facts.record_count == source_facts.record_count + 1;
        if !transformed
            && (facts.records_sha256 != source_facts.records_sha256
                || facts.record_count != source_facts.record_count)
        {
            return Err(invalid(
                "Target is neither the exact source nor the complete accepted transformation",
            ));
        }
        let identity = nw_platform::files::source_object(&target.join("store.redb"))?;
        let database = self.open_retained_control(lease, &target, &identity)?;
        if !transformed {
            let mut write = database.begin_write().map_err(storage)?;
            write
                .set_durability(redb::Durability::Immediate)
                .map_err(storage)?;
            write.set_two_phase_commit(true);
            {
                let mut table = write.open_table(RECORDS).map_err(storage)?;
                let original = guard(&table)?;
                if original.migration_id != operation.source_operation_id
                    || original.activation_id != operation.source_activation_id
                    || original.revision != operation.source_revision
                {
                    return Err(invalid(
                        "Copied source guard differs before target transformation",
                    ));
                }
                table
                    .insert(
                        "meta/rollback-guard",
                        encode(&Guard {
                            schema_version: 1,
                            migration_id: operation.id.clone(),
                            activation_id: operation.activation_id.clone(),
                            revision: "0".into(),
                        })?
                        .as_slice(),
                    )
                    .map_err(storage)?;
                record_quota(&table, &marker)?;
                table
                    .insert(marker.as_str(), encode(operation)?.as_slice())
                    .map_err(storage)?;
            }
            write.commit().map_err(storage)?;
        }
        drop(database);
        // A transformed target is proved against immutable copied source bytes; it is never recopied.
        let source_db = redb::Database::builder()
            .open_read_only(source.join("store.redb"))
            .map_err(versions::database_error)?;
        let target_db = redb::Database::builder()
            .open_read_only(target.join("store.redb"))
            .map_err(versions::database_error)?;
        let source_read = source_db.begin_read().map_err(storage)?;
        let target_read = target_db.begin_read().map_err(storage)?;
        let source_table = source_read.open_table(RECORDS).map_err(storage)?;
        let target_table = target_read.open_table(RECORDS).map_err(storage)?;
        for row in source_table.iter().map_err(storage)? {
            let (key, bytes) = row.map_err(storage)?;
            if key.value() == "meta/rollback-guard" {
                continue;
            }
            let actual = target_table
                .get(key.value())
                .map_err(storage)?
                .ok_or_else(|| invalid("Transformed target lost source history"))?;
            if actual.value() != bytes.value() {
                return Err(invalid("Transformed target changed source raw rows"));
            }
        }
        if target_table.len().map_err(storage)? != source_table.len().map_err(storage)? + 1 {
            return Err(invalid("Transformed target has unexplained extra rows"));
        }
        let row = target_table
            .get(marker.as_str())
            .map_err(storage)?
            .ok_or_else(|| invalid("Target transformation proof missing"))?;
        if row.value() != encode(operation)? {
            return Err(invalid(
                "Target transformation belongs to another accepted operation",
            ));
        }
        let target_guard = guard(&target_table)?;
        if target_guard.migration_id != operation.id
            || target_guard.activation_id != operation.activation_id
            || target_guard.validate()? != 0
        {
            return Err(invalid("New target guard is not its distinct zero epoch"));
        }
        drop(row);
        drop(target_table);
        drop(source_table);
        drop(target_read);
        drop(source_read);
        drop(target_db);
        drop(source_db);
        flush_copy(&target)
    }
    pub(crate) fn attach_media_selection(&self, lease: &RootLease) -> Result<()> {
        let path = lease.path(&lease.selection()?)?;
        let identity = nw_platform::files::source_object(&path.join("store.redb"))?;
        let database = self.open_retained_control(lease, &path, &identity)?;
        let adoption = crate::adoption::enabled(&database)?;
        self.db
            .attach_retained_generation(database, &path, adoption)
    }
}
pub(crate) fn flush_copy(path: &std::path::Path) -> Result<()> {
    let directory = nw_platform::generation_restoration::QualifiedDirectory::open(path)?;
    for name in ["store.redb", "copy.json", "complete.json"] {
        let file = path.join(name);
        let identity = nw_platform::files::identity(&file, &CancellationToken::new())?;
        directory.flush_final_file(
            name,
            &nw_platform::files::source_object(&file)?,
            &identity.sha256,
            versions::MAX_GENERATION_BYTES,
        )?;
    }
    Ok(())
}
pub(crate) fn flush_activation(lease: &RootLease) -> Result<()> {
    let history = generations::activations(lease.root())?;
    let (last, hash) = history
        .last()
        .ok_or_else(|| invalid("Activation to flush is missing"))?;
    let directory = lease.state().join("activations");
    let name = format!("{:010}.json", last.sequence);
    nw_platform::generation_restoration::QualifiedDirectory::open(&directory)?.flush_final_file(
        &name,
        &nw_platform::files::source_object(&directory.join(&name))?,
        hash,
        65536,
    )
}
fn tracing_error(error: &DomainError) {
    // Acceptance is already durable. Inspection/recovery retains the original receipt.
    tracing::warn!(code=%error.code, "Accepted media generation remains pending; retained source and target were preserved");
}

fn transformed_target_hash(source: &std::path::Path, operation: &Enablement) -> Result<String> {
    let database = redb::Database::builder()
        .open_read_only(source.join("store.redb"))
        .map_err(versions::database_error)?;
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    let marker = format!("generation-transform/{}", operation.id);
    if table.get(marker.as_str()).map_err(storage)?.is_some() {
        return Err(invalid(
            "Source already contains this target transformation",
        ));
    }
    let marker_bytes = encode(operation)?;
    let target_guard = encode(&Guard {
        schema_version: 1,
        migration_id: operation.id.clone(),
        activation_id: operation.activation_id.clone(),
        revision: "0".into(),
    })?;
    let mut hash = Sha256::new();
    hash.update(b"numera-generation-records-v1\0");
    let mut added = false;
    let mut append = |key: &str, raw: &[u8]| {
        hash.update((key.len() as u64).to_le_bytes());
        hash.update(key.as_bytes());
        hash.update((raw.len() as u64).to_le_bytes());
        hash.update(raw);
    };
    for row in table.iter().map_err(storage)? {
        let (key, raw) = row.map_err(storage)?;
        if !added && marker.as_str() < key.value() {
            append(&marker, &marker_bytes);
            added = true;
        }
        append(
            key.value(),
            if key.value() == "meta/rollback-guard" {
                &target_guard
            } else {
                raw.value()
            },
        );
    }
    if !added {
        append(&marker, &marker_bytes);
    }
    Ok(hex::encode(hash.finalize()))
}
