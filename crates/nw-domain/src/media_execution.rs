//! Separate accepted media records. Legacy plan, job and receipt codecs are unchanged.
use crate::{
    DomainError, Result,
    media::{AudioLayout, MediaPlanV1},
    records::{FileIdentity, Job, NativeObjectIdentity, StagingIntent},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MEDIA_RENDER_STAGE: &str = "render";
pub const MEDIA_VERIFY_STAGE: &str = "verify";
pub const MEDIA_PUBLISH_STAGE: &str = "publish";
pub const MEDIA_OUTPUT_ARTIFACT: &str = "output";
pub const MAX_QUEUE: usize = 128;
pub const MAX_INVOCATIONS: usize = 16;
pub const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_VERIFICATION_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MEDIA_RENDER_TIMEOUT_SECONDS: u32 = 600;
pub const MEDIA_VERIFY_TIMEOUT_SECONDS: u32 = 300;
pub const MEDIA_RESERVATION_POOL_BYTES: u64 = 1024 * 1024 * 1024;
pub const MEDIA_QUEUE_METADATA_BYTES: u64 = MAX_QUEUE as u64 * 512 * 1024;

#[derive(Debug, Clone)]
pub struct MediaExecutionEvent {
    pub job_id: String,
    pub stage_id: String,
    pub artifact_id: String,
    pub fact: MediaExecutionFact,
}
#[derive(Debug, Clone)]
pub enum MediaExecutionFact {
    StagingIntent(StagingIntent),
    Prepared { executable_sha256: String, argv: Vec<String> },
    Running,
    Verifying,
    Fence(crate::access::Publication),
    Published { identity: FileIdentity, verification: String },
    Progress(crate::execution::RemuxProgress),
}
#[derive(Debug, Clone)]
pub struct MediaExecutionOutcome {
    pub artifact: FileIdentity,
    pub verification: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaJobKind {
    MediaJobV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaJobState {
    Queued,
    Preparing,
    Running,
    Verifying,
    Publishing,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
impl MediaJobState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptedMediaPayload {
    pub schema_version: u32,
    pub plan_id: String,
    pub plan_hash: String,
    pub raw_sha256: String,
    /// Exact caller string bytes, including insignificant whitespace and field order.
    pub media_plan_json: String,
}
impl AcceptedMediaPayload {
    pub fn from_json(raw: String) -> Result<Self> {
        let plan = decode_plan(&raw)?;
        Ok(Self {
            schema_version: 1,
            plan_id: plan.id,
            plan_hash: plan.hash,
            raw_sha256: crate::projects::hash(raw.as_bytes()),
            media_plan_json: raw,
        })
    }
    pub fn plan(&self) -> Result<MediaPlanV1> {
        let plan = decode_plan(&self.media_plan_json)?;
        if self.schema_version != 1
            || self.plan_id != plan.id
            || self.plan_hash != plan.hash
            || self.raw_sha256 != crate::projects::hash(self.media_plan_json.as_bytes())
        {
            return Err(invalid("Accepted media payload identity differs"));
        }
        Ok(plan)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaOutputClaim {
    pub artifact_id: String,
    pub job_id: String,
    pub principal: String,
    pub output: String,
    pub parent: NativeObjectIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaResourceReservation {
    pub schema_version: u32,
    pub plan_id: String,
    pub plan_hash: String,
    pub raw_sha256: String,
    pub cpu_threads: u32,
    pub process_slots: u32,
    pub render_timeout_seconds: u32,
    pub verify_timeout_seconds: u32,
    /// Conservative admission estimate, not a measured or OS-enforced RSS limit.
    pub estimated_memory_bytes: String,
    pub reservation_pool_bytes: String,
    pub queue_metadata_reservation_bytes: String,
    pub scratch_bytes: String,
    pub maximum_output_bytes: String,
    /// Unknown observed process pressure remains absent, never reported as zero.
    pub observed_peak_memory_bytes: Option<String>,
}
impl MediaResourceReservation {
    pub fn estimate(payload: &AcceptedMediaPayload) -> Result<Self> {
        let plan = payload.plan()?;
        let capture = &plan.payload.capture;
        let rate = capture.settings.frame_rate;
        if (1000i128 * i128::from(rate.denominator())) % i128::from(rate.numerator()) != 0 {
            return Err(DomainError::new("UNSUPPORTED_MEDIA_SETTINGS", "This exact-PTS Matroska execution route requires a frame interval representable on its millisecond clock"));
        }
        let pixels = u64::from(capture.settings.width)
            .checked_mul(u64::from(capture.settings.height))
            .ok_or_else(|| invalid("Media resource arithmetic overflow"))?;
        let frames = exact(&plan.payload.output.video_frames)?;
        let samples = exact(&plan.payload.output.audio_samples)?;
        let channels = match capture.settings.audio_layout {
            AudioLayout::Mono => 1,
            AudioLayout::Stereo => 2,
        };
        let video_decode = pixels.checked_mul(4).and_then(|n| n.checked_mul(frames));
        let audio_decode = samples.checked_mul(channels * 2);
        if video_decode.is_none_or(|n| n > MAX_VERIFICATION_BYTES)
            || audio_decode.is_none_or(|n| n > MAX_VERIFICATION_BYTES)
        {
            return Err(DomainError::new("RESOURCE_LIMIT", "Media verification exceeds the existing4GiB per-child stream budget"));
        }
        // FFV1 worst-case coding plus PCM, muxing overhead and bounded finalization.
        let maximum = pixels.checked_mul(8)
            .and_then(|n| n.checked_mul(frames))
            .and_then(|n| n.checked_add(samples.checked_mul(channels * 4)?))
            .and_then(|n| n.checked_add(frames.checked_mul(4096)?))
            .and_then(|n| n.checked_add(64 * 1024 * 1024))
            .filter(|n| *n <= MAX_OUTPUT_BYTES)
            .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Media output reservation exceeds64GiB"))?;
        let memory = pixels.checked_mul(64)
            .and_then(|n| n.checked_mul(plan.payload.graph.segments.len() as u64))
            .and_then(|n| n.checked_add(192 * 1024 * 1024))
            .and_then(|n| n.checked_add(MEDIA_QUEUE_METADATA_BYTES))
            .ok_or_else(|| invalid("Media memory estimate overflow"))?;
        let scratch = maximum.checked_mul(2)
            .and_then(|n| n.checked_add(64 * 1024 * 1024))
            .ok_or_else(|| invalid("Media scratch estimate overflow"))?;
        Ok(Self {
            schema_version: 1,
            plan_id: plan.id,
            plan_hash: plan.hash,
            raw_sha256: payload.raw_sha256.clone(),
            cpu_threads: 2,
            process_slots: 2,
            render_timeout_seconds: MEDIA_RENDER_TIMEOUT_SECONDS,
            verify_timeout_seconds: MEDIA_VERIFY_TIMEOUT_SECONDS,
            estimated_memory_bytes: memory.to_string(),
            reservation_pool_bytes: MEDIA_RESERVATION_POOL_BYTES.to_string(),
            queue_metadata_reservation_bytes: MEDIA_QUEUE_METADATA_BYTES.to_string(),
            scratch_bytes: scratch.to_string(),
            maximum_output_bytes: maximum.to_string(),
            observed_peak_memory_bytes: None,
        })
    }
    pub fn validate(&self, payload: &AcceptedMediaPayload) -> Result<()> {
        let mut expected = Self::estimate(payload)?;
        if let Some(observed) = &self.observed_peak_memory_bytes {
            exact(observed)?;
            expected.observed_peak_memory_bytes = Some(observed.clone());
        }
        if *self != expected {
            return Err(invalid("Media reservation differs from the accepted payload"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaInvocation {
    pub executable_sha256: String,
    pub argv: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaStage {
    pub id: String,
    pub state: MediaJobState,
    pub invocations: Vec<MediaInvocation>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaArtifactState {
    Planned,
    Staged,
    Verified,
    Publishing,
    Published,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaArtifact {
    pub id: String,
    pub state: MediaArtifactState,
    pub identity: Option<FileIdentity>,
    pub verification: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaJobV1 {
    pub envelope_version: u32,
    pub payload_kind: MediaJobKind,
    pub id: String,
    pub creator: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub payload_sha256: String,
    pub revision: String,
    pub accepted_at: String,
    pub queue_ticket: String,
    pub state: MediaJobState,
    pub output: MediaOutputClaim,
    pub resources: MediaResourceReservation,
    pub stages: Vec<MediaStage>,
    pub artifact: MediaArtifact,
    pub owned_staging: Option<StagingIntent>,
    pub progress: Option<crate::execution::RemuxProgress>,
    pub error: Option<DomainError>,
}
impl MediaJobV1 {
    /// Finish only stages that actually started; future queued stages retain that fact.
    pub fn finish_active_stages(&mut self) {
        finish_stages(self.state, &mut self.stages);
    }
    pub fn validate(&self) -> Result<()> {
        crate::projects::validate_id(&self.id)?;
        crate::projects::validate_id(&self.plan_id)?;
        hash(&self.plan_hash)?;
        hash(&self.payload_sha256)?;
        exact(&self.revision)?;
        let accepted = exact(&self.accepted_at)?;
        if self.envelope_version != 1 || self.creator.is_empty()
            || self.queue_ticket != format!("{accepted:020}/{}", self.id)
            || self.output.job_id != self.id || self.output.principal != self.creator
            || self.output.artifact_id != MEDIA_OUTPUT_ARTIFACT
            || self.artifact.id != MEDIA_OUTPUT_ARTIFACT
            || self.resources.plan_id != self.plan_id
            || self.resources.plan_hash != self.plan_hash
            || self.resources.raw_sha256 != self.payload_sha256
            || self.stages.iter().map(|s| s.id.as_str()).ne([
                MEDIA_RENDER_STAGE, MEDIA_VERIFY_STAGE, MEDIA_PUBLISH_STAGE,
            ])
            || self.stages.iter().any(|s| s.invocations.len() > MAX_INVOCATIONS)
            || (self.state.terminal() && self.stages.iter().any(|stage|
                !stage.state.terminal() && stage.state != MediaJobState::Queued))
        {
            return Err(invalid("Media job identity or stage contract differs"));
        }
        if self.state == MediaJobState::Succeeded
            && (self.artifact.state != MediaArtifactState::Published
                || self.artifact.identity.is_none() || self.artifact.verification.is_none()
                || self.stages.iter().any(|stage| stage.state != MediaJobState::Succeeded))
        {
            return Err(invalid("Media success requires a published verified artifact"));
        }
        Ok(())
    }
    pub fn validate_payload(&self, payload: &AcceptedMediaPayload) -> Result<MediaPlanV1> {
        self.validate()?;
        let plan = payload.plan()?;
        if self.plan_id != payload.plan_id || self.plan_hash != payload.plan_hash
            || self.payload_sha256 != payload.raw_sha256 || self.creator != plan.creator
        {
            return Err(invalid("Media job and accepted payload differ"));
        }
        self.resources.validate(payload)?;
        Ok(plan)
    }
}

pub(crate) fn finish_stages(state: MediaJobState, stages: &mut [MediaStage]) {
    if state.terminal() && state != MediaJobState::Succeeded {
        for stage in stages {
            if !stage.state.terminal() && stage.state != MediaJobState::Queued {
                stage.state = state;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum StoredJob {
    Legacy(Job),
    MediaV1(MediaJobV1),
}
impl StoredJob {
    pub fn id(&self) -> &str {
        match self { Self::Legacy(job) => &job.id, Self::MediaV1(job) => &job.id }
    }
    pub fn creator(&self) -> &str {
        match self { Self::Legacy(job) => &job.creator, Self::MediaV1(job) => &job.creator }
    }
    pub fn terminal(&self) -> bool {
        match self { Self::Legacy(job) => job.state.terminal(), Self::MediaV1(job) => job.state.terminal() }
    }
    pub fn queued(&self) -> bool {
        match self {
            Self::Legacy(job) => job.state == crate::records::JobState::Queued,
            Self::MediaV1(job) => job.state == MediaJobState::Queued,
        }
    }
    pub fn plan_id(&self) -> &str {
        match self {
            Self::Legacy(job) => &job.plan.id,
            Self::MediaV1(job) => &job.plan_id,
        }
    }
    pub fn output(&self) -> &str {
        match self {
            Self::Legacy(job) => &job.plan.output,
            Self::MediaV1(job) => &job.output.output,
        }
    }
}
pub fn decode_job(raw: Value) -> Result<StoredJob> {
    if raw.get("envelope_version").is_some() || raw.get("payload_kind").is_some() {
        if raw.get("envelope_version").and_then(Value::as_u64) != Some(1)
            || raw.get("payload_kind").and_then(Value::as_str) != Some("media_job_v1")
        {
            return Err(invalid("Unknown or partial job discriminator"));
        }
        let job: MediaJobV1 = serde_json::from_value(raw).map_err(codec)?;
        job.validate()?;
        Ok(StoredJob::MediaV1(job))
    } else {
        crate::plans::decode(raw.get("plan").cloned()
            .ok_or_else(|| invalid("Stored legacy job has no plan"))?)?;
        Ok(StoredJob::Legacy(serde_json::from_value(raw).map_err(codec)?))
    }
}
pub fn decode_plan(raw: &str) -> Result<MediaPlanV1> {
    if raw.is_empty() || raw.len() > crate::media::MAX_PLAN_BYTES {
        return Err(DomainError::new("RESOURCE_LIMIT", "Media plan exceeds128KiB"));
    }
    // Direct struct decoding rejects duplicate keys as well as unknown fields at every level.
    let plan: MediaPlanV1 = serde_json::from_str(raw).map_err(codec)?;
    plan.validate_hash()?;
    Ok(plan)
}
pub fn exact(value: &str) -> Result<u64> {
    let number = value.parse::<u64>().map_err(|_| invalid("Invalid decimal counter"))?;
    if number.to_string() != value { return Err(invalid("Noncanonical decimal counter")); }
    Ok(number)
}
pub fn hash(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(invalid("Expected lowercase SHA256"));
    }
    Ok(())
}
fn codec(error: serde_json::Error) -> DomainError { invalid(error.to_string()) }
fn invalid(message: impl Into<String>) -> DomainError { DomainError::new("INVALID_RECORD", message) }
