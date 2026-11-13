//! Opaque, principal-scoped streamed intake; a browser filename is never a native path.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadInput {
    pub display_name: String,
    pub bytes: String,
    pub sha256: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_timeout: Option<crate::execution::DeadlinePolicy>,
    pub path: Option<String>,
    pub upload_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UploadTicket {
    pub upload_id: String,
    pub expected_bytes: String,
    pub expires_at: String,
}

/// A deliberately small first intake snapshot fits one ordinary receipt transaction.
pub const MAX_FOLDER_ENTRIES: usize = 32;
pub const MAX_FOLDER_VISITS: usize = 4096;
pub const MAX_FOLDER_DIRECTORIES: usize = 128;
pub const MAX_FOLDER_DEPTH: usize = 16;
pub const MAX_FOLDER_BYTES: usize = 224 * 1024;
pub const MAX_FOLDER_SOURCE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_FOLDER_DIRECTORY_NAME_BYTES: usize = 512 * 1024;
pub const MAX_FOLDER_SECONDS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderIntakeLimits {
    pub manifest_entries: u32,
    pub filesystem_entries: u32,
    pub directories: u32,
    pub depth: u32,
    pub aggregate_source_bytes: String,
    pub directory_name_bytes: u32,
    /// Checked before the next entry; an active probe retains its explicit probe policy.
    pub seconds_before_next_entry: u64,
}
impl Default for FolderIntakeLimits {
    fn default() -> Self {
        Self {
            manifest_entries: MAX_FOLDER_ENTRIES as u32,
            filesystem_entries: MAX_FOLDER_VISITS as u32,
            directories: MAX_FOLDER_DIRECTORIES as u32,
            depth: MAX_FOLDER_DEPTH as u32,
            aggregate_source_bytes: MAX_FOLDER_SOURCE_BYTES.to_string(),
            directory_name_bytes: MAX_FOLDER_DIRECTORY_NAME_BYTES as u32,
            seconds_before_next_entry: MAX_FOLDER_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompanionIntakePolicy {
    /// Retain a verified byte identity; this does not schedule or publish a copy.
    Inventory,
    Exclude,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterFolderInput {
    pub path: String,
    pub recursive: bool,
    pub exclude_relative: Vec<String>,
    pub companion_policy: CompanionIntakePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_timeout: Option<crate::execution::DeadlinePolicy>,
}
impl RegisterFolderInput {
    pub fn validate(&self) -> crate::Result<()> {
        let invalid = || {
            crate::DomainError::new(
                "VALIDATION_ERROR",
                "Folder intake needs a bounded native root and at most 32 distinct normalized relative exclusions",
            )
        };
        if self.path.is_empty() || self.path.len() > 4096 || self.exclude_relative.len() > 32 {
            return Err(invalid());
        }
        let mut exclusions = std::collections::BTreeSet::new();
        for path in &self.exclude_relative {
            if !relative_path(path) || path.contains('\\') || !exclusions.insert(path) {
                return Err(invalid().field("exclude_relative"));
            }
        }
        if let Some(timeout) = self.probe_timeout {
            timeout.duration()?;
        }
        Ok(())
    }
}
fn relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FolderEntryKind {
    Media,
    Companion,
    Directory,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FolderEntryOutcome {
    Registered,
    ProbeFailed,
    CompanionInventoried,
    CompanionExcluded,
    Excluded,
    Rejected,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderEntry {
    pub index: u32,
    pub relative_path: String,
    pub kind: FolderEntryKind,
    pub outcome: FolderEntryOutcome,
    pub identity: Option<crate::records::FileIdentity>,
    pub source_object: Option<crate::records::NativeObjectIdentity>,
    pub asset_id: Option<String>,
    pub error: Option<crate::DomainError>,
}
/// Immutable observations, including failed probes. This is not an execution plan.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderSnapshot {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub data_root: String,
    pub hash: String,
    pub created_at: String,
    pub root: String,
    pub root_object: crate::records::NativeObjectIdentity,
    pub input: RegisterFolderInput,
    pub limits: FolderIntakeLimits,
    pub complete: bool,
    pub visited_entries: u32,
    pub visited_directories: u32,
    pub scan_error: Option<crate::DomainError>,
    pub entries: Vec<FolderEntry>,
}
impl FolderSnapshot {
    pub fn digest(&self) -> crate::Result<String> {
        use sha2::{Digest, Sha256};
        self.input.validate()?;
        let invalid =
            || crate::DomainError::new("INVALID_RECORD", "Invalid folder intake snapshot");
        if self.schema_version != 1
            || self.id.is_empty()
            || self.id.len() > 128
            || self.creator.is_empty()
            || self.data_root.is_empty()
            || self.root.is_empty()
            || self.root.len() > 4096
            || self.limits != FolderIntakeLimits::default()
            || self.entries.len() > MAX_FOLDER_ENTRIES
            || self.visited_entries as usize > MAX_FOLDER_VISITS
            || self.visited_directories as usize > MAX_FOLDER_DIRECTORIES
            || !self
                .created_at
                .parse::<u64>()
                .is_ok_and(|time| time.to_string() == self.created_at)
            || (self.complete && self.scan_error.is_some())
        {
            return Err(invalid());
        }
        let mut previous: Option<&str> = None;
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.index as usize != index
                || !relative_path(&entry.relative_path)
                || previous.is_some_and(|path| path >= entry.relative_path.as_str())
            {
                return Err(invalid());
            }
            previous = Some(&entry.relative_path);
            match entry.outcome {
                FolderEntryOutcome::Registered
                    if entry.kind == FolderEntryKind::Media
                        && entry.identity.is_some()
                        && entry.source_object.is_some()
                        && entry.asset_id.is_some()
                        && entry.error.is_none() => {}
                FolderEntryOutcome::CompanionInventoried
                    if entry.kind == FolderEntryKind::Companion
                        && entry.identity.is_some()
                        && entry.source_object.is_some()
                        && entry.asset_id.is_none()
                        && entry.error.is_none() => {}
                FolderEntryOutcome::ProbeFailed
                    if entry.kind == FolderEntryKind::Media
                        && entry.asset_id.is_none()
                        && entry.error.is_some() => {}
                FolderEntryOutcome::Rejected
                    if entry.asset_id.is_none() && entry.error.is_some() => {}
                FolderEntryOutcome::CompanionExcluded | FolderEntryOutcome::Excluded
                    if entry.asset_id.is_none()
                        && entry.identity.is_none()
                        && entry.source_object.is_none()
                        && entry.error.is_none() => {}
                _ => return Err(invalid()),
            }
        }
        let mut unhashed = self.clone();
        unhashed.hash.clear();
        let bytes = serde_json::to_vec(&unhashed).map_err(|_| invalid())?;
        if bytes.len().saturating_add(64) > MAX_FOLDER_BYTES {
            return Err(crate::DomainError::new(
                "RESOURCE_LIMIT",
                "Folder manifest exceeds its bounded record budget",
            ));
        }
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    pub fn validate_hash(&self) -> crate::Result<()> {
        if self.digest()? != self.hash {
            return Err(crate::DomainError::new(
                "INVALID_RECORD",
                "Folder snapshot differs from its retained digest",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderEntryReview {
    pub index: u32,
    pub relative_path: String,
    pub kind: FolderEntryKind,
    pub outcome: FolderEntryOutcome,
    pub identity: Option<crate::records::FileIdentity>,
    pub source_object: Option<crate::records::NativeObjectIdentity>,
    pub asset_id: Option<String>,
    pub error: Option<crate::views::ErrorView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderReview {
    pub schema_version: u32,
    pub id: String,
    /// Digest of the immutable retained snapshot, before principal-specific diagnostic redaction.
    pub hash: String,
    pub created_at: String,
    pub root: String,
    pub input: RegisterFolderInput,
    pub limits: FolderIntakeLimits,
    pub complete: bool,
    pub visited_entries: u32,
    pub visited_directories: u32,
    pub scan_error: Option<crate::views::ErrorView>,
    pub entries: Vec<FolderEntryReview>,
}
impl FolderReview {
    pub fn new(snapshot: &FolderSnapshot, diagnostics: bool) -> Self {
        let error_view = |error: &crate::DomainError| crate::views::ErrorView {
            code: error.code.clone(),
            message: diagnostics.then(|| error.message.clone()),
            retryable: error.retryable,
        };
        Self {
            schema_version: snapshot.schema_version,
            id: snapshot.id.clone(),
            hash: snapshot.hash.clone(),
            created_at: snapshot.created_at.clone(),
            root: snapshot.root.clone(),
            input: snapshot.input.clone(),
            limits: snapshot.limits.clone(),
            complete: snapshot.complete,
            visited_entries: snapshot.visited_entries,
            visited_directories: snapshot.visited_directories,
            scan_error: snapshot.scan_error.as_ref().map(error_view),
            entries: snapshot
                .entries
                .iter()
                .map(|entry| FolderEntryReview {
                    index: entry.index,
                    relative_path: entry.relative_path.clone(),
                    kind: entry.kind,
                    outcome: entry.outcome,
                    identity: entry.identity.clone(),
                    source_object: entry.source_object.clone(),
                    asset_id: entry.asset_id.clone(),
                    error: entry.error.as_ref().map(error_view),
                })
                .collect(),
        }
    }
}
