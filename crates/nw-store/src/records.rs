//! Bounded records, replay-first acceptance and atomic execution journals.
use super::*;
use nw_domain::{
    access::{AccessProfile, ExecutionAuthorization, Publication},
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
fn execution_policy(
    profile: &AccessProfile,
    authorization: &ExecutionAuthorization,
    job: &Job,
) -> Result<()> {
    use std::path::Path;
    let source = &job.plan.source;
    if !profile.enabled
        || profile.principal != authorization.principal
        || !profile.commands.contains("jobs.submit")
        || (source.creator != authorization.principal && !profile.objects.contains(&source.id))
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
impl Store {
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
        let write = self.db.begin_write().map_err(storage)?;
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
        let write = self.db.begin_write().map_err(storage)?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            for (key, value) in records {
                valid_key(key)?;
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
        let (key, _) = self.replay_key(principal, request, now)?;
        let read = self.db.begin_read().map_err(storage)?;
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
        request.validate()?;
        if records.len() > 64 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Mutation record budget exceeded",
            ));
        }
        let (key, scope) = self.replay_key(principal, request, now)?;
        let digest = request.structural_digest();
        let write = self.db.begin_write().map_err(storage)?;
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
            response = {
                let mut response = Response::success(&request.request_id, result);
                response.receipt_id = Some(Uuid::new_v4().to_string());
                response
            };
            if serde_json::to_vec(&response).map_err(storage)?.len() > nw_domain::commands::MAX_RESULT_BYTES {
                return Err(DomainError::new("RESOURCE_LIMIT", "Mutation result exceeds its admitted response budget before acceptance"));
            }
            for (record_key, value) in records {
                valid_key(record_key)?;
                table
                    .insert(record_key.as_str(), encode(value)?.as_slice())
                    .map_err(storage)?;
            }
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
                    .ok_or_else(|| DomainError::new("OBJECT_NOT_FOUND", "Job does not exist"))?;
                decode(value.value())?
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
        if let Some(name) = &authorization.profile {
            let profile: AccessProfile =
                self.get(&format!("profile/{name}"))?.ok_or_else(|| {
                    DomainError::new("PERMISSION_DENIED", "Execution profile removed")
                })?;
            let job = self.inspect_job(id)?;
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
                let value = table.get(key.as_str()).map_err(storage)?.ok_or_else(|| {
                    DomainError::new("OBJECT_NOT_FOUND", "Job missing at publication fence")
                })?;
                decode(value.value())?
            };
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
                let value = table.get(key.as_str()).map_err(storage)?.ok_or_else(|| {
                    DomainError::new("OBJECT_NOT_FOUND", "Job missing after publication")
                })?;
                decode(value.value())?
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
