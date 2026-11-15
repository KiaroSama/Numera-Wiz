//! Suspended launch, private Job and coordinator-owned overlapped output.
use super::{OutputReader, super::error};
use nw_domain::{DomainError, Result};
use std::{os::windows::{ffi::OsStrExt, io::BorrowedHandle, process::ExitStatusExt}, path::Path, ptr, sync::atomic::Ordering};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_BROKEN_PIPE, ERROR_INSUFFICIENT_BUFFER, ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile},
    System::{
        IO::{CancelIoEx, GetOverlappedResultEx, OVERLAPPED},
        JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject},
        Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT},
        Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, ResetEvent, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject},
    },
};

fn last_code() -> u32 { std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32 }
fn failure(operation: &str, code: u32) -> DomainError { error("SESSION_NATIVE_FAILED", &format!("{operation} failed ({code})")) }

// A failed close is ambiguous: retain this exact value and never retry it as if
// it still named the same object. There is deliberately no unchecked Drop close.
#[derive(Default)]
struct Handle { value: Option<usize>, uncertain: Option<u32> }
impl Handle {
    fn raw(&self) -> Result<HANDLE> {
        if let Some(code) = self.uncertain { return Err(failure("handle ownership", code)); }
        self.value.map(|value| value as HANDLE).ok_or_else(|| error("SESSION_STATE_FAILED", "Native handle missing"))
    }
    fn adopt(&mut self, value: HANDLE) -> Result<()> {
        if value.is_null() || value == INVALID_HANDLE_VALUE { return Err(failure("handle creation", last_code())); }
        self.value = Some(value as usize);
        Ok(())
    }
    fn absent(&self) -> bool { self.value.is_none() }
    #[expect(unsafe_code, reason = "Close only the exact retained live handle; preserve ambiguous failures without retry")]
    fn close(&mut self) -> Result<()> {
        if self.absent() { return Ok(()); }
        let handle = self.raw()?;
        // SAFETY: this owner uniquely holds the handle and has settled its I/O.
        if unsafe { CloseHandle(handle) } == 0 {
            let code = last_code(); self.uncertain = Some(code); return Err(failure("CloseHandle", code));
        }
        self.value = None;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Idle, Connect, Read, Unknown }
struct Storage { operation: OVERLAPPED, bytes: [u8; 8192] }
struct OutputPipe {
    pipe: Handle,
    event: Handle,
    writer: Handle,
    storage: Box<Storage>,
    phase: Phase,
    ready: usize,
    connected: bool,
    ended: bool,
    cancel_sent: bool,
    cancel_error: Option<u32>,
    sender: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
}
impl OutputPipe {
    fn new() -> Self {
        Self { pipe: Handle::default(), event: Handle::default(), writer: Handle::default(), storage: Box::new(Storage { operation: OVERLAPPED::default(), bytes: [0; 8192] }), phase: Phase::Idle, ready: 0, connected: false, ended: false, cancel_sent: false, cancel_error: None, sender: None }
    }
    #[expect(unsafe_code, reason = "Create private noninherited overlapped output server/event and a synchronous child writer; retain each before fallible work")]
    fn prepare(&mut self, nonce: &str, id: usize) -> Result<OutputReader> {
        let name = format!(r"\\.\pipe\numera-wiz-output-{nonce}-{id}").encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let security = crate::security::PipeSecurity::current()?;
        // SAFETY: bounded name and descriptor remain live for synchronous creation.
        security.with_attributes(|attributes| self.pipe.adopt(unsafe { CreateNamedPipeW(name.as_ptr(), PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 0, 8192, 50, attributes) }))?;
        // SAFETY: borrow cannot outlive the retained unique server handle.
        security.verify_pipe(unsafe { BorrowedHandle::borrow_raw(self.pipe.raw()?) })?;
        // SAFETY: private unnamed manual-reset event with no inheritance.
        self.event.adopt(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?;
        self.reset()?;
        self.phase = Phase::Unknown;
        // SAFETY: stable boxed OVERLAPPED and event survive all pending outcomes.
        if unsafe { ConnectNamedPipe(self.pipe.raw()?, &mut self.storage.operation) } != 0 {
            self.phase = Phase::Idle; self.connected = true;
        } else {
            match last_code() {
                ERROR_IO_PENDING => self.phase = Phase::Connect,
                ERROR_PIPE_CONNECTED => { self.phase = Phase::Idle; self.connected = true; },
                code => return Err(failure("ConnectNamedPipe", code)),
            }
        }
        // SAFETY: the first-instance server above already exists. This same-user
        // synchronous write endpoint is initially noninherited; only launch
        // admission below makes the selected three child handles inheritable.
        self.writer.adopt(unsafe { CreateFileW(name.as_ptr(), GENERIC_WRITE, 0, ptr::null(), OPEN_EXISTING, 0, ptr::null_mut()) })?;
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        self.sender = Some(sender);
        Ok(OutputReader::new(receiver))
    }
    #[expect(unsafe_code, reason = "Reset only idle exact-operation storage while the unique private event remains live")]
    fn reset(&mut self) -> Result<()> {
        if self.phase != Phase::Idle { return Err(error("OUTPUT_SETTLEMENT_UNCERTAIN", "Output operation is still owned")); }
        let event = self.event.raw()?;
        // SAFETY: no prior operation remains pending or can signal this event.
        if unsafe { ResetEvent(event) } == 0 { return Err(failure("ResetEvent", last_code())); }
        self.storage.operation = OVERLAPPED { hEvent: event, ..OVERLAPPED::default() };
        self.cancel_sent = false;
        Ok(())
    }
    #[expect(unsafe_code, reason = "Cancel and poll only this owner's stable OVERLAPPED; no native storage is released before exact terminal completion")]
    fn pump(&mut self, cancel: bool) -> Result<()> {
        if self.pipe.absent() { self.sender.take(); return Ok(()); }
        if let Some(code) = self.cancel_error { return Err(failure("CancelIoEx", code)); }
        if self.phase == Phase::Unknown { return Err(error("OUTPUT_SETTLEMENT_UNCERTAIN", "Output submission ownership is uncertain")); }
        let abandoned = self.sender.as_ref().is_some_and(tokio::sync::mpsc::Sender::is_closed);
        let cancel = cancel || abandoned;
        if cancel && matches!(self.phase, Phase::Connect | Phase::Read) && !self.cancel_sent {
            self.cancel_sent = true;
            // SAFETY: the exact operation, handle, buffer and private event are
            // still retained; ERROR_NOT_FOUND is not itself completion proof.
            if unsafe { CancelIoEx(self.pipe.raw()?, &self.storage.operation) } == 0 {
                let code = last_code();
                if code != ERROR_NOT_FOUND { self.cancel_error = Some(code); return Err(failure("CancelIoEx", code)); }
            }
        }
        if matches!(self.phase, Phase::Connect | Phase::Read) {
            // SAFETY: zero-time observation of this live private event.
            let wait = unsafe { WaitForSingleObject(self.event.raw()?, 0) };
            if wait == WAIT_TIMEOUT { return Ok(()); }
            if wait != WAIT_OBJECT_0 {
                let code = if wait == WAIT_FAILED { last_code() } else { wait };
                return Err(failure("output event wait", code));
            }
            let mut count = 0;
            // SAFETY: event witness and query refer to this exact operation;
            // the query never waits and the stable output pointers remain live.
            let success = unsafe { GetOverlappedResultEx(self.pipe.raw()?, &self.storage.operation, &mut count, 0, 0) } != 0;
            let code = if success { 0 } else { last_code() };
            let phase = self.phase;
            if success {
                if phase == Phase::Read && count > 8192 { return Err(error("OUTPUT_SETTLEMENT_UNCERTAIN", "Impossible output byte count")); }
                self.phase = Phase::Idle;
                if phase == Phase::Connect { self.connected = true; } else { self.ready = count as usize; }
            } else if code == ERROR_OPERATION_ABORTED || (phase == Phase::Read && code == ERROR_BROKEN_PIPE) {
                self.phase = Phase::Idle; self.ended = true;
            } else { return Err(failure("output completion", code)); }
        }
        if cancel { self.ready = 0; self.ended = true; }
        if self.ready != 0 && let Some(sender) = &self.sender {
            match sender.try_reserve() {
                Ok(permit) => { permit.send(self.storage.bytes[..self.ready].to_vec()); self.ready = 0; },
                Err(tokio::sync::mpsc::error::TrySendError::Full(())) => return Ok(()),
                Err(tokio::sync::mpsc::error::TrySendError::Closed(())) => { self.ready = 0; self.ended = true; },
            }
        }
        if self.ended {
            self.pipe.close()?;
            self.event.close()?;
            self.sender.take();
            return Ok(());
        }
        if !self.connected || self.ready != 0 { return Ok(()); }
        self.reset()?;
        self.phase = Phase::Unknown;
        // SAFETY: exactly8192 live boxed bytes and one stable OVERLAPPED. The
        // byte-count pointer is null for overlapped I/O; completion counts come
        // from an exact-operation query, never stack storage passed to ReadFile.
        if unsafe { ReadFile(self.pipe.raw()?, self.storage.bytes.as_mut_ptr(), 8192, ptr::null_mut(), &mut self.storage.operation) } != 0 {
            let mut count = 0;
            // SAFETY: ReadFile reported immediate completion; the exact stable
            // operation remains owned while this zero-time query reads its count.
            if unsafe { GetOverlappedResultEx(self.pipe.raw()?, &self.storage.operation, &mut count, 0, 0) } == 0 {
                return Err(failure("immediate output completion", last_code()));
            }
            if count > 8192 { return Err(error("OUTPUT_SETTLEMENT_UNCERTAIN", "Impossible immediate output count")); }
            self.phase = Phase::Idle; self.ready = count as usize;
        } else {
            match last_code() {
                ERROR_IO_PENDING => self.phase = Phase::Read,
                ERROR_BROKEN_PIPE => { self.phase = Phase::Idle; self.ended = true; },
                code => return Err(failure("ReadFile", code)),
            }
        }
        Ok(())
    }
    fn settled(&self) -> bool { self.pipe.absent() && self.event.absent() && self.writer.absent() && self.phase == Phase::Idle }
}

struct Attributes { storage: Vec<usize>, initialized: bool }
impl Attributes {
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST { self.storage.as_mut_ptr().cast() }
    #[expect(unsafe_code, reason = "Build one bounded aligned process-attribute list containing only the exact child std-handle whitelist")]
    fn new(handles: &[HANDLE; 3]) -> Result<Self> {
        let mut size = 0;
        // SAFETY: documented zero-buffer sizing query; no list exists yet.
        let sized = unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        let code = last_code();
        if sized != 0 || code != ERROR_INSUFFICIENT_BUFFER || size == 0 || size > 64 * 1024 { return Err(failure("attribute-list size", code)); }
        let mut value = Self { storage: vec![0; size.div_ceil(std::mem::size_of::<usize>())], initialized: false };
        // SAFETY: aligned initialized allocation covers the returned byte size.
        if unsafe { InitializeProcThreadAttributeList(value.pointer(), 1, 0, &mut size) } == 0 { return Err(failure("attribute-list initialize", last_code())); }
        value.initialized = true;
        // SAFETY: the exact three real handles and list remain live through
        // CreateProcessW; no pseudo handle or unbounded attribute is accepted.
        if unsafe { UpdateProcThreadAttribute(value.pointer(), 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, handles.as_ptr().cast(), std::mem::size_of_val(handles), ptr::null_mut(), ptr::null()) } == 0 { return Err(failure("attribute-list update", last_code())); }
        Ok(value)
    }
}
#[expect(unsafe_code, reason = "Delete the successfully initialized attribute list only after the synchronous launch call returns")]
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: initialized list remains in its original aligned storage.
            unsafe { DeleteProcThreadAttributeList(self.pointer()); }
        }
    }
}

pub(in crate::process::media_session) struct OwnedChild {
    job: Handle,
    process: Handle,
    thread: Handle,
    stdin: Handle,
    outputs: [OutputPipe; 2],
    spawned: bool,
    assigned: bool,
    terminated: bool,
    tree_empty: bool,
    exit: Option<std::process::ExitStatus>,
}
impl OwnedChild {
    pub(in crate::process::media_session) fn new() -> Self { Self { job: Handle::default(), process: Handle::default(), thread: Handle::default(), stdin: Handle::default(), outputs: [OutputPipe::new(), OutputPipe::new()], spawned: false, assigned: false, terminated: false, tree_empty: false, exit: None } }
    pub(in crate::process::media_session) fn spawned(&self) -> bool { self.spawned }
    #[expect(unsafe_code, reason = "Create a suspended process with explicit argv/environment/stdio whitelist; retain raw process/thread/job before any fallible assignment or resume")]
    pub(in crate::process::media_session) fn launch(&mut self, program: &Path, args: &[String]) -> Result<(OutputReader, OutputReader)> {
        let application = wide(program.as_os_str())?;
        let cwd = wide(program.parent().ok_or_else(|| error("PROCESS_LAUNCH_FAILED", "Executable parent missing"))?.as_os_str())?;
        let mut command_line = command_line(program, args)?;
        let environment = environment()?;
        // SAFETY: unnamed noninherited private job; stored immediately.
        self.job.adopt(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: exact typed structure/size; no breakaway permission is enabled.
        if unsafe { SetInformationJobObject(self.job.raw()?, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) } == 0 { return Err(failure("job limits", last_code())); }
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let stdout = self.outputs[0].prepare(&nonce, 0)?;
        let stderr = self.outputs[1].prepare(&nonce, 1)?;
        let nul = [b'N' as u16, b'U' as u16, b'L' as u16, 0];
        // SAFETY: documented NUL device; synchronous read-only child stdin.
        self.stdin.adopt(unsafe { CreateFileW(nul.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, ptr::null(), OPEN_EXISTING, 0, ptr::null_mut()) })?;
        let handles = [self.stdin.raw()?, self.outputs[0].writer.raw()?, self.outputs[1].writer.raw()?];
        let mut attributes = Attributes::new(&handles)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.pointer();
        let mut information = PROCESS_INFORMATION::default();
        let guard = super::super::super::acquire_launch_guard().map_err(crate::files::io_error)?;
        // If interrupted before every parent copy is closed, all later launch
        // sites refuse admission rather than inherit a live output writer.
        super::super::super::INHERITANCE_UNCERTAIN.store(true, Ordering::Release);
        let inherited = handles.iter().try_for_each(|handle| {
            // SAFETY: the short global launch guard owns this inheritance window.
            if unsafe { SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 { Err(failure("stdio inheritance", last_code())) } else { Ok(()) }
        });
        let launched = inherited.and_then(|()| {
            // SAFETY: all strings, environment, attribute backing and handles
            // are stable. No application code executes before Job assignment.
            if unsafe { CreateProcessW(application.as_ptr(), command_line.as_mut_ptr(), ptr::null(), ptr::null(), 1, CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT, environment.as_ptr().cast(), cwd.as_ptr(), &startup.StartupInfo, &mut information) } == 0 { return Err(failure("CreateProcessW", last_code())); }
            // No fallible wrapper/registration can consume these native owners.
            self.process.value = Some(information.hProcess as usize);
            self.thread.value = Some(information.hThread as usize);
            self.spawned = true;
            Ok(())
        });
        // Close every inheritable parent copy even when a preceding close fails.
        let stdin_close = self.stdin.close();
        let stdout_close = self.outputs[0].writer.close();
        let stderr_close = self.outputs[1].writer.close();
        if stdin_close.is_ok() && stdout_close.is_ok() && stderr_close.is_ok() { super::super::super::INHERITANCE_UNCERTAIN.store(false, Ordering::Release); }
        drop(guard);
        launched?;
        stdin_close?; stdout_close?; stderr_close?;
        // SAFETY: exact retained suspended primary and live private Job.
        if unsafe { AssignProcessToJobObject(self.job.raw()?, self.process.raw()?) } == 0 { return Err(failure("AssignProcessToJobObject", last_code())); }
        self.assigned = true;
        // SAFETY: this is the original CREATE_SUSPENDED primary thread. Only a
        // previous count of1 proves our single resume enabled execution.
        let resumed = unsafe { ResumeThread(self.thread.raw()?) };
        if resumed != 1 { return Err(failure("ResumeThread", if resumed == u32::MAX { last_code() } else { resumed })); }
        self.thread.close()?;
        Ok((stdout, stderr))
    }
    #[expect(unsafe_code, reason = "Observe exact process and private Job accounting; terminate retained Job or never-resumed primary without opaque waiters")]
    pub(in crate::process::media_session) fn observe(&mut self, cancel: bool) -> Result<Option<std::process::ExitStatus>> {
        if !self.spawned {
            self.stdin.close()?;
            for output in &mut self.outputs { output.writer.close()?; }
            self.job.close()?;
            return Ok(None);
        }
        if self.exit.is_none() {
            // SAFETY: nonblocking observation while the exact process is owned.
            let wait = unsafe { WaitForSingleObject(self.process.raw()?, 0) };
            match wait {
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    // SAFETY: the process is signaled, so even STILL_ACTIVE259
                    // is an actual exit code rather than a running-state guess.
                    if unsafe { GetExitCodeProcess(self.process.raw()?, &mut code) } == 0 { return Err(failure("GetExitCodeProcess", last_code())); }
                    self.exit = Some(std::process::ExitStatus::from_raw(code));
                },
                WAIT_TIMEOUT => {},
                other => return Err(failure("process wait", if other == WAIT_FAILED { last_code() } else { other })),
            }
        }
        if (cancel || self.exit.is_some()) && !self.terminated {
            // SAFETY: unassigned process never resumed, so it has no descendants.
            // An assigned tree is terminated only through its owned private Job.
            // Assignment can itself fail by terminating the suspended process.
            // A signaled, never-resumed primary needs no second TerminateProcess
            // call, which otherwise fails for an already terminated process.
            let success = if self.assigned {
                unsafe { TerminateJobObject(self.job.raw()?, 1) }
            } else if self.exit.is_some() { 1 } else {
                unsafe { TerminateProcess(self.process.raw()?, 1) }
            };
            if success == 0 { return Err(failure("tree termination", last_code())); }
            self.terminated = true;
        }
        if self.exit.is_some() {
            self.thread.close()?;
            self.process.close()?;
            if !self.tree_empty {
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                let mut returned = 0;
                // SAFETY: exact structure and size, private Job remains live;
                // an arbitrary completion-port message is never closure proof.
                if unsafe { QueryInformationJobObject(self.job.raw()?, JobObjectBasicAccountingInformation, (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(), std::mem::size_of_val(&accounting) as u32, &mut returned) } == 0 { return Err(failure("job accounting", last_code())); }
                if returned as usize != std::mem::size_of_val(&accounting) { return Err(error("TREE_SETTLEMENT_UNCERTAIN", "Unexpected Job accounting size")); }
                self.tree_empty = accounting.ActiveProcesses == 0;
            }
            if self.tree_empty { self.job.close()?; }
        }
        Ok(self.exit)
    }
    pub(in crate::process::media_session) fn pump_outputs(&mut self, cancel: bool) -> Result<()> {
        // Attempt both independent pipes even if one reports an error.
        let first = self.outputs[0].pump(cancel);
        let second = self.outputs[1].pump(cancel);
        first.and(second)
    }
    pub(in crate::process::media_session) fn outputs_settled(&self) -> bool { self.outputs.iter().all(OutputPipe::settled) }
    pub(in crate::process::media_session) fn settled(&self) -> bool { self.process.absent() && self.thread.absent() && self.job.absent() && self.stdin.absent() && (!self.spawned || (self.exit.is_some() && self.tree_empty)) }
}

fn wide(value: &std::ffi::OsStr) -> Result<Vec<u16>> {
    let mut result = value.encode_wide().collect::<Vec<_>>();
    if result.len() > 32766 || result.contains(&0) { return Err(error("INVOCATION_DENIED", "Invalid Windows launch text")); }
    result.push(0); Ok(result)
}
fn command_line(program: &Path, args: &[String]) -> Result<Vec<u16>> {
    let program = program.as_os_str().encode_wide().collect::<Vec<_>>();
    if program.contains(&0) || program.contains(&(b'"' as u16)) { return Err(error("INVOCATION_DENIED", "Invalid executable text")); }
    let mut result = vec![b'"' as u16]; result.extend_from_slice(&program); result.push(b'"' as u16);
    for argument in args {
        result.push(b' ' as u16); result.push(b'"' as u16);
        let mut slashes = 0;
        for unit in argument.encode_utf16() {
            if unit == b'\\' as u16 { slashes += 1; continue; }
            result.extend(std::iter::repeat_n(b'\\' as u16, if unit == b'"' as u16 { slashes * 2 + 1 } else { slashes }));
            slashes = 0; result.push(unit);
        }
        result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2)); result.push(b'"' as u16);
    }
    if result.len() > 32766 || result.contains(&0) { return Err(error("INVOCATION_DENIED", "Windows command line exceeds bound")); }
    result.push(0); Ok(result)
}
fn environment() -> Result<Vec<u16>> {
    let mut result = Vec::new();
    for key in ["LANG", "SystemRoot", "TEMP", "TMP", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value.encode_wide().collect::<Vec<_>>();
            if value.contains(&0) { return Err(error("INVOCATION_DENIED", "Invalid environment value")); }
            result.extend(key.encode_utf16()); result.push(b'=' as u16); result.extend(value); result.push(0);
        }
    }
    if result.len() > 32765 { return Err(error("INVOCATION_DENIED", "Windows environment exceeds bound")); }
    if result.is_empty() { result.push(0); }
    result.push(0); Ok(result)
}
