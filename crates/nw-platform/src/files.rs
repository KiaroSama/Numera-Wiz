use nw_domain::{
    DomainError, Result,
    records::{FileIdentity, NativeObjectIdentity, NativeObjectKind},
};
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
/// Open the checked native input without following a final link or waiting on a FIFO.
pub fn open_regular_read(path: &Path) -> Result<File> {
    let canonical = source_path(path)?;
    open_regular_handle(&canonical)
}
fn open_regular_handle(path: &Path) -> Result<File> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags};
        let descriptor = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
            Mode::empty(),
        )
        .map_err(io_error)?;
        File::from(descriptor)
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(0)
            .open(path)
            .map_err(io_error)?
    };
    #[cfg(not(any(unix, windows)))]
    let file = File::open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "An opened ordinary regular file is required",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(DomainError::new(
                "INPUT_DENIED",
                "An ordinary input cannot be a reparse point",
            ));
        }
    }
    Ok(file)
}
pub(crate) fn validate_output_name(name: &std::ffi::OsStr) -> Result<()> {
    #[cfg(windows)]
    {
        windows_output_name(name.to_str().ok_or_else(|| {
            DomainError::new(
                "NATIVE_PATH_UNAVAILABLE",
                "Output filename must be representable as UTF-8",
            )
        })?)
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Ok(())
    }
}
#[cfg(any(windows, test))]
fn windows_output_name(name: &str) -> Result<()> {
    let base = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let numbered_device = ["COM", "LPT"].into_iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|suffix| {
            [
                "1", "2", "3", "4", "5", "6", "7", "8", "9", "\u{b9}", "\u{b2}", "\u{b3}",
            ]
            .contains(&suffix)
        })
    });
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|character| character < '\u{20}' || r#"<>:"/\|?*"#.contains(character))
        || matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        )
        || numbered_device
    {
        return Err(DomainError::new(
            "PATH_DENIED", "Windows output requires an ordinary filename without device, stream or normalization aliases",
        ).field("output"));
    }
    Ok(())
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
    validate_output_name(name)?;
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
    identity_observed(path, cancel, |_| Ok(()))
}
fn identity_observed(
    path: &Path,
    cancel: &CancellationToken,
    mut on_bytes: impl FnMut(u64) -> Result<()>,
) -> Result<FileIdentity> {
    on_bytes(0)?;
    let canonical = source_path(path)?;
    let mut file = open_regular_handle(&canonical)?;
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
        on_bytes(count)?;
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
/// Held identity of a reviewed output directory; child changes do not change its identity.
pub struct DirectoryStamp {
    path: PathBuf,
    file: File,
    object: NativeObjectIdentity,
}
impl DirectoryStamp {
    pub fn capture_resolved(path: &Path) -> Result<Self> {
        if !path.is_absolute() || fs::canonicalize(path).map_err(directory_changed)? != path {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Output directory must retain its canonical reviewed path",
            ));
        }
        let file = open_directory_handle(path).map_err(directory_changed)?;
        let object = native_object(&file)?;
        let stamp = Self {
            path: path.to_owned(),
            file,
            object,
        };
        stamp.revalidate()?;
        Ok(stamp)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    #[cfg(unix)]
    pub(crate) fn held_file(&self) -> &File {
        &self.file
    }
    pub fn object_identity(&self) -> Result<NativeObjectIdentity> {
        self.revalidate()?;
        Ok(self.object.clone())
    }
    pub fn require_object(&self, expected: &NativeObjectIdentity) -> Result<()> {
        self.revalidate()?;
        if &self.object != expected {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "The original reviewed output directory has been replaced",
            ));
        }
        Ok(())
    }
    pub fn revalidate(&self) -> Result<()> {
        if fs::canonicalize(&self.path).map_err(directory_changed)? != self.path
            || native_object(&self.file)? != self.object
            || native_object(&open_directory_handle(&self.path).map_err(directory_changed)?)?
                != self.object
        {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "The held output directory no longer names its original reviewed object",
            ));
        }
        Ok(())
    }
}
fn directory_changed(error: impl std::fmt::Display) -> DomainError {
    DomainError::new(
        "TARGET_CHANGED",
        format!("Reviewed output directory is unavailable: {error}"),
    )
}
fn open_directory_handle(path: &Path) -> Result<File> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags};
        File::from(
            rustix::fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io_error)?,
        )
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        // Excluding delete sharing retains the parent namespace while this lease is live.
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(0)
            .open(path)
            .map_err(io_error)?
    };
    #[cfg(not(any(unix, windows)))]
    let file = File::open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(DomainError::new(
            "TARGET_CHANGED",
            "An ordinary output directory is required",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Output directories cannot be reparse points",
            ));
        }
    }
    Ok(file)
}

/// Hold physical identity and a cheap change stamp across expensive preflight and acceptance.
/// This supplements the full content digest; metadata is never a replacement for that digest.
pub struct SourceStamp {
    path: PathBuf,
    handle: same_file::Handle,
    object: NativeObjectIdentity,
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
        let file = open_regular_handle(&canonical)?;
        Self::capture_open_resolved(&canonical, file)
    }
    /// Bind an already opened regular input to its canonical physical path.
    pub fn capture_open_resolved(path: &Path, file: File) -> Result<Self> {
        let canonical = source_path(path)?;
        if canonical != path {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Preflight source no longer matches its admitted canonical path",
            ));
        }
        let path = canonical;
        let handle = same_file::Handle::from_file(file).map_err(io_error)?;
        let metadata = handle.as_file().metadata().map_err(io_error)?;
        if !metadata.is_file() {
            return Err(DomainError::new(
                "INPUT_DENIED",
                "A regular held source is required",
            ));
        }
        let object = native_object(handle.as_file())?;
        let stamp = Self {
            path,
            handle,
            object,
            bytes: metadata.len(),
            modified: metadata.modified().map_err(io_error)?,
        };
        stamp.revalidate()?;
        Ok(stamp)
    }
    pub fn object_identity(&self) -> Result<NativeObjectIdentity> {
        self.revalidate()?;
        Ok(self.object.clone())
    }
    pub fn require_object(&self, expected: &NativeObjectIdentity) -> Result<()> {
        if &self.object_identity()? != expected {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Registered native object no longer matches its original physical identity",
            ));
        }
        Ok(())
    }
    pub(crate) fn held_file(&self) -> &File {
        self.handle.as_file()
    }
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        self.object == other.object
    }
    pub fn revalidate(&self) -> Result<()> {
        let changed = || {
            DomainError::new(
                "SOURCE_CHANGED",
                "Source physical identity or change stamp no longer matches preflight",
            )
        };
        let path = source_path(&self.path).map_err(|_| changed())?;
        let current =
            same_file::Handle::from_file(open_regular_handle(&path).map_err(|_| changed())?)
                .map_err(|_| changed())?;
        let metadata = current.as_file().metadata().map_err(|_| changed())?;
        if path != self.path
            || current != self.handle
            || native_object(current.as_file()).map_err(|_| changed())? != self.object
            || metadata.len() != self.bytes
            || metadata.modified().map_err(|_| changed())? != self.modified
        {
            return Err(changed());
        }
        Ok(())
    }
}
pub(crate) fn native_object(file: &File) -> Result<NativeObjectIdentity> {
    let metadata = file.metadata().map_err(io_error)?;
    #[cfg(unix)]
    let (kind, volume, object) = {
        use std::os::unix::fs::MetadataExt;
        (
            NativeObjectKind::UnixDeviceInode,
            metadata.dev().to_string(),
            metadata.ino().to_string(),
        )
    };
    #[cfg(windows)]
    let (kind, volume, object) = {
        let (volume, id) = windows_object(file)?;
        (
            NativeObjectKind::WindowsFileId128,
            volume.to_string(),
            hex::encode(id),
        )
    };
    #[cfg(not(any(unix, windows)))]
    return Err(DomainError::new(
        "NATIVE_PATH_UNAVAILABLE",
        "No admitted native object identity on this target",
    ));
    #[cfg(any(unix, windows))]
    {
        let created_ns =
            metadata
                .created()
                .ok()
                .map(|time| match time.duration_since(std::time::UNIX_EPOCH) {
                    Ok(duration) => duration.as_nanos().to_string(),
                    Err(error) => format!("-{}", error.duration().as_nanos()),
                });
        Ok(NativeObjectIdentity {
            schema_version: 1,
            kind,
            volume,
            object,
            created_ns,
        })
    }
}
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "One borrowed-handle Win32 FILE_ID_INFO query; its output buffer and lifetime are checked here"
)]
fn windows_object(file: &File) -> Result<(u64, [u8; 16])> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut information = FILE_ID_INFO::default();
    // SAFETY: File keeps its valid disk handle alive for this call. The initialized,
    // exclusively borrowed repr(C) FILE_ID_INFO has the exact size passed to Win32.
    // No pointer is retained, and output is consumed only after a nonzero result.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            std::ptr::from_mut(&mut information).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(DomainError::new(
            "NATIVE_IDENTITY_UNAVAILABLE",
            format!(
                "Full Windows file identity is unavailable: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok((
        information.VolumeSerialNumber,
        information.FileId.Identifier,
    ))
}
pub fn source_object(path: &Path) -> Result<NativeObjectIdentity> {
    SourceStamp::capture_resolved(&source_path(path)?)?.object_identity()
}
pub fn revalidate_source_object(path: &Path, expected: &NativeObjectIdentity) -> Result<()> {
    SourceStamp::capture_resolved(path)?.require_object(expected)
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
    let output = output_path(destination)?;
    let parent = DirectoryStamp::capture_resolved(output.parent().expect("validated parent"))?;
    let (staging, actual) = prepare_verified_copy(expected, &output, cancel)?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Copy cancelled before publication fence",
        ));
    }
    let staging = crate::publication::RetainedStaging::new(staging)?;
    staging.publish_no_replace(&parent, &output)?;
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
    staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
) -> Result<(tempfile::NamedTempFile, FileIdentity)> {
    let source_stamp = SourceStamp::capture_resolved(Path::new(&expected.path))?;
    copy_from_held(expected, source_stamp, staging, cancel)
        .map(|(staging, actual, _)| (staging, actual))
}
pub fn copy_into_staging_bound(
    expected: &FileIdentity,
    object: &NativeObjectIdentity,
    staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
) -> Result<(tempfile::NamedTempFile, FileIdentity)> {
    copy_into_staging_bound_held(expected, object, staging, cancel)
        .map(|(staging, actual, _)| (staging, actual))
}
/// Retain the exact source lease through post-verification metadata and publication.
pub fn copy_into_staging_bound_held(
    expected: &FileIdentity,
    object: &NativeObjectIdentity,
    staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
) -> Result<(tempfile::NamedTempFile, FileIdentity, SourceStamp)> {
    let source_stamp = SourceStamp::capture_resolved(Path::new(&expected.path))?;
    source_stamp.require_object(object)?;
    copy_from_held(expected, source_stamp, staging, cancel)
}
/// Copy and independent full-byte verification share one explicit operation budget.
/// Checks run between bounded filesystem calls; admitted synchronous I/O is drained.
pub fn copy_into_staging_observed(
    expected: &FileIdentity,
    object: &NativeObjectIdentity,
    staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
    deadline: nw_domain::execution::DeadlinePolicy,
    observer: &mut dyn FnMut(nw_domain::execution::StageProgress) -> Result<()>,
) -> Result<(tempfile::NamedTempFile, FileIdentity, SourceStamp)> {
    let source = SourceStamp::capture_resolved(Path::new(&expected.path))?;
    source.require_object(object)?;
    copy_from_held_observed(expected, source, staging, cancel, deadline, observer)
}
struct CopyProgress<'a> {
    started: std::time::Instant,
    sent: std::time::Instant,
    deadline: Option<std::time::Duration>,
    total: u64,
    stage: Option<nw_domain::execution::ProgressStage>,
    coalesced: u64,
    observer: &'a mut dyn FnMut(nw_domain::execution::StageProgress) -> Result<()>,
}
impl CopyProgress<'_> {
    fn report(
        &mut self,
        stage: nw_domain::execution::ProgressStage,
        completed: u64,
        force: bool,
    ) -> Result<()> {
        if self
            .deadline
            .is_some_and(|limit| self.started.elapsed() >= limit)
        {
            return Err(DomainError::new(
                "COPY_TIMEOUT",
                "Reviewed copy/byte-verification deadline expired; admitted I/O drained and no output published",
            ));
        }
        if !force
            && self.stage == Some(stage)
            && self.sent.elapsed()
                < std::time::Duration::from_millis(nw_domain::execution::PROGRESS_INTERVAL_MS)
        {
            self.coalesced = self.coalesced.saturating_add(1);
            return Ok(());
        }
        let progress = nw_domain::execution::StageProgress {
            stage,
            completed_bytes: completed.to_string(),
            total_bytes: self.total.to_string(),
            elapsed_ms: self
                .started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX))
                .to_string(),
            coalesced_updates: self.coalesced.to_string(),
        };
        progress.validate()?;
        (self.observer)(progress)?;
        if self
            .deadline
            .is_some_and(|limit| self.started.elapsed() >= limit)
        {
            return Err(DomainError::new(
                "COPY_TIMEOUT",
                "Reviewed copy deadline expired after its admitted progress boundary drained",
            ));
        }
        self.stage = Some(stage);
        self.sent = std::time::Instant::now();
        Ok(())
    }
}
fn copy_from_held(
    expected: &FileIdentity,
    source_stamp: SourceStamp,
    staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
) -> Result<(tempfile::NamedTempFile, FileIdentity, SourceStamp)> {
    copy_from_held_observed(
        expected,
        source_stamp,
        staging,
        cancel,
        nw_domain::execution::DeadlinePolicy::Disabled,
        &mut |_| Ok(()),
    )
}
fn copy_from_held_observed(
    expected: &FileIdentity,
    source_stamp: SourceStamp,
    mut staging: tempfile::NamedTempFile,
    cancel: &CancellationToken,
    deadline: nw_domain::execution::DeadlinePolicy,
    observer: &mut dyn FnMut(nw_domain::execution::StageProgress) -> Result<()>,
) -> Result<(tempfile::NamedTempFile, FileIdentity, SourceStamp)> {
    let expected_bytes = expected
        .bytes
        .parse::<u64>()
        .map_err(|_| DomainError::new("INVALID_IDENTITY", "Invalid reviewed byte length"))?;
    use nw_domain::execution::ProgressStage as Stage;
    let started = std::time::Instant::now();
    let mut progress = CopyProgress {
        started,
        sent: started,
        deadline: deadline.duration()?,
        total: expected_bytes,
        stage: None,
        coalesced: 0,
        observer,
    };
    progress.report(Stage::CopyBytes, 0, true)?;
    let mut input = source_stamp.held_file().try_clone().map_err(io_error)?;
    let mut buffer = vec![0; nw_domain::execution::COPY_BUFFER_BYTES];
    let mut copied = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Copy cancelled before publication",
            ));
        }
        progress.report(Stage::CopyBytes, copied, false)?;
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
    progress.report(Stage::CopyBytes, copied, true)?;
    staging.as_file().sync_all().map_err(io_error)?;
    progress.report(Stage::VerifyStagingBytes, 0, true)?;
    let actual = identity_observed(staging.path(), cancel, |count| {
        progress.report(Stage::VerifyStagingBytes, count, false)
    })?;
    progress.report(Stage::VerifyStagingBytes, expected_bytes, true)?;
    if actual.sha256 != expected.sha256 || actual.bytes != expected.bytes {
        return Err(DomainError::new(
            "VERIFICATION_FAILED",
            "Full source and staging byte digests differ",
        ));
    }
    source_stamp.revalidate()?;
    progress.report(Stage::VerifySourceBytes, 0, true)?;
    let source_identity = identity_observed(Path::new(&expected.path), cancel, |count| {
        progress.report(Stage::VerifySourceBytes, count, false)
    })?;
    if source_identity != *expected {
        return Err(DomainError::new(
            "SOURCE_CHANGED",
            "Source changed during observed copy verification",
        ));
    }
    progress.report(Stage::VerifySourceBytes, expected_bytes, true)?;
    source_stamp.revalidate()?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Copy cancelled before publication fence",
        ));
    }
    Ok((staging, actual, source_stamp))
}
pub fn same_object(first: &Path, second: &Path) -> Result<bool> {
    let first = SourceStamp::capture_resolved(&source_path(first)?)?;
    let second = SourceStamp::capture_resolved(&source_path(second)?)?;
    let same = first.same_as(&second);
    first.revalidate()?;
    second.revalidate()?;
    Ok(same)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_copy_measures_each_full_pass_without_publishing_early() {
        use nw_domain::execution::{DeadlinePolicy, ProgressStage};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let mut file = File::create(&source).unwrap();
        let block = vec![37; 1024 * 1024];
        for _ in 0..9 {
            file.write_all(&block).unwrap();
        }
        file.sync_all().unwrap();
        drop(file);
        let cancel = CancellationToken::new();
        let expected = identity(&source, &cancel).unwrap();
        let object = source_object(&source).unwrap();
        let staging = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        let path = staging.path().to_owned();
        let mut events = Vec::new();
        let (staging, actual, _source) = copy_into_staging_observed(
            &expected,
            &object,
            staging,
            &cancel,
            DeadlinePolicy::Finite { seconds: 120 },
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(actual.bytes, expected.bytes);
        assert_eq!(actual.sha256, expected.sha256);
        for stage in [
            ProgressStage::CopyBytes,
            ProgressStage::VerifyStagingBytes,
            ProgressStage::VerifySourceBytes,
        ] {
            let last = events.iter().rfind(|event| event.stage == stage).unwrap();
            assert_eq!(last.completed_bytes, expected.bytes);
            assert_eq!(last.total_bytes, expected.bytes);
        }
        assert!(!root.path().join("published.bin").exists());
        drop(staging);
        assert!(!path.exists());
    }
    #[test]
    fn observed_copy_drains_the_current_boundary_before_timeout_or_cancellation() {
        use nw_domain::execution::DeadlinePolicy;
        for timeout in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source.bin");
            fs::write(&source, b"bounded copy fixture").unwrap();
            let cancel = CancellationToken::new();
            let expected = identity(&source, &cancel).unwrap();
            let object = source_object(&source).unwrap();
            let staging = tempfile::NamedTempFile::new_in(root.path()).unwrap();
            let path = staging.path().to_owned();
            let mut delayed = false;
            let result = copy_into_staging_observed(
                &expected,
                &object,
                staging,
                &cancel,
                DeadlinePolicy::Finite { seconds: 1 },
                &mut |_| {
                    if !delayed {
                        delayed = true;
                        if timeout {
                            std::thread::sleep(std::time::Duration::from_millis(1100));
                        } else {
                            cancel.cancel();
                        }
                    }
                    Ok(())
                },
            );
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("Interrupted copy must not succeed"),
            };
            assert_eq!(
                error.code,
                if timeout { "COPY_TIMEOUT" } else { "CANCELLED" }
            );
            assert!(!path.exists());
            assert_eq!(fs::read(&source).unwrap(), b"bounded copy fixture");
        }
    }
    #[test]
    fn windows_output_names_reject_devices_streams_and_silently_trimmed_aliases() {
        for name in [
            "NUL.mkv",
            "con .mkv",
            "COM1.mkv",
            "LPT9",
            "COM\u{b9}.mkv",
            "output.mkv:secret",
            "output.mkv.",
            "output.mkv ",
            "out\nput.mkv",
        ] {
            assert_eq!(windows_output_name(name).unwrap_err().code, "PATH_DENIED");
        }
        for name in ["résumé.mkv", ".output.mkv", "composition.mkv", "COM10.mkv"] {
            windows_output_name(name).unwrap();
        }
    }
    #[test]
    fn reviewed_directory_identity_rejects_replacement_after_its_handle_closes() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("exports");
        fs::create_dir(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let stamp = DirectoryStamp::capture_resolved(&parent).unwrap();
        let original = stamp.object_identity().unwrap();
        drop(stamp);
        fs::rename(&parent, parent.with_file_name("original-exports")).unwrap();
        fs::create_dir(&parent).unwrap();
        let replacement = DirectoryStamp::capture_resolved(&parent).unwrap();
        assert_eq!(
            replacement.require_object(&original).unwrap_err().code,
            "TARGET_CHANGED"
        );
    }
    #[cfg(unix)]
    #[test]
    fn held_directory_detects_replacement_without_relying_on_child_metadata() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("exports");
        fs::create_dir(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let stamp = DirectoryStamp::capture_resolved(&parent).unwrap();
        fs::rename(&parent, parent.with_file_name("retained-exports")).unwrap();
        fs::create_dir(&parent).unwrap();
        assert_eq!(stamp.revalidate().unwrap_err().code, "TARGET_CHANGED");
    }
    #[cfg(windows)]
    #[test]
    fn held_windows_directory_excludes_namespace_delete_until_its_lease_ends() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("exports");
        fs::create_dir(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        let stamp = DirectoryStamp::capture_resolved(&parent).unwrap();
        let moved = parent.with_file_name("retained-exports");
        assert!(fs::rename(&parent, &moved).is_err());
        stamp.revalidate().unwrap();
        drop(stamp);
        fs::rename(&parent, &moved).unwrap();
    }
    #[test]
    fn changing_directory_children_keeps_the_reviewed_parent_identity() {
        let root = tempfile::tempdir().unwrap();
        let parent = fs::canonicalize(root.path()).unwrap();
        let stamp = DirectoryStamp::capture_resolved(&parent).unwrap();
        let original = stamp.object_identity().unwrap();
        fs::write(parent.join("another-output.mkv"), b"independent child").unwrap();
        stamp.require_object(&original).unwrap();
        fs::remove_file(parent.join("another-output.mkv")).unwrap();
        stamp.require_object(&original).unwrap();
    }
    #[test]
    fn original_object_survives_handle_drop_but_rejects_identical_replacement() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let retained = root.path().join("original.bin");
        std::fs::write(&source, b"original identity bytes").unwrap();
        let canonical = source_path(&source).unwrap();
        let identity = identity(&canonical, &CancellationToken::new()).unwrap();
        let original = source_object(&canonical).unwrap();
        revalidate_source_object(&canonical, &original).unwrap();
        std::fs::rename(&canonical, &retained).unwrap();
        std::fs::copy(&retained, &canonical).unwrap();
        revalidate(&identity, &CancellationToken::new()).unwrap();
        assert_ne!(source_object(&canonical).unwrap(), original);
        assert_eq!(
            revalidate_source_object(&canonical, &original)
                .unwrap_err()
                .code,
            "SOURCE_CHANGED"
        );
        assert_eq!(source_object(&retained).unwrap(), original);
    }
    #[test]
    fn native_object_identity_is_shared_by_hard_links_without_granting_a_path() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let alias = root.path().join("alias.bin");
        std::fs::write(&source, b"same physical object").unwrap();
        std::fs::hard_link(&source, &alias).unwrap();
        assert_eq!(
            source_object(&source).unwrap(),
            source_object(&alias).unwrap()
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn opened_input_rejects_fifo_without_waiting_for_a_writer() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input");
        let fixture_tool = [Path::new("/usr/bin/mkfifo"), Path::new("/bin/mkfifo")]
            .into_iter()
            .find(|path| path.is_file())
            .expect("Hosted POSIX FIFO fixture tool");
        crate::process::run(
            fixture_tool,
            &[path_text(&source).unwrap()],
            std::time::Duration::from_secs(5),
            1024,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let started = std::time::Instant::now();
        assert!(matches!(open_regular_handle(&source), Err(error) if error.code == "INPUT_DENIED"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
    #[cfg(unix)]
    #[test]
    fn opened_input_does_not_follow_a_replaced_final_symlink() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input");
        let original = root.path().join("original");
        std::fs::write(&original, b"held").unwrap();
        std::os::unix::fs::symlink(&original, &source).unwrap();
        assert!(open_regular_handle(&source).is_err());
    }
    #[test]
    fn opened_input_rejects_a_directory() {
        let root = tempfile::tempdir().unwrap();
        assert!(open_regular_handle(root.path()).is_err());
    }
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
    fn a_preopened_original_cannot_be_bound_to_a_replacement_at_its_old_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.bin");
        fs::write(&path, b"same bytes, distinct physical files").unwrap();
        let canonical = source_path(&path).unwrap();
        let original = File::open(&canonical).unwrap();
        fs::rename(&canonical, root.path().join("retained.bin")).unwrap();
        fs::write(&canonical, b"same bytes, distinct physical files").unwrap();
        assert!(
            matches!(SourceStamp::capture_open_resolved(&canonical, original), Err(error) if error.code == "SOURCE_CHANGED")
        );
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
