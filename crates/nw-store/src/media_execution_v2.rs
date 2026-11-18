//! Atomic mixed-media acceptance shares the existing durable replay transaction.
use crate::{
    Store, decode, encode,
    media_execution::{MediaFloor, require_media_floor, reserve_queue_room, validate_queue_identity},
    records::record_quota, storage,
};
use nw_domain::{
    DomainError, Result,
    access::{AccessProfile, ObjectAction, ObjectKind, ProfileMode, Publication},
    commands::{Request, Response},
    media::MediaPlanV2,
    media_execution::{self as domain, MediaJobState},
    media_v2_execution::{AcceptedMixedPayload, MediaJobV2, MixedExecutionAuthority, MixedOriginalBindings, decode_mixed_job},
    plans::SourcePurpose, projects, records::{Asset, StagingIntent},
};
use redb::ReadableTable;
use serde_json::json;
use std::path::Path;

pub struct MixedAcceptance<'a> {
    pub job: &'a MediaJobV2,
    pub payload: &'a AcceptedMixedPayload,
    pub originals: &'a MixedOriginalBindings,
    pub authority: &'a MixedExecutionAuthority,
}

pub struct MixedExecution {
    pub job: MediaJobV2,
    pub payload: AcceptedMixedPayload,
    pub plan: MediaPlanV2,
    pub originals: MixedOriginalBindings,
    pub authority: MixedExecutionAuthority,
}

fn denied(message: &str) -> DomainError {
    DomainError::new("PERMISSION_DENIED", message)
}

fn read_required<T: serde::de::DeserializeOwned>(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
) -> Result<T> {
    let row = table.get(key).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
    decode(row.value())
}

pub(crate) fn read_execution(table: &impl ReadableTable<&'static str, &'static [u8]>, id: &str) -> Result<MixedExecution> {
    projects::validate_id(id)?;
    let job = table.get(format!("job/{id}").as_str()).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
    let job = decode_mixed_job(job.value())?;
    let payload: AcceptedMixedPayload = read_required(table, &format!("job-payload/{id}"))?;
    let plan = payload.plan()?;
    let authority: MixedExecutionAuthority = read_required(table, &format!("job-auth-v2/{id}"))?;
    let originals: MixedOriginalBindings = read_required(table, &format!("job-input-v2/{id}"))?;
    let raw = table.get(format!("plan/{}", plan.id).as_str()).map_err(storage)?
        .ok_or_else(|| denied("Exact reviewed plan row is missing"))?;
    if job.id != id || job.output != authority.output || raw.value() != payload.raw_plan.as_bytes() {
        return Err(denied("Mixed execution record and stored raw plan differ"));
    }
    validate_queue_identity(table, id, &job.queue_ticket, job.state.terminal(), 2)?;
    job.validate_accepted(&payload, &plan)?;
    authority.validate(&payload)?;
    originals.validate(&payload, &plan)?;
    Ok(MixedExecution { job, payload, plan, originals, authority })
}

fn save_state(
    table: &mut redb::Table<'_, &'static str, &'static [u8]>,
    job: &mut MediaJobV2,
    state: MediaJobState,
) -> Result<()> {
    job.revision = domain::exact(&job.revision)?.checked_add(1)
        .ok_or_else(|| denied("Mixed execution revision counter exhausted"))?.to_string();
    job.state = state;
    let bytes = encode(job)?;
    decode_mixed_job(&bytes)?;
    table.insert(format!("job/{}", job.id).as_str(), bytes.as_slice()).map_err(storage)?;
    if state.terminal() {
        table.remove(format!("job-queue/{}", job.id).as_str()).map_err(storage)?;
    }
    Ok(())
}

fn current_authority(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    plan: &MediaPlanV2,
    originals: &MixedOriginalBindings,
    authority: &MixedExecutionAuthority,
) -> Result<()> {
    let capture = &plan.payload.capture;
    let header_key = format!("project/{}", capture.project_id);
    let header = table.get(header_key.as_str()).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
    let profile = if let Some(profile) = &authority.profile {
        let key = format!("profile/{profile}");
        let row = table.get(key.as_str()).map_err(storage)?.ok_or_else(|| denied("Mixed operation profile is absent"))?;
        let profile: AccessProfile = decode(row.value())?;
        let header: projects::ProjectHeader = decode(header.value())?;
        if !profile.enabled || profile.principal != authority.principal || !profile.allows_command("jobs.submit")
            || !profile.allows_object(ObjectKind::Project, capture.project_id.as_str(), &header.creator, ObjectAction::Read)
            || !profile.write_roots.iter().any(|root| Path::new(&authority.output).starts_with(root))
        {
            return Err(denied("Current mixed project or publication authority differs"));
        }
        Some(profile)
    } else { None };
    for original in &originals.sources {
        let key = format!("asset/{}", original.source.asset_id);
        let row = table.get(key.as_str()).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
        if projects::hash(row.value()) != original.source.asset_row_sha256 {
            return Err(DomainError::new("SOURCE_CHANGED", "Registered source row changed before mixed media acceptance"));
        }
        let asset: Asset = decode(row.value())?;
        if asset.identity != original.original || asset.source_object.as_ref() != Some(&original.native_object) {
            return Err(DomainError::new("SOURCE_CHANGED", "Registered source native identity differs from the bound original"));
        }
        if let Some(profile) = &profile
            && (!profile.allows_object(ObjectKind::Asset, &asset.id, &asset.creator, ObjectAction::Read)
                || !profile.read_roots.iter().any(|root| Path::new(&original.original.path).starts_with(root)))
        {
            return Err(denied("Current mixed source object or path authority differs"));
        }
        if let (Some(profile), SourcePurpose::CreatorBoundManaged { principal }) = (&profile, &original.source_purpose) {
            let key = format!("managed-read/{}/{}", projects::hash(original.original.path.as_bytes()), principal);
            if principal != &authority.principal || profile.mode != ProfileMode::Operate || !profile.owned_object_access
                || table.get(key.as_str()).map_err(storage)?.map(|value| decode::<bool>(value.value())).transpose()? != Some(true)
            {
                return Err(denied("Mixed managed input no longer belongs to the accepted creator"));
            }
        }
    }
    Ok(())
}

fn captured_facts(table: &impl ReadableTable<&'static str, &'static [u8]>, plan: &MediaPlanV2) -> Result<()> {
    let capture = &plan.payload.capture;
    let prefix = format!("project-entity/{}/", capture.project_id);
    let header_key = format!("project/{}", capture.project_id);
    let header = table.get(header_key.as_str()).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
    if header.value().len() != capture.header_bytes || projects::hash(header.value()) != capture.header_sha256 {
        return Err(DomainError::new("STALE_REVISION", "Project changed before mixed media acceptance"));
    }
    for fact in &capture.facts {
        let key = format!("{prefix}{}", fact.id);
        let row = table.get(key.as_str()).map_err(storage)?.ok_or_else(DomainError::object_not_found)?;
        if row.value().len() != fact.bytes || projects::hash(row.value()) != fact.sha256 {
            return Err(DomainError::new("STALE_REVISION", "Captured entity changed before mixed media acceptance"));
        }
    }
    Ok(())
}

impl Store {
    pub fn require_mixed_media_floor(&self) -> Result<()> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(crate::RECORDS).map_err(storage)?;
        require_media_floor(&table, MediaFloor::V2)
    }

    pub fn inspect_mixed_execution(&self, id: &str) -> Result<MixedExecution> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(crate::RECORDS).map_err(storage)?;
        read_execution(&table, id)
    }

    pub fn allow_mixed_execution(&self, id: &str) -> Result<MixedExecution> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(crate::RECORDS).map_err(storage)?;
        require_media_floor(&table, MediaFloor::V2)?;
        let accepted = read_execution(&table, id)?;
        if accepted.payload.root_id != self.root_id() {
            return Err(denied("Accepted mixed media belongs to a different root"));
        }
        current_authority(&table, &accepted.plan, &accepted.originals, &accepted.authority)?;
        Ok(accepted)
    }

    pub fn journal_mixed_workspace(&self, id: &str, directory: &Path) -> Result<()> {
        let write = self.db.begin_write().map_err(storage)?;
        {
            let mut table = write.open_table(crate::RECORDS).map_err(storage)?;
            let MixedExecution { mut job, authority, .. } = read_execution(&table, id)?;
            let name = directory.file_name().and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(".numera-job-")).ok_or_else(|| denied("Mixed workspace name is outside the owned namespace"))?;
            projects::validate_id(name)?;
            if job.state != MediaJobState::Preparing || job.owned_staging.is_some() || directory.parent() != Path::new(&authority.output).parent() {
                return Err(denied("Mixed workspace lies outside the authorized output parent or preparation phase"));
            }
            job.owned_staging = Some(StagingIntent {
                job_id: id.to_owned(), directory: nw_platform::files::path_text(directory)?,
                payload: nw_platform::files::path_text(&directory.join("payload.mkv"))?,
            });
            save_state(&mut table, &mut job, MediaJobState::Preparing)?;
        }
        write.commit().map_err(storage)
    }

    pub fn advance_mixed_job(&self, id: &str, next: MediaJobState, failure: Option<DomainError>) -> Result<MediaJobV2> {
        let write = self.db.begin_write().map_err(storage)?;
        let job = {
            let mut table = write.open_table(crate::RECORDS).map_err(storage)?;
            require_media_floor(&table, MediaFloor::V2)?;
            let MixedExecution { mut job, plan, originals, authority, .. } = read_execution(&table, id)?;
            let valid = match (job.state, next) {
                (MediaJobState::Queued, MediaJobState::Preparing)
                | (MediaJobState::Preparing, MediaJobState::Running)
                | (MediaJobState::Running, MediaJobState::Verifying) => {
                    current_authority(&table, &plan, &originals, &authority)?;
                    failure.is_none()
                }
                (MediaJobState::Queued | MediaJobState::Preparing | MediaJobState::Running | MediaJobState::Verifying, MediaJobState::Cancelling) => failure.is_none(),
                (MediaJobState::Queued | MediaJobState::Preparing | MediaJobState::Running | MediaJobState::Verifying | MediaJobState::Cancelling, MediaJobState::Interrupted | MediaJobState::Failed | MediaJobState::Cancelled) => failure.is_some(),
                (MediaJobState::Publishing, MediaJobState::Interrupted) => failure.is_some(),
                _ => false,
            };
            if !valid {
                return Err(DomainError::new("STALE_JOB_STATE", "Mixed job cannot enter the requested lifecycle stage"));
            }
            job.error = failure;
            save_state(&mut table, &mut job, next)?;
            job
        };
        write.commit().map_err(storage)?;
        Ok(job)
    }

    pub fn cancel_mixed_with_clock(&self, principal: &str, request: &Request, id: &str, clock: impl FnOnce() -> Result<u64>) -> Result<Response> {
        self.commit_composed_with_clock(principal, request, clock, |table, _now| {
            let mut job = read_execution(table, id)?.job;
            if job.state == MediaJobState::Publishing {
                return Err(DomainError::new("CANCELLATION_TOO_LATE", "Mixed publication fence has committed"));
            }
            if !job.state.terminal() && job.state != MediaJobState::Cancelling {
                let state = if job.state == MediaJobState::Queued { MediaJobState::Cancelled } else { MediaJobState::Cancelling };
                save_state(table, &mut job, state)?;
            }
            Ok(json!({"job_id":id,"state":job.state}))
        })
    }

    pub fn fence_mixed_publication(&self, publication: &Publication) -> Result<()> {
        let write = self.db.begin_write().map_err(storage)?;
        {
            let mut table = write.open_table(crate::RECORDS).map_err(storage)?;
            require_media_floor(&table, MediaFloor::V2)?;
            let id = &publication.job_id;
            let MixedExecution { mut job, plan, originals, authority, .. } = read_execution(&table, id)?;
            current_authority(&table, &plan, &originals, &authority)?;
            let staged = job.owned_staging.as_ref().ok_or_else(|| denied("Mixed output workspace was not journaled"))?;
            if job.state != MediaJobState::Verifying || publication.published || publication.read_purpose_version != 1
                || publication.output != authority.output || publication.identity.path != authority.output
                || Path::new(&publication.proof).parent() != Some(Path::new(&staged.directory))
                || domain::exact(&publication.identity.bytes)? == 0 || publication.verified.is_empty()
            {
                return Err(denied("Mixed output publication does not match the authorized verification state"));
            }
            domain::hash(&publication.identity.sha256)?;
            let key = format!("publication/{id}/{}", domain::MEDIA_OUTPUT_ARTIFACT);
            record_quota(&table, &key)?;
            if table.insert(key.as_str(), encode(publication)?.as_slice()).map_err(storage)?.is_some() {
                return Err(denied("Mixed output already has a durable publication fence"));
            }
            save_state(&mut table, &mut job, MediaJobState::Publishing)?;
        }
        write.commit().map_err(storage)
    }

    pub fn complete_mixed_publication(&self, id: &str, file: nw_domain::records::FileIdentity, verification: &str) -> Result<MediaJobV2> {
        let write = self.db.begin_write().map_err(storage)?;
        let job = {
            let mut table = write.open_table(crate::RECORDS).map_err(storage)?;
            let MixedExecution { mut job, authority, .. } = read_execution(&table, id)?;
            let key = format!("publication/{id}/{}", domain::MEDIA_OUTPUT_ARTIFACT);
            let mut fence: Publication = read_required(&table, &key)?;
            if job.state != MediaJobState::Publishing || fence.published || fence.verified != verification
                || fence.output != authority.output || fence.identity != file
            {
                return Err(denied("Published mixed output differs from its durable fence"));
            }
            fence.published = true;
            table.insert(key.as_str(), encode(&fence)?.as_slice()).map_err(storage)?;
            job.artifact = Some(file);
            save_state(&mut table, &mut job, MediaJobState::Succeeded)?;
            job
        };
        write.commit().map_err(storage)?;
        Ok(job)
    }

    pub fn accept_mixed_with_clock(
        &self,
        principal: &str,
        request: &Request,
        acceptance: MixedAcceptance<'_>,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Response> {
        let MixedAcceptance { job, payload, originals, authority } = acceptance;
        self.commit_composed_with_clock(principal, request, clock, |table, now| {
            require_media_floor(table, MediaFloor::V2)?;
            let plan = payload.plan()?;
            if request.command.id != "jobs.submit" || request.params["media_plan_json"].as_str() != Some(payload.raw_plan.as_str())
                || job.state != MediaJobState::Queued || job.owned_staging.is_some() || job.output != authority.output
                || payload.principal != principal || payload.root_id != self.root_id()
            {
                return Err(denied("Mixed acceptance identity or exact request bytes differ"));
            }
            job.validate_accepted(payload, &plan)?;
            originals.validate(payload, &plan)?;
            authority.validate(payload)?;
            if !domain::review_current(&plan.created_at, &plan.expires_at, now)? {
                return Err(DomainError::new("PLAN_EXPIRED", "Mixed plan expired before atomic acceptance"));
            }
            captured_facts(table, &plan)?;
            current_authority(table, &plan, originals, authority)?;
            reserve_queue_room(table, domain::exact(&job.reservation.retained_buffer_bytes)?)?;
            let mut accepted = job.clone();
            accepted.accepted_at = now.to_string();
            accepted.queue_ticket = format!("{now:020}/{}", job.id);
            let encoded = encode(&accepted)?;
            decode_mixed_job(&encoded)?;
            for (key, row) in [
                (format!("job/{}", job.id), encoded),
                (format!("job-payload/{}", job.id), encode(payload)?),
                (format!("job-auth-v2/{}", job.id), encode(authority)?),
                (format!("job-input-v2/{}", job.id), encode(originals)?),
                (format!("job-queue/{}", job.id), encode(&json!({"schema_version":2,"job_id":job.id,"ticket":accepted.queue_ticket}))?),
                (format!("plan/{}", plan.id), payload.raw_plan.as_bytes().to_vec()),
            ] {
                if let Some(previous) = table.get(key.as_str()).map_err(storage)? {
                    if key == format!("plan/{}", plan.id) && previous.value() == row.as_slice() {
                        continue;
                    }
                    return Err(DomainError::new("OBJECT_CONFLICT", "Accepted mixed record identity already exists"));
                }
                record_quota(table, &key)?;
                table.insert(key.as_str(), row.as_slice()).map_err(storage)?;
            }
            for (index, original) in originals.sources.iter().enumerate() {
                let key = format!("job-dependency-v2/{}/{index:03}", job.id);
                let row = encode(&json!({"schema_version":2,"asset_id":original.source.asset_id,"asset_row_sha256":original.source.asset_row_sha256,"stream_index":original.source.stream_index}))?;
                record_quota(table, &key)?;
                if table.insert(key.as_str(), row.as_slice()).map_err(storage)?.is_some() {
                    return Err(DomainError::new("OBJECT_CONFLICT", "Accepted mixed dependency identity already exists"));
                }
            }
            Ok(json!({"job_id":job.id,"state":"queued"}))
        })
    }
}
