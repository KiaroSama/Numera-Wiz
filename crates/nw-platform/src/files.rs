use nw_domain::{DomainError, Result, records::FileIdentity};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

pub fn io_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::new("FILESYSTEM_ERROR", error.to_string())
}
pub fn path_text(path: &Path) -> Result<String> {
    path.to_str().map(String::from).ok_or_else(|| {
        DomainError::new(
            "NATIVE_PATH_UNAVAILABLE",
            "This development adapter cannot represent non-UTF-8 paths",
        )
    })
}
pub fn source_path(path: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(path).map_err(io_error)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(DomainError::new(
            "PATH_DENIED",
            "Source must be an admitted regular file; symbolic links are not followed",
        ));
    }
    fs::canonicalize(path).map_err(io_error)
}
pub fn output_path(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(DomainError::new(
            "PATH_DENIED",
            "Output path must be absolute",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Output must name a file"))?;
    let parent = path
        .parent()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Output parent is required"))?;
    let parent = fs::canonicalize(parent).map_err(io_error)?;
    let dest = parent.join(name);
    match fs::symlink_metadata(&dest) {
        Ok(_) => {
            return Err(DomainError::new(
                "OUTPUT_COLLISION",
                "Destination already exists; this development path is no-replace only",
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(e)),
    }
    Ok(dest)
}
/// Re-resolve a frozen destination instead of accepting a changed directory alias.
pub fn revalidate_output(expected: &str) -> Result<PathBuf> {
    let resolved = output_path(Path::new(expected))?;
    if resolved != Path::new(expected) {
        return Err(DomainError::new(
            "TARGET_CHANGED",
            "The reviewed output parent now resolves to another destination",
        ));
    }
    Ok(resolved)
}
pub fn identity(path: &Path, cancel: &CancellationToken) -> Result<FileIdentity> {
    let canonical = source_path(path)?;
    let mut file = File::open(&canonical).map_err(io_error)?;
    let before = file.metadata().map_err(io_error)?;
    if before.len() > 256 * 1024 * 1024 * 1024 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Source exceeds the development adapter's 256 GiB bound",
        ));
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut count = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Identity read cancelled"));
        }
        let n = file.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        count += n as u64;
        if count > before.len() {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Source grew while hashing",
            ));
        }
    }
    let after = file.metadata().map_err(io_error)?;
    if before.len() != after.len()
        || count != before.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(DomainError::new(
            "SOURCE_CHANGED",
            "Source changed while hashing",
        ));
    }
    Ok(FileIdentity {
        path: path_text(&canonical)?,
        bytes: count.to_string(),
        sha256: hex::encode(hash.finalize()),
    })
}
pub fn revalidate(expected: &FileIdentity, cancel: &CancellationToken) -> Result<()> {
    if &identity(Path::new(&expected.path), cancel)? != expected {
        return Err(DomainError::new(
            "SOURCE_CHANGED",
            "Reviewed bytes no longer match their identity",
        ));
    }
    Ok(())
}
/// Hold physical identity and a cheap change stamp across expensive preflight and acceptance.
/// This supplements the full content digest; metadata is never a replacement for that digest.
pub struct SourceStamp {
    path: PathBuf,
    handle: same_file::Handle,
    bytes: u64,
    modified: std::time::SystemTime,
}
impl SourceStamp {
    pub fn capture_resolved(path: &Path) -> Result<Self> {
        let canonical = source_path(path)?;
        if canonical != path {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Preflight source no longer matches its admitted canonical path",
            ));
        }
        let path = canonical;
        let handle =
            same_file::Handle::from_file(File::open(&path).map_err(io_error)?).map_err(io_error)?;
        let metadata = handle.as_file().metadata().map_err(io_error)?;
        Ok(Self {
            path,
            handle,
            bytes: metadata.len(),
            modified: metadata.modified().map_err(io_error)?,
        })
    }
    pub fn revalidate(&self) -> Result<()> {
        let changed = || {
            DomainError::new(
                "SOURCE_CHANGED",
                "Source physical identity or change stamp no longer matches preflight",
            )
        };
        let path = source_path(&self.path).map_err(|_| changed())?;
        let current = same_file::Handle::from_file(File::open(&path).map_err(|_| changed())?)
            .map_err(|_| changed())?;
        let metadata = current.as_file().metadata().map_err(|_| changed())?;
        if path != self.path
            || current != self.handle
            || metadata.len() != self.bytes
            || metadata.modified().map_err(|_| changed())? != self.modified
        {
            return Err(changed());
        }
        Ok(())
    }
}
pub fn private_root(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path).map_err(io_error)?;
    let root = fs::canonicalize(path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(root)
}
pub struct RootLock {
    _file: File,
}
impl RootLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let path = root.join("owner.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(io_error)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .map_err(|_| DomainError::new("OWNER_BUSY", "Another worker owns this data root"))?;
        Ok(Self { _file: file })
    }
}
pub fn verified_copy(
    source: &Path,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<FileIdentity> {
    let expected = identity(source, cancel)?;
    verified_copy_expected(&expected, destination, cancel)
}
pub fn verified_copy_expected(
    expected: &FileIdentity,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<FileIdentity> {
    let (staging, actual) = prepare_verified_copy(expected, destination, cancel)?;
    let output = output_path(destination)?;
    #[cfg(unix)]
    let parent = output.parent().expect("validated parent");
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Copy cancelled before publication fence",
        ));
    }
    staging
        .persist_noclobber(&output)
        .map_err(|error| DomainError::new("OUTPUT_COLLISION", error.error.to_string()))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    Ok(FileIdentity {
        path: path_text(&output)?,
        bytes: actual.bytes,
        sha256: actual.sha256,
    })
}
pub fn prepare_verified_copy(
    expected: &FileIdentity,
    destination: &Path,
    cancel: &CancellationToken,
) -> Result<(tempfile::NamedTempFile, FileIdentity)> {
    let output = output_path(destination)?;
    let parent = output.parent().expect("validated parent");
    let staging = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    copy_into_staging(expected, staging, cancel)
}

pub fn copy_into_staging(
    expected: &FileIdentity,
    mut staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
) -> Result<(tempfile::NamedTempFile, FileIdentity)> {
    let expected_bytes = expected
        .bytes
        .parse::<u64>()
        .map_err(|_| DomainError::new("INVALID_IDENTITY", "Invalid reviewed byte length"))?;
    let mut input = File::open(&expected.path).map_err(io_error)?;
    let mut buffer = vec![0; 4 * 1024 * 1024];
    let mut copied = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Copy cancelled before publication",
            ));
        }
        let n = input.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        copied = copied
            .checked_add(n as u64)
            .ok_or_else(|| DomainError::new("SOURCE_CHANGED", "Copy length overflow"))?;
        if copied > expected_bytes {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Source exceeds its reviewed byte length",
            ));
        }
        staging.write_all(&buffer[..n]).map_err(io_error)?;
    }
    staging.as_file().sync_all().map_err(io_error)?;
    let actual = identity(staging.path(), cancel)?;
    if actual.sha256 != expected.sha256 || actual.bytes != expected.bytes {
        return Err(DomainError::new(
            "VERIFICATION_FAILED",
            "Full source and staging byte digests differ",
        ));
    }
    revalidate(expected, cancel)?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Copy cancelled before publication fence",
        ));
    }
    Ok((staging, actual))
}
pub fn same_object(first: &Path, second: &Path) -> Result<bool> {
    same_file::is_same_file(first, second).map_err(io_error)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_acceptance_stamp_rejects_replacement_even_when_bytes_are_identical() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        fs::write(
            &source,
            b"Identical content does not prove the same physical source",
        )
        .unwrap();
        let canonical = source_path(&source).unwrap();
        let stamp = SourceStamp::capture_resolved(&canonical).unwrap();
        stamp.revalidate().unwrap();
        fs::rename(&source, root.path().join("retained-original.bin")).unwrap();
        fs::write(
            &source,
            b"Identical content does not prove the same physical source",
        )
        .unwrap();
        assert_eq!(stamp.revalidate().unwrap_err().code, "SOURCE_CHANGED");
        let stamp = SourceStamp::capture_resolved(&canonical).unwrap();
        fs::write(&source, b"changed length").unwrap();
        assert_eq!(stamp.revalidate().unwrap_err().code, "SOURCE_CHANGED");
    }
    #[test]
    fn reviewed_copy_rejects_replacement_before_copy_and_publishes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let output = root.path().join("output.bin");
        let cancel = CancellationToken::new();
        fs::write(&source, b"reviewed").unwrap();
        let expected = identity(&source, &cancel).unwrap();
        fs::write(&source, b"replaced").unwrap();
        assert!(verified_copy_expected(&expected, &output, &cancel).is_err());
        assert!(!output.exists());
        fs::write(&source, b"longer than the reviewed length").unwrap();
        assert_eq!(
            verified_copy_expected(&expected, &output, &cancel)
                .unwrap_err()
                .code,
            "SOURCE_CHANGED"
        );
        assert!(!output.exists());
    }
    #[test]
    fn copy_verifies_new_bytes_and_preserves_existing_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let output = root.path().join("output.bin");
        fs::write(&source, b"new media bytes").unwrap();
        let cancel = CancellationToken::new();
        let artifact = verified_copy(&source, &output, &cancel).unwrap();
        assert_eq!(artifact.bytes, "15");
        assert_eq!(fs::read(&output).unwrap(), b"new media bytes");
        fs::write(&source, b"different bytes").unwrap();
        assert!(verified_copy(&source, &output, &cancel).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"new media bytes");
    }
    #[test]
    fn cancellation_cannot_report_a_preexisting_file_as_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let output = root.path().join("output.bin");
        fs::write(&source, b"source").unwrap();
        fs::write(&output, b"old").unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(verified_copy(&source, &output, &cancel).is_err());
        assert_eq!(fs::read(output).unwrap(), b"old");
    }
    #[test]
    fn changed_output_parent_alias_is_rejected_without_creating_any_artifact() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("reviewed");
        let outside = root.path().join("outside");
        fs::create_dir(&original).unwrap();
        fs::create_dir(&outside).unwrap();
        let expected = path_text(&output_path(&original.join("result.mkv")).unwrap()).unwrap();
        fs::rename(&original, root.path().join("moved")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &original).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, &original)
            .expect("Create the hosted Windows directory-symlink fixture");

        assert_eq!(
            revalidate_output(&expected).unwrap_err().code,
            "TARGET_CHANGED"
        );
        assert!(!outside.join("result.mkv").exists());
        assert!(!root.path().join("moved/result.mkv").exists());
    }
}
