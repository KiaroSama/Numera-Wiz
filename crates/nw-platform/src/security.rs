//! OS-enforced protection is established before credentials are written.
use crate::files::io_error;
use nw_domain::{DomainError, Result};
use std::{fs, io::Write, path::{Path, PathBuf}, process::Command};

fn denied(message: impl Into<String>) -> DomainError {
    DomainError::new("INSECURE_DATA_ROOT", message)
}
pub fn private_directory(path: &Path) -> Result<PathBuf> {
    let existed = path.try_exists().map_err(io_error)?;
    if !existed {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)] {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io_error)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(denied("Credential/store directory must be a real owned directory"));
    }
    let canonical = fs::canonicalize(path).map_err(io_error)?;
    #[cfg(unix)] {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let output = Command::new("/usr/bin/id").arg("-u").output().map_err(io_error)?;
        let uid = String::from_utf8(output.stdout).map_err(io_error)?.trim().parse::<u32>().map_err(io_error)?;
        if !output.status.success() || metadata.uid() != uid || metadata.permissions().mode() & 0o077 != 0 {
            return Err(denied("Directory must belong to the current user and exclude group/other access; choose a protected data root"));
        }
    }
    #[cfg(windows)] protect_windows(&canonical, !existed)?;
    Ok(canonical)
}
#[cfg(windows)]
fn system_tool(name: &str) -> Result<PathBuf> {
    let root = std::env::var_os("SystemRoot").ok_or_else(|| denied("Windows SystemRoot unavailable"))?;
    Ok(PathBuf::from(root).join("System32").join(name))
}
#[cfg(windows)]
fn protect_windows(path: &Path, fresh: bool) -> Result<()> {
    let output = Command::new(system_tool("whoami.exe")?).args(["/user", "/fo", "csv", "/nh"]).output().map_err(io_error)?;
    if !output.status.success() { return Err(denied("Cannot establish the current Windows SID")); }
    let text = String::from_utf8(output.stdout).map_err(io_error)?;
    let sid = text.split([',', '"', '\r', '\n']).find(|part| part.starts_with("S-1-5-")).ok_or_else(|| denied("Windows SID unavailable"))?;
    if fresh {
        let status = Command::new(system_tool("icacls.exe")?).arg(path).args(["/inheritance:r", "/grant:r"]).arg(format!("*{sid}:(OI)(CI)F")).arg("/q").output().map_err(io_error)?;
        if !status.status.success() { return Err(denied("User-only Windows ACL could not be established; use NTFS with ACL support")); }
    }
    let scratch = tempfile::Builder::new().prefix("acl-check-").tempfile_in(path).map_err(io_error)?;
    let check = scratch.path().to_owned();
    scratch.close().map_err(io_error)?;
    let output = Command::new(system_tool("icacls.exe")?).arg(path).arg("/save").arg(&check).arg("/q").output().map_err(io_error)?;
    let bytes = fs::read(&check).map_err(io_error);
    let _ = fs::remove_file(&check);
    if !output.status.success() { return Err(denied("Cannot verify the Windows DACL")); }
    let bytes = bytes?;
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        String::from_utf16(&bytes[2..].chunks_exact(2).map(|v| u16::from_le_bytes([v[0], v[1]])).collect::<Vec<_>>()).map_err(io_error)?
    } else { String::from_utf8(bytes).map_err(io_error)? };
    let mut own = false;
    let mut entries = 0;
    for entry in text.split('(').skip(1) {
        let entry = entry.split(')').next().ok_or_else(|| denied("Invalid ACL evidence"))?;
        let parts = entry.split(';').collect::<Vec<_>>();
        if parts.len() != 6 || parts[0] != "A" || ![sid, "SY", "BA", "S-1-5-18", "S-1-5-32-544"].contains(&parts[5]) {
            return Err(denied("Directory grants access to another user or group; choose a protected root"));
        }
        if parts[5] == sid && ["FA", "0x1f01ff"].contains(&parts[2]) { own = true; }
        entries += 1;
    }
    if !own || entries == 0 { return Err(denied("Current user full-control ACL was not verified")); }
    Ok(())
}
pub fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let parent = path.parent().ok_or_else(|| denied("Protected parent required"))?;
    private_directory(parent)?;
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(denied("Credential/runtime file is not an admitted bounded regular file"));
    }
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 { return Err(denied("Credential/runtime file permissions are too broad")); }
    }
    fs::read(path).map_err(io_error)
}
pub fn write_private(path: &Path, bytes: &[u8], replace_owned: bool) -> Result<()> {
    let parent = private_directory(path.parent().ok_or_else(|| denied("Protected parent required"))?)?;
    let mut file = tempfile::Builder::new().prefix(".credential-").tempfile_in(&parent).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.as_file().sync_all().map_err(io_error)?;
    if replace_owned { file.persist(path).map_err(|error| io_error(error.error))?; }
    else { file.persist_noclobber(path).map_err(|error| io_error(error.error))?; }
    #[cfg(unix)] fs::File::open(parent).and_then(|file| file.sync_all()).map_err(io_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_protected_and_existing_exports_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let secure = root.path().join("owned-auth");
        private_directory(&secure).unwrap();
        let path = secure.join("token");
        write_private(&path, b"first", false).unwrap();
        assert_eq!(read_private(&path, 64).unwrap(), b"first");
        assert!(write_private(&path, b"replacement", false).is_err());
        assert_eq!(read_private(&path, 64).unwrap(), b"first");
        assert!(read_private(&path, 2).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn an_existing_shared_directory_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(private_directory(root.path()).unwrap_err().code, "INSECURE_DATA_ROOT");
    }
}
