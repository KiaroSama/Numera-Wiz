//! Bounded owner-only diagnostic status; these counters are not media qualification.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogStatus {
    pub configured: bool,
    pub writer_healthy: bool,
    pub directory: Option<String>,
    pub file_byte_limit: String,
    pub retained_file_limit: usize,
    pub record_byte_limit: String,
    pub queue_record_limit: usize,
    pub dropped_records: String,
    pub omitted_records: String,
    pub write_failures: String,
}
