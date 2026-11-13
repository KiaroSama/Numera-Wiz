//! Linux-only bounded native execution using the already locked rustix APIs.
use anyhow::{Context, Result, ensure};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub const DIAGNOSTIC_LIMIT: u64 = 16 * 1024 * 1024;
pub const FILE_LIMIT: u64 = 128 * 1024 * 1024;
static WATCH_PRIVATE: AtomicBool = AtomicBool::new(false);
static MAX_SCRATCH_BYTES: AtomicU64 = AtomicU64::new(0);
static MAX_SCRATCH_FILES: AtomicU64 = AtomicU64::new(0);

pub fn begin_worker_limits() -> Result<u64> {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
        let previous = getrlimit(Resource::Fsize);
        let limit = FILE_LIMIT
            .min(previous.current.unwrap_or(u64::MAX))
            .min(previous.maximum.unwrap_or(u64::MAX));
        setrlimit(
            Resource::Fsize,
            Rlimit {
                current: Some(limit),
                maximum: Some(limit),
            },
        )?;
        let actual = getrlimit(Resource::Fsize);
        ensure!(
            actual.current == Some(limit) && actual.maximum == Some(limit),
            "worker inherited file-size limit not established"
        );
        WATCH_PRIVATE.store(true, Ordering::SeqCst);
        Ok(limit)
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("worker file limits are Linux-only")
}

pub fn scratch_observation() -> (u64, u64) {
    (
        MAX_SCRATCH_BYTES.load(Ordering::SeqCst),
        MAX_SCRATCH_FILES.load(Ordering::SeqCst),
    )
}

fn sample_private_scratch() -> Result<()> {
    if !WATCH_PRIVATE.load(Ordering::SeqCst) {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_millis(50);
    let mut queue = vec![
        (std::path::PathBuf::from("/capture/lists"), 0),
        (std::path::PathBuf::from("/capture/cache"), 0),
    ];
    let mut count = 0_u64;
    let mut total = 0_u64;
    let mut visited = 0_u64;
    if let Ok(entries) = std::fs::read_dir("/capture") {
        for entry in entries {
            let entry = entry?;
            visited += 1;
            ensure!(visited <= 1000, "scratch root entry bound exceeded");
            if entry.file_name().to_string_lossy().starts_with("download-") {
                queue.push((entry.path(), 0));
            }
        }
    }
    while let Some((path, depth)) = queue.pop() {
        ensure!(
            depth <= 8 && Instant::now() < deadline,
            "scratch walk depth/time bound exceeded"
        );
        visited = visited.checked_add(1).context("scratch entry overflow")?;
        ensure!(visited <= 1000, "scratch entry bound exceeded");
        let entry = match std::fs::symlink_metadata(&path) {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        ensure!(
            !entry.file_type().is_symlink(),
            "symlink in private native scratch"
        );
        if entry.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                ensure!(
                    queue.len() < 1000 && Instant::now() < deadline,
                    "scratch queue/time bound exceeded"
                );
                queue.push((entry?.path(), depth + 1));
            }
        } else {
            ensure!(
                entry.is_file() && entry.len() <= FILE_LIMIT,
                "special/oversized native scratch file"
            );
            count = count
                .checked_add(1)
                .context("scratch file count overflow")?;
            ensure!(count <= 500, "native scratch file count exceeded");
            total = check_count(total, entry.len(), 512 * 1024 * 1024)?;
        }
    }
    MAX_SCRATCH_BYTES.fetch_max(total, Ordering::SeqCst);
    MAX_SCRATCH_FILES.fetch_max(count, Ordering::SeqCst);
    Ok(())
}

pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub failure: Option<String>,
}

impl Output {
    pub fn success(&self) -> Result<()> {
        ensure!(
            self.failure.is_none(),
            "{}; bounded captured output is diagnostic evidence only",
            self.failure.as_deref().unwrap_or("supervision failed")
        );
        ensure!(
            self.status.success(),
            "native command failed ({}): {} (bounded excerpt; captured diagnostics are separate evidence)",
            self.status,
            String::from_utf8_lossy(&self.stderr[..self.stderr.len().min(2048)])
        );
        Ok(())
    }
    pub fn text(&self) -> Result<&str> {
        Ok(std::str::from_utf8(&self.stdout)?)
    }
}

pub fn check_count(previous: u64, additional: u64, limit: u64) -> Result<u64> {
    let total = previous
        .checked_add(additional)
        .context("byte count overflow")?;
    ensure!(
        total <= limit,
        "output exceeded {limit} bytes; incomplete output is refused"
    );
    Ok(total)
}

pub fn signal_target(pid: u32) -> Result<i32> {
    ensure!(pid > 1, "refuse group signal/wait for PID zero or one");
    Ok(i32::try_from(pid)?)
}

pub fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("HOME", "/nonexistent")
        .env("TZ", "UTC")
        .stdin(Stdio::null());
    cmd
}

fn prepare_supervision() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // rustix 1.1.6 exposes PR_SET_CHILD_SUBREAPER's boolean as Option<Pid>.
        // INIT's numeric one sets true; it is NEVER a group-signal target.
        use rustix::process::{Pid, child_subreaper, set_child_subreaper};
        set_child_subreaper(Some(Pid::INIT))?;
        ensure!(
            child_subreaper()? == Some(Pid::INIT),
            "subreaper setup not established"
        );
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("capture process supervision is Linux-only")
}

#[cfg(target_os = "linux")]
fn owned_pid(pid: u32) -> Result<rustix::process::Pid> {
    rustix::process::Pid::from_raw(signal_target(pid)?).context("invalid owned PID")
}

fn exited_without_reaping(pid: u32) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        Ok(waitid(
            WaitId::Pid(owned_pid(pid)?),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )?
        .is_some())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        anyhow::bail!("Linux waitid unavailable")
    }
}

fn signal_unreaped_group(pid: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::{
            io::Errno,
            process::{Signal, kill_process_group},
        };
        match kill_process_group(owned_pid(pid)?, Signal::KILL) {
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        anyhow::bail!("Linux group signalling unavailable")
    }
}

fn reap_group(pid: u32, deadline: Instant) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::{
            io::Errno,
            process::{WaitOptions, waitpgid},
        };
        loop {
            match waitpgid(owned_pid(pid)?, WaitOptions::NOHANG) {
                Err(Errno::CHILD) => return Ok(()),
                Ok(Some(_)) => {}
                Ok(None) => {
                    ensure!(
                        Instant::now() < deadline,
                        "owned descendants not reaped within cleanup budget; state UNKNOWN"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, deadline);
        anyhow::bail!("Linux scoped reaping unavailable")
    }
}

struct OwnedChild {
    child: Child,
    reaped: bool,
    deadline: Instant,
}

impl OwnedChild {
    fn terminate(&mut self, deadline: Instant) -> Result<ExitStatus> {
        // waitid(NOWAIT) keeps the leader's PID allocated until this signal.
        // Neither Child::try_wait nor a reaping wait runs before the group kill.
        // This object owns the sole Child handle; after reaping no group signal
        // is permitted. An exited leader therefore cannot yield a reused PGID.
        // A waitid error (especially ECHILD) loses the ownership proof: do not
        // signal that numeric PID/PGID at all. Container cleanup remains separate.
        exited_without_reaping(self.child.id())
            .context("owned leader wait relationship unproven; numeric signals refused")?;
        let signalled = signal_unreaped_group(self.child.id());
        let _ = self.child.kill();
        let status = loop {
            if let Some(status) = self.child.try_wait()? {
                self.reaped = true;
                break status;
            }
            ensure!(
                Instant::now() < deadline,
                "owned child termination/reaping UNKNOWN at cleanup deadline"
            );
            thread::sleep(Duration::from_millis(5));
        };
        signalled?;
        reap_group(self.child.id(), deadline)?;
        Ok(status)
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Covers post-spawn errors, including pipe setup. A successfully reaped
        // leader is never signalled. This final bounded attempt reports no proof.
        if !self.reaped {
            let _ = self.terminate(self.deadline);
        }
    }
}

fn nonblocking(
    stdout: &std::process::ChildStdout,
    stderr: &std::process::ChildStderr,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
        fcntl_setfl(stdout, fcntl_getfl(stdout)? | OFlags::NONBLOCK)?;
        fcntl_setfl(stderr, fcntl_getfl(stderr)? | OFlags::NONBLOCK)?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (stdout, stderr);
        anyhow::bail!("Linux nonblocking pipes unavailable")
    }
}

fn reader(
    mut input: impl Read,
    mut file: Option<File>,
    limit: u64,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut size = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        ensure!(
            !stop.load(Ordering::SeqCst),
            "pipe did not reach EOF within bounded drain; incomplete output refused"
        );
        let count = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        size = check_count(size, count as u64, limit)?;
        if let Some(file) = &mut file {
            file.write_all(&buffer[..count])?;
        } else {
            output.extend_from_slice(&buffer[..count]);
        }
    }
    if let Some(file) = file {
        file.sync_all()?;
    }
    Ok(output)
}

pub fn run(
    cmd: &mut Command,
    deadline: Instant,
    seconds: u64,
    stream: Option<&Path>,
) -> Result<Output> {
    prepare_supervision()?;
    // Cleanup and bounded reader drain are INSIDE each command's time cap.
    let end = deadline.min(Instant::now() + Duration::from_secs(seconds));
    ensure!(
        end.saturating_duration_since(Instant::now()) > Duration::from_secs(1),
        "capture deadline exhausted before command"
    );
    let work_end = end - Duration::from_millis(750);
    let streaming = stream.is_some();
    let destination = stream
        .map(|path| OpenOptions::new().write(true).create_new(true).open(path))
        .transpose()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn bounded native command")?;
    let mut owned = OwnedChild {
        child,
        reaped: false,
        deadline: end,
    };
    let stdout = owned.child.stdout.take().context("native stdout pipe")?;
    let stderr = owned.child.stderr.take().context("native stderr pipe")?;
    nonblocking(&stdout, &stderr)?;
    let stop = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let out_stop = Arc::clone(&stop);
    let out_failed = Arc::clone(&failed);
    let out_sender = sender.clone();
    let out = thread::spawn(move || {
        let result = reader(
            stdout,
            destination,
            if streaming {
                FILE_LIMIT
            } else {
                DIAGNOSTIC_LIMIT
            },
            &out_stop,
        );
        if result.is_err() {
            out_failed.store(true, Ordering::SeqCst);
        }
        let _ = out_sender.send((true, result));
    });
    let err_stop = Arc::clone(&stop);
    let err_failed = Arc::clone(&failed);
    let err = thread::spawn(move || {
        let result = reader(stderr, None, DIAGNOSTIC_LIMIT, &err_stop);
        if result.is_err() {
            err_failed.store(true, Ordering::SeqCst);
        }
        let _ = sender.send((false, result));
    });
    let mut failure = None;
    let mut next_sample = Instant::now();
    loop {
        if Instant::now() >= next_sample {
            if let Err(error) = sample_private_scratch() {
                failure = Some(format!("private scratch bound: {error:#}"));
                break;
            }
            next_sample = Instant::now() + Duration::from_millis(50);
        }
        match exited_without_reaping(owned.child.id()) {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => {
                failure = Some(format!("nonreaping native wait failed: {error}"));
                break;
            }
        }
        if failed.load(Ordering::SeqCst) || Instant::now() >= work_end {
            failure =
                Some("native output failure or deadline exceeded; no truncation accepted".into());
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let termination = owned.terminate(end - Duration::from_millis(100));
    let mut stdout = None;
    let mut stderr = None;
    for _ in 0..2 {
        match receiver.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok((true, result)) => stdout = Some(result),
            Ok((false, result)) => stderr = Some(result),
            Err(_) => {
                failure = Some("native pipe drain incomplete at deadline".into());
                break;
            }
        }
    }
    stop.store(true, Ordering::SeqCst);
    // Nonblocking readers check stop between each bounded read/WouldBlock wait.
    out.join()
        .map_err(|_| anyhow::anyhow!("stdout reader panicked"))?;
    err.join()
        .map_err(|_| anyhow::anyhow!("stderr reader panicked"))?;
    let status = termination?;
    Ok(Output {
        status,
        stdout: stdout.context("missing complete stdout")??,
        stderr: stderr.context("missing complete stderr")??,
        failure,
    })
}
