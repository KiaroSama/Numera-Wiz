//! Adoption is a fact in the same writer transaction as newly accepted work.
use crate::{RECORDS, Store, decode, encode, storage};
use nw_domain::{DomainError, Result};
use redb::{Database, ReadableDatabase, ReadableTable, WriteTransaction};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Guard {
    pub schema_version: u32,
    pub migration_id: String,
    pub activation_id: String,
    pub revision: String,
}
impl Guard {
    pub(crate) fn validate(&self) -> Result<u64> {
        let revision = self.revision.parse::<u64>().map_err(storage)?;
        if self.schema_version != 1
            || revision.to_string() != self.revision
            || uuid::Uuid::parse_str(&self.migration_id).is_err()
            || uuid::Uuid::parse_str(&self.activation_id).is_err()
        {
            return Err(DomainError::new(
                "STORE_RECOVERY_REQUIRED",
                "Generation adoption metadata is invalid; rollback cannot assume no new work",
            ));
        }
        Ok(revision)
    }
}
pub(crate) fn enabled(database: &Database) -> Result<bool> {
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    let capabilities: Vec<String> = table
        .get("meta/required-capabilities")
        .map_err(storage)?
        .map(|value| decode(value.value()))
        .transpose()?
        .unwrap_or_default();
    let declared = capabilities
        .iter()
        .any(|value| value == "adoption_guard.v1");
    if let Some(value) = table.get("meta/rollback-guard").map_err(storage)? {
        if !declared {
            return Err(DomainError::new(
                "STORE_RECOVERY_REQUIRED",
                "Adoption guard is missing its required reader capability",
            ));
        }
        let guard: Guard = decode(value.value())?;
        guard.validate()?;
        Ok(true)
    } else if declared {
        Err(DomainError::new(
            "STORE_RECOVERY_REQUIRED",
            "Required adoption guard is missing",
        ))
    } else {
        Ok(false)
    }
}
pub(crate) fn mark(write: &WriteTransaction) -> Result<()> {
    let mut table = write.open_table(RECORDS).map_err(storage)?;
    let mut guard: Guard = {
        let value = table
            .get("meta/rollback-guard")
            .map_err(storage)?
            .ok_or_else(|| {
                DomainError::new(
                    "STORE_RECOVERY_REQUIRED",
                    "Tracked generation lost its adoption guard",
                )
            })?;
        decode(value.value())?
    };
    let revision = guard.validate()?.checked_add(1).ok_or_else(|| {
        DomainError::new("RESOURCE_LIMIT", "Generation adoption revision exhausted")
    })?;
    guard.revision = revision.to_string();
    table
        .insert("meta/rollback-guard", encode(&guard)?.as_slice())
        .map_err(storage)?;
    Ok(())
}
impl Store {
    /// Missing legacy metadata is unknown, never proof of rollback eligibility.
    pub fn adoption_revision(&self, migration_id: &str, activation_id: &str) -> Result<String> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let value = table
            .get("meta/rollback-guard")
            .map_err(storage)?
            .ok_or_else(|| {
                DomainError::new(
                    "ROLLBACK_UNQUALIFIED",
                    "This generation has no trusted transactional adoption history",
                )
            })?;
        let guard: Guard = decode(value.value())?;
        guard.validate()?;
        if guard.migration_id != migration_id || guard.activation_id != activation_id {
            return Err(DomainError::new(
                "STALE_REVISION",
                "The migration/activation no longer matches this adoption history",
            ));
        }
        Ok(guard.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generations::RootLease;
    use nw_domain::commands::{Request, Response};
    use nw_platform::security;
    struct Fixture {
        store: Store,
        lease: std::sync::Arc<RootLease>,
        principal: String,
        scope: String,
        migration_id: String,
        activation_id: String,
        original: Request,
        response: Response,
        _directory: tempfile::TempDir,
    }
    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let root = security::private_directory(&directory.path().join("root")).unwrap();
        let generation = security::private_directory(&root.join("state/generations/1")).unwrap();
        let store = Store::open(&generation).unwrap();
        let lease = RootLease::acquire(&root).unwrap();
        let principal = format!("owner-{}", store.root_id());
        let scope = store.issue_scope(&principal, 100).unwrap();
        let mut original =
            Request::query("assets.register", serde_json::json!({"path":"original"}));
        original.replay_scope = Some(scope.clone());
        original.idempotency_key = Some("original".into());
        let response = store
            .commit(
                &principal,
                &original,
                101,
                serde_json::json!({"id":"original"}),
                &[],
            )
            .unwrap();
        let plan = store
            .prepare_migration(&lease, &"ab".repeat(32), 102)
            .unwrap();
        let migration_id = plan.id.clone();
        let mut migrate = Request::query("maintenance.migrate", serde_json::json!({"plan":plan}));
        migrate.replay_scope = Some(scope.clone());
        migrate.idempotency_key = Some("migration".into());
        store
            .migrate_generation(
                &lease,
                &principal,
                &migrate,
                &"ab".repeat(32),
                || Ok(103),
                &tokio_util::sync::CancellationToken::new(),
            )
            .unwrap();
        assert!(store.pending_migration().unwrap().is_none());
        let activation_id = lease.selection().unwrap().activation_id.unwrap();
        Fixture {
            store,
            lease,
            principal,
            scope,
            migration_id,
            activation_id,
            original,
            response,
            _directory: directory,
        }
    }
    #[test]
    fn replay_denials_bootstrap_credentials_and_reopen_do_not_invent_adoption() {
        let f = fixture();
        let revision = || {
            f.store
                .adoption_revision(&f.migration_id, &f.activation_id)
                .unwrap()
        };
        assert_eq!(revision(), "0");
        let replay = f
            .store
            .commit(
                &f.principal,
                &f.original,
                104,
                serde_json::json!({"ignored":true}),
                &[("unit/ignored".into(), serde_json::json!(true))],
            )
            .unwrap();
        assert_eq!(replay.receipt_id, f.response.receipt_id);
        assert!(
            f.store
                .get::<serde_json::Value>("unit/ignored")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            revision(),
            "0",
            "A matching receipt aborts the speculative writer before adoption commits"
        );
        let mut conflict = f.original.clone();
        conflict.params["path"] = serde_json::json!("changed");
        assert_eq!(
            f.store
                .commit(&f.principal, &conflict, 104, serde_json::json!(true), &[])
                .unwrap_err()
                .code,
            "IDEMPOTENCY_CONFLICT"
        );
        assert_eq!(revision(), "0");
        f.store.issue_scope(&f.principal, 104).unwrap();
        f.store
            .put("credential/fixture", &serde_json::json!({"expires":105}))
            .unwrap();
        f.store.remove("credential/fixture").unwrap();
        assert_eq!(
            revision(),
            "0",
            "Ephemeral auth bootstrap is not acknowledged business work"
        );
        let path = f.store.generation_path().unwrap();
        drop(f.store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened
                .adoption_revision(&f.migration_id, &f.activation_id)
                .unwrap(),
            "0"
        );
        assert_eq!(f.lease.selection().unwrap().generation, 3);
    }
    #[test]
    fn accepted_business_facts_and_adoption_commit_atomically_and_missing_history_is_unknown() {
        let f = fixture();
        let mut fresh = Request::query("assets.register", serde_json::json!({"path":"new"}));
        fresh.replay_scope = Some(f.scope);
        fresh.idempotency_key = Some("new".into());
        let large = serde_json::json!({"result":"x".repeat(nw_domain::commands::MAX_RESULT_BYTES)});
        assert_eq!(
            f.store
                .commit(
                    &f.principal,
                    &fresh,
                    104,
                    large,
                    &[("unit/new".into(), serde_json::json!(true))]
                )
                .unwrap_err()
                .code,
            "RESOURCE_LIMIT"
        );
        assert!(
            f.store
                .get::<serde_json::Value>("unit/new")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            f.store
                .adoption_revision(&f.migration_id, &f.activation_id)
                .unwrap(),
            "0"
        );
        f.store
            .commit(
                &f.principal,
                &fresh,
                104,
                serde_json::json!({"accepted":true}),
                &[("unit/new".into(), serde_json::json!(true))],
            )
            .unwrap();
        assert_eq!(
            f.store
                .adoption_revision(&f.migration_id, &f.activation_id)
                .unwrap(),
            "1"
        );
        assert_eq!(f.store.get::<bool>("unit/new").unwrap(), Some(true));
        f.store
            .put("unit/later", &serde_json::json!({"acknowledged":true}))
            .unwrap();
        assert_eq!(
            f.store
                .adoption_revision(&f.migration_id, &f.activation_id)
                .unwrap(),
            "2"
        );
        let write = f.store.db.begin_control_write().unwrap();
        {
            write
                .open_table(RECORDS)
                .unwrap()
                .remove("meta/rollback-guard")
                .unwrap();
        }
        write.commit().unwrap();
        assert_eq!(
            f.store
                .adoption_revision(&f.migration_id, &f.activation_id)
                .unwrap_err()
                .code,
            "ROLLBACK_UNQUALIFIED"
        );
        assert!(
            f.store.put("unit/untracked", &true).is_err(),
            "A required missing guard must block fresh writes"
        );
        exercise_restoration_overlay();
        exercise_ancestry_refusal();
        exercise_reserved_workspace_boundary();
    }
    fn raw_rows(store: &Store, prefix: &str) -> Vec<(String, Vec<u8>)> {
        let read = store.db.begin_read().unwrap();
        let table = read.open_table(RECORDS).unwrap();
        table
            .range(prefix..)
            .unwrap()
            .map(|row| {
                let (key, value) = row.unwrap();
                (key.value().to_owned(), value.value().to_vec())
            })
            .take_while(|(key, _)| key.starts_with(prefix))
            .collect()
    }
    fn replace_raw(store: &Store, key: &str, raw: &[u8]) {
        let write = store.db.begin_control_write().unwrap();
        write.open_table(RECORDS).unwrap().insert(key, raw).unwrap();
        write.commit().unwrap();
    }
    fn exercise_ancestry_refusal() {
        let f = fixture();
        let key = format!("generation-migration/{}", f.migration_id);
        let original = raw_rows(&f.store, &key)[0].1.clone();
        for field in ["schema_version", "id", "plan_id", "plan_root", "plan_hash"] {
            let mut corrupted: serde_json::Value = decode(&original).unwrap();
            match field {
                "schema_version" => corrupted["schema_version"] = serde_json::json!(999),
                "id" => corrupted["id"] = serde_json::json!(uuid::Uuid::new_v4().to_string()),
                "plan_id" => {
                    corrupted["plan"]["id"] = serde_json::json!(uuid::Uuid::new_v4().to_string())
                }
                "plan_root" => {
                    corrupted["plan"]["root_id"] =
                        serde_json::json!(uuid::Uuid::new_v4().to_string())
                }
                "plan_hash" => corrupted["plan"]["hash"] = serde_json::json!("00".repeat(32)),
                _ => unreachable!(),
            }
            replace_raw(&f.store, &key, &encode(&corrupted).unwrap());
            let read = f.store.db.begin_read().unwrap();
            let table = read.open_table(RECORDS).unwrap();
            assert!(
                crate::initialization::source_ancestry(
                    &table,
                    f.store.root_id(),
                    &f.lease.selection().unwrap(),
                    &f.lease.compatible_prefix().unwrap(),
                )
                .is_err(),
                "Malformed migrated-r0 ancestry must be refused: {field}"
            );
            drop(table);
            drop(read);
            replace_raw(&f.store, &key, &original);
        }
        let write = f.store.db.begin_control_write().unwrap();
        write
            .open_table(RECORDS)
            .unwrap()
            .remove(key.as_str())
            .unwrap();
        write.commit().unwrap();
        let read = f.store.db.begin_read().unwrap();
        assert!(
            crate::initialization::source_ancestry(
                &read.open_table(RECORDS).unwrap(),
                f.store.root_id(),
                &f.lease.selection().unwrap(),
                &f.lease.compatible_prefix().unwrap(),
            )
            .is_err(),
            "Missing original migration is unknown, never trusted r0"
        );
        drop(read);
        replace_raw(&f.store, &key, &original);
        f.store.put("unit/genuine-adoption", &true).unwrap();
        let mut plan = f
            .store
            .prepare_media_promotion(&f.lease, &"ab".repeat(32), || Ok(105))
            .unwrap();
        let before = raw_rows(&f.store, "");
        let mut future = plan.clone();
        future.created_at = "1000".into();
        future.expires_at = "1300".into();
        future.hash = future.digest().unwrap();
        let mut request = Request::query(
            "maintenance.promotion.accept",
            serde_json::json!({"plan":future}),
        );
        request.replay_scope = Some(f.scope.clone());
        request.idempotency_key = Some("future-promotion-refused".into());
        assert!(
            f.store
                .accept_media_promotion(
                    &f.lease,
                    &f.principal,
                    &request,
                    &"ab".repeat(32),
                    || Ok(106),
                    &tokio_util::sync::CancellationToken::new()
                )
                .is_err()
        );
        assert_eq!(raw_rows(&f.store, ""), before);
        let mut forged: Guard = decode(&raw_rows(&f.store, "meta/rollback-guard")[0].1).unwrap();
        forged.migration_id = uuid::Uuid::new_v4().to_string();
        replace_raw(&f.store, "meta/rollback-guard", &encode(&forged).unwrap());
        plan.source_guard.migration_id = forged.migration_id;
        let read = f.store.db.begin_read().unwrap();
        let (count, hash) =
            crate::media_generations::raw_hash(&read.open_table(RECORDS).unwrap()).unwrap();
        drop(read);
        plan.source_records = count;
        plan.source_records_sha256 = hash;
        plan.hash = plan.digest().unwrap();
        assert!(
            f.store
                .prepare_media_promotion(&f.lease, &"ab".repeat(32), || Ok(106))
                .is_err()
        );
        request.params = serde_json::json!({"plan":plan});
        request.idempotency_key = Some("self-hashed-untrusted-promotion".into());
        let before = raw_rows(&f.store, "");
        assert!(
            f.store
                .accept_media_promotion(
                    &f.lease,
                    &f.principal,
                    &request,
                    &"ab".repeat(32),
                    || Ok(107),
                    &tokio_util::sync::CancellationToken::new()
                )
                .is_err()
        );
        assert_eq!(
            raw_rows(&f.store, ""),
            before,
            "No floor, receipt or journal may follow caller-computable forged ancestry"
        );
        assert!(!f.store.pending_media_maintenance().unwrap());
        assert!(
            f.store
                .replay(&f.principal, &request, 108)
                .unwrap()
                .is_none()
        );
        assert!(!nw_domain::media_execution::review_current("1000", "1300", 999).unwrap());
        assert!(nw_domain::media_execution::review_current("1000", "1300", 1000).unwrap());
        assert!(nw_domain::media_execution::review_current("1000", "1300", 1299).unwrap());
        assert!(!nw_domain::media_execution::review_current("1000", "1300", 1300).unwrap());
        assert!(!nw_domain::media_execution::review_current("1000", "1301", 1100).unwrap());
    }
    fn exercise_reserved_workspace_boundary() {
        let f = fixture();
        f.store.put("unit/genuine-adoption", &true).unwrap();
        // Real cancelled enablement preflights retain their bounded allocation;
        // no successful floor, journal or receipt is fabricated for this pressure.
        for ordinal in 0..6 {
            let plan = f
                .store
                .prepare_media_promotion(&f.lease, &"ab".repeat(32), || Ok(105))
                .unwrap();
            let mut request = Request::query(
                "maintenance.promotion.accept",
                serde_json::json!({"plan":plan}),
            );
            request.replay_scope = Some(f.scope.clone());
            request.idempotency_key = Some(format!("cancelled-workspace-{ordinal}"));
            let cancel = tokio_util::sync::CancellationToken::new();
            cancel.cancel();
            assert!(
                f.store
                    .accept_media_promotion(
                        &f.lease,
                        &f.principal,
                        &request,
                        &"ab".repeat(32),
                        || Ok(106),
                        &cancel
                    )
                    .is_err()
            );
            assert!(!f.store.pending_media_maintenance().unwrap());
        }
        let plan = f
            .store
            .prepare_media_promotion(&f.lease, &"ab".repeat(32), || Ok(107))
            .unwrap();
        let mut request = Request::query(
            "maintenance.promotion.accept",
            serde_json::json!({"plan":plan}),
        );
        request.replay_scope = Some(f.scope.clone());
        request.idempotency_key = Some("last-restorable-enablement".into());
        assert!(
            f.store
                .accept_media_promotion(
                    &f.lease,
                    &f.principal,
                    &request,
                    &"ab".repeat(32),
                    || Ok(108),
                    &tokio_util::sync::CancellationToken::new()
                )
                .unwrap()
                .ok
        );
        f.store.close_generation(&f.lease).unwrap();
        crate::copies::copy_locked(
            &f.lease,
            &uuid::Uuid::new_v4().to_string(),
            "rollback",
            &f.store.generation_path().unwrap(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .unwrap();
        f.store.reopen_current_generation(&f.lease).unwrap();
        assert_eq!(
            std::fs::read_dir(f.lease.state().join("generation-workspaces"))
                .unwrap()
                .count(),
            16
        );
        assert!(crate::copies::workspace(&f.lease, &uuid::Uuid::new_v4().to_string()).is_err());
        let restored = f
            .store
            .prepare_media_restoration(&f.lease, &plan.id, false, &"ab".repeat(32), || Ok(109))
            .unwrap();
        request.command.id = "maintenance.promotion.restore.accept".into();
        request.params = serde_json::json!({"plan":restored});
        request.idempotency_key = Some("reserved-restoration-at-cap".into());
        assert!(
            f.store
                .accept_media_restoration(
                    &f.lease,
                    &f.principal,
                    &request,
                    false,
                    &"ab".repeat(32),
                    || Ok(110)
                )
                .unwrap()
                .ok
        );
        assert!(!f.store.pending_media_maintenance().unwrap());
        assert_eq!(f.lease.selection().unwrap().generation, 3);
        assert_eq!(
            f.store
                .prepare_media_promotion(&f.lease, &"ab".repeat(32), || Ok(111))
                .unwrap_err()
                .code,
            "RESOURCE_LIMIT"
        );
    }
    fn exercise_restoration_overlay() {
        let f = fixture();
        f.store
            .put(
                "unit/opaque-baseline",
                &serde_json::json!({"unknown":[1,2,3]}),
            )
            .unwrap();
        let source_path = f.store.generation_path().unwrap();
        let source_object =
            nw_platform::files::source_object(&source_path.join("store.redb")).unwrap();
        let original_guard = raw_rows(&f.store, "meta/rollback-guard");
        let original_receipts = raw_rows(&f.store, "receipt/");
        let plan = f
            .store
            .prepare_media_promotion(&f.lease, &"ab".repeat(32), || Ok(105))
            .unwrap();
        let mut accept = Request::query(
            "maintenance.promotion.accept",
            serde_json::json!({"plan":plan}),
        );
        accept.replay_scope = Some(f.scope.clone());
        accept.idempotency_key = Some("real-promotion-for-overlay".into());
        let promoted = f
            .store
            .accept_media_promotion(
                &f.lease,
                &f.principal,
                &accept,
                &"ab".repeat(32),
                || Ok(106),
                &tokio_util::sync::CancellationToken::new(),
            )
            .unwrap();
        assert!(promoted.ok);
        assert!(!f.store.pending_media_maintenance().unwrap());
        let restoration = f
            .store
            .prepare_media_restoration(&f.lease, &plan.id, false, &"ab".repeat(32), || Ok(107))
            .unwrap();
        let mut restore = Request::query(
            "maintenance.promotion.restore.accept",
            serde_json::json!({"plan":restoration}),
        );
        restore.replay_scope = Some(f.scope.clone());
        restore.idempotency_key = Some("real-restoration-with-abort".into());
        // First install validates the independent overlay copy; abort the second,
        // real-S transaction after receipt/capability updates but before B.
        crate::restoration_control::ABORT_INSTALL.with(|count| count.set(2));
        let accepted = f
            .store
            .accept_media_restoration(
                &f.lease,
                &f.principal,
                &restore,
                false,
                &"ab".repeat(32),
                || Ok(108),
            )
            .unwrap();
        assert!(accepted.ok);
        assert!(accepted.receipt_id.is_some());
        assert!(f.store.pending_media_maintenance().unwrap());
        assert_eq!(f.lease.selection().unwrap().generation, 5);
        let source = redb::Database::builder()
            .open_read_only(source_path.join("store.redb"))
            .unwrap();
        let read = source.begin_read().unwrap();
        let table = read.open_table(RECORDS).unwrap();
        assert!(
            table
                .get(format!("restoration-control/{}", restoration.id).as_str())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            table.get("meta/rollback-guard").unwrap().unwrap().value(),
            original_guard[0].1
        );
        assert_eq!(
            crate::decode::<Vec<String>>(
                table
                    .get("meta/required-capabilities")
                    .unwrap()
                    .unwrap()
                    .value()
            )
            .unwrap()
            .len(),
            9
        );
        let receipt_key = f.store.replay_key(&f.principal, &restore, 108).unwrap().0;
        assert!(
            table.get(receipt_key.as_str()).unwrap().is_none(),
            "Aborted receipt union and B must disappear together"
        );
        drop(table);
        drop(read);
        drop(source);
        crate::restoration_control::UNCERTAIN_INSTALL.with(|count| count.set(1));
        f.store.recover_media_restoration(&f.lease).unwrap();
        assert_eq!(f.lease.selection().unwrap().generation, 3);
        assert_eq!(
            nw_platform::files::source_object(&source_path.join("store.redb")).unwrap(),
            source_object
        );
        assert_eq!(raw_rows(&f.store, "meta/rollback-guard"), original_guard);
        for (key, raw) in original_receipts {
            assert_eq!(raw_rows(&f.store, &key), vec![(key, raw)]);
        }
        let barrier = raw_rows(&f.store, "restoration-control/");
        assert_eq!(barrier.len(), 1);
        let replay = f
            .store
            .replay_with_clock(&f.principal, &restore, || Ok(10000))
            .unwrap()
            .unwrap();
        assert_eq!(
            replay.receipt_id, accepted.receipt_id,
            "Expired review still replays the genuine raw receipt"
        );
        let observation_path = f
            .lease
            .state()
            .join("generation-workspaces")
            .join(&restoration.id)
            .join("restoration-installed.json");
        let captured = std::fs::read(&observation_path).unwrap();
        let observation: serde_json::Value = decode(&captured).unwrap();
        assert_eq!(
            observation["source_object"],
            serde_json::to_value(&source_object).unwrap()
        );
        assert!(
            observation["file_sha256"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64)
        );
        // Real old control traffic may change allocator bytes after exposure.
        f.store.issue_scope(&f.principal, 109).unwrap();
        drop(f.store);
        crate::restorations::repair_owned_suffix(&f.lease).unwrap();
        let reopened = Store::open_selected(&f.lease).unwrap();
        assert_eq!(raw_rows(&reopened, "restoration-control/"), barrier);
        assert_eq!(
            std::fs::read(&observation_path).unwrap(),
            captured,
            "Post-exposure observations are immutable, never rewritten against later physical bytes"
        );
        assert_eq!(
            reopened
                .get::<serde_json::Value>("unit/opaque-baseline")
                .unwrap()
                .unwrap(),
            serde_json::json!({"unknown":[1,2,3]})
        );
        reopened.close_generation(&f.lease).unwrap();
        let source_file = source_path.join("store.redb");
        let authority: crate::restorations::Authority = decode(
            &std::fs::read(
                observation_path
                    .parent()
                    .unwrap()
                    .join("restoration-authority.json"),
            )
            .unwrap(),
        )
        .unwrap();
        // Retain the actual open-writer recovery header, then restore those same
        // fault bytes through the existing file handle after clean close. Capture
        // through the actual backend so Windows does not independently reopen a
        // file whose byte ranges are still exclusively locked by our writer.
        let dirty = f._directory.path().join("dirty-source-capture.redb");
        assert!(std::fs::metadata(&source_file).unwrap().len() <= 32 * 1024 * 1024);
        reopened.capture_dirty_restoration_source(
            &f.lease,
            &source_path,
            &authority,
            &dirty,
            |database| {
                let write = database.begin_write().unwrap();
                write
                    .open_table(RECORDS)
                    .unwrap()
                    .insert(
                        "unit/opaque-baseline",
                        br#"{"contradiction":true}"#.as_slice(),
                    )
                    .unwrap();
                write.commit().unwrap();
            },
        );
        let mut original_file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&source_file)
            .unwrap();
        let mut dirty_file = std::fs::File::open(&dirty).unwrap();
        std::io::copy(&mut dirty_file, &mut original_file).unwrap();
        original_file.sync_all().unwrap();
        drop(original_file);
        assert_eq!(
            crate::versions::inspect_closed(&source_path)
                .unwrap_err()
                .code,
            "STORE_RECOVERY_REQUIRED"
        );
        let before =
            nw_platform::files::identity(&source_file, &tokio_util::sync::CancellationToken::new())
                .unwrap();
        assert!(
            reopened
                .open_restoration_control(&f.lease, &source_path, &source_object, &authority)
                .is_err()
        );
        let after =
            nw_platform::files::identity(&source_file, &tokio_util::sync::CancellationToken::new())
                .unwrap();
        assert_eq!(
            (after.bytes, after.sha256),
            (before.bytes.clone(), before.sha256.clone()),
            "Contradictory dirty S must not be repaired before full continuity validation"
        );
        drop(reopened);
        assert!(crate::restorations::repair_owned_suffix(&f.lease).is_err());
        let after =
            nw_platform::files::identity(&source_file, &tokio_util::sync::CancellationToken::new())
                .unwrap();
        assert_eq!((after.bytes, after.sha256), (before.bytes, before.sha256));
        assert_eq!(
            nw_platform::files::source_object(&source_file).unwrap(),
            source_object
        );
    }
}
