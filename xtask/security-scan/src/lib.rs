//! Isolated Linux security candidate; activation requires a separately qualified pin.

use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
pub mod assets;
#[cfg(target_os = "linux")]
pub mod classifier;
#[cfg(target_os = "linux")]
pub mod classifier_git;
#[cfg(target_os = "linux")]
pub mod private;
#[cfg(target_os = "linux")]
pub mod process;
#[cfg(target_os = "linux")]
pub mod protocol;
#[cfg(target_os = "linux")]
pub mod report;
#[cfg(target_os = "linux")]
pub mod scanner;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanFailure(pub &'static str);
pub type Result<T> = std::result::Result<T, ScanFailure>;

pub trait PrivateResult<T> {
    fn private(self, category: &'static str) -> Result<T>;
}

impl<T, E> PrivateResult<T> for std::result::Result<T, E> {
    fn private(self, category: &'static str) -> Result<T> {
        self.map_err(|_| ScanFailure(category))
    }
}

pub fn require(condition: bool, category: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(ScanFailure(category))
    }
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
