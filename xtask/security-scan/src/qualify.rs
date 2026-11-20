//! Executable qualification assertions; no application test inventory entrypoints.
use nw_security_scan::{diagnostic, install_private_panic_hook};
use std::process::ExitCode;
#[cfg(target_os = "linux")]
mod fixtures;
fn main() -> ExitCode {
    install_private_panic_hook();
    #[cfg(target_os = "linux")]
    {
        if let Some(status) = nw_security_scan::scanner::internal_dispatch() {
            return status;
        }
        if let Some(status) = fixtures::child_dispatch() {
            return status;
        }
    }
    #[cfg(target_os = "linux")]
    let result = fixtures::qualify();
    #[cfg(not(target_os = "linux"))]
    let result: nw_security_scan::Result<()> =
        Err(nw_security_scan::ScanFailure("SECURITY_HOST_UNSUPPORTED"));
    match result {
        Ok(()) => {
            diagnostic("INFO", "SECURITY_QUALIFICATION_PASS");
            ExitCode::SUCCESS
        }
        Err(error) => {
            diagnostic("ERROR", error.0);
            ExitCode::FAILURE
        }
    }
}
