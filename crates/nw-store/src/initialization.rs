//! Explicit r0 initialization and genuinely absent-root genesis; legacy factories stay unchanged.
use crate::{
    RECORDS, Store,
    adoption::Guard,
    copies, decode, encode,
    generations::{Activation, CURRENT_CAPABILITIES, GenerationLocation, RootLease},
    media_generations::{flush_copy, guard, raw_hash},
    records::record_quota,
    storage,
};
use nw_domain::{
    DomainError, Result,
    access::AccessProfile,
    commands::{Request, Response},
    maintenance::{MigrationPhase, media_lifecycle::*, promotion::MEDIA_V1_CAPABILITIES},
};
use nw_platform::{files, generation_restoration::QualifiedDirectory, security};
use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenesisOrigin {
    schema_version: u32,
    operation_id: String,
    activation_id: String,
    root_id: String,
    profiles_sha256: String,
}
fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::new("INITIALIZATION_UNQUALIFIED", message)
}
pub(crate) fn source_ancestry(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    root_id: &str,
    selection: &crate::generations::Selection,
    prefix: &crate::generations::CompatiblePrefix,
) -> Result<(Guard, InitializationOrigin)> {
    let refused = || DomainError::new("GENERATION_ANCESTRY_INVALID", "Current source lacks the selected genuine migration or genesis ancestry");
    let original = guard(table)?;
    let root = table.get("meta/root").map_err(storage)?.ok_or_else(refused)?;
    if root.value() != root_id.as_bytes() || prefix.root_id != root_id
        || selection.root_id.as_deref() != Some(root_id)
        || selection.activation_id.as_deref() != Some(prefix.activation_id.as_str())
        || original.activation_id != prefix.activation_id
        || original.migration_id != prefix.operation_id
    {
        return Err(refused());
    }
    let genesis = table.get(format!("generation-genesis/{}", original.migration_id).as_str()).map_err(storage)?;
    if let Some(row) = genesis {
        let origin: GenesisOrigin = decode(row.value())?;
        let marker = table.get("meta/genesis-origin").map_err(storage)?.ok_or_else(refused)?;
        if selection.generation != 1 || !matches!(&selection.location, GenerationLocation::LegacyOne {})
            || origin.schema_version != 1 || origin.operation_id != original.migration_id
            || origin.activation_id != original.activation_id || origin.root_id != root_id
            || marker.value() != original.migration_id.as_bytes()
        {
            return Err(refused());
        }
        nw_domain::media_execution::hash(&origin.profiles_sha256)?;
        return Ok((original, InitializationOrigin::CompatibleGenesisV1));
    }
    let row = table.get(format!("generation-migration/{}", original.migration_id).as_str())
        .map_err(storage)?.ok_or_else(refused)?;
    let migration: nw_domain::maintenance::MigrationRecord = decode(row.value())?;
    migration.plan.validate_hash()?;
    if migration.schema_version != 1 || migration.id != original.migration_id
        || migration.plan.id != original.migration_id || migration.phase != MigrationPhase::Succeeded
        || migration.plan.root_id != root_id || migration.plan.target_generation != selection.generation
        || migration.activation_id.as_deref() != Some(original.activation_id.as_str())
    {
        return Err(refused());
    }
    Ok((original, InitializationOrigin::MigratedZeroRevision))
}

impl Store {
    pub fn prepare_media_initialization(
        &self,
        lease: &RootLease,
        build: &str,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<MediaInitializationPlan> {
        self.prepare_initialization_with_reservation(lease, build, clock, None)
    }
    fn prepare_initialization_with_reservation(
        &self,
        lease: &RootLease,
        build: &str,
        clock: impl Fn() -> Result<u64>,
        reserved: Option<&str>,
    ) -> Result<MediaInitializationPlan> {
        if self.pending_migration()?.is_some() || self.pending_media_maintenance()? {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "Accepted maintenance must finish before initialization review",
            ));
        }
        if self.store_version()? != 3 {
            return Err(invalid(
                "Existing schema2 roots require the unchanged public legacy migration first",
            ));
        }
        let prefix = lease.compatible_prefix()?;
        let selection = lease.selection()?;
        let (original, origin) = {
            let read = self.db.begin_read().map_err(storage)?;
            source_ancestry(&read.open_table(RECORDS).map_err(storage)?, self.root_id(), &selection, &prefix)?
        };
        if original.validate()? != 0
            || original.activation_id != prefix.activation_id
            || original.migration_id != prefix.operation_id
        {
            return Err(invalid(
                "Initialization requires a trusted original zero guard and nonempty prefix",
            ));
        }
        for name in ["agent", "browser"] {
            let profile: AccessProfile = self
                .get(&format!("profile/{name}"))?
                .ok_or_else(|| invalid("Default profiles must exist before r0 review"))?;
            if profile.schema_version != 2 || profile.revision == 0 {
                return Err(invalid(
                    "Existing profile baseline is not ready for initialization",
                ));
            }
        }
        let existing_reservation = reserved.map(|id| {
            lease.state().join("generation-workspaces").join(id).join("restoration-slot.json")
                .try_exists().map_err(storage)
        }).transpose()?.unwrap_or(false);
        if existing_reservation {
            copies::restoration_slot(lease, reserved.expect("existing reservation has an identity"))?;
        } else {
            copies::workspace_headroom(lease, 2)?;
        }
        self.validate_migration_records()?;
        self.close_generation(lease)?;
        let result = (|| {
            let facts = lease.inspect_selected()?;
            if facts.required_capabilities.len() != CURRENT_CAPABILITIES.len()
                || facts
                    .required_capabilities
                    .iter()
                    .any(|cap| !CURRENT_CAPABILITIES.contains(&cap.as_str()))
                || facts.record_count > 49000
            {
                return Err(invalid(
                    "Initialization requires the exact legacy-compatible floor and row headroom",
                ));
            }
            let rollback_generation = selection
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("Generation overflow"))?;
            let target_generation = rollback_generation
                .checked_add(1)
                .ok_or_else(|| invalid("Generation overflow"))?;
            if target_generation > 16 {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Initialization exceeds16 generations",
                ));
            }
            let bytes = facts.store_bytes.parse::<u64>().map_err(storage)?;
            copies::require_copy_space(lease.state(), bytes, 3)?;
            let now = clock()?;
            let mut plan = MediaInitializationPlan {
                schema_version: 1,
                kind: InitializationKind::MediaInitializationV1,
                id: uuid::Uuid::new_v4().to_string(),
                hash: String::new(),
                root_id: self.root_id().into(),
                source_generation: selection.generation,
                source_activation_id: prefix.activation_id,
                source_activation_sequence: prefix.sequence,
                source_prefix_sha256: prefix.sha256,
                source_records_sha256: facts.records_sha256,
                source_records: facts.record_count,
                source_bytes: facts.store_bytes,
                source_capabilities: facts.required_capabilities,
                source_operation_id: original.migration_id,
                source_guard_revision: original.revision,
                origin,
                rollback_generation,
                target_generation,
                required_capabilities: MEDIA_V1_CAPABILITIES
                    .iter()
                    .map(|cap| (*cap).into())
                    .collect(),
                estimated_additional_bytes: bytes
                    .checked_mul(3)
                    .and_then(|n| n.checked_add(64 * 1024 * 1024))
                    .ok_or_else(|| invalid("Initialization space estimate overflow"))?
                    .to_string(),
                application_build: build.into(),
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
    pub fn accept_media_initialization(
        &self,
        lease: &RootLease,
        principal: &str,
        request: &Request,
        build: &str,
        clock: impl Fn() -> Result<u64>,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if request.command.id != "maintenance.initialization.accept" {
            return Err(invalid("Initialization acceptance command differs"));
        }
        if let Some(replay) = self.replay_with_clock(principal, request, &clock)? {
            return Ok(replay);
        }
        let input: InitializationSubmitInput = nw_domain::commands::parse(request.params.clone())?;
        let plan = input.plan;
        plan.validate_hash()?;
        if plan.root_id != self.root_id()
            || plan.application_build != build
            || !nw_domain::media_execution::review_current(&plan.created_at, &plan.expires_at, clock()?)?
        {
            return Err(invalid("Initialization review root/build/lifetime differs"));
        }
        let mut current = self.prepare_initialization_with_reservation(lease, build, &clock, Some(&plan.id))?;
        current.source_bytes = plan.source_bytes.clone();
        current.estimated_additional_bytes = plan.estimated_additional_bytes.clone();
        if current.digest()? != plan.hash {
            return Err(invalid("Initialization baseline changed after review"));
        }
        copies::reserve_restoration_slot(lease, &plan.id)?;
        self.close_generation(lease)?;
        let prepared = (|| {
            QualifiedDirectory::open(lease.state())?;
            let copy = copies::copy_locked(
                lease,
                &plan.id,
                "rollback",
                &self.generation_path()?,
                cancel,
            )?;
            flush_copy(&copy.path)
        })();
        let reopened = self.reopen_current_generation(lease);
        prepared?;
        reopened?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Initialization cancelled before acceptance",
            ));
        }
        let activation_id = uuid::Uuid::new_v4().to_string();
        let (receipt_key, _) = self.replay_key(principal, request, clock()?)?;
        let predecessor = self.terminal_restoration_predecessor()?;
        let response = self.commit_composed_with_clock(principal, request, &clock, |table, now| {
            copies::restoration_slot(lease, &plan.id)?;
            if raw_hash(table)? != (plan.source_records, plan.source_records_sha256.clone())
                || !nw_domain::media_execution::review_current(&plan.created_at, &plan.expires_at, now)?
            { return Err(invalid("Initialization baseline changed or expired before atomic acceptance")); }
            let selection = lease.selection()?;
            let prefix = lease.compatible_prefix()?;
            let (original, origin) = source_ancestry(table, self.root_id(), &selection, &prefix)?;
            if origin != plan.origin || selection.generation != plan.source_generation
                || prefix.sha256 != plan.source_prefix_sha256 || prefix.sequence != plan.source_activation_sequence
                || original.migration_id != plan.source_operation_id || original.activation_id != plan.source_activation_id
                || original.validate()? != 0
            { return Err(invalid("Original zero guard provenance changed")); }
            let record = MediaInitializationRecord {
                schema_version: 1, id: plan.id.clone(), creator: principal.into(),
                receipt_key: receipt_key.clone(), request_digest: request.structural_digest(), plan: plan.clone(),
                activation_id: activation_id.clone(), predecessor_restoration: predecessor.clone(),
                phase: EnablementPhase::Accepted, accepted_at: now.to_string(), completed_at: None,
            };
            let key = format!("generation-initialization/{}", plan.id);
            record_quota(table, &key)?;
            if table.get(key.as_str()).map_err(storage)?.is_some() { return Err(invalid("Initialization identity already exists")); }
            table.insert("meta/required-capabilities", encode(&MEDIA_V1_CAPABILITIES)?.as_slice()).map_err(storage)?;
            table.insert("meta/pending-initialization", plan.id.as_bytes()).map_err(storage)?;
            table.insert(key.as_str(), encode(&record)?.as_slice()).map_err(storage)?;
            Ok(serde_json::json!({"initialization_id":plan.id,"target_generation":plan.target_generation,"state":"accepted"}))
        })?;
        if let Err(error) = self.recover_media_enablement(lease, &clock) {
            tracing::warn!(code=%error.code, "Accepted initialization remains in maintenance mode");
        }
        Ok(response)
    }
    /// Application-startup-only constructor. It does not change Store::open defaults.
    pub fn initialize_fresh_legacy_prefix(
        lease: &RootLease,
        profiles: &[AccessProfile],
    ) -> Result<Self> {
        if lease.selection()?.activation_id.is_some()
            || profiles.len() != 2
            || profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .ne(["agent", "browser"])
            || profiles.iter().any(|profile| {
                profile.schema_version != 2 || profile.revision != 1 || profile.principal.is_empty()
            })
        {
            return Err(invalid(
                "Genesis requires an absent compatible prefix and exact default profiles",
            ));
        }
        for parent in [
            lease.state().join("generation-workspaces"),
            lease.state().join("activations"),
        ] {
            if parent.try_exists().map_err(storage)?
                && std::fs::read_dir(parent).map_err(storage)?.next().is_some()
            {
                return Err(invalid(
                    "Existing activation/workspace history is not a fresh root",
                ));
            }
        }
        let generations = security::private_directory(&lease.state().join("generations"))?;
        let parent_stamp = files::DirectoryStamp::capture_resolved(&generations)?;
        for (count, entry) in std::fs::read_dir(&generations).map_err(storage)?.enumerate() {
            let entry = entry.map_err(storage)?;
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(storage)?;
            if count >= 256 || entry.file_name() != "1" || !metadata.is_dir()
                || metadata.file_type().is_symlink()
            {
                return Err(invalid("Retained generation siblings are not empty bootstrap scaffolding"));
            }
        }
        parent_stamp.revalidate()?;
        let path = security::private_directory(&lease.state().join("generations/1"))?;
        let lock = files::RootLock::acquire(&path)?;
        let file = path.join("store.redb");
        if file.try_exists().map_err(storage)? {
            return Err(invalid(
                "An existing or partial database is not an absent fresh root",
            ));
        }
        for entry in std::fs::read_dir(&path).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if entry.file_name() != "owner.lock" {
                return Err(invalid("Unknown genesis scaffolding was preserved"));
            }
        }
        let operation_id = uuid::Uuid::new_v4().to_string();
        let activation_id = uuid::Uuid::new_v4().to_string();
        let root_id = uuid::Uuid::new_v4().to_string();
        let mut signing_key = uuid::Uuid::new_v4().as_bytes().to_vec();
        signing_key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        let origin = GenesisOrigin {
            schema_version: 1,
            operation_id: operation_id.clone(),
            activation_id: activation_id.clone(),
            root_id: root_id.clone(),
            profiles_sha256: nw_domain::projects::hash(&encode(&profiles)?),
        };
        QualifiedDirectory::open(lease.state())?;
        let fresh_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&file)
            .map_err(storage)?;
        let database = redb::Database::builder()
            .set_cache_size(8 * 1024 * 1024)
            .create_file(fresh_file)
            .map_err(crate::versions::database_error)?;
        let mut write = database.begin_write().map_err(storage)?;
        write
            .set_durability(redb::Durability::Immediate)
            .map_err(storage)?;
        write.set_two_phase_commit(true);
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            table
                .insert("meta/schema", b"3".as_slice())
                .map_err(storage)?;
            table
                .insert("meta/root", root_id.as_bytes())
                .map_err(storage)?;
            table
                .insert("meta/signing-key", signing_key.as_slice())
                .map_err(storage)?;
            table
                .insert("meta/clock", b"0".as_slice())
                .map_err(storage)?;
            table
                .insert(
                    "meta/required-capabilities",
                    encode(&CURRENT_CAPABILITIES)?.as_slice(),
                )
                .map_err(storage)?;
            table
                .insert(
                    "meta/rollback-guard",
                    encode(&Guard {
                        schema_version: 1,
                        migration_id: operation_id.clone(),
                        activation_id: activation_id.clone(),
                        revision: "0".into(),
                    })?
                    .as_slice(),
                )
                .map_err(storage)?;
            table
                .insert("meta/genesis-origin", operation_id.as_bytes())
                .map_err(storage)?;
            table
                .insert(
                    format!("generation-genesis/{operation_id}").as_str(),
                    encode(&origin)?.as_slice(),
                )
                .map_err(storage)?;
            for profile in profiles {
                table
                    .insert(
                        format!("profile/{}", profile.id).as_str(),
                        encode(profile)?.as_slice(),
                    )
                    .map_err(storage)?;
            }
        }
        write.commit().map_err(storage)?;
        drop(database);
        let store = Self::open_owned(path, lock)?;
        store.publish_genesis_prefix(lease, &origin)?;
        Ok(store)
    }
    fn publish_genesis_prefix(&self, lease: &RootLease, origin: &GenesisOrigin) -> Result<()> {
        self.close_generation(lease)?;
        let published = lease
            .append_activation(Activation {
                schema_version: 1,
                sequence: 0,
                id: origin.activation_id.clone(),
                operation_id: origin.operation_id.clone(),
                root_id: origin.root_id.clone(),
                generation: 1,
                store_version: 3,
                location: GenerationLocation::LegacyOne {},
                required_capabilities: CURRENT_CAPABILITIES
                    .iter()
                    .map(|cap| (*cap).into())
                    .collect(),
                previous_sha256: None,
            })
            .and_then(|()| crate::media_generations::flush_activation(lease));
        let reopened = self.reopen_current_generation(lease);
        published?;
        reopened
    }
    pub fn fresh_initialization_origin(&self) -> Result<bool> {
        if self.terminal_restoration_predecessor()?.is_some() {
            return Ok(false);
        }
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let Some(row) = table.get("meta/genesis-origin").map_err(storage)? else {
            return Ok(false);
        };
        let id = std::str::from_utf8(row.value()).map_err(storage)?;
        let original = guard(&table)?;
        Ok(original.migration_id == id && original.validate()? == 0)
    }
    pub fn recover_genesis_prefix(&self, lease: &RootLease) -> Result<()> {
        if lease.selection()?.activation_id.is_some() {
            return Ok(());
        }
        let origin = {
            let read = self.db.begin_read().map_err(storage)?;
            let table = read.open_table(RECORDS).map_err(storage)?;
            let Some(id) = table.get("meta/genesis-origin").map_err(storage)? else {
                return Ok(());
            };
            let id = std::str::from_utf8(id.value()).map_err(storage)?;
            let raw = table
                .get(format!("generation-genesis/{id}").as_str())
                .map_err(storage)?
                .ok_or_else(|| invalid("Interrupted genesis has no original identity proof"))?;
            let origin: GenesisOrigin = decode(raw.value())?;
            let original = guard(&table)?;
            if origin.schema_version != 1
                || origin.operation_id != id
                || origin.root_id != self.root_id()
                || original.migration_id != origin.operation_id
                || original.activation_id != origin.activation_id
                || original.validate()? != 0
            {
                return Err(invalid(
                    "Interrupted genesis changed identity or received old-reader business; automatic enablement stopped",
                ));
            }
            let mut profiles = Vec::new();
            for name in ["agent", "browser"] {
                let row = table
                    .get(format!("profile/{name}").as_str())
                    .map_err(storage)?
                    .ok_or_else(|| invalid("Interrupted genesis profile is missing"))?;
                profiles.push(decode::<AccessProfile>(row.value())?);
            }
            if nw_domain::projects::hash(&encode(&profiles)?) != origin.profiles_sha256 {
                return Err(invalid("Interrupted genesis profile baseline changed"));
            }
            for row in table.iter().map_err(storage)? {
                let (key, value) = row.map_err(storage)?;
                if [
                    "meta/schema",
                    "meta/root",
                    "meta/signing-key",
                    "meta/clock",
                    "meta/required-capabilities",
                    "meta/rollback-guard",
                    "meta/genesis-origin",
                    "profile/agent",
                    "profile/browser",
                ]
                .contains(&key.value())
                    || key.value() == format!("generation-genesis/{id}")
                {
                    continue;
                }
                if key.value().starts_with("scope-rate/") {
                    let (_, count): (u64, u32) = decode(value.value())?;
                    if count <= 32 {
                        continue;
                    }
                }
                return Err(invalid(
                    "Interrupted genesis contains unexplained raw rows; existing data was preserved",
                ));
            }
            origin
        };
        self.publish_genesis_prefix(lease, &origin)
    }
}
