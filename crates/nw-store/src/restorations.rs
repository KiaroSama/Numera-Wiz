//! Accepted stopped restoration: independent archives, atomic overlay+B, forward-only exposure.
use crate::{
    RECORDS, Store, copies, decode, encode,
    generations::{self, Activation, GenerationLocation, RootLease},
    media_generations::{Enablement, LifecycleTiming, flush_copy, prefix_digest, raw_hash},
    records::record_quota,
    restoration_control::{self as control, ExposureBarrier},
    storage, versions,
};
use nw_domain::{
    DomainError, Result,
    commands::{Request, Response},
    maintenance::media_lifecycle::*,
};
use nw_platform::{files, generation_restoration::QualifiedDirectory, security};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchivedActivation {
    pub name: String,
    pub sha256: String,
    pub activation: Activation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Authority {
    pub schema_version: u32,
    pub record: GenerationRestorationRecord,
    pub operation: Enablement,
    pub source_location: GenerationLocation,
    pub source_prestate: versions::GenerationFacts,
    pub promoted_archive: versions::GenerationFacts,
    pub barrier: ExposureBarrier,
    pub history: Vec<ArchivedActivation>,
}
fn refused(message: impl Into<String>) -> DomainError {
    DomainError::new("RESTORATION_REFUSED", message)
}
fn open_closed(path: &Path) -> Result<redb::ReadOnlyDatabase> {
    redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(path.join("store.redb"))
        .map_err(versions::database_error)
}
fn operation_path(lease: &RootLease, id: &str) -> Result<PathBuf> {
    nw_domain::projects::validate_id(id)?;
    Ok(lease.state().join("generation-workspaces").join(id))
}
fn verify_copy(path: &Path) -> Result<versions::GenerationFacts> {
    let mut timing = LifecycleTiming::start("Read immutable archive proof and physical hash");
    let manifest: copies::CopyManifest =
        decode(&security::read_private(&path.join("copy.json"), 65536)?)?;
    let identity = files::identity(&path.join("store.redb"), &CancellationToken::new())?;
    timing.advance("Inspect immutable archive raw records");
    let facts = versions::inspect_closed(path)?;
    if identity.sha256 != manifest.source_file_sha256
        || identity.bytes != manifest.source_file_bytes
        || facts != manifest.source
    {
        return Err(refused(
            "Independent archive no longer matches its immutable copy proof",
        ));
    }
    let complete: serde_json::Value =
        decode(&security::read_private(&path.join("complete.json"), 65536)?)?;
    if complete["store_sha256"].as_str() != Some(identity.sha256.as_str())
        || complete["records_sha256"].as_str() != Some(facts.records_sha256.as_str())
    {
        return Err(refused("Independent archive completion proof differs"));
    }
    Ok(facts)
}
fn reserve_restoration(lease: &RootLease, source: u64, promoted: u64) -> Result<u64> {
    let additional = source
        .checked_add(promoted)
        .and_then(|bytes| bytes.checked_mul(2))
        .and_then(|bytes| bytes.checked_add(64 * 1024 * 1024))
        .ok_or_else(|| refused("Restoration aggregate space estimate overflow"))?;
    if source == 0
        || promoted == 0
        || source > versions::MAX_GENERATION_BYTES
        || promoted > versions::MAX_GENERATION_BYTES
        || fs2::available_space(lease.state()).map_err(storage)? < additional
    {
        return Err(DomainError::new(
            "SCRATCH_EXHAUSTED",
            "Independent source/promoted archives, overlay and control COW headroom do not fit",
        ));
    }
    Ok(additional)
}
impl Store {
    pub(crate) fn has_restored_enablement(&self, id: &str) -> Result<bool> {
        let mut restored = false;
        self.visit::<ExposureBarrier>("restoration-control/", |barrier| {
            if barrier.root_id != self.root_id() || barrier.schema_version != 1 {
                return Err(refused("Retained restoration barrier identity differs"));
            }
            restored |= barrier.enablement_id == id;
            Ok(())
        })?;
        Ok(restored)
    }
    pub fn prepare_media_restoration(
        &self,
        lease: &RootLease,
        enablement_id: &str,
        initialization: bool,
        build: &str,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<GenerationRestorationPlan> {
        let mut timing = LifecycleTiming::start("Validate restoration lineage and workspace");
        if self.pending_migration()?.is_some() || self.pending_media_maintenance()? {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "Accepted maintenance must finish before restoration review",
            ));
        }
        let operation = self.enablement(enablement_id, initialization)?;
        let restoration_id = copies::restoration_slot(lease, enablement_id)?;
        if operation.phase != EnablementPhase::Succeeded {
            return Err(refused("Restoration requires completed genuine enablement"));
        }
        let history = generations::activations(lease.root())?;
        let prefix = operation.source_sequence as usize;
        if prefix == 0
            || prefix >= history.len()
            || history[prefix].0.id != operation.activation_id
            || prefix_digest(&history[..prefix]) != operation.source_prefix_sha256
            || history[..prefix].iter().any(|(entry, _)| {
                entry
                    .required_capabilities
                    .iter()
                    .any(|cap| !generations::CURRENT_CAPABILITIES.contains(&cap.as_str()))
            })
        {
            return Err(refused(
                "Restoration requires the original nonempty compatible prefix and real incompatible successor",
            ));
        }
        if lease.selection()?.activation_id.as_deref() != Some(operation.activation_id.as_str()) {
            return Err(refused(
                "A later active generation requires its own reviewed restoration",
            ));
        }
        self.validate_migration_records_for_media()?;
        timing.advance("Close restoration target and qualify source");
        self.close_generation(lease)?;
        let result = (|| {
            let source = operation.source_path(lease)?;
            self.db.retain_existing_lock(&source)?;
            QualifiedDirectory::open(&source)?;
            QualifiedDirectory::open(&lease.state().join("activations"))?;
            let baseline = operation_path(lease, &operation.id)?.join("rollback");
            timing.advance("Verify restoration baseline archive");
            let baseline_facts = verify_copy(&baseline)?;
            if baseline_facts.records_sha256 != operation.source_records_sha256
                || baseline_facts.record_count != operation.source_records
            {
                return Err(refused(
                    "Original rollback snapshot differs from accepted enablement",
                ));
            }
            timing.advance("Inspect restoration source and promoted raw records");
            let source_facts = versions::inspect_closed(&source)?;
            let promoted_facts = versions::inspect_closed(&self.generation_path()?)?;
            timing.advance("Validate restoration prestate and space admission");
            let baseline_db = open_closed(&baseline)?;
            let source_db = open_closed(&source)?;
            let promoted_db = open_closed(&self.generation_path()?)?;
            let baseline_read = baseline_db.begin_read().map_err(storage)?;
            let source_read = source_db.begin_read().map_err(storage)?;
            let promoted_read = promoted_db.begin_read().map_err(storage)?;
            control::validate_prestate(
                &baseline_read.open_table(RECORDS).map_err(storage)?,
                &source_read.open_table(RECORDS).map_err(storage)?,
                &promoted_read.open_table(RECORDS).map_err(storage)?,
                &operation,
                None,
            )?;
            let additional = reserve_restoration(
                lease,
                source_facts.store_bytes.parse().map_err(storage)?,
                promoted_facts.store_bytes.parse().map_err(storage)?,
            )?;
            let now = clock()?;
            let mut plan = GenerationRestorationPlan {
                schema_version: 1,
                kind: if initialization {
                    RestorationKind::InitializationRestorationV1
                } else {
                    RestorationKind::GenerationPromotionRestorationV1
                },
                id: restoration_id,
                hash: String::new(),
                enablement_id: operation.id,
                root_id: self.root_id().into(),
                source_generation: operation.source_generation,
                source_activation_id: operation.source_activation_id,
                source_operation_id: operation.source_operation_id,
                source_guard_revision: operation.source_revision,
                prefix_count: prefix as u32,
                prefix_sha256: prefix_digest(&history[..prefix]),
                fenced_activation_id: operation.activation_id,
                history_sha256: prefix_digest(&history),
                history_count: history.len() as u32,
                source_records_sha256: source_facts.records_sha256,
                promoted_records_sha256: promoted_facts.records_sha256,
                source_bytes: source_facts.store_bytes,
                promoted_bytes: promoted_facts.store_bytes,
                estimated_additional_bytes: additional.to_string(),
                application_build: build.into(),
                created_at: now.to_string(),
                expires_at: now
                    .checked_add(300)
                    .ok_or_else(|| refused("Review clock overflow"))?
                    .to_string(),
            };
            plan.hash = plan.digest()?;
            Ok(plan)
        })();
        timing.advance("Reattach reviewed restoration target");
        let reopened = self.reopen_current_generation(lease);
        match (result, reopened) {
            (Ok(plan), Ok(())) => Ok(plan),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
    pub fn accept_media_restoration(
        &self,
        lease: &RootLease,
        principal: &str,
        request: &Request,
        initialization: bool,
        build: &str,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<Response> {
        let expected = if initialization {
            "maintenance.initialization.restore.accept"
        } else {
            "maintenance.promotion.restore.accept"
        };
        if request.command.id != expected {
            return Err(refused("Restoration command identity differs"));
        }
        if let Some(replay) = self.replay_with_clock(principal, request, &clock)? {
            return Ok(replay);
        }
        let input: RestorationSubmitInput = nw_domain::commands::parse(request.params.clone())?;
        let plan = input.plan;
        plan.validate_hash()?;
        if matches!(plan.kind, RestorationKind::InitializationRestorationV1) != initialization
            || plan.application_build != build
            || plan.root_id != self.root_id()
            || !nw_domain::media_execution::review_current(
                &plan.created_at,
                &plan.expires_at,
                clock()?,
            )?
        {
            return Err(refused(
                "Restoration review kind, root, build or lifetime differs",
            ));
        }
        let mut timing = LifecycleTiming::start("Revalidate restoration review");
        let mut current = self.prepare_media_restoration(
            lease,
            &plan.enablement_id,
            initialization,
            build,
            &clock,
        )?;
        // Physical allocator length may change on reopen; full raw snapshots and actual
        // current copy-space admission above remain authoritative freshness evidence.
        current.source_bytes = plan.source_bytes.clone();
        current.promoted_bytes = plan.promoted_bytes.clone();
        current.estimated_additional_bytes = plan.estimated_additional_bytes.clone();
        if current.id != plan.id || current.digest()? != plan.hash {
            return Err(refused("Reviewed restoration source/history changed"));
        }
        let (receipt_key, _) = self.replay_key(principal, request, clock()?)?;
        timing.advance("Commit restoration receipt and pending record");
        let response = self.commit_composed_with_clock(principal, request, &clock, |table, now| {
            if copies::restoration_slot(lease, &plan.enablement_id)? != plan.id { return Err(refused("Restoration reservation changed")); }
            if !nw_domain::media_execution::review_current(&plan.created_at, &plan.expires_at, now)?
                || raw_hash(table)?.1 != plan.promoted_records_sha256
            {
                return Err(refused("Restoration source changed or expired before acceptance"));
            }
            let record = GenerationRestorationRecord {
                schema_version: 1, id: plan.id.clone(), creator: principal.into(),
                receipt_key: receipt_key.clone(), request_digest: request.structural_digest(),
                plan: plan.clone(), accepted_at: now.to_string(),
            };
            let key = format!("generation-restoration/{}", plan.id);
            record_quota(table, &key)?;
            if table.get(key.as_str()).map_err(storage)?.is_some() { return Err(refused("Restoration identity already exists")); }
            table.insert(key.as_str(), encode(&record)?.as_slice()).map_err(storage)?;
            table.insert("meta/pending-restoration", plan.id.as_bytes()).map_err(storage)?;
            Ok(serde_json::json!({"restoration_id":plan.id,"source_generation":plan.source_generation,"state":"accepted"}))
        })?;
        timing.advance("Recover accepted restoration");
        if let Err(error) = self.recover_media_restoration(lease) {
            tracing::warn!(code=%error.code, "Accepted restoration remains pending; no source rewind is permitted after B");
        }
        Ok(response)
    }
    pub fn inspect_media_restoration(&self, id: &str) -> Result<GenerationRestorationRecord> {
        nw_domain::projects::validate_id(id)?;
        let record: GenerationRestorationRecord = self
            .get(&format!("generation-restoration/{id}"))?
            .ok_or_else(DomainError::object_not_found)?;
        record.plan.validate_hash()?;
        if record.schema_version != 1
            || record.id != id
            || record.plan.id != id
            || record.plan.root_id != self.root_id()
        {
            return Err(refused("Restoration record identity differs"));
        }
        Ok(record)
    }
    pub fn media_restoration_status(&self, id: &str) -> Result<&'static str> {
        self.inspect_media_restoration(id)?;
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let barrier = table
            .get(format!("restoration-control/{id}").as_str())
            .map_err(storage)?;
        if let Some(barrier) = barrier {
            let barrier: ExposureBarrier = decode(barrier.value())?;
            if barrier.schema_version != 1
                || barrier.restoration_id != id
                || barrier.root_id != self.root_id()
            {
                return Err(refused(
                    "Restoration inspection found contradictory barrier identity",
                ));
            }
            return Ok("armed");
        }
        Ok("accepted")
    }
    pub fn recover_media_restoration(&self, lease: &RootLease) -> Result<()> {
        let id = {
            let read = self.db.begin_read().map_err(storage)?;
            let table = read.open_table(RECORDS).map_err(storage)?;
            table
                .get("meta/pending-restoration")
                .map_err(storage)?
                .map(|row| String::from_utf8(row.value().to_vec()).map_err(storage))
                .transpose()?
        };
        let Some(id) = id else {
            return Ok(());
        };
        let mut timing = LifecycleTiming::start("Read accepted restoration and close target");
        let record = self.inspect_media_restoration(&id)?;
        let operation = self.enablement(
            &record.plan.enablement_id,
            matches!(
                record.plan.kind,
                RestorationKind::InitializationRestorationV1
            ),
        )?;
        self.close_generation(lease)?;
        let recovered = (|| {
            timing.advance("Prepare independent restoration authority");
            let authority = self.prepare_restoration_authority(lease, &record, &operation)?;
            timing.advance("Install and classify restoration exposure barrier");
            self.install_restoration_overlay(lease, &authority)?;
            timing.advance("Expose compatible prefix and retain observation");
            expose_prefix(lease, &authority)?;
            complete_observation(lease, &authority)?;
            timing.advance("Attach restored source selection");
            self.attach_media_selection(lease)
        })();
        if let Err(error) = recovered {
            // Reattach the currently selected complete generation, never a saved baseline.
            self.attach_media_selection(lease)?;
            return Err(error);
        }
        Ok(())
    }
    fn prepare_restoration_authority(
        &self,
        lease: &RootLease,
        record: &GenerationRestorationRecord,
        operation: &Enablement,
    ) -> Result<Authority> {
        let mut timing = LifecycleTiming::start("Validate retained restoration workspace");
        let workspace = copies::workspace(lease, &record.id)?;
        let authority_path = workspace.join("restoration-authority.json");
        if authority_path.try_exists().map_err(storage)? {
            let authority: Authority = decode(&security::read_private(&authority_path, 65536)?)?;
            verify_authority(lease, &authority)?;
            if encode(&authority.record)? != encode(record)? {
                return Err(refused("Retained restoration acceptance differs"));
            }
            return Ok(authority);
        }
        let source = operation.source_path(lease)?;
        self.db.retain_existing_lock(&source)?;
        let history = generations::activations(lease.root())?;
        if prefix_digest(&history) != record.plan.history_sha256 {
            return Err(refused(
                "Accepted activation history changed before archival",
            ));
        }
        timing.advance("Copy restoration source preimage");
        let source_copy = copies::copy_locked(
            lease,
            &record.id,
            "rollback",
            &source,
            &CancellationToken::new(),
        )?;
        timing.advance("Copy restoration promoted archive");
        let promoted_copy = copies::copy_locked(
            lease,
            &record.id,
            "target",
            &self.generation_path()?,
            &CancellationToken::new(),
        )?;
        timing.advance("Copy restoration overlay workspace");
        copies::copy_locked(
            lease,
            &record.id,
            "restoration",
            &source,
            &CancellationToken::new(),
        )?;
        timing.advance("Flush independent restoration copies");
        flush_copy(&source_copy.path)?;
        flush_copy(&promoted_copy.path)?;
        flush_copy(&workspace.join("restoration"))?;
        timing.advance("Validate archived receipts and build exposure barrier");
        let baseline = operation_path(lease, &operation.id)?.join("rollback");
        let baseline_facts = verify_copy(&baseline)?;
        let source_db = open_closed(&source_copy.path)?;
        let promoted_db = open_closed(&promoted_copy.path)?;
        let baseline_db = open_closed(&baseline)?;
        let source_read = source_db.begin_read().map_err(storage)?;
        let promoted_read = promoted_db.begin_read().map_err(storage)?;
        let baseline_read = baseline_db.begin_read().map_err(storage)?;
        let source_table = source_read.open_table(RECORDS).map_err(storage)?;
        let promoted_table = promoted_read.open_table(RECORDS).map_err(storage)?;
        let baseline_table = baseline_read.open_table(RECORDS).map_err(storage)?;
        control::validate_prestate(
            &baseline_table,
            &source_table,
            &promoted_table,
            operation,
            Some(record),
        )?;
        let accepted = control::receipt(
            &promoted_table,
            &record.receipt_key,
            &record.request_digest,
            &record.id,
            "restoration_id",
        )?;
        let barrier = ExposureBarrier {
            schema_version: 1,
            restoration_id: record.id.clone(),
            enablement_id: operation.id.clone(),
            root_id: self.root_id().into(),
            source_object: files::source_object(&source.join("store.redb"))?,
            source_operation_id: operation.source_operation_id.clone(),
            source_activation_id: operation.source_activation_id.clone(),
            source_revision: operation.source_revision.clone(),
            source_preimage_sha256: source_copy.facts.records_sha256.clone(),
            promoted_archive_sha256: promoted_copy.facts.records_sha256.clone(),
            baseline_sha256: baseline_facts.records_sha256,
            accepted_receipt_sha256: nw_domain::projects::hash(&accepted),
            history_sha256: record.plan.history_sha256.clone(),
            prefix_sha256: record.plan.prefix_sha256.clone(),
            input_delta_sha256: control::delta_digest(
                &source_table,
                &promoted_table,
                &baseline_table,
                record,
            )?,
            predecessor_restoration: operation.predecessor_restoration.clone(),
        };
        let history_directory = security::private_directory(&workspace.join("history"))?;
        timing.advance("Archive and flush activation history");
        let held_archive = QualifiedDirectory::open(&history_directory)?;
        let mut archived = Vec::with_capacity(history.len());
        for (activation, sha256) in history {
            let name = format!("{:010}.json", activation.sequence);
            let raw =
                security::read_private(&lease.state().join("activations").join(&name), 65536)?;
            if nw_domain::projects::hash(&raw) != sha256 {
                return Err(refused(
                    "Active history changed during independent archival",
                ));
            }
            let destination = history_directory.join(&name);
            if destination.try_exists().map_err(storage)? {
                if security::read_private(&destination, 65536)? != raw {
                    return Err(refused("Existing archived history differs"));
                }
            } else {
                security::write_private(&destination, &raw, false)?;
            }
            held_archive.flush_final_file(
                &name,
                &files::source_object(&destination)?,
                &sha256,
                65536,
            )?;
            archived.push(ArchivedActivation {
                name,
                sha256,
                activation,
            });
        }
        let source_location = archived
            .get(record.plan.prefix_count.saturating_sub(1) as usize)
            .ok_or_else(|| refused("Nonempty source prefix is missing"))?
            .activation
            .location
            .clone();
        let authority = Authority {
            schema_version: 1,
            record: record.clone(),
            operation: operation.clone(),
            source_location,
            source_prestate: source_copy.facts,
            promoted_archive: promoted_copy.facts,
            barrier,
            history: archived,
        };
        timing.advance("Publish and verify restoration authority");
        write_fact(&workspace, "restoration-authority.json", &authority)?;
        verify_authority(lease, &authority)?;
        Ok(authority)
    }
    fn install_restoration_overlay(&self, lease: &RootLease, authority: &Authority) -> Result<()> {
        let mut timing =
            LifecycleTiming::start("Verify authority and open immutable restoration inputs");
        verify_authority(lease, authority)?;
        let workspace = operation_path(lease, &authority.record.id)?;
        let source = generations::location_path(lease.root(), &authority.source_location)?;
        let baseline_db =
            open_closed(&operation_path(lease, &authority.operation.id)?.join("rollback"))?;
        let promoted_db = open_closed(&workspace.join("target"))?;
        let baseline_read = baseline_db.begin_read().map_err(storage)?;
        let promoted_read = promoted_db.begin_read().map_err(storage)?;
        let baseline = baseline_read.open_table(RECORDS).map_err(storage)?;
        let promoted = promoted_read.open_table(RECORDS).map_err(storage)?;
        let preimage_db = open_closed(&workspace.join("rollback"))?;
        let preimage_read = preimage_db.begin_read().map_err(storage)?;
        let preimage = preimage_read.open_table(RECORDS).map_err(storage)?;
        // B on S is authoritative. A missing external completion or reappearing F
        // cannot authorize copying, applying the overlay again, or resetting clocks.
        timing.advance("Validate original source before writable repair");
        let repaired = self.open_restoration_control(
            lease,
            &source,
            &authority.barrier.source_object,
            authority,
        )?;
        drop(repaired);
        let live = open_closed(&source)?;
        let armed = control::has_barrier(&live, &authority.record.id)?;
        drop(live);
        if let Some(barrier) = armed {
            if encode(&barrier)? != encode(&authority.barrier)? {
                return Err(refused("Exposure barrier contradicts accepted lineage"));
            }
            return validate_continuity(lease, authority);
        }
        for path in [workspace.join("restoration"), source.clone()] {
            timing.advance("Validate restoration candidate before writable repair");
            let identity = files::source_object(&path.join("store.redb"))?;
            if path == source && identity != authority.barrier.source_object {
                return Err(refused("Original source file identity changed"));
            }
            let database = self.open_restoration_control(lease, &path, &identity, authority)?;
            let existing = {
                let read = database.begin_read().map_err(storage)?;
                let table = read.open_table(RECORDS).map_err(storage)?;
                table
                    .get(format!("restoration-control/{}", authority.record.id).as_str())
                    .map_err(storage)?
                    .map(|row| decode::<ExposureBarrier>(row.value()))
                    .transpose()?
            };
            if let Some(existing) = existing {
                if encode(&existing)? != encode(&authority.barrier)? {
                    return Err(refused("Existing overlay barrier differs"));
                }
                let read = database.begin_read().map_err(storage)?;
                control::validate_continuity(
                    &preimage,
                    &read.open_table(RECORDS).map_err(storage)?,
                    &promoted,
                    &baseline,
                    &authority.operation,
                    &authority.record,
                    &authority.barrier,
                )?;
                drop(read);
                drop(database);
                continue;
            }
            timing.advance("Commit exact restoration overlay and barrier");
            let result = control::install(
                &database,
                &baseline,
                &promoted,
                &authority.operation,
                &authority.record,
                &authority.barrier,
            );
            drop(database);
            timing.advance("Classify restoration commit from retained source facts");
            // A commit error can mean all or none. Never infer rollback from Err.
            let repaired = self.open_restoration_control(lease, &path, &identity, authority)?;
            drop(repaired);
            let observed = open_closed(&path)?;
            let barrier = control::has_barrier(&observed, &authority.record.id)?;
            if let Some(barrier) = barrier {
                if encode(&barrier)? != encode(&authority.barrier)? {
                    return Err(refused("Uncertain commit produced contradictory B"));
                }
                let read = observed.begin_read().map_err(storage)?;
                control::validate_continuity(
                    &preimage,
                    &read.open_table(RECORDS).map_err(storage)?,
                    &promoted,
                    &baseline,
                    &authority.operation,
                    &authority.record,
                    &authority.barrier,
                )?;
            } else {
                let read = observed.begin_read().map_err(storage)?;
                if raw_hash(&read.open_table(RECORDS).map_err(storage)?)?.1
                    != authority.source_prestate.records_sha256
                {
                    return Err(refused(
                        "Uncertain commit has neither complete B nor exact prestate",
                    ));
                }
                let selection = lease.selection()?;
                if selection.activation_id.as_deref()
                    != Some(authority.record.plan.fenced_activation_id.as_str())
                {
                    return Err(refused(
                        "Absent B cannot be retried after the fence is no longer selected",
                    ));
                }
                return result.and(Err(refused(
                    "B is absent; accepted recovery may retry only while F remains fenced",
                )));
            }
        }
        timing.advance("Validate final original source continuity");
        validate_continuity(lease, authority)
    }
    fn validate_migration_records_for_media(&self) -> Result<()> {
        self.visit_executions(|job| {
            if !job.terminal() {
                return Err(DomainError::new(
                    "OWNER_BUSY",
                    "Accepted work must drain before restoration",
                ));
            }
            Ok(())
        })?;
        self.visit::<serde_json::Value>("upload/", |raw| {
            if matches!(
                raw.get("phase").and_then(serde_json::Value::as_str),
                Some("receiving" | "reserved")
            ) {
                return Err(DomainError::new(
                    "OWNER_BUSY",
                    "Intake must drain before restoration",
                ));
            }
            Ok(())
        })
    }
}
fn write_fact(directory: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let raw = encode(value)?;
    if raw.len() > 65536 {
        return Err(refused("Restoration proof exceeds64KiB"));
    }
    let path = directory.join(name);
    if path.try_exists().map_err(storage)? {
        if security::read_private(&path, 65536)? != raw {
            return Err(refused("Immutable restoration proof differs"));
        }
    } else {
        security::write_private(&path, &raw, false)?;
    }
    QualifiedDirectory::open(directory)?.flush_final_file(
        name,
        &files::source_object(&path)?,
        &nw_domain::projects::hash(&raw),
        65536,
    )
}
pub(crate) fn verify_authority(lease: &RootLease, authority: &Authority) -> Result<()> {
    let _timing =
        LifecycleTiming::start("Verify genuine restoration authority and immutable archives");
    authority.record.plan.validate_hash()?;
    let record = &authority.record;
    let plan = &record.plan;
    let prefix = plan.prefix_count as usize;
    if authority.schema_version != 1
        || record.schema_version != 1
        || record.id != plan.id
        || authority.operation.id != plan.enablement_id
        || authority.operation.root_id != plan.root_id
        || authority.history.len() != plan.history_count as usize
        || prefix == 0
        || prefix >= authority.history.len()
        || authority.history[prefix].activation.id != plan.fenced_activation_id
        || authority.history[prefix - 1].activation.id != plan.source_activation_id
        || authority.history[prefix - 1].activation.location != authority.source_location
    {
        return Err(refused("Accepted restoration lineage is inconsistent"));
    }
    let workspace = operation_path(lease, &record.id)?;
    if verify_copy(&workspace.join("rollback"))? != authority.source_prestate
        || verify_copy(&workspace.join("target"))? != authority.promoted_archive
    {
        return Err(refused("Restoration retained input facts differ"));
    }
    let promoted_db = open_closed(&workspace.join("target"))?;
    let read = promoted_db.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    let row = table
        .get(format!("generation-restoration/{}", record.id).as_str())
        .map_err(storage)?
        .ok_or_else(|| refused("Archived accepted restoration journal is missing"))?;
    if row.value() != encode(record)? {
        return Err(refused("Archived acceptance journal differs"));
    }
    let receipt = control::receipt(
        &table,
        &record.receipt_key,
        &record.request_digest,
        &record.id,
        "restoration_id",
    )?;
    if nw_domain::projects::hash(&receipt) != authority.barrier.accepted_receipt_sha256 {
        return Err(refused("Archived genuine receipt differs"));
    }
    let source_db = open_closed(&workspace.join("rollback"))?;
    let source_read = source_db.begin_read().map_err(storage)?;
    let source = source_read.open_table(RECORDS).map_err(storage)?;
    let baseline_path = operation_path(lease, &authority.operation.id)?.join("rollback");
    let baseline_facts = verify_copy(&baseline_path)?;
    let baseline_db = open_closed(&baseline_path)?;
    let baseline_read = baseline_db.begin_read().map_err(storage)?;
    let baseline = baseline_read.open_table(RECORDS).map_err(storage)?;
    let operation = &authority.operation;
    verify_enablement_journal(&source, operation)?;
    control::validate_prestate(&baseline, &source, &table, operation, Some(record))?;
    let barrier = &authority.barrier;
    let source_path = generations::location_path(lease.root(), &authority.source_location)?;
    if barrier.schema_version != 1
        || barrier.restoration_id != record.id
        || barrier.enablement_id != operation.id
        || barrier.root_id != plan.root_id
        || barrier.source_operation_id != operation.source_operation_id
        || barrier.source_activation_id != operation.source_activation_id
        || barrier.source_revision != operation.source_revision
        || barrier.source_preimage_sha256 != authority.source_prestate.records_sha256
        || authority.source_prestate.records_sha256 != plan.source_records_sha256
        || barrier.promoted_archive_sha256 != authority.promoted_archive.records_sha256
        || barrier.baseline_sha256 != baseline_facts.records_sha256
        || baseline_facts.records_sha256 != operation.source_records_sha256
        || baseline_facts.record_count != operation.source_records
        || barrier.history_sha256 != plan.history_sha256
        || barrier.prefix_sha256 != plan.prefix_sha256
        || barrier.predecessor_restoration != operation.predecessor_restoration
        || barrier.source_object != files::source_object(&source_path.join("store.redb"))?
        || barrier.input_delta_sha256 != control::delta_digest(&source, &table, &baseline, record)?
    {
        return Err(refused(
            "Exposure barrier does not match independently accepted archive facts",
        ));
    }
    let mut history = Vec::with_capacity(authority.history.len());
    for (index, entry) in authority.history.iter().enumerate() {
        let raw = security::read_private(&workspace.join("history").join(&entry.name), 65536)?;
        if entry.name != format!("{:010}.json", index + 1)
            || entry.activation.sequence as usize != index + 1
            || nw_domain::projects::hash(&raw) != entry.sha256
            || serde_json::to_value(decode::<Activation>(&raw)?).map_err(storage)?
                != serde_json::to_value(&entry.activation).map_err(storage)?
            || entry.activation.root_id != plan.root_id
        {
            return Err(refused("Independent archived activation bytes differ"));
        }
        history.push((entry.activation.clone(), entry.sha256.clone()));
    }
    if prefix_digest(&history) != plan.history_sha256
        || prefix_digest(&history[..prefix]) != plan.prefix_sha256
    {
        return Err(refused("Archived history/prefix commitment differs"));
    }
    Ok(())
}
pub(crate) fn validate_source_snapshot(
    lease: &RootLease,
    authority: &Authority,
    snapshot: &Path,
) -> Result<()> {
    verify_authority(lease, authority)?;
    let current_db = open_closed(snapshot)?;
    let current_read = current_db.begin_read().map_err(storage)?;
    let current = current_read.open_table(RECORDS).map_err(storage)?;
    let workspace = operation_path(lease, &authority.record.id)?;
    let preimage_db = open_closed(&workspace.join("rollback"))?;
    let promoted_db = open_closed(&workspace.join("target"))?;
    let baseline_db =
        open_closed(&operation_path(lease, &authority.operation.id)?.join("rollback"))?;
    let preimage_read = preimage_db.begin_read().map_err(storage)?;
    let promoted_read = promoted_db.begin_read().map_err(storage)?;
    let baseline_read = baseline_db.begin_read().map_err(storage)?;
    let promoted = promoted_read.open_table(RECORDS).map_err(storage)?;
    let baseline = baseline_read.open_table(RECORDS).map_err(storage)?;
    if let Some(barrier) = control::has_barrier(&current_db, &authority.record.id)? {
        if encode(&barrier)? != encode(&authority.barrier)? {
            return Err(refused(
                "Source exposure barrier contradicts accepted lineage",
            ));
        }
        control::validate_continuity(
            &preimage_read.open_table(RECORDS).map_err(storage)?,
            &current,
            &promoted,
            &baseline,
            &authority.operation,
            &authority.record,
            &authority.barrier,
        )
    } else {
        if raw_hash(&current)?
            != (
                authority.source_prestate.record_count,
                authority.source_prestate.records_sha256.clone(),
            )
        {
            return Err(refused(
                "Unarmed source differs from the exact accepted prestate",
            ));
        }
        let history = generations::activations(lease.root())?;
        if history
            .last()
            .is_none_or(|(entry, _)| entry.id != authority.record.plan.fenced_activation_id)
            || prefix_digest(&history) != authority.record.plan.history_sha256
        {
            return Err(refused(
                "Absent B requires the complete original history with F still selected",
            ));
        }
        control::validate_prestate(
            &baseline,
            &current,
            &promoted,
            &authority.operation,
            Some(&authority.record),
        )
    }
}
pub(crate) fn validate_continuity(lease: &RootLease, authority: &Authority) -> Result<()> {
    let _timing =
        LifecycleTiming::start("Validate current source barrier and old business continuity");
    let source = generations::location_path(lease.root(), &authority.source_location)?;
    if files::source_object(&source.join("store.redb"))? != authority.barrier.source_object {
        return Err(refused(
            "Armed source native identity changed; physical rewind is forbidden",
        ));
    }
    let database = open_closed(&source)?;
    let installed = control::has_barrier(&database, &authority.record.id)?
        .ok_or_else(|| refused("Complete source exposure barrier is absent"))?;
    if encode(&installed)? != encode(&authority.barrier)? {
        return Err(refused("Source B contradicts accepted restoration"));
    }
    let current_read = database.begin_read().map_err(storage)?;
    let current = current_read.open_table(RECORDS).map_err(storage)?;
    let workspace = operation_path(lease, &authority.record.id)?;
    let preimage_db = open_closed(&workspace.join("rollback"))?;
    let promoted_db = open_closed(&workspace.join("target"))?;
    let baseline_db =
        open_closed(&operation_path(lease, &authority.operation.id)?.join("rollback"))?;
    let preimage_read = preimage_db.begin_read().map_err(storage)?;
    let promoted_read = promoted_db.begin_read().map_err(storage)?;
    let baseline_read = baseline_db.begin_read().map_err(storage)?;
    control::validate_continuity(
        &preimage_read.open_table(RECORDS).map_err(storage)?,
        &current,
        &promoted_read.open_table(RECORDS).map_err(storage)?,
        &baseline_read.open_table(RECORDS).map_err(storage)?,
        &authority.operation,
        &authority.record,
        &authority.barrier,
    )
}
pub(crate) fn expose_prefix(lease: &RootLease, authority: &Authority) -> Result<()> {
    let _timing = LifecycleTiming::start("Verify and remove owned suffix with fence last");
    verify_authority(lease, authority)?;
    validate_continuity(lease, authority)?;
    observe_installed_source(lease, authority)?;
    let directory = lease.state().join("activations");
    let held = QualifiedDirectory::open(&directory)?;
    let mut seen = std::collections::BTreeSet::new();
    for (count, entry) in std::fs::read_dir(&directory).map_err(storage)?.enumerate() {
        let entry = entry.map_err(storage)?;
        if count >= 256 {
            return Err(refused("Activation directory exceeds256 entries"));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| refused("Unknown activation name encoding"))?;
        if name.starts_with(".credential-") {
            continue;
        }
        let archived = authority
            .history
            .iter()
            .find(|item| item.name == name)
            .ok_or_else(|| refused("Unowned activation suffix member was preserved"))?;
        let raw = security::read_private(&entry.path(), 65536)?;
        if nw_domain::projects::hash(&raw) != archived.sha256 {
            return Err(refused("Active suffix differs from independent archive"));
        }
        seen.insert(name);
    }
    let prefix = authority.record.plan.prefix_count as usize;
    if authority.history[..prefix]
        .iter()
        .any(|entry| !seen.contains(&entry.name))
    {
        return Err(refused(
            "Original nonempty compatible prefix is missing; no fallback is allowed",
        ));
    }
    for entry in authority.history[prefix..].iter().rev() {
        if !seen.contains(&entry.name) {
            continue;
        }
        if entry.activation.sequence == authority.record.plan.prefix_count + 1 {
            validate_continuity(lease, authority)?;
        }
        let identity = files::source_object(&directory.join(&entry.name))?;
        held.remove_activation(&entry.name, &identity, &entry.sha256)?;
    }
    let selected = lease.selection()?;
    if selected.activation_id.as_deref()
        != Some(authority.record.plan.source_activation_id.as_str())
    {
        return Err(refused(
            "Restoration did not expose the exact original compatible selection",
        ));
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledObservation {
    schema_version: u32,
    restoration_id: String,
    root_id: String,
    authority_sha256: String,
    barrier_sha256: String,
    source_object: nw_domain::records::NativeObjectIdentity,
    records_sha256: String,
    record_count: u32,
    file_sha256: String,
    file_bytes: String,
    observation: String,
}
fn observe_installed_source(lease: &RootLease, authority: &Authority) -> Result<()> {
    let directory = operation_path(lease, &authority.record.id)?;
    let path = directory.join("restoration-installed.json");
    let authority_sha256 = nw_domain::projects::hash(&encode(authority)?);
    let barrier_sha256 = nw_domain::projects::hash(&encode(&authority.barrier)?);
    if path.try_exists().map_err(storage)? {
        let observed: InstalledObservation = decode(&security::read_private(&path, 65536)?)?;
        if observed.schema_version != 1
            || observed.restoration_id != authority.record.id
            || observed.root_id != authority.record.plan.root_id
            || observed.authority_sha256 != authority_sha256
            || observed.barrier_sha256 != barrier_sha256
            || observed.source_object != authority.barrier.source_object
            || observed.record_count > versions::MAX_GENERATION_RECORDS
            || observed.observation != "closed_source_with_verified_B_point_in_time_only"
            || !(1..=versions::MAX_GENERATION_BYTES)
                .contains(&nw_domain::media_execution::exact(&observed.file_bytes)?)
        {
            return Err(refused("Retained installed observation binding differs"));
        }
        nw_domain::media_execution::hash(&observed.records_sha256)?;
        nw_domain::media_execution::hash(&observed.file_sha256)?;
        // Old work may legitimately change all physical/raw facts after exposure.
        // Never compare this immutable observation with today's file or replace it.
        return Ok(());
    }
    validate_continuity(lease, authority)?;
    let source = generations::location_path(lease.root(), &authority.source_location)?;
    let file = source.join("store.redb");
    let stamp = files::SourceStamp::capture_resolved(&file)?;
    let facts = versions::inspect_closed(&source)?;
    let physical = files::identity(&file, &CancellationToken::new())?;
    let source_object = files::source_object(&file)?;
    stamp.revalidate()?;
    if source_object != authority.barrier.source_object || physical.bytes != facts.store_bytes {
        return Err(refused(
            "Installed observation source changed during capture",
        ));
    }
    write_fact(
        &directory,
        "restoration-installed.json",
        &InstalledObservation {
            schema_version: 1,
            restoration_id: authority.record.id.clone(),
            root_id: facts.root_id,
            authority_sha256,
            barrier_sha256,
            source_object,
            records_sha256: facts.records_sha256,
            record_count: facts.record_count,
            file_sha256: physical.sha256,
            file_bytes: physical.bytes,
            observation: "closed_source_with_verified_B_point_in_time_only".into(),
        },
    )
}
pub(crate) fn complete_observation(lease: &RootLease, authority: &Authority) -> Result<()> {
    observe_installed_source(lease, authority)?;
    write_fact(
        &operation_path(lease, &authority.record.id)?,
        "restoration-complete.json",
        &serde_json::json!({
            "schema_version":1,"restoration_id":authority.record.id,
            "barrier_sha256":nw_domain::projects::hash(&encode(&authority.barrier)?),
            "source_activation_id":authority.record.plan.source_activation_id,
            "completion":"compatible_prefix_observed_after_owned_removal",
            "durability":"documented_local_file_flush; process_crash_recovery; no_universal_power_loss_claim"
        }),
    )
}
/// Runs under the stable root lock before normal activation parsing. Only an
/// independently archived genuine acceptance plus B can authorize suffix repair.
pub(crate) fn repair_owned_suffix(lease: &RootLease) -> Result<()> {
    let parent = lease.state().join("generation-workspaces");
    if !parent.try_exists().map_err(storage)? {
        return Ok(());
    }
    let mut nodes = Vec::new();
    for (count, member) in std::fs::read_dir(&parent).map_err(storage)?.enumerate() {
        let member = member.map_err(storage)?;
        if count >= 16 {
            return Err(refused("Accepted lineage exceeds16 retained workspaces"));
        }
        let path = member.path().join("restoration-authority.json");
        if !path.try_exists().map_err(storage)? {
            continue;
        }
        let authority: Authority = decode(&security::read_private(&path, 65536)?)?;
        if member.file_name().to_str() != Some(authority.record.id.as_str()) {
            return Err(refused("Accepted restoration workspace identity differs"));
        }
        verify_authority(lease, &authority)?;
        nodes.push(authority);
    }
    if nodes.is_empty() {
        return Ok(());
    }
    let mut predecessors = std::collections::BTreeSet::new();
    for node in &nodes {
        if let Some(previous) = &node.operation.predecessor_restoration
            && (!nodes
                .iter()
                .any(|candidate| candidate.record.id == *previous)
                || !predecessors.insert(previous.clone()))
        {
            return Err(refused(
                "Accepted restoration successor lineage is missing or branches",
            ));
        }
    }
    let leaves: Vec<_> = nodes
        .iter()
        .filter(|node| !predecessors.contains(&node.record.id))
        .collect();
    if leaves.len() != 1 {
        return Err(refused(
            "Accepted restoration lineage has no unique successor",
        ));
    }
    let authority = leaves[0];
    let mut visited = std::collections::BTreeSet::new();
    let mut next = Some(authority.record.id.as_str());
    while let Some(id) = next {
        if !visited.insert(id) {
            return Err(refused("Accepted restoration lineage is cyclic"));
        }
        let node = nodes
            .iter()
            .find(|node| node.record.id == id)
            .ok_or_else(|| refused("Accepted restoration predecessor is missing"))?;
        next = node.operation.predecessor_restoration.as_deref();
    }
    if visited.len() != nodes.len() {
        return Err(refused("Accepted restoration lineage is disconnected"));
    }
    // A later accepted enablement explicitly binds the terminal restoration. Old
    // nodes must not capture its newly fenced source or newly published activation.
    if accepted_successor(lease, authority)? {
        return Ok(());
    }
    let source = generations::location_path(lease.root(), &authority.source_location)?;
    crate::control_backend::repair_accepted_source(lease, &source, authority)?;
    let database = open_closed(&source)?;
    let barrier = control::has_barrier(&database, &authority.record.id)?;
    drop(database);
    if let Some(barrier) = barrier {
        if encode(&barrier)? != encode(&authority.barrier)? {
            return Err(refused("Preselection B contradicts accepted lineage"));
        }
        validate_continuity(lease, authority)?;
        expose_prefix(lease, authority)?;
        complete_observation(lease, authority)?;
    } else {
        // Without B no deletion is authorized. Strict history must still contain F.
        let history = generations::activations(lease.root())?;
        if history
            .get(authority.record.plan.prefix_count as usize)
            .is_none_or(|(entry, _)| entry.id != authority.record.plan.fenced_activation_id)
        {
            return Err(refused(
                "Absent B cannot authorize a missing or partial incompatible fence",
            ));
        }
    }
    Ok(())
}
fn accepted_successor(lease: &RootLease, authority: &Authority) -> Result<bool> {
    let history = match generations::activations(lease.root()) {
        Ok(history) => history,
        Err(_) => return Ok(false),
    };
    let Some((selected, _)) = history.last() else {
        return Ok(false);
    };
    let path = generations::location_path(lease.root(), &selected.location)?;
    // A dirty successor is inspected through an independently repaired probe;
    // no writable successor access precedes proving its accepted predecessor.
    let (facts, snapshot) = versions::existing_snapshot(&path)?;
    if facts.root_id != authority.record.plan.root_id {
        return Err(refused("Successor root identity differs"));
    }
    let database = open_closed(&snapshot)?;
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    for (prefix, pending, field) in [
        (
            "generation-promotion",
            "meta/pending-promotion",
            "promotion_id",
        ),
        (
            "generation-initialization",
            "meta/pending-initialization",
            "initialization_id",
        ),
    ] {
        let pending_id = table
            .get(pending)
            .map_err(storage)?
            .map(|row| String::from_utf8(row.value().to_vec()).map_err(storage))
            .transpose()?;
        let candidate = pending_id.as_deref().unwrap_or(&selected.operation_id);
        if candidate == authority.operation.id {
            continue;
        }
        let Some(row) = table
            .get(format!("{prefix}/{candidate}").as_str())
            .map_err(storage)?
        else {
            continue;
        };
        let (id, predecessor, root, receipt_key, request_digest) =
            if prefix == "generation-promotion" {
                let record: GenerationPromotionRecord = decode(row.value())?;
                record.plan.validate_hash()?;
                if record.schema_version != 1 || record.id != record.plan.id {
                    return Err(refused("Successor promotion identity differs"));
                }
                (
                    record.id,
                    record.predecessor_restoration,
                    record.plan.root_id,
                    record.receipt_key,
                    record.request_digest,
                )
            } else {
                let record: MediaInitializationRecord = decode(row.value())?;
                record.plan.validate_hash()?;
                if record.schema_version != 1 || record.id != record.plan.id {
                    return Err(refused("Successor initialization identity differs"));
                }
                (
                    record.id,
                    record.predecessor_restoration,
                    record.plan.root_id,
                    record.receipt_key,
                    record.request_digest,
                )
            };
        if id != candidate
            || predecessor.as_deref() != Some(authority.record.id.as_str())
            || root != authority.record.plan.root_id
        {
            return Err(refused(
                "A later generation lacks the required accepted predecessor binding",
            ));
        }
        control::receipt(&table, &receipt_key, &request_digest, candidate, field)?;
        return Ok(true);
    }
    Ok(false)
}
impl Store {
    pub(crate) fn terminal_restoration_predecessor(&self) -> Result<Option<String>> {
        let mut nodes = Vec::new();
        self.visit::<ExposureBarrier>("restoration-control/", |barrier| {
            if barrier.schema_version != 1 || barrier.root_id != self.root_id() {
                return Err(refused("Retained exposure lineage identity differs"));
            }
            nw_domain::projects::validate_id(&barrier.restoration_id)?;
            nodes.push(barrier);
            if nodes.len() > 16 {
                return Err(refused("Retained exposure lineage exceeds16 operations"));
            }
            Ok(())
        })?;
        let predecessors: std::collections::BTreeSet<_> = nodes
            .iter()
            .filter_map(|node| node.predecessor_restoration.as_ref())
            .collect();
        let leaves: Vec<_> = nodes
            .iter()
            .filter(|node| !predecessors.contains(&node.restoration_id))
            .collect();
        match leaves.as_slice() {
            [] if nodes.is_empty() => Ok(None),
            [leaf] => {
                let mut visited = std::collections::BTreeSet::new();
                let mut next = Some(leaf.restoration_id.as_str());
                while let Some(id) = next {
                    if !visited.insert(id) {
                        return Err(refused("Retained restoration lineage is cyclic"));
                    }
                    let node = nodes
                        .iter()
                        .find(|node| node.restoration_id == id)
                        .ok_or_else(|| refused("Retained restoration predecessor is missing"))?;
                    next = node.predecessor_restoration.as_deref();
                }
                if visited.len() != nodes.len() {
                    return Err(refused(
                        "Retained restoration lineage contains a disconnected branch",
                    ));
                }
                Ok(Some(leaf.restoration_id.clone()))
            }
            _ => Err(refused(
                "Retained source lineage does not have one terminal predecessor",
            )),
        }
    }
}

fn verify_enablement_journal(
    source: &impl ReadableTable<&'static str, &'static [u8]>,
    expected: &Enablement,
) -> Result<()> {
    let key = expected.record_key();
    let raw = source
        .get(key.as_str())
        .map_err(storage)?
        .ok_or_else(|| refused("Accepted enablement journal missing"))?;
    let actual = if expected.initialization {
        let record: MediaInitializationRecord = decode(raw.value())?;
        record.plan.validate_hash()?;
        let plan = record.plan;
        Enablement {
            id: record.id,
            initialization: true,
            root_id: plan.root_id,
            receipt_key: record.receipt_key,
            request_digest: record.request_digest,
            source_generation: plan.source_generation,
            source_activation_id: plan.source_activation_id,
            source_operation_id: plan.source_operation_id,
            source_revision: plan.source_guard_revision,
            source_sequence: plan.source_activation_sequence,
            source_prefix_sha256: plan.source_prefix_sha256,
            source_records_sha256: plan.source_records_sha256,
            source_records: plan.source_records,
            source_bytes: plan.source_bytes,
            source_capabilities: plan.source_capabilities,
            target_generation: plan.target_generation,
            activation_id: record.activation_id,
            phase: record.phase,
            predecessor_restoration: record.predecessor_restoration,
        }
    } else {
        let record: GenerationPromotionRecord = decode(raw.value())?;
        record.plan.validate_hash()?;
        let plan = record.plan;
        Enablement {
            id: record.id,
            initialization: false,
            root_id: plan.root_id,
            receipt_key: record.receipt_key,
            request_digest: record.request_digest,
            source_generation: plan.source_generation,
            source_activation_id: plan.source_activation_id,
            source_operation_id: plan.source_guard.migration_id,
            source_revision: plan.source_guard.revision,
            source_sequence: plan.source_activation_sequence,
            source_prefix_sha256: plan.source_prefix_sha256,
            source_records_sha256: plan.source_records_sha256,
            source_records: plan.source_records,
            source_bytes: plan.source_bytes,
            source_capabilities: plan.source_capabilities,
            target_generation: plan.target_generation,
            activation_id: record.activation_id,
            phase: record.phase,
            predecessor_restoration: record.predecessor_restoration,
        }
    };
    let mut expected = expected.clone();
    expected.phase = EnablementPhase::Accepted;
    if encode(&actual)? != encode(&expected)? {
        return Err(refused(
            "External authority differs from the original accepted enablement journal",
        ));
    }
    control::receipt(
        source,
        &actual.receipt_key,
        &actual.request_digest,
        &actual.id,
        if actual.initialization {
            "initialization_id"
        } else {
            "promotion_id"
        },
    )?;
    Ok(())
}
