//! Exact filesystem timestamp policy, separate from media-byte preservation.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileTimeRequest {
    New,
    #[default]
    PreserveBestEffort,
    RequireReviewed,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileTimesSnapshot {
    pub accessed_ns: String,
    pub modified_ns: String,
}
impl FileTimesSnapshot {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.accessed_ns, &self.modified_ns] {
            let number = value.parse::<i128>().map_err(|_| invalid())?;
            if number.to_string() != *value {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
fn invalid() -> DomainError {
    DomainError::new(
        "INVALID_PLAN",
        "File times require canonical signed decimal nanoseconds",
    )
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileTimePlan {
    New,
    PreserveBestEffort,
    RequireReviewed { times: FileTimesSnapshot },
}
impl FileTimePlan {
    pub fn validate(&self) -> Result<()> {
        if let Self::RequireReviewed { times } = self {
            times.validate()?;
        }
        Ok(())
    }
    pub fn request(&self) -> FileTimeRequest {
        match self {
            Self::New => FileTimeRequest::New,
            Self::PreserveBestEffort => FileTimeRequest::PreserveBestEffort,
            Self::RequireReviewed { .. } => FileTimeRequest::RequireReviewed,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileTimeOutcome {
    pub policy: FileTimeRequest,
    pub desired: Option<FileTimesSnapshot>,
    pub observed: Option<FileTimesSnapshot>,
    pub preserved: Option<bool>,
    pub warnings: Vec<DomainError>,
    pub scope: String,
}
pub const SCOPE: &str = "Access and modification times checked on owned staging after media verification, before publication; later reads can change access time. New artifacts retain their own creation time and private staging permissions; source ACLs and extended attributes are not copied.";
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_reject_noncanonical_and_nonfinite_values() {
        for value in ["NaN", "inf", "1.0", "+1", "01", "-0"] {
            assert!(
                FileTimesSnapshot {
                    accessed_ns: value.into(),
                    modified_ns: "0".into()
                }
                .validate()
                .is_err()
            );
        }
        FileTimesSnapshot {
            accessed_ns: "-1".into(),
            modified_ns: "1700000000123456700".into(),
        }
        .validate()
        .unwrap();
    }
}
