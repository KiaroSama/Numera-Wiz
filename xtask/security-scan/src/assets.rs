//! Checksum-pinned release; the downloader runs as an owned bounded child.
use crate::bootstrap::Deadline;
use crate::{PrivateResult, Result, ScanFailure, private, process, require};
use flate2::read::MultiGzDecoder;
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub const VERSION: &str = "8.30.1";
pub const ARCHIVE_SHA: &str = "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb";
pub const URL: &str = "https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz";
pub struct Release {
    archive: PathBuf,
    executable: PathBuf,
    executable_sha: String,
}
impl Release {
    pub fn prepare(root: &Path, deadline: Deadline) -> Result<Self> {
        require(!process::cancelled(), "SECURITY_CANCELLED")?;
        let helper = root.join("security-download");
        let current = std::env::current_exe().private("SECURITY_EXECUTABLE_INVALID")?;
        let helper_sha = private::copy_executable_bounded(&current, &helper, deadline)?;
        let token = private::nonce()?;
        let download_deadline = deadline
            .reserve(process::GRACE)?
            .cap(Duration::from_secs(60))?;
        private::write_bounded(
            &root.join("download-context.json"),
            &serde_json::to_vec(&json!({
                "token": token, "parent": rustix::process::getpid().as_raw_pid(),
                "root": private::identity(root)?, "helper": helper_sha, "deadline": download_deadline.value()
            }))
            .private("SECURITY_CONTEXT_INVALID")?,
         deadline)?;
        let mut command = Command::new(&helper);
        private::controlled(&mut command, root);
        command
            .current_dir(root)
            .env("NW_SECURITY_DOWNLOAD_TOKEN", &token);
        let result = process::run(
            &mut command,
            process::Options::command(Duration::from_secs(60), deadline),
            || match fs::symlink_metadata(root.join("release.tar.gz")) {
                Ok(meta) => require(
                    meta.is_file() && meta.len() <= 64 * private::MIB,
                    "SECURITY_DOWNLOAD_OVERSIZE",
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(ScanFailure("SECURITY_DOWNLOAD_FAILED")),
            },
        )?;
        result.successful()?;
        require(
            result.stdout.quiet() && result.stderr.quiet() && result.adopted == 0,
            "SECURITY_DOWNLOAD_FAILED",
        )?;
        let archive = root.join("release.tar.gz");
        let bytes = private::read_bounded(&archive, 64 * private::MIB, deadline)?;
        let executable = root.join("gitleaks");
        let payload = verify_pinned(&bytes, deadline)?;
        private::write_bounded(&executable, &payload, deadline)?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .private("SECURITY_PRIVATE_IO_FAILED")?;
        let release = Self {
            archive,
            executable,
            executable_sha: private::hash_bounded(&payload, deadline)?,
        };
        let mut version = Command::new(&release.executable);
        private::controlled(&mut version, root);
        version.current_dir(root).arg("version");
        let observed = process::run(
            &mut version,
            process::Options::command(Duration::from_secs(15), deadline),
            || Ok(()),
        )?;
        observed.successful()?;
        verify_version(
            observed.status,
            &observed.stdout.bytes,
            observed.stderr.quiet(),
        )?;
        Ok(release)
    }
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn validate(&self, deadline: Deadline) -> Result<()> {
        require(
            private::hash_bounded(
                &private::read_bounded(&self.archive, 64 * private::MIB, deadline)?,
                deadline,
            )? == ARCHIVE_SHA
                && private::executable_hash_bounded(&self.executable, deadline)?
                    == self.executable_sha,
            "SECURITY_EXECUTABLE_INVALID",
        )
    }
}
pub fn verify_version(status: Option<i32>, stdout: &[u8], quiet_stderr: bool) -> Result<()> {
    require(
        status == Some(0)
            && quiet_stderr
            && std::str::from_utf8(stdout).is_ok_and(|s| s.trim() == VERSION),
        "SECURITY_VERSION_INVALID",
    )
}
pub fn verify_pinned(bytes: &[u8], deadline: Deadline) -> Result<Vec<u8>> {
    require(
        bytes.len() as u64 <= 64 * private::MIB
            && private::hash_bounded(bytes, deadline)? == ARCHIVE_SHA,
        "SECURITY_ARCHIVE_HASH_INVALID",
    )?;
    inspect_archive(bytes, deadline)
}
/// Verifier boundary for malformed-archive fixtures, never a production pin override.
pub fn inspect_archive(bytes: &[u8], deadline: Deadline) -> Result<Vec<u8>> {
    require(
        bytes.len() as u64 <= 64 * private::MIB,
        "SECURITY_ARCHIVE_OVERSIZE",
    )?;
    let mut decoder = MultiGzDecoder::new(bytes);
    let mut decoded = Vec::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        deadline.check()?;
        require(!process::cancelled(), "SECURITY_DEADLINE")?;
        let size = decoder
            .read(&mut buffer)
            .private("SECURITY_ARCHIVE_INVALID")?;
        deadline.check()?;
        if size == 0 {
            break;
        }
        require(
            decoded.len() as u64 + size as u64 <= 256 * private::MIB,
            "SECURITY_ARCHIVE_OVERSIZE",
        )?;
        decoded.extend_from_slice(&buffer[..size]);
    }
    let mut archive = tar::Archive::new(Cursor::new(decoded));
    // Continue through padding/concatenated entries; never stop validation at the selected member.
    archive.set_ignore_zeros(true);
    let mut paths = BTreeSet::new();
    let mut payload = None;
    for entry in archive
        .entries()
        .private("SECURITY_ARCHIVE_INVALID")?
        .raw(true)
    {
        deadline.check()?;
        require(!process::cancelled(), "SECURITY_DEADLINE")?;
        let mut entry = entry.private("SECURITY_ARCHIVE_INVALID")?;
        require(paths.len() < 128, "SECURITY_ARCHIVE_MEMBER_LIMIT")?;
        let path_bytes = entry.path_bytes();
        let path = std::str::from_utf8(&path_bytes).private("SECURITY_ARCHIVE_PATH_INVALID")?;
        require(
            path.len() <= 4096 && !path.starts_with('/'),
            "SECURITY_ARCHIVE_PATH_INVALID",
        )?;
        let components = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
        require(
            components.len() <= 16
                && components.iter().all(|p| {
                    !p.is_empty() && *p != "." && *p != ".." && p.len() <= 255 && !p.contains('\\')
                }),
            "SECURITY_ARCHIVE_PATH_INVALID",
        )?;
        require(paths.insert(path.to_owned()), "SECURITY_ARCHIVE_DUPLICATE")?;
        let selected = path == "gitleaks";
        let kind = entry.header().entry_type();
        // Raw entries expose GNU/PAX metadata; this pinned release must fit simple
        // regular files/directories. Never unpack links, sparse or metadata entries.
        require(
            kind.is_file() || (kind.is_dir() && entry.size() == 0),
            "SECURITY_ARCHIVE_TYPE_INVALID",
        )?;
        if selected {
            require(
                kind.is_file() && entry.size() > 0 && entry.size() <= 128 * private::MIB,
                "SECURITY_ARCHIVE_EXECUTABLE_INVALID",
            )?;
            let expected = entry.size();
            let mut content = Vec::new();
            loop {
                deadline.check()?;
                let count = entry
                    .read(&mut buffer)
                    .private("SECURITY_ARCHIVE_INVALID")?;
                deadline.check()?;
                if count == 0 {
                    break;
                }
                require(
                    content.len() as u64 + count as u64 <= 128 * private::MIB,
                    "SECURITY_ARCHIVE_OVERSIZE",
                )?;
                content.extend_from_slice(&buffer[..count]);
            }
            require(content.len() as u64 == expected, "SECURITY_ARCHIVE_INVALID")?;
            payload = Some(content);
        } else {
            loop {
                deadline.check()?;
                if entry
                    .read(&mut buffer)
                    .private("SECURITY_ARCHIVE_INVALID")?
                    == 0
                {
                    break;
                }
            }
        }
    }
    deadline.check()?;
    payload.ok_or(ScanFailure("SECURITY_ARCHIVE_EXECUTABLE_INVALID"))
}
pub fn downloader_worker() -> Result<()> {
    let executable = fs::canonicalize(std::env::current_exe().private("SECURITY_CONTEXT_INVALID")?)
        .private("SECURITY_CONTEXT_INVALID")?;
    let root = executable
        .parent()
        .ok_or(ScanFailure("SECURITY_CONTEXT_INVALID"))?;
    let meta = fs::symlink_metadata(root).private("SECURITY_CONTEXT_INVALID")?;
    require(
        meta.is_dir()
            && meta.mode() & 0o777 == 0o700
            && meta.uid() == rustix::process::getuid().as_raw(),
        "SECURITY_CONTEXT_INVALID",
    )?;
    let value = crate::protocol::read_json(&root.join("download-context.json"))?;
    let deadline = Deadline::from_value(&value["deadline"])?;
    let token = std::env::var("NW_SECURITY_DOWNLOAD_TOKEN").private("SECURITY_CONTEXT_INVALID")?;
    require(
        value["token"].as_str() == Some(&token)
            && token.len() == 32
            && value["root"].as_str() == Some(&private::identity(root)?)
            && value["helper"].as_str()
                == Some(&private::executable_hash_bounded(&executable, deadline)?)
            && value["parent"].as_i64()
                == rustix::process::getppid().map(|p| i64::from(p.as_raw_pid())),
        "SECURITY_CONTEXT_INVALID",
    )?;
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .no_proxy()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(60).min(deadline.remaining()?))
        .connect_timeout(Duration::from_secs(15).min(deadline.remaining()?))
        .build()
        .private("SECURITY_DOWNLOAD_FAILED")?;
    deadline.check()?;
    let mut response = client
        .get(URL)
        .send()
        .and_then(|r| r.error_for_status())
        .private("SECURITY_DOWNLOAD_FAILED")?;
    deadline.check()?;
    if let Some(length) = response.content_length() {
        require(length <= 64 * private::MIB, "SECURITY_DOWNLOAD_OVERSIZE")?;
    }
    let mut file = private::create(&root.join("release.tar.gz"))?;
    let mut total = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        deadline.check()?;
        let size = response
            .read(&mut buffer)
            .private("SECURITY_DOWNLOAD_FAILED")?;
        deadline.check()?;
        if size == 0 {
            break;
        }
        total += size as u64;
        require(total <= 64 * private::MIB, "SECURITY_DOWNLOAD_OVERSIZE")?;
        file.write_all(&buffer[..size])
            .private("SECURITY_DOWNLOAD_FAILED")?;
    }
    deadline.check()?;
    file.sync_all().private("SECURITY_DOWNLOAD_FAILED")?;
    deadline.check()
}
