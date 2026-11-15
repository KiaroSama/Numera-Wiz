//! Finite byte oracle. Neither this fixture nor its hashes qualify media.
use sha2::{Digest, Sha256};
use std::{io::{self, Write}, time::{Duration, Instant}};
#[cfg(unix)]
use std::io::Read;

const LENGTHS: [usize; 2] = [65_553, 98_335];
const TAILS: [&[u8]; 2] = [b"INPUT0-TAIL-\x00\xffEND!", b"INPUT1-TAIL-\x00\xffEND!"];
struct Oracle { id: usize, count: usize, hash: Sha256, tail: std::collections::VecDeque<u8>, eof: bool }
impl Oracle {
    fn new(id: usize) -> Self { Self { id, count: 0, hash: Sha256::new(), tail: std::collections::VecDeque::with_capacity(18), eof: false } }
    fn accept(&mut self, bytes: &[u8]) -> io::Result<()> {
        for byte in bytes {
            let at = self.count;
            if at >= LENGTHS[self.id] { return Err(io::Error::other("input append")); }
            let tail_at = LENGTHS[self.id] - 18;
            let expected = if at >= tail_at { TAILS[self.id][at - tail_at] } else { ((at * 17 + self.id * 73) % 256) as u8 };
            if *byte != expected { return Err(io::Error::other("independent input oracle mismatch")); }
            if self.tail.len() == 18 { self.tail.pop_front(); }
            self.tail.push_back(*byte);
            self.count = self.count.checked_add(1).ok_or_else(|| io::Error::other("input counter overflow"))?;
        }
        self.hash.update(bytes);
        Ok(())
    }
    fn finish(self) -> io::Result<()> {
        if !self.eof || self.count != LENGTHS[self.id] || TAILS[self.id].len() != 18 || self.tail.into_iter().collect::<Vec<_>>() != TAILS[self.id] { return Err(io::Error::other("input length, tail or EOF mismatch")); }
        println!("RELAY1 {} {} {} EOF TAIL18", self.id, self.count, hex::encode(self.hash.finalize()));
        Ok(())
    }
}

pub(super) fn run(args: &[String]) -> std::process::ExitCode {
    let (done, wait) = std::sync::mpsc::sync_channel::<()>(1);
    let watchdog = std::thread::Builder::new().name("relay-fixture-watchdog".into()).spawn(move || {
        if wait.recv_timeout(Duration::from_secs(3)).is_err() { std::process::exit(86); }
    });
    let Ok(watchdog) = watchdog else { return std::process::ExitCode::from(87); };
    let result = if args.len() == 4 { exercise(&args[1], [&args[2], &args[3]]) } else { Err(io::Error::other("exactly two endpoints required")) };
    let _ = done.send(());
    if watchdog.join().is_err() { return std::process::ExitCode::from(87); }
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(problem) => { eprintln!("relay fixture failed: {problem}"); std::process::ExitCode::from(88) },
    }
}
fn exercise(mode: &str, endpoints: [&str; 2]) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_millis(2800);
    if mode == "relay-early-exit" { return Err(io::Error::other("finite early client exit")); }
    if mode == "relay-open-stall" { std::thread::sleep(Duration::from_millis(250)); }
    io::stderr().write_all(&[b'd'; 8192])?;
    println!("RELAY1 READY");
    io::stdout().flush()?;
    if mode == "relay-read-stall" {
        let _first = Client::open(endpoints[0], deadline)?;
        let _second = Client::open(endpoints[1], deadline)?;
        while Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
        return Err(io::Error::other("finite reader stall"));
    }
    if mode == "relay-two-input-interleaved" {
        let mut clients = [Client::open(endpoints[0], deadline)?, Client::open(endpoints[1], deadline)?];
        let mut oracles = [Oracle::new(0), Oracle::new(1)];
        while !oracles.iter().all(|oracle| oracle.eof) {
            for (client, oracle) in clients.iter_mut().zip(&mut oracles) { if !oracle.eof { read_once(client, oracle, false, deadline)?; } }
        }
        for oracle in oracles { oracle.finish()?; }
    } else {
        for (id, endpoint) in endpoints.iter().enumerate() {
            let mut client = Client::open(endpoint, deadline)?;
            let mut oracle = Oracle::new(id);
            while !oracle.eof { read_once(&mut client, &mut oracle, mode == "relay-slow-tail", deadline)?; }
            oracle.finish()?;
        }
    }
    io::stdout().flush()
}
fn read_once(client: &mut Client, oracle: &mut Oracle, slow: bool, deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "finite client deadline")); }
    let mut bytes = [0; 4096];
    match client.read(&mut bytes, deadline)? {
        Some(0) => oracle.eof = true,
        Some(count) => { oracle.accept(&bytes[..count])?; if slow { std::thread::sleep(Duration::from_millis(1)); } },
        None => std::thread::sleep(Duration::from_millis(1)),
    }
    Ok(())
}

#[cfg(unix)]
struct Client { file: std::fs::File, started: bool }
#[cfg(unix)]
impl Client {
    fn open(path: &str, _deadline: Instant) -> io::Result<Self> {
        use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
        let file = std::fs::OpenOptions::new().read(true).custom_flags((rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits() as i32).open(path)?;
        if !file.metadata()?.file_type().is_fifo() { return Err(io::Error::other("client endpoint is not FIFO")); }
        Ok(Self { file, started: false })
    }
    fn read(&mut self, bytes: &mut [u8], _deadline: Instant) -> io::Result<Option<usize>> {
        match self.file.read(bytes) {
            Ok(0) if !self.started => Ok(None),
            Ok(count) => { self.started = true; Ok(Some(count)) },
            Err(problem) if problem.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(problem) => Err(problem),
        }
    }
}

#[cfg(windows)]
use native_client::Client;
#[cfg(windows)]
mod native_client {
    use super::*;
    use std::{os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle}, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY, GENERIC_READ, GetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_IO_COMPLETION, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT},
        System::{IO::{GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED}, Threading::{CreateEventW, ResetEvent, WaitForSingleObject}},
    };
    struct Storage { pipe: OwnedHandle, event: OwnedHandle, operation: OVERLAPPED, buffer: [u8; 4096], active: bool, generation: u64 }
    pub(super) struct Client { storage: Box<Storage> }
    impl Drop for Client {
        fn drop(&mut self) {
            if self.storage.active {
                // Fixture failure only: OS process teardown retains pending
                // pointers until the kernel retires them. Never unwind/free an
                // uncertain read, including in the direct test-only fixture.
                std::process::exit(89);
            }
        }
    }
    fn code() -> u32 { io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32 }
    fn failed(value: u32) -> io::Error { io::Error::from_raw_os_error(value as i32) }
    #[expect(unsafe_code, reason = "Read actual inheritance flags from a live uniquely owned fixture handle")]
    fn noninherited(handle: &OwnedHandle) -> io::Result<()> {
        let mut flags = 0;
        // SAFETY: live owned handle and initialized output.
        if unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) } == 0 { return Err(failed(code())); }
        if flags & HANDLE_FLAG_INHERIT != 0 { return Err(io::Error::other("inheritable client handle")); }
        Ok(())
    }
    impl Client {
        #[expect(unsafe_code, reason = "Open the exact local fixture pipe with frozen least-access flags and one private manual-reset event")]
        pub(super) fn open(path: &str, deadline: Instant) -> io::Result<Self> {
            if !path.starts_with(r"\\.\pipe\numera-wiz-") || path.encode_utf16().count() >= 256 || path.contains('\0') { return Err(io::Error::other("invalid private fixture endpoint")); }
            let wide = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let pipe = loop {
                if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "client open deadline")); }
                // SAFETY: terminated exact local name; NULL attributes prevent
                // inheritance; SQOS limits impersonation without adding access.
                let raw = unsafe { CreateFileW(wide.as_ptr(), GENERIC_READ, 0, ptr::null(), OPEN_EXISTING, FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, ptr::null_mut()) };
                if raw != INVALID_HANDLE_VALUE {
                    // SAFETY: successful API transfers the unique handle.
                    break unsafe { OwnedHandle::from_raw_handle(raw) };
                }
                let problem = code();
                if problem != ERROR_PIPE_BUSY { return Err(failed(problem)); }
                std::thread::sleep(Duration::from_millis(10));
            };
            noninherited(&pipe)?;
            // SAFETY: unnamed, noninherited, manual-reset event with no user
            // signaler and no native operation yet referring to it.
            let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
            if event.is_null() { return Err(failed(code())); }
            // SAFETY: successful API transfers the unique event handle.
            let event = unsafe { OwnedHandle::from_raw_handle(event) };
            noninherited(&event)?;
            Ok(Self { storage: Box::new(Storage { pipe, event, operation: OVERLAPPED::default(), buffer: [0; 4096], active: false, generation: 0 }) })
        }
        #[expect(unsafe_code, reason = "One stable fixture read; pending storage survives until exact terminal evidence or whole-process failure")]
        pub(super) fn read(&mut self, bytes: &mut [u8], deadline: Instant) -> io::Result<Option<usize>> {
            let storage = &mut self.storage;
            if storage.active || bytes.is_empty() || bytes.len() > storage.buffer.len() { return Err(io::Error::other("invalid fixture read ownership")); }
            storage.generation = storage.generation.checked_add(1).ok_or_else(|| io::Error::other("client generation exhausted"))?;
            let generation = storage.generation;
            let pipe = storage.pipe.as_raw_handle();
            let event = storage.event.as_raw_handle();
            // SAFETY: no outstanding operation exists before reset.
            if unsafe { ResetEvent(event) } == 0 { return Err(failed(code())); }
            storage.operation = OVERLAPPED { hEvent: event, ..OVERLAPPED::default() };
            storage.active = true;
            let requested = bytes.len() as u32;
            // SAFETY: stable boxed nonempty buffer and unique OVERLAPPED remain
            // valid through completion, including synchronous submission stalls.
            let okay = unsafe { ReadFile(pipe, storage.buffer.as_mut_ptr(), requested, ptr::null_mut(), &mut storage.operation) };
            let mut immediate = okay != 0;
            let pending = if !immediate {
                let problem = code();
                if problem != ERROR_IO_PENDING {
                    storage.active = false;
                    return if problem == ERROR_BROKEN_PIPE { Ok(Some(0)) } else { Err(failed(problem)) };
                }
                true
            } else { false };
            loop {
                if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "pending fixture read expired")); }
                if generation != storage.generation || storage.operation.hEvent != event { return Err(io::Error::other("client operation identity changed")); }
                let was_immediate = immediate;
                let mut count = 0;
                // SAFETY: query the exact operation with no APC dispatch.
                let mut okay = unsafe { if immediate { GetOverlappedResult(pipe, &storage.operation, &mut count, 0) } else { GetOverlappedResultEx(pipe, &storage.operation, &mut count, 10, 0) } };
                immediate = false;
                let mut problem = if okay == 0 { code() } else { 0 };
                if okay == 0 && problem == ERROR_BROKEN_PIPE && pending {
                    // SAFETY: this private event belongs to this pending read,
                    // with no manual signal/reset/close since submission.
                    let signal = unsafe { WaitForSingleObject(event, 0) };
                    let wait_error = if signal == WAIT_FAILED { Some(code()) } else { None };
                    if signal != WAIT_OBJECT_0 { return Err(io::Error::other(format!("peer closure lacks exact event witness (wait={signal}, error={wait_error:?})"))); }
                    count = 0;
                    // SAFETY: fresh zero-time query AFTER the event witness.
                    okay = unsafe { GetOverlappedResultEx(pipe, &storage.operation, &mut count, 0, 0) };
                    problem = if okay == 0 { code() } else { 0 };
                    if okay == 0 && problem == ERROR_BROKEN_PIPE { storage.active = false; return Ok(Some(0)); }
                    if okay == 0 && problem != ERROR_OPERATION_ABORTED { return Err(io::Error::other("fresh client query contradicted completion")); }
                }
                if okay != 0 {
                    if count > requested { return Err(io::Error::other("impossible native client count")); }
                    storage.active = false;
                    if count == 0 { return Err(io::Error::other("zero-byte read is not EOF")); }
                    let count = count as usize;
                    bytes[..count].copy_from_slice(&storage.buffer[..count]);
                    return Ok(Some(count));
                }
                match problem {
                    ERROR_OPERATION_ABORTED => { storage.active = false; return Err(failed(problem)); },
                    ERROR_IO_INCOMPLETE | WAIT_TIMEOUT | WAIT_IO_COMPLETION if !was_immediate => {},
                    _ => return Err(failed(problem)),
                }
            }
        }
    }
}
