//! OS-enforced protection is established before credentials are written.
use crate::files::{diagnostic_io_error, io_error};
use nw_domain::{DomainError, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[cfg(all(test, windows))]
use std::process::Command;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::protect as protect_windows;

/// Prefix of legacy Windows ACL evidence files in protected directories.
/// Native checks no longer create these files; old retained names remain part of
/// directory-reader protocols and their regression fixtures.
pub const ACL_EVIDENCE_PREFIX: &str = ".credential-acl-check-";

fn denied(message: impl Into<String>) -> DomainError {
    DomainError::new("INSECURE_DATA_ROOT", message)
}
pub fn private_directory(path: &Path) -> Result<PathBuf> {
    let existed = path
        .try_exists()
        .map_err(|error| diagnostic_io_error("private_directory.exists", error))?;
    if !existed {
        #[cfg(windows)]
        windows::create_directory(path)?;
        #[cfg(not(windows))]
        {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(path)
                .map_err(|error| diagnostic_io_error("private_directory.create", error))?;
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| diagnostic_io_error("private_directory.metadata", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(denied(
            "Credential/store directory must be a real owned directory",
        ));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| diagnostic_io_error("private_directory.canonicalize", error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let uid = rustix::process::geteuid().as_raw();
        if metadata.uid() != uid
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(denied(
                "Directory must belong to the current user and exclude group/other access; choose a protected data root",
            ));
        }
    }
    #[cfg(windows)]
    protect_windows(&canonical)?;
    Ok(canonical)
}
#[cfg(all(test, windows))]
fn system_tool(name: &str) -> Result<PathBuf> {
    let root =
        std::env::var_os("SystemRoot").ok_or_else(|| denied("Windows SystemRoot unavailable"))?;
    Ok(PathBuf::from(root).join("System32").join(name))
}
#[cfg(any(windows, test))]
fn check_dacl(text: &str, sid: &str) -> Result<()> {
    let mut own = false;
    let mut entries = 0;
    for entry in text
        .lines()
        .filter_map(|line| line.find("D:").map(|at| &line[at..]))
        .flat_map(|line| line.split('(').skip(1))
    {
        let entry = entry
            .split(')')
            .next()
            .ok_or_else(|| denied("Invalid ACL evidence"))?;
        let parts = entry.split(';').collect::<Vec<_>>();
        if parts.len() != 6 || parts[0] != "A" {
            return Err(denied("Unsupported ACL entry in protected directory"));
        }
        // Windows may serialize the current local Administrator SID using its LA alias.
        let current = parts[5] == sid
            || (parts[5] == "LA" && sid.starts_with("S-1-5-21-") && sid.ends_with("-500"));
        let privileged = ["SY", "BA", "S-1-5-18", "S-1-5-32-544"].contains(&parts[5]);
        if !current && !privileged {
            return Err(denied(format!(
                "Directory has a disallowed ACL trustee {}; choose a protected root",
                parts[5]
            )));
        }
        if current
            && (parts[2] == "FA"
                || parts[2]
                    .strip_prefix("0x")
                    .is_some_and(|mask| u32::from_str_radix(mask, 16) == Ok(0x1f01ff)))
        {
            own = true;
        }
        entries += 1;
    }
    if !own || entries == 0 {
        return Err(denied(format!(
            "Current user full-control ACL was not verified (entries={entries}); ACL evidence: {}",
            text.chars()
                .take(512)
                .flat_map(char::escape_debug)
                .collect::<String>()
        )));
    }
    Ok(())
}

pub fn verify_private_file(path: &Path, limit: u64) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| denied("Protected parent required"))?;
    private_directory(parent)?;
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(denied(
            "Credential/runtime file is not an admitted bounded regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != fs::metadata(parent).map_err(io_error)?.uid()
        {
            return Err(denied("Credential/runtime file permissions are too broad"));
        }
    }
    #[cfg(windows)]
    protect_windows(path)?;
    Ok(())
}
pub fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>> {
    verify_private_file(path, limit)?;
    fs::read(path).map_err(io_error)
}
pub fn write_private(path: &Path, bytes: &[u8], replace_owned: bool) -> Result<()> {
    let parent = private_directory(
        path.parent()
            .ok_or_else(|| denied("Protected parent required"))?,
    )?;
    let mut file = tempfile::Builder::new()
        .prefix(".credential-")
        .tempfile_in(&parent)
        .map_err(|error| diagnostic_io_error("private_write.create", error))?;
    file.write_all(bytes)
        .map_err(|error| diagnostic_io_error("private_write.write", error))?;
    file.as_file()
        .sync_all()
        .map_err(|error| diagnostic_io_error("private_write.sync", error))?;
    if replace_owned {
        file.persist(path)
            .map_err(|error| diagnostic_io_error("private_write.persist", error.error))?;
    } else {
        file.persist_noclobber(path)
            .map_err(|error| io_error(error.error))?;
    }
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn private_parent_does_not_substitute_for_the_credential_files_own_dacl() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("credential.txt");
        write_private(&path, b"private file ACL fixture", false).unwrap();
        assert!(read_private(&path, 128).is_ok());
        let changed = Command::new(system_tool("icacls.exe").unwrap())
            .arg(&path)
            .args(["/grant", "*S-1-1-0:R", "/q"])
            .output()
            .unwrap();
        assert!(changed.status.success());
        assert_eq!(
            read_private(&path, 128).unwrap_err().code,
            "INSECURE_DATA_ROOT"
        );
        let directory = private_directory(&root.path().join("existing-directory")).unwrap();
        let changed = Command::new(system_tool("icacls.exe").unwrap())
            .arg(&directory)
            .args(["/grant", "*S-1-1-0:(OI)(CI)R", "/q"])
            .output()
            .unwrap();
        assert!(changed.status.success());
        for _ in 0..2 {
            assert_eq!(
                windows::create_directory(&directory).unwrap_err().code,
                "INSECURE_DATA_ROOT",
                "A creator racing with an existing broad DACL must not repair it"
            );
            assert_eq!(
                private_directory(&directory).unwrap_err().code,
                "INSECURE_DATA_ROOT",
                "An existing broad DACL must be refused without repairing it"
            );
        }
    }
    #[test]
    fn local_admin_sid_alias_does_not_admit_everyone_or_another_normal_user() {
        assert!(check_dacl("D:PAI(A;OICI;FA;;;LA)", "S-1-5-21-1-2-3-500").is_ok());
        assert!(check_dacl("D:PAI(A;OICI;FA;;;LA)", "S-1-5-21-1-2-3-1001").is_err());
        assert!(
            check_dacl(
                "D:PAI(A;OICI;FA;;;S-1-5-21-1-2-3-1001)(A;;FR;;;WD)",
                "S-1-5-21-1-2-3-1001"
            )
            .is_err()
        );
    }
    #[test]
    fn credentials_are_protected_and_existing_exports_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let secure = root.path().join("owned-auth");
        private_directory(&secure).unwrap();
        let nested = secure.join("new-parent/new-child");
        private_directory(&nested).unwrap();
        private_directory(nested.parent().unwrap()).unwrap();
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
        assert_eq!(
            private_directory(root.path()).unwrap_err().code,
            "INSECURE_DATA_ROOT"
        );
    }
}
