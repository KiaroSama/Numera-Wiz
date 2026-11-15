//! Pointer-bearing storage is constructed, used and retained on one worker.
use super::*;
use std::{
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_NOT_FOUND,
        ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, GetHandleInformation, HANDLE_FLAG_INHERIT,
        INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_IO_COMPLETION, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FlushFileBuffers,
        PIPE_ACCESS_OUTBOUND, WriteFile,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    NoOperation,
    SubmittingConnect,
    SubmittingWrite { submitted: u32, offset: usize },
    PendingConnect,
    PendingWrite { submitted: u32, offset: usize },
    Terminal,
    Unknown,
}
struct Storage {
    operation: OVERLAPPED,
    buffer: [u8; 8192],
}
pub(super) struct Native {
    storage: Box<Storage>,
    pipe: Option<OwnedHandle>,
    event: Option<OwnedHandle>,
    phase: Operation,
    cancel_sent: bool,
    cancel_error: Option<u32>,
    close_ambiguity: Option<u32>,
    generation: u64,
    event_witness: bool,
}
fn last_code() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32
}
fn native_error(code: u32) -> DomainError {
    error(
        "RELAY_NATIVE_FAILED",
        &format!("Native pipe operation failed ({code})"),
    )
}
enum Observation {
    Complete(u32),
    Aborted,
    PeerClosed,
    Waiting,
    Unknown,
}
fn classify(success: bool, count: u32, code: u32) -> Observation {
    if success {
        Observation::Complete(count)
    } else {
        match code {
            ERROR_OPERATION_ABORTED => Observation::Aborted,
            ERROR_IO_INCOMPLETE | WAIT_TIMEOUT | WAIT_IO_COMPLETION => Observation::Waiting,
            _ => Observation::Unknown,
        }
    }
}
// `witnessed` is supplied only after the exact private event was signaled and
// the caller performed a fresh same-generation zero-time query.
fn queried(
    success: bool,
    count: u32,
    code: u32,
    submitted: Option<u32>,
    witnessed: bool,
) -> Observation {
    if witnessed && !success && code == ERROR_BROKEN_PIPE {
        return Observation::PeerClosed;
    }
    match classify(success, count, code) {
        Observation::Complete(count)
            if submitted.is_some_and(|limit| count_completed(count, limit).is_err()) =>
        {
            Observation::Unknown
        }
        Observation::Waiting if witnessed => Observation::Unknown,
        observation => observation,
    }
}
fn count_completed(count: u32, submitted: u32) -> Result<usize> {
    if count == 0 || count > submitted {
        return Err(error(
            "RELAY_WRITE_FAILED",
            "Native completion reported zero or impossible count",
        ));
    }
    Ok(count as usize)
}

impl Native {
    pub(super) fn new() -> Self {
        Self {
            storage: Box::new(Storage {
                operation: OVERLAPPED::default(),
                buffer: [0; 8192],
            }),
            pipe: None,
            event: None,
            phase: Operation::NoOperation,
            cancel_sent: false,
            cancel_error: None,
            close_ambiguity: None,
            generation: 0,
            event_witness: false,
        }
    }
    #[expect(
        unsafe_code,
        reason = "Create exactly one noninherited first-instance outbound pipe and manual-reset event, storing ownership immediately"
    )]
    pub(super) fn prepare(
        &mut self,
        parent: &DirectoryStamp,
        id: RelayInputId,
        nonce: &str,
        receipt: &mut PumpReceipt,
    ) -> Result<String> {
        parent.revalidate()?;
        if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(error(
                "RELAY_ENDPOINT_DENIED",
                "Invalid internal namespace nonce",
            ));
        }
        let name = format!(r"\\.\pipe\numera-wiz-{nonce}-{}", id.0);
        let wide = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        if wide.len() > 256 {
            return Err(error(
                "RELAY_ENDPOINT_DENIED",
                "Native pipe name exceeds bound",
            ));
        }
        let security = crate::security::PipeSecurity::current()?;
        let pipe = security.with_attributes(|attributes| {
            // SAFETY: name/attributes/descriptor are live; only documented
            // outbound, overlapped and first-instance flags; no inherited handle.
            let pipe = unsafe {
                CreateNamedPipeW(
                    wide.as_ptr(),
                    PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    64 * 1024,
                    0,
                    50,
                    attributes,
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                return Err(native_error(last_code()));
            }
            // SAFETY: the successful API transferred this unique handle.
            Ok(unsafe { OwnedHandle::from_raw_handle(pipe) })
        })?;
        self.pipe = Some(pipe);
        receipt.created = true;
        let pipe = self
            .pipe
            .as_ref()
            .ok_or_else(|| error("RELAY_STATE_FAILED", "Pipe owner absent"))?;
        security.verify_pipe(pipe.as_handle())?;
        check_noninheritance(pipe)?;
        // SAFETY: null attributes request a noninherited unnamed event; manual
        // reset is required for this one exact OVERLAPPED owner.
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(native_error(last_code()));
        }
        // SAFETY: successful CreateEventW transferred unique ownership.
        self.event = Some(unsafe { OwnedHandle::from_raw_handle(event) });
        check_noninheritance(
            self.event
                .as_ref()
                .ok_or_else(|| error("RELAY_STATE_FAILED", "Event owner absent"))?,
        )?;
        self.storage.operation.hEvent = event;
        parent.revalidate()?;
        Ok(name)
    }
    #[expect(
        unsafe_code,
        reason = "Reset only terminal/no-operation storage while its live event stays worker-owned"
    )]
    fn reset(&mut self) -> Result<()> {
        if self.phase == Operation::Terminal {
            self.phase = Operation::NoOperation;
        }
        if self.phase != Operation::NoOperation {
            return Err(error(
                "RELAY_SETTLEMENT_UNCERTAIN",
                "Cannot reuse live native operation storage",
            ));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| error("RESOURCE_LIMIT", "Native operation generation exhausted"))?;
        let event = self
            .event
            .as_ref()
            .ok_or_else(|| error("RELAY_STATE_FAILED", "Event absent"))?
            .as_raw_handle();
        // SAFETY: owned event is live and no prior operation can signal it.
        if unsafe { ResetEvent(event) } == 0 {
            return Err(native_error(last_code()));
        }
        self.storage.operation = OVERLAPPED {
            hEvent: event,
            ..OVERLAPPED::default()
        };
        self.cancel_sent = false;
        self.cancel_error = None;
        self.phase = Operation::NoOperation;
        Ok(())
    }
    #[expect(
        unsafe_code,
        reason = "Submit connect with one stable worker-local OVERLAPPED, classifying immediate/raced/pending outcomes"
    )]
    fn connect(&mut self, cancel: &CancellationToken, receipt: &mut PumpReceipt) -> Result<()> {
        self.reset()?;
        if cancel.is_cancelled() {
            return Err(error("CANCELLED", "Connection cancelled before submission"));
        }
        let pipe = self
            .pipe
            .as_ref()
            .ok_or_else(|| error("RELAY_STATE_FAILED", "Pipe absent"))?
            .as_raw_handle();
        self.phase = Operation::SubmittingConnect;
        // SAFETY: boxed storage is stable, event/pipe remain owned by this worker.
        if unsafe { ConnectNamedPipe(pipe, &mut self.storage.operation) } != 0 {
            self.phase = Operation::Terminal;
            receipt.connect = "Immediate";
            return Ok(());
        }
        let code = last_code();
        match code {
            ERROR_PIPE_CONNECTED => {
                self.phase = Operation::Terminal;
                receipt.connect = "Raced";
                Ok(())
            }
            ERROR_IO_PENDING => {
                self.phase = Operation::PendingConnect;
                receipt.connect = "Pending";
                self.observe_pending(cancel, Instant::now() + Duration::from_secs(1), false)
                    .map(|_| ())
            }
            _ => {
                self.phase = Operation::NoOperation;
                Err(native_error(code))
            }
        }
    }
    #[expect(
        unsafe_code,
        reason = "Cancel only the exact retained operation; cancellation success never proves completion"
    )]
    fn cancel_pending(&mut self) {
        if self.cancel_sent
            || !matches!(
                self.phase,
                Operation::PendingConnect | Operation::PendingWrite { .. } | Operation::Unknown
            )
        {
            return;
        }
        self.cancel_sent = true;
        if let Some(pipe) = &self.pipe {
            // SAFETY: same live handle and same stable operation used to submit.
            if unsafe { CancelIoEx(pipe.as_raw_handle(), &self.storage.operation) } == 0 {
                let code = last_code();
                if code != ERROR_NOT_FOUND {
                    self.cancel_error = Some(code);
                }
                // Preserve the submission facts until observe_pending records
                // the unknown failure. Never turn a lost write range into an
                // unchecked successful completion during cleanup.
            }
        }
    }
    #[expect(
        unsafe_code,
        reason = "Observe the exact stable operation in finite slices, retaining all storage unless terminality is established"
    )]
    fn observe_pending(
        &mut self,
        cancel: &CancellationToken,
        deadline: Instant,
        immediate: bool,
    ) -> Result<u32> {
        let generation = self.generation;
        let pending_write = matches!(self.phase, Operation::PendingWrite { .. });
        let submitted = match self.phase {
            Operation::PendingWrite { submitted, .. }
            | Operation::SubmittingWrite { submitted, .. } => Some(submitted),
            _ => None,
        };
        let mut immediate = immediate;
        loop {
            if cancel.is_cancelled() || Instant::now() >= deadline {
                cancel.cancel();
                self.cancel_pending();
            }
            if let Some(code) = self.cancel_error {
                self.phase = Operation::Unknown;
                return Err(error(
                    "RELAY_SETTLEMENT_UNCERTAIN",
                    &format!("Exact cancellation returned an unclassified failure ({code})"),
                ));
            }
            let pipe = self
                .pipe
                .as_ref()
                .ok_or_else(|| {
                    error(
                        "RELAY_SETTLEMENT_UNCERTAIN",
                        "Pending operation lost pipe owner",
                    )
                })?
                .as_raw_handle();
            let event = self
                .event
                .as_ref()
                .ok_or_else(|| {
                    error(
                        "RELAY_SETTLEMENT_UNCERTAIN",
                        "Pending operation lost event owner",
                    )
                })?
                .as_raw_handle();
            if self.generation != generation || self.storage.operation.hEvent != event {
                self.phase = Operation::Unknown;
                return Err(error(
                    "RELAY_SETTLEMENT_UNCERTAIN",
                    "Native operation generation or event contradicted ownership",
                ));
            }
            let mut count = 0;
            let was_immediate = immediate;
            // SAFETY: boxed OVERLAPPED/buffer/event are stable and not reused.
            let mut okay = unsafe {
                if immediate {
                    GetOverlappedResult(pipe, &self.storage.operation, &mut count, 0)
                } else {
                    GetOverlappedResultEx(pipe, &self.storage.operation, &mut count, 10, 0)
                }
            };
            immediate = false;
            let mut code = if okay == 0 { last_code() } else { 0 };
            let mut witnessed = false;
            if okay == 0 && code == ERROR_BROKEN_PIPE && pending_write {
                // SAFETY: the sole private event has never been manually set,
                // shared, reset or closed during this exact pending generation.
                let signal = unsafe { WaitForSingleObject(event, 0) };
                let wait_error = if signal == WAIT_FAILED {
                    Some(last_code())
                } else {
                    None
                };
                if signal != WAIT_OBJECT_0 {
                    self.phase = Operation::Unknown;
                    return Err(error(
                        "RELAY_SETTLEMENT_UNCERTAIN",
                        &format!(
                            "Peer failure lacks exact event completion witness (wait={signal}, error={wait_error:?})"
                        ),
                    ));
                }
                self.event_witness = true;
                count = 0;
                // SAFETY: fresh query AFTER the witness, same live handle and
                // operation; a pre-wait error is never reused as terminal proof.
                okay = unsafe {
                    GetOverlappedResultEx(pipe, &self.storage.operation, &mut count, 0, 0)
                };
                code = if okay == 0 { last_code() } else { 0 };
                witnessed = true;
            }
            match queried(okay != 0, count, code, submitted, witnessed) {
                Observation::Complete(count) => {
                    self.phase = Operation::Terminal;
                    if cancel.is_cancelled() {
                        return Err(error(
                            "CANCELLED",
                            "Native completion raced sticky cancellation",
                        ));
                    }
                    return Ok(count);
                }
                Observation::Aborted => {
                    self.phase = Operation::Terminal;
                    return Err(error("CANCELLED", "Exact native operation aborted"));
                }
                Observation::PeerClosed => {
                    self.phase = Operation::Terminal;
                    return Err(error(
                        "RELAY_PEER_CLOSED",
                        "Exact pending write completed with peer closure",
                    ));
                }
                Observation::Waiting if !was_immediate => {}
                Observation::Waiting => {
                    self.phase = Operation::Unknown;
                    return Err(error(
                        "RELAY_SETTLEMENT_UNCERTAIN",
                        "Immediate completion query contradicted submission",
                    ));
                }
                Observation::Unknown => {
                    self.phase = Operation::Unknown;
                    cancel.cancel();
                    return Err(error(
                        "RELAY_SETTLEMENT_UNCERTAIN",
                        &format!(
                            "Native query did not establish terminality (query={code}, cancel={:?})",
                            self.cancel_error
                        ),
                    ));
                }
            }
        }
    }
    #[expect(
        unsafe_code,
        reason = "WriteFile receives one stable buffer suffix and OVERLAPPED; only confirmed completion advances bytes"
    )]
    fn write_chunk(
        &mut self,
        count: usize,
        cancel: &CancellationToken,
        receipt: &mut PumpReceipt,
    ) -> Result<()> {
        let mut offset = 0;
        while offset < count {
            self.reset()?;
            if cancel.is_cancelled() {
                return Err(error(
                    "CANCELLED",
                    "Native write cancelled before submission",
                ));
            }
            let submitted = u32::try_from(count - offset).map_err(crate::files::io_error)?;
            let pipe = self
                .pipe
                .as_ref()
                .ok_or_else(|| error("RELAY_STATE_FAILED", "Pipe absent"))?
                .as_raw_handle();
            self.phase = Operation::SubmittingWrite { submitted, offset };
            // SAFETY: buffer suffix and OVERLAPPED belong to this stable Box;
            // null synchronous count follows the overlapped WriteFile contract.
            let okay = unsafe {
                WriteFile(
                    pipe,
                    self.storage.buffer.as_ptr().add(offset),
                    submitted,
                    ptr::null_mut(),
                    &mut self.storage.operation,
                )
            };
            let immediate = okay != 0;
            if !immediate {
                let code = last_code();
                if code != ERROR_IO_PENDING {
                    self.phase = Operation::NoOperation;
                    return Err(native_error(code));
                }
                self.phase = Operation::PendingWrite { submitted, offset };
                receipt.pending_write = true;
            }
            let (confirmed_submission, confirmed_offset) = match self.phase {
                Operation::PendingWrite { submitted, offset }
                | Operation::SubmittingWrite { submitted, offset } => (submitted, offset),
                _ => {
                    return Err(error(
                        "RELAY_SETTLEMENT_UNCERTAIN",
                        "Write operation owner changed",
                    ));
                }
            };
            if confirmed_offset != offset {
                return Err(error(
                    "RELAY_SETTLEMENT_UNCERTAIN",
                    "Write suffix owner changed",
                ));
            }
            let completed =
                self.observe_pending(cancel, Instant::now() + Duration::from_secs(1), immediate)?;
            let completed = count_completed(completed, confirmed_submission)?;
            offset = offset
                .checked_add(completed)
                .ok_or_else(|| error("RESOURCE_LIMIT", "Native suffix count overflow"))?;
            checked_progress(receipt, completed)?;
        }
        Ok(())
    }
    #[expect(
        unsafe_code,
        reason = "Flush only after exact input verification and terminal writes; a blocked syscall retains this entire worker"
    )]
    pub(super) fn pump(
        &mut self,
        parent: &DirectoryStamp,
        input: &mut ByteInput,
        cancel: &CancellationToken,
        receipt: &mut PumpReceipt,
    ) -> Result<()> {
        self.connect(cancel, receipt)?;
        parent.revalidate()?;
        let mut hash = Sha256::new();
        while receipt.bytes < input.len {
            if cancel.is_cancelled() {
                return Err(error("CANCELLED", "Input cancelled before read"));
            }
            let available = usize::try_from((input.len - receipt.bytes).min(8192))
                .map_err(crate::files::io_error)?;
            let count = input
                .reader
                .read(&mut self.storage.buffer[..available])
                .map_err(crate::files::io_error)?;
            if count == 0 {
                return Err(error("SOURCE_CHANGED", "Byte input truncated"));
            }
            hash.update(&self.storage.buffer[..count]);
            receipt.max_buffer = receipt.max_buffer.max(count);
            self.write_chunk(count, cancel, receipt)?;
        }
        let hash: [u8; 32] = hash.finalize().into();
        input.verify(receipt.bytes, hash)?;
        if cancel.is_cancelled() {
            return Err(error("CANCELLED", "Input cancelled before drain"));
        }
        let pipe = self
            .pipe
            .as_ref()
            .ok_or_else(|| error("RELAY_STATE_FAILED", "Pipe absent before drain"))?;
        // SAFETY: all operations are terminal and this is the sole pipe owner.
        // Flush may block: the independent coordinator retains this JoinHandle.
        if unsafe { FlushFileBuffers(pipe.as_raw_handle()) } == 0 {
            return Err(native_error(last_code()));
        }
        receipt.drained = true;
        receipt.hash = Some(hash);
        receipt.transport = true;
        Ok(())
    }
    pub(super) fn close(
        &mut self,
        _parent: &DirectoryStamp,
        cancel: &CancellationToken,
        receipt: &mut PumpReceipt,
    ) -> Result<()> {
        if matches!(
            self.phase,
            Operation::SubmittingConnect | Operation::SubmittingWrite { .. }
        ) {
            self.phase = Operation::Unknown;
        }
        if matches!(
            self.phase,
            Operation::PendingConnect | Operation::PendingWrite { .. }
        ) {
            cancel.cancel();
            self.cancel_pending();
            let _ = self.observe_pending(cancel, Instant::now(), false);
        }
        if self.phase == Operation::Unknown {
            return Err(error(
                "RELAY_SETTLEMENT_UNCERTAIN",
                "Unknown native terminality retains storage",
            ));
        }
        if let Some(pipe) = self.pipe.take()
            && let Err(code) = close_owned(pipe)
        {
            self.close_ambiguity = Some(code);
        }
        if let Some(event) = self.event.take()
            && let Err(code) = close_owned(event)
        {
            self.close_ambiguity.get_or_insert(code);
        }
        if self.close_ambiguity.is_some() {
            return Err(error(
                "RELAY_SETTLEMENT_UNCERTAIN",
                "Consuming native close is ambiguous; never retried",
            ));
        }
        receipt.closed = receipt.created;
        receipt.cleaned = receipt.created;
        receipt.event_completion = self.event_witness;
        Ok(())
    }
}
#[expect(
    unsafe_code,
    reason = "Consume each unique owned handle exactly once and record native BOOL/error immediately"
)]
fn close_owned(handle: OwnedHandle) -> std::result::Result<(), u32> {
    let raw = handle.into_raw_handle();
    // SAFETY: no operation references this handle; IntoRawHandle prevents Drop
    // retry. A failed close is retained ambiguity, never a second CloseHandle.
    if unsafe { CloseHandle(raw) } == 0 {
        Err(last_code())
    } else {
        Ok(())
    }
}
#[expect(
    unsafe_code,
    reason = "Query only a live owned handle's actual inheritance flags"
)]
fn check_noninheritance(handle: &OwnedHandle) -> Result<()> {
    let mut flags = 0;
    // SAFETY: live handle and one initialized native flags output.
    if unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) } == 0 {
        return Err(native_error(last_code()));
    }
    if flags & HANDLE_FLAG_INHERIT != 0 {
        return Err(error(
            "RELAY_ENDPOINT_DENIED",
            "Native handle is inheritable",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn run_native_lifetime_cases() {
    assert!(matches!(classify(true, 7, 0), Observation::Complete(7)));
    assert!(matches!(
        classify(false, 0, ERROR_OPERATION_ABORTED),
        Observation::Aborted
    ));
    for code in [ERROR_IO_INCOMPLETE, WAIT_TIMEOUT, WAIT_IO_COMPLETION] {
        assert!(matches!(classify(false, 0, code), Observation::Waiting));
    }
    assert!(matches!(classify(false, 0, 109), Observation::Unknown));
    assert_eq!(count_completed(3, 7).unwrap(), 3);
    assert!(count_completed(0, 7).is_err());
    assert!(count_completed(8, 7).is_err());
    assert!(matches!(
        queried(false, 0, ERROR_BROKEN_PIPE, Some(7), false),
        Observation::Unknown
    ));
    assert!(matches!(
        queried(false, 0, ERROR_BROKEN_PIPE, Some(7), true),
        Observation::PeerClosed
    ));
    assert!(matches!(
        queried(true, 3, 0, Some(7), true),
        Observation::Complete(3)
    ));
    assert!(matches!(
        queried(false, 0, ERROR_OPERATION_ABORTED, Some(7), true),
        Observation::Aborted
    ));
    for code in [
        ERROR_IO_INCOMPLETE,
        WAIT_TIMEOUT,
        WAIT_IO_COMPLETION,
        6,
        87,
        232,
        234,
    ] {
        assert!(matches!(
            queried(false, 0, code, Some(7), true),
            Observation::Unknown
        ));
    }
    for count in [0, 8] {
        assert!(matches!(
            queried(true, count, 0, Some(7), true),
            Observation::Unknown
        ));
    }

    struct DropCount(Arc<AtomicUsize>);
    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
    let mut synthetic = Native::new();
    synthetic.phase = Operation::PendingWrite {
        submitted: 7,
        offset: 0,
    };
    synthetic.cancel_error = Some(5);
    let failed = synthetic.observe_pending(
        &CancellationToken::new(),
        Instant::now() + Duration::from_secs(1),
        false,
    );
    assert!(
        failed.is_err_and(|cause| cause.code == "RELAY_SETTLEMENT_UNCERTAIN"
            && cause.message.contains("cancellation"))
    );
    assert!(synthetic.phase == Operation::Unknown);
    // No native call or pointer was submitted in this state-machine case.
    drop(synthetic);

    let drops = Arc::new(AtomicUsize::new(0));
    let owned_drops = drops.clone();
    let (entered, observe) = std::sync::mpsc::sync_channel(1);
    let (release, terminal) = std::sync::mpsc::sync_channel(1);
    let owner = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let storage = ManuallyDrop::new(Box::new(DropCount(owned_drops.clone())));
            let unwind =
                std::panic::catch_unwind(|| panic!("finite simulated pending-operation unwind"));
            let before = owned_drops.load(Ordering::Acquire);
            entered.send((unwind.is_err(), before)).unwrap();
            // Finite test gate, no native cancellation claim. The ordinary owner
            // cannot drop its simulated pending storage before this observation.
            terminal.recv_timeout(Duration::from_secs(2)).unwrap();
            drop(ManuallyDrop::into_inner(storage));
        })
        .unwrap();
    let observed = observe.recv_timeout(Duration::from_secs(2));
    let before_terminal = drops.load(Ordering::Acquire);
    let released = release.send(());
    let joined = owner.join();
    assert_eq!(observed.unwrap(), (true, 0));
    assert_eq!(before_terminal, 0);
    released.unwrap();
    joined.unwrap();
    assert_eq!(drops.load(Ordering::Acquire), 1);
}
