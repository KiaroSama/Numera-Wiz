//! Explicit schema2-to3 migration using the ordinary durable acceptance kernel.
//! The rollback image is immutable; reader fencing and the acceptance receipt commit together.
use crate::{
    RECORDS, Store, copies, encode,
    generations::{
        Activation, CURRENT_CAPABILITIES, GenerationLocation, GenerationRole, RootLease,
    },
    storage,
};
use nw_domain::{
    DomainError, Result,
    commands::{Request, Response},
    maintenance::{CURRENT_STORE_VERSION, MigrationPhase, MigrationPlan, MigrationRecord},
};
use redb::ReadableTable;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
thread_local! {
    static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}
fn checkpoint(name: &'static str) -> Result<()> {
    #[cfg(test)]
    if FAULT.with(|fault| fault.get() == Some(name)) {
        return Err(DomainError::new(
            "INJECTED_INTERRUPTION",
            format!("Simulated stop at{name}"),
        ));
    }
    let _ = name;
    Ok(())
}
fn invalid(message: &'static str) -> DomainError {
    DomainError::new("INVALID_MIGRATION_PLAN", message)
}
impl Store {
    pub fn store_version(&self) -> Result<u32> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let value = table
            .get("meta/schema")
            .map_err(storage)?
            .ok_or_else(|| invalid("Missing store version"))?;
        std::str::from_utf8(value.value())
            .map_err(storage)?
            .parse()
            .map_err(storage)
    }
    pub fn pending_migration(&self) -> Result<Option<String>> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        table
            .get("meta/pending-migration")
            .map_err(storage)?
            .map(|value| String::from_utf8(value.value().to_vec()).map_err(storage))
            .transpose()
    }
    pub fn inspect_migration(&self, id: &str) -> Result<MigrationRecord> {
        let parsed =
            uuid::Uuid::parse_str(id).map_err(|_| invalid("Migration identity must be a UUID"))?;
        if parsed.to_string() != id {
            return Err(invalid("Migration identity must be canonical"));
        }
        self.get(&format!("generation-migration/{id}"))?
            .ok_or_else(DomainError::object_not_found)
    }
    /// Called with application admission drained. No migration or reader-floor is accepted here.
    pub fn prepare_migration(
        &self,
        lease: &RootLease,
        application_build: &str,
        now: u64,
    ) -> Result<MigrationPlan> {
        let selection = lease.selection()?;
        if self.pending_migration()?.is_some() {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "An accepted migration must recover before another review",
            ));
        }
        if self.store_version()? != 2 {
            return Err(DomainError::new(
                "MIGRATION_NOT_REQUIRED",
                "This generation already uses the current store codec",
            ));
        }
        self.validate_migration_records()?;
        self.close_generation(lease)?;
        let result = (|| {
            let facts = lease.inspect_selected()?;
            if facts.store_version != 2 {
                return Err(invalid("The reviewed source must use schema2"));
            }
            let rollback_generation = selection
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("Generation identity overflow"))?;
            let target_generation = rollback_generation
                .checked_add(1)
                .ok_or_else(|| invalid("Generation identity overflow"))?;
            copies::require_copy_space(
                lease.state(),
                facts.store_bytes.parse().map_err(storage)?,
                2,
            )?;
            let mut plan = MigrationPlan {
                schema_version: 1,
                id: uuid::Uuid::new_v4().to_string(),
                hash: String::new(),
                root_id: self.root_id().into(),
                source_generation: selection.generation,
                source_activation_id: selection.activation_id,
                rollback_generation,
                target_generation,
                source_store_version: 2,
                target_store_version: CURRENT_STORE_VERSION,
                source_records_sha256: facts.records_sha256,
                source_bytes: facts.store_bytes,
                source_records: facts.record_count,
                application_build: application_build.into(),
                required_capabilities: CURRENT_CAPABILITIES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                created_at: now.to_string(),
                expires_at: now
                    .checked_add(300)
                    .ok_or_else(|| invalid("Clock range exhausted"))?
                    .to_string(),
            };
            plan.hash = plan.digest()?;
            Ok(plan)
        })();
        let reopened = self.reopen_current_generation(lease);
        match (result, reopened) {
            (Ok(plan), Ok(())) => Ok(plan),
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(error),
        }
    }
    fn validate_migration_records(&self) -> Result<()> {
        self.visit::<nw_domain::access::AccessProfile>("profile/", |profile| {
            if profile.schema_version != 2 || profile.principal.is_empty() || profile.revision == 0
            {
                return Err(invalid(
                    "A retained profile has unsupported identity or revision facts",
                ));
            }
            Ok(())
        })?;
        self.visit::<serde_json::Value>("plan/", |raw| nw_domain::plans::decode(raw).map(|_| ()))?;
        self.visit::<serde_json::Value>("job/", |raw| {
            let bytes = serde_json::to_vec(&raw).map_err(storage)?;
            let job = crate::records::decode_job(&bytes)?;
            if !job.state.terminal() {
                return Err(DomainError::new(
                    "OWNER_BUSY",
                    "Accepted jobs must finish or be explicitly cancelled before migration",
                ));
            }
            if job.state == nw_domain::records::JobState::Succeeded
                && (job.artifact.is_none() || job.verification.is_none())
            {
                return Err(invalid("A successful job lacks verified publication facts"));
            }
            Ok(())
        })?;
        self.visit::<serde_json::Value>("upload/", |raw| {
            if matches!(
                raw.get("phase").and_then(|value| value.as_str()),
                Some("receiving" | "reserved")
            ) {
                return Err(DomainError::new(
                    "OWNER_BUSY",
                    "Accepted intake must finish before migration",
                ));
            }
            Ok(())
        })
    }
    /// The caller drains application work and serializes admission before entering this method.
    /// Preparation failures leave schema2 untouched; accepted work remains resumable after restart.
    pub fn migrate_generation(
        &self,
        lease: &RootLease,
        principal: &str,
        request: &Request,
        application_build: &str,
        clock: impl Fn() -> Result<u64>,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if request.command.id != "maintenance.migrate" {
            return Err(invalid(
                "This acceptance primitive owns maintenance.migrate only",
            ));
        }
        if let Some(response) = self.replay_with_clock(principal, request, &clock)? {
            return Ok(response);
        }
        let submitted: nw_domain::maintenance::MigrationSubmitInput =
            nw_domain::commands::parse(request.params.clone())?;
        let plan = &submitted.plan;
        plan.validate_hash()?;
        if plan.root_id != self.root_id()
            || plan.application_build != application_build
            || plan.expires_at.parse::<u64>().map_err(storage)? <= clock()?
        {
            return Err(invalid(
                "Migration root/build changed or the review expired",
            ));
        }
        let selection = lease.selection()?;
        if selection.generation != plan.source_generation
            || selection.activation_id != plan.source_activation_id
        {
            return Err(invalid("The reviewed active generation changed"));
        }
        self.validate_migration_records()?;
        self.close_generation(lease)?;
        let prepared = (|| {
            let facts = lease.inspect_selected()?;
            if facts.store_version != 2
                || facts.records_sha256 != plan.source_records_sha256
                || facts.record_count != plan.source_records
            {
                return Err(invalid(
                    "Source version or raw records changed after review; prepare a new migration plan",
                ));
            }
            // redb may resize its physical file when a clean writer reopens/closes.
            // Freshness binds every raw key/value, not allocator-only file length.
            // Recompute complete-copy space from the actual current file below.
            copies::require_copy_space(
                lease.state(),
                facts.store_bytes.parse().map_err(storage)?,
                2,
            )?;
            let source = self.generation_path()?;
            copies::copy_locked(lease, &plan.id, "rollback", &source, cancel)
        })();
        let reopened = self.reopen_current_generation(lease);
        prepared?;
        reopened?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Migration cancelled before durable acceptance",
            ));
        }
        let activation_id = uuid::Uuid::new_v4().to_string();
        let response=self.commit_composed_with_clock(principal,request,&clock,|table,now|{
            if now >= plan.expires_at.parse::<u64>().map_err(storage)? {return Err(DomainError::new("PLAN_EXPIRED","Migration review expired before acceptance"));}
            let schema=table.get("meta/schema").map_err(storage)?.ok_or_else(||invalid("Missing source version"))?.value().to_vec();
            if schema!=b"2" {return Err(invalid("Source reader floor changed"));}
            let record=MigrationRecord{
                schema_version:1,id:plan.id.clone(),creator:principal.into(),plan:plan.clone(),phase:MigrationPhase::Accepted,
                accepted_at:now.to_string(),completed_at:None,activation_id:Some(activation_id.clone()),error:None,
            };
            let key=format!("generation-migration/{}",plan.id);
            if table.get(key.as_str()).map_err(storage)?.is_some(){return Err(invalid("Migration operation identity already exists"));}
            // A historical binary recognizes schema2 only. This fence and the ordinary
            // replay receipt commit in the same redb transaction, after a complete backup.
            table.insert("meta/schema",b"3".as_slice()).map_err(storage)?;
            table.insert("meta/required-capabilities",encode(&CURRENT_CAPABILITIES)?.as_slice()).map_err(storage)?;
            table.insert("meta/pending-migration",plan.id.as_bytes()).map_err(storage)?;
            table.insert(key.as_str(),encode(&record)?.as_slice()).map_err(storage)?;
            Ok(serde_json::json!({"migration_id":plan.id,"target_generation":plan.target_generation,"state":"accepted"}))
        })?;
        // The accepted operation continues independently of a request disconnect.
        if let Err(error) =
            checkpoint("accepted").and_then(|()| self.recover_migration(lease, &clock))
        {
            self.write_migration_error(lease, &plan.id, &error)?;
        }
        Ok(response)
    }
    fn write_migration_error(
        &self,
        lease: &RootLease,
        id: &str,
        error: &DomainError,
    ) -> Result<()> {
        let operation = copies::workspace(lease, id)?;
        let path = operation.join("attempt-error.json");
        let bytes = serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"migration_id":id,"error":error}),
        )
        .map_err(storage)?;
        nw_platform::security::write_private(
            &path,
            &bytes,
            path.try_exists().map_err(nw_platform::files::io_error)?,
        )
    }
    /// Recover only an already accepted operation. No fresh command or receipt is invented.
    pub fn recover_migration(
        &self,
        lease: &RootLease,
        clock: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        let Some(id) = self.pending_migration()? else {
            return Ok(());
        };
        let record = self.inspect_migration(&id)?;
        record.plan.validate_hash()?;
        if record.plan.root_id != self.root_id() || self.store_version()? != 3 {
            return Err(invalid(
                "Accepted migration identity or source fence differs",
            ));
        }
        let expected = record
            .activation_id
            .clone()
            .ok_or_else(|| invalid("Accepted migration has no activation identity"))?;
        let selection = lease.selection()?;
        if selection.activation_id.as_deref() != Some(expected.as_str()) {
            if selection.generation != record.plan.source_generation
                || selection.activation_id != record.plan.source_activation_id
            {
                return Err(invalid(
                    "Another activation replaced the accepted migration source",
                ));
            }
            self.close_generation(lease)?;
            let switch = (|| {
                let source = self.generation_path()?;
                let target =
                    copies::copy_locked(lease, &id, "target", &source, &CancellationToken::new())?;
                checkpoint("copied")?;
                if target.facts.store_version != 3 || target.facts.root_id != self.root_id() {
                    return Err(invalid(
                        "Target copy lacks accepted reader floor or identity",
                    ));
                }
                lease.append_activation(Activation {
                    schema_version: 1,
                    sequence: 0,
                    id: expected.clone(),
                    operation_id: id.clone(),
                    root_id: self.root_id().into(),
                    generation: record.plan.target_generation,
                    store_version: 3,
                    location: GenerationLocation::Workspace {
                        operation_id: id.clone(),
                        role: GenerationRole::Target,
                    },
                    required_capabilities: record.plan.required_capabilities.clone(),
                    previous_sha256: None,
                })?;
                checkpoint("activated")?;
                self.attach_activated_generation(lease)
            })();
            if let Err(error) = switch {
                // Preserve all copies. Reattach only the generation selected by a
                // complete activation, or the original when no activation committed.
                if lease.selection()?.activation_id.as_deref() == Some(expected.as_str()) {
                    self.attach_activated_generation(lease)?;
                } else {
                    self.reopen_current_generation(lease)?;
                }
                return Err(error);
            }
        }
        let write = self.db.begin_write().map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let mut completed = record;
            completed.phase = MigrationPhase::Succeeded;
            let now = clock()?;
            crate::check_clock(&table, now)?;
            completed.completed_at = Some(now.to_string());
            completed.error = None;
            table
                .insert(
                    format!("generation-migration/{id}").as_str(),
                    encode(&completed)?.as_slice(),
                )
                .map_err(storage)?;
            table.remove("meta/pending-migration").map_err(storage)?;
            table
                .insert("meta/clock", now.to_string().as_bytes())
                .map_err(storage)?;
        }
        write.commit().map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions;
    use nw_platform::security;
    #[test]
    fn actual_receipts_and_exact_records_survive_fence_copy_activation_interruptions_and_restart() {
        for fault in ["accepted", "copied", "activated"] {
            let temporary = tempfile::tempdir().unwrap();
            let root = security::private_directory(&temporary.path().join("root")).unwrap();
            let source = security::private_directory(&root.join("state/generations/1")).unwrap();
            let store = Store::open(&source).unwrap();
            let lease = RootLease::acquire(&root).unwrap();
            let principal = format!("owner-{}", store.root_id());
            let scope = store.issue_scope(&principal, 100).unwrap();
            store
                .put(
                    "unit/exact",
                    &serde_json::json!({"pts":"9007199254740993","unknown":{"keep":true}}),
                )
                .unwrap();
            let mut original =
                Request::query("assets.register", serde_json::json!({"path":"historical"}));
            original.replay_scope = Some(scope.clone());
            original.idempotency_key = Some("before-migration".into());
            let before = store
                .commit_with_clock(
                    &principal,
                    &original,
                    || Ok(101),
                    serde_json::json!({"asset_id":"retained"}),
                    &[],
                )
                .unwrap();
            let plan = store
                .prepare_migration(&lease, &"ab".repeat(32), 102)
                .unwrap();
            let mut request =
                Request::query("maintenance.migrate", serde_json::json!({"plan":plan}));
            request.replay_scope = Some(scope);
            request.idempotency_key = Some("one-migration".into());
            let submitted: nw_domain::maintenance::MigrationSubmitInput =
                nw_domain::commands::parse(request.params.clone()).unwrap();
            FAULT.with(|value| value.set(Some(fault)));
            let accepted = store
                .migrate_generation(
                    &lease,
                    &principal,
                    &request,
                    &"ab".repeat(32),
                    || Ok(103),
                    &CancellationToken::new(),
                )
                .unwrap();
            FAULT.with(|value| value.set(None));
            assert!(accepted.receipt_id.is_some());
            assert_eq!(
                versions::inspect_closed(
                    &root
                        .join("state/generation-workspaces")
                        .join(&submitted.plan.id)
                        .join("rollback")
                )
                .unwrap()
                .store_version,
                2
            );
            drop(store);
            drop(lease);
            // A historical schema2 reader sees a durable incompatible floor at
            // its hard-coded generation1, even if activation did not yet commit.
            assert_eq!(versions::inspect_closed(&source).unwrap().store_version, 3);
            let lease = RootLease::acquire(&root).unwrap();
            let path = lease.path(&lease.selection().unwrap()).unwrap();
            let recovered = Store::open(&path).unwrap();
            recovered.recover_migration(&lease, || Ok(104)).unwrap();
            assert_eq!(lease.selection().unwrap().generation, 3);
            assert_eq!(recovered.store_version().unwrap(), 3);
            assert_eq!(
                recovered
                    .inspect_migration(&submitted.plan.id)
                    .unwrap()
                    .phase,
                MigrationPhase::Succeeded
            );
            assert_eq!(
                recovered
                    .replay(&principal, &request, 105)
                    .unwrap()
                    .unwrap()
                    .receipt_id,
                accepted.receipt_id
            );
            assert_eq!(
                recovered
                    .replay(&principal, &original, 105)
                    .unwrap()
                    .unwrap()
                    .receipt_id,
                before.receipt_id
            );
            assert_eq!(
                recovered
                    .get::<serde_json::Value>("unit/exact")
                    .unwrap()
                    .unwrap()["pts"],
                "9007199254740993"
            );
            let again = recovered
                .migrate_generation(
                    &lease,
                    &principal,
                    &request,
                    &"ab".repeat(32),
                    || Ok(105),
                    &CancellationToken::new(),
                )
                .unwrap();
            assert_eq!(again.receipt_id, accepted.receipt_id);
            assert_eq!(
                recovered
                    .list::<MigrationRecord>("generation-migration/", 16)
                    .unwrap()
                    .len(),
                1
            );
        }
    }
    #[test]
    fn stale_review_never_fences_or_creates_an_accepted_migration() {
        let temporary = tempfile::tempdir().unwrap();
        let root = security::private_directory(&temporary.path().join("root")).unwrap();
        let source = security::private_directory(&root.join("state/generations/1")).unwrap();
        let store = Store::open(&source).unwrap();
        let lease = RootLease::acquire(&root).unwrap();
        let scope = store.issue_scope("owner", 100).unwrap();
        let plan = store
            .prepare_migration(&lease, &"ab".repeat(32), 101)
            .unwrap();
        store
            .put("unit/new", &serde_json::json!({"new":true}))
            .unwrap();
        let mut request = Request::query("maintenance.migrate", serde_json::json!({"plan":plan}));
        request.replay_scope = Some(scope);
        request.idempotency_key = Some("stale".into());
        assert_eq!(
            store
                .migrate_generation(
                    &lease,
                    "owner",
                    &request,
                    &"ab".repeat(32),
                    || Ok(102),
                    &CancellationToken::new()
                )
                .unwrap_err()
                .code,
            "INVALID_MIGRATION_PLAN"
        );
        assert_eq!(store.store_version().unwrap(), 2);
        assert!(store.replay("owner", &request, 103).unwrap().is_none());
        assert!(
            store
                .get::<serde_json::Value>("unit/new")
                .unwrap()
                .is_some()
        );
    }
}
