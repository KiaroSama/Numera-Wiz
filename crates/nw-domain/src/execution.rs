//! Explicit cleanup budgets and bounded stage progress shared by every adapter.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const TERMINATION_GRACE_SECONDS: u64 = 5;
pub const COPY_BUFFER_BYTES: usize = 4 * 1024 * 1024;
pub const PROGRESS_INTERVAL_MS: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeadlinePolicy {
    Finite { seconds: u32 },
    Disabled,
}
impl DeadlinePolicy {
    pub fn duration(self) -> Result<Option<Duration>> {
        match self {
            Self::Finite { seconds } if (1..=604800).contains(&seconds) => {
                Ok(Some(Duration::from_secs(u64::from(seconds))))
            }
            Self::Disabled => Ok(None),
            _ => Err(DomainError::new(
                "VALIDATION_ERROR",
                "Deadline seconds must be an integer from 1 through 604800",
            )),
        }
    }
    pub fn disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CleanupLimits {
    pub probe: DeadlinePolicy,
    pub operation: DeadlinePolicy,
}
impl Default for CleanupLimits {
    fn default() -> Self {
        Self {
            probe: DeadlinePolicy::Finite { seconds: 120 },
            operation: DeadlinePolicy::Finite { seconds: 10800 },
        }
    }
}
impl CleanupLimits {
    pub fn validate(&self) -> Result<()> {
        self.probe.duration()?;
        self.operation.duration()?;
        Ok(())
    }
    pub fn has_disabled(&self) -> bool {
        self.probe.disabled() || self.operation.disabled()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    CopyBytes,
    VerifyStagingBytes,
    VerifySourceBytes,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StageProgress {
    pub stage: ProgressStage,
    pub completed_bytes: String,
    pub total_bytes: String,
    pub elapsed_ms: String,
    pub coalesced_updates: String,
}
impl StageProgress {
    pub fn validate(&self) -> Result<()> {
        let exact = |value: &str| -> Result<u64> {
            let parsed = value
                .parse::<u64>()
                .map_err(|_| DomainError::new("INVALID_PROGRESS", "Invalid progress counter"))?;
            if parsed.to_string() != value {
                return Err(DomainError::new(
                    "INVALID_PROGRESS",
                    "Progress counters must be canonical decimal strings",
                ));
            }
            Ok(parsed)
        };
        if exact(&self.completed_bytes)? > exact(&self.total_bytes)? {
            return Err(DomainError::new(
                "INVALID_PROGRESS",
                "Stage byte count exceeds its reviewed bound",
            ));
        }
        exact(&self.elapsed_ms)?;
        exact(&self.coalesced_updates)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_is_typed_and_nonfinite_or_ambiguous_values_are_rejected() {
        for value in [
            r#"{"mode":"finite","seconds":-1}"#,
            r#"{"mode":"finite","seconds":1.5}"#,
            r#"{"mode":"finite","seconds":"NaN"}"#,
            r#"{"mode":"finite","seconds":1e999}"#,
            r#"{"mode":"disabled","seconds":0}"#,
        ] {
            assert!(
                serde_json::from_str::<DeadlinePolicy>(value).is_err(),
                "{value}"
            );
        }
        assert!(DeadlinePolicy::Finite { seconds: 0 }.duration().is_err());
        assert!(
            DeadlinePolicy::Finite { seconds: 604801 }
                .duration()
                .is_err()
        );
        assert_eq!(DeadlinePolicy::Disabled.duration().unwrap(), None);
        assert_eq!(
            CleanupLimits::default().operation.duration().unwrap(),
            Some(Duration::from_secs(10800))
        );
    }
}
