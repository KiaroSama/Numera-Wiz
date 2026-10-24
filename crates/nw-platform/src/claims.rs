//! Bounded user-scoped destination publication coordination across data roots.
use crate::{files, security};
use nw_domain::{DomainError, Result, records::{NativeObjectIdentity, NativeObjectKind}};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const MAX_DESTINATIONS: usize = 64;
const CLAIM_DEADLINE: Duration = Duration::from_secs(30);
static PREPARE_SLOTS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
const MARKER: &[u8] = b"Numera Wiz user coordination namespace version 1\n";

pub struct Destination {
    path: PathBuf,
    parent: files::DirectoryStamp,
    object: NativeObjectIdentity,
}
impl Destination {
    pub fn reviewed(path: &Path, expected_parent: &NativeObjectIdentity) -> Result<Self> {
        if !path.is_absolute() || path.file_name().and_then(|name| name.to_str()).is_none() {
            return Err(DomainError::new("PATH_DENIED", "A native absolute output filename is required"));
        }
        files::validate_output_name(path.file_name().expect("Validated filename"))?;
        let parent = files::DirectoryStamp::capture_resolved(
            path.parent().ok_or_else(|| DomainError::new("PATH_DENIED", "Output parent required"))?,
        )?;
        parent.require_object(expected_parent)?;
        Ok(Self { path: path.to_owned(), parent, object: expected_parent.clone() })
    }
    fn parent_key(&self) -> [u8; 32] {
        let kind = match &self.object.kind {
            NativeObjectKind::UnixDeviceInode => "unix-device-inode",
            NativeObjectKind::WindowsFileId128 => "windows-file-id-128",
        };
        let mut hash = Sha256::new();
        hash.update(b"numera-parent-v1\0");
        for field in [
            kind,
            self.object.volume.as_str(),
            self.object.object.as_str(),
            self.object.created_ns.as_deref().unwrap_or(""),
        ] {
            hash.update((field.len() as u64).to_le_bytes());
            hash.update(field.as_bytes());
        }
        hash.finalize().into()
    }
    fn lock_names(&self) -> [String; 2] {
        let parent = self.parent_key();
        let mut hash = Sha256::new();
        hash.update(b"numera-entry-v1\0");
        hash.update(parent);
        hash.update(self.path.file_name().expect("Validated filename").as_encoded_bytes());
        let entry: [u8; 32] = hash.finalize().into();
        // The parent stripe conservatively covers every case/normalization alias.
        // Entry stripes retain the exact native-name key without unbounded lock files.
        [format!("entry-{:02x}.lock", entry[0]), format!("parent-{:02x}.lock", parent[0])]
    }
}
pub struct DestinationClaims {
    _locks: Vec<File>,
    destinations: Vec<Destination>,
}
impl DestinationClaims {
    /// Only call after rendering/verification, before final target checks and the fence.
    pub async fn acquire(
        destinations: Vec<Destination>,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let started = Instant::now();
        let slots = PREPARE_SLOTS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(8))).clone();
        let permit = tokio::select! {
            _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Publication claim preparation cancelled")),
            result = tokio::time::timeout(CLAIM_DEADLINE, slots.acquire_owned()) => {
                result.map_err(|_| busy("Publication preparation capacity deadline exceeded"))?
                    .map_err(claim_error)?
            }
        };
        let preparation = tokio::task::spawn_blocking(move || {
            // A stalled OS operation retains its slot instead of spawning unbounded replacements.
            let _permit = permit;
            let files = prepare(&destinations)?;
            Ok::<_, DomainError>((destinations, files))
        });
        let (destinations, files) = tokio::select! {
            _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Publication claim preparation cancelled")),
            result = tokio::time::timeout(CLAIM_DEADLINE.saturating_sub(started.elapsed()), preparation) => {
                result.map_err(|_| busy("Publication preparation deadline exceeded"))?
                    .map_err(claim_error)??
            }
        };
        let mut locks = Vec::with_capacity(files.len());
        for file in files {
            loop {
                if started.elapsed() >= CLAIM_DEADLINE {
                    return Err(busy("Destination publication claim deadline exceeded"));
                }
                if cancel.is_cancelled() {
                    return Err(DomainError::new("CANCELLED", "Cancelled while waiting for destination publication"));
                }
                match fs2::FileExt::try_lock_exclusive(&file) {
                    Ok(()) => break,
                    Err(error) if contended(&error) => {}
                    Err(error) => return Err(claim_error(error)),
                }
                if started.elapsed() >= CLAIM_DEADLINE {
                    return Err(busy("Destination publication claim deadline exceeded"));
                }
                tokio::select! {
                    _ = cancel.cancelled() => {
                        return Err(DomainError::new("CANCELLED", "Cancelled while waiting for destination publication"));
                    }
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {}
                }
            }
            locks.push(file);
        }
        let claims = Self { _locks: locks, destinations };
        claims.revalidate()?;
        Ok(claims)
    }
    /// Recovery refuses contention before changing any publication fact.
    pub fn try_acquire(destinations: Vec<Destination>) -> Result<Self> {
        let files = prepare(&destinations)?;
        let mut locks = Vec::with_capacity(files.len());
        for file in files {
            fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
                if contended(&error) {
                    busy("Another owner holds the destination publication claim")
                } else {
                    claim_error(error)
                }
            })?;
            locks.push(file);
        }
        let claims = Self { _locks: locks, destinations };
        claims.revalidate()?;
        Ok(claims)
    }
    pub fn revalidate(&self) -> Result<()> {
        for destination in &self.destinations {
            destination.parent.require_object(&destination.object)?;
        }
        Ok(())
    }
}
fn contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error().is_some_and(|code| Some(code) == fs2::lock_contended_error().raw_os_error())
}
fn busy(message: &str) -> DomainError {
    let mut error = DomainError::new("PUBLICATION_BUSY", message);
    error.retryable = true;
    error
}
fn claim_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::new("PUBLICATION_CLAIM_UNAVAILABLE", error.to_string())
}
fn prepare(destinations: &[Destination]) -> Result<Vec<File>> {
    if destinations.is_empty() || destinations.len() > MAX_DESTINATIONS {
        return Err(DomainError::new("RESOURCE_LIMIT", "Publication requires 1-64 destinations"));
    }
    let root = namespace()?;
    let mut names = BTreeSet::new();
    for destination in destinations {
        destination.parent.require_object(&destination.object)?;
        names.extend(destination.lock_names());
    }
    let mut files = Vec::with_capacity(names.len());
    // All outputs acquire deduplicated stripes in one deterministic global order.
    for name in names {
        files.push(open_lock(&root.join(name))?);
    }
    Ok(files)
}
fn open_lock(path: &Path) -> Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(claim_error(error)),
        }
        // Lock files are never removed or truncated: that would permit inode ABA.
        security::verify_private_file(path, 0)?;
        files::open_regular_read(path)
}
fn namespace() -> Result<PathBuf> {
    #[cfg(unix)]
    let root = {
        let uid = rustix::process::geteuid().as_raw();
        // Ignore HOME/TMPDIR overrides so separate portable copies use the same namespace.
        security::private_directory(&PathBuf::from(format!("/tmp/numera-wiz-user-{uid}")))?
            .join("coordination-v1")
    };
    #[cfg(windows)]
    let root = windows_local_data()?.join("NumeraWizCoordination-v1");
    #[cfg(not(any(unix, windows)))]
    return Err(DomainError::new("PUBLICATION_CLAIM_UNAVAILABLE", "No user coordination namespace on this platform"));
    #[cfg(any(unix, windows))]
    {
        let root = security::private_directory(&root)?;
        let marker = root.join("namespace-v1");
        if !marker.try_exists().map_err(files::io_error)? {
            let gate = open_lock(&root.join("namespace.guard"))?;
            let started = Instant::now();
            loop {
                match fs2::FileExt::try_lock_exclusive(&gate) {
                    Ok(()) => break,
                    Err(error) if contended(&error) && started.elapsed() < Duration::from_secs(5) => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(error) => return Err(claim_error(error)),
                }
            }
            if !marker.try_exists().map_err(files::io_error)?
                && std::fs::read_dir(&root).map_err(files::io_error)?
                    .any(|entry| entry.map_or(true, |entry| entry.file_name() != "namespace.guard"))
            {
                return Err(claim_error("Unrecognized contents in an uninitialized coordination namespace"));
            }
            match security::write_private(&marker, MARKER, false) {
                Ok(()) => {}
                Err(error) if marker.try_exists().unwrap_or(false) => {
                    tracing::debug!(code = %error.code, "Another owner initialized the coordination marker");
                }
                Err(error) => return Err(error),
            }
        }
        if security::read_private(&marker, 256)? != MARKER {
            return Err(DomainError::new("PUBLICATION_CLAIM_UNAVAILABLE", "Unrecognized coordination namespace marker"));
        }
        Ok(root)
    }
}
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Audited Windows known-folder/COM lifetime; bounded UTF-16 copy and balanced OS allocation/initialization"
)]
fn windows_local_data() -> Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED},
        UI::Shell::{SHGetKnownFolderPath, FOLDERID_LocalAppData},
    };
    // SAFETY: No reserved context is passed. A successful COM initialization is
    // balanced below; an existing different apartment remains owned by its caller.
    let initialized = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if initialized < 0 && initialized != RPC_E_CHANGED_MODE {
        return Err(claim_error(format!("Cannot initialize known-folder lookup: {initialized:#x}")));
    }
    let mut allocated = std::ptr::null_mut();
    // SAFETY: The static folder GUID and exclusive PWSTR output have valid lifetimes.
    // A null token selects the current OS user; no environment path is trusted.
    let status = unsafe {
        SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut allocated)
    };
    let result = (|| {
        if status < 0 || allocated.is_null() {
            return Err(claim_error(format!("Current-user known folder unavailable: {status:#x}")));
        }
        for length in 0..32768 {
            // SAFETY: Windows returns an allocated NUL-terminated UTF-16 string.
            // Reading stops at its terminator; the explicit cap bounds work.
            if unsafe { *allocated.add(length) } == 0 {
                // SAFETY: Every element up to the observed terminator was within that
                // same live OS allocation. Copying finishes before the allocation is freed.
                let units = unsafe { std::slice::from_raw_parts(allocated, length) };
                return Ok(PathBuf::from(std::ffi::OsString::from_wide(units)));
            }
        }
        Err(claim_error("Current-user known folder exceeds its path bound"))
    })();
    // SAFETY: The API requires CoTaskMemFree even on failed folder lookup. NULL
    // is allowed. No borrowed slice or pointer survives this point.
    unsafe { CoTaskMemFree(allocated.cast()) };
    if initialized >= 0 {
        // SAFETY: Balance this function's successful initialization on the same thread.
        unsafe { CoUninitialize() };
    }
    result
}
