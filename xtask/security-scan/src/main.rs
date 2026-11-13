//! The active workflow must pin an already reviewed, hosted-qualified tool source.
#[cfg(target_os = "linux")]
use nw_security_scan::classifier::Disposition;
use nw_security_scan::{diagnostic, install_private_panic_hook};
use std::process::ExitCode;
fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    if let Some(status) = nw_security_scan::scanner::internal_dispatch() {
        return status;
    }
    install_private_panic_hook();
    #[cfg(target_os = "linux")]
    let result = run();
    #[cfg(not(target_os = "linux"))]
    let result: nw_security_scan::Result<bool> =
        Err(nw_security_scan::ScanFailure("SECURITY_HOST_UNSUPPORTED"));
    #[cfg(target_os = "linux")]
    match result {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(Disposition::Clean)) => {
            diagnostic("INFO", "SECURITY_SCAN_CLEAN");
            ExitCode::SUCCESS
        }
        Ok(Some(Disposition::ReviewedPublicData)) => {
            diagnostic("INFO", "SECURITY_SCAN_REVIEWED_PUBLIC_DATA");
            ExitCode::SUCCESS
        }
        Ok(Some(Disposition::UnreviewedFindings)) => {
            diagnostic("WARNING", "SECURITY_SCAN_UNREVIEWED_FINDINGS");
            ExitCode::FAILURE
        }
        Err(error) => {
            diagnostic("ERROR", error.0);
            ExitCode::FAILURE
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if let Err(error) = result {
            diagnostic("ERROR", error.0);
        }
        ExitCode::FAILURE
    }
}
#[cfg(target_os = "linux")]
fn run() -> nw_security_scan::Result<Option<Disposition>> {
    use nw_security_scan::{
        ScanFailure,
        assets::Release,
        bootstrap::{self, Stage},
        private::Scratch,
        process,
        protocol::Scope,
        require,
        scanner::{self, ScanRequest},
    };
    use std::{env, path::PathBuf};
    scanner::guard()?;
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "budget-from-marker" {
        bootstrap::export()?;
        return Ok(None);
    }
    require(
        args.len() == 2 && args[0] == "scan",
        "SECURITY_ARGUMENTS_INVALID",
    )?;
    let repository = PathBuf::from(&args[1]);
    let summary = env::var_os("GITHUB_STEP_SUMMARY").map(PathBuf::from);
    let stage = Stage::admit()?;
    let deadline = stage.deadline;
    process::initialize()?;
    process::barrier()?;
    let scope = Scope::from_event(deadline)?;
    scanner::provenance(deadline)?;
    let scratch = Scratch::create(&scanner::temporary()?)?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let release = Release::prepare(scratch.path(), deadline)?;
        scanner::scan(&ScanRequest {
            repository: &repository,
            summary: None,
            scope: &scope,
            release: &release,
            deadline,
        })
    }))
    .unwrap_or(Err(ScanFailure("SECURITY_SCAN_PANIC")));
    match process::barrier() {
        Ok(barrier) => {
            scratch.remove(&barrier, deadline)?;
            let verified = result?;
            deadline.check()?;
            require(!process::cancelled(), "SECURITY_DEADLINE")?;
            stage.consume(&barrier)?;
            nw_security_scan::report::publish(summary.as_deref(), &verified.summary, deadline)?;
            Ok(Some(verified.disposition))
        }
        Err(_) => Err(ScanFailure("SECURITY_CLEANUP_UNVERIFIED")),
    }
}
