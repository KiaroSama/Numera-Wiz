//! Two dedicated owners; no payload channel or cross-thread native pointers.
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
use crate::files::{DirectoryStamp, SourceStamp};
use nw_domain::{DomainError, Result, records::NativeObjectIdentity};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    mem::ManuallyDrop,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
#[cfg(unix)]
use unix::Native;
#[cfg(windows)]
use windows::Native;
#[cfg(all(test, windows))]
pub(crate) use windows::run_native_lifetime_cases;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayInputId(pub u8);
pub struct ByteInput {
    id: RelayInputId,
    source: SourceStamp,
    reader: File,
    identity: NativeObjectIdentity,
    len: u64,
    hash: [u8; 32],
}
impl ByteInput {
    /// Move the only active shared-cursor lease; no other clone may read or seek
    /// concurrently. This checks transport identity, not a grant or media format.
    pub fn new(
        id: RelayInputId,
        source: SourceStamp,
        expected_len: u64,
        expected_sha256: [u8; 32],
    ) -> Result<Self> {
        if id.0 > 1 || expected_len > 8 * 1024 * 1024 || source.byte_len()? != expected_len {
            return Err(error(
                "INPUT_DENIED",
                "Invalid byte input identity or length",
            ));
        }
        let identity = source.object_identity()?;
        let reader = source.try_clone_file()?;
        if !reader.metadata().map_err(crate::files::io_error)?.is_file() {
            return Err(error("INPUT_DENIED", "Regular byte input required"));
        }
        Ok(Self {
            id,
            source,
            reader,
            identity,
            len: expected_len,
            hash: expected_sha256,
        })
    }
    fn rewind(&mut self) -> Result<()> {
        self.source.require_object(&self.identity)?;
        if self.source.byte_len()? != self.len {
            return Err(error("SOURCE_CHANGED", "Input length changed"));
        }
        self.reader
            .seek(SeekFrom::Start(0))
            .map_err(crate::files::io_error)?;
        Ok(())
    }
    fn verify(&mut self, count: u64, hash: [u8; 32]) -> Result<()> {
        let mut end = [0];
        if count != self.len
            || hash != self.hash
            || self.reader.read(&mut end).map_err(crate::files::io_error)? != 0
        {
            return Err(error("SOURCE_CHANGED", "Input length, hash or EOF changed"));
        }
        self.source.require_object(&self.identity)
    }
}
#[derive(Clone)]
pub struct PumpReceipt {
    id: RelayInputId,
    created: bool,
    bytes: u64,
    hash: Option<[u8; 32]>,
    writes: u64,
    max_buffer: usize,
    transport: bool,
    joined: bool,
    closed: bool,
    cleaned: bool,
    connect: &'static str,
    pending_write: bool,
    drained: bool,
    event_completion: bool,
}
impl PumpReceipt {
    pub(super) fn not_created(id: RelayInputId) -> Self {
        Self {
            id,
            created: false,
            bytes: 0,
            hash: None,
            writes: 0,
            max_buffer: 0,
            transport: false,
            joined: false,
            closed: false,
            cleaned: false,
            connect: "NotCreated",
            pending_write: false,
            drained: false,
            event_completion: false,
        }
    }
    pub fn input_id(&self) -> RelayInputId {
        self.id
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn sha256(&self) -> Option<[u8; 32]> {
        self.hash
    }
    pub fn completed_writes(&self) -> u64 {
        self.writes
    }
    pub fn max_buffer_bytes(&self) -> usize {
        self.max_buffer
    }
    pub fn transport_complete(&self) -> bool {
        self.transport
    }
    pub fn worker_joined(&self) -> bool {
        self.joined
    }
    pub fn endpoint_created(&self) -> bool {
        self.created
    }
    pub fn endpoint_closed(&self) -> bool {
        self.closed
    }
    pub fn endpoint_cleaned(&self) -> bool {
        self.cleaned
    }
    pub fn connect_observation(&self) -> &'static str {
        self.connect
    }
    pub fn pending_write_observed(&self) -> bool {
        self.pending_write
    }
    pub fn drain_completed(&self) -> bool {
        self.drained
    }
    pub fn exact_event_completion_observed(&self) -> bool {
        self.event_completion
    }
}
fn error(code: &str, message: &str) -> DomainError {
    DomainError::new(code, message)
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
struct WorkerControl {
    registered: AtomicBool,
    start: AtomicBool,
    ready: Mutex<Option<String>>,
    failure: Mutex<Option<DomainError>>,
    cancel: CancellationToken,
}
struct WorkerResult {
    receipt: PumpReceipt,
    result: Result<()>,
    settled: bool,
}
struct Worker {
    handle: Option<JoinHandle<WorkerResult>>,
    control: Arc<WorkerControl>,
    result: Option<WorkerResult>,
}
pub(super) struct OwnedInputRelaySet {
    inputs: [Option<ByteInput>; 2],
    parent: Arc<DirectoryStamp>,
    namespace_path: Option<PathBuf>,
    namespace: Option<Arc<DirectoryStamp>>,
    #[cfg(unix)]
    namespace_held: Option<File>,
    workers: [Option<Worker>; 2],
    cancel: CancellationToken,
    threads: Arc<AtomicUsize>,
    initialized: bool,
    failure: Option<DomainError>,
    cleaned: bool,
    #[cfg(test)]
    fault_after_first: Option<bool>,
}
impl OwnedInputRelaySet {
    pub(super) fn prepare(
        inputs: [ByteInput; 2],
        parent: DirectoryStamp,
        cancel: CancellationToken,
        threads: Arc<AtomicUsize>,
    ) -> Result<Self> {
        if inputs[0].id != RelayInputId(0) || inputs[1].id != RelayInputId(1) {
            return Err(error(
                "INPUT_DENIED",
                "Two distinct ordered byte inputs are required",
            ));
        }
        // Construction has no external effects. The coordinator installs this
        // owner before observe performs any filesystem or thread operation.
        let [first, second] = inputs;
        Ok(Self {
            inputs: [Some(first), Some(second)],
            parent: Arc::new(parent),
            namespace_path: None,
            namespace: None,
            #[cfg(unix)]
            namespace_held: None,
            workers: [None, None],
            cancel,
            threads,
            initialized: false,
            failure: None,
            cleaned: false,
            #[cfg(test)]
            fault_after_first: None,
        })
    }
    #[cfg(test)]
    pub(super) fn fail_after_first(&mut self, panics: bool) {
        self.fault_after_first = Some(panics);
    }
    fn initialize(&mut self) -> Result<()> {
        self.initialized = true;
        self.parent.revalidate()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let metadata = self
                .parent
                .held_file()
                .metadata()
                .map_err(crate::files::io_error)?;
            if metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(error(
                    "INSECURE_DATA_ROOT",
                    "Relay parent must retain private current-user permissions",
                ));
            }
        }
        // Windows DirectoryStamp excludes delete sharing for this entire call;
        // an existing parent cannot disappear and trigger creation/repair.
        #[cfg(windows)]
        crate::security::private_directory(self.parent.path())?;
        if self.cancel.is_cancelled() {
            return Err(error("CANCELLED", "Preparation cancelled"));
        }
        #[cfg(unix)]
        let (namespace, nonce) = {
            use rustix::fs::{AtFlags, FileType, Mode, OFlags};
            let name = format!(".nw-relay-{}", uuid::Uuid::new_v4().simple());
            let path = self.parent.path().join(&name);
            // Exclusivity and lookup are both relative to the original held
            // parent. A pathname swap cannot redirect namespace creation.
            rustix::fs::mkdirat(
                self.parent.held_file(),
                &name,
                Mode::RUSR | Mode::WUSR | Mode::XUSR,
            )
            .map_err(crate::files::io_error)?;
            self.namespace_path = Some(path);
            let fd = rustix::fs::openat(
                self.parent.held_file(),
                &name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(crate::files::io_error)?;
            self.namespace_held = Some(File::from(fd));
            let held = rustix::fs::fstat(
                self.namespace_held
                    .as_ref()
                    .ok_or_else(|| error("SESSION_STATE_FAILED", "Namespace handle missing"))?,
            )
            .map_err(crate::files::io_error)?;
            let named =
                rustix::fs::statat(self.parent.held_file(), &name, AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(crate::files::io_error)?;
            if FileType::from_raw_mode(held.st_mode) != FileType::Directory
                || held.st_mode & 0o777 != 0o700
                || held.st_uid != rustix::process::geteuid().as_raw()
                || held.st_dev != named.st_dev
                || held.st_ino != named.st_ino
            {
                return Err(error(
                    "TARGET_CHANGED",
                    "Created private namespace identity or permissions changed",
                ));
            }
            let path = self
                .namespace_path
                .as_ref()
                .ok_or_else(|| error("SESSION_STATE_FAILED", "Namespace owner missing"))?;
            let namespace = Arc::new(DirectoryStamp::capture_resolved(path)?);
            let captured =
                rustix::fs::fstat(namespace.held_file()).map_err(crate::files::io_error)?;
            if held.st_dev != captured.st_dev || held.st_ino != captured.st_ino {
                return Err(error(
                    "TARGET_CHANGED",
                    "Namespace pathname differs from the held created directory",
                ));
            }
            self.namespace = Some(namespace.clone());
            (namespace, String::new())
        };
        #[cfg(windows)]
        let (namespace, nonce) = (
            self.parent.clone(),
            uuid::Uuid::new_v4().simple().to_string(),
        );
        self.parent.revalidate()?;
        for index in 0..2 {
            if self.cancel.is_cancelled() {
                return Err(error("CANCELLED", "Preparation cancelled"));
            }
            let input = self.inputs[index]
                .take()
                .ok_or_else(|| error("SESSION_STATE_FAILED", "Input owner missing"))?;
            let control = Arc::new(WorkerControl {
                registered: AtomicBool::new(false),
                start: AtomicBool::new(false),
                ready: Mutex::new(None),
                failure: Mutex::new(None),
                cancel: self.cancel.clone(),
            });
            let worker_control = control.clone();
            let worker_parent = namespace.clone();
            let worker_nonce = nonce.clone();
            let handle = std::thread::Builder::new()
                .name(format!("nw-relay-input-{index}"))
                .stack_size(2 * 1024 * 1024)
                .spawn(move || {
                    while !worker_control.registered.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                    worker(input, worker_parent, worker_control, worker_nonce)
                })
                .map_err(|problem| error("RELAY_THREAD_FAILED", &problem.to_string()))?;
            self.workers[index] = Some(Worker {
                handle: Some(handle),
                control: control.clone(),
                result: None,
            });
            self.threads.fetch_add(1, Ordering::AcqRel);
            control.registered.store(true, Ordering::Release);
            #[cfg(test)]
            if index == 0
                && let Some(panics) = self.fault_after_first.take()
            {
                assert!(
                    !panics,
                    "finite preparation unwind after first registered worker"
                );
                return Err(error(
                    "RELAY_PREPARATION_FAILED",
                    "Finite failure after first registered worker",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn observe(&mut self) -> Result<()> {
        if !self.initialized
            && let Err(cause) = self.initialize()
        {
            self.failure = Some(cause);
            self.cancel.cancel();
        }
        for worker in self.workers.iter_mut().flatten() {
            if let Some(cause) = lock(&worker.control.failure).as_ref() {
                self.failure.get_or_insert_with(|| cause.clone());
                self.cancel.cancel();
            }
            if worker.handle.as_ref().is_some_and(JoinHandle::is_finished)
                && let Some(handle) = worker.handle.take()
            {
                match handle.join() {
                    Ok(mut result) => {
                        result.receipt.joined = true;
                        if let Err(cause) = &result.result {
                            self.failure.get_or_insert_with(|| cause.clone());
                            self.cancel.cancel();
                        }
                        worker.result = Some(result);
                    }
                    Err(_) => {
                        self.failure.get_or_insert_with(|| {
                            error("RELAY_SETTLEMENT_UNCERTAIN", "Worker escaped unwind guard")
                        });
                        self.cancel.cancel();
                    }
                }
                self.threads.fetch_sub(1, Ordering::AcqRel);
            }
        }
        match &self.failure {
            Some(cause) => Err(cause.clone()),
            None => Ok(()),
        }
    }
    pub(super) fn ready(&self) -> bool {
        self.workers.iter().all(|worker| {
            worker
                .as_ref()
                .is_some_and(|worker| lock(&worker.control.ready).is_some())
        })
    }
    pub(super) fn endpoints(&self) -> Result<[String; 2]> {
        let mut endpoints = [String::new(), String::new()];
        for (index, worker) in self.workers.iter().enumerate() {
            endpoints[index] = worker
                .as_ref()
                .and_then(|worker| lock(&worker.control.ready).clone())
                .ok_or_else(|| error("RELAY_NOT_READY", "Endpoint preparation not complete"))?;
        }
        Ok(endpoints)
    }
    pub(super) fn start(&mut self) -> Result<()> {
        if !self.ready() || self.cancel.is_cancelled() {
            return Err(error("CANCELLED", "Relays cannot start"));
        }
        for worker in self.workers.iter().flatten() {
            worker.control.start.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub(super) fn joined(&self) -> bool {
        self.initialized
            && self.workers.iter().flatten().all(|worker| {
                worker.handle.is_none()
                    && worker.result.as_ref().is_some_and(|result| result.settled)
            })
    }
    pub(super) fn cleanup(&mut self) -> Result<()> {
        if self.cleaned {
            return Ok(());
        }
        if !self.joined() {
            return Err(error(
                "RELAY_SETTLEMENT_UNCERTAIN",
                "Workers are still owned",
            ));
        }
        if self.namespace_path.is_some() {
            let namespace = self.namespace.as_ref().ok_or_else(|| {
                error(
                    "RELAY_SETTLEMENT_UNCERTAIN",
                    "Created namespace identity could not be captured",
                )
            })?;
            self.parent.revalidate()?;
            namespace.revalidate()?;
            #[cfg(unix)]
            unix::remove_namespace(&self.parent, namespace)?;
        }
        self.cleaned = true;
        Ok(())
    }
    pub(super) fn take_receipts(&mut self) -> [PumpReceipt; 2] {
        std::array::from_fn(|index| {
            self.workers[index]
                .as_mut()
                .and_then(|worker| worker.result.take())
                .map(|result| result.receipt)
                .unwrap_or_else(|| PumpReceipt::not_created(RelayInputId(index as u8)))
        })
    }
}

fn worker(
    mut input: ByteInput,
    parent: Arc<DirectoryStamp>,
    control: Arc<WorkerControl>,
    nonce: String,
) -> WorkerResult {
    let mut native = ManuallyDrop::new(Native::new());
    let mut receipt = PumpReceipt::not_created(input.id);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let endpoint = native.prepare(&parent, input.id, &nonce, &mut receipt)?;
        *lock(&control.ready) = Some(endpoint);
        while !control.start.load(Ordering::Acquire) {
            if control.cancel.is_cancelled() {
                return Err(error("CANCELLED", "Prepared worker cancelled"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if control.cancel.is_cancelled() {
            return Err(error("CANCELLED", "Worker cancelled before connection"));
        }
        input.rewind()?;
        native.pump(&parent, &mut input, &control.cancel, &mut receipt)
    }));
    let result = match outcome {
        Ok(result) => result,
        Err(_) => Err(error(
            "RELAY_WORKER_PANIC",
            "Worker panicked; exact native owner retained for cleanup",
        )),
    };
    if let Err(cause) = &result {
        *lock(&control.failure) = Some(cause.clone());
        control.cancel.cancel();
    }
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        native.close(&parent, &control.cancel, &mut receipt)
    }));
    match cleanup {
        Ok(Ok(())) => {
            drop(ManuallyDrop::into_inner(native));
            WorkerResult {
                receipt,
                result,
                settled: true,
            }
        }
        Ok(Err(cause)) => {
            *lock(&control.failure) = Some(cause);
            control.cancel.cancel();
            // The worker and its ManuallyDrop native storage remain the owner,
            // including checked-close ambiguity. No detached leaked pointer.
            loop {
                std::thread::park_timeout(Duration::from_secs(60));
            }
        }
        Err(_) => {
            *lock(&control.failure) = Some(error(
                "RELAY_SETTLEMENT_UNCERTAIN",
                "Native cleanup panicked",
            ));
            control.cancel.cancel();
            // The native guard and live worker retain all pointer storage. An
            // unexpected second unwind never turns into a successful join.
            loop {
                std::thread::park_timeout(Duration::from_secs(60));
            }
        }
    }
}

fn checked_progress(receipt: &mut PumpReceipt, count: usize) -> Result<()> {
    receipt.bytes = receipt
        .bytes
        .checked_add(count as u64)
        .ok_or_else(|| error("RESOURCE_LIMIT", "Input byte count overflow"))?;
    receipt.writes = receipt
        .writes
        .checked_add(1)
        .ok_or_else(|| error("RESOURCE_LIMIT", "Input write count overflow"))?;
    Ok(())
}
