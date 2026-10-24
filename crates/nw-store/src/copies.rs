//! Manifest-bound closed-store copies used by generation migration and recovery.
//! The source is never opened writable, overwritten, repaired or deleted here.
use crate::{storage, versions::{self, GenerationFacts}};
use nw_domain::{DomainError, Result, records::FileIdentity};
use nw_platform::{files::{self, DirectoryStamp, RootLock}, security};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

const MAX_WORKSPACES: usize = 16;
const SPACE_MARGIN: u64 = 64 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CopyManifest {
    pub schema_version: u32,
    pub operation_id: String,
    pub role: String,
    pub source: GenerationFacts,
    pub source_file_sha256: String,
    pub source_file_bytes: String,
}
pub struct ClosedCopy {
    pub path: PathBuf,
    pub facts: GenerationFacts,
    pub file_sha256: String,
}
fn conflict(message: &'static str) -> DomainError {
    DomainError::new("STORE_COPY_CONFLICT", message)
}
fn same_facts(expected: &GenerationFacts, actual: &GenerationFacts) -> bool {
    expected == actual
}
fn manifest_bytes(manifest: &CopyManifest) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(storage)?;
    if bytes.len() > 65536 {
        return Err(DomainError::new("RESOURCE_LIMIT", "Generation copy manifest exceeds64KiB"));
    }
    Ok(bytes)
}
/// Reserve one canonical UUID workspace without claiming unrelated files as owned.
pub fn workspace(lease: &crate::generations::RootLease, operation_id: &str) -> Result<PathBuf> {
    let parsed = uuid::Uuid::parse_str(operation_id).map_err(|_| conflict("Copy operation identity must be a UUID"))?;
    if parsed.to_string() != operation_id {
        return Err(conflict("Copy operation identity must be canonical"));
    }
    lease.revalidate()?;
    let parent = security::private_directory(&lease.state().join("generation-workspaces"))?;
    let stamp = DirectoryStamp::capture_resolved(&parent)?;
    let mut count = 0usize;
    for member in std::fs::read_dir(&parent).map_err(files::io_error)? {
        member.map_err(files::io_error)?;
        count += 1;
        if count > MAX_WORKSPACES {
            return Err(DomainError::new("RESOURCE_LIMIT", "Retained generation workspace quota reached; nothing was evicted"));
        }
    }
    let path = parent.join(operation_id);
    if !path.try_exists().map_err(files::io_error)? && count >= MAX_WORKSPACES {
        return Err(DomainError::new("RESOURCE_LIMIT", "At most16 generation workspaces may be retained"));
    }
    let path = security::private_directory(&path)?;
    stamp.revalidate()?;
    lease.revalidate()?;
    Ok(path)
}
/// Check the complete copy requirement before allocating either retained generation.
/// Reservations remain conservative; another disk user can still exhaust space later.
pub fn require_copy_space(parent: &Path, bytes: u64, copies: u64) -> Result<()> {
    if bytes == 0 || bytes > versions::MAX_GENERATION_BYTES || !(1..=3).contains(&copies) {
        return Err(DomainError::new("RESOURCE_LIMIT", "Generation copy size/count is outside admitted limits"));
    }
    let required = bytes.checked_mul(copies).and_then(|value| value.checked_add(SPACE_MARGIN))
        .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Generation space estimate overflow"))?;
    if fs2::available_space(parent).map_err(files::io_error)? < required {
        return Err(DomainError::new("SCRATCH_EXHAUSTED", "Complete retained generation copies and64MiB safety margin do not fit; source is untouched"));
    }
    Ok(())
}
/// Copy a closed generation through the existing full-byte verified copy primitive.
/// A resumable destination requires the exact private manifest and complete file hash.
/// A matching-looking filename alone never proves ownership or completeness.
pub fn copy_closed(
    lease: &crate::generations::RootLease,
    operation_id: &str,
    role: &str,
    source: &Path,
    cancel: &CancellationToken,
) -> Result<ClosedCopy> {
    if !["rollback", "target", "restoration"].contains(&role) {
        return Err(conflict("Unknown generation workspace role"));
    }
    lease.revalidate()?;
    let source = std::fs::canonicalize(source).map_err(files::io_error)?;
    let _source_guard = RootLock::acquire(&source)?;
    copy_locked(lease, operation_id, role, &source, cancel)
}
/// The caller already retains the source's generation lock, including after redb closes.
/// Kept crate-private so the ordinary command path cannot bypass ownership.
pub(crate) fn copy_locked(
    lease: &crate::generations::RootLease,
    operation_id: &str,
    role: &str,
    source: &Path,
    cancel: &CancellationToken,
) -> Result<ClosedCopy> {
    if !["rollback", "target", "restoration"].contains(&role) {
        return Err(conflict("Unknown generation workspace role"));
    }
    let source_facts = versions::inspect_closed(source)?;
    let source_file = source.join("store.redb");
    let source_identity = files::identity(&source_file, cancel)?;
    if source_identity.bytes != source_facts.store_bytes {
        return Err(conflict("Source length changed after closed inspection"));
    }
    let operation = workspace(lease, operation_id)?;
    require_copy_space(&operation, source_identity.bytes.parse().map_err(storage)?, 1)?;
    let destination = security::private_directory(&operation.join(role))?;
    let destination_stamp = DirectoryStamp::capture_resolved(&destination)?;
    let _destination_guard = RootLock::acquire(&destination)?;
    let manifest = CopyManifest {
        schema_version: 1,
        operation_id: operation_id.into(),
        role: role.into(),
        source: source_facts.clone(),
        source_file_sha256: source_identity.sha256.clone(),
        source_file_bytes: source_identity.bytes.clone(),
    };
    let manifest_path = destination.join("copy.json");
    if manifest_path.try_exists().map_err(files::io_error)? {
        let bytes = security::read_private(&manifest_path, 65536)?;
        let existing: CopyManifest = serde_json::from_slice(&bytes).map_err(storage)?;
        if existing != manifest {
            return Err(conflict("Existing workspace manifest belongs to different source facts"));
        }
    } else {
        if destination.join("store.redb").try_exists().map_err(files::io_error)? {
            return Err(conflict("Existing database has no matching ownership manifest; it was preserved"));
        }
        security::write_private(&manifest_path, &manifest_bytes(&manifest)?, false)?;
    }
    let output = destination.join("store.redb");
    let actual: FileIdentity = if output.try_exists().map_err(files::io_error)? {
        files::identity(&output, cancel)?
    } else {
        files::verified_copy_expected(&source_identity, &output, cancel)?
    };
    if actual.bytes != source_identity.bytes || actual.sha256 != source_identity.sha256 {
        return Err(conflict("Owned copy is incomplete or differs; it was retained for explicit recovery"));
    }
    // A second independent database traversal proves record and metadata identity.
    let copied = versions::inspect_closed(&destination)?;
    if !same_facts(&source_facts, &copied) {
        return Err(conflict("Copied generation records differ from the closed source"));
    }
    let current = versions::inspect_closed(source)?;
    if !same_facts(&source_facts, &current) {
        return Err(conflict("Source facts changed during verified copy"));
    }
    destination_stamp.revalidate()?;
    lease.revalidate()?;
    let complete = serde_json::to_vec(&serde_json::json!({
        "schema_version":1,
        "manifest_sha256":hex::encode(Sha256::digest(manifest_bytes(&manifest)?)),
        "store_sha256":actual.sha256,
        "store_bytes":actual.bytes,
        "records_sha256":copied.records_sha256,
    })).map_err(storage)?;
    let completion = destination.join("complete.json");
    if completion.try_exists().map_err(files::io_error)? {
        if security::read_private(&completion,65536)? != complete {
            return Err(conflict("Existing completion proof differs"));
        }
    } else {
        security::write_private(&completion, &complete, false)?;
    }
    Ok(ClosedCopy { path: destination, facts: copied, file_sha256: actual.sha256 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, generations::RootLease};
    #[test]
    fn immutable_copy_preserves_exact_records_receipts_and_restarts_from_complete_proof() {
        let temporary = tempfile::tempdir().unwrap();
        let root = security::private_directory(&temporary.path().join("root")).unwrap();
        let source = security::private_directory(&root.join("state/generations/1")).unwrap();
        let store = Store::open(&source).unwrap();
        store.put("exact/value", &serde_json::json!({"ticks":"9007199254740993","unknown":{"keep":true}})).unwrap();
        let scope = store.issue_scope("alice",100).unwrap();
        let mut request = nw_domain::commands::Request::query("assets.register",serde_json::json!({"path":"retained"}));
        request.replay_scope=Some(scope);
        request.idempotency_key=Some("retained-key".into());
        let accepted=store.commit_with_clock("alice",&request,||Ok(101),serde_json::json!({"asset_id":"same"}),&[]).unwrap();
        let root_id=store.root_id().to_owned();
        drop(store);
        let lease=RootLease::acquire(&root).unwrap();
        let operation=uuid::Uuid::new_v4().to_string();
        let copied=copy_closed(&lease,&operation,"rollback",&source,&CancellationToken::new()).unwrap();
        let repeated=copy_closed(&lease,&operation,"rollback",&source,&CancellationToken::new()).unwrap();
        assert_eq!(copied.file_sha256,repeated.file_sha256);
        assert_eq!(copied.facts,repeated.facts);
        let copy=Store::open(&copied.path).unwrap();
        assert_eq!(copy.root_id(),root_id);
        assert_eq!(copy.get::<serde_json::Value>("exact/value").unwrap().unwrap()["ticks"],"9007199254740993");
        assert_eq!(copy.replay("alice",&request,102).unwrap().unwrap().receipt_id,accepted.receipt_id);
    }
    #[test]
    fn live_source_and_unproven_existing_destination_are_preserved() {
        let temporary=tempfile::tempdir().unwrap();
        let root=security::private_directory(&temporary.path().join("root")).unwrap();
        let source=security::private_directory(&root.join("state/generations/1")).unwrap();
        let store=Store::open(&source).unwrap();
        let lease=RootLease::acquire(&root).unwrap();
        let operation=uuid::Uuid::new_v4().to_string();
        assert_eq!(copy_closed(&lease,&operation,"rollback",&source,&CancellationToken::new()).err().unwrap().code,"OWNER_BUSY");
        drop(store);
        let destination=security::private_directory(&workspace(&lease,&operation).unwrap().join("rollback")).unwrap();
        let output=destination.join("store.redb");
        std::fs::write(&output,b"unproven").unwrap();
        assert_eq!(copy_closed(&lease,&operation,"rollback",&source,&CancellationToken::new()).err().unwrap().code,"STORE_COPY_CONFLICT");
        assert_eq!(std::fs::read(output).unwrap(),b"unproven");
        assert!(!destination.join("copy.json").exists());
    }
}
