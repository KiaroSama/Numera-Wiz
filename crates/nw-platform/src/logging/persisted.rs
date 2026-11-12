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
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
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
    control: Arc<Control>,
    counters: Arc<Counters>,
    identity: Mutex<Option<(String, String)>>,
}
struct Control {
    state: Mutex<Lifecycle>,
    completed: Condvar,
    counters: Arc<Counters>,
}
struct Lifecycle {
    sender: Option<mpsc::SyncSender<Packet>>,
    close_deadline: Option<Instant>,
    rotation_closed: bool,
    completed_at: Option<Instant>,
    write_healthy: bool,
    scope_taken: bool,
    keepalive: Option<Box<dyn Send>>,
}

/// A resource-release proof is separate from the writer's final I/O health.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerLoggingClose {
    pub resources_closed: bool,
    pub writer_healthy: bool,
    pub confirmed_in_time: bool,
}
impl OwnerLoggingClose {
    pub fn is_clean(self) -> bool {
        self.resources_closed && self.writer_healthy && self.confirmed_in_time
    }
}

/// Owner-only close guard. Drop disconnects admission without waiting for I/O.
pub struct OwnerLoggingScope {
    control: Arc<Control>,
}
/// One non-cloneable keepalive registration token, consumed after service open.
pub struct OwnerLoggingBinding {
    control: Arc<Control>,
}
pub fn owner_scope() -> Result<(OwnerLoggingScope, OwnerLoggingBinding)> {
    ACTIVE
        .get()
        .ok_or_else(handoff_error)?
        .control
        .owner_scope()
}
fn handoff_error() -> DomainError {
    DomainError::new(
        "LOG_STORAGE_ERROR",
        "Protected owner log handoff could not be confirmed",
    )
}
impl OwnerLoggingScope {
    pub fn finish(&self) -> OwnerLoggingClose {
        self.control.wait_until(self.control.begin_close())
    }
}
impl Drop for OwnerLoggingScope {
    fn drop(&mut self) {
        self.control.begin_close();
    }
}
impl OwnerLoggingBinding {
    /// Always stores ownership or observes actual Rotation drop. False rejects
    /// startup; it is NOT a resource-release proof. While Rotation is alive,
    /// even a closing sink retains the passed ownership until finalization.
    pub fn bind_keepalive<T: Send + 'static>(self, keepalive: T) -> bool {
        let mut state = self.control.lock();
        if state.rotation_closed {
            drop(state);
            drop(keepalive);
            false
        } else {
            let open = state.sender.is_some() && state.close_deadline.is_none();
            // There is exactly one token: no previous opaque value can be
            // overwritten or destroyed while the control mutex is held.
            state.keepalive = Some(Box::new(keepalive));
            open
        }
    }
}
impl Control {
    fn lock(&self) -> std::sync::MutexGuard<'_, Lifecycle> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn owner_scope(self: &Arc<Self>) -> Result<(OwnerLoggingScope, OwnerLoggingBinding)> {
        let mut state = self.lock();
        if state.scope_taken {
            return Err(handoff_error());
        }
        state.scope_taken = true;
        Ok((
            OwnerLoggingScope { control: self.clone() },
            OwnerLoggingBinding { control: self.clone() },
        ))
    }
    fn is_open(&self) -> bool {
        let state = self.lock();
        state.sender.is_some() && state.close_deadline.is_none()
    }
    fn admit(&self, packet: Packet) -> bool {
        let outcome = {
            let state = self.lock();
            match &state.sender {
                Some(sender) => sender.try_send(packet),
                None => Err(mpsc::TrySendError::Disconnected(packet)),
            }
        };
        // Rejected packet storage/ack senders are dropped outside the mutex.
        outcome.is_ok()
    }
    fn begin_close(&self) -> Instant {
        let (deadline, sender) = {
            let mut state = self.lock();
            let deadline = *state
                .close_deadline
                .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
            self.counters.healthy.store(false, Ordering::Release);
            (deadline, state.sender.take())
        };
        // This is the only admitted SyncSender. Disconnection works with a full
        // queue and wakes the same worker after it drains all admitted packets.
        drop(sender);
        deadline
    }
    fn wait_until(&self, deadline: Instant) -> OwnerLoggingClose {
        let mut state = self.lock();
        while state.completed_at.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            // Condvar releases the mutex for the entire wait.
            let (next, _) = self
                .completed
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
        }
        OwnerLoggingClose {
            resources_closed: state.completed_at.is_some() && state.rotation_closed,
            writer_healthy: state.write_healthy,
            confirmed_in_time: state
                .completed_at
                .zip(state.close_deadline)
                .is_some_and(|(completed, deadline)| completed <= deadline),
        }
    }
    fn flush(&self) -> bool {
        let flush_deadline = Instant::now() + Duration::from_secs(2);
        let (sender, receiver) = mpsc::channel();
        let state = self.lock();
        if let Some(deadline) = state.close_deadline {
            drop(state);
            return self.wait_until(deadline).is_clean();
        }
        let admission = match &state.sender {
            Some(admitted) => admitted.try_send(Packet::Flush(sender)),
            None => Err(mpsc::TrySendError::Disconnected(Packet::Flush(sender))),
        };
        drop(state);
        admission.is_ok()
            && receiver
                .recv_timeout(flush_deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(false)
    }
}

// Rotation is owned by this finalizer before the worker starts. Every exit,
// including unwind and failed thread startup, closes it before the keepalive.
struct Worker {
    rotation: Option<Rotation>,
    control: Arc<Control>,
    clean: bool,
}
struct Completed {
    control: Arc<Control>,
    clean: bool,
}
impl Drop for Completed {
    fn drop(&mut self) {
        {
            let mut state = self.control.lock();
            state.write_healthy = self.clean && !std::thread::panicking();
            state.completed_at = Some(Instant::now());
        }
        self.control.completed.notify_all();
    }
}
impl Worker {
    fn prepare(rotation: Rotation) -> (Self, mpsc::Receiver<Packet>) {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let counters = Arc::new(Counters::default());
        counters.healthy.store(true, Ordering::Release);
        let control = Arc::new(Control {
            state: Mutex::new(Lifecycle {
                sender: Some(sender),
                close_deadline: None,
                rotation_closed: false,
                completed_at: None,
                write_healthy: false,
                scope_taken: false,
                keepalive: None,
            }),
            completed: Condvar::new(),
            counters,
        });
        (
            Self { rotation: Some(rotation), control, clean: false },
            receiver,
        )
    }
    fn run(mut self, receiver: mpsc::Receiver<Packet>) {
        let mut write_healthy = true;
        while let Ok(packet) = receiver.recv() {
            match packet {
                Packet::Record(bytes) => {
                    if self.rotation.as_mut().expect("owned rotation").append(&bytes).is_err() {
                        self.control.counters.failed.fetch_add(1, Ordering::AcqRel);
                        self.control.counters.dropped.fetch_add(1, Ordering::AcqRel);
                        self.control.begin_close();
                        for packet in receiver.try_iter() {
                            if matches!(packet, Packet::Record(_)) {
                                self.control.counters.dropped.fetch_add(1, Ordering::AcqRel);
                            }
                        }
                        write_healthy = false;
                        eprintln!("[{}] [WARNING] Structured log writer failed; query logs.status. Authoritative job facts remain in the store.", chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                        break;
                    }
                }
                Packet::Flush(ack) => {
                    let confirmed = self.rotation.as_mut().expect("owned rotation").flush();
                    write_healthy &= confirmed;
                    let _ = ack.send(confirmed);
                }
            }
        }
        let flushed = self.rotation.as_mut().expect("owned rotation").flush();
        self.clean = write_healthy && flushed;
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.control.begin_close();
        drop(self.rotation.take());
        let keepalive = {
            let mut state = self.control.lock();
            state.rotation_closed = true;
            state.keepalive.take()
        };
        // Even a panicking opaque destructor cannot skip completion publication.
        // No control lock spans Rotation I/O, waits, or opaque Drop.
        let completed = Completed { control: self.control.clone(), clean: self.clean };
        drop(keepalive);
        drop(completed);
    }
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
            return if active.control.is_open() && active.counters.healthy.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(handoff_error())
            };
        }
        return Err(DomainError::new(
            "OWNER_IDENTITY_CONFLICT",
            "One process cannot attach its log writer to another root",
        ));
    }
    let rotation = Rotation::open(&selected)?;
    let directory = rotation.root.clone();
    let (worker, receiver) = Worker::prepare(rotation);
    let control = worker.control.clone();
    let counters = control.counters.clone();
    std::thread::Builder::new()
        .name("numera-log-writer".into())
        .spawn(move || worker.run(receiver))
        .map_err(log_error)?;
    if ACTIVE
        .set(Sink {
            root: directory,
            control: control.clone(),
            counters,
            identity: Mutex::new(None),
        })
        .is_err()
    {
        control.begin_close();
        return Err(log_error("Log writer was initialized concurrently"));
    }
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
    let confirmed = sink.control.flush();
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
            enabled: ACTIVE.get().is_some_and(|sink| sink.control.is_open()),
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
        if !sink.control.admit(Packet::Record(bytes)) {
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

        owner_log_handoff_cases();
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

    struct TestFence {
        state: Option<RootLock>,
        logs: PathBuf,
        observed: mpsc::Sender<bool>,
    }
    impl Drop for TestFence {
        fn drop(&mut self) {
            // Observe the real log lease before releasing our real state lease.
            let log_was_released = RootLock::acquire(&self.logs).is_ok();
            drop(self.state.take());
            let _ = self.observed.send(log_was_released);
        }
    }
    struct WorkerGate(Option<mpsc::Sender<()>>);
    impl WorkerGate {
        fn release(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    impl Drop for WorkerGate {
        fn drop(&mut self) {
            self.release();
        }
    }
    fn local_worker(rotation: Rotation, unwind: bool) -> (Arc<Control>, WorkerGate) {
        let (worker, receiver) = Worker::prepare(rotation);
        let control = worker.control.clone();
        let (release, gate) = mpsc::channel();
        std::thread::Builder::new()
            .name("numera-local-log-writer".into())
            .spawn(move || {
                let worker = worker;
                gate.recv_timeout(Duration::from_secs(5))
                    .expect("Local worker gate watchdog");
                assert!(!unwind, "Controlled local worker unwind");
                worker.run(receiver);
            })
            .unwrap();
        (control, WorkerGate(Some(release)))
    }
    fn test_fence(root: &Path) -> (TestFence, mpsc::Receiver<bool>) {
        let state = security::private_directory(&root.join("state")).unwrap();
        let (observed, receiver) = mpsc::channel();
        (
            TestFence {
                state: Some(RootLock::acquire(&state).unwrap()),
                logs: root.join("logs"),
                observed,
            },
            receiver,
        )
    }
    fn assert_busy(path: &Path) {
        match RootLock::acquire(path) {
            Ok(_) => panic!("A local ownership fence was released too early"),
            Err(error) => assert_eq!(error.code, "OWNER_BUSY"),
        }
    }
    fn assert_ordered_release(root: &Path, observed: mpsc::Receiver<bool>) {
        assert!(observed.recv_timeout(Duration::from_secs(5)).unwrap());
        drop(RootLock::acquire(&root.join("logs")).unwrap());
        drop(RootLock::acquire(&root.join("state")).unwrap());
    }
    fn owner_log_handoff_cases() {
        // Local instances use the production worker/control, with no ACTIVE reset.
        let root = tempfile::tempdir().unwrap();
        let rotation = Rotation::open(&root.path().join("logs")).unwrap();
        let before = fs::read(rotation.path(0)).unwrap();
        let (control, mut gate) = local_worker(rotation, false);
        let (scope, binding) = control.owner_scope().unwrap();
        let (fence, observed) = test_fence(root.path());
        assert!(binding.bind_keepalive(fence));
        let record = b"{\"message\":\"before flush\"}\n";
        assert!(control.admit(Packet::Record(record.to_vec())));
        gate.release();
        assert!(control.flush());
        assert_busy(&root.path().join("logs"));
        assert_busy(&root.path().join("state"));
        let mut expected = before;
        expected.extend_from_slice(record);
        assert!(
            fs::read(root.path().join("logs/owner.jsonl")).unwrap() == expected,
            "Admitted local log bytes must be retained exactly"
        );
        let closed = scope.finish();
        assert!(closed.is_clean());
        assert_ordered_release(root.path(), observed);
        assert_eq!(scope.finish(), closed);
        assert!(!control.admit(Packet::Record(b"after close\n".to_vec())));

        let root = tempfile::tempdir().unwrap();
        let rotation = Rotation::open(&root.path().join("logs")).unwrap();
        let mut expected = fs::read(rotation.path(0)).unwrap();
        let (control, mut gate) = local_worker(rotation, false);
        let (scope, binding) = control.owner_scope().unwrap();
        let (fence, observed) = test_fence(root.path());
        assert!(binding.bind_keepalive(fence));
        for ordinal in 0..QUEUE_LIMIT {
            let bytes = format!("{{\"queued\":{ordinal}}}\n").into_bytes();
            expected.extend_from_slice(&bytes);
            assert!(control.admit(Packet::Record(bytes)));
        }
        assert!(!control.admit(Packet::Record(b"queue overflow\n".to_vec())));
        let deadline = control.begin_close();
        assert_eq!(control.begin_close(), deadline);
        assert!(!control.wait_until(std::time::Instant::now()).resources_closed);
        assert_busy(&root.path().join("logs"));
        assert_busy(&root.path().join("state"));
        assert!(!control.admit(Packet::Record(b"closed admission\n".to_vec())));
        gate.release();
        let closed = scope.finish();
        assert!(closed.is_clean());
        assert_eq!(control.begin_close(), deadline);
        assert!(control.flush());
        assert_eq!(control.begin_close(), deadline);
        assert!(
            fs::read(root.path().join("logs/owner.jsonl")).unwrap() == expected,
            "A full queue must drain its exact admitted bytes"
        );
        assert_ordered_release(root.path(), observed);

        // Early caller Drop must close admission without releasing the service
        // fence. A late token must also store it while Rotation remains alive.
        for bind_before_close in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let rotation = Rotation::open(&root.path().join("logs")).unwrap();
            let (control, mut gate) = local_worker(rotation, false);
            let (scope, binding) = control.owner_scope().unwrap();
            let mut binding = Some(binding);
            let (fence, observed) = test_fence(root.path());
            let mut fence = Some(fence);
            if bind_before_close {
                assert!(binding.take().unwrap().bind_keepalive(fence.take().unwrap()));
            }
            drop(scope);
            if !bind_before_close {
                assert!(!binding.take().unwrap().bind_keepalive(fence.take().unwrap()));
            }
            assert_busy(&root.path().join("logs"));
            assert_busy(&root.path().join("state"));
            assert!(!control.admit(Packet::Record(b"after caller Drop\n".to_vec())));
            gate.release();
            let closed = control.wait_until(control.begin_close());
            assert!(closed.is_clean());
            assert_ordered_release(root.path(), observed);
        }

        // Binding races only with close, never with a second authority slot.
        let root = tempfile::tempdir().unwrap();
        let rotation = Rotation::open(&root.path().join("logs")).unwrap();
        let (control, mut gate) = local_worker(rotation, false);
        let (scope, binding) = control.owner_scope().unwrap();
        let (fence, observed) = test_fence(root.path());
        let closer = control.clone();
        let (sent, completed) = mpsc::channel();
        std::thread::spawn(move || {
            let deadline = closer.begin_close();
            let _ = sent.send(deadline);
        });
        let _was_open = binding.bind_keepalive(fence);
        let deadline = completed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(control.owner_scope().is_err(), "Only one binding token may exist");
        assert_busy(&root.path().join("logs"));
        assert_busy(&root.path().join("state"));
        gate.release();
        assert!(scope.finish().is_clean());
        assert_eq!(control.begin_close(), deadline);
        assert_ordered_release(root.path(), observed);

        // Real rotation failure preserves the unowned collision and releases in order.
        for bind_before_exit in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let mut rotation = Rotation::open(&root.path().join("logs")).unwrap();
            let record = vec![b'x'; RECORD_LIMIT];
            for _ in 0..(FILE_LIMIT as usize / RECORD_LIMIT - 1) {
                rotation.append(&record).unwrap();
            }
            let retained = rotation.path(1);
            let unowned = b"Unowned archive must survive worker failure";
            fs::write(&retained, unowned).unwrap();
            let before = fs::read(rotation.path(0)).unwrap();
            let (control, mut gate) = local_worker(rotation, false);
            let (scope, binding) = control.owner_scope().unwrap();
            let mut binding = Some(binding);
            let (fence, observed) = test_fence(root.path());
            let mut fence = Some(fence);
            if bind_before_exit {
                assert!(binding.take().unwrap().bind_keepalive(fence.take().unwrap()));
            }
            assert!(control.admit(Packet::Record(record)));
            gate.release();
            let closed = scope.finish();
            assert!(closed.resources_closed && !closed.writer_healthy);
            if !bind_before_exit {
                assert!(!binding.take().unwrap().bind_keepalive(fence.take().unwrap()));
            }
            assert_ordered_release(root.path(), observed);
            assert_eq!(fs::read(&retained).unwrap(), unowned);
            assert!(
                fs::read(root.path().join("logs/owner.jsonl")).unwrap() == before,
                "Failed append must preserve prior local log bytes"
            );
            assert_eq!(control.counters.failed.load(Ordering::Acquire), 1);
            assert!(!control.flush());
        }

        // Unwinding uses the same owned finalizer, even after the caller has gone.
        let root = tempfile::tempdir().unwrap();
        let rotation = Rotation::open(&root.path().join("logs")).unwrap();
        let (control, mut gate) = local_worker(rotation, true);
        let (scope, binding) = control.owner_scope().unwrap();
        let (fence, observed) = test_fence(root.path());
        assert!(binding.bind_keepalive(fence));
        drop(scope);
        gate.release();
        let closed = control.wait_until(control.begin_close());
        assert!(closed.resources_closed && !closed.writer_healthy);
        assert_ordered_release(root.path(), observed);
    }
}
