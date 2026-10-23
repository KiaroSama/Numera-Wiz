//! Native boundary: identity, no-replace publication and bounded process I/O.
pub mod files;
pub mod logging;
pub mod process;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .event_format(logging::PrimaryFormatter)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tracing::debug!("DEBUG: Diagnostic logging initialized; credentials are excluded");
}
