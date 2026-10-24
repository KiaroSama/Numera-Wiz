//! Held-directory, same-filesystem, atomic no-replace publication.
use crate::files::{DirectoryStamp, io_error, native_object};
use nw_domain::{DomainError, Result, records::NativeObjectIdentity};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    path::{Component, Path, PathBuf},
};

/// Retains a journal-owned staging name on every exit, including after a fence.
/// Retention/GC decides when its payload and publication proof may be removed.
pub struct RetainedStaging {
    file: File,
    directory: DirectoryStamp,
    name: OsString,
    path: PathBuf,
    object: NativeObjectIdentity,
}
impl RetainedStaging {
    pub fn new(staging: tempfile::NamedTempFile) -> Result<Self> {
        let object = native_object(staging.as_file())?;
        let (file, path) = staging.keep().map_err(|error| io_error(error.error))?;
        let parent = path.parent().ok_or_else(|| {
            DomainError::new("PATH_DENIED", "An owned staging directory is required")
        })?;
        let name = filename(&path)?.to_owned();
        let directory = DirectoryStamp::capture_resolved(parent)?;
        let retained = Self { file, directory, name, path, object };
        retained.revalidate()?;
        Ok(retained)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn revalidate(&self) -> Result<()> {
        self.directory.revalidate()?;
        if native_object(&self.file)? != self.object
            || native_object(&open_relative(&self.directory, &self.name)?)? != self.object
        {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "The retained staging name no longer identifies the verified file",
            ));
        }
        Ok(())
    }
    /// Creates the durable proof inside the retained, private staging directory.
    pub fn create_proof(&self, name: &OsStr) -> Result<PathBuf> {
        filename(Path::new(name))?;
        self.link_into(&self.directory, name, || Ok(()))?;
        Ok(self.directory.path().join(name))
    }
    /// Atomically adds an absent output name; an existing entry is never replaced.
    /// Payload and proof remain links to the same verified object for recovery.
    pub fn publish_no_replace(&self, parent: &DirectoryStamp, output: &Path) -> Result<()> {
        if output.parent() != Some(parent.path()) {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Output does not belong to its held reviewed directory",
            ));
        }
        self.link_into(parent, filename(output)?, || Ok(()))
    }
    fn link_into(
        &self,
        parent: &DirectoryStamp,
        name: &OsStr,
        before_link: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let path = Path::new(name);
        let mut components = path.components();
        if !matches!(components.next(), Some(Component::Normal(_)))
            || components.next().is_some()
            || filename(path)? != name
        {
            return Err(DomainError::new("PATH_DENIED", "Publication requires one ordinary filename"));
        }
        crate::files::validate_output_name(name)?;
        self.revalidate()?;
        parent.revalidate()?;
        self.file.sync_all().map_err(io_error)?;
        before_link()?;
        // Both operands are single names relative to retained directory handles.
        // No pathname reopening or check-then-replace fallback follows this point.
        #[cfg(unix)]
        rustix::fs::linkat(
            self.directory.held_file(),
            self.name.as_os_str(),
            parent.held_file(),
            name,
            rustix::fs::AtFlags::empty(),
        )
        .map_err(|error| publication_error(std::io::Error::from(error), parent, name))?;
        // Windows directory handles exclude delete sharing. The existing native
        // no-replace hard-link primitive cannot redirect either held parent.
        #[cfg(not(unix))]
        std::fs::hard_link(&self.path, parent.path().join(name))
            .map_err(|error| publication_error(error, parent, name))?;
        if native_object(&open_relative(parent, name)?)? != self.object {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Publication does not identify the verified staging object",
            ));
        }
        #[cfg(unix)]
        parent.held_file().sync_all().map_err(io_error)?;
        self.revalidate()?;
        parent.revalidate()?;
        Ok(())
    }
}
fn publication_error(error: std::io::Error, parent: &DirectoryStamp, name: &OsStr) -> DomainError {
    // Some filesystems report an existing directory as access denied. Inspect
    // only to classify the failed atomic operation; never replace that entry.
    #[cfg(unix)]
    let exists = rustix::fs::statat(
        parent.held_file(),
        name,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    ).is_ok();
    #[cfg(not(unix))]
    let exists = std::fs::symlink_metadata(parent.path().join(name)).is_ok();
    if error.kind() == std::io::ErrorKind::AlreadyExists || exists {
        DomainError::new("OUTPUT_COLLISION", "Publication destination already exists")
    } else if matches!(
        error.kind(),
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded
    ) {
        DomainError::new("OUTPUT_STORAGE_FULL", "Publication filesystem has insufficient storage or quota")
    } else if matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    ) {
        DomainError::new("PUBLICATION_DENIED", "Publication filesystem denied the atomic operation")
    } else {
        DomainError::new(
            "PUBLICATION_UNAVAILABLE",
            format!("Atomic same-filesystem no-replace publication failed: {error}"),
        )
    }
}
fn filename(path: &Path) -> Result<&OsStr> {
    let name = path.file_name().ok_or_else(|| {
        DomainError::new("PATH_DENIED", "An ordinary publication filename is required")
    })?;
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DomainError::new("PATH_DENIED", "Publication requires one ordinary filename"));
    }
    Ok(name)
}
fn open_relative(directory: &DirectoryStamp, name: &OsStr) -> Result<File> {
    #[cfg(unix)]
    let file = File::from(
        rustix::fs::openat(
            directory.held_file(),
            name,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(io_error)?,
    );
    #[cfg(not(unix))]
    let file = crate::files::open_regular_read(&directory.path().join(name))?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(DomainError::new("TARGET_CHANGED", "Publication requires an ordinary file"));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn retained(root: &Path) -> RetainedStaging {
        let mut file = tempfile::NamedTempFile::new_in(root).unwrap();
        file.write_all(b"independently verified bytes").unwrap();
        file.as_file().sync_all().unwrap();
        RetainedStaging::new(file).unwrap()
    }
    #[test]
    fn publication_and_proof_are_the_verified_object_and_never_replace_any_entry() {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let file = retained(&root);
        let parent = DirectoryStamp::capture_resolved(&root).unwrap();
        let proof = file.create_proof(OsStr::new("proof")).unwrap();
        let output = root.join("résumé.mkv");
        file.publish_no_replace(&parent, &output).unwrap();
        assert!(crate::files::same_object(&proof, &output).unwrap());
        assert!(crate::files::same_object(file.path(), &output).unwrap());
        assert_eq!(std::fs::read(&output).unwrap(), b"independently verified bytes");
        let other = retained(&root);
        assert_eq!(other.publish_no_replace(&parent, &output).unwrap_err().code, "OUTPUT_COLLISION");
        assert_eq!(std::fs::read(&output).unwrap(), b"independently verified bytes");
        let directory = root.join("directory.mkv");
        std::fs::create_dir(&directory).unwrap();
        assert_eq!(other.publish_no_replace(&parent, &directory).unwrap_err().code, "OUTPUT_COLLISION");
        assert!(directory.is_dir());
    }
    #[test]
    fn proof_names_cannot_escape_the_owned_directory() {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let file = retained(&root);
        for name in ["../outside", "nested/proof", ".", "..", ""] {
            assert_eq!(file.create_proof(OsStr::new(name)).unwrap_err().code, "PATH_DENIED");
        }
        assert!(file.path().exists());
    }
    #[cfg(unix)]
    #[test]
    fn final_namespace_swap_cannot_publish_into_the_replacement_or_symlink_target() {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let staging = root.join("staging");
        let output = root.join("exports");
        let moved = root.join("original-exports");
        let outside = root.join("outside");
        for path in [&staging, &output, &outside] {
            std::fs::create_dir(path).unwrap();
        }
        let file = retained(&staging);
        let parent = DirectoryStamp::capture_resolved(&output).unwrap();
        let result = file.link_into(&parent, OsStr::new("result.mkv"), || {
            std::fs::rename(&output, &moved).map_err(io_error)?;
            std::os::unix::fs::symlink(&outside, &output).map_err(io_error)?;
            Ok(())
        });
        assert_eq!(result.unwrap_err().code, "TARGET_CHANGED");
        assert!(!outside.join("result.mkv").exists());
        assert_eq!(std::fs::read(moved.join("result.mkv")).unwrap(), b"independently verified bytes");
        assert!(file.path().exists());
    }
    #[cfg(unix)]
    #[test]
    fn a_final_symlink_collision_is_preserved_without_touching_its_target() {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let outside = root.join("outside");
        std::fs::write(&outside, b"unrelated bytes").unwrap();
        let output = root.join("output.mkv");
        std::os::unix::fs::symlink(&outside, &output).unwrap();
        let file = retained(&root);
        let parent = DirectoryStamp::capture_resolved(&root).unwrap();
        assert_eq!(file.publish_no_replace(&parent, &output).unwrap_err().code, "OUTPUT_COLLISION");
        assert!(std::fs::symlink_metadata(&output).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&outside).unwrap(), b"unrelated bytes");
    }
}
