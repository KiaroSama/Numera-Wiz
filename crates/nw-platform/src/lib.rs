//! Native boundary: identity, no-replace publication and bounded process I/O.
pub mod claims;
pub mod files;
pub mod logging;
pub mod process;
pub mod security;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn init_logging() {
    use tracing_subscriber::prelude::*;
    let human = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .event_format(logging::PrimaryFormatter);
    let protected = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(logging::JsonWriter)
        .with_current_span(false)
        .with_span_list(false)
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target().starts_with("nw_")
                || metadata.target().starts_with("numera_wiz")
                || metadata.target() == "xtask"
        }));
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(human)
        .with(protected)
        .init();
    tracing::debug!("DEBUG: Diagnostic logging initialized; credentials are excluded");
}

pub mod artifacts;

pub mod file_times;
