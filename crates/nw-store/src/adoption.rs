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
        f.store.close_generation(&f.lease).unwrap();
        crate::restorations::repair_owned_suffix(&f.lease).unwrap();
        f.store.attach_media_selection(&f.lease).unwrap();
        assert_eq!(raw_rows(&f.store, "restoration-control/"), barrier);
        assert_eq!(
            f.store
                .get::<serde_json::Value>("unit/opaque-baseline")
                .unwrap()
                .unwrap(),
            serde_json::json!({"unknown":[1,2,3]})
        );
    }
}
