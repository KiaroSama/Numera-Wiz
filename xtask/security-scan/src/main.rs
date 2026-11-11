//! Incomplete candidate entry point; not an activated security workflow tool.

use nw_security_scan::{ScanFailure, ScanRequest, diagnostic, install_private_panic_hook, scan};
use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    install_private_panic_hook();
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[0] != "scan" {
        diagnostic("ERROR", "SECURITY_SCAN_ARGUMENTS_INVALID");
        return ExitCode::FAILURE;
    }
    let repository = PathBuf::from(&arguments[1]);
    let summary = env::var_os("GITHUB_STEP_SUMMARY").map(PathBuf::from);
    let request = ScanRequest {
        repository: &repository,
        summary: summary.as_deref(),
    };
    match scan(&request) {
        Ok(result) if result.finding_count == 0 => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(ScanFailure::MissingHandler) => {
            diagnostic("ERROR", "SECURITY_SCAN_MISSING_HANDLER");
            ExitCode::FAILURE
        }
    }
}
