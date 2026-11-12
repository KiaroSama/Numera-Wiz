//! One immutable authored-bootstrap origin. Wall time is compatibility metadata only.
use crate::{PrivateResult, Result, ScanFailure, private, require};
use rustix::fs::{Mode, OFlags};
use serde_json::{Value, json};
use std::{
    env,
    fs::{File, Metadata},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const SECOND: u64 = 1_000_000_000;
const STAGE: u64 = 1140 * SECOND;
const KEY: &str = "NW_SECURITY_DEADLINE_UNIX_MS";
const FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::CLOEXEC)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK);

pub fn boot_ns() -> Result<u64> {
    let time = rustix::time::clock_gettime_dynamic(rustix::time::DynamicClockId::Boottime)
        .private("SECURITY_CLOCK_INVALID")?;
    require(
        time.tv_sec >= 0 && (0..1_000_000_000).contains(&time.tv_nsec),
        "SECURITY_CLOCK_INVALID",
    )?;
    u64::try_from(time.tv_sec)
        .ok()
        .and_then(|s| s.checked_mul(SECOND))
        .and_then(|s| s.checked_add(time.tv_nsec as u64))
        .ok_or(ScanFailure("SECURITY_CLOCK_INVALID"))
}
pub fn stage_end(origin: u64) -> Result<u64> {
    origin
        .checked_add(STAGE)
        .ok_or(ScanFailure("SECURITY_DEADLINE"))
}
pub fn evaluate_active(end: u64, now: u64, local_active: bool) -> Result<()> {
    require(now < end && local_active, "SECURITY_DEADLINE")
}
pub fn evaluate_remaining(origin: u64, end: u64, now: u64, wall_ms: u64) -> Result<u64> {
    require(
        end == stage_end(origin)? && now >= origin && now < end,
        "SECURITY_DEADLINE",
    )?;
    wall_ms
        .checked_add((end - now) / 1_000_000)
        .ok_or(ScanFailure("SECURITY_DEADLINE"))
}
pub fn cleanup_end(start: u64, end: u64) -> Result<u64> {
    Ok(start
        .checked_add(5 * SECOND)
        .ok_or(ScanFailure("SECURITY_DEADLINE"))?
        .min(end))
}
pub fn exact_export(value: &str, expected: u64) -> Result<()> {
    require(
        value == expected.to_string(),
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )
}
pub fn parse_origin(bytes: &[u8]) -> Result<u64> {
    let invalid = "SECURITY_BOOTSTRAP_MARKER_INVALID";
    require(bytes.len() <= 128 && bytes.last() == Some(&b'\n'), invalid)?;
    let text = std::str::from_utf8(&bytes[..bytes.len() - 1]).private(invalid)?;
    let (uptime, idle) = text.split_once(' ').ok_or(ScanFailure(invalid))?;
    fn decimal(text: &str) -> Result<u64> {
        let invalid = "SECURITY_BOOTSTRAP_MARKER_INVALID";
        let (whole, fraction) = text.split_once('.').ok_or(ScanFailure(invalid))?;
        require(
            !whole.is_empty()
                && whole.len() <= 20
                && (whole == "0" || !whole.starts_with('0'))
                && whole.bytes().all(|b| b.is_ascii_digit())
                && fraction.len() == 2
                && fraction.bytes().all(|b| b.is_ascii_digit()),
            invalid,
        )?;
        let seconds: u64 = whole.parse().private(invalid)?;
        let hundredths: u64 = fraction.parse().private(invalid)?;
        seconds
            .checked_mul(SECOND)
            .and_then(|n| n.checked_add(hundredths * 10_000_000))
            .ok_or(ScanFailure(invalid))
    }
    let origin = decimal(uptime)?;
    decimal(idle)?;
    Ok(origin)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileFacts {
    pub dev: u64,
    pub ino: u64,
    pub uid: u32,
    pub mode: u32,
    pub links: u64,
    pub len: u64,
    modified: i64,
    modified_ns: i64,
    changed: i64,
    changed_ns: i64,
}
impl FileFacts {
    pub fn from_metadata(m: &Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            uid: m.uid(),
            mode: m.mode(),
            links: m.nlink(),
            len: m.len(),
            modified: m.mtime(),
            modified_ns: m.mtime_nsec(),
            changed: m.ctime(),
            changed_ns: m.ctime_nsec(),
        }
    }
    fn regular(self, limit: u64) -> Result<()> {
        require(
            self.mode & 0o170000 == 0o100000
                && self.uid == rustix::process::geteuid().as_raw()
                && self.links == 1
                && self.len <= limit,
            "SECURITY_BOOTSTRAP_FILE_INVALID",
        )
    }
    pub fn marker(self) -> Result<()> {
        self.regular(128)?;
        require(
            self.mode & 0o7777 == 0o600,
            "SECURITY_BOOTSTRAP_FILE_INVALID",
        )
    }
    pub fn receipt(self) -> Result<()> {
        self.regular(private::RECORD_LIMIT)?;
        require(
            self.mode & 0o7777 == 0o600,
            "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
        )
    }
    pub fn environment(self) -> Result<()> {
        self.regular(private::MIB)?;
        require(self.mode & 0o022 == 0, "SECURITY_BOOTSTRAP_ENV_INVALID")
    }
    pub fn unchanged(self, other: Self) -> Result<()> {
        require(self == other, "SECURITY_BOOTSTRAP_FILE_CHANGED")
    }
    fn value(self) -> Value {
        json!({"dev":self.dev,"ino":self.ino,"uid":self.uid,"mode":self.mode,"links":self.links,"len":self.len,
            "modified":self.modified,"modified_ns":self.modified_ns,"changed":self.changed,"changed_ns":self.changed_ns})
    }
}
fn facts(file: &File) -> Result<FileFacts> {
    Ok(FileFacts::from_metadata(
        &file.metadata().private("SECURITY_BOOTSTRAP_FILE_INVALID")?,
    ))
}
fn read_file(file: &mut File, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .private("SECURITY_BOOTSTRAP_FILE_INVALID")?;
    require(
        bytes.len() as u64 <= limit,
        "SECURITY_BOOTSTRAP_FILE_INVALID",
    )?;
    Ok(bytes)
}
fn open_file(directory: &File, name: &str, flags: OFlags) -> Result<File> {
    require(
        !name.is_empty() && name.len() <= 255 && !name.contains('/') && name != "." && name != "..",
        "SECURITY_BOOTSTRAP_PATH_INVALID",
    )?;
    Ok(File::from(
        rustix::fs::openat(
            directory,
            name,
            flags | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )
        .private("SECURITY_BOOTSTRAP_FILE_INVALID")?,
    ))
}
fn directory(path: &Path) -> Result<File> {
    require(
        path.is_absolute() && path.as_os_str().len() <= 4096,
        "SECURITY_BOOTSTRAP_PATH_INVALID",
    )?;
    let mut current = File::from(
        rustix::fs::open("/", FLAGS | OFlags::DIRECTORY, Mode::empty())
            .private("SECURITY_BOOTSTRAP_PATH_INVALID")?,
    );
    let mut count = 0;
    for part in path.components() {
        let Component::Normal(name) = part else {
            require(
                part == Component::RootDir,
                "SECURITY_BOOTSTRAP_PATH_INVALID",
            )?;
            continue;
        };
        count += 1;
        require(
            count <= 32 && name.len() <= 255,
            "SECURITY_BOOTSTRAP_PATH_INVALID",
        )?;
        current = File::from(
            rustix::fs::openat(&current, name, FLAGS | OFlags::DIRECTORY, Mode::empty())
                .private("SECURITY_BOOTSTRAP_PATH_INVALID")?,
        );
        let m = current
            .metadata()
            .private("SECURITY_BOOTSTRAP_PATH_INVALID")?;
        require(
            m.is_dir()
                && (m.uid() == 0 || m.uid() == rustix::process::geteuid().as_raw())
                && m.mode() & 0o022 == 0,
            "SECURITY_BOOTSTRAP_PATH_INVALID",
        )?;
    }
    require(
        count > 0
            && current
                .metadata()
                .private("SECURITY_BOOTSTRAP_PATH_INVALID")?
                .uid()
                == rustix::process::geteuid().as_raw(),
        "SECURITY_BOOTSTRAP_PATH_INVALID",
    )?;
    Ok(current)
}
fn same_directory(opened: &File, path: &Path) -> Result<()> {
    let expected = opened
        .metadata()
        .private("SECURITY_BOOTSTRAP_PATH_INVALID")?;
    let current = directory(path)?
        .metadata()
        .private("SECURITY_BOOTSTRAP_PATH_INVALID")?;
    require(
        expected.dev() == current.dev()
            && expected.ino() == current.ino()
            && expected.uid() == current.uid()
            && expected.mode() == current.mode()
            && expected.nlink() > 0,
        "SECURITY_BOOTSTRAP_PATH_INVALID",
    )
}
pub fn fixture_read_marker(path: &Path) -> Result<Vec<u8>> {
    // Same FD validator, called only with private synthetic paths by qualification.
    let mut file = File::from(
        rustix::fs::open(path, FLAGS, Mode::empty()).private("SECURITY_BOOTSTRAP_FILE_INVALID")?,
    );
    let before = facts(&file)?;
    before.marker()?;
    let bytes = read_file(&mut file, 128)?;
    before.unchanged(facts(&file)?)?;
    parse_origin(&bytes)?;
    Ok(bytes)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Host {
    boot: [u8; 36],
    namespace_dev: u64,
    namespace_ino: u64,
}
impl Host {
    fn current() -> Result<Self> {
        let mut boot = File::from(
            rustix::fs::open("/proc/sys/kernel/random/boot_id", FLAGS, Mode::empty())
                .private("SECURITY_BOOTSTRAP_HOST_INVALID")?,
        );
        let bytes = read_file(&mut boot, 64)?;
        require(
            bytes.len() == 37
                && bytes[36] == b'\n'
                && bytes[..36].iter().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        *b == b'-'
                    } else {
                        b.is_ascii_digit() || (b'a'..=b'f').contains(b)
                    }
                }),
            "SECURITY_BOOTSTRAP_HOST_INVALID",
        )?;
        let mut id = [0; 36];
        id.copy_from_slice(&bytes[..36]);
        // Deliberately follow ONLY this fixed kernel namespace reference. The
        // opened nsfs FD identifies our current namespace; arbitrary paths never do.
        let namespace = File::from(
            rustix::fs::open(
                "/proc/self/ns/time",
                OFlags::RDONLY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .private("SECURITY_BOOTSTRAP_HOST_INVALID")?,
        );
        let m = namespace
            .metadata()
            .private("SECURITY_BOOTSTRAP_HOST_INVALID")?;
        Ok(Self {
            boot: id,
            namespace_dev: m.dev(),
            namespace_ino: m.ino(),
        })
    }
    fn value(self) -> Value {
        json!({"boot":String::from_utf8_lossy(&self.boot),"namespace_dev":self.namespace_dev,"namespace_ino":self.namespace_ino})
    }
    fn matches(self, value: &Value) -> Result<()> {
        require(self.value() == *value, "SECURITY_BOOTSTRAP_HOST_INVALID")
    }
}

#[derive(Clone, Copy)]
pub struct Deadline {
    end_ns: u64,
    local: Instant,
    origin: u64,
    stage_end: u64,
    exported: u64,
    run: u64,
    attempt: u64,
    host: Host,
    receipt_dev: u64,
    receipt_ino: u64,
    receipt_sha: [u8; 64],
}
impl Deadline {
    pub fn check(self) -> Result<()> {
        evaluate_active(self.end_ns, boot_ns()?, Instant::now() < self.local)
    }
    pub fn check_as(self, category: &'static str) -> Result<()> {
        self.check().map_err(|error| {
            if error.0 == "SECURITY_DEADLINE" {
                ScanFailure(category)
            } else {
                error
            }
        })
    }
    pub fn remaining(self) -> Result<Duration> {
        self.check()?;
        Ok(Duration::from_nanos(
            self.end_ns
                .checked_sub(boot_ns()?)
                .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
        )
        .min(self.local.saturating_duration_since(Instant::now())))
    }
    pub fn cap(self, duration: Duration) -> Result<Self> {
        self.check()?;
        self.cap_window(duration)
    }
    pub fn cleanup(self) -> Result<(Self, u64)> {
        let local = Instant::now();
        let now = boot_ns()?;
        Ok((
            Self {
                end_ns: cleanup_end(now, self.end_ns)?,
                local: self.local.min(
                    local
                        .checked_add(Duration::from_secs(5))
                        .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
                ),
                ..self
            },
            now,
        ))
    }
    pub fn cap_window(self, duration: Duration) -> Result<Self> {
        let local = Instant::now();
        let now = boot_ns()?;
        let ns = u64::try_from(duration.as_nanos()).private("SECURITY_DEADLINE")?;
        Ok(Self {
            end_ns: self.end_ns.min(
                now.checked_add(ns)
                    .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
            ),
            local: self.local.min(
                local
                    .checked_add(duration)
                    .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
            ),
            ..self
        })
    }
    pub fn reserve(self, duration: Duration) -> Result<Self> {
        let ns = u64::try_from(duration.as_nanos()).private("SECURITY_DEADLINE")?;
        let result = Self {
            end_ns: self
                .end_ns
                .checked_sub(ns)
                .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
            local: self
                .local
                .checked_sub(duration)
                .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
            ..self
        };
        result.check()?;
        Ok(result)
    }
    pub fn value(self) -> Value {
        json!({"end_ns":self.end_ns,"origin":self.origin,"stage_end":self.stage_end,"exported":self.exported,
            "run":self.run,"attempt":self.attempt,"host":self.host.value(),"receipt_dev":self.receipt_dev,
            "receipt_ino":self.receipt_ino,"receipt_sha":String::from_utf8_lossy(&self.receipt_sha)})
    }
    pub fn from_value(value: &Value) -> Result<Self> {
        let integer = |key: &str| value[key].as_u64().ok_or(ScanFailure("SECURITY_DEADLINE"));
        let local = Instant::now();
        let now = boot_ns()?;
        let host = Host::current()?;
        host.matches(&value["host"])?;
        let origin = integer("origin")?;
        let stage_end = integer("stage_end")?;
        let end_ns = integer("end_ns")?;
        require(
            origin <= now
                && stage_end == self::stage_end(origin)?
                && end_ns <= stage_end
                && now < end_ns,
            "SECURITY_DEADLINE",
        )?;
        let run = integer("run")?;
        let attempt = integer("attempt")?;
        require(run > 0 && attempt > 0, "SECURITY_DEADLINE")?;
        let sha = value["receipt_sha"]
            .as_str()
            .ok_or(ScanFailure("SECURITY_DEADLINE"))?;
        require(
            sha.len() == 64
                && sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "SECURITY_DEADLINE",
        )?;
        let mut receipt_sha = [0; 64];
        receipt_sha.copy_from_slice(sha.as_bytes());
        let result = Self {
            end_ns,
            local: local
                .checked_add(Duration::from_nanos(end_ns - now))
                .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
            origin,
            stage_end,
            exported: integer("exported")?,
            run,
            attempt,
            host,
            receipt_dev: integer("receipt_dev")?,
            receipt_ino: integer("receipt_ino")?,
            receipt_sha,
        };
        require(result.value() == *value, "SECURITY_DEADLINE")?;
        result.check()?;
        Ok(result)
    }
}

fn canonical_id(text: &str) -> Result<u64> {
    require(
        !text.is_empty()
            && text.len() <= 20
            && !text.starts_with('0')
            && text.bytes().all(|b| b.is_ascii_digit()),
        "SECURITY_BOOTSTRAP_PATH_INVALID",
    )?;
    text.parse().private("SECURITY_BOOTSTRAP_PATH_INVALID")
}
pub struct Stage {
    directory: File,
    root: PathBuf,
    marker_name: String,
    receipt_name: String,
    marker: File,
    marker_facts: FileFacts,
    marker_bytes: Vec<u8>,
    receipt: File,
    receipt_facts: FileFacts,
    pub deadline: Deadline,
}
struct Paths {
    directory: File,
    root: PathBuf,
    marker: String,
    receipt: String,
    run: u64,
    attempt: u64,
}
impl Paths {
    fn environment() -> Result<Self> {
        let root = PathBuf::from(
            env::var_os("RUNNER_TEMP").ok_or(ScanFailure("SECURITY_BOOTSTRAP_PATH_INVALID"))?,
        );
        let run =
            canonical_id(&env::var("GITHUB_RUN_ID").private("SECURITY_BOOTSTRAP_PATH_INVALID")?)?;
        let attempt = canonical_id(
            &env::var("GITHUB_RUN_ATTEMPT").private("SECURITY_BOOTSTRAP_PATH_INVALID")?,
        )?;
        let directory = directory(&root)?;
        Ok(Self {
            directory,
            root,
            marker: format!("nw-security-bootstrap-{run}-{attempt}.stamp"),
            receipt: format!("nw-security-bootstrap-{run}-{attempt}.budget.json"),
            run,
            attempt,
        })
    }
    fn marker(&self) -> Result<(File, FileFacts, Vec<u8>, u64)> {
        let mut file = open_file(&self.directory, &self.marker, OFlags::RDONLY)?;
        let metadata = facts(&file)?;
        metadata.marker()?;
        let bytes = read_file(&mut file, 128)?;
        metadata.unchanged(facts(&file)?)?;
        let origin = parse_origin(&bytes)?;
        Ok((file, metadata, bytes, origin))
    }
}
pub fn validate_env_bytes(bytes: &[u8]) -> Result<()> {
    require(
        bytes.len() as u64 <= private::MIB && (bytes.is_empty() || bytes.last() == Some(&b'\n')),
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )?;
    let text = std::str::from_utf8(bytes).private("SECURITY_BOOTSTRAP_ENV_INVALID")?;
    require(
        !text.lines().any(|line| line.starts_with(KEY)),
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )
}
pub fn validate_receipt(bytes: &[u8], expected: &Value) -> Result<()> {
    require(
        bytes.len() as u64 <= private::RECORD_LIMIT,
        "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
    )?;
    let _: Value = serde_json::from_slice(bytes).private("SECURITY_BOOTSTRAP_RECEIPT_INVALID")?;
    require(
        serde_json::to_vec(expected).private("SECURITY_BOOTSTRAP_RECEIPT_INVALID")? == bytes,
        "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
    )
}
pub fn export() -> Result<()> {
    let paths = Paths::environment()?;
    let (marker, marker_facts, bytes, origin) = paths.marker()?;
    let host = Host::current()?;
    let local = Instant::now();
    let wall = crate::protocol::unix_millis()?;
    let now = boot_ns()?;
    let end = stage_end(origin)?;
    let exported = evaluate_remaining(origin, end, now, wall)?;
    let local_end = local
        .checked_add(Duration::from_nanos(end - now))
        .ok_or(ScanFailure("SECURITY_DEADLINE"))?;
    let record = json!({"version":1,"run":paths.run,"attempt":paths.attempt,"marker":marker_facts.value(),
        "marker_sha":private::hash(&bytes),"origin":origin,"end":end,"host":host.value(),"exported":exported});
    let record_bytes = serde_json::to_vec(&record).private("SECURITY_BOOTSTRAP_RECEIPT_INVALID")?;
    require(
        record_bytes.len() as u64 <= private::RECORD_LIMIT,
        "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
    )?;
    let env_path = PathBuf::from(
        env::var_os("GITHUB_ENV").ok_or(ScanFailure("SECURITY_BOOTSTRAP_ENV_INVALID"))?,
    );
    require(
        env_path.starts_with(&paths.root) && env_path != paths.root,
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )?;
    let env_parent = directory(
        env_path
            .parent()
            .ok_or(ScanFailure("SECURITY_BOOTSTRAP_ENV_INVALID"))?,
    )?;
    let env_name = env_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(ScanFailure("SECURITY_BOOTSTRAP_ENV_INVALID"))?;
    let mut env_file = open_file(&env_parent, env_name, OFlags::RDWR | OFlags::APPEND)?;
    let before = facts(&env_file)?;
    before.environment()?;
    let old = read_file(&mut env_file, private::MIB)?;
    validate_env_bytes(&old)?;
    before.unchanged(facts(&env_file)?)?;
    let line = format!("{KEY}={exported}\n");
    require(
        old.len() + line.len() <= private::MIB as usize,
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )?;
    evaluate_active(end, boot_ns()?, Instant::now() < local_end)?;
    same_directory(&paths.directory, &paths.root)?;
    marker_facts.unchanged(facts(&marker)?)?;
    marker_facts.unchanged(facts(&open_file(
        &paths.directory,
        &paths.marker,
        OFlags::RDONLY,
    )?)?)?;
    let mut receipt = open_file(
        &paths.directory,
        &paths.receipt,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
    )?;
    receipt
        .write_all(&record_bytes)
        .private("SECURITY_BOOTSTRAP_RECEIPT_INVALID")?;
    receipt
        .sync_all()
        .private("SECURITY_BOOTSTRAP_RECEIPT_INVALID")?;
    let receipt_facts = facts(&receipt)?;
    receipt_facts.receipt()?;
    require(
        receipt_facts.mode & 0o7777 == 0o600 && receipt_facts.len == record_bytes.len() as u64,
        "SECURITY_BOOTSTRAP_RECEIPT_INVALID",
    )?;
    receipt_facts.unchanged(facts(&open_file(
        &paths.directory,
        &paths.receipt,
        OFlags::RDONLY,
    )?)?)?;
    evaluate_active(end, boot_ns()?, Instant::now() < local_end)?;
    before.unchanged(facts(&env_file)?)?;
    before.unchanged(facts(&open_file(&env_parent, env_name, OFlags::RDONLY)?)?)?;
    env_file
        .write_all(line.as_bytes())
        .private("SECURITY_BOOTSTRAP_ENV_INVALID")?;
    env_file
        .sync_all()
        .private("SECURITY_BOOTSTRAP_ENV_INVALID")?;
    let after = facts(&env_file)?;
    require(
        after.dev == before.dev
            && after.ino == before.ino
            && after.uid == before.uid
            && after.mode == before.mode
            && after.links == before.links
            && after.len == before.len + line.len() as u64,
        "SECURITY_BOOTSTRAP_ENV_INVALID",
    )?;
    after.unchanged(facts(&open_file(&env_parent, env_name, OFlags::RDONLY)?)?)?;
    same_directory(
        &env_parent,
        env_path
            .parent()
            .ok_or(ScanFailure("SECURITY_BOOTSTRAP_ENV_INVALID"))?,
    )?;
    same_directory(&paths.directory, &paths.root)?;
    receipt_facts.unchanged(facts(&receipt)?)?;
    receipt_facts.unchanged(facts(&open_file(
        &paths.directory,
        &paths.receipt,
        OFlags::RDONLY,
    )?)?)?;
    evaluate_active(end, boot_ns()?, Instant::now() < local_end)?;
    eprintln!(
        "{} [INFO] SECURITY_BOOTSTRAP_BUDGET_EXPORTED marker_mode={} env_mode={} owner_valid=true regular=true links_valid=true append=true",
        crate::protocol::unix_millis().unwrap_or(0),
        marker_facts.mode & 0o7777,
        before.mode & 0o7777
    );
    Ok(())
}
impl Stage {
    pub fn admit() -> Result<Self> {
        let paths = Paths::environment()?;
        let (marker, marker_facts, marker_bytes, origin) = paths.marker()?;
        let mut receipt = open_file(&paths.directory, &paths.receipt, OFlags::RDONLY)?;
        let receipt_facts = facts(&receipt)?;
        receipt_facts.receipt()?;
        let bytes = read_file(&mut receipt, private::RECORD_LIMIT)?;
        receipt_facts.unchanged(facts(&receipt)?)?;
        let value: Value =
            serde_json::from_slice(&bytes).private("SECURITY_BOOTSTRAP_RECEIPT_INVALID")?;
        let host = Host::current()?;
        let end = stage_end(origin)?;
        let exported = value["exported"]
            .as_u64()
            .ok_or(ScanFailure("SECURITY_BOOTSTRAP_RECEIPT_INVALID"))?;
        let expected = json!({"version":1,"run":paths.run,"attempt":paths.attempt,"marker":marker_facts.value(),
            "marker_sha":private::hash(&marker_bytes),"origin":origin,"end":end,"host":host.value(),"exported":exported});
        validate_receipt(&bytes, &expected)?;
        exact_export(
            &env::var(KEY).private("SECURITY_BOOTSTRAP_ENV_INVALID")?,
            exported,
        )?;
        let local = Instant::now();
        let now = boot_ns()?;
        require(now >= origin && now < end, "SECURITY_DEADLINE")?;
        let mut receipt_sha = [0; 64];
        receipt_sha.copy_from_slice(private::hash(&bytes).as_bytes());
        let deadline = Deadline {
            end_ns: end,
            local: local
                .checked_add(Duration::from_nanos(end - now))
                .ok_or(ScanFailure("SECURITY_DEADLINE"))?,
            origin,
            stage_end: end,
            exported,
            run: paths.run,
            attempt: paths.attempt,
            host,
            receipt_dev: receipt_facts.dev,
            receipt_ino: receipt_facts.ino,
            receipt_sha,
        };
        deadline.check()?;
        let stage = Self {
            directory: paths.directory,
            root: paths.root,
            marker_name: paths.marker,
            receipt_name: paths.receipt,
            marker,
            marker_facts,
            marker_bytes,
            receipt,
            receipt_facts,
            deadline,
        };
        stage.revalidate()?;
        stage.deadline.check()?;
        Ok(stage)
    }
    pub fn fixture_copy(&self, root: &Path) -> Result<()> {
        self.deadline.check()?;
        self.revalidate()?;
        let dir = directory(root)?;
        let mut file = open_file(
            &dir,
            &self.marker_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
        )?;
        file.write_all(&self.marker_bytes)
            .private("SECURITY_BOOTSTRAP_FILE_INVALID")?;
        file.sync_all().private("SECURITY_BOOTSTRAP_FILE_INVALID")?;
        require(
            fixture_read_marker(&root.join(&self.marker_name))? == self.marker_bytes,
            "SECURITY_BOOTSTRAP_FILE_CHANGED",
        )?;
        self.deadline.check()
    }
    pub fn configure(&self, command: &mut std::process::Command, root: &Path) {
        command
            .env("RUNNER_TEMP", root)
            .env("GITHUB_RUN_ID", self.deadline.run.to_string())
            .env("GITHUB_RUN_ATTEMPT", self.deadline.attempt.to_string());
    }
    pub fn configure_original(&self, command: &mut std::process::Command) {
        self.configure(command, &self.root);
        command.env(KEY, self.deadline.exported.to_string());
    }
    fn revalidate(&self) -> Result<()> {
        same_directory(&self.directory, &self.root)?;
        self.marker_facts.unchanged(facts(&self.marker)?)?;
        self.receipt_facts.unchanged(facts(&self.receipt)?)?;
        self.marker_facts.unchanged(facts(&open_file(
            &self.directory,
            &self.marker_name,
            OFlags::RDONLY,
        )?)?)?;
        self.receipt_facts.unchanged(facts(&open_file(
            &self.directory,
            &self.receipt_name,
            OFlags::RDONLY,
        )?)?)
    }
    pub fn consume(self, barrier: &crate::process::NoChildren) -> Result<()> {
        self.deadline.check()?;
        barrier.recheck()?;
        self.revalidate()?;
        rustix::fs::unlinkat(
            &self.directory,
            self.receipt_name.as_str(),
            rustix::fs::AtFlags::empty(),
        )
        .private("SECURITY_BOOTSTRAP_CLEANUP_FAILED")?;
        self.deadline.check()?;
        rustix::fs::unlinkat(
            &self.directory,
            self.marker_name.as_str(),
            rustix::fs::AtFlags::empty(),
        )
        .private("SECURITY_BOOTSTRAP_CLEANUP_FAILED")?;
        self.deadline.check()
    }
    pub fn verify_consumed(&self) -> Result<()> {
        self.verify_absent(&self.directory)
    }
    pub fn verify_fixture_consumed(&self, root: &Path) -> Result<()> {
        self.verify_absent(&directory(root)?)
    }
    fn verify_absent(&self, directory: &File) -> Result<()> {
        self.deadline.check()?;
        for name in [&self.marker_name, &self.receipt_name] {
            require(
                matches!(
                    rustix::fs::statat(
                        directory,
                        name.as_str(),
                        rustix::fs::AtFlags::SYMLINK_NOFOLLOW
                    ),
                    Err(rustix::io::Errno::NOENT)
                ),
                "SECURITY_BOOTSTRAP_CLEANUP_FAILED",
            )?;
        }
        Ok(())
    }
}
