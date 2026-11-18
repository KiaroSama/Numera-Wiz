//! Bounded accepted mixed-media rows and their private original bindings.
use crate::{
    DomainError, Result, media::{BoundSource, MediaPlanV2, MAX_CLIPS},
    media_execution::{MediaJobState, ReviewedMediaPlan, decode_reviewed_media_plan, exact, hash},
    plans::SourcePurpose, projects, records::{FileIdentity, NativeObjectIdentity, StagingIntent},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MixedJobKind {
    MediaJobV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptedMixedPayload {
    pub schema_version: u32,
    pub job_id: String,
    pub root_id: String,
    pub principal: String,
    pub raw_plan: String,
    pub raw_plan_sha256: String,
    pub plan_hash: String,
    pub original_bindings_sha256: String,
}

/// Persisted only in the private binding namespace after original grants are checked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixedOriginalBindings {
    pub schema_version: u32,
    pub job_id: String,
    pub root_id: String,
    pub principal: String,
    pub raw_plan_sha256: String,
    pub plan_hash: String,
    pub sources: Vec<MixedOriginal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixedOriginal {
    pub source: BoundSource,
    pub original: FileIdentity,
    pub source_purpose: SourcePurpose,
    pub native_object: NativeObjectIdentity,
    pub native_parent: NativeObjectIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedReservation {
    pub schema_version: u32,
    pub decoded_output_bytes: String,
    pub retained_buffer_bytes: String,
    pub source_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixedExecutionAuthority {
    pub schema_version: u32,
    pub job_id: String,
    pub root_id: String,
    pub principal: String,
    pub profile: Option<String>,
    pub plan_hash: String,
    pub raw_plan_sha256: String,
    pub original_bindings_sha256: String,
    pub output: String,
    pub output_parent: NativeObjectIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaJobV2 {
    pub envelope_version: u32,
    pub payload_kind: MixedJobKind,
    pub id: String,
    pub creator: String,
    pub plan_id: String,
    pub state: MediaJobState,
    pub accepted_at: String,
    pub queue_ticket: String,
    pub revision: String,
    pub output: String,
    pub raw_plan_sha256: String,
    pub plan_hash: String,
    pub original_bindings_sha256: String,
    pub reservation: MixedReservation,
    pub owned_staging: Option<StagingIntent>,
    pub artifact: Option<FileIdentity>,
    pub error: Option<DomainError>,
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_MEDIA_JOB", message)
}

impl AcceptedMixedPayload {
    pub fn plan(&self) -> Result<MediaPlanV2> {
        projects::validate_id(&self.job_id)?;
        projects::validate_id(&self.root_id)?;
        hash(&self.original_bindings_sha256)?;
        hash(&self.raw_plan_sha256)?;
        hash(&self.plan_hash)?;
        if self.schema_version != 2 || projects::hash(self.raw_plan.as_bytes()) != self.raw_plan_sha256 {
            return Err(invalid("Accepted mixed plan bytes or schema differ"));
        }
        let ReviewedMediaPlan::V2(plan) = decode_reviewed_media_plan(&self.raw_plan)? else {
            return Err(invalid("Mixed accepted payload requires a MediaV2 plan"));
        };
        if plan.hash != self.plan_hash || plan.payload.root_id != self.root_id || plan.payload.principal != self.principal {
            return Err(invalid("Accepted mixed plan root, principal or semantic digest differs"));
        }
        Ok(*plan)
    }
}

impl MixedOriginalBindings {
    pub fn digest(&self) -> Result<String> {
        if self.sources.len() > MAX_CLIPS {
            return Err(invalid("Too many accepted mixed sources"));
        }
        let encoded = serde_json::to_vec(self).map_err(|_| invalid("Cannot encode private mixed bindings"))?;
        if encoded.len() > 128 * 1024 {
            return Err(invalid("Private mixed bindings exceed their serialized budget"));
        }
        let mut framed = b"NumeraWiz.MixedOriginalBindings.v2\0".to_vec();
        framed.extend(encoded);
        Ok(projects::hash(&framed))
    }

    pub fn validate(&self, accepted: &AcceptedMixedPayload, plan: &MediaPlanV2) -> Result<()> {
        if self.schema_version != 2 || self.job_id != accepted.job_id || self.root_id != accepted.root_id
            || self.principal != accepted.principal || self.raw_plan_sha256 != accepted.raw_plan_sha256
            || self.plan_hash != plan.hash || self.digest()? != accepted.original_bindings_sha256
            || self.sources.len() != plan.payload.sources.len()
        {
            return Err(invalid("Private mixed inputs differ from the accepted plan binding"));
        }
        for (original, frozen) in self.sources.iter().zip(&plan.payload.sources) {
            if original.source != *frozen || original.original.sha256 != frozen.content_sha256
                || original.native_object.schema_version != 1 || original.native_parent.schema_version != 1
                || original.original.path.is_empty() || original.original.path.len() > 16384
                || original.original.path.contains('\0') || exact(&original.original.bytes)? == 0
                || matches!(&original.source_purpose, SourcePurpose::CreatorBoundManaged { principal } if principal != &self.principal)
            {
                return Err(invalid("An accepted original differs from the reviewed source metadata"));
            }
        }
        Ok(())
    }
}

impl MediaJobV2 {
    pub fn validate_accepted(&self, accepted: &AcceptedMixedPayload, plan: &MediaPlanV2) -> Result<()> {
        if self.envelope_version != 2 || self.id != accepted.job_id || self.creator != accepted.principal || self.plan_id != plan.id
            || self.raw_plan_sha256 != accepted.raw_plan_sha256 || self.plan_hash != plan.hash
            || self.original_bindings_sha256 != accepted.original_bindings_sha256
            || self.reservation.schema_version != 2 || self.reservation.source_count != plan.payload.sources.len()
            || self.reservation.decoded_output_bytes != plan.payload.resources.decoded_output_bytes
            || exact(&self.reservation.retained_buffer_bytes)? == 0
            || exact(&self.reservation.retained_buffer_bytes)? > crate::media_execution::MEDIA_RESERVATION_POOL_BYTES
            || (self.state == MediaJobState::Succeeded) != self.artifact.is_some()
        {
            return Err(invalid("Mixed job execution rows differ from the accepted plan"));
        }
        Ok(())
    }
}

impl MixedExecutionAuthority {
    pub fn validate(&self, accepted: &AcceptedMixedPayload) -> Result<()> {
        if self.schema_version != 2 || self.job_id != accepted.job_id || self.root_id != accepted.root_id
            || self.principal != accepted.principal || self.plan_hash != accepted.plan_hash
            || self.raw_plan_sha256 != accepted.raw_plan_sha256
            || self.original_bindings_sha256 != accepted.original_bindings_sha256
            || self.output_parent.schema_version != 1 || self.output.is_empty() || self.output.len() > 16384
            || self.output.contains('\0')
        {
            return Err(invalid("Mixed execution authority does not match the accepted source and output bindings"));
        }
        Ok(())
    }
}

pub fn decode_mixed_job(bytes: &[u8]) -> Result<MediaJobV2> {
    crate::commands::decode_json(bytes)?;
    let job: MediaJobV2 = serde_json::from_slice(bytes).map_err(|_| invalid("Invalid mixed job row"))?;
    projects::validate_id(&job.id)?;
    projects::validate_id(&job.plan_id)?;
    hash(&job.raw_plan_sha256)?;
    hash(&job.plan_hash)?;
    hash(&job.original_bindings_sha256)?;
    let accepted_at = exact(&job.accepted_at)?;
    exact(&job.revision)?;
    if job.envelope_version != 2 || job.queue_ticket != format!("{accepted_at:020}/{}", job.id)
        || job.creator.is_empty() || job.creator.len() > 256 || job.creator.chars().any(char::is_control)
        || job.output.is_empty() || job.output.len() > 16384 || job.output.contains('\0')
        || job.owned_staging.as_ref().is_some_and(|intent| intent.job_id != job.id)
        || (job.state == MediaJobState::Succeeded) != job.artifact.is_some()
    {
        return Err(invalid("Mixed job identity or state is inconsistent"));
    }
    Ok(job)
}
