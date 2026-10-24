//! Closed-store inspection for explicit stopped-generation migration.
//! Inspection never creates, repairs, upgrades or opens a writable database.
use crate::{MAX_RECORD, RECORDS, storage};
use nw_domain::{DomainError, Result};
use nw_platform::files::{self, DirectoryStamp, SourceStamp};
use redb::{Database, DatabaseError, ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

pub const MAX_GENERATION_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_GENERATION_RECORDS: u32 = 50000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GenerationFacts {
    pub store_version: u32,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    pub root_id: String,
    pub store_bytes: String,
    pub record_count: u32,
    pub records_sha256: String,
    pub namespaces: BTreeMap<String, u32>,
}
pub(crate) fn database_error(error: DatabaseError) -> DomainError {
    match error {
        DatabaseError::DatabaseAlreadyOpen => DomainError::new(
            "OWNER_BUSY",
            "Close the active owner before inspecting or migrating this generation",
        ),
        DatabaseError::RepairAborted => DomainError::new(
            "STORE_RECOVERY_REQUIRED",
            "This generation did not close cleanly; inspection does not repair it. Use the explicit owned-copy migration/recovery path",
        ),
        DatabaseError::UpgradeRequired(_) => DomainError::new(
            "STORE_VERSION_UNSUPPORTED",
            "The redb file format requires a separately qualified upgrade",
        ),
        other => storage(other),
    }
}
fn version(value: &[u8]) -> Result<u32> {
    let text = std::str::from_utf8(value).map_err(|_| unsupported())?;
    let parsed = text.parse::<u32>().map_err(|_| unsupported())?;
    if parsed == 0 || parsed.to_string() != text {
        return Err(unsupported());
    }
    Ok(parsed)
}
fn unsupported() -> DomainError {
    DomainError::new(
        "STORE_VERSION_UNSUPPORTED",
        "The existing store has no recognized version envelope; no replacement was created",
    )
}
pub fn inspect_closed(generation: &Path) -> Result<GenerationFacts> {
    let generation = std::fs::canonicalize(generation).map_err(files::io_error)?;
    let directory = DirectoryStamp::capture_resolved(&generation)?;
    let path = generation.join("store.redb");
    let source = SourceStamp::capture_resolved(&path)?;
    let bytes = std::fs::metadata(&path).map_err(files::io_error)?.len();
    if bytes == 0 {
        return Err(unsupported());
    }
    if bytes > MAX_GENERATION_BYTES {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "This store exceeds the declared64GiB generation inspection/migration limit",
        ));
    }
    let database = Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(&path)
        .map_err(database_error)?;
    source.revalidate()?;
    directory.revalidate()?;
    let transaction = database.begin_read().map_err(storage)?;
    let table = transaction.open_table(RECORDS).map_err(|_| unsupported())?;
    let store_version = version(
        table
            .get("meta/schema")
            .map_err(storage)?
            .ok_or_else(unsupported)?
            .value(),
    )?;
    let required_capabilities: Vec<String> = table
        .get("meta/required-capabilities")
        .map_err(storage)?
        .map(|value| crate::decode(value.value()))
        .transpose()?
        .unwrap_or_default();
    if required_capabilities.len() > 64
        || required_capabilities
            .iter()
            .any(|value| value.is_empty() || value.len() > 128)
    {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Required generation capabilities exceed bounded metadata limits",
        ));
    }
    let root = table
        .get("meta/root")
        .map_err(storage)?
        .ok_or_else(unsupported)?;
    let root_id = std::str::from_utf8(root.value())
        .map_err(|_| unsupported())?
        .to_owned();
    if uuid::Uuid::parse_str(&root_id).is_err() {
        return Err(unsupported());
    }
    let key = table
        .get("meta/signing-key")
        .map_err(storage)?
        .ok_or_else(unsupported)?;
    if key.value().len() != 32 {
        return Err(unsupported());
    }
    let mut record_count = 0u32;
    let mut namespaces = BTreeMap::<String, u32>::new();
    let mut hash = Sha256::new();
    hash.update(b"numera-generation-records-v1\0");
    for row in table.iter().map_err(storage)? {
        let (name, value) = row.map_err(storage)?;
        let name = name.value();
        let value = value.value();
        if name.len() > 512 || value.len() > MAX_RECORD {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "A stored record exceeds the admitted inspection bound",
            ));
        }
        record_count = record_count.checked_add(1).ok_or_else(|| {
            DomainError::new("RESOURCE_LIMIT", "Generation record count overflow")
        })?;
        if record_count > MAX_GENERATION_RECORDS {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Generation exceeds50000 admitted records; no subset was returned",
            ));
        }
        let namespace = name.split('/').next().unwrap_or("");
        if !namespaces.contains_key(namespace) && namespaces.len() >= 128 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Generation namespace count exceeds128",
            ));
        }
        *namespaces.entry(namespace.to_owned()).or_default() += 1;
        if !name.starts_with("meta/") {
            // Validate bounded JSON, retaining the original bytes rather than reserializing
            // receipt digests, exact numeric strings or unknown legacy record fields.
            let _: serde_json::Value = serde_json::from_slice(value).map_err(storage)?;
        }
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    source.revalidate()?;
    directory.revalidate()?;
    Ok(GenerationFacts {
        store_version,
        required_capabilities,
        root_id,
        store_bytes: bytes.to_string(),
        record_count,
        records_sha256: hex::encode(hash.finalize()),
        namespaces,
    })
}

/// Establish application codec compatibility before redb gets write access.
/// A dirty legacy file is repaired only in a separately owned verified probe;
/// an unknown source remains byte-for-byte untouched, including its dirty header.
pub(crate) fn preflight_existing(generation: &Path) -> Result<()> {
    let facts = match inspect_closed(generation) {
        Ok(facts) => facts,
        Err(error) if error.code == "STORE_RECOVERY_REQUIRED" => {
            let file = generation.join("store.redb");
            let cancel = tokio_util::sync::CancellationToken::new();
            let identity = files::identity(&file, &cancel)?;
            let bytes: u64 = identity.bytes.parse().map_err(storage)?;
            if bytes == 0 || bytes > MAX_GENERATION_BYTES {
                return Err(unsupported());
            }
            let parent =
                nw_platform::security::private_directory(&generation.join("recovery-probes"))?;
            let mut count = 0usize;
            for entry in std::fs::read_dir(&parent).map_err(files::io_error)? {
                entry.map_err(files::io_error)?;
                count += 1;
                if count >= 8 {
                    return Err(DomainError::new(
                        "RESOURCE_LIMIT",
                        "Eight retained recovery probes require explicit review; no probe was deleted",
                    ));
                }
            }
            crate::copies::require_copy_space(&parent, bytes, 1)?;
            let operation = uuid::Uuid::new_v4().to_string();
            let probe = nw_platform::security::private_directory(&parent.join(&operation))?;
            let manifest = serde_json::to_vec(&serde_json::json!({
                "schema_version":1,"operation_id":operation,
                "source_bytes":identity.bytes,"source_sha256":identity.sha256,
                "purpose":"read_compatibility_before_source_repair"
            }))
            .map_err(storage)?;
            nw_platform::security::write_private(&probe.join("probe.json"), &manifest, false)?;
            files::verified_copy_expected(&identity, &probe.join("store.redb"), &cancel)?;
            let database = Database::builder()
                .set_cache_size(8 * 1024 * 1024)
                .open(probe.join("store.redb"))
                .map_err(database_error)?;
            drop(database);
            let facts = inspect_closed(&probe)?;
            let unchanged = files::identity(&file, &cancel)?;
            if unchanged.bytes != identity.bytes || unchanged.sha256 != identity.sha256 {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Original store changed during owned recovery inspection",
                ));
            }
            facts
        }
        Err(error) => return Err(error),
    };
    if ![2, 3].contains(&facts.store_version)
        || (facts.store_version == 3
            && (facts.required_capabilities.is_empty()
                || facts.required_capabilities.iter().any(|value| {
                    !crate::generations::CURRENT_CAPABILITIES.contains(&value.as_str())
                })))
    {
        return Err(DomainError::new(
            "STORE_VERSION_UNSUPPORTED",
            "The existing application store version is unsupported; original bytes were not opened writable",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    // The hashing oracle below uses a fixed buffer independently of redb traversal.
    fn file_hash(path: &Path) -> String {
        use std::io::Read;
        let mut file = std::fs::File::open(path).unwrap();
        let mut buffer = [0u8; 65536];
        let mut hash = Sha256::new();
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        hex::encode(hash.finalize())
    }
    #[test]
    fn inspection_is_read_only_and_retains_raw_receipt_identity_and_numeric_strings() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::open(temporary.path()).unwrap();
        let root_id = store.root_id().to_owned();
        store
            .put(
                "unit/exact",
                &serde_json::json!({"pts":"9007199254740993","future_field":{"preserve":true}}),
            )
            .unwrap();
        let scope = store.issue_scope("alice", 100).unwrap();
        let mut request = nw_domain::commands::Request::query(
            "assets.register",
            serde_json::json!({"path":"/synthetic-unit"}),
        );
        request.replay_scope = Some(scope);
        request.idempotency_key = Some("original".into());
        let accepted = store
            .commit_with_clock(
                "alice",
                &request,
                || Ok(101),
                serde_json::json!({"id":"retained"}),
                &[],
            )
            .unwrap();
        assert_eq!(
            inspect_closed(temporary.path()).unwrap_err().code,
            "OWNER_BUSY"
        );
        drop(store);
        let path = temporary.path().join("store.redb");
        let before = file_hash(&path);
        let facts = inspect_closed(temporary.path()).unwrap();
        let repeated = inspect_closed(temporary.path()).unwrap();
        assert_eq!(facts, repeated);
        assert_eq!(facts.root_id, root_id);
        assert_eq!(facts.namespaces.get("receipt"), Some(&1));
        assert_eq!(
            before,
            file_hash(&path),
            "Read-only inspection must not repair or modify the existing file"
        );
        let store = Store::open(temporary.path()).unwrap();
        assert_eq!(
            store
                .replay("alice", &request, 102)
                .unwrap()
                .unwrap()
                .receipt_id,
            accepted.receipt_id
        );
        assert_eq!(
            store
                .get::<serde_json::Value>("unit/exact")
                .unwrap()
                .unwrap()["pts"],
            "9007199254740993"
        );
    }
    #[test]
    fn newer_versions_are_visible_without_writes_and_empty_unknown_files_are_not_initialized() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::open(temporary.path()).unwrap();
        let write = store.db.begin_write().unwrap();
        {
            let mut table = write.open_table(RECORDS).unwrap();
            table.insert("meta/schema", b"999".as_slice()).unwrap();
        }
        write.commit().unwrap();
        drop(store);
        let file = temporary.path().join("store.redb");
        let original = file_hash(&file);
        assert_eq!(inspect_closed(temporary.path()).unwrap().store_version, 999);
        assert_eq!(file_hash(&file), original);
        let other = tempfile::tempdir().unwrap();
        let empty = other.path().join("store.redb");
        std::fs::write(&empty, []).unwrap();
        assert_eq!(
            inspect_closed(other.path()).unwrap_err().code,
            "STORE_VERSION_UNSUPPORTED"
        );
        assert_eq!(std::fs::metadata(empty).unwrap().len(), 0);
    }
}
