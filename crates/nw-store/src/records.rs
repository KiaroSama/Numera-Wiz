//! Bounded records, replay-first acceptance and atomic execution journals.
use super::*;
use nw_domain::{
    access::{AccessProfile, ExecutionAuthorization, ObjectAction, ObjectKind, Publication},
    records::{FileIdentity, JobState},
};
use serde_json::Value;

fn valid_key(key: &str) -> Result<()> {
    if key.len() > 512 || key.is_empty() || key.starts_with("meta/") || key.starts_with("receipt/")
    {
        return Err(DomainError::new(
            "INVALID_RECORD",
            "Invalid internal record key",
        ));
    }
    Ok(())
}
fn record_quota(table: &impl ReadableTable<&'static str, &'static [u8]>, key: &str) -> Result<()> {
    if table.get(key).map_err(storage)?.is_some() {
        return Ok(());
    }
    for (prefix, limit) in [
        ("asset/", 4096usize),
        ("batch-plan/", 128),
        ("batch/", 1024),
        ("generation-migration/", 16),
        ("job/", 4096),
        ("profile/", 128),
        ("plan/", 512),
        ("receipt/", 10000),
    ] {
        if key.starts_with(prefix) {
            let mut count = 0;
            for row in table.range(prefix..).map_err(storage)? {
                let (name, _) = row.map_err(storage)?;
                if !name.value().starts_with(prefix) {
                    break;
                }
                count += 1;
                if count >= limit {
                    return Err(DomainError::new(
                        "RESOURCE_LIMIT",
                        format!(
                            "Retained {prefix} record quota reached; protected history was not evicted"
                        ),
                    ));
                }
            }
            break;
        }
    }
    Ok(())
}
fn execution_policy(
    profile: &AccessProfile,
    authorization: &ExecutionAuthorization,
    job: &Job,
) -> Result<()> {
    use std::path::Path;
    let source = &job.plan.source;
    if job.plan.schema_version == 2 {
        let rights = nw_domain::plans::validate_requirements(&job.plan)?;
        if let nw_domain::plans::SourcePurpose::CreatorBoundManaged { principal } =
            &rights.source_read.purpose
            && (principal != &authorization.principal || !profile.owned_object_access)
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Creator-bound execution authority was withdrawn",
            ));
        }
    }
    if !profile.enabled
        || profile.principal != authorization.principal
        || !profile.allows_command("jobs.submit")
        || !profile.allows_object(
            ObjectKind::Plan,
            &job.plan.id,
            &job.plan.creator,
            ObjectAction::Submit,
        )
        || !profile
            .read_roots
            .iter()
            .any(|root| Path::new(&source.identity.path).starts_with(root))
        || !profile
            .write_roots
            .iter()
            .any(|root| Path::new(&job.plan.output).starts_with(root))
    {
        return Err(DomainError::new(
            "PERMISSION_DENIED",
            "Current execution command, object or path authority was revoked",
        ));
    }
    Ok(())
}
pub(super) fn decode_job(bytes: &[u8]) -> Result<Job> {
    let raw: Value = decode(bytes)?;
    nw_domain::plans::decode(
        raw.get("plan")
            .cloned()
            .ok_or_else(|| DomainError::new("INVALID_PLAN", "Stored job has no plan"))?,
    )?;
    serde_json::from_value(raw).map_err(storage)
}
impl Store {
    pub fn get_plan(&self, id: &str) -> Result<Option<nw_domain::records::Plan>> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Invalid bounded plan identifier",
            ));
        }
        let raw: Option<Value> = self.get(&format!("plan/{id}"))?;
        raw.map(nw_domain::plans::decode).transpose()
    }
    /// Visit one immutable read transaction without retaining full records in a collection.
    pub fn visit<T: DeserializeOwned>(
        &self,
        prefix: &str,
        mut visitor: impl FnMut(T) -> Result<()>,
    ) -> Result<()> {
        valid_key(prefix)?;
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        for row in table.range(prefix..).map_err(storage)? {
            let (key, value) = row.map_err(storage)?;
            if !key.value().starts_with(prefix) {
                break;
            }
            visitor(decode(value.value())?)?;
        }
        Ok(())
    }
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        valid_key(key)?;
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        table
            .get(key)
            .map_err(storage)?
            .map(|value| decode(value.value()))
            .transpose()
    }
    pub fn list<T: DeserializeOwned>(&self, prefix: &str, limit: usize) -> Result<Vec<T>> {
        self.rows(prefix, limit)
            .map(|rows| rows.into_iter().map(|(_, value)| value).collect())
    }
    pub fn rows<T: DeserializeOwned>(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<(String, T)>> {
        valid_key(prefix)?;
        if limit == 0 || limit > 10000 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Record page limit exceeds 10000",
            ));
        }
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let mut records = Vec::new();
        for row in table.range(prefix..).map_err(storage)? {
            let (key, value) = row.map_err(storage)?;
            if !key.value().starts_with(prefix) {
                break;
            }
            if records.len() == limit {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Result exceeds the admitted page; narrow the query",
                ));
            }
            records.push((key.value().to_owned(), decode(value.value())?));
        }
        Ok(records)
    }
    pub fn remove(&self, key: &str) -> Result<()> {
        valid_key(key)?;
        let write = if key.starts_with("credential/") {
            self.db.begin_control_write()
        } else {
            self.db.begin_write()
        }.map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            table.remove(key).map_err(storage)?;
        }
        write.commit().map_err(storage)
    }
    pub fn put(&self, key: &str, value: &impl Serialize) -> Result<()> {
        self.internal_batch(&[(key.into(), serde_json::to_value(value).map_err(storage)?)])
    }
    pub fn internal_batch(&self, records: &[(String, Value)]) -> Result<()> {
        if records.len() > 64 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Transaction record budget exceeded",
            ));
        }
        let write = if records.iter().all(|(key,_)|key.starts_with("credential/")) {
            self.db.begin_control_write()
        } else {
            self.db.begin_write()
        }.map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            for (key, value) in records {
                valid_key(key)?;
                record_quota(&table, key)?;
                table
                    .insert(key.as_str(), encode(value)?.as_slice())
                    .map_err(storage)?;
            }
        }
        write.commit().map_err(storage)
    }
    fn replay_key(&self, principal: &str, request: &Request, now: u64) -> Result<(String, Scope)> {
        let scope = self.verify_scope(
            request.replay_scope.as_deref().ok_or_else(|| {
                DomainError::new("REPLAY_SCOPE_INVALID", "A server-issued scope is required")
            })?,
            principal,
            now,
        )?;
        let key = request
            .idempotency_key
            .as_deref()
            .filter(|key| !key.is_empty() && key.len() <= 128)
            .ok_or_else(|| {
                DomainError::new("VALIDATION_ERROR", "A bounded idempotency key is required")
            })?;
        let namespace = encode(&(principal, &scope.id, &request.command.id, key))?;
        Ok((
            format!("receipt/{}", hex::encode(Sha256::digest(namespace))),
            scope,
        ))
    }
    /// Inspect receipt identity before resolving any mutable target or checking its current revision.
    pub fn replay(&self, principal: &str, request: &Request, now: u64) -> Result<Option<Response>> {
        self.replay_with_clock(principal, request, || Ok(now))
    }
    pub fn replay_with_clock(
        &self,
        principal: &str,
        request: &Request,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Option<Response>> {
        let read = self.db.begin_read().map_err(storage)?;
        let now = clock()?;
        let (key, _) = self.replay_key(principal, request, now)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        check_clock(&table, now)?;
        if let Some(value) = table.get(key.as_str()).map_err(storage)? {
            let receipt: Receipt = decode(value.value())?;
            if receipt.digest != request.structural_digest() {
                return Err(DomainError::new(
                    "IDEMPOTENCY_CONFLICT",
                    "The same scope/key names different content",
                ));
            }
            let mut response = receipt.response;
            response.request_id = request.request_id.clone();
            return Ok(Some(response));
        }
        Ok(None)
    }
    /// Caller serializes semantic acceptance; this transaction rechecks replay and commits every record with the receipt.
    pub fn commit(
        &self,
        principal: &str,
        request: &Request,
        now: u64,
        result: Value,
        records: &[(String, Value)],
    ) -> Result<Response> {
        self.commit_with_clock(principal, request, || Ok(now), result, records)
    }
    pub fn commit_with_clock(
        &self,
        principal: &str,
        request: &Request,
        clock: impl FnOnce() -> Result<u64>,
        result: Value,
        records: &[(String, Value)],
    ) -> Result<Response> {
        self.commit_records_with_clock(principal, request, clock, result, records, 64)
    }
    fn commit_records_with_clock(
        &self,
        principal: &str,
        request: &Request,
        clock: impl FnOnce() -> Result<u64>,
        result: Value,
        records: &[(String, Value)],
        record_limit: usize,
    ) -> Result<Response> {
        if records.len() > record_limit || record_limit > 129 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Mutation record budget exceeded",
            ));
        }
        self.commit_composed_with_clock(principal, request, clock, |table, _now| {
            for (record_key, value) in records {
                valid_key(record_key)?;
                record_quota(table, record_key)?;
                table
                    .insert(record_key.as_str(), encode(value)?.as_slice())
                    .map_err(storage)?;
            }
            Ok(result)
        })
    }
    /// The same replay/clock/receipt transaction kernel serves ordinary and composed mutations.
    pub(super) fn commit_composed_with_clock(
        &self,
        principal: &str,
        request: &Request,
        clock: impl FnOnce() -> Result<u64>,
        compose: impl FnOnce(&mut redb::Table<'_, &'static str, &'static [u8]>, u64) -> Result<Value>,
    ) -> Result<Response> {
        request.validate()?;
        let digest = request.structural_digest();
        let write = if request.command.id.starts_with("maintenance.") {
            self.db.begin_control_write()
        } else {
            self.db.begin_write()
        }.map_err(storage)?;
        let now = clock()?;
        let (key, scope) = self.replay_key(principal, request, now)?;
        let response;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            check_clock(&table, now)?;
            if let Some(value) = table.get(key.as_str()).map_err(storage)? {
                let receipt: Receipt = decode(value.value())?;
                if receipt.digest != digest {
                    return Err(DomainError::new(
                        "IDEMPOTENCY_CONFLICT",
                        "Scope/key content conflict",
                    ));
                }
                let mut response = receipt.response;
                response.request_id = request.request_id.clone();
                return Ok(response);
            }
            let result = compose(&mut table, now)?;
            response = {
                let mut response = Response::success(&request.request_id, result);
                response.receipt_id = Some(Uuid::new_v4().to_string());
                response
            };
            if serde_json::to_vec(&response).map_err(storage)?.len()
                > nw_domain::commands::MAX_RESULT_BYTES
            {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Mutation result exceeds its admitted response budget before acceptance",
                ));
            }
            record_quota(&table, &key)?;
            let receipt = Receipt {
                digest,
                response: response.clone(),
                expires_at: scope.expires_at.parse().map_err(storage)?,
            };
            table
                .insert(key.as_str(), encode(&receipt)?.as_slice())
                .map_err(storage)?;
            table
                .insert("meta/clock", now.to_string().as_bytes())
                .map_err(storage)?;
        }
        write.commit().map_err(storage)?;
        Ok(response)
    }
    /// Stream child plans through one atomic receipt transaction; retain one full plan at a time.
    pub fn accept_batch_with_clock(
        &self,
        principal: &str,
        request: &Request,
        plan: &nw_domain::batches::BatchPlan,
        batch: &nw_domain::batches::Batch,
        authorization: &ExecutionAuthorization,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Response> {
        use nw_domain::batches::SubmitBatchInput;
        if request.command.id != "batches.submit" {
            return Err(DomainError::new(
                "INVALID_ACCEPTANCE",
                "This primitive accepts cleanup batches only",
            ));
        }
        self.commit_composed_with_clock(principal, request, clock, |table, now| {
            if batch.creator != principal
                || authorization.principal != principal
                || plan.data_root != self.root_id()
            {
                return Err(DomainError::new(
                    "INVALID_ACCEPTANCE",
                    "Batch acceptance principal/root differs",
                ));
            }
            plan.admits_submission()?;
            batch.validate(plan)?;
            let input: SubmitBatchInput = nw_domain::commands::parse(request.params.clone())?;
            if input.plan_id != plan.id
                || input.plan_hash != plan.hash
                || plan.expires_at.parse::<u64>().unwrap_or(0) <= now
            {
                return Err(DomainError::new(
                    "INVALID_ACCEPTANCE",
                    "Batch acceptance must match the current reviewed plan",
                ));
            }
            let batch_key = format!("batch/{}", batch.id);
            if table.get(batch_key.as_str()).map_err(storage)?.is_some() {
                return Err(DomainError::new(
                    "OBJECT_CONFLICT",
                    "Batch identity already exists",
                ));
            }
            let mut accepted = batch.clone();
            accepted.accepted_at = now.to_string();
            record_quota(table, &batch_key)?;
            table
                .insert(batch_key.as_str(), encode(&accepted)?.as_slice())
                .map_err(storage)?;
            let auth_bytes = encode(authorization)?;
            let mut count = 0usize;
            for (member, item) in accepted.members.iter().zip(&plan.items) {
                let Some(id) = &member.job_id else { continue };
                let reference = item.plan.as_ref().ok_or_else(|| {
                    DomainError::new(
                        "INVALID_ACCEPTANCE",
                        "Rejected candidate cannot start a job",
                    )
                })?;
                let child: nw_domain::records::Plan = {
                    let value = table
                        .get(format!("plan/{}", reference.id).as_str())
                        .map_err(storage)?
                        .ok_or_else(|| {
                            DomainError::new("INVALID_PLAN", "Batch child plan is missing")
                        })?;
                    nw_domain::plans::decode(decode(value.value())?)?
                };
                if child.hash != reference.hash
                    || child.source.id != item.asset_id
                    || child.source.identity.bytes != item.source_bytes
                    || child.source.identity.sha256 != item.source_sha256
                    || Some(&child.resolution.action) != item.action.as_ref()
                    || child.output != item.requested_output
                    || child.output.rsplit(['/', '\\']).next() != Some(item.output_name.as_str())
                    || child.expires_at.parse::<u64>().unwrap_or(0) <= now
                    || nw_domain::plans::validate_requirements(&child)?.data_root != self.root_id()
                {
                    return Err(DomainError::new(
                        "INVALID_ACCEPTANCE",
                        "Child plan differs from its frozen batch outcome",
                    ));
                }
                let key = format!("job/{id}");
                let auth_key = format!("job-auth/{id}");
                if table.get(key.as_str()).map_err(storage)?.is_some()
                    || table.get(auth_key.as_str()).map_err(storage)?.is_some()
                {
                    return Err(DomainError::new(
                        "OBJECT_CONFLICT",
                        "New batch job identity already exists",
                    ));
                }
                let job = Job {
                    remux_progress: None,
                    progress: None,
                    file_times: None,
                    id: id.clone(),
                    creator: principal.into(),
                    plan: child,
                    state: JobState::Queued,
                    prepared_argv: None,
                    prepared_stages: Vec::new(),
                    owned_staging: None,
                    artifact: None,
                    verification: None,
                    error: None,
                };
                record_quota(table, &key)?;
                table
                    .insert(key.as_str(), encode(&job)?.as_slice())
                    .map_err(storage)?;
                table
                    .insert(auth_key.as_str(), auth_bytes.as_slice())
                    .map_err(storage)?;
                count += 1;
            }
            Ok(serde_json::json!({
                "batch_id":accepted.id,"plan_id":accepted.plan_id,"plan_hash":accepted.plan_hash,
                "members":accepted.members,"accepted_jobs":count,"accepted_at":accepted.accepted_at,
                "rejected_candidates":plan.items.iter().filter(|item| item.error.is_some()).count(),
            }))
        })
    }
    /// Cancellation patches and their scoped receipt commit together before tokens are signalled.
    pub fn cancel_batch_with_clock(
        &self,
        principal: &str,
        request: &Request,
        batch: &nw_domain::batches::Batch,
        patches: &[nw_domain::batches::CancelMember],
        result: Value,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Response> {
        if request.command.id != "batches.cancel"
            || patches.len() > nw_domain::batches::MAX_BATCH_ITEMS
        {
            return Err(DomainError::new(
                "INVALID_ACCEPTANCE",
                "Invalid bounded batch cancellation",
            ));
        }
        self.commit_composed_with_clock(principal, request, clock, |table, _now| {
            let input: nw_domain::commands::IdInput =
                nw_domain::commands::parse(request.params.clone())?;
            let stored: nw_domain::batches::Batch = {
                let value = table
                    .get(format!("batch/{}", input.id).as_str())
                    .map_err(storage)?
                    .ok_or_else(DomainError::object_not_found)?;
                decode(value.value())?
            };
            if input.id != batch.id
                || stored.id != batch.id
                || stored.plan_id != batch.plan_id
                || stored.plan_hash != batch.plan_hash
                || stored.members.len() != batch.members.len()
                || !stored
                    .members
                    .iter()
                    .zip(&batch.members)
                    .all(|(a, b)| a.index == b.index && a.job_id == b.job_id)
            {
                return Err(DomainError::new(
                    "INVALID_BATCH",
                    "Batch changed before cancellation",
                ));
            }
            let mut seen = std::collections::BTreeSet::new();
            for patch in patches {
                if !seen.insert(&patch.job_id)
                    || !stored
                        .members
                        .iter()
                        .any(|member| member.job_id.as_deref() == Some(patch.job_id.as_str()))
                {
                    return Err(DomainError::new(
                        "INVALID_BATCH",
                        "Cancellation contains an unknown/duplicate child",
                    ));
                }
                let key = format!("job/{}", patch.job_id);
                let mut job: Job = {
                    let value = table
                        .get(key.as_str())
                        .map_err(storage)?
                        .ok_or_else(DomainError::object_not_found)?;
                    decode_job(value.value())?
                };
                if job.state != patch.expected_state
                    || !matches!(
                        job.state,
                        JobState::Queued | JobState::Running | JobState::Verifying
                    )
                {
                    return Err(DomainError::new(
                        "STALE_JOB_STATE",
                        "A child passed its cancellable boundary",
                    ));
                }
                if job.state == JobState::Queued {
                    job.state = JobState::Cancelled;
                    table
                        .insert(key.as_str(), encode(&job)?.as_slice())
                        .map_err(storage)?;
                }
                let cancel = format!("cancel/{}", job.id);
                table
                    .insert(cancel.as_str(), encode(&true)?.as_slice())
                    .map_err(storage)?;
            }
            Ok(result)
        })
    }
    pub fn update_job(&self, id: &str, update: impl FnOnce(&mut Job) -> Result<()>) -> Result<Job> {
        let write = self.db.begin_write().map_err(storage)?;
        let job;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let key = format!("job/{id}");
            let mut current: Job = {
                let value = table
                    .get(key.as_str())
                    .map_err(storage)?
                    .ok_or_else(DomainError::object_not_found)?;
                decode_job(value.value())?
            };
            if current.state.terminal() {
                return Err(DomainError::new(
                    "INVALID_JOB_TRANSITION",
                    "Terminal job facts are immutable",
                ));
            }
            let immutable = (&current.id, &current.creator, &current.plan.hash);
            let identity = (
                immutable.0.clone(),
                immutable.1.clone(),
                immutable.2.clone(),
            );
            update(&mut current)?;
            if identity
                != (
                    current.id.clone(),
                    current.creator.clone(),
                    current.plan.hash.clone(),
                )
            {
                return Err(DomainError::new(
                    "INVALID_JOB_TRANSITION",
                    "Job identity/creator/plan cannot change",
                ));
            }
            if current.state == JobState::Succeeded
                && (current.artifact.is_none() || current.verification.is_none())
            {
                return Err(DomainError::new(
                    "VERIFICATION_REQUIRED",
                    "Published artifact and verification facts are required",
                ));
            }
            table
                .insert(key.as_str(), encode(&current)?.as_slice())
                .map_err(storage)?;
            job = current;
        }
        write.commit().map_err(storage)?;
        Ok(job)
    }
    pub fn execution_allowed(&self, id: &str) -> Result<()> {
        let authorization: ExecutionAuthorization =
            self.get(&format!("job-auth/{id}"))?.ok_or_else(|| {
                DomainError::new(
                    "PERMISSION_DENIED",
                    "Durable execution authorization missing",
                )
            })?;
        let job = self.inspect_job(id)?;
        if job.plan.schema_version == 2
            && nw_domain::plans::validate_requirements(&job.plan)?.data_root != self.root_id()
        {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Accepted plan belongs to another data root",
            ));
        }
        if let Some(name) = &authorization.profile {
            let profile: AccessProfile =
                self.get(&format!("profile/{name}"))?.ok_or_else(|| {
                    DomainError::new("PERMISSION_DENIED", "Execution profile removed")
                })?;
            execution_policy(&profile, &authorization, &job)?;
        }
        Ok(())
    }
    pub fn publication_fence(&self, publication: &Publication) -> Result<()> {
        let write = self.db.begin_write().map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let id = &publication.job_id;
            let key = format!("job/{id}");
            let mut job: Job = {
                let value = table
                    .get(key.as_str())
                    .map_err(storage)?
                    .ok_or_else(DomainError::object_not_found)?;
                decode_job(value.value())?
            };
            if job.plan.schema_version == 2
                && nw_domain::plans::validate_requirements(&job.plan)?.data_root != self.root_id()
            {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Publication plan belongs to another data root",
                ));
            }
            if job.state != JobState::Verifying {
                return Err(DomainError::new(
                    "STALE_JOB_STATE",
                    "Publication requires verified execution",
                ));
            }
            if table
                .get(format!("cancel/{id}").as_str())
                .map_err(storage)?
                .is_some()
            {
                return Err(DomainError::new(
                    "CANCELLED",
                    "Cancellation committed before publication fence",
                ));
            }
            let authorization: ExecutionAuthorization = {
                let value = table
                    .get(format!("job-auth/{id}").as_str())
                    .map_err(storage)?
                    .ok_or_else(|| {
                        DomainError::new("PERMISSION_DENIED", "Execution authorization missing")
                    })?;
                decode(value.value())?
            };
            if let Some(name) = &authorization.profile {
                let profile: AccessProfile = {
                    let value = table
                        .get(format!("profile/{name}").as_str())
                        .map_err(storage)?
                        .ok_or_else(|| {
                            DomainError::new("PERMISSION_DENIED", "Execution profile missing")
                        })?;
                    decode(value.value())?
                };
                execution_policy(&profile, &authorization, &job)?;
            }
            job.state = JobState::Publishing;
            table
                .insert(key.as_str(), encode(&job)?.as_slice())
                .map_err(storage)?;
            table
                .insert(
                    format!("publication/{id}").as_str(),
                    encode(publication)?.as_slice(),
                )
                .map_err(storage)?;
        }
        write.commit().map_err(storage)
    }
    pub fn published(&self, id: &str, artifact: FileIdentity, verification: String) -> Result<Job> {
        let write = self.db.begin_write().map_err(storage)?;
        let job;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            let key = format!("job/{id}");
            let mut current: Job = {
                let value = table
                    .get(key.as_str())
                    .map_err(storage)?
                    .ok_or_else(DomainError::object_not_found)?;
                decode_job(value.value())?
            };
            if current.state != JobState::Publishing {
                return Err(DomainError::new(
                    "STALE_JOB_STATE",
                    "Published result requires a durable publication fence",
                ));
            }
            let publication_key = format!("publication/{id}");
            let mut publication: Publication = {
                let value = table
                    .get(publication_key.as_str())
                    .map_err(storage)?
                    .ok_or_else(|| {
                        DomainError::new("VERIFICATION_REQUIRED", "Publication journal missing")
                    })?;
                decode(value.value())?
            };
            if publication.output != artifact.path
                || publication.identity.bytes != artifact.bytes
                || publication.identity.sha256 != artifact.sha256
            {
                return Err(DomainError::new(
                    "VERIFICATION_FAILED",
                    "Published identity differs from verified staging",
                ));
            }
            current.state = JobState::Succeeded;
            current.artifact = Some(artifact);
            current.verification = Some(verification);
            publication.published = true;
            table
                .insert(key.as_str(), encode(&current)?.as_slice())
                .map_err(storage)?;
            table
                .insert(publication_key.as_str(), encode(&publication)?.as_slice())
                .map_err(storage)?;
            job = current;
        }
        write.commit().map_err(storage)?;
        Ok(job)
    }
}
