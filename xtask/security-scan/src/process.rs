//! Dedicated Linux main-thread ownership: retained leaders, subreaping, pidfds,
//! and actual wait-any ECHILD before scratch deletion. Never mixes std waits.
use crate::{PrivateResult, Result, ScanFailure, private, require};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    io::Errno,
    process::{self as os, Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, WaitOptions},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{ErrorKind, Read},
    os::unix::process::CommandExt,
    panic::{AssertUnwindSafe, catch_unwind},
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const GRACE: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(10);
// Linux v6.8 include/uapi/linux/wait.h: include non-SIGCHLD clone children too.
const WALL: u32 = 0x4000_0000;
static CANCEL: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn wait_options() -> WaitOptions {
    WaitOptions::NOHANG | WaitOptions::from_bits_retain(WALL)
}
fn observe_options() -> WaitIdOptions {
    WaitIdOptions::EXITED
        | WaitIdOptions::NOHANG
        | WaitIdOptions::NOWAIT
        | WaitIdOptions::from_bits_retain(WALL)
}

pub struct NoChildren {
    owner: Pid,
}
impl NoChildren {
    pub fn recheck(&self) -> Result<()> {
        require(self.owner == os::getpid(), "SECURITY_WAIT_OWNER_INVALID")?;
        barrier().map(|_| ())
    }
}
pub fn barrier() -> Result<NoChildren> {
    match os::wait(wait_options()) {
        Err(Errno::CHILD) => Ok(NoChildren {
            owner: os::getpid(),
        }),
        _ => Err(ScanFailure("SECURITY_CHILDREN_REMAIN")),
    }
}

fn kernel_bytes(path: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(private::MIB + 1).read_to_end(&mut bytes))
        .private("SECURITY_PROC_UNAVAILABLE")?;
    require(bytes.len() as u64 <= private::MIB, "SECURITY_PROC_OVERFLOW")?;
    Ok(bytes)
}

pub fn signal_state() -> Result<(u64, u64, u64)> {
    let bytes = kernel_bytes("/proc/self/status")?;
    let text = std::str::from_utf8(&bytes).private("SECURITY_SIGNAL_STATE_INVALID")?;
    let field = |name: &str| -> Result<u64> {
        let mut values = text.lines().filter_map(|line| line.strip_prefix(name));
        let value = values
            .next()
            .ok_or(ScanFailure("SECURITY_SIGNAL_STATE_INVALID"))?
            .trim();
        require(
            values.next().is_none()
                && !value.is_empty()
                && value.len() <= 16
                && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "SECURITY_SIGNAL_STATE_INVALID",
        )?;
        u64::from_str_radix(value, 16).private("SECURITY_SIGNAL_STATE_INVALID")
    };
    Ok((field("SigBlk:")?, field("SigIgn:")?, field("SigCgt:")?))
}
pub fn initialize() -> Result<()> {
    let pid = os::getpid();
    let main = std::fs::read_link("/proc/thread-self").private("SECURITY_THREAD_INVALID")?;
    require(
        main.ends_with(format!("task/{}", pid.as_raw_pid())),
        "SECURITY_THREAD_INVALID",
    )?;
    let (blocked, ignored, caught) = signal_state()?;
    let bit = |sig: i32| 1_u64 << (sig - 1);
    let cancellation = bit(signal_hook::consts::SIGINT)
        | bit(signal_hook::consts::SIGTERM)
        | bit(signal_hook::consts::SIGHUP);
    let chld = bit(signal_hook::consts::SIGCHLD);
    require(
        blocked & cancellation == 0 && ignored & (cancellation | chld) == 0 && caught & chld == 0,
        "SECURITY_SIGNAL_STATE_REJECTED",
    )?;
    os::set_child_subreaper(Some(Pid::INIT)).private("SECURITY_SUBREAPER_UNAVAILABLE")?;
    require(
        os::child_subreaper().private("SECURITY_SUBREAPER_UNAVAILABLE")? == Some(Pid::INIT),
        "SECURITY_SUBREAPER_UNAVAILABLE",
    )?;
    if CANCEL.get().is_none() {
        let cancelled = Arc::new(AtomicBool::new(false));
        for signal in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            signal_hook::flag::register(signal, Arc::clone(&cancelled))
                .private("SECURITY_SIGNAL_SETUP_FAILED")?;
        }
        CANCEL
            .set(cancelled)
            .map_err(|_| ScanFailure("SECURITY_SIGNAL_SETUP_FAILED"))?;
    }
    Ok(())
}
pub fn cancelled() -> bool {
    CANCEL
        .get()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

#[derive(Default)]
pub struct Capture {
    pub bytes: Vec<u8>,
    pub nonwhite: bool,
    pub overflow: bool,
    pub failed: bool,
}
impl Capture {
    pub fn consume(&mut self, bytes: &[u8]) {
        self.nonwhite |= bytes.iter().any(|b| !b.is_ascii_whitespace());
        let room = private::CAPTURE_LIMIT.saturating_sub(self.bytes.len());
        self.overflow |= bytes.len() > room;
        self.bytes
            .extend_from_slice(&bytes[..bytes.len().min(room)]);
    }
    pub fn quiet(&self) -> bool {
        !self.nonwhite && !self.overflow && !self.failed
    }
}

pub struct Options {
    pub budget: Duration,
    pub deadline: Instant,
    /// Proxy Git must remain in the scanner's group; it owns only its direct pidfd.
    pub inherited_group: bool,
    /// Only the authenticated Git proxy may use unchanged binary stdout forwarding.
    pub forward_stdout: bool,
}
impl Options {
    pub fn command(budget: Duration, deadline: Instant) -> Self {
        Self {
            budget,
            deadline,
            inherited_group: false,
            forward_stdout: false,
        }
    }
}
pub struct Outcome {
    pub leader: i32,
    pub status: Option<i32>,
    pub stdout: Capture,
    pub stderr: Capture,
    pub failure: Option<ScanFailure>,
    pub reaped: usize,
    pub adopted: usize,
    pub barrier: NoChildren,
    pub cleanup_elapsed: Duration,
}
impl Outcome {
    /// Completed normal exit, verified wait ownership and complete bounded
    /// captures. Nonwhite output alone is allowed here for private Git errors.
    pub fn completed(&self) -> bool {
        self.failure.is_none()
            && self
                .status
                .is_some_and(|status| (0..=255).contains(&status))
            && self.reaped > 0
            && self.adopted == 0
            && !self.stdout.failed
            && !self.stdout.overflow
            && !self.stderr.failed
            && !self.stderr.overflow
    }
    pub fn completed_nonzero(&self) -> bool {
        self.completed() && self.status.is_some_and(|status| status != 0)
    }
    pub fn successful(&self) -> Result<()> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        require(self.status == Some(0), "SECURITY_CHILD_FAILED")
    }
}
struct Running {
    child: Option<Child>,
    pid: Option<Pid>,
    leader_fd: Option<OwnedFd>,
    group: Option<Pid>,
    stdout_pipe: Option<ChildStdout>,
    stderr_pipe: Option<ChildStderr>,
    stdout: Capture,
    stderr: Capture,
    records: BTreeMap<i32, OwnedFd>,
    status: Option<i32>,
    leader_exited: bool,
    reaped: usize,
    adopted: usize,
    failure: Option<ScanFailure>,
}
impl Running {
    fn fail(&mut self, error: ScanFailure) {
        self.failure.get_or_insert(error);
    }
    fn drain(&mut self) {
        fn stream<R: Read>(pipe: &mut Option<R>, capture: &mut Capture) {
            let mut buffer = [0; 16 * 1024];
            for _ in 0..4 {
                let Some(reader) = pipe.as_mut() else {
                    break;
                };
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        *pipe = None;
                        break;
                    }
                    Ok(size) => capture.consume(&buffer[..size]),
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => {
                        capture.failed = true;
                        *pipe = None;
                        break;
                    }
                }
            }
        }
        stream(&mut self.stdout_pipe, &mut self.stdout);
        stream(&mut self.stderr_pipe, &mut self.stderr);
        if self.stdout.overflow || self.stderr.overflow || self.stdout.failed || self.stderr.failed
        {
            self.fail(ScanFailure("SECURITY_CAPTURE_FAILED"));
        }
    }
    fn observe(&mut self) -> Result<()> {
        let pid = self.pid.ok_or(ScanFailure("SECURITY_WAIT_OWNER_INVALID"))?;
        if let Some(status) =
            os::waitid(WaitId::Pid(pid), observe_options()).private("SECURITY_WAIT_FAILED")?
        {
            self.leader_exited = true;
            self.status = status.exit_status();
        }
        Ok(())
    }
    fn discover(&mut self, fault: Option<FixtureFault>) -> Result<()> {
        if fault == Some(FixtureFault::Discovery) {
            return Err(ScanFailure("SECURITY_INJECTED_DISCOVERY_FAILURE"));
        }
        let bytes = kernel_bytes(&format!(
            "/proc/self/task/{}/children",
            os::getpid().as_raw_pid()
        ))?;
        let text = std::str::from_utf8(&bytes).private("SECURITY_PROC_INVALID")?;
        for token in text.split_ascii_whitespace() {
            require(
                token.len() <= 10 && token.bytes().all(|b| b.is_ascii_digit()),
                "SECURITY_PROC_INVALID",
            )?;
            let raw: i32 = token.parse().private("SECURITY_PROC_INVALID")?;
            require(
                raw > 1 && raw != os::getpid().as_raw_pid(),
                "SECURITY_PROC_INVALID",
            )?;
            let pid = Pid::from_raw(raw).ok_or(ScanFailure("SECURITY_PROC_INVALID"))?;
            if Some(pid) == self.pid {
                continue;
            }
            let has_capacity = self.records.len() < private::MAX_RECORDS;
            if let std::collections::btree_map::Entry::Vacant(entry) = self.records.entry(raw) {
                require(has_capacity, "SECURITY_PROCESS_RECORD_OVERFLOW")?;
                let fd = match os::pidfd_open(pid, PidfdFlags::empty()) {
                    Ok(fd) => fd,
                    Err(Errno::SRCH) => continue,
                    Err(_) => return Err(ScanFailure("SECURITY_PIDFD_FAILED")),
                };
                if fault == Some(FixtureFault::Probe) {
                    return Err(ScanFailure("SECURITY_INJECTED_PROBE_FAILURE"));
                }
                // Only the kernel's wait-parenthood probe authorizes an adopted pidfd.
                match os::waitid(WaitId::PidFd(fd.as_fd()), observe_options()) {
                    Ok(_) => {
                        entry.insert(fd);
                    }
                    Err(Errno::CHILD) => continue,
                    Err(_) => return Err(ScanFailure("SECURITY_WAIT_FAILED")),
                }
            }
        }
        Ok(())
    }
    fn signal_adopted(&self, signal: Signal) -> Result<()> {
        let mut result = Ok(());
        for fd in self.records.values() {
            match os::pidfd_send_signal(fd, signal) {
                Ok(()) | Err(Errno::SRCH) => {}
                Err(_) => result = Err(ScanFailure("SECURITY_SIGNAL_FAILED")),
            }
        }
        result
    }
    fn direct_signal(&self, signal: Signal) -> Result<()> {
        let result = if let Some(group) = self.group {
            require(
                group.as_raw_pid() > 1
                    && group != os::getpgid(None).private("SECURITY_GROUP_INVALID")?,
                "SECURITY_GROUP_INVALID",
            )?;
            os::kill_process_group(group, signal)
        } else if let Some(fd) = &self.leader_fd {
            os::pidfd_send_signal(fd, signal)
        } else if let Some(pid) = self.pid {
            // This is the retained direct Child, never a /proc hint. No wait is
            // consumed before the final direct signal, so its PID cannot recycle.
            os::kill_process(pid, signal)
        } else {
            return Err(ScanFailure("SECURITY_PIDFD_FAILED"));
        };
        match result {
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            Err(_) => Err(ScanFailure("SECURITY_SIGNAL_FAILED")),
        }
    }
    fn reap(&mut self) -> Result<bool> {
        loop {
            match os::wait(wait_options()) {
                Ok(Some((pid, status))) => {
                    self.reaped += 1;
                    if Some(pid) == self.pid {
                        self.status = status.exit_status();
                    } else {
                        self.adopted += 1;
                    }
                    self.records.remove(&pid.as_raw_pid());
                }
                Ok(None) => return Ok(false),
                Err(Errno::CHILD) => return Ok(true),
                Err(Errno::INTR) => continue,
                Err(_) => return Err(ScanFailure("SECURITY_WAIT_FAILED")),
            }
        }
    }
    fn cleanup(&mut self, deadline: Instant, fault: Option<FixtureFault>) -> Result<NoChildren> {
        let start = Instant::now();
        let end = (start + GRACE).min(deadline);
        if let Err(e) = self.direct_signal(Signal::Term) {
            self.fail(e);
        }
        let mut killed = false;
        loop {
            self.drain();
            // No leader is reaped until the last numeric group signal is retired.
            if !killed
                && (start.elapsed() >= Duration::from_secs(1)
                    || (self.leader_exited
                        && self.stdout_pipe.is_none()
                        && self.stderr_pipe.is_none())
                    || Instant::now() + Duration::from_millis(100) >= end)
            {
                if let Err(e) = self.direct_signal(Signal::Kill) {
                    self.fail(e);
                }
                self.group = None;
                killed = true;
            }
            let discovered = self.discover(fault);
            if let Err(e) = discovered {
                self.fail(e);
            }
            // Existing owned handles remain actionable even if discovery fails.
            if let Err(e) = self.signal_adopted(if killed { Signal::Kill } else { Signal::Term }) {
                self.fail(e);
            }
            if killed {
                match self.reap() {
                    Ok(true) if self.stdout_pipe.is_none() && self.stderr_pipe.is_none() => {
                        if fault == Some(FixtureFault::CleanupBarrier) {
                            return Err(ScanFailure("SECURITY_CLEANUP_UNVERIFIED"));
                        }
                        return barrier();
                    }
                    Ok(_) => {}
                    Err(e) => self.fail(e),
                }
            }
            if Instant::now() >= end {
                return Err(ScanFailure("SECURITY_CLEANUP_UNVERIFIED"));
            }
            thread::sleep(TICK);
        }
    }
}

pub fn run(
    command: &mut Command,
    options: Options,
    monitor: impl FnMut() -> Result<()>,
) -> Result<Outcome> {
    run_inner(command, options, monitor, None)
}
/// Evaluator-only failure injection: never selects executables or weakens success.
/// The production dispatch has no argument/environment route to this function.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FixtureFault {
    Subreaper,
    Registration,
    Discovery,
    Probe,
    CleanupBarrier,
}
pub fn run_fixture_fault(
    command: &mut Command,
    options: Options,
    fault: FixtureFault,
) -> Result<Outcome> {
    run_inner(command, options, || Ok(()), Some(fault))
}
fn run_inner(
    command: &mut Command,
    options: Options,
    mut monitor: impl FnMut() -> Result<()>,
    fault: Option<FixtureFault>,
) -> Result<Outcome> {
    if fault == Some(FixtureFault::Subreaper) {
        return Err(ScanFailure("SECURITY_INJECTED_SUBREAPER_FAILURE"));
    }
    initialize()?;
    barrier()?;
    require(!cancelled(), "SECURITY_CANCELLED")?;
    let usable = options
        .deadline
        .checked_sub(GRACE)
        .ok_or(ScanFailure("SECURITY_DEADLINE"))?;
    require(Instant::now() < usable, "SECURITY_DEADLINE")?;
    let end = (Instant::now() + options.budget).min(usable);
    let mut state = Running {
        child: None,
        pid: None,
        leader_fd: None,
        group: None,
        stdout_pipe: None,
        stderr_pipe: None,
        stdout: Capture {
            bytes: Vec::with_capacity(private::CAPTURE_LIMIT),
            ..Capture::default()
        },
        stderr: Capture {
            bytes: Vec::with_capacity(private::CAPTURE_LIMIT),
            ..Capture::default()
        },
        records: BTreeMap::new(),
        status: None,
        leader_exited: false,
        reaped: 0,
        adopted: 0,
        failure: None,
    };
    command.stdin(Stdio::null()).stderr(Stdio::piped());
    command.stdout(if options.forward_stdout {
        Stdio::inherit()
    } else {
        Stdio::piped()
    });
    if !options.inherited_group {
        command.process_group(0);
    }
    // Record immediately: no callback/allocation between receiving Child and ownership.
    state.child = Some(command.spawn().private("SECURITY_SPAWN_FAILED")?);
    let execution = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        let child = state
            .child
            .as_mut()
            .ok_or(ScanFailure("SECURITY_WAIT_OWNER_INVALID"))?;
        let pid = Pid::from_child(child);
        state.pid = Some(pid);
        if !options.inherited_group {
            state.group = Some(pid);
        }
        if fault == Some(FixtureFault::Registration) {
            return Err(ScanFailure("SECURITY_INJECTED_REGISTRATION_FAILURE"));
        }
        state.leader_fd =
            Some(os::pidfd_open(pid, PidfdFlags::empty()).private("SECURITY_PIDFD_FAILED")?);
        state.stdout_pipe = child.stdout.take();
        state.stderr_pipe = child.stderr.take();
        if let Some(pipe) = &state.stdout_pipe {
            fcntl_setfl(
                pipe,
                fcntl_getfl(pipe).private("SECURITY_CAPTURE_FAILED")? | OFlags::NONBLOCK,
            )
            .private("SECURITY_CAPTURE_FAILED")?;
        }
        if let Some(pipe) = &state.stderr_pipe {
            fcntl_setfl(
                pipe,
                fcntl_getfl(pipe).private("SECURITY_CAPTURE_FAILED")? | OFlags::NONBLOCK,
            )
            .private("SECURITY_CAPTURE_FAILED")?;
        }
        loop {
            state.drain();
            monitor()?;
            state.observe()?;
            if let Some(error) = state.failure {
                return Err(error);
            }
            require(!cancelled(), "SECURITY_CANCELLED")?;
            require(Instant::now() < end, "SECURITY_CHILD_TIMEOUT")?;
            if state.leader_exited {
                return Ok(());
            }
            thread::sleep(TICK);
        }
    }));
    match execution {
        Ok(Ok(())) => {}
        Ok(Err(e)) => state.fail(e),
        Err(_) => state.fail(ScanFailure("SECURITY_CHILD_PANIC")),
    }
    // If pipe setup failed, close all ends before cleanup; never attempt blocking reads.
    if state.failure.is_some_and(|e| {
        matches!(
            e.0,
            "SECURITY_CAPTURE_FAILED"
                | "SECURITY_PIDFD_FAILED"
                | "SECURITY_INJECTED_REGISTRATION_FAILURE"
        )
    }) {
        state.stdout_pipe = None;
        state.stderr_pipe = None;
        if let Some(child) = state.child.as_mut() {
            let _ = child.stdout.take();
            let _ = child.stderr.take();
        }
    }
    let cleanup_start = Instant::now();
    let barrier = state.cleanup(options.deadline, fault)?;
    let cleanup_elapsed = cleanup_start.elapsed();
    if state.adopted > 0 {
        state.fail(ScanFailure("SECURITY_UNEXPECTED_DESCENDANT"));
    }
    Ok(Outcome {
        leader: state
            .pid
            .ok_or(ScanFailure("SECURITY_WAIT_OWNER_INVALID"))?
            .as_raw_pid(),
        status: state.status,
        stdout: state.stdout,
        stderr: state.stderr,
        failure: state.failure,
        reaped: state.reaped,
        adopted: state.adopted,
        barrier,
        cleanup_elapsed,
    })
}
