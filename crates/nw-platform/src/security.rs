//! OS-enforced protection is established before credentials are written.
use crate::files::io_error;
use nw_domain::{DomainError, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

fn denied(message: impl Into<String>) -> DomainError {
    DomainError::new("INSECURE_DATA_ROOT", message)
}
pub fn private_directory(path: &Path) -> Result<PathBuf> {
    let existed = path.try_exists().map_err(io_error)?;
    if !existed {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io_error)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(denied(
            "Credential/store directory must be a real owned directory",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let output = Command::new("/usr/bin/id")
            .arg("-u")
            .output()
            .map_err(io_error)?;
        let uid = String::from_utf8(output.stdout)
            .map_err(io_error)?
            .trim()
            .parse::<u32>()
            .map_err(io_error)?;
        if !output.status.success()
            || metadata.uid() != uid
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(denied(
                "Directory must belong to the current user and exclude group/other access; choose a protected data root",
            ));
        }
    }
    #[cfg(windows)]
    protect_windows(&canonical, !existed)?;
    Ok(canonical)
}
#[cfg(windows)]
fn system_tool(name: &str) -> Result<PathBuf> {
    let root =
        std::env::var_os("SystemRoot").ok_or_else(|| denied("Windows SystemRoot unavailable"))?;
    Ok(PathBuf::from(root).join("System32").join(name))
}
#[cfg(windows)]
fn protect_windows(path: &Path, fresh: bool) -> Result<()> {
    let output = Command::new(system_tool("whoami.exe")?)
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .map_err(io_error)?;
    if !output.status.success() {
        return Err(denied("Cannot establish the current Windows SID"));
    }
    let text = String::from_utf8(output.stdout).map_err(io_error)?;
    let sid = text
        .split([',', '"', '\r', '\n'])
        .find(|part| part.starts_with("S-1-5-"))
        .ok_or_else(|| denied("Windows SID unavailable"))?;
    if fresh {
        let status = Command::new(system_tool("icacls.exe")?)
            .arg(path)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("*{sid}:(OI)(CI)F"))
            .arg("/q")
            .output()
            .map_err(io_error)?;
        if !status.status.success() {
            return Err(denied(
                "User-only Windows ACL could not be established; use NTFS with ACL support",
            ));
        }
    }
    let scratch_parent = if path.is_dir() { path } else { path.parent().ok_or_else(|| denied("Protected file parent is required"))? };
    let scratch = tempfile::Builder::new()
        .prefix("acl-check-")
        .tempfile_in(scratch_parent)
        .map_err(io_error)?;
    let check = scratch.path().to_owned();
    scratch.close().map_err(io_error)?;
    let output = Command::new(system_tool("icacls.exe")?)
        .arg(path)
        .arg("/save")
        .arg(&check)
        .arg("/q")
        .output()
        .map_err(io_error)?;
    let bytes = fs::read(&check).map_err(io_error);
    let _ = fs::remove_file(&check);
    if !output.status.success() {
        return Err(denied("Cannot verify the Windows DACL"));
    }
    let bytes = bytes?;
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || bytes.get(1) == Some(&0);
    let offset = if bytes.starts_with(&[0xff, 0xfe]) {
        2
    } else {
        0
    };
    let text = if utf16 {
        let (words, remainder) = bytes[offset..].as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(denied("Invalid odd-byte UTF-16 ACL evidence"));
        }
        String::from_utf16(
            &words
                .iter()
                .map(|word| u16::from_le_bytes(*word))
                .collect::<Vec<_>>(),
        )
        .map_err(io_error)?
    } else {
        String::from_utf8(bytes).map_err(io_error)?
    };
    check_dacl(&text, sid)
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

pub fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>> {
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
        use std::os::unix::fs::{PermissionsExt, MetadataExt};
        if metadata.permissions().mode() & 0o077 != 0 || metadata.uid() != fs::metadata(parent).map_err(io_error)?.uid() {
            return Err(denied("Credential/runtime file permissions are too broad"));
        }
    }
    #[cfg(windows)]
    protect_windows(path, false)?;
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
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.as_file().sync_all().map_err(io_error)?;
    if replace_owned {
        file.persist(path).map_err(|error| io_error(error.error))?;
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
        let changed = Command::new(system_tool("icacls.exe").unwrap()).arg(&path).args(["/grant", "*S-1-1-0:R", "/q"]).output().unwrap();
        assert!(changed.status.success());
        assert_eq!(read_private(&path, 128).unwrap_err().code, "INSECURE_DATA_ROOT");
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
