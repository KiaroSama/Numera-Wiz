//! Keep the exact primary waitable until the last destructive group operation.
use super::super::error;
use nw_domain::Result;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use std::{
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
};

pub(in crate::process::media_session) struct OwnedChild {
    child: Option<Child>,
    group: Option<Pid>,
    exit: Option<ExitStatus>,
    kill_sent: bool,
    group_absent: bool,
}
impl OwnedChild {
    pub(in crate::process::media_session) fn new() -> Self {
        Self {
            child: None,
            group: None,
            exit: None,
            kill_sent: false,
            group_absent: false,
        }
    }
    pub(in crate::process::media_session) fn spawned(&self) -> bool {
        self.child.is_some()
    }
    pub(in crate::process::media_session) fn launch(
        &mut self,
        program: &Path,
        args: &[String],
    ) -> Result<(tokio::process::ChildStdout, tokio::process::ChildStderr)> {
        let cwd = program
            .parent()
            .ok_or_else(|| error("PROCESS_LAUNCH_FAILED", "Executable parent missing"))?;
        let mut command = Command::new(program);
        command
            .current_dir(cwd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .process_group(0);
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "LANG"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        // Install the raw child before either fallible I/O registration. It is
        // never given to Tokio's child reaper or an opaque process-group wait.
        self.child = Some(
            command
                .spawn()
                .map_err(|e| error("PROCESS_LAUNCH_FAILED", &e.to_string()))?,
        );
        let child = self.child.as_mut().expect("Installed native child");
        self.group = Pid::from_raw(child.id() as i32);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| error("PROCESS_CAPTURE_FAILED", "stdout missing"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| error("PROCESS_CAPTURE_FAILED", "stderr missing"))?;
        let stdout =
            tokio::process::ChildStdout::from_std(stdout).map_err(crate::files::io_error)?;
        let stderr =
            tokio::process::ChildStderr::from_std(stderr).map_err(crate::files::io_error)?;
        Ok((stdout, stderr))
    }
    pub(in crate::process::media_session) fn observe(
        &mut self,
        cancel: bool,
    ) -> Result<Option<ExitStatus>> {
        let Some(group) = self.group else {
            return Ok(None);
        };
        if self.exit.is_none() {
            let observation = rustix::process::waitid(
                WaitId::Pid(group),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )
            .map_err(|e| error("PROCESS_WAIT_FAILED", &e.to_string()))?;
            // WNOWAIT anchors PID/PGID authority even after primary exit. After
            // the actual reap below, no destructive operation uses this number.
            if (cancel || observation.is_some()) && !self.kill_sent {
                match rustix::process::kill_process_group(group, Signal::KILL) {
                    Ok(()) | Err(rustix::io::Errno::SRCH) => self.kill_sent = true,
                    Err(e) => return Err(error("TREE_TERMINATION_UNCERTAIN", &e.to_string())),
                }
            }
            if observation.is_some() && self.kill_sent {
                self.exit = self
                    .child
                    .as_mut()
                    .expect("Waitable primary owner")
                    .try_wait()
                    .map_err(crate::files::io_error)?;
            }
        }
        if self.exit.is_some() && !self.group_absent {
            match rustix::process::test_kill_process_group(group) {
                Err(rustix::io::Errno::SRCH) => self.group_absent = true,
                Ok(()) => {}
                Err(e) => return Err(error("TREE_SETTLEMENT_UNCERTAIN", &e.to_string())),
            }
        }
        Ok(self.exit)
    }
    // Unix output polls issue nonblocking read syscalls; no kernel write into
    // user storage survives a poll. The coordinator still joins both consumers.
    pub(in crate::process::media_session) fn pump_outputs(&mut self, _cancel: bool) -> Result<()> {
        Ok(())
    }
    pub(in crate::process::media_session) fn outputs_settled(&self) -> bool {
        true
    }
    pub(in crate::process::media_session) fn settled(&self) -> bool {
        !self.spawned() || (self.exit.is_some() && self.group_absent)
    }
}
