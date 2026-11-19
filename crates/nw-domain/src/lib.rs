//! Shared, display-independent command and media contracts.
pub mod audio_analysis;
pub mod audio_edit;
pub mod audio_normalization;
pub mod capabilities;
pub mod chapters;
pub mod commands;
pub mod config_import;
pub mod dispositions;
pub mod execution;
pub mod media;
pub mod media_execution;
pub mod media_join;
pub mod media_v2_execution;
pub mod metadata;
pub mod mux;
pub mod packet_cut;
pub mod preview;
pub mod reference_metrics;
pub mod stream_observations;
pub mod subtitle_burn;
pub mod time;
pub mod track_append;

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
    pub fn object_not_found() -> Self {
        Self::new(
            "OBJECT_NOT_FOUND",
            "Object is absent or unavailable to this principal",
        )
    }
    /// Stable process status for a failed synchronous CLI request.
    pub fn exit_code(&self) -> u8 {
        match self.code.as_str() {
            "RESOURCE_LIMIT" | "SCRATCH_EXHAUSTED" | "OUTPUT_STORAGE_FULL" => 10,
            "PARTIAL_BATCH_FAILURE" => 9,
            "TRANSPORT_ERROR" | "OWNER_START_TIMEOUT" | "OWNER_STOPPING" => 8,
            "CANCELLED" => 7,
            "AUTHENTICATION_REQUIRED"
            | "PERMISSION_DENIED"
            | "PATH_DENIED"
            | "INPUT_DENIED"
            | "PUBLICATION_DENIED"
            | "INSECURE_DATA_ROOT"
            | "REPLAY_SCOPE_INVALID"
            | "REPLAY_SCOPE_EXPIRED"
            | "RECEIPT_RESULT_RESTRICTED"
            | "CLOCK_ROLLBACK" => 5,
            "IDEMPOTENCY_CONFLICT"
            | "OBJECT_CONFLICT"
            | "OUTPUT_COLLISION"
            | "SOURCE_CHANGED"
            | "ARTIFACT_CHANGED"
            | "TARGET_CHANGED"
            | "COMPONENT_CHANGED"
            | "STALE_REVISION"
            | "STALE_JOB_STATE"
            | "PLAN_EXPIRED"
            | "INVALID_PLAN"
            | "FOLDER_SNAPSHOT_MISMATCH"
            | "INVALID_JOB_TRANSITION"
            | "CANCELLATION_TOO_LATE"
            | "OWNER_BUSY"
            | "STORE_COPY_CONFLICT"
            | "PUBLICATION_BUSY"
            | "PRINCIPAL_CHANGED"
            | "OWNER_IDENTITY_CONFLICT"
            | "OWNER_VERSION_CONFLICT" => 4,
            "LOG_FILE_UNOWNED" => 4,
            "CAPABILITY_UNAVAILABLE"
            | "UNSUPPORTED_MEDIA_SETTINGS"
            | "UNSUPPORTED_AUDIO_LAYOUT"
            | "UNSUPPORTED_AUDIO_TRACK"
            | "UNSUPPORTED_REGISTERED_INPUT"
            | "UNSUPPORTED_COLOR_PRECISION"
            | "UNSUPPORTED_LAYERING"
            | "UNSUPPORTED_OVERLAP"
            | "COMPONENT_UNAVAILABLE"
            | "CONTAINER_UNQUALIFIED"
            | "INPUT_ROUTE_REQUIRED"
            | "NATIVE_PATH_UNAVAILABLE"
            | "DATA_ROOT_UNAVAILABLE"
            | "NATIVE_IDENTITY_UNAVAILABLE"
            | "NATIVE_FILE_TIMES_UNAVAILABLE"
            | "PUBLICATION_CLAIM_UNAVAILABLE"
            | "PUBLICATION_UNAVAILABLE"
            | "PROCESS_ISOLATION_UNAVAILABLE"
            | "PROFILE_NOT_FOUND"
            | "OBJECT_NOT_FOUND"
            | "ARTIFACT_UNAVAILABLE"
            | "FRAME_UNAVAILABLE"
            | "INGEST_NOT_FOUND"
            | "INGEST_NOT_READY"
            | "STORE_SCHEMA_UNSUPPORTED"
            | "STORE_VERSION_UNSUPPORTED"
            | "STORE_RECOVERY_REQUIRED"
            | "MIGRATION_NOT_REQUIRED" => 3,
            "VALIDATION_ERROR"
            | "INVALID_RANGE"
            | "UNSUPPORTED_VERSION"
            | "UNKNOWN_COMMAND"
            | "INVALID_TIME"
            | "INVALID_IDENTITY"
            | "INVALID_ACCEPTANCE"
            | "INVALID_RECORD"
            | "INVALID_MIGRATION_PLAN"
            | "BATCH_REJECTED"
            | "FOLDER_INTAKE_INCOMPLETE"
            | "FOLDER_INTAKE_FAILED"
            | "FOLDER_EMPTY"
            | "UNRESOLVED_SOURCE_INDEX"
            | "SOURCE_OUTPUT_ALIAS"
            | "VIDEO_REQUIRED"
            | "INCOMPATIBLE_PRESERVATION"
            | "CONTAINER_CHANGE_REQUIRED" => 2,
            _ => 6,
        }
    }
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
pub mod artifacts;
pub mod batches;
pub mod diagnostics;
pub mod plans;
pub mod records;
pub mod views;

pub mod ingest;

pub mod file_times;

pub mod reports;

pub mod download;

pub mod maintenance;

pub mod projects;

pub mod video_retime;
