//! Shared, display-independent command and media contracts.
pub mod commands;
pub mod mux;
pub mod time;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{code}: {message}")]
#[serde(deny_unknown_fields)]
pub struct DomainError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub retryable: bool,
}
impl DomainError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            field: None,
            retryable: false,
        }
    }
    pub fn field(mut self, field: &str) -> Self {
        self.field = Some(field.into());
        self
    }
}
pub type Result<T> = std::result::Result<T, DomainError>;

pub mod access;
pub mod records;

pub mod ingest;
