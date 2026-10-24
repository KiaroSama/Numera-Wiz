//! Single-writer, bounded durable acceptance primitives. Policy stays in the service.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use nw_domain::{
    DomainError, Result,
    commands::{Request, Response, SubmitInput},
    records::{Job, JobState},
};
use nw_platform::files::{RootLock, private_root};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;
const RECORDS: TableDefinition<&str, &[u8]> = TableDefinition::new("versioned_records");
// Older readers must refuse records whose timestamp policies/facts they could discard.
const STORE_SCHEMA: &[u8] = b"2";
const MAX_RECORD: usize = 8 * 1024 * 1024;
const SCOPE_TTL: u64 = 7 * 24 * 3600;
mod profiles;
mod records;
fn storage(error: impl std::fmt::Display) -> DomainError {
    DomainError::new("STORAGE_ERROR", error.to_string())
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(storage)?;
    if bytes.len() > MAX_RECORD {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Durable record exceeds 8 MiB",
        ));
    }
    Ok(bytes)
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > MAX_RECORD {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Stored record exceeds its admitted budget",
        ));
    }
    serde_json::from_slice(bytes).map_err(storage)
}
#[derive(Serialize, Deserialize)]
struct Scope {
    id: String,
    principal: String,
    root: String,
    issued_at: String,
    expires_at: String,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    digest: String,
    response: Response,
    #[serde(default)]
    expires_at: u64,
}
pub struct Store {
    db: Database,
    _lock: RootLock,
    root_id: String,
    signing_key: Vec<u8>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let root = private_root(path)?;
        let lock = RootLock::acquire(&root)?;
        let file = root.join("store.redb");
        let fresh = !file.try_exists().map_err(storage)?;
        let db = Database::builder()
            .set_cache_size(8 * 1024 * 1024)
            .create(file)
            .map_err(storage)?;
        if fresh {
            let write = db.begin_write().map_err(storage)?;
            {
                let mut table = write.open_table(RECORDS).map_err(storage)?;
                let root_id = Uuid::new_v4().to_string();
                let mut key = Uuid::new_v4().as_bytes().to_vec();
                key.extend_from_slice(Uuid::new_v4().as_bytes());
                table.insert("meta/schema", STORE_SCHEMA).map_err(storage)?;
                table
                    .insert("meta/root", root_id.as_bytes())
                    .map_err(storage)?;
                table
                    .insert("meta/signing-key", key.as_slice())
                    .map_err(storage)?;
                table
                    .insert("meta/clock", b"0".as_slice())
                    .map_err(storage)?;
            }
            write.commit().map_err(storage)?;
        }
        let read = db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(|_| {
            DomainError::new(
                "STORE_SCHEMA_UNSUPPORTED",
                "Existing store has no recognized metadata; refusing empty reinitialization",
            )
        })?;
        if table
            .get("meta/schema")
            .map_err(storage)?
            .map(|v| v.value().to_vec())
            != Some(STORE_SCHEMA.to_vec())
        {
            return Err(DomainError::new(
                "STORE_SCHEMA_UNSUPPORTED",
                "Store schema is not supported; no replacement generation was initialized",
            ));
        }
        let root_id = String::from_utf8(
            table
                .get("meta/root")
                .map_err(storage)?
                .ok_or_else(|| storage("Root identity missing"))?
                .value()
                .to_vec(),
        )
        .map_err(storage)?;
        let signing_key = table
            .get("meta/signing-key")
            .map_err(storage)?
            .ok_or_else(|| storage("Scope signing key missing"))?
            .value()
            .to_vec();
        drop(table);
        drop(read);
        if signing_key.len() != 32 {
            return Err(storage("Invalid stored signing-key length"));
        }
        let store = Self {
            db,
            _lock: lock,
            root_id,
            signing_key,
        };
        store.upgrade_access_profiles()?;
        Ok(store)
    }
    pub fn root_id(&self) -> &str {
        &self.root_id
    }
    pub fn issue_scope(&self, principal: &str, now: u64) -> Result<String> {
        self.issue_scope_with_clock(principal, || Ok(now))
    }
    pub fn issue_scope_with_clock(
        &self,
        principal: &str,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<String> {
        if principal.is_empty() || principal.len() > 128 {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Invalid authenticated principal identity",
            ));
        }
        let write = self.db.begin_write().map_err(storage)?;
        let now = clock()?;
        let expires = now.checked_add(SCOPE_TTL).ok_or_else(|| {
            DomainError::new("VALIDATION_ERROR", "Clock exceeds supported scope range")
        })?;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            check_clock(&table, now)?;
            let quota_key = format!(
                "scope-rate/{}",
                hex::encode(Sha256::digest(principal.as_bytes()))
            );
            let current: Option<(u64, u32)> = table
                .get(quota_key.as_str())
                .map_err(storage)?
                .map(|value| decode(value.value()))
                .transpose()?;
            let window = now / 3600;
            let count = current
                .filter(|(previous, _)| *previous == window)
                .map_or(0, |(_, count)| count);
            if count >= 32 {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Replay-scope bootstrap is limited to 32 scopes per principal per hour",
                ));
            }
            table
                .insert(quota_key.as_str(), encode(&(window, count + 1))?.as_slice())
                .map_err(storage)?;
            table
                .insert("meta/clock", now.to_string().as_bytes())
                .map_err(storage)?;
        }
        write.commit().map_err(storage)?;
        let payload = encode(&Scope {
            id: Uuid::new_v4().to_string(),
            principal: principal.into(),
            root: self.root_id.clone(),
            issued_at: now.to_string(),
            expires_at: expires.to_string(),
        })?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing_key).map_err(storage)?;
        mac.update(&payload);
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        ))
    }
    fn verify_scope(&self, token: &str, principal: &str, now: u64) -> Result<Scope> {
        let invalid = || {
            DomainError::new(
                "REPLAY_SCOPE_INVALID",
                "Replay scope does not match the authenticated principal/root/signature",
            )
        };
        if token.len() > 2048 {
            return Err(invalid());
        }
        let (payload, signature) = token.split_once('.').ok_or_else(invalid)?;
        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| invalid())?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing_key).map_err(|_| invalid())?;
        mac.update(&payload);
        mac.verify_slice(&signature).map_err(|_| invalid())?;
        let scope: Scope = decode(&payload).map_err(|_| invalid())?;
        if scope.root != self.root_id || scope.principal != principal {
            return Err(invalid());
        }
        let expires = scope.expires_at.parse::<u64>().map_err(|_| invalid())?;
        let issued = scope.issued_at.parse::<u64>().map_err(|_| invalid())?;
        if now < issued {
            return Err(DomainError::new(
                "REPLAY_SCOPE_INVALID",
                "Clock rollback detected; scope acceptance is blocked",
            ));
        }
        if now >= expires {
            return Err(DomainError::new(
                "REPLAY_SCOPE_EXPIRED",
                "Expired scopes never authorize fresh execution",
            ));
        }
        Ok(scope)
    }
    /// Commit the queued job and immutable receipt together; replay precedes target validation.
    pub fn accept_job(
        &self,
        principal: &str,
        request: &Request,
        job: &Job,
        now: u64,
    ) -> Result<Response> {
        request.validate()?;
        if request.command.id != "jobs.submit" {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "This acceptance primitive owns jobs.submit only",
            ));
        }
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
            .filter(|k| !k.is_empty() && k.len() <= 128)
            .ok_or_else(|| {
                DomainError::new("VALIDATION_ERROR", "A bounded idempotency key is required")
            })?;
        let receipt_key = format!(
            "receipt/{}",
            hex::encode(Sha256::digest(encode(&(
                principal,
                scope.id,
                &request.command.id,
                key
            ))?))
        );
        let digest = request.structural_digest();
        let write = self.db.begin_write().map_err(storage)?;
        let response;
        {
            let mut table = write.open_table(RECORDS).map_err(storage)?;
            check_clock(&table, now)?;
            if let Some(bytes) = table.get(receipt_key.as_str()).map_err(storage)? {
                let receipt: Receipt = decode(bytes.value())?;
                if receipt.digest != digest {
                    return Err(DomainError::new(
                        "IDEMPOTENCY_CONFLICT",
                        "The same scope/key names a different structural request",
                    ));
                }
                let mut replay = receipt.response;
                replay.request_id = request.request_id.clone();
                tracing::debug!(
                    "Returning the existing durable receipt without revalidating its old target"
                );
                return Ok(replay);
            }
            let params: SubmitInput = nw_domain::commands::parse(request.params.clone())?;
            if job.creator != principal
                || job.state != JobState::Queued
                || params.plan_id != job.plan.id
                || params.plan_hash != job.plan.hash
            {
                return Err(DomainError::new(
                    "INVALID_ACCEPTANCE",
                    "New job must match the accepting principal and frozen queued plan",
                ));
            }
            let job_key = format!("job/{}", job.id);
            if table.get(job_key.as_str()).map_err(storage)?.is_some() {
                return Err(DomainError::new(
                    "OBJECT_CONFLICT",
                    "Job identity already exists",
                ));
            }
            response = {
                let mut value = Response::success(
                    &request.request_id,
                    serde_json::json!({"job_id":job.id,"state":"queued"}),
                );
                value.receipt_id = Some(Uuid::new_v4().to_string());
                value
            };
            table
                .insert(job_key.as_str(), encode(job)?.as_slice())
                .map_err(storage)?;
            table
                .insert(
                    receipt_key.as_str(),
                    encode(&Receipt {
                        digest,
                        response: response.clone(),
                        expires_at: scope.expires_at.parse().map_err(storage)?,
                    })?
                    .as_slice(),
                )
                .map_err(storage)?;
            table
                .insert("meta/clock", now.to_string().as_bytes())
                .map_err(storage)?;
        }
        write.commit().map_err(storage)?;
        Ok(response)
    }
    pub fn inspect_job(&self, id: &str) -> Result<Job> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let key = format!("job/{id}");
        let value = table
            .get(key.as_str())
            .map_err(storage)?
            .ok_or_else(DomainError::object_not_found)?;
        records::decode_job(value.value())
    }
    pub fn transition_job(&self, id: &str, expected: JobState, next: JobState) -> Result<Job> {
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
                records::decode_job(value.value())?
            };
            if current.state != expected {
                return Err(DomainError::new(
                    "STALE_JOB_STATE",
                    "Job state changed before the guarded transition",
                ));
            }
            let valid = matches!(
                (&current.state, &next),
                (JobState::Queued, JobState::Running | JobState::Cancelled)
                    | (
                        JobState::Running,
                        JobState::Verifying
                            | JobState::Failed
                            | JobState::Cancelled
                            | JobState::Interrupted
                    )
                    | (
                        JobState::Verifying,
                        JobState::Publishing
                            | JobState::Failed
                            | JobState::Cancelled
                            | JobState::Interrupted
                    )
                    | (JobState::Publishing, JobState::Succeeded | JobState::Failed)
            );
            if !valid {
                return Err(DomainError::new(
                    if current.state == JobState::Publishing {
                        "CANCELLATION_TOO_LATE"
                    } else {
                        "INVALID_JOB_TRANSITION"
                    },
                    "Terminal facts are immutable; a retry requires a new linked job",
                ));
            }
            if next == JobState::Succeeded
                && (current.artifact.is_none() || current.verification.is_none())
            {
                return Err(DomainError::new(
                    "VERIFICATION_REQUIRED",
                    "A terminal success requires verified publication facts",
                ));
            }
            current.state = next;
            table
                .insert(key.as_str(), encode(&current)?.as_slice())
                .map_err(storage)?;
            job = current;
        }
        write.commit().map_err(storage)?;
        Ok(job)
    }
}
fn check_clock(table: &impl ReadableTable<&'static str, &'static [u8]>, now: u64) -> Result<()> {
    let previous = table
        .get("meta/clock")
        .map_err(storage)?
        .ok_or_else(|| storage("Clock guard missing"))?;
    let previous = std::str::from_utf8(previous.value())
        .map_err(storage)?
        .parse::<u64>()
        .map_err(storage)?;
    if now < previous {
        return Err(DomainError::new(
            "REPLAY_SCOPE_INVALID",
            "Clock rollback detected; durable acceptance is blocked",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_domain::{
        commands::CommandRef,
        mux::{CleanAction, CleanResolution, Preservation},
        records::{Asset, FileIdentity, MediaProbe, Plan},
    };
    #[test]
    fn scope_issuance_samples_time_after_waiting_for_the_writer() {
        use std::sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
            mpsc,
        };
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let writer = store.db.begin_write().unwrap();
        let wall = Arc::new(AtomicU64::new(40));
        let source = wall.clone();
        let issuer = store.clone();
        let (started, ready) = mpsc::channel();
        let (sampled, observed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            issuer.issue_scope_with_clock("writer-time-fixture", || {
                let time = source.load(Ordering::SeqCst);
                sampled.send(time).unwrap();
                Ok(time)
            })
        });
        ready
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            observed.recv_timeout(std::time::Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        wall.store(80, Ordering::SeqCst);
        drop(writer);
        assert_eq!(
            observed
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            80
        );
        let token = worker.join().unwrap().unwrap();
        assert_eq!(
            store
                .verify_scope(&token, "writer-time-fixture", 80)
                .unwrap()
                .issued_at,
            "80"
        );
        assert!(
            store
                .verify_scope(&token, "writer-time-fixture", 79)
                .is_err()
        );
    }
    fn job() -> Job {
        let identity = FileIdentity {
            path: "synthetic-unit-fixture".into(),
            bytes: "1".into(),
            sha256: "00".repeat(32),
        };
        let asset = Asset {
            source_object: None,
            display_name: String::new(),
            id: "test-asset".into(),
            creator: "alice".into(),
            identity: identity.clone(),
            probe: MediaProbe {
                probe_timeout: None,
                stream_details: Default::default(),
                streams: vec![],
                format: "unit-fixture".into(),
                global_tags: Default::default(),
                chapters: serde_json::json!([]),
                verification_scope: "unit-only".into(),
            },
        };
        let resolution = CleanResolution {
            action: CleanAction::CopyUnchanged,
            streams: vec![],
            outcomes: vec![],
            reasons: vec![],
            warnings: vec![],
            preservation: Preservation::default(),
        };
        let plan = Plan {
            execution_limits: None,
            file_times: None,
            schema_version: 1,
            required_rights: None,
            creation_audit: None,
            id: "test-plan".into(),
            creator: "alice".into(),
            hash: "unit-plan-hash".into(),
            source: asset,
            output: "unit-output".into(),
            ffmpeg: identity.clone(),
            ffprobe: identity,
            resolution,
            argv_template: vec![],
            created_at: "100".into(),
            expires_at: "1000".into(),
            verification_scope: "unit-only".into(),
        };
        Job {
            remux_progress: None,
            progress: None,
            file_times: None,
            id: "test-job".into(),
            creator: "alice".into(),
            plan,
            state: JobState::Queued,
            prepared_argv: None,
            prepared_stages: Vec::new(),
            owned_staging: None,
            artifact: None,
            verification: None,
            error: None,
        }
    }
    fn request(scope: String) -> Request {
        Request {
            api_version: 1,
            request_id: "first".into(),
            command: CommandRef {
                id: "jobs.submit".into(),
                version: 1,
            },
            project: None,
            replay_scope: Some(scope),
            idempotency_key: Some("unit-key".into()),
            params: serde_json::json!({"plan_id":"test-plan","plan_hash":"unit-plan-hash"}),
        }
    }
    // Synthetic queue facts exercise storage only; these are never media/runtime evidence.
    fn batch_fixture(
        store: &Store,
        count: usize,
        persist: usize,
    ) -> (
        nw_domain::batches::BatchPlan,
        nw_domain::batches::Batch,
        Vec<Plan>,
        Request,
        nw_domain::access::ExecutionAuthorization,
    ) {
        use nw_domain::batches::*;
        let mut children = Vec::new();
        let mut items = Vec::new();
        for index in 0..count {
            let mut plan = job().plan;
            plan.schema_version = 2;
            plan.id = format!("child-{index}");
            plan.source.id = format!("asset-{index}");
            plan.source.identity.path = format!("/synthetic/source-{index}.mkv");
            plan.source.identity.sha256 = format!("{index:064x}");
            plan.output = format!("/synthetic/output-{index}.mkv");
            let mut rights = nw_domain::plans::RequiredRights::native(
                plan.source.identity.clone(),
                plan.output.clone(),
            );
            rights.data_root = store.root_id().into();
            plan.required_rights = Some(rights);
            plan.creation_audit = Some(nw_domain::plans::CreationAudit {
                principal: "alice".into(),
                policy_revision: 0,
            });
            plan.hash = nw_domain::plans::digest(&plan).unwrap();
            if index < persist {
                store.put(&format!("plan/{}", plan.id), &plan).unwrap();
            }
            items.push(PlannedItem {
                index: index as u32,
                asset_id: plan.source.id.clone(),
                source_sha256: plan.source.identity.sha256.clone(),
                source_bytes: plan.source.identity.bytes.clone(),
                output_name: format!("output-{index}.mkv"),
                requested_output: plan.output.clone(),
                plan: Some(PlanReference {
                    id: plan.id.clone(),
                    hash: plan.hash.clone(),
                }),
                action: Some(plan.resolution.action.clone()),
                error: None,
            });
            children.push(plan);
        }
        let mut plan = BatchPlan {
            schema_version: 1,
            id: "batch-plan".into(),
            creator: "alice".into(),
            data_root: store.root_id().into(),
            hash: String::new(),
            created_at: "100".into(),
            expires_at: "1000".into(),
            failure_policy: FailurePolicy::RejectAll,
            unresolved_policy: UnresolvedPolicy::Reject,
            unresolved: Vec::new(),
            items,
        };
        plan.hash = plan.digest().unwrap();
        store.put("batch-plan/batch-plan", &plan).unwrap();
        let batch = Batch {
            schema_version: 1,
            id: "batch".into(),
            creator: "alice".into(),
            plan_id: plan.id.clone(),
            plan_hash: plan.hash.clone(),
            accepted_at: "99999".into(),
            members: (0..count)
                .map(|index| BatchMember {
                    index: index as u32,
                    job_id: Some(format!("batch-job-{index}")),
                })
                .collect(),
        };
        let mut request = Request::query(
            "batches.submit",
            serde_json::json!({"plan_id":plan.id,"plan_hash":plan.hash}),
        );
        request.replay_scope = Some(store.issue_scope("alice", 100).unwrap());
        request.idempotency_key = Some("accept-batch".into());
        let auth = nw_domain::access::ExecutionAuthorization {
            principal: "alice".into(),
            profile: None,
            revision: 0,
        };
        (plan, batch, children, request, auth)
    }
    #[test]
    fn full_bounded_batch_and_receipt_commit_together_and_replay_ignores_new_targets() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let (plan, batch, _children, request, auth) = batch_fixture(&store, 64, 64);
        let accepted = store
            .accept_batch_with_clock("alice", &request, &plan, &batch, &auth, || Ok(102))
            .unwrap();
        assert_eq!(accepted.result.as_ref().unwrap()["accepted_jobs"], 64);
        assert_eq!(accepted.result.as_ref().unwrap()["accepted_at"], "102");
        let stored: nw_domain::batches::Batch = store.get("batch/batch").unwrap().unwrap();
        assert_eq!(
            stored.accepted_at, "102",
            "The writer admission clock replaces caller audit time"
        );
        assert_eq!(store.list::<Job>("job/", 128).unwrap().len(), 64);
        for index in 0..64 {
            assert_eq!(
                store
                    .inspect_job(&format!("batch-job-{index}"))
                    .unwrap()
                    .state,
                JobState::Queued
            );
            assert!(
                store
                    .get::<nw_domain::access::ExecutionAuthorization>(&format!(
                        "job-auth/batch-job-{index}"
                    ))
                    .unwrap()
                    .is_some()
            );
        }
        drop(store);
        let store = Store::open(root.path()).unwrap();
        let mut invalid = plan.clone();
        invalid.hash = "not-a-valid-new-plan".into();
        let mut retry = request.clone();
        retry.request_id = "separate-process-retry".into();
        let replay = store
            .accept_batch_with_clock("alice", &retry, &invalid, &batch, &auth, || Ok(103))
            .unwrap();
        assert_eq!(replay.receipt_id, accepted.receipt_id);
        assert_eq!(replay.request_id, "separate-process-retry");
        assert_eq!(store.list::<Job>("job/", 128).unwrap().len(), 64);
        retry.params["plan_hash"] = "changed".into();
        assert_eq!(
            store
                .accept_batch_with_clock("alice", &retry, &plan, &batch, &auth, || Ok(103))
                .unwrap_err()
                .code,
            "IDEMPOTENCY_CONFLICT"
        );
        let mut ordinary = Request::query(
            "assets.register",
            serde_json::json!({"path":"/synthetic/source"}),
        );
        ordinary.replay_scope = request.replay_scope.clone();
        ordinary.idempotency_key = Some("ordinary-budget".into());
        let records = (0..65)
            .map(|index| (format!("unit/budget-{index}"), serde_json::json!(true)))
            .collect::<Vec<_>>();
        assert_eq!(
            store
                .commit_with_clock(
                    "alice",
                    &ordinary,
                    || Ok(104),
                    serde_json::json!({}),
                    &records
                )
                .unwrap_err()
                .code,
            "RESOURCE_LIMIT"
        );
    }
    #[test]
    fn late_child_failure_rolls_back_every_job_group_and_receipt_before_a_safe_retry() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let (plan, batch, children, request, auth) = batch_fixture(&store, 2, 1);
        assert_eq!(
            store
                .accept_batch_with_clock("alice", &request, &plan, &batch, &auth, || Ok(101))
                .unwrap_err()
                .code,
            "INVALID_PLAN"
        );
        assert!(
            store
                .get::<nw_domain::batches::Batch>("batch/batch")
                .unwrap()
                .is_none()
        );
        assert!(store.list::<Job>("job/", 8).unwrap().is_empty());
        assert!(store.replay("alice", &request, 101).unwrap().is_none());
        store
            .put(&format!("plan/{}", children[1].id), &children[1])
            .unwrap();
        let accepted = store
            .accept_batch_with_clock("alice", &request, &plan, &batch, &auth, || Ok(102))
            .unwrap();
        assert!(accepted.ok);
        assert_eq!(store.list::<Job>("job/", 8).unwrap().len(), 2);
    }
    #[test]
    fn batch_cancellation_journals_requests_without_claiming_a_running_writer_stopped() {
        use nw_domain::batches::CancelMember;
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let (plan, batch, _children, accepted, auth) = batch_fixture(&store, 2, 2);
        store
            .accept_batch_with_clock("alice", &accepted, &plan, &batch, &auth, || Ok(101))
            .unwrap();
        store
            .transition_job("batch-job-0", JobState::Queued, JobState::Running)
            .unwrap();
        let mut request = Request::query("batches.cancel", serde_json::json!({"id":batch.id}));
        request.replay_scope = accepted.replay_scope.clone();
        request.idempotency_key = Some("cancel-batch".into());
        let patches = [
            CancelMember {
                job_id: "batch-job-0".into(),
                expected_state: JobState::Running,
            },
            CancelMember {
                job_id: "batch-job-1".into(),
                expected_state: JobState::Queued,
            },
        ];
        let result = serde_json::json!({"batch_id":batch.id});
        let response = store
            .cancel_batch_with_clock("alice", &request, &batch, &patches, result.clone(), || {
                Ok(102)
            })
            .unwrap();
        assert_eq!(
            store.inspect_job("batch-job-0").unwrap().state,
            JobState::Running
        );
        assert_eq!(
            store.inspect_job("batch-job-1").unwrap().state,
            JobState::Cancelled
        );
        for id in ["batch-job-0", "batch-job-1"] {
            assert_eq!(
                store.get::<bool>(&format!("cancel/{id}")).unwrap(),
                Some(true)
            );
        }
        let replay = store
            .cancel_batch_with_clock("alice", &request, &batch, &patches, result.clone(), || {
                Ok(103)
            })
            .unwrap();
        assert_eq!(replay.receipt_id, response.receipt_id);
        request.idempotency_key = Some("stale-patch".into());
        let wrong = [CancelMember {
            job_id: "batch-job-1".into(),
            expected_state: JobState::Queued,
        }];
        assert_eq!(
            store
                .cancel_batch_with_clock("alice", &request, &batch, &wrong, result, || Ok(103))
                .unwrap_err()
                .code,
            "STALE_JOB_STATE"
        );
        assert!(store.replay("alice", &request, 103).unwrap().is_none());
    }
    #[test]
    fn receipt_and_queued_job_survive_restart_and_replay_precedes_target_validation() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let identity = store.root_id().to_owned();
        let req = request(store.issue_scope("alice", 100).unwrap());
        let accepted = store.accept_job("alice", &req, &job(), 100).unwrap();
        drop(store);
        let store = Store::open(root.path()).unwrap();
        assert_eq!(store.root_id(), identity);
        assert_eq!(
            store.inspect_job("test-job").unwrap().state,
            JobState::Queued
        );
        let mut obsolete = job();
        obsolete.creator = "invalid-new-target".into();
        let mut retry = req.clone();
        retry.request_id = "lost-response-retry".into();
        let replay = store.accept_job("alice", &retry, &obsolete, 101).unwrap();
        assert_eq!(replay.receipt_id, accepted.receipt_id);
        assert_eq!(replay.request_id, "lost-response-retry");
        retry.params["plan_hash"] = "different-meaning".into();
        assert_eq!(
            store
                .accept_job("alice", &retry, &job(), 101)
                .unwrap_err()
                .code,
            "IDEMPOTENCY_CONFLICT"
        );
    }
    #[test]
    fn scope_is_bound_to_principal_root_expiry_and_monotonic_clock() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let req = request(store.issue_scope("alice", 100).unwrap());
        assert_eq!(
            store.accept_job("bob", &req, &job(), 100).unwrap_err().code,
            "REPLAY_SCOPE_INVALID"
        );
        assert_eq!(
            store
                .accept_job("alice", &req, &job(), 100 + SCOPE_TTL)
                .unwrap_err()
                .code,
            "REPLAY_SCOPE_EXPIRED"
        );
        let _new = store.issue_scope("alice", 200).unwrap();
        assert_eq!(
            store
                .accept_job("alice", &req, &job(), 150)
                .unwrap_err()
                .code,
            "REPLAY_SCOPE_INVALID"
        );
        let other = tempfile::tempdir().unwrap();
        let other = Store::open(other.path()).unwrap();
        assert_eq!(
            other
                .accept_job("alice", &req, &job(), 200)
                .unwrap_err()
                .code,
            "REPLAY_SCOPE_INVALID"
        );
    }
    #[test]
    fn single_owner_and_terminal_state_guards_are_enforced() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        assert!(Store::open(root.path()).is_err());
        let req = request(store.issue_scope("alice", 100).unwrap());
        store.accept_job("alice", &req, &job(), 100).unwrap();
        store
            .transition_job("test-job", JobState::Queued, JobState::Cancelled)
            .unwrap();
        assert_eq!(
            store
                .transition_job("test-job", JobState::Cancelled, JobState::Running)
                .unwrap_err()
                .code,
            "INVALID_JOB_TRANSITION"
        );
        assert_eq!(
            store.inspect_job("test-job").unwrap().state,
            JobState::Cancelled
        );
    }
    #[test]
    fn version_one_roots_are_preserved_for_an_explicit_generation_migration() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let identity = store.root_id().to_owned();
        store
            .put(
                "migration/sentinel",
                &serde_json::json!({"retained":"original"}),
            )
            .unwrap();
        let write = store.db.begin_write().unwrap();
        {
            let mut table = write.open_table(RECORDS).unwrap();
            table.insert("meta/schema", b"1".as_slice()).unwrap();
        }
        write.commit().unwrap();
        drop(store);
        match Store::open(root.path()) {
            Err(error) => assert_eq!(error.code, "STORE_SCHEMA_UNSUPPORTED"),
            Ok(_) => panic!("A prior store generation was implicitly rewritten"),
        }
        let db = Database::create(root.path().join("store.redb")).unwrap();
        let read = db.begin_read().unwrap();
        let table = read.open_table(RECORDS).unwrap();
        assert_eq!(table.get("meta/schema").unwrap().unwrap().value(), b"1");
        assert_eq!(
            table.get("meta/root").unwrap().unwrap().value(),
            identity.as_bytes()
        );
        let raw = table.get("migration/sentinel").unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_slice(raw.value()).unwrap();
        assert_eq!(value["retained"], "original");
    }
    #[test]
    fn unsupported_store_schema_is_never_reinitialized() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let write = store.db.begin_write().unwrap();
        {
            let mut table = write.open_table(RECORDS).unwrap();
            table
                .insert("meta/schema", b"future-version".as_slice())
                .unwrap();
        }
        write.commit().unwrap();
        drop(store);
        match Store::open(root.path()) {
            Err(error) => assert_eq!(error.code, "STORE_SCHEMA_UNSUPPORTED"),
            Ok(_) => panic!("Future schema was wrongly initialized"),
        };
        let db = Database::create(root.path().join("store.redb")).unwrap();
        let read = db.begin_read().unwrap();
        let table = read.open_table(RECORDS).unwrap();
        assert_eq!(
            table.get("meta/schema").unwrap().unwrap().value(),
            b"future-version"
        );
    }
    #[test]
    fn additive_policy_revisions_preserve_accepted_work_but_revocation_blocks_the_atomic_fence() {
        use nw_domain::access::{AccessProfile, ExecutionAuthorization, Publication};
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let path = root.path().to_str().unwrap().to_owned();
        let mut job = job();
        job.plan.source.identity.path = root.path().join("source.mkv").to_str().unwrap().into();
        job.plan.output = root.path().join("result.mkv").to_str().unwrap().into();
        job.state = JobState::Verifying;
        let auth = ExecutionAuthorization {
            principal: "alice".into(),
            profile: Some("operator".into()),
            revision: 1,
        };
        let mut policy = AccessProfile {
            schema_version: 2,
            id: "operator".into(),
            name: "operator".into(),
            principal: "alice".into(),
            revision: 1,
            enabled: true,
            client_sessions: false,
            mode: nw_domain::access::ProfileMode::Operate,
            owned_object_access: true,
            allowed_command_ids: std::collections::BTreeSet::from(["jobs.submit".into()]),
            object_grants: Default::default(),
            read_roots: vec![path.clone()],
            write_roots: vec![path.clone()],
        };
        store.put("job/test-job", &job).unwrap();
        store.put("job-auth/test-job", &auth).unwrap();
        store.put("profile/operator", &policy).unwrap();
        assert!(store.execution_allowed("test-job").is_ok());
        policy.revision = 2;
        policy.allowed_command_ids.insert("jobs.inspect".into());
        store.put("profile/operator", &policy).unwrap();
        assert!(store.execution_allowed("test-job").is_ok());
        let publication = Publication {
            read_purpose_version: 1,
            job_id: "test-job".into(),
            proof: "unit-journal-proof".into(),
            output: job.plan.output.clone(),
            identity: job.plan.source.identity.clone(),
            verified: "unit authorization journal only".into(),
            published: false,
        };
        policy.revision = 3;
        policy.write_roots.clear();
        store.put("profile/operator", &policy).unwrap();
        assert!(store.execution_allowed("test-job").is_err());
        assert!(store.publication_fence(&publication).is_err());
        assert_eq!(
            store.inspect_job("test-job").unwrap().state,
            JobState::Verifying
        );
        assert!(
            store
                .get::<Publication>("publication/test-job")
                .unwrap()
                .is_none()
        );
        policy.revision = 4;
        policy.write_roots.push(path);
        store.put("profile/operator", &policy).unwrap();
        store.publication_fence(&publication).unwrap();
        assert_eq!(
            store.inspect_job("test-job").unwrap().state,
            JobState::Publishing
        );
    }
    #[test]
    fn oversized_reply_is_rejected_before_any_mutation_or_receipt_commits() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let request = request(store.issue_scope("alice", 100).unwrap());
        let record = ("unit/side-effect".into(), serde_json::json!(true));
        let oversized =
            serde_json::json!({"payload":"x".repeat(nw_domain::commands::MAX_RESULT_BYTES)});
        assert_eq!(
            store
                .commit("alice", &request, 100, oversized, &[record])
                .unwrap_err()
                .code,
            "RESOURCE_LIMIT"
        );
        assert!(store.get::<bool>("unit/side-effect").unwrap().is_none());
        assert!(store.replay("alice", &request, 100).unwrap().is_none());
    }

    #[test]
    fn profile_quota_rejects_only_new_records_and_keeps_existing_policy_writable() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        for page in 0..2 {
            let records = (0..64)
                .map(|index| {
                    (
                        format!("profile/unit-{}", page * 64 + index),
                        serde_json::json!(true),
                    )
                })
                .collect::<Vec<_>>();
            store.internal_batch(&records).unwrap();
        }
        assert_eq!(
            store.put("profile/one-too-many", &true).unwrap_err().code,
            "RESOURCE_LIMIT"
        );
        store.put("profile/unit-0", &false).unwrap();
        assert_eq!(store.get::<bool>("profile/unit-0").unwrap(), Some(false));
        assert!(store.get::<bool>("profile/one-too-many").unwrap().is_none());
    }
}
