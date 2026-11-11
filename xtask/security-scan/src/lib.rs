//! Isolated security candidate. The scanner contract is intentionally incomplete.
//!
//! Stage A1 must fail the real-clean qualification before production scanner,
//! Git proxy, archive/report parsing or descendant supervision is implemented.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct ScanRequest<'a> {
    /// Explicit synthetic checkout in A1; never inferred from the build tree.
    pub repository: &'a Path,
    /// A private fixture summary, or the optional workflow summary in the CLI.
    pub summary: Option<&'a Path>,
}

pub struct VerifiedScan {
    pub scanner_version: String,
    pub finding_count: usize,
    pub successful_history_invocations: usize,
}

pub enum ScanFailure {
    MissingHandler,
}

/// The initial contract covers all fetched refs of an explicit Git checkout.
/// A2 must verify the pinned scanner, complete report and fresh Git proofs before
/// constructing a result. No synthetic clean result is available in A1.
pub fn scan(_request: &ScanRequest<'_>) -> Result<VerifiedScan, ScanFailure> {
    Err(ScanFailure::MissingHandler)
}

/// Callers supply fixed categories only, never paths, child output or errors.
pub fn diagnostic(level: &'static str, category: &'static str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    eprintln!("{timestamp} [{level}] {category}");
}

pub fn install_private_panic_hook() {
    std::panic::set_hook(Box::new(|_| {
        diagnostic("ERROR", "SECURITY_CANDIDATE_PANIC");
    }));
}
