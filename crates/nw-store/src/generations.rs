//! Data-root owner lease and immutable generation activation records.
//! The service keeps this lease across ordinary work and maintenance cutover.
use crate::{
    Store, storage,
    versions::{self, GenerationFacts},
};
use nw_domain::{DomainError, Result};
use nw_platform::{
    files::{self, DirectoryStamp, RootLock},
    security,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_ACTIVATIONS: usize = 64;
const MAX_DIRECTORY_ENTRIES: usize = 256;
const MAX_ACTIVATION_BYTES: u64 = 65536;
pub const CURRENT_CAPABILITIES: &[&str] = &[
    "access.v2",
    "cleanup.v2",
    "cleanup_batch.v1",
    "receipts.v1",
    "generation_activation.v1",
    "migration.v1",
    "adoption_guard.v1",
];
/// Reader support is separate from the frozen seven-capability migration producer.
pub(crate) fn supports_capabilities(capabilities: &[String]) -> bool {
    if capabilities.is_empty() {
        return false;
    }
    if capabilities
        .iter()
        .all(|value| CURRENT_CAPABILITIES.contains(&value.as_str()))
    {
        return true;
    }
    capabilities.len() == nw_domain::maintenance::promotion::MEDIA_V1_CAPABILITIES.len()
        && capabilities
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
            == nw_domain::maintenance::promotion::MEDIA_V1_CAPABILITIES
                .iter()
                .copied()
                .collect()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationRole {
    Rollback,
    Target,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GenerationLocation {
    LegacyOne {},
    Workspace {
        operation_id: String,
        role: GenerationRole,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activation {
    pub schema_version: u32,
    pub sequence: u32,
    pub id: String,
    pub operation_id: String,
    pub root_id: String,
    pub generation: u32,
    pub store_version: u32,
    pub location: GenerationLocation,
    pub required_capabilities: Vec<String>,
    pub previous_sha256: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Selection {
    pub generation: u32,
    pub store_version: Option<u32>,
    pub root_id: Option<String>,
    pub location: GenerationLocation,
    pub activation_id: Option<String>,
}
pub(crate) struct CompatiblePrefix {
    pub root_id: String,
    pub activation_id: String,
    pub operation_id: String,
    pub sequence: u32,
    pub sha256: String,
}
pub struct RootLease {
    root: PathBuf,
    state: PathBuf,
    stamp: DirectoryStamp,
    _guard: RootLock,
    _legacy_guard: std::sync::Mutex<Option<RootLock>>,
    historical_guards: std::sync::Mutex<Vec<(PathBuf, RootLock)>>,
}
fn invalid(message: &'static str) -> DomainError {
    DomainError::new("STORE_ACTIVATION_INVALID", message)
}
fn uuid(value: &str) -> Result<()> {
    let parsed =
        uuid::Uuid::parse_str(value).map_err(|_| invalid("Activation identity is not a UUID"))?;
    if parsed.to_string() != value {
        return Err(invalid("Activation identity is not canonical"));
    }
    Ok(())
}
pub(crate) fn location_path(root: &Path, location: &GenerationLocation) -> Result<PathBuf> {
    Ok(match location {
        GenerationLocation::LegacyOne {} => root.join("state/generations/1"),
        GenerationLocation::Workspace { operation_id, role } => {
            uuid(operation_id)?;
            root.join("state/generation-workspaces")
                .join(operation_id)
                .join(match role {
                    GenerationRole::Rollback => "rollback",
                    GenerationRole::Target => "target",
                })
        }
    })
}
fn require_identity(path: &Path, root_id: &str, signing_key: &[u8]) -> Result<()> {
    use redb::ReadableDatabase;
    let database = redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(path.join("store.redb"))
        .map_err(versions::database_error)?;
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(crate::RECORDS).map_err(storage)?;
    let root = table
        .get("meta/root")
        .map_err(storage)?
        .ok_or_else(|| invalid("Generation root identity is missing"))?;
    let key = table
        .get("meta/signing-key")
        .map_err(storage)?
        .ok_or_else(|| invalid("Generation replay identity is missing"))?;
    if root.value() != root_id.as_bytes()
        || key.value().len() != signing_key.len()
        || key
            .value()
            .iter()
            .zip(signing_key)
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            != 0
    {
        return Err(DomainError::new(
            "OWNER_IDENTITY_CONFLICT",
            "Generation switch would change the root or replay signing identity",
        ));
    }
    Ok(())
}
fn same_replay_identity(source: &Path, target: &Path, root_id: &str) -> Result<()> {
    use redb::ReadableDatabase;
    let database = redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(source.join("store.redb"))
        .map_err(versions::database_error)?;
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(crate::RECORDS).map_err(storage)?;
    let source_root = table
        .get("meta/root")
        .map_err(storage)?
        .ok_or_else(|| invalid("Source root identity is missing"))?;
    if source_root.value() != root_id.as_bytes() {
        return Err(DomainError::new(
            "OWNER_IDENTITY_CONFLICT",
            "Activation would change the original root identity",
        ));
    }
    let key = table
        .get("meta/signing-key")
        .map_err(storage)?
        .ok_or_else(|| invalid("Source replay signing key is missing"))?;
    require_identity(target, root_id, key.value())
}
fn validate_activation(activation: &Activation) -> Result<()> {
    if activation.schema_version != 1 {
        return Err(DomainError::new(
            "STORE_VERSION_UNSUPPORTED",
            "This activation format requires a newer reader",
        ));
    }
    uuid(&activation.id)?;
    uuid(&activation.operation_id)?;
    uuid(&activation.root_id)?;
    if ![2, 3].contains(&activation.store_version) {
        return Err(DomainError::new(
            "STORE_VERSION_UNSUPPORTED",
            "This development reader admits store schemas2/3 only",
        ));
    }
    if activation.sequence == 0
        || activation.sequence as usize > MAX_ACTIVATIONS
        || activation.generation == 0
        || activation.generation > 16
        || activation.required_capabilities.is_empty()
        || activation.required_capabilities.len() > 64
    {
        return Err(invalid("Activation versions or bounds are invalid"));
    }
    let mut capabilities = std::collections::BTreeSet::new();
    for capability in &activation.required_capabilities {
        if !supports_capabilities(&activation.required_capabilities) {
            return Err(DomainError::new(
                "STORE_VERSION_UNSUPPORTED",
                "This generation requires an unsupported record capability",
            ));
        }
        if !capabilities.insert(capability) {
            return Err(invalid("Activation contains duplicate capabilities"));
        }
    }
    if let GenerationLocation::Workspace { operation_id, .. } = &activation.location {
        uuid(operation_id)?;
    }
    Ok(())
}
pub(crate) fn activations(root: &Path) -> Result<Vec<(Activation, String)>> {
    let directory = root.join("state/activations");
    if !directory.try_exists().map_err(files::io_error)? {
        return Ok(Vec::new());
    }
    let stamp = DirectoryStamp::capture_resolved(&directory)?;
    let mut paths = Vec::new();
    let mut visited = 0usize;
    for entry in std::fs::read_dir(&directory).map_err(files::io_error)? {
        let entry = entry.map_err(files::io_error)?;
        visited += 1;
        if visited > MAX_DIRECTORY_ENTRIES {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Activation directory exceeds its bounded entry budget",
            ));
        }
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| invalid("Unexpected activation filename encoding"))?;
        // An interrupted atomic metadata write is not an activation or deletion proof.
        // It is retained and charged to the directory budget.
        if name.starts_with(".credential-") {
            continue;
        }
        if name.len() != 15
            || !name.ends_with(".json")
            || !name[..10].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid("Unexpected activation directory member"));
        }
        paths.push(entry.path());
        if paths.len() > MAX_ACTIVATIONS {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Activation history exceeds64 retained records",
            ));
        }
    }
    paths.sort();
    let mut result: Vec<(Activation, String)> = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = security::read_private(&path, MAX_ACTIVATION_BYTES)?;
        let record: Activation = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("Activation record is not recognized"))?;
        validate_activation(&record)?;
        let expected = result
            .last()
            .map_or(1, |(previous, _)| previous.sequence + 1);
        if record.sequence != expected
            || path.file_name().and_then(|value| value.to_str())
                != Some(format!("{:010}.json", record.sequence).as_str())
        {
            return Err(invalid(
                "Activation sequence is incomplete or does not match its name",
            ));
        }
        let previous = result.last().map(|(_, hash)| hash.clone());
        if record.previous_sha256 != previous {
            return Err(invalid("Activation history hash chain differs"));
        }
        if let Some((first, _)) = result.first()
            && record.root_id != first.root_id
        {
            return Err(invalid("Activation history crosses root identities"));
        }
        let hash = hex::encode(Sha256::digest(&bytes));
        result.push((record, hash));
    }
    stamp.revalidate()?;
    Ok(result)
}
/// Resolve a selected path from the bounded immutable activation index.
/// Application integration must also enforce ownership, reader floors and recovery.
pub fn active_generation_path(root: &Path) -> Result<PathBuf> {
    let history = activations(root)?;
    let location = history
        .last()
        .map(|(record, _)| record.location.clone())
        .unwrap_or(GenerationLocation::LegacyOne {});
    location_path(root, &location)
}
impl RootLease {
    pub fn acquire(root: &Path) -> Result<Arc<Self>> {
        let root = std::fs::canonicalize(root).map_err(files::io_error)?;
        let state = security::private_directory(&root.join("state"))?;
        let guard = RootLock::acquire(&state)?;
        let stamp = DirectoryStamp::capture_resolved(&state)?;
        let lease = Arc::new(Self {
            root,
            state,
            stamp,
            _guard: guard,
            _legacy_guard: std::sync::Mutex::new(None),
            historical_guards: std::sync::Mutex::new(Vec::new()),
        });
        // Only verified accepted restoration may repair a bounded owned suffix
        // before the ordinary strict selector sees a possible reappearance gap.
        crate::restorations::repair_owned_suffix(&lease)?;
        let root = &lease.root;
        let history = activations(&root)?;
        let legacy_guard = if history.last().is_some_and(|(record, _)| {
            matches!(&record.location, GenerationLocation::Workspace { .. })
        }) {
            let legacy = root.join("state/generations/1");
            if !legacy
                .join("store.redb")
                .try_exists()
                .map_err(files::io_error)?
            {
                return Err(invalid(
                    "Legacy reader fence is missing; no empty legacy generation was initialized",
                ));
            }
            let legacy = security::private_directory(&legacy)?;
            Some(RootLock::acquire(&legacy)?)
        } else {
            None
        };
        *lease._legacy_guard.lock().map_err(storage)? = legacy_guard;
        Ok(lease)
    }
    pub(crate) fn retain_existing_generation(&self, path: &Path) -> Result<()> {
        self.revalidate()?;
        if !path.starts_with(&self.state)
            || !path.join("store.redb").try_exists().map_err(storage)?
        {
            return Err(invalid(
                "Accepted historical source is missing or outside the stable root",
            ));
        }
        let mut guards = self.historical_guards.lock().map_err(storage)?;
        if !guards.iter().any(|(held, _)| held == path) {
            guards.push((path.to_owned(), RootLock::acquire(path)?));
        }
        Ok(())
    }
    pub(crate) fn take_generation_guard(&self, path: &Path) -> Result<Option<RootLock>> {
        let mut guards = self.historical_guards.lock().map_err(storage)?;
        Ok(guards
            .iter()
            .position(|(held, _)| held == path)
            .map(|index| guards.remove(index).1))
    }
    pub(crate) fn take_historical_guards(&self) -> Result<Vec<(PathBuf, RootLock)>> {
        Ok(std::mem::take(
            &mut *self.historical_guards.lock().map_err(storage)?,
        ))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn state(&self) -> &Path {
        &self.state
    }
    pub fn revalidate(&self) -> Result<()> {
        self.stamp.revalidate()
    }
    pub fn selection(&self) -> Result<Selection> {
        self.revalidate()?;
        let history = activations(&self.root)?;
        Ok(if let Some((last, _)) = history.last() {
            Selection {
                generation: last.generation,
                store_version: Some(last.store_version),
                root_id: Some(last.root_id.clone()),
                location: last.location.clone(),
                activation_id: Some(last.id.clone()),
            }
        } else {
            Selection {
                generation: 1,
                store_version: None,
                root_id: None,
                location: GenerationLocation::LegacyOne {},
                activation_id: None,
            }
        })
    }
    pub fn path(&self, selection: &Selection) -> Result<PathBuf> {
        self.revalidate()?;
        location_path(&self.root, &selection.location)
    }
    pub fn inspect_selected(&self) -> Result<GenerationFacts> {
        let selection = self.selection()?;
        let facts = versions::inspect_closed(&self.path(&selection)?)?;
        if selection
            .root_id
            .as_ref()
            .is_some_and(|id| id != &facts.root_id)
            || selection
                .store_version
                .is_some_and(|version| version != facts.store_version)
        {
            return Err(invalid(
                "Selected generation differs from its activation envelope",
            ));
        }
        self.revalidate()?;
        Ok(facts)
    }
    pub(crate) fn compatible_prefix(&self) -> Result<CompatiblePrefix> {
        self.revalidate()?;
        let history = activations(&self.root)?;
        let (last, _) = history.last().ok_or_else(|| {
            invalid("Promotion requires a genuine nonempty compatible activation prefix")
        })?;
        if history.iter().any(|(record, _)| {
            record
                .required_capabilities
                .iter()
                .any(|capability| !CURRENT_CAPABILITIES.contains(&capability.as_str()))
        }) {
            return Err(invalid(
                "The complete prefix must be readable by the legacy reader",
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"NumeraWiz.CompatibleActivationPrefix.v1\0");
        for (record, file_hash) in &history {
            hash.update(record.sequence.to_le_bytes());
            hash.update(file_hash.as_bytes());
        }
        self.revalidate()?;
        Ok(CompatiblePrefix {
            root_id: last.root_id.clone(),
            activation_id: last.id.clone(),
            operation_id: last.operation_id.clone(),
            sequence: last.sequence,
            sha256: hex::encode(hash.finalize()),
        })
    }
    pub fn append_activation(&self, mut activation: Activation) -> Result<()> {
        self.revalidate()?;
        let history = activations(&self.root)?;
        activation.sequence = history.last().map_or(1, |(record, _)| record.sequence + 1);
        activation.previous_sha256 = history.last().map(|(_, hash)| hash.clone());
        validate_activation(&activation)?;
        if let Some((first, _)) = history.first()
            && first.root_id != activation.root_id
        {
            return Err(invalid("Cannot activate another data-root identity"));
        }
        let path = location_path(&self.root, &activation.location)?;
        let facts = versions::inspect_closed(&path)?;
        let file_capabilities = facts
            .required_capabilities
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        let declared_capabilities = activation
            .required_capabilities
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        if facts.store_version == 3 && file_capabilities != declared_capabilities {
            return Err(invalid("Store and activation required capabilities differ"));
        }
        if facts.root_id != activation.root_id || facts.store_version != activation.store_version {
            return Err(invalid(
                "Activation requires a complete matching closed generation",
            ));
        }
        let source_location = history
            .last()
            .map(|(record, _)| record.location.clone())
            .unwrap_or(GenerationLocation::LegacyOne {});
        let source = location_path(&self.root, &source_location)?;
        same_replay_identity(&source, &path, &activation.root_id)?;
        let bytes = serde_json::to_vec_pretty(&activation).map_err(storage)?;
        if bytes.len() as u64 > MAX_ACTIVATION_BYTES {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Activation exceeds64KiB",
            ));
        }
        let directory = security::private_directory(&self.state.join("activations"))?;
        self.revalidate()?;
        security::write_private(
            &directory.join(format!("{:010}.json", activation.sequence)),
            &bytes,
            false,
        )?;
        self.revalidate()?;
        Ok(())
    }
}
impl Store {
    pub fn generation_path(&self) -> Result<PathBuf> {
        self.db.path()
    }
    pub fn close_generation(&self, lease: &RootLease) -> Result<()> {
        let selected = lease.path(&lease.selection()?)?;
        if selected != self.db.path()? {
            return Err(invalid(
                "The root lease does not name this store attachment",
            ));
        }
        lease.revalidate()?;
        self.db.close_drained()
    }
    pub fn reopen_current_generation(&self, lease: &RootLease) -> Result<()> {
        let path = self.db.path()?;
        if lease.path(&lease.selection()?)? != path {
            return Err(invalid(
                "Activation changed before reopening the original generation",
            ));
        }
        let facts = versions::inspect_closed(&path)?;
        if facts.root_id != self.root_id || ![2, 3].contains(&facts.store_version) {
            return Err(invalid(
                "Reopening requires the original identity and currently admitted codec",
            ));
        }
        require_identity(&path, &self.root_id, &self.signing_key)?;
        let database = redb::Database::builder()
            .set_cache_size(8 * 1024 * 1024)
            .open(path.join("store.redb"))
            .map_err(versions::database_error)?;
        let track_adoption = crate::adoption::enabled(&database)?;
        self.db.reopen(database, track_adoption)
    }
    pub fn attach_activated_generation(&self, lease: &RootLease) -> Result<()> {
        let selection = lease.selection()?;
        let path = lease.path(&selection)?;
        let facts = lease.inspect_selected()?;
        if facts.root_id != self.root_id || ![2, 3].contains(&facts.store_version) {
            return Err(invalid(
                "The activation requires a codec not yet admitted by this store implementation",
            ));
        }
        let guard = RootLock::acquire(&path)?;
        require_identity(&path, &self.root_id, &self.signing_key)?;
        let database = redb::Database::builder()
            .set_cache_size(8 * 1024 * 1024)
            .open(path.join("store.redb"))
            .map_err(versions::database_error)?;
        let track_adoption = crate::adoption::enabled(&database)?;
        self.db
            .attach_generation(database, &path, guard, track_adoption)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn activation(root_id: &str, location: GenerationLocation) -> Activation {
        Activation {
            schema_version: 1,
            sequence: 0,
            id: uuid::Uuid::new_v4().to_string(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            root_id: root_id.into(),
            generation: 1,
            store_version: 2,
            location,
            required_capabilities: vec!["receipts.v1".into()],
            previous_sha256: None,
        }
    }
    #[test]
    fn activation_is_append_only_and_unsupported_capabilities_cannot_fall_back() {
        let temporary = tempfile::tempdir().unwrap();
        let root = security::private_directory(&temporary.path().join("data-root")).unwrap();
        let generation = security::private_directory(&root.join("state/generations/1")).unwrap();
        let store = Store::open(&generation).unwrap();
        let root_id = store.root_id().to_owned();
        drop(store);
        let lease = RootLease::acquire(&root).unwrap();
        lease
            .append_activation(activation(&root_id, GenerationLocation::LegacyOne {}))
            .unwrap();
        assert_eq!(lease.selection().unwrap().generation, 1);
        let first =
            security::read_private(&root.join("state/activations/0000000001.json"), 65536).unwrap();
        let mut second = activation(&root_id, GenerationLocation::LegacyOne {});
        second.id = uuid::Uuid::new_v4().to_string();
        lease.append_activation(second).unwrap();
        assert_eq!(
            security::read_private(&root.join("state/activations/0000000001.json"), 65536).unwrap(),
            first
        );
        let mut unsupported = activation(&root_id, GenerationLocation::LegacyOne {});
        unsupported
            .required_capabilities
            .push("future.unsupported".into());
        assert_eq!(
            lease.append_activation(unsupported).unwrap_err().code,
            "STORE_VERSION_UNSUPPORTED"
        );
        assert_eq!(activations(&root).unwrap().len(), 2);
        owned_acl_evidence_preserves_activation_selection(
            &root,
            &lease,
            &generation,
            &root_id,
            &first,
        );
    }
    fn owned_acl_evidence_preserves_activation_selection(
        root: &Path,
        lease: &RootLease,
        generation: &Path,
        root_id: &str,
        first: &[u8],
    ) {
        use std::io::Write as _;

        let directory = root.join("state/activations");
        let paths = [
            directory.join("0000000001.json"),
            directory.join("0000000002.json"),
        ];
        let bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
        assert_eq!(bytes[0].as_slice(), first);
        let records: [Activation; 2] = bytes
            .each_ref()
            .map(|bytes| serde_json::from_slice(bytes).unwrap());
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[1].sequence, 2);
        let ids = records.each_ref().map(|record| record.id.clone());
        assert_ne!(ids[0], ids[1]);
        let assert_raw_unchanged = || {
            for (path, expected) in paths.iter().zip(&bytes) {
                assert_eq!(std::fs::read(path).unwrap(), *expected);
            }
        };
        let assert_selection_unchanged = || {
            assert_eq!(
                active_generation_path(root)
                    .expect("Owned auxiliary files must preserve the active generation path"),
                generation
            );
            let selected = lease
                .selection()
                .expect("Owned auxiliary files must preserve the activation selection");
            assert_eq!(selected.generation, 1);
            assert_eq!(selected.store_version, Some(2));
            assert_eq!(selected.root_id.as_deref(), Some(root_id));
            assert_eq!(selected.activation_id.as_deref(), Some(ids[1].as_str()));
            assert_eq!(selected.location, GenerationLocation::LegacyOne {});
            assert_eq!(lease.path(&selected).unwrap(), generation);
            let history = activations(root).unwrap();
            assert_eq!(history.len(), 2);
            for ((record, hash), (id, raw)) in history.iter().zip(ids.iter().zip(&bytes)) {
                assert_eq!(&record.id, id);
                assert_eq!(record.root_id, root_id);
                assert_eq!(*hash, hex::encode(Sha256::digest(raw)));
            }
            assert_raw_unchanged();
        };
        assert_selection_unchanged();

        // Hold a real producer-named file throughout the reader's enumeration window.
        let mut evidence = tempfile::Builder::new()
            .prefix(security::ACL_EVIDENCE_PREFIX)
            .tempfile_in(&directory)
            .unwrap();
        evidence.write_all(b"Owned ACL evidence fixture").unwrap();
        assert_selection_unchanged();
        assert!(evidence.path().is_file());
        evidence.close().unwrap();
        assert_selection_unchanged();

        let unknown = tempfile::Builder::new()
            .prefix("unexpected-activation-")
            .tempfile_in(&directory)
            .unwrap();
        assert_eq!(
            active_generation_path(root).unwrap_err().code,
            "STORE_ACTIVATION_INVALID"
        );
        assert_eq!(
            lease.selection().unwrap_err().code,
            "STORE_ACTIVATION_INVALID"
        );
        assert_raw_unchanged();
        unknown.close().unwrap();
        assert_selection_unchanged();

        // Reserved auxiliary names are ignored as records but still consume the budget.
        let mut reserved = Vec::new();
        for _ in 0..MAX_DIRECTORY_ENTRIES - paths.len() {
            reserved.push(
                tempfile::Builder::new()
                    .prefix(".credential-budget-")
                    .tempfile_in(&directory)
                    .unwrap(),
            );
        }
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            MAX_DIRECTORY_ENTRIES
        );
        assert_selection_unchanged();
        let overflow = tempfile::Builder::new()
            .prefix(".credential-budget-")
            .tempfile_in(&directory)
            .unwrap();
        assert_eq!(
            active_generation_path(root).unwrap_err().code,
            "RESOURCE_LIMIT"
        );
        assert_eq!(lease.selection().unwrap_err().code, "RESOURCE_LIMIT");
        assert_raw_unchanged();
        overflow.close().unwrap();
        for entry in reserved {
            entry.close().unwrap();
        }
        assert_selection_unchanged();
    }
    #[test]
    fn a_generation_switch_preserves_the_same_store_identity_key_and_receipt_kernel() {
        let temporary = tempfile::tempdir().unwrap();
        let root = security::private_directory(&temporary.path().join("data-root")).unwrap();
        let generation = security::private_directory(&root.join("state/generations/1")).unwrap();
        let store = Store::open(&generation).unwrap();
        let root_id = store.root_id().to_owned();
        store
            .put(
                "unit/preserved",
                &serde_json::json!({"value":"9007199254740993"}),
            )
            .unwrap();
        let lease = RootLease::acquire(&root).unwrap();
        store.close_generation(&lease).unwrap();
        let operation_id = uuid::Uuid::new_v4().to_string();
        let target = root
            .join("state/generation-workspaces")
            .join(&operation_id)
            .join("target");
        security::private_directory(&target).unwrap();
        std::fs::copy(generation.join("store.redb"), target.join("store.redb")).unwrap();
        let mut event = activation(
            &root_id,
            GenerationLocation::Workspace {
                operation_id,
                role: GenerationRole::Target,
            },
        );
        event.generation = 3;
        lease.append_activation(event).unwrap();
        store.attach_activated_generation(&lease).unwrap();
        assert_eq!(store.root_id(), root_id);
        assert_eq!(
            store
                .get::<serde_json::Value>("unit/preserved")
                .unwrap()
                .unwrap()["value"],
            "9007199254740993"
        );
        assert_eq!(store.generation_path().unwrap(), target);
        assert_eq!(
            RootLock::acquire(&generation).err().unwrap().code,
            "OWNER_BUSY"
        );
        assert_eq!(RootLock::acquire(&target).err().unwrap().code, "OWNER_BUSY");
        store.close_generation(&lease).unwrap();
        store.reopen_current_generation(&lease).unwrap();
    }
    #[test]
    fn a_copy_with_the_same_root_uuid_but_another_replay_key_cannot_activate() {
        let temporary = tempfile::tempdir().unwrap();
        let root = security::private_directory(&temporary.path().join("data-root")).unwrap();
        let source = security::private_directory(&root.join("state/generations/1")).unwrap();
        let store = Store::open(&source).unwrap();
        let root_id = store.root_id().to_owned();
        drop(store);
        let operation_id = uuid::Uuid::new_v4().to_string();
        let target = security::private_directory(
            &root
                .join("state/generation-workspaces")
                .join(&operation_id)
                .join("target"),
        )
        .unwrap();
        std::fs::copy(source.join("store.redb"), target.join("store.redb")).unwrap();
        let changed = redb::Database::open(target.join("store.redb")).unwrap();
        let write = changed.begin_write().unwrap();
        {
            let mut table = write.open_table(crate::RECORDS).unwrap();
            table
                .insert("meta/signing-key", [7u8; 32].as_slice())
                .unwrap();
        }
        write.commit().unwrap();
        drop(changed);
        let lease = RootLease::acquire(&root).unwrap();
        let event = activation(
            &root_id,
            GenerationLocation::Workspace {
                operation_id,
                role: GenerationRole::Target,
            },
        );
        assert_eq!(
            lease.append_activation(event).unwrap_err().code,
            "OWNER_IDENTITY_CONFLICT"
        );
        assert!(activations(&root).unwrap().is_empty());
        assert_eq!(
            lease.selection().unwrap().location,
            GenerationLocation::LegacyOne {}
        );
    }
}
