//! Held-object restoration primitives. Database overlay durability is owned by redb.
//! Completion records observed namespace removal; it is not a universal power-loss claim.
use crate::files::{self, DirectoryStamp};
use nw_domain::{DomainError, Result, records::NativeObjectIdentity};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
#[cfg(windows)]
use std::fs::OpenOptions;

/// Open an already verified generation file without any create/truncate disposition.
pub fn open_existing_control_file(path: &Path, expected: &NativeObjectIdentity) -> Result<File> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags};
        File::from(rustix::fs::open(path, OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(files::io_error)?)
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
        OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(files::io_error)?
    };
    #[cfg(not(any(unix, windows)))]
    return Err(unavailable("No admitted held generation file on this target"));
    #[cfg(any(unix, windows))]
    {
        let metadata = file.metadata().map_err(files::io_error)?;
        if !metadata.is_file() || metadata.len() == 0 || files::native_object(&file)? != *expected {
            return Err(unavailable("Existing generation file identity or length differs"));
        }
        Ok(file)
    }
}

pub struct QualifiedDirectory {
    stamp: DirectoryStamp,
    handle: File,
    filesystem: &'static str,
}
impl QualifiedDirectory {
    pub fn open(path: &Path) -> Result<Self> {
        let stamp = DirectoryStamp::capture_resolved(path)?;
        #[cfg(unix)]
        let handle = {
            use rustix::fs::{Mode, OFlags};
            File::from(rustix::fs::open(path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(files::io_error)?)
        };
        #[cfg(windows)]
        let handle = {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
            OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path).map_err(files::io_error)?
        };
        #[cfg(not(any(unix, windows)))]
        return Err(unavailable("No qualified local restoration filesystem on this target"));
        #[cfg(any(unix, windows))]
        {
            if files::native_object(&handle)? != stamp.object_identity()? {
                return Err(unavailable("Restoration directory handle identity changed"));
            }
            let filesystem = qualify_filesystem(&handle)?;
            stamp.revalidate()?;
            Ok(Self { stamp, handle, filesystem })
        }
    }
    pub fn filesystem(&self) -> &str { self.filesystem }
    pub fn identity(&self) -> Result<NativeObjectIdentity> { self.stamp.object_identity() }
    pub fn revalidate(&self) -> Result<()> {
        self.stamp.revalidate()?;
        if files::native_object(&self.handle)? != self.stamp.object_identity()? {
            return Err(unavailable("Restoration directory identity changed"));
        }
        Ok(())
    }
    pub fn flush_final_file(&self, name: &str, expected: &NativeObjectIdentity, sha256: &str, maximum: u64) -> Result<()> {
        valid_name(name)?;
        self.revalidate()?;
        let path = self.stamp.path().join(name);
        let mut file = open_existing_control_file(&path, expected)?;
        verify(&mut file, expected, sha256, maximum)?;
        file.sync_all().map_err(files::io_error)?;
        #[cfg(unix)]
        self.handle.sync_all().map_err(files::io_error)?;
        self.revalidate()?;
        if files::source_object(&path)? != *expected {
            return Err(unavailable("Final archive name changed after flush"));
        }
        Ok(())
    }
    /// The caller has already verified immutable B and the independent activation archive.
    pub fn remove_activation(&self, name: &str, expected: &NativeObjectIdentity, sha256: &str) -> Result<()> {
        valid_name(name)?;
        if name.len() != 15 || !name.ends_with(".json") || !name[..10].bytes().all(|b| b.is_ascii_digit()) {
            return Err(unavailable("Only exact activation suffix names may be removed"));
        }
        self.revalidate()?;
        #[cfg(unix)]
        {
            use rustix::fs::{AtFlags, Mode, OFlags};
            let descriptor = rustix::fs::openat(&self.handle, name, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(files::io_error)?;
            let mut file = File::from(descriptor);
            verify(&mut file, expected, sha256, 65536)?;
            // A held private parent plus the exact identity prevents unlinking a substituted name.
            let actual = rustix::fs::statat(&self.handle, name, AtFlags::SYMLINK_NOFOLLOW).map_err(files::io_error)?;
            let held = rustix::fs::fstat(&file).map_err(files::io_error)?;
            if actual.st_dev != held.st_dev || actual.st_ino != held.st_ino {
                return Err(unavailable("Activation name changed before held relative removal"));
            }
            rustix::fs::unlinkat(&self.handle, name, AtFlags::empty()).map_err(files::io_error)?;
            self.handle.sync_all().map_err(files::io_error)?;
        }
        #[cfg(windows)]
        remove_windows(&self.stamp.path().join(name), expected, sha256)?;
        self.revalidate()?;
        if self.stamp.path().join(name).try_exists().map_err(files::io_error)? {
            return Err(unavailable("Activation deletion remains pending after handle close"));
        }
        Ok(())
    }
}
fn unavailable(message: &str) -> DomainError { DomainError::new("RESTORATION_UNAVAILABLE", message) }
fn valid_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 128 || name == "." || name == ".." || name.bytes().any(|b| !b.is_ascii_alphanumeric() && !b"._-".contains(&b)) {
        return Err(unavailable("Expected one bounded ordinary archive name"));
    }
    Ok(())
}
fn verify(file: &mut File, expected: &NativeObjectIdentity, sha256: &str, maximum: u64) -> Result<()> {
    let metadata = file.metadata().map_err(files::io_error)?;
    if !metadata.is_file() || metadata.len() > maximum || files::native_object(file)? != *expected {
        return Err(unavailable("Held restoration file identity or size differs"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(files::io_error)?;
        if count == 0 { break; }
        bytes = bytes.checked_add(count as u64).filter(|n| *n <= maximum)
            .ok_or_else(|| unavailable("Restoration file grew beyond its bound"))?;
        hash.update(&buffer[..count]);
    }
    if bytes != metadata.len() || hex::encode(hash.finalize()) != sha256 || files::native_object(file)? != *expected {
        return Err(unavailable("Restoration file content differs from its independent proof"));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn qualify_filesystem(file: &File) -> Result<&'static str> {
    use std::os::fd::AsRawFd;
    if rustix::fs::fstatfs(file).map_err(files::io_error)?.f_type as u64 != 0xef53 {
        return Err(unavailable("Restoration requires qualified local ext4 storage"));
    }
    let mut info = String::new();
    File::open(format!("/proc/self/fdinfo/{}", file.as_raw_fd())).map_err(files::io_error)?
        .take(65537).read_to_string(&mut info).map_err(files::io_error)?;
    if info.len() > 65536 { return Err(unavailable("Held mount identity exceeds its bound")); }
    let mount = info.lines().find_map(|line| line.strip_prefix("mnt_id:").map(str::trim))
        .ok_or_else(|| unavailable("Held mount identity is unavailable"))?;
    let mut mounts = String::new();
    File::open("/proc/self/mountinfo").map_err(files::io_error)?
        .take(1024 * 1024 + 1).read_to_string(&mut mounts).map_err(files::io_error)?;
    if mounts.len() > 1024 * 1024 { return Err(unavailable("Local mount inventory exceeds its bound")); }
    let qualified = mounts.lines().any(|line| {
        line.split_whitespace().next() == Some(mount)
            && line.split_once(" - ").is_some_and(|(_, kind)| kind.split_whitespace().next() == Some("ext4"))
    });
    if !qualified { return Err(unavailable("Held filesystem is not local ext4")); }
    Ok("ext4")
}
#[cfg(target_os = "macos")]
fn qualify_filesystem(file: &File) -> Result<&'static str> {
    let facts = rustix::fs::fstatfs(file).map_err(files::io_error)?;
    let name: Vec<u8> = facts.f_fstypename.iter().map(|value| *value as u8).take_while(|value| *value != 0).collect();
    if name != b"apfs" || facts.f_flags & 0x1000 == 0 {
        return Err(unavailable("Restoration requires qualified local APFS storage"));
    }
    Ok("apfs")
}
#[cfg(windows)]
#[expect(unsafe_code, reason = "Bounded borrowed-handle volume queries retain the File and initialized output buffers")]
fn qualify_filesystem(file: &File) -> Result<&'static str> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, VOLUME_NAME_GUID};
    let mut filesystem = [0u16; 32];
    // SAFETY: the borrowed handle stays live; nullable optional outputs are unused,
    // and the filesystem buffer is writable for exactly its provided length.
    let okay = unsafe { GetVolumeInformationByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), filesystem.as_mut_ptr(), filesystem.len() as u32) };
    let count = filesystem.iter().position(|value| *value == 0).unwrap_or(filesystem.len());
    let mut volume = [0u16; 1024];
    // SAFETY: the initialized output array has the exact supplied capacity; no pointer escapes.
    let length = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), volume.as_mut_ptr(), volume.len() as u32, VOLUME_NAME_GUID) } as usize;
    if okay == 0 || String::from_utf16_lossy(&filesystem[..count]) != "NTFS" || length == 0 || length >= volume.len()
        || !String::from_utf16_lossy(&volume[..length]).starts_with(r"\\?\Volume{") {
        return Err(unavailable("Restoration requires a held local NTFS volume"));
    }
    Ok("ntfs")
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn qualify_filesystem(_file: &File) -> Result<&'static str> { Err(unavailable("Filesystem has no qualified restoration contract")) }

#[cfg(windows)]
#[expect(unsafe_code, reason = "FileDispositionInfo applies only to the verified held DELETE handle and exact repr(C) buffer")]
fn remove_windows(path: &Path, expected: &NativeObjectIdentity, sha256: &str) -> Result<()> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_READ, FileDispositionInfo, SetFileInformationByHandle};
    let mut file = OpenOptions::new().access_mode(DELETE | FILE_GENERIC_READ).share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(files::io_error)?;
    verify(&mut file, expected, sha256, 65536)?;
    let information = FILE_DISPOSITION_INFO { DeleteFile: 1 };
    // SAFETY: File retains the verified DELETE handle. The input is the exact
    // initialized repr(C) disposition structure and is borrowed only for this call.
    let okay = unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo, std::ptr::from_ref(&information).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) };
    if okay == 0 { return Err(files::io_error(std::io::Error::last_os_error())); }
    drop(file);
    Ok(())
}

#[cfg(test)]
pub(crate) fn exercise_restoration_primitives() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = crate::security::private_directory(temporary.path()).unwrap();
    let held = QualifiedDirectory::open(&directory).unwrap();
    let original = directory.join("0000000002.json");
    std::fs::write(&original, b"independent archived activation").unwrap();
    let expected = files::source_object(&original).unwrap();
    let digest = hex::encode(Sha256::digest(b"independent archived activation"));
    assert!(held.remove_activation("../0000000002.json", &expected, &digest).is_err());
    assert!(held.remove_activation("copy.json", &expected, &digest).is_err());
    assert!(held.remove_activation("0000000002.json", &expected, &"0".repeat(64)).is_err());
    assert!(original.exists());
    let replacement = directory.join("replacement.json");
    std::fs::write(&replacement, b"independent archived activation").unwrap();
    let foreign = files::source_object(&replacement).unwrap();
    assert!(held.remove_activation("0000000002.json", &foreign, &digest).is_err());
    assert!(open_existing_control_file(&original, &foreign).is_err());
    assert!(open_existing_control_file(&directory.join("absent.redb"), &expected).is_err());
    assert!(!directory.join("absent.redb").exists());
    held.flush_final_file("0000000002.json", &expected, &digest, 65536).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        let sharing = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&original).unwrap();
        assert!(held.remove_activation("0000000002.json", &expected, &digest).is_err());
        assert!(original.exists(), "Sharing refusal must retain the fence");
        drop(sharing);
    }
    held.remove_activation("0000000002.json", &expected, &digest).unwrap();
    assert!(!original.exists());
    assert!(replacement.exists());
    #[cfg(target_os = "linux")]
    assert!(QualifiedDirectory::open(Path::new("/proc")).is_err());
}
