//! Opaque, principal-scoped streamed intake; a browser filename is never a native path.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadInput {
    pub display_name: String,
    pub bytes: String,
    pub sha256: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_timeout: Option<crate::execution::DeadlinePolicy>,
    pub path: Option<String>,
    pub upload_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UploadTicket {
    pub upload_id: String,
    pub expected_bytes: String,
    pub expires_at: String,
}
