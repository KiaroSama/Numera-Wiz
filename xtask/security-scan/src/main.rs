//! The active workflow must pin an already reviewed, hosted-qualified tool source.
use nw_security_scan::{diagnostic, install_private_panic_hook};
use std::process::ExitCode;
#[cfg(target_os = "linux")]
use nw_security_scan::classifier::Disposition;
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
        Ok(Disposition::Clean) => {
            diagnostic("INFO", "SECURITY_SCAN_CLEAN");
            ExitCode::SUCCESS
        }
        Ok(Disposition::ReviewedPublicData) => {
            diagnostic("INFO", "SECURITY_SCAN_REVIEWED_PUBLIC_DATA");
            ExitCode::SUCCESS
        }
        Ok(Disposition::UnreviewedFindings) => {
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
        if let Err(error) = result { diagnostic("ERROR",error.0); }
        ExitCode::FAILURE
    }
}
#[cfg(target_os = "linux")]
fn run() -> nw_security_scan::Result<Disposition> {
    use nw_security_scan::{
        PrivateResult, ScanFailure,
        assets::Release,
        private::Scratch,
        process,
        protocol::{self, Scope},
        require,
        scanner::{self, ScanRequest},
    };
    use std::{env, path::PathBuf};
    scanner::guard()?;
    process::initialize()?;
    process::barrier()?;
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    require(
        args.len() == 2 && args[0] == "scan",
        "SECURITY_ARGUMENTS_INVALID",
    )?;
    let repository = PathBuf::from(&args[1]);
    let summary = env::var_os("GITHUB_STEP_SUMMARY").map(PathBuf::from);
    // Later workflow supplies its absolute original job deadline, including cold
    // bootstrap time. Never silently grant the scan a fresh twenty minutes.
    let end: u64 = env::var("NW_SECURITY_DEADLINE_UNIX_MS")
        .private("SECURITY_DEADLINE")?
        .parse()
        .private("SECURITY_DEADLINE")?;
    let deadline = protocol::deadline_from(&serde_json::json!({"deadline_ms": end}))?;
    let scope = Scope::from_event()?;
    scanner::provenance()?;
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
            scratch.remove(&barrier)?;
            let verified = result?;
            require(std::time::Instant::now() < deadline && !process::cancelled(),"SECURITY_DEADLINE")?;
            nw_security_scan::report::publish(summary.as_deref(),&verified.summary)?;
            Ok(verified.disposition)
        }
        Err(_) => Err(ScanFailure("SECURITY_CLEANUP_UNVERIFIED")),
    }
}
