//! Authorized artifact metadata and bounded byte-transfer contracts.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const ARTIFACT_CHUNK_BYTES: usize = 128 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManifestInput {
    pub artifact_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChunkInput {
    pub artifact_id: String,
    pub expected_sha256: String,
    pub offset: String,
    pub length: u32,
}
impl ChunkInput {
    pub fn range(&self) -> Result<(u64, usize)> {
        let offset = decimal(&self.offset)?;
        if self.length == 0 || self.length as usize > ARTIFACT_CHUNK_BYTES {
            return Err(DomainError::new("INVALID_RANGE", "Chunk length must be 1..131072 bytes"));
        }
        offset.checked_add(self.length as u64).ok_or_else(|| DomainError::new("INVALID_RANGE", "Requested byte range overflows"))?;
        if self.expected_sha256.len() != 64 || !self.expected_sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(DomainError::new("VALIDATION_ERROR", "Expected SHA-256 must contain 64 lowercase hexadecimal digits"));
        }
        Ok((offset, self.length as usize))
    }
}
pub fn decimal(value: &str) -> Result<u64> {
    if value.is_empty() || value.len() > 20 || (value.len() > 1 && value.starts_with('0')) || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DomainError::new("INVALID_RANGE", "Byte offsets must be canonical unsigned decimal strings"));
    }
    value.parse().map_err(|_| DomainError::new("INVALID_RANGE", "Byte offset exceeds the supported integer range"))
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub artifact_id: String,
    pub manifest_uri: String,
    pub job_id: String,
    pub plan_id: String,
    pub mime_type: String,
    pub encoded_byte_count: String,
    pub sha256: String,
    pub availability: String,
    pub expires_at: Option<String>,
    pub retrieval_methods: Vec<String>,
    pub bytes_route: Option<String>,
    pub max_chunk_bytes: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactChunk {
    pub artifact_id: String,
    pub offset: String,
    pub actual_length: u32,
    pub total_size: String,
    pub sha256: String,
    pub data_base64: String,
    pub eof: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactSummary {
    pub artifact_id: String,
    pub manifest_uri: String,
    pub job_id: String,
    pub encoded_byte_count: String,
    pub sha256: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_rejects_noncanonical_overflow_and_unbounded_lengths() {
        for value in ["", "01", "-1", "+1", "1.0", "18446744073709551616"] { assert!(decimal(value).is_err()); }
        let mut input = ChunkInput { artifact_id: "job".into(), expected_sha256: "0".repeat(64), offset: "18446744073709551615".into(), length: 1 };
        assert!(input.range().is_err());
        input.offset = "0".into();
        input.length = ARTIFACT_CHUNK_BYTES as u32 + 1;
        assert!(input.range().is_err());
        input.length = ARTIFACT_CHUNK_BYTES as u32;
        assert_eq!(input.range().unwrap(), (0, ARTIFACT_CHUNK_BYTES));
    }
}
