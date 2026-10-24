//! One protected writer, finite records/queue/rotation, and no protocol-stdout output.
use crate::{
    files::{self, RootLock},
    security,
};
use nw_domain::{DomainError, Result, commands::decode_json, diagnostics::LogStatus};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use tracing_subscriber::fmt::MakeWriter;

const RECORD_LIMIT: usize = 16 * 1024;
const QUEUE_LIMIT: usize = 32;
const FILE_LIMIT: u64 = 2 * 1024 * 1024;
const FILE_COUNT: usize = 5;
static ACTIVE: OnceLock<Sink> = OnceLock::new();
enum Packet {
    Record(Vec<u8>),
    Flush(mpsc::Sender<bool>),
}
#[derive(Default)]
struct Counters {
    dropped: AtomicU64,
    omitted: AtomicU64,
    failed: AtomicU64,
    healthy: AtomicBool,
}
struct Sink {
    root: PathBuf,
    sender: mpsc::SyncSender<Packet>,
    counters: Arc<Counters>,
    identity: Mutex<Option<(String, String)>>,
}
struct Rotation {
    root: PathBuf,
    marker: String,
    current: Option<File>,
    bytes: u64,
    _lock: RootLock,
}
fn log_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::new("LOG_STORAGE_ERROR", error.to_string())
}
fn unowned() -> DomainError {
    DomainError::new(
        "LOG_FILE_UNOWNED",
        "A reserved log filename has no matching protected ownership header; it was preserved",
    )
}
impl Rotation {
    fn path(&self, slot: usize) -> PathBuf {
        self.root.join(if slot == 0 {
            "owner.jsonl".into()
        } else {
            format!("owner.{slot}.jsonl")
        })
    }
    fn validate(&self, path: &Path) -> Result<bool> {
        if !path.try_exists().map_err(log_error)? {
            return Ok(false);
        }
        let bytes = security::read_private(path, FILE_LIMIT + RECORD_LIMIT as u64 + 1024)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .filter(|end| *end <= 1024)
            .ok_or_else(unowned)?;
        let header = decode_json(&bytes[..end]).map_err(|_| unowned())?;
        if header["kind"] != "numera_log_header"
            || header["schema_version"] != 1
            || header["owner_nonce"].as_str() != Some(self.marker.as_str())
        {
            return Err(unowned());
        }
        Ok(true)
    }
    fn open(root: &Path) -> Result<Self> {
        let root = security::private_directory(root)?;
        let lock = RootLock::acquire(&root)?;
        let marker_path = root.join("owner.marker");
        let marker = if marker_path.try_exists().map_err(log_error)? {
            let bytes = security::read_private(&marker_path, 64)?;
            let marker = String::from_utf8(bytes).map_err(log_error)?;
            if marker.len() != 64
                || !marker
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err(unowned());
            }
            marker
        } else {
            let nonce = tempfile::Builder::new()
                .prefix(".identity-")
                .rand_bytes(32)
                .tempfile_in(&root)
                .map_err(log_error)?;
            let marker = hex::encode(Sha256::digest(files::path_text(nonce.path())?.as_bytes()));
            security::write_private(&marker_path, marker.as_bytes(), false)?;
            marker
        };
        let mut writer = Self {
            root,
            marker,
            current: None,
            bytes: 0,
            _lock: lock,
        };
        for slot in 0..FILE_COUNT {
            writer.validate(&writer.path(slot))?;
        }
        // A crash between link and unlink can leave two archive names for one owned inode.
        for older in 1..FILE_COUNT {
            if !writer.path(older).try_exists().map_err(log_error)? {
                continue;
            }
            for newer in 0..older {
                if writer.path(newer).try_exists().map_err(log_error)?
                    && files::same_object(&writer.path(newer), &writer.path(older))?
                {
                    fs::remove_file(writer.path(older)).map_err(log_error)?;
                    break;
                }
            }
        }
        let active = writer.path(0);
        if writer.validate(&active)? {
            let mut file = OpenOptions::new()
                .read(true)
                .append(true)
                .open(&active)
                .map_err(log_error)?;
            let bytes = file.metadata().map_err(log_error)?.len();
            let mut last = [0u8; 1];
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::End(-1)).map_err(log_error)?;
            file.read_exact(&mut last).map_err(log_error)?;
            writer.bytes = bytes;
            writer.current = Some(file);
            if last != *b"\n" {
                writer.rotate()?;
            }
        } else {
            writer.create()?;
        }
        Ok(writer)
    }
    fn create(&mut self) -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(self.path(0)).map_err(log_error)?;
        let header = format!(
            "{{\"timestamp\":\"{}\",\"level\":\"INFO\",\"kind\":\"numera_log_header\",\"schema_version\":1,\"owner_nonce\":\"{}\"}}\n",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            self.marker
        );
        file.write_all(header.as_bytes()).map_err(log_error)?;
        file.sync_all().map_err(log_error)?;
        self.bytes = header.len() as u64;
        self.current = Some(file);
        Ok(())
    }
    fn rotate(&mut self) -> Result<()> {
        for slot in 0..FILE_COUNT {
            self.validate(&self.path(slot))?;
        }
        self.current.take();
        let oldest = self.path(FILE_COUNT - 1);
        if oldest.try_exists().map_err(log_error)? {
            fs::remove_file(&oldest).map_err(log_error)?;
        }
        for slot in (0..FILE_COUNT - 1).rev() {
            let path = self.path(slot);
            if path.try_exists().map_err(log_error)? {
                // Link creation refuses a racing destination instead of replacing it.
                fs::hard_link(&path, self.path(slot + 1)).map_err(log_error)?;
                fs::remove_file(path).map_err(log_error)?;
            }
        }
        self.create()
    }
    fn append(&mut self, record: &[u8]) -> Result<()> {
        if record.len() > RECORD_LIMIT {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Structured log record exceeds 16 KiB",
            ));
        }
        if self.bytes + record.len() as u64 > FILE_LIMIT {
            self.rotate()?;
        }
        self.current
            .as_mut()
            .ok_or_else(|| log_error("Log file is closed"))?
            .write_all(record)
            .map_err(log_error)?;
        self.bytes += record.len() as u64;
        Ok(())
    }
    fn flush(&mut self) -> bool {
        self.current
            .as_mut()
            .is_some_and(|file| file.sync_all().is_ok())
    }
}

pub fn activate(root: &Path) -> Result<()> {
    let selected = root.join("logs");
    if let Some(active) = ACTIVE.get() {
        if active.root == selected {
            return Ok(());
        }
        return Err(DomainError::new(
            "OWNER_IDENTITY_CONFLICT",
            "One process cannot attach its log writer to another root",
        ));
    }
    let mut rotation = Rotation::open(&selected)?;
    let directory = rotation.root.clone();
    let counters = Arc::new(Counters::default());
    counters.healthy.store(true, Ordering::Release);
    let worker_counters = counters.clone();
    let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
    std::thread::Builder::new().name("numera-log-writer".into()).spawn(move || {
        while let Ok(packet) = receiver.recv() {
            match packet {
                Packet::Record(bytes) => if rotation.append(&bytes).is_err() {
                    worker_counters.failed.fetch_add(1, Ordering::AcqRel);
                    worker_counters.dropped.fetch_add(1, Ordering::AcqRel);
                    for packet in receiver.try_iter() { if matches!(packet, Packet::Record(_)) { worker_counters.dropped.fetch_add(1, Ordering::AcqRel); } }
                    worker_counters.healthy.store(false, Ordering::Release);
                    eprintln!("[{}] [WARNING] Structured log writer failed; query logs.status. Authoritative job facts remain in the store.", chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                    break;
                },
                Packet::Flush(ack) => { let _ = ack.send(rotation.flush()); }
            }
        }
    }).map_err(log_error)?;
    ACTIVE
        .set(Sink {
            root: directory,
            sender,
            counters,
            identity: Mutex::new(None),
        })
        .map_err(|_| log_error("Log writer was initialized concurrently"))?;
    Ok(())
}
pub fn bind_owner(root_id: &str, epoch: &str) -> Result<()> {
    if [root_id, epoch].iter().any(|value| {
        value.len() > 128
            || value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err(DomainError::new(
            "INVALID_IDENTITY",
            "Owner logging identity is not a bounded engine identifier",
        ));
    }
    if let Some(sink) = ACTIVE.get()
        && let Ok(mut identity) = sink.identity.lock()
    {
        *identity = Some((root_id.to_owned(), epoch.to_owned()));
    }
    Ok(())
}
pub fn flush() -> bool {
    let Some(sink) = ACTIVE.get() else {
        return true;
    };
    let (sender, receiver) = mpsc::channel();
    let confirmed = sink.sender.try_send(Packet::Flush(sender)).is_ok()
        && receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or(false);
    if !confirmed {
        eprintln!(
            "[{}] [WARNING] Protected log flush could not be confirmed",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        );
    }
    confirmed
}
pub fn status() -> LogStatus {
    let active = ACTIVE.get();
    LogStatus {
        configured: active.is_some(),
        writer_healthy: active.is_some_and(|sink| sink.counters.healthy.load(Ordering::Acquire)),
        directory: active.and_then(|sink| files::path_text(&sink.root).ok()),
        file_byte_limit: FILE_LIMIT.to_string(),
        retained_file_limit: FILE_COUNT,
        record_byte_limit: RECORD_LIMIT.to_string(),
        queue_record_limit: QUEUE_LIMIT,
        dropped_records: active
            .map_or(0, |sink| sink.counters.dropped.load(Ordering::Acquire))
            .to_string(),
        omitted_records: active
            .map_or(0, |sink| sink.counters.omitted.load(Ordering::Acquire))
            .to_string(),
        write_failures: active
            .map_or(0, |sink| sink.counters.failed.load(Ordering::Acquire))
            .to_string(),
    }
}
pub(super) fn sensitive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "bearer"
            | "token"
            | "password"
            | "authorization"
            | "credential"
            | "replay_scope"
            | "idempotency_key"
            | "headers"
            | "request"
            | "response"
            | "params"
            | "context"
            | "error"
    )
}
fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut record = decode_json(bytes)?;
    if !record.is_object() {
        return Err(log_error("Structured event root must be an object"));
    }
    let hidden = decode_json(br#""<redacted>""#)?;
    if let Some(fields) = record["fields"].as_object_mut() {
        for (name, value) in fields {
            if sensitive(name) {
                *value = hidden.clone();
            }
        }
    }
    record["timestamp"] = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .into();
    record["level"] = match record["level"].as_str() {
        Some("WARN" | "WARNING") => "WARNING".into(),
        Some("TRACE") => "DEBUG".into(),
        Some("ERROR" | "INFO" | "DEBUG") => record["level"].clone(),
        _ => {
            return Err(log_error(
                "Structured event has no admitted primary severity",
            ));
        }
    };
    record["process_id"] = std::process::id().into();
    if let Some(sink) = ACTIVE.get()
        && let Ok(identity) = sink.identity.lock()
        && let Some((root, epoch)) = identity.as_ref()
    {
        record["root_id"] = root.as_str().into();
        record["owner_epoch"] = epoch.as_str().into();
    }
    let mut encoded = record.to_string().into_bytes();
    encoded.push(b'\n');
    if encoded.len() > RECORD_LIMIT {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Normalized structured record exceeds its limit",
        ));
    }
    Ok(encoded)
}
#[derive(Clone, Copy)]
pub struct JsonWriter;
pub struct RecordBuffer {
    bytes: Vec<u8>,
    overflow: bool,
    enabled: bool,
}
impl<'a> MakeWriter<'a> for JsonWriter {
    type Writer = RecordBuffer;
    fn make_writer(&'a self) -> Self::Writer {
        RecordBuffer {
            bytes: Vec::new(),
            overflow: false,
            enabled: ACTIVE.get().is_some(),
        }
    }
}
impl Write for RecordBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.enabled {
            let remaining = RECORD_LIMIT.saturating_sub(self.bytes.len());
            self.bytes
                .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
            self.overflow |= bytes.len() > remaining;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Drop for RecordBuffer {
    fn drop(&mut self) {
        let Some(sink) = ACTIVE.get() else {
            return;
        };
        if !self.enabled || self.bytes.is_empty() {
            return;
        }
        let normalized = if self.overflow {
            Err(DomainError::new("RESOURCE_LIMIT", "Oversized log record"))
        } else {
            normalize(&self.bytes)
        };
        let bytes = match normalized {
            Ok(bytes) => bytes,
            Err(_) => {
                sink.counters.omitted.fetch_add(1, Ordering::AcqRel);
                normalize(br#"{"level":"WARN","fields":{"message":"Structured log record omitted because its size or encoding exceeded the admitted budget"},"target":"nw_platform::logging"}"#).unwrap_or_default()
            }
        };
        if sink.sender.try_send(Packet::Record(bytes)).is_err() {
            sink.counters.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_records_escape_text_redact_secrets_and_keep_safe_identifiers() {
        let encoded = normalize(r#"{"level":"WARN","fields":{"message":"quoted \"text\"\nand Unicode سلام","bearer":"unit-secret","replay_scope":"private-scope","token_id":"safe-id","error":"private-error"},"target":"nw_platform::test"}"#.as_bytes()).unwrap();
        let row = decode_json(&encoded).unwrap();
        assert_eq!(row["level"], "WARNING");
        assert_eq!(row["fields"]["bearer"], "<redacted>");
        assert_eq!(row["fields"]["replay_scope"], "<redacted>");
        assert_eq!(row["fields"]["error"], "<redacted>");
        assert_eq!(row["fields"]["token_id"], "safe-id");
        assert!(row["fields"]["message"].as_str().unwrap().contains('\n'));
        assert_eq!(encoded.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert!(!String::from_utf8(encoded).unwrap().contains("unit-secret"));
        assert!(normalize(b"[]").is_err());
        assert!(normalize(br#"{"level":"INVALID"}"#).is_err());
    }
    #[test]
    fn structured_capture_is_bounded_before_json_parsing() {
        let mut buffer = RecordBuffer {
            bytes: Vec::new(),
            overflow: false,
            enabled: true,
        };
        buffer.write_all(&vec![b'x'; RECORD_LIMIT * 4]).unwrap();
        assert_eq!(buffer.bytes.len(), RECORD_LIMIT);
        assert!(buffer.overflow);
    }
    #[test]
    fn rotation_keeps_only_the_five_owned_files_and_preserves_an_unowned_collision() {
        let root = tempfile::tempdir().unwrap();
        let mut writer = Rotation::open(&root.path().join("logs")).unwrap();
        for ordinal in 0..6 {
            writer
                .append(format!("{{\"ordinal\":{ordinal}}}\n").as_bytes())
                .unwrap();
            writer.rotate().unwrap();
        }
        assert_eq!(
            fs::read_dir(&writer.root)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl"))
                .count(),
            FILE_COUNT
        );
        for slot in 0..FILE_COUNT {
            let bytes = security::read_private(&writer.path(slot), FILE_LIMIT).unwrap();
            for row in bytes
                .split(|byte| *byte == b'\n')
                .filter(|row| !row.is_empty())
            {
                decode_json(row).unwrap();
            }
        }
        let retained = writer.path(FILE_COUNT - 1);
        fs::write(
            &retained,
            b"User data at a reserved name must not be discarded",
        )
        .unwrap();
        assert_eq!(writer.rotate().unwrap_err().code, "LOG_FILE_UNOWNED");
        assert_eq!(
            fs::read(&retained).unwrap(),
            b"User data at a reserved name must not be discarded"
        );
    }
    #[test]
    fn interrupted_tail_is_archived_instead_of_being_spliced_into_a_new_record() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("logs");
        let mut writer = Rotation::open(&selected).unwrap();
        writer.append(b"{\"interrupted\":").unwrap();
        assert!(writer.flush());
        drop(writer);
        let writer = Rotation::open(&selected).unwrap();
        assert!(
            fs::read(writer.path(1))
                .unwrap()
                .ends_with(b"{\"interrupted\":")
        );
        let active = fs::read(writer.path(0)).unwrap();
        assert!(active.ends_with(b"\n"));
        decode_json(&active).unwrap();
    }
    #[test]
    fn interrupted_link_rotation_recovers_without_appending_to_two_archive_names() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("logs");
        let mut writer = Rotation::open(&selected).unwrap();
        writer
            .append(b"{\"message\":\"retained diagnostic\"}\n")
            .unwrap();
        assert!(writer.flush());
        fs::hard_link(writer.path(0), writer.path(1)).unwrap();
        drop(writer);
        let mut writer = Rotation::open(&selected).unwrap();
        assert!(!writer.path(1).exists());
        assert!(
            String::from_utf8(fs::read(writer.path(0)).unwrap())
                .unwrap()
                .contains("retained diagnostic")
        );
        writer.rotate().unwrap();
        assert!(writer.path(1).exists());
    }
}
