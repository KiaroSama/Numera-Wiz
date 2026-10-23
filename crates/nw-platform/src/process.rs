//! Bounded leaf execution; arguments are never executed as shell text.
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

#[derive(Debug)]
pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr_tail: String,
    pub exit_code: Option<i32>,
    pub diagnostic_bytes: u64,
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
    if deadline.is_zero() || deadline > Duration::from_secs(10800) || stdout_limit > 8 * 1024 * 1024
    {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Invalid process deadline or stdout budget",
        ));
    }
    if !program.is_absolute() {
        return Err(DomainError::new(
            "COMPONENT_UNAVAILABLE",
            "An explicit absolute executable is required",
        ));
    }
    let cwd=program.parent().ok_or_else(||DomainError::new("COMPONENT_UNAVAILABLE","Executable parent directory is required"))?;
    tracing::info!(program=%program.display(),cwd=%cwd.display(),argv=?args,"stage.prepared");
    let mut command = Command::new(program);
    command
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::null())
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
    let mut child = owned
        .spawn()
        .map_err(|e| DomainError::new("PROCESS_LAUNCH_FAILED", e.to_string()))?;
    let stdout = child
        .stdout()
        .take()
        .ok_or_else(|| DomainError::new("PROCESS_CAPTURE_FAILED", "stdout was not captured"))?;
    let stderr = child
        .stderr()
        .take()
        .ok_or_else(|| DomainError::new("PROCESS_CAPTURE_FAILED", "stderr was not captured"))?;
    let mut out_task = tokio::spawn(capture(stdout, stdout_limit, false));
    let mut err_task = tokio::spawn(capture(stderr, 64 * 1024, true));
    let result = tokio::select! {
        status=child.wait()=>status.map_err(|e|DomainError::new("PROCESS_WAIT_FAILED",e.to_string())),
        _=cancel.cancelled()=>{match terminate(&mut child).await {Ok(())=>Err(DomainError::new("CANCELLED","Owned process was cancelled and reaped")),Err(error)=>Err(error)}},
        _=tokio::time::sleep(deadline)=>{match terminate(&mut child).await {Ok(())=>Err(DomainError::new("PROCESS_TIMEOUT","Owned process exceeded its finite deadline and was reaped")),Err(error)=>Err(error)}},
    };
    let out = tokio::time::timeout(Duration::from_secs(5), &mut out_task).await;
    if out.is_err() {
        out_task.abort();
    }
    let err = tokio::time::timeout(Duration::from_secs(5), &mut err_task).await;
    if err.is_err() {
        err_task.abort();
    }
    let status = result?;
    let (stdout, total) = out
        .map_err(|_| DomainError::new("PROCESS_CAPTURE_FAILED", "stdout drain deadline exceeded"))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))?;
    let (stderr, diagnostic_bytes) = err
        .map_err(|_| DomainError::new("PROCESS_CAPTURE_FAILED", "stderr drain deadline exceeded"))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))?
        .map_err(|e| DomainError::new("PROCESS_CAPTURE_FAILED", e.to_string()))?;
    if total > stdout_limit as u64 {
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
    tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| DomainError::new("TREE_TERMINATION_UNCERTAIN", "Reap deadline exceeded"))?
        .map_err(|e| DomainError::new("TREE_TERMINATION_UNCERTAIN", e.to_string()))?;
    Ok(())
}
