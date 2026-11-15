use super::*;
use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use tokio::net::unix::pipe::Sender;

pub(super) struct Native {
    name: Option<String>,
    identity: Option<rustix::fs::Stat>,
    created: bool,
    sender: Option<Sender>,
}
impl Native {
    pub(super) fn new() -> Self { Self { name: None, identity: None, created: false, sender: None } }
    pub(super) fn prepare(&mut self, parent: &DirectoryStamp, id: RelayInputId, _nonce: &str, receipt: &mut PumpReceipt) -> Result<String> {
        parent.revalidate()?;
        let name = format!("input-{}", id.0);
        self.name = Some(name.clone());
        make_fifo(parent, &name)?;
        self.created = true;
        receipt.created = true;
        let stat = rustix::fs::statat(parent.held_file(), &name, AtFlags::SYMLINK_NOFOLLOW).map_err(crate::files::io_error)?;
        check_fifo(&stat)?;
        self.identity = Some(stat);
        parent.revalidate()?;
        crate::files::path_text(&parent.path().join(name))
    }
    pub(super) fn pump(&mut self, parent: &DirectoryStamp, input: &mut ByteInput, cancel: &CancellationToken, receipt: &mut PumpReceipt) -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(crate::files::io_error)?;
        runtime.block_on(async {
            let name = self.name.as_ref().ok_or_else(|| error("RELAY_STATE_FAILED", "FIFO name absent"))?;
            let stop = Instant::now() + Duration::from_secs(1);
            loop {
                if cancel.is_cancelled() { return Err(error("CANCELLED", "FIFO connection cancelled")); }
                parent.revalidate()?;
                match rustix::fs::openat(parent.held_file(), name, OFlags::WRONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NOCTTY, Mode::empty()) {
                    Ok(fd) => {
                        let stat = rustix::fs::fstat(&fd).map_err(crate::files::io_error)?;
                        check_fifo(&stat)?;
                        if !self.identity.as_ref().is_some_and(|expected| expected.st_dev == stat.st_dev && expected.st_ino == stat.st_ino) { return Err(error("TARGET_CHANGED", "Opened FIFO differs from created identity")); }
                        self.sender = Some(Sender::from_owned_fd(fd).map_err(crate::files::io_error)?);
                        receipt.connect = "Connected";
                        break;
                    },
                    Err(rustix::io::Errno::NXIO) if Instant::now() < stop => tokio::time::sleep(Duration::from_millis(10)).await,
                    Err(rustix::io::Errno::NXIO) => return Err(error("RELAY_CONNECT_TIMEOUT", "FIFO reader did not connect")),
                    Err(problem) => return Err(crate::files::io_error(problem)),
                }
            }
            let sender = self.sender.as_ref().ok_or_else(|| error("RELAY_STATE_FAILED", "FIFO writer absent"))?;
            let mut buffer = [0; 8192];
            let mut hash = Sha256::new();
            while receipt.bytes < input.len {
                if cancel.is_cancelled() { return Err(error("CANCELLED", "Input cancelled before reading")); }
                let available = usize::try_from((input.len - receipt.bytes).min(buffer.len() as u64)).map_err(crate::files::io_error)?;
                let count = input.reader.read(&mut buffer[..available]).map_err(crate::files::io_error)?;
                if count == 0 { return Err(error("SOURCE_CHANGED", "Byte input truncated")); }
                receipt.max_buffer = receipt.max_buffer.max(count);
                hash.update(&buffer[..count]);
                let mut offset = 0;
                let mut progress = Instant::now();
                while offset < count {
                    if cancel.is_cancelled() { return Err(error("CANCELLED", "FIFO write cancelled")); }
                    match sender.try_write(&buffer[offset..count]) {
                        Ok(0) => return Err(error("RELAY_WRITE_FAILED", "FIFO made zero progress")),
                        Ok(written) => { offset += written; checked_progress(receipt, written)?; progress = Instant::now(); },
                        Err(problem) if problem.kind() == std::io::ErrorKind::WouldBlock => {
                            receipt.pending_write = true;
                            let remaining = Duration::from_secs(1).saturating_sub(progress.elapsed());
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => return Err(error("CANCELLED", "FIFO readiness cancelled")),
                                result = tokio::time::timeout(remaining, sender.writable()) => { result.map_err(|_| error("RELAY_PROGRESS_TIMEOUT", "FIFO made no progress"))?.map_err(crate::files::io_error)?; },
                            }
                        },
                        Err(problem) => return Err(crate::files::io_error(problem)),
                    }
                }
            }
            let hash: [u8; 32] = hash.finalize().into();
            input.verify(receipt.bytes, hash)?;
            if cancel.is_cancelled() { return Err(error("CANCELLED", "Input cancelled before transport completion")); }
            receipt.hash = Some(hash);
            receipt.transport = true;
            Ok(())
        })
    }
    pub(super) fn close(&mut self, parent: &DirectoryStamp, _cancel: &CancellationToken, receipt: &mut PumpReceipt) -> Result<()> {
        // No write future survives pump's return/unwind. Tokio owns exactly one
        // descriptor; this is OwnedFdReleased evidence, not a close(2) result.
        drop(self.sender.take());
        receipt.closed = self.created;
        if !self.created { return Ok(()); }
        parent.revalidate()?;
        let name = self.name.as_ref().ok_or_else(|| error("RELAY_SETTLEMENT_UNCERTAIN", "Created FIFO name unavailable"))?;
        let expected = self.identity.as_ref().ok_or_else(|| error("RELAY_SETTLEMENT_UNCERTAIN", "Created FIFO identity unavailable"))?;
        let stat = rustix::fs::statat(parent.held_file(), name, AtFlags::SYMLINK_NOFOLLOW).map_err(crate::files::io_error)?;
        check_fifo(&stat)?;
        if stat.st_dev != expected.st_dev || stat.st_ino != expected.st_ino { return Err(error("TARGET_CHANGED", "FIFO replacement left untouched")); }
        rustix::fs::unlinkat(parent.held_file(), name, AtFlags::empty()).map_err(crate::files::io_error)?;
        receipt.cleaned = true;
        Ok(())
    }
}

fn check_fifo(stat: &rustix::fs::Stat) -> Result<()> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::Fifo || stat.st_mode & 0o777 != 0o600 || stat.st_uid != rustix::process::geteuid().as_raw() { return Err(error("RELAY_ENDPOINT_DENIED", "Endpoint must be a current-user 0600 FIFO")); }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn make_fifo(parent: &DirectoryStamp, name: &str) -> Result<()> {
    rustix::fs::mkfifoat(parent.held_file(), name, Mode::RUSR | Mode::WUSR).map_err(crate::files::io_error)
}
#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "Pinned libc mkfifoat uses one live borrowed directory fd and bounded single-component CString")]
fn make_fifo(parent: &DirectoryStamp, name: &str) -> Result<()> {
    use std::os::fd::AsRawFd;
    let name = std::ffi::CString::new(name).map_err(crate::files::io_error)?;
    // SAFETY: The held directory and terminated component remain live through
    // the maintained target binding; no process cwd or handwritten ABI.
    if unsafe { libc::mkfifoat(parent.held_file().as_raw_fd(), name.as_ptr(), 0o600) } == -1 { return Err(crate::files::io_error(std::io::Error::last_os_error())); }
    Ok(())
}

pub(super) fn remove_namespace(parent: &DirectoryStamp, namespace: &DirectoryStamp) -> Result<()> {
    parent.revalidate()?;
    namespace.revalidate()?;
    let name = namespace.path().file_name().ok_or_else(|| error("TARGET_CHANGED", "Namespace component absent"))?;
    let named = rustix::fs::statat(parent.held_file(), name, AtFlags::SYMLINK_NOFOLLOW).map_err(crate::files::io_error)?;
    let held = rustix::fs::fstat(namespace.held_file()).map_err(crate::files::io_error)?;
    if FileType::from_raw_mode(named.st_mode) != FileType::Directory || named.st_dev != held.st_dev || named.st_ino != held.st_ino { return Err(error("TARGET_CHANGED", "Namespace replacement left untouched")); }
    rustix::fs::unlinkat(parent.held_file(), name, AtFlags::REMOVEDIR).map_err(crate::files::io_error)
}
