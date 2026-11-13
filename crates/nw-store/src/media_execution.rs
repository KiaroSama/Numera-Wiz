//! Mixed job ownership and media mutations share the existing receipt/business writers.
use crate::{RECORDS, Store, decode, encode, records::record_quota, storage};
use nw_domain::{
    DomainError, Result,
    access::{AccessProfile, ExecutionAuthorization, ObjectAction, ObjectKind, Publication},
    commands::{Request, Response},
    media_execution::{
        self as domain, AcceptedMediaPayload, MediaArtifactState, MediaJobState, MediaJobV1,
        StoredJob,
    },
    records::FileIdentity,
};
use redb::ReadableTable;
use serde_json::json;

type Records<'a> = redb::Table<'a, &'static str, &'static [u8]>;
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueFact {
    schema_version: u32,
    job_id: String,
    ticket: String,
}
fn validate_queue(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    job: &MediaJobV1,
) -> Result<()> {
    let row = table
        .get(format!("job-queue/{}", job.id).as_str())
        .map_err(storage)?;
    if job.state.terminal() {
        if row.is_some() {
            return Err(DomainError::new(
                "INVALID_RECORD",
                "Terminal media retains an active queue fact",
            ));
        }
    } else {
        let row = row.ok_or_else(|| {
            DomainError::new(
                "INVALID_RECORD",
                "Active media lost its accepted queue fact",
            )
        })?;
        let queue: QueueFact = decode(row.value())?;
        if queue.schema_version != 1 || queue.job_id != job.id || queue.ticket != job.queue_ticket {
            return Err(DomainError::new(
                "INVALID_RECORD",
                "Media queue identity differs from acceptance",
            ));
        }
    }
    Ok(())
}
fn job_key(id: &str) -> Result<String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(DomainError::new("VALIDATION_ERROR", "Invalid job identity"));
    }
    Ok(format!("job/{id}"))
}
fn read_job(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    id: &str,
) -> Result<StoredJob> {
    let key = job_key(id)?;
    let row = table
        .get(key.as_str())
        .map_err(storage)?
        .ok_or_else(DomainError::object_not_found)?;
    domain::decode_job(decode(row.value())?)
}
fn media_job(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    id: &str,
) -> Result<MediaJobV1> {
    match read_job(table, id)? {
        StoredJob::MediaV1(job) => {
            validate_queue(table, &job)?;
            Ok(*job)
        }
        StoredJob::Legacy(_) => Err(DomainError::new(
            "INVALID_RECORD",
            "Expected accepted media job",
        )),
    }
}
pub(crate) fn require_floor(table: &impl ReadableTable<&'static str, &'static [u8]>) -> Result<()> {
    let schema = table.get("meta/schema").map_err(storage)?;
    let caps = table.get("meta/required-capabilities").map_err(storage)?;
    let capabilities: Vec<String> = caps
        .as_ref()
        .map(|value| decode(value.value()))
        .transpose()?
        .unwrap_or_default();
    if schema.as_ref().map(|v| v.value()) != Some(b"3".as_slice())
        || !nw_domain::maintenance::promotion::MEDIA_V1_CAPABILITIES
            .iter()
            .all(|required| capabilities.iter().any(|cap| cap == required))
        || capabilities.len() != nw_domain::maintenance::promotion::MEDIA_V1_CAPABILITIES.len()
    {
        return Err(DomainError::new(
            "STORE_VERSION_UNSUPPORTED",
            "Media acceptance requires a completed reviewed media generation",
        ));
    }
    for key in [
        "meta/pending-promotion",
        "meta/pending-initialization",
        "meta/pending-restoration",
    ] {
        if table.get(key).map_err(storage)?.is_some() {
            return Err(DomainError::new(
                "OWNER_BUSY",
                "Accepted generation maintenance must complete before media work",
            ));
        }
    }
    Ok(())
}
fn payload(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    id: &str,
) -> Result<AcceptedMediaPayload> {
    let row = table
        .get(format!("job-payload/{id}").as_str())
        .map_err(storage)?
        .ok_or_else(|| DomainError::new("INVALID_RECORD", "Accepted media payload is missing"))?;
    decode(row.value())
}
fn allowed(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    job: &MediaJobV1,
    root: &str,
) -> Result<()> {
    require_floor(table)?;
    validate_queue(table, job)?;
    let plan = job.validate_payload(&payload(table, &job.id)?)?;
    if plan.payload.root_id != root {
        return Err(DomainError::new(
            "INVALID_PLAN",
            "Accepted media belongs to another root",
        ));
    }
    let authorization: ExecutionAuthorization = {
        let row = table
            .get(format!("job-auth/{}", job.id).as_str())
            .map_err(storage)?
            .ok_or_else(|| {
                DomainError::new("PERMISSION_DENIED", "Media authorization is missing")
            })?;
        decode(row.value())?
    };
    if authorization.principal != job.creator {
        return Err(DomainError::new(
            "PERMISSION_DENIED",
            "Media creator and authorization differ",
        ));
    }
    if let Some(name) = authorization.profile {
        let profile: AccessProfile = {
            let row = table
                .get(format!("profile/{name}").as_str())
                .map_err(storage)?
                .ok_or_else(|| {
                    DomainError::new("PERMISSION_DENIED", "Media profile was removed")
                })?;
            decode(row.value())?
        };
        let project: nw_domain::projects::ProjectHeader = {
            let row = table
                .get(format!("project/{}", plan.payload.capture.project_id).as_str())
                .map_err(storage)?
                .ok_or_else(DomainError::object_not_found)?;
            decode(row.value())?
        };
        if !profile.enabled
            || profile.principal != authorization.principal
            || !profile.allows_command("jobs.submit")
            || !profile.allows_object(
                ObjectKind::Project,
                plan.payload.capture.project_id.as_str(),
                &project.creator,
                ObjectAction::Read,
            )
            || !profile
                .write_roots
                .iter()
                .any(|path| std::path::Path::new(&job.output.output).starts_with(path))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Current media command/project/publication authority was revoked",
            ));
        }
    }
    Ok(())
}
fn write_job(table: &mut Records<'_>, job: &MediaJobV1) -> Result<()> {
    job.validate()?;
    table
        .insert(job_key(&job.id)?.as_str(), encode(job)?.as_slice())
        .map_err(storage)?;
    Ok(())
}
fn increment(job: &mut MediaJobV1) -> Result<()> {
    job.revision = domain::exact(&job.revision)?
        .checked_add(1)
        .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Media lifecycle revision exhausted"))?
        .to_string();
    Ok(())
}

impl Store {
    pub fn require_media_space(&self, parent: &std::path::Path, bytes: u64) -> Result<()> {
        if fs2::available_space(parent).map_err(storage)? < bytes {
            return Err(DomainError::new(
                "SCRATCH_EXHAUSTED",
                "Media output and verification reservation exceeds available disk space",
            ));
        }
        Ok(())
    }
    pub fn require_media_floor(&self) -> Result<()> {
        let read = self.db.begin_read().map_err(storage)?;
        require_floor(&read.open_table(RECORDS).map_err(storage)?)
    }
    pub fn inspect_execution(&self, id: &str) -> Result<StoredJob> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let job = read_job(&table, id)?;
        if let StoredJob::MediaV1(media) = &job {
            media.validate_payload(&payload(&table, id)?)?;
            validate_queue(&table, media)?;
        }
        Ok(job)
    }
    pub fn visit_executions(&self, mut visitor: impl FnMut(StoredJob) -> Result<()>) -> Result<()> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        for (index, row) in table.range("job/"..).map_err(storage)?.enumerate() {
            let (key, value) = row.map_err(storage)?;
            if !key.value().starts_with("job/") {
                break;
            }
            if index >= 4096 {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Retained job quota exceeded",
                ));
            }
            let job = domain::decode_job(decode(value.value())?)?;
            if key.value() != job_key(job.id())? {
                return Err(DomainError::new(
                    "INVALID_RECORD",
                    "Stored job key and identity differ",
                ));
            }
            if let StoredJob::MediaV1(media) = &job {
                media.validate_payload(&payload(&table, media.id.as_str())?)?;
                validate_queue(&table, media)?;
            }
            visitor(job)?;
        }
        Ok(())
    }
    pub fn accepted_media_payload(&self, id: &str) -> Result<AcceptedMediaPayload> {
        job_key(id)?;
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let result = payload(&table, id)?;
        media_job(&table, id)?.validate_payload(&result)?;
        Ok(result)
    }
    pub fn accept_media_with_clock(
        &self,
        principal: &str,
        request: &Request,
        job: &MediaJobV1,
        accepted: &AcceptedMediaPayload,
        authorization: &ExecutionAuthorization,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Response> {
        let plan = job.validate_payload(accepted)?;
        if job.state != MediaJobState::Queued
            || job.creator != principal
            || authorization.principal != principal
            || plan.payload.root_id != self.root_id()
        {
            return Err(DomainError::new(
                "INVALID_ACCEPTANCE",
                "Media acceptance identity differs",
            ));
        }
        self.commit_composed_with_clock(principal, request, clock, |table, now| {
            require_floor(table)?;
            if domain::exact(&job.resources.estimated_memory_bytes)? > domain::MEDIA_RESERVATION_POOL_BYTES {
                return Err(DomainError::new("RESOURCE_LIMIT", "Worker and bounded queue reservations exceed the fixed admission pool"));
            }
            if !domain::review_current(&plan.created_at, &plan.expires_at, now)? {
                return Err(DomainError::new("PLAN_EXPIRED", "Media review expired before acceptance"));
            }
            let header = table.get(format!("project/{}", plan.payload.capture.project_id).as_str()).map_err(storage)?
                .ok_or_else(DomainError::object_not_found)?;
            if nw_domain::projects::hash(header.value()) != plan.payload.capture.header_sha256 {
                return Err(DomainError::new("STALE_REVISION", "Project changed before atomic media acceptance"));
            }
            drop(header);
            for fact in &plan.payload.capture.facts {
                let key = format!("project-entity/{}/{}", plan.payload.capture.project_id, fact.id);
                let row = table.get(key.as_str()).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
                if row.value().len() != fact.bytes || nw_domain::projects::hash(row.value()) != fact.sha256 {
                    return Err(DomainError::new("STALE_REVISION", "Captured entity changed before atomic media acceptance"));
                }
            }
            let mut pending = 0;
            for row in table.range("job/"..).map_err(storage)? {
                let (key, value) = row.map_err(storage)?;
                if !key.value().starts_with("job/") { break; }
                if !domain::decode_job(decode(value.value())?)?.terminal() { pending += 1; }
                if pending >= domain::MAX_QUEUE { return Err(DomainError::new("RESOURCE_LIMIT", "Durable job queue budget reached")); }
            }
            let mut accepted_job = job.clone();
            accepted_job.accepted_at = now.to_string();
            accepted_job.queue_ticket = format!("{now:020}/{}", job.id);
            for (key, bytes) in [
                (job_key(&job.id)?, encode(&accepted_job)?),
                (format!("job-payload/{}", job.id), encode(accepted)?),
                (format!("job-auth/{}", job.id), encode(authorization)?),
                (format!("job-queue/{}", job.id), encode(&json!({"schema_version":1,"job_id":job.id,"ticket":accepted_job.queue_ticket}))?),
                (format!("plan/{}", plan.id), accepted.media_plan_json.as_bytes().to_vec()),
            ] {
                if let Some(existing) = table.get(key.as_str()).map_err(storage)? {
                    if key == format!("plan/{}", plan.id) {
                        let prior = domain::decode_plan(std::str::from_utf8(existing.value()).map_err(storage)?)?;
                        if serde_json::to_value(&prior).map_err(storage)? == serde_json::to_value(&plan).map_err(storage)? {
                            continue;
                        }
                    }
                    return Err(DomainError::new("OBJECT_CONFLICT", "Accepted media identity already exists with different content"));
                }
                record_quota(table, &key)?;
                table.insert(key.as_str(), bytes.as_slice()).map_err(storage)?;
            }
            allowed(table, &accepted_job, self.root_id())?;
            Ok(json!({"job_id":job.id,"state":"queued"}))
        })
    }
    pub fn media_execution_allowed(&self, id: &str) -> Result<()> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        allowed(&table, &media_job(&table, id)?, self.root_id())
    }
    pub fn update_media_job(
        &self,
        id: &str,
        update: impl FnOnce(&mut MediaJobV1) -> Result<()>,
    ) -> Result<MediaJobV1> {
        let write = self.db.begin_write().map_err(storage)?;
        let job;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let mut current = media_job(&table, id)?;
            if current.state.terminal() {
                return Err(DomainError::new(
                    "INVALID_JOB_TRANSITION",
                    "Terminal media facts are immutable",
                ));
            }
            let immutable = (
                current.id.clone(),
                current.creator.clone(),
                current.plan_id.clone(),
                current.plan_hash.clone(),
                current.payload_sha256.clone(),
                current.accepted_at.clone(),
                current.queue_ticket.clone(),
                encode(&current.output)?,
                encode(&current.resources)?,
            );
            update(&mut current)?;
            current.finish_active_stages();
            if immutable
                != (
                    current.id.clone(),
                    current.creator.clone(),
                    current.plan_id.clone(),
                    current.plan_hash.clone(),
                    current.payload_sha256.clone(),
                    current.accepted_at.clone(),
                    current.queue_ticket.clone(),
                    encode(&current.output)?,
                    encode(&current.resources)?,
                )
            {
                return Err(DomainError::new(
                    "INVALID_JOB_TRANSITION",
                    "Accepted media identity/resources/output cannot change",
                ));
            }
            increment(&mut current)?;
            current.validate_payload(&payload(&table, id)?)?;
            write_job(&mut table, &current)?;
            if current.state.terminal() {
                table
                    .remove(format!("job-queue/{id}").as_str())
                    .map_err(storage)?;
            }
            job = current;
        }
        write.commit().map_err(storage)?;
        Ok(job)
    }
    pub fn cancel_media_with_clock(
        &self,
        principal: &str,
        request: &Request,
        id: &str,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Response> {
        self.commit_composed_with_clock(principal, request, clock, |table, _now| {
            let mut job = media_job(table, id)?;
            if job.state == MediaJobState::Publishing { return Err(DomainError::new("CANCELLATION_TOO_LATE", "Media publication fence has committed")); }
            if !job.state.terminal() {
                job.state = if job.state == MediaJobState::Queued { MediaJobState::Cancelled } else { MediaJobState::Cancelling };
                increment(&mut job)?;
                write_job(table, &job)?;
                table.insert(format!("cancel/{id}").as_str(), b"true".as_slice()).map_err(storage)?;
                if job.state.terminal() { table.remove(format!("job-queue/{id}").as_str()).map_err(storage)?; }
            }
            Ok(json!({"job_id":id,"state":job.state,"cancel_requested":job.state == MediaJobState::Cancelling}))
        })
    }
    pub fn media_published(
        &self,
        id: &str,
        artifact: FileIdentity,
        verification: String,
    ) -> Result<MediaJobV1> {
        let write = self.db.begin_write().map_err(storage)?;
        let job;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let mut current = media_job(&table, id)?;
            let key = format!("publication/{id}/{}", domain::MEDIA_OUTPUT_ARTIFACT);
            let mut publication: Publication = {
                let row = table
                    .get(key.as_str())
                    .map_err(storage)?
                    .ok_or_else(DomainError::object_not_found)?;
                decode(row.value())?
            };
            if current.state != MediaJobState::Publishing
                || publication.identity != artifact
                || publication.verified != verification
                || publication.output != current.output.output
            {
                return Err(DomainError::new(
                    "VERIFICATION_FAILED",
                    "Published media differs from the durable fence",
                ));
            }
            publication.published = true;
            current.state = MediaJobState::Succeeded;
            current.artifact.state = MediaArtifactState::Published;
            current.artifact.identity = Some(artifact);
            current.artifact.verification = Some(verification);
            current.stages[2].state = MediaJobState::Succeeded;
            increment(&mut current)?;
            table
                .insert(key.as_str(), encode(&publication)?.as_slice())
                .map_err(storage)?;
            write_job(&mut table, &current)?;
            table
                .remove(format!("job-queue/{id}").as_str())
                .map_err(storage)?;
            job = current;
        }
        write.commit().map_err(storage)?;
        Ok(job)
    }
}

/// Dispatch inside the existing publication transaction, before the legacy decoder.
pub(crate) fn publication_fence(
    table: &mut Records<'_>,
    root: &str,
    publication: &Publication,
) -> Result<bool> {
    let StoredJob::MediaV1(mut job) = read_job(table, &publication.job_id)? else {
        return Ok(false);
    };
    allowed(table, &job, root)?;
    if job.state != MediaJobState::Verifying
        || table
            .get(format!("cancel/{}", job.id).as_str())
            .map_err(storage)?
            .is_some()
    {
        return Err(DomainError::new(
            "STALE_JOB_STATE",
            "Media publication requires uncancelled verification",
        ));
    }
    let staged = job.owned_staging.as_ref().ok_or_else(|| {
        DomainError::new("VERIFICATION_REQUIRED", "Media staging intent is missing")
    })?;
    let proof = std::path::Path::new(&publication.proof);
    if publication.published
        || publication.read_purpose_version != 1
        || publication.output != job.output.output
        || publication.identity.path != job.output.output
        || staged.job_id != job.id
        || proof.parent() != Some(std::path::Path::new(&staged.directory))
        || !proof
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(".numera-proof-"))
            .is_some_and(|n| uuid::Uuid::parse_str(n).is_ok())
        || domain::exact(&publication.identity.bytes)?
            > domain::exact(&job.resources.maximum_output_bytes)?
        || publication.verified.is_empty()
    {
        return Err(DomainError::new(
            "VERIFICATION_FAILED",
            "Media fence differs from the accepted output claim",
        ));
    }
    domain::hash(&publication.identity.sha256)?;
    job.state = MediaJobState::Publishing;
    job.artifact.state = MediaArtifactState::Publishing;
    job.artifact.identity = Some(publication.identity.clone());
    job.artifact.verification = Some(publication.verified.clone());
    job.stages[1].state = MediaJobState::Succeeded;
    job.stages[2].state = MediaJobState::Publishing;
    increment(&mut job)?;
    let key = format!("publication/{}/{}", job.id, domain::MEDIA_OUTPUT_ARTIFACT);
    record_quota(table, &key)?;
    table
        .insert(key.as_str(), encode(publication)?.as_slice())
        .map_err(storage)?;
    write_job(table, &job)?;
    Ok(true)
}
