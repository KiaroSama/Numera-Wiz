//! Owned private storage. No destructor deletes scratch while children may live.
use crate::{PrivateResult, Result, require};
use rustix::fs::{Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

pub const MIB: u64 = 1024 * 1024;
pub const CAPTURE_LIMIT: usize = MIB as usize;
pub const REPORT_LIMIT: u64 = 64 * MIB;
pub const PROOF_LIMIT: u64 = 4 * MIB;
pub const RECORD_LIMIT: u64 = 16 * 1024;
pub const MAX_RECORDS: usize = 4096;

pub fn nonce() -> Result<String> {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .private("SECURITY_RANDOM_FAILED")?;
    Ok(format!("{:032x}", u128::from_ne_bytes(bytes)))
}
pub fn directory(path: &Path) -> Result<()> {
    DirBuilder::new()
        .mode(0o700)
        .create(path)
        .private("SECURITY_PRIVATE_IO_FAILED")
}
pub struct Scratch {
    path: PathBuf,
}
impl Scratch {
    pub fn create(parent: &Path) -> Result<Self> {
        let parent = fs::canonicalize(parent).private("SECURITY_TEMP_INVALID")?;
        let path = parent.join(format!("nw-security-{}", nonce()?));
        directory(&path)?;
        Ok(Self { path })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn remove(self, barrier: &crate::process::NoChildren) -> Result<()> {
        barrier.recheck()?;
        fs::remove_dir_all(self.path).private("SECURITY_CLEANUP_FAILED")
    }
}
pub fn create(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .private("SECURITY_PRIVATE_IO_FAILED")
}
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create(path)?;
    file.write_all(bytes)
        .private("SECURITY_PRIVATE_IO_FAILED")?;
    file.sync_all().private("SECURITY_PRIVATE_IO_FAILED")
}
pub fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .private("SECURITY_PRIVATE_IO_FAILED")?;
    let file = File::from(fd);
    let metadata = file.metadata().private("SECURITY_PRIVATE_IO_FAILED")?;
    require(
        metadata.is_file() && metadata.len() <= limit,
        "SECURITY_SIZE_INVALID",
    )?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .private("SECURITY_PRIVATE_IO_FAILED")?;
    require(bytes.len() as u64 <= limit, "SECURITY_SIZE_INVALID")?;
    Ok(bytes)
}
pub fn identity(path: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(path).private("SECURITY_IDENTITY_INVALID")?;
    require(!meta.file_type().is_symlink(), "SECURITY_IDENTITY_INVALID")?;
    Ok(format!("{}:{}", meta.dev(), meta.ino()))
}
pub fn hash(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for &byte in digest.as_slice() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
pub fn executable_hash(path: &Path) -> Result<String> {
    Ok(hash(&read(path, 128 * MIB)?))
}
pub fn copy_executable(source: &Path, target: &Path) -> Result<String> {
    let bytes = read(source, 128 * MIB)?;
    write(target, &bytes)?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o700))
        .private("SECURITY_PRIVATE_IO_FAILED")?;
    let digest = hash(&bytes);
    require(
        executable_hash(target)? == digest,
        "SECURITY_EXECUTABLE_INVALID",
    )?;
    Ok(digest)
}
pub fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or(crate::ScanFailure("SECURITY_PATH_INVALID"))
}
pub fn controlled(command: &mut Command, scratch: &Path) {
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", scratch)
        .env("XDG_CONFIG_HOME", scratch)
        .env("TMPDIR", scratch)
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_PAGER", "cat")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_COUNT", "6")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", "/dev/null")
        .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_1", "false")
        .env("GIT_CONFIG_KEY_2", "core.attributesFile")
        .env("GIT_CONFIG_VALUE_2", "/dev/null")
        .env("GIT_CONFIG_KEY_3", "gc.auto")
        .env("GIT_CONFIG_VALUE_3", "0")
        .env("GIT_CONFIG_KEY_4", "maintenance.auto")
        .env("GIT_CONFIG_VALUE_4", "false")
        .env("GIT_CONFIG_KEY_5", "protocol.allow")
        .env("GIT_CONFIG_VALUE_5", "never");
}
