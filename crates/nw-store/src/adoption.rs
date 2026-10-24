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
    }
}
