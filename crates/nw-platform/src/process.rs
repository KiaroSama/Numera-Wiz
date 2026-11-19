//! Bounded leaf execution; arguments are never executed as shell text.
mod input_relay;
mod media_session;
pub use media_session::{
    AsyncChunkSink, ByteInput, ClosedSession, InvocationSpec, MediaProcessSession, PreparedRelays,
    PumpReceipt, RegistryStatus, RelayInputId, SessionArg, SessionCharge, SessionFailure,
    SessionId, SessionOutput, SessionPolicy, SessionResult, Settlement, SinkFuture, StdoutMode,
    reconcile_finished_sessions, session_registry_status,
};
#[cfg(test)]
pub(crate) static RELAY_CASE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(all(test, windows))]
pub(crate) use input_relay::run_native_lifetime_cases;
#[cfg(test)]
pub(crate) use media_session::{direct_cases, foundation_cases};
#[cfg(windows)]
mod windows_stdio;
use nw_domain::{DomainError, Result};
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{collections::VecDeque, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;
#[cfg(windows)]
pub use windows_stdio::prevent_stdio_inheritance;

#[cfg(windows)]
static PROCESS_LAUNCH_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(windows)]
static INHERITANCE_UNCERTAIN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Serialize the short Windows handle-inheritance window across application
/// launch sites. Process execution and pipe draining never hold this guard.
pub fn with_process_launch<T>(launch: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    #[cfg(windows)]
    let _guard = acquire_launch_guard()?;
    launch()
}
#[cfg(windows)]
fn acquire_launch_guard() -> std::io::Result<std::sync::MutexGuard<'static, ()>> {
    let stop = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        match PROCESS_LAUNCH_GATE.try_lock() {
            Ok(guard) => {
                if INHERITANCE_UNCERTAIN.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(std::io::Error::other(
                        "A previous launch retained uncertain inherited handles",
                    ));
                }
                return Ok(guard);
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(std::io::Error::other(
                    "Process launch ownership was interrupted",
                ));
            }
            Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < stop => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Process launch admission expired",
                ));
            }
        }
    }
}

#[derive(Debug)]
pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr_tail: String,
    pub exit_code: Option<i32>,
    pub diagnostic_bytes: u64,
}
struct CaptureLimits {
    stdout: usize,
    stderr: usize,
}
impl CaptureLimits {
    fn standard(stdout: usize) -> Self {
        Self {
            stdout,
            stderr: 64 * 1024,
        }
    }
}
async fn capture<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
    tail: bool,
) -> std::io::Result<(Vec<u8>, u64)> {
    let mut first = Vec::new();
    let mut recent = VecDeque::new();
    let mut buffer = [0; 8192];
    let mut bytes = 0u64;
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        bytes = bytes.saturating_add(n as u64);
        if tail {
            for value in &buffer[..n] {
                if recent.len() == limit {
                    recent.pop_front();
                }
                recent.push_back(*value);
            }
        } else {
            let keep = n.min(limit.saturating_sub(first.len()));
            first.extend_from_slice(&buffer[..keep]);
        }
    }
    Ok((
        if tail {
            recent.into_iter().collect()
        } else {
            first
        },
        bytes,
    ))
}
pub async fn run(
    program: &Path,
    args: &[String],
    deadline: Duration,
    stdout_limit: usize,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits::standard(stdout_limit),
        Stdio::null(),
        None,
        cancel,
    )
    .await
}

/// Lower the retained raw stderr budget while preserving the shared supervisor.
/// UTF-8 replacement may expand the diagnostic text; raw captured bytes obey this limit.
pub async fn run_with_output_limits(
    program: &Path,
    args: &[String],
    deadline: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits {
            stdout: stdout_limit,
            stderr: stderr_limit,
        },
        Stdio::null(),
        None,
        cancel,
    )
    .await
}

/// Inherit an already-authorized regular file without losing seekability to a pipe.
pub async fn run_readonly(
    program: &Path,
    args: &[String],
    deadline: Duration,
    stdout_limit: usize,
    input: std::fs::File,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    if !input.metadata().map_err(crate::files::io_error)?.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular input file is required",
        ));
    }
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits::standard(stdout_limit),
        Stdio::from(input),
        None,
        cancel,
    )
    .await
}

/// Execute an explicit reviewed finite/disabled policy. Service admission owns authority.
pub async fn run_readonly_with_policy(
    program: &Path,
    args: &[String],
    deadline: nw_domain::execution::DeadlinePolicy,
    stdout_limit: usize,
    input: std::fs::File,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    if !input.metadata().map_err(crate::files::io_error)?.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular input file is required",
        ));
    }
    run_with_stdin(
        program,
        args,
        deadline.duration()?,
        CaptureLimits::standard(stdout_limit),
        Stdio::from(input),
        None,
        cancel,
    )
    .await
}

pub type ChunkConsumer = Box<dyn FnMut(&[u8]) -> Result<()> + Send>;

const LEGACY_STREAM_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_VERIFIED_STREAM_BYTES: u64 = 64 * 1024 * 1024 * 1024;

enum StreamConsumer {
    Synchronous(ChunkConsumer),
    Asynchronous(Box<dyn AsyncChunkSink>),
}

struct StreamedOutput {
    max_bytes: u64,
    consumer: StreamConsumer,
}

impl StreamedOutput {
    fn legacy(consumer: ChunkConsumer) -> Self {
        Self {
            max_bytes: LEGACY_STREAM_BYTES,
            consumer: StreamConsumer::Synchronous(consumer),
        }
    }
}

/// Stream a zero-input producer without retaining stdout or inheriting stdin.
pub async fn stream_with_output_limits(
    program: &Path,
    args: &[String],
    deadline: Duration,
    stderr_limit: usize,
    consumer: ChunkConsumer,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits {
            stdout: 0,
            stderr: stderr_limit,
        },
        Stdio::null(),
        Some(StreamedOutput::legacy(consumer)),
        cancel,
    )
    .await
}

/// Drive a bounded asynchronous consumer under the same process-tree supervisor.
pub async fn stream_with_sink(
    program: &Path,
    args: &[String],
    deadline: Duration,
    limits: StreamLimits,
    sink: Box<dyn AsyncChunkSink>,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    if !(1..=MAX_VERIFIED_STREAM_BYTES).contains(&limits.stdout_bytes) {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Reviewed streamed stdout budget is outside its supported range",
        ));
    }
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits { stdout: 0, stderr: limits.stderr_bytes },
        Stdio::null(),
        Some(StreamedOutput {
            max_bytes: limits.stdout_bytes,
            consumer: StreamConsumer::Asynchronous(sink),
        }),
        cancel,
    )
    .await
}

/// Stream an admitted regular input while lowering only the raw stderr budget.
pub async fn stream_readonly_with_output_limits(
    program: &Path,
    args: &[String],
    deadline: Duration,
    input: std::fs::File,
    stderr_limit: usize,
    consumer: ChunkConsumer,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    stream_verified_readonly(
        program,
        args,
        deadline,
        input,
        StreamLimits {
            stdout_bytes: LEGACY_STREAM_BYTES,
            stderr_bytes: stderr_limit,
        },
        consumer,
        cancel,
    )
    .await
}

pub struct StreamLimits {
    pub stdout_bytes: u64,
    pub stderr_bytes: usize,
}

/// Stream the reviewed byte count through the bounded process supervisor.
pub async fn stream_verified_readonly(
    program: &Path,
    args: &[String],
    deadline: Duration,
    input: std::fs::File,
    limits: StreamLimits,
    consumer: ChunkConsumer,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    if !(1..=MAX_VERIFIED_STREAM_BYTES).contains(&limits.stdout_bytes) {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Reviewed streamed stdout budget is outside its supported range",
        ));
    }
    if !input.metadata().map_err(crate::files::io_error)?.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular input file is required",
        ));
    }
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits {
            stdout: 0,
            stderr: limits.stderr_bytes,
        },
        Stdio::from(input),
        Some(StreamedOutput {
            max_bytes: limits.stdout_bytes,
            consumer: StreamConsumer::Synchronous(consumer),
        }),
        cancel,
    )
    .await
}

/// Consume stdout incrementally under the same process-tree and cancellation boundary.
pub async fn stream_readonly(
    program: &Path,
    args: &[String],
    deadline: Duration,
    input: std::fs::File,
    consumer: ChunkConsumer,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    if !input.metadata().map_err(crate::files::io_error)?.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular input file is required",
        ));
    }
    run_with_stdin(
        program,
        args,
        Some(deadline),
        CaptureLimits::standard(0),
        Stdio::from(input),
        Some(StreamedOutput::legacy(consumer)),
        cancel,
    )
    .await
}

/// Stream a reviewed policy without accumulating packet output.
pub async fn stream_readonly_with_policy(
    program: &Path,
    args: &[String],
    deadline: nw_domain::execution::DeadlinePolicy,
    input: std::fs::File,
    consumer: ChunkConsumer,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    if !input.metadata().map_err(crate::files::io_error)?.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular input file is required",
        ));
    }
    run_with_stdin(
        program,
        args,
        deadline.duration()?,
        CaptureLimits::standard(0),
        Stdio::from(input),
        Some(StreamedOutput::legacy(consumer)),
        cancel,
    )
    .await
}

async fn consume<R: AsyncRead + Unpin>(
    mut reader: R,
    StreamedOutput {
        max_bytes,
        mut consumer,
    }: StreamedOutput,
    cancel: CancellationToken,
) -> Result<(Vec<u8>, u64)> {
    let mut buffer = [0; 8192];
    let mut total = 0u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(crate::files::io_error)?;
        if count == 0 {
            if let StreamConsumer::Asynchronous(sink) = &mut consumer {
                tokio::select! {
                    result = sink.finish() => result?,
                    _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Stream consumption was cancelled")),
                }
            }
            return Ok((Vec::new(), total));
        }
        total = total.checked_add(count as u64).ok_or_else(|| {
            DomainError::new("RESOURCE_LIMIT", "Streamed stdout counter overflow")
        })?;
        if total > max_bytes {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Streamed verification stdout exceeded the reviewed byte budget",
            ));
        }
        match &mut consumer {
            StreamConsumer::Synchronous(handle) => handle(&buffer[..count])?,
            StreamConsumer::Asynchronous(sink) => {
                tokio::select! {
                    result = sink.write(&buffer[..count]) => result?,
                    _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Stream consumption was cancelled")),
                }
            }
        }
    }
}

async fn run_with_stdin(
    program: &Path,
    args: &[String],
    deadline: Option<Duration>,
    limits: CaptureLimits,
    input: Stdio,
    consumer: Option<StreamedOutput>,
    cancel: CancellationToken,
) -> Result<RunOutput> {
    let CaptureLimits {
        stdout: stdout_limit,
        stderr: stderr_limit,
    } = limits;
    if deadline.is_some_and(|value| value.is_zero() || value > Duration::from_secs(604800))
        || stdout_limit > 8 * 1024 * 1024
    {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Invalid process deadline or stdout budget",
        ));
    }
    if !(1..=64 * 1024).contains(&stderr_limit) {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Invalid process stderr budget",
        ));
    }
    if !program.is_absolute() {
        return Err(DomainError::new(
            "COMPONENT_UNAVAILABLE",
            "An explicit absolute executable is required",
        ));
    }
    let _cwd = program.parent().ok_or_else(|| {
        DomainError::new(
            "COMPONENT_UNAVAILABLE",
            "Executable parent directory is required",
        )
    })?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Media stage cancelled before process launch",
        ));
    }
    let mut child = spawn_owned(program, args, input)?;
    let stdout = child
        .stdout()
        .take()
        .ok_or_else(|| DomainError::new("PROCESS_CAPTURE_FAILED", "stdout was not captured"))?;
    let stderr = child
        .stderr()
        .take()
        .ok_or_else(|| DomainError::new("PROCESS_CAPTURE_FAILED", "stderr was not captured"))?;
    let output_failed = CancellationToken::new();
    let failed = output_failed.clone();
    let streamed = consumer.is_some();
    let sink_cancel = cancel.child_token();
    let reader_cancel = sink_cancel.clone();
    let mut out_task = tokio::spawn(async move {
        let result = if let Some(consumer) = consumer {
            consume(stdout, consumer, reader_cancel).await
        } else {
            capture(stdout, stdout_limit, false)
                .await
                .map_err(crate::files::io_error)
        };
        if result.is_err() {
            failed.cancel();
        }
        result
    });
    let mut err_task = tokio::spawn(capture(stderr, stderr_limit, true));
    let result = tokio::select! {
        status=child.wait()=>status.map_err(|e|DomainError::new("PROCESS_WAIT_FAILED",e.to_string())),
        _=cancel.cancelled()=>{match terminate(&mut child).await {Ok(())=>Err(DomainError::new("CANCELLED","Owned process was cancelled and reaped")),Err(error)=>Err(error)}},
        _=output_failed.cancelled()=>{match terminate(&mut child).await {Ok(())=>Err(DomainError::new("PROCESS_OUTPUT_REJECTED","The streamed consumer rejected stdout")),Err(error)=>Err(error)}},
        _=async { match deadline { Some(value) => tokio::time::sleep(value).await, None => std::future::pending::<()>().await } }=>{match terminate(&mut child).await {Ok(())=>Err(DomainError::new("PROCESS_TIMEOUT","Owned process exceeded its reviewed deadline and was reaped")),Err(error)=>Err(error)}},
    };
    if result.is_err() {
        sink_cancel.cancel();
    }
    let out = tokio::time::timeout(Duration::from_secs(5), &mut out_task).await;
    if out.is_err() {
        out_task.abort();
    }
    let err = tokio::time::timeout(Duration::from_secs(5), &mut err_task).await;
    if err.is_err() {
        err_task.abort();
    }
    let status = match result {
        Err(error) if error.code == "PROCESS_OUTPUT_REJECTED" => {
            return Err(out
                .ok()
                .and_then(|joined| joined.ok())
                .and_then(|captured| captured.err())
                .unwrap_or(error));
        }
        other => other?,
    };
    let (stdout, total) = out
        .map_err(|_| DomainError::new("PROCESS_CAPTURE_FAILED", "stdout drain deadline exceeded"))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))??;
    let (stderr, diagnostic_bytes) = err
        .map_err(|_| DomainError::new("PROCESS_CAPTURE_FAILED", "stderr drain deadline exceeded"))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))?;
    if !streamed && total > stdout_limit as u64 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Process stdout exceeded its admitted byte budget",
        ));
    }
    let output = RunOutput {
        stdout,
        stderr_tail: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: status.code(),
        diagnostic_bytes,
    };
    if !status.success() {
        return Err(DomainError::new(
            "PROCESS_FAILED",
            format!(
                "Child exit {:?}; bounded diagnostics: {}",
                output.exit_code, output.stderr_tail
            ),
        ));
    }
    tracing::debug!(
        diagnostic_bytes,
        "Process exited successfully; output still requires semantic verification"
    );
    Ok(output)
}

async fn terminate(child: &mut Box<dyn ChildWrapper>) -> Result<()> {
    tracing::warn!("Terminating the owned process group/job object");
    child
        .start_kill()
        .map_err(|e| DomainError::new("TREE_TERMINATION_UNCERTAIN", e.to_string()))?;
    tokio::time::timeout(
        Duration::from_secs(nw_domain::execution::TERMINATION_GRACE_SECONDS),
        child.wait(),
    )
    .await
    .map_err(|_| DomainError::new("TREE_TERMINATION_UNCERTAIN", "Reap deadline exceeded"))?
    .map_err(|e| DomainError::new("TREE_TERMINATION_UNCERTAIN", e.to_string()))?;
    Ok(())
}

fn spawn_owned(program: &Path, args: &[String], input: Stdio) -> Result<Box<dyn ChildWrapper>> {
    let cwd = program.parent().ok_or_else(|| {
        DomainError::new(
            "COMPONENT_UNAVAILABLE",
            "Executable parent directory is required",
        )
    })?;
    tracing::info!(program=%program.display(),cwd=%cwd.display(),argv=?args,"stage.prepared");
    let mut command = Command::new(program);
    command
        .current_dir(cwd)
        .args(args)
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "LANG"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut owned = CommandWrap::from(command);
    #[cfg(unix)]
    owned.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    owned.wrap(JobObject);
    owned.wrap(KillOnDrop);
    let result = with_process_launch(|| owned.spawn());
    result.map_err(|e| DomainError::new("PROCESS_LAUNCH_FAILED", e.to_string()))
}
