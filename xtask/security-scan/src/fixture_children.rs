//! Scheduler oracles only. These modes are absent from the scanner binary.
use super::*;
use rustix::process::{self as os, Signal};
use std::{os::unix::process::CommandExt, process::{ExitCode, Stdio},
    sync::{Arc, atomic::AtomicBool}, thread};

pub fn dispatch() -> Option<ExitCode> {
    let executable = env::current_exe().ok()?;
    if executable.file_name()?.to_str()? == "fixture-sentinel" {
        let result = executable.parent().ok_or(ScanFailure("SECURITY_FIXTURE_IO"))
            .and_then(|p| private::write(&p.join("sentinel-hit"), b"executed"));
        return Some(if result.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE });
    }
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_none_or(|a| a != "--fixture-child") { return None; }
    let result = (|| {
        require(args.len() == 3, "SECURITY_FIXTURE_ARGUMENTS")?;
        let mode = args[1].to_str().ok_or(ScanFailure("SECURITY_FIXTURE_ARGUMENTS"))?;
        child(mode, Path::new(&args[2]))
    })();
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { diagnostic("ERROR", error.0); ExitCode::FAILURE }
    })
}
fn command(mode: &str, root: &Path) -> Result<Command> {
    let mut command = Command::new(env::current_exe().private("SECURITY_FIXTURE_IO")?);
    private::controlled(&mut command, root);
    command.current_dir(root).args(["--fixture-child", mode]).arg(root);
    Ok(command)
}
fn forever() -> ! { loop { thread::sleep(Duration::from_millis(10)); } }
fn ignore_term() -> Result<()> {
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::new(AtomicBool::new(false)))
        .private("SECURITY_FIXTURE_SIGNAL")?;
    Ok(())
}
fn readiness(root: &Path, name: &str) -> Result<()> {
    let path = root.join(name);
    if path.try_exists().private("SECURITY_FIXTURE_IO")? {
        require(private::read(&path, 5)? == b"ready", "SECURITY_FIXTURE_READINESS")
    } else { private::write(&path, b"ready") }
}
fn await_ready(path: &Path) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(2);
    while !path.try_exists().private("SECURITY_FIXTURE_IO")? {
        require(Instant::now() < end, "SECURITY_FIXTURE_READINESS")?;
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
fn child(mode: &str, root: &Path) -> Result<()> {
    match mode {
        "sleep" => forever(),
        "ignored-leaf" => { ignore_term()?; readiness(root, "leaf-ready")?; forever() }
        "session-leaf" => {
            os::setsid().private("SECURITY_FIXTURE_SIGNAL")?;
            ignore_term()?; readiness(root, "leaf-ready")?; forever()
        }
        "tree-exit" | "tree-detach" | "nested" => {
            if mode == "nested" { ignore_term()?; }
            let (leaf, detached) = match mode {
                "nested" => ("tree-detach", true), "tree-detach" => ("session-leaf", false),
                _ => ("ignored-leaf", false),
            };
            let mut spawned = command(leaf, root)?;
            if detached { spawned.process_group(0); }
            // Intentional incomplete-tool analogue: outer supervisor must adopt.
            let _owned_until_exit = spawned.spawn().private("SECURITY_FIXTURE_SPAWN")?;
            await_ready(&root.join("leaf-ready"))?;
            readiness(root, "tree-ready")?;
            if mode == "tree-exit" { Ok(()) } else { forever() }
        }
        "race" => {
            for _ in 0..8 {
                let _owned_until_exit = command("exit", root)?.spawn().private("SECURITY_FIXTURE_SPAWN")?;
            }
            let _owned_until_exit = command("sleep", root)?.spawn().private("SECURITY_FIXTURE_SPAWN")?;
            readiness(root, "tree-ready")?; forever()
        }
        "exit" => Ok(()),
        "proxy-dies" => {
            readiness(root, "proxy-started")?;
            let _owned_until_exit = command("ignored-leaf", root)?.spawn().private("SECURITY_FIXTURE_SPAWN")?;
            await_ready(&root.join("leaf-ready"))?;
            os::kill_process(os::getpid(), Signal::Kill).private("SECURITY_FIXTURE_SIGNAL")?;
            forever()
        }
        "pressure" => {
            let output = [b'x'; 16 * 1024];
            loop {
                std::io::stdout().write_all(&output).private("SECURITY_FIXTURE_IO")?;
                std::io::stderr().write_all(&output).private("SECURITY_FIXTURE_IO")?;
            }
        }
        "report-growth" => {
            let mut file = private::create(&root.join("findings.json"))?;
            let bytes = [b' '; 64 * 1024];
            for _ in 0..1025 { file.write_all(&bytes).private("SECURITY_FIXTURE_IO")?; }
            file.sync_all().private("SECURITY_FIXTURE_IO")?;
            readiness(root, "writer-ready")?; forever()
        }
        "cancel" => cancellation(root, Signal::Term),
        "cancel-int" => cancellation(root, Signal::Int),
        "unrelated" => unrelated(root),
        "fault-registration" | "fault-discovery" | "fault-probe" | "fault-cleanup" | "fault-subreaper" => fault(root, mode),
        "blocked" | "ignored" => {
            let (blocked, ignored, _) = process::signal_state()?;
            let observed = if mode == "blocked" { blocked & (1 << (signal_hook::consts::SIGTERM - 1)) != 0 }
                else { ignored & (1 << (signal_hook::consts::SIGCHLD - 1)) != 0 };
            require(observed, "SECURITY_FIXTURE_SIGNAL_NOT_OBSERVED")?;
            require(process::initialize() == Err(ScanFailure("SECURITY_SIGNAL_STATE_REJECTED")), "SECURITY_FIXTURE_SIGNAL_ACCEPTED")?;
            process::barrier()?;
            std::io::stdout().write_all(b"FIXTURE_STATE_OBSERVED_REJECTED\n").private("SECURITY_FIXTURE_IO")
        }
        "debug" => {
            require(scanner::guard() == Err(ScanFailure("SECURITY_DEBUG_REJECTED")), "SECURITY_FIXTURE_DEBUG_ACCEPTED")?;
            process::barrier()?; Ok(())
        }
        _ => Err(ScanFailure("SECURITY_FIXTURE_ARGUMENTS")),
    }
}
fn cancellation(root: &Path, signal: Signal) -> Result<()> {
    process::initialize()?;
    let start = Instant::now();
    let mut delivered = false;
    let outcome = process::run(&mut command("sleep", root)?,
        Options::command(Duration::from_secs(3), start + Duration::from_secs(7)), || {
            if !delivered && start.elapsed() >= Duration::from_millis(100) {
                os::kill_process(os::getpid(), signal).private("SECURITY_FIXTURE_SIGNAL")?;
                delivered = true;
            }
            Ok(())
        })?;
    require(delivered && outcome.failure == Some(ScanFailure("SECURITY_CANCELLED")) && outcome.reaped == 1,
        "SECURITY_FIXTURE_CANCEL_NOT_PROVEN")?;
    outcome.barrier.recheck()?;
    std::io::stdout().write_all(b"FIXTURE_CONTROL_PASS\n").private("SECURITY_FIXTURE_IO")
}
fn unrelated(root: &Path) -> Result<()> {
    // This outer oracle uses std wait exclusively; it is never a wait-any
    // supervisor. Its live sentinel is a sibling of the tested supervisor.
    let mut sentinel = command("sleep", root)?.process_group(0).stdin(Stdio::null())
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().private("SECURITY_FIXTURE_SPAWN")?;
    private::write(&root.join("untrusted-stale-pid"), sentinel.id().to_string().as_bytes())?;
    let result = (|| {
        let mut subject = command("cancel", root)?.process_group(0).stdout(Stdio::null())
            .stderr(Stdio::null()).spawn().private("SECURITY_FIXTURE_SPAWN")?;
        require(subject.wait().private("SECURITY_FIXTURE_WAIT")?.success()
            && sentinel.try_wait().private("SECURITY_FIXTURE_WAIT")?.is_none(), "SECURITY_UNRELATED_SIGNALLED")
    })();
    sentinel.kill().private("SECURITY_FIXTURE_SIGNAL")?;
    sentinel.wait().private("SECURITY_FIXTURE_WAIT")?;
    result?;
    std::io::stdout().write_all(b"FIXTURE_CONTROL_PASS\n").private("SECURITY_FIXTURE_IO")
}
fn fault(root: &Path, mode: &str) -> Result<()> {
    use process::FixtureFault;
    process::initialize()?;
    let scratch = Scratch::create(root)?;
    private::write(&root.join("retained-path"), private::text(scratch.path())?.as_bytes())?;
    let fault = match mode {
        "fault-subreaper" => FixtureFault::Subreaper,
        "fault-registration" => FixtureFault::Registration,
        "fault-discovery" => FixtureFault::Discovery,
        "fault-probe" => FixtureFault::Probe,
        _ => FixtureFault::CleanupBarrier,
    };
    let child_mode = if matches!(fault, FixtureFault::Discovery | FixtureFault::Probe) { "tree-exit" } else { "sleep" };
    let budget = if matches!(fault, FixtureFault::Discovery | FixtureFault::Probe) {
        Duration::from_secs(2)
    } else { Duration::from_millis(100) };
    let result = process::run_fixture_fault(&mut command(child_mode, scratch.path())?,
        Options::command(budget, Instant::now() + Duration::from_secs(7)), fault);
    let expected = match fault {
        FixtureFault::Subreaper => "SECURITY_INJECTED_SUBREAPER_FAILURE",
        FixtureFault::Registration => "SECURITY_INJECTED_REGISTRATION_FAILURE",
        FixtureFault::Discovery => "SECURITY_INJECTED_DISCOVERY_FAILURE",
        FixtureFault::Probe => "SECURITY_INJECTED_PROBE_FAILURE",
        FixtureFault::CleanupBarrier => "SECURITY_CLEANUP_UNVERIFIED",
    };
    let observed = match &result { Err(error) => Some(*error), Ok(outcome) => outcome.failure };
    require(observed == Some(ScanFailure(expected)), "SECURITY_INJECTED_FAULT_ACCEPTED")?;
    // No Drop cleanup: the outer oracle checks retention, then its actual ECHILD
    // barrier permits deleting this subtree. This is not an unkillable-kernel test.
    require(scratch.path().is_dir(), "SECURITY_PREMATURE_SCRATCH_REMOVAL")?;
    std::io::stdout().write_all(b"FIXTURE_INJECTED_FAILURE_RETAINED\n").private("SECURITY_FIXTURE_IO")
}
