//! Cleanup batch composition. Child media plans and jobs remain the authoritative engine work.
use super::*;
use nw_domain::batches::{
    Batch, BatchPlan, BatchPlanReview, BatchResult, CleanBatchInput, ItemResult, PlanReference,
    PlannedItem, PlannedItemView,
};

pub(super) struct MemberLease {
    index: u32,
    reference: PlanReference,
    source: files::SourceStamp,
    output_parent: files::DirectoryStamp,
}
pub(super) struct BatchPreflight {
    components: Vec<files::SourceStamp>,
    members: Vec<MemberLease>,
}
fn rejected(mut error: DomainError) -> DomainError {
    let mut end = error.message.len().min(2048);
    while !error.message.is_char_boundary(end) {
        end -= 1;
    }
    error.message.truncate(end);
    error
}
impl Service {
    pub(super) fn batch_child_command(&self, context: &Context, command: &str) -> Result<()> {
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(command))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                format!("Batch composition requires current {command} command access"),
            ));
        }
        Ok(())
    }
    pub(super) fn prepare_batch(
        &self,
        context: &Context,
        input: CleanBatchInput,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<BatchPlan> {
        input.validate()?;
        let revision = self
            .policy(context)?
            .as_ref()
            .map_or(0, |policy| policy.revision);
        self.batch_child_command(context, "streams.clean")?;
        self.admit_execution_limits(context, &input.execution_limits)?;
        let mut unresolved = BTreeSet::new();
        for (kind, selector) in [
            (0, &input.intent.audio_selection.selector),
            (1, &input.intent.subtitle_selection.selector),
        ] {
            if let nw_domain::mux::Selector::SourceIndices { values } = selector {
                for index in values {
                    unresolved.insert((kind, *index));
                }
            }
        }
        // Authorize every source before retaining a preview; an invisible source never becomes a row.
        for item in &input.items {
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Batch preview cancelled"));
            }
            let asset: Asset = self
                .store
                .get(&named("asset", &item.asset_id)?)?
                .ok_or_else(DomainError::object_not_found)?;
            self.object(context, &asset.id, &asset.creator)?;
            for stream in &asset.probe.streams {
                let kind = match stream.kind {
                    nw_domain::mux::StreamKind::Audio => Some(0),
                    nw_domain::mux::StreamKind::Subtitle => Some(1),
                    _ => None,
                };
                if let Some(kind) = kind {
                    unresolved.remove(&(kind, stream.index));
                }
            }
        }
        let mut items = Vec::with_capacity(input.items.len());
        let mut resolved_outputs = BTreeSet::new();
        for (index, item) in input.items.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Batch preview cancelled"));
            }
            let asset: Asset = self
                .store
                .get(&named("asset", &item.asset_id)?)?
                .ok_or_else(DomainError::object_not_found)?;
            self.object(context, &asset.id, &asset.creator)?;
            let mut candidate = PlannedItem {
                index: index as u32,
                asset_id: asset.id.clone(),
                source_sha256: asset.identity.sha256.clone(),
                source_bytes: asset.identity.bytes.clone(),
                output_name: item
                    .output
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                requested_output: item.output.clone(),
                plan: None,
                action: None,
                error: None,
            };
            match self.prepare_cleanup(context, input.clean_input(item), now) {
                Ok(plan) => {
                    if !resolved_outputs.insert(plan.output.clone()) {
                        candidate.error = Some(DomainError::new(
                            "OUTPUT_COLLISION",
                            "Two reviewed candidates select the same canonical output",
                        ));
                    } else {
                        candidate.output_name = plan
                            .output
                            .rsplit(['/', '\\'])
                            .next()
                            .unwrap_or("")
                            .to_owned();
                        candidate.requested_output = plan.output.clone();
                        candidate.action = Some(plan.resolution.action.clone());
                        candidate.plan = Some(PlanReference {
                            id: plan.id.clone(),
                            hash: plan.hash.clone(),
                        });
                        self.store.put(&named("plan", &plan.id)?, &plan)?;
                    }
                }
                Err(error) => candidate.error = Some(rejected(error)),
            }
            items.push(candidate);
        }
        let current = self.policy(context)?;
        if current.as_ref().map_or(0, |policy| policy.revision) != revision
            || current
                .as_ref()
                .is_some_and(|policy| !policy.allows_command("streams.clean.batch_plan"))
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Access policy changed while preparing the cleanup batch",
            ));
        }
        let mut plan = BatchPlan {
            schema_version: 1,
            id: Uuid::new_v4().to_string(),
            creator: context.principal.clone(),
            data_root: self.store.root_id().into(),
            hash: String::new(),
            created_at: now.to_string(),
            expires_at: now
                .checked_add(1800)
                .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Batch plan clock exhausted"))?
                .to_string(),
            failure_policy: input.failure_policy,
            unresolved_policy: input.unresolved_policy,
            unresolved: unresolved
                .into_iter()
                .map(|(kind, source_index)| nw_domain::batches::UnresolvedIndex {
                    kind: if kind == 0 {
                        nw_domain::mux::StreamKind::Audio
                    } else {
                        nw_domain::mux::StreamKind::Subtitle
                    },
                    source_index,
                })
                .collect(),
            items,
        };
        plan.hash = plan.digest()?;
        plan.validate_hash()?;
        self.store.put(&named("batch-plan", &plan.id)?, &plan)?;
        Ok(plan)
    }
    pub(super) fn read_batch_plan(
        &self,
        context: &Context,
        id: &str,
        action: ObjectAction,
    ) -> Result<BatchPlan> {
        let plan: BatchPlan = self
            .store
            .get(&named("batch-plan", id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object_action(context, ObjectKind::Plan, id, &plan.creator, action)?;
        if plan.id != id || plan.data_root != self.store.root_id() {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Cleanup batch belongs to another identity/root",
            ));
        }
        plan.validate_hash()?;
        Ok(plan)
    }
    pub(super) fn batch_review(
        &self,
        context: &Context,
        plan: &BatchPlan,
    ) -> Result<BatchPlanReview> {
        let policy = self.policy(context)?;
        let revision = policy.as_ref().map_or(0, |value| value.revision);
        let owner = policy.is_none();
        let mut items = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let output_path = if let Some(reference) = &item.plan {
                match self.read_plan(context, &reference.id, ObjectAction::Read) {
                    Ok(child) if child.hash == reference.hash => {
                        self.plan_review(context, &child)?.output_path
                    }
                    Ok(_) => {
                        return Err(DomainError::new(
                            "INVALID_PLAN",
                            "A child plan differs from its reviewed batch",
                        ));
                    }
                    Err(error) if error.code == "OBJECT_NOT_FOUND" => None,
                    Err(error) => return Err(error),
                }
            } else {
                owner.then(|| item.requested_output.clone())
            };
            items.push(PlannedItemView {
                index: item.index,
                asset_id: item.asset_id.clone(),
                source_sha256: item.source_sha256.clone(),
                source_bytes: item.source_bytes.clone(),
                output_name: item.output_name.clone(),
                output_path,
                plan: item.plan.clone(),
                action: item.action.clone(),
                error: item
                    .error
                    .as_ref()
                    .map(|error| nw_domain::views::ErrorView {
                        code: error.code.clone(),
                        message: owner.then(|| error.message.clone()),
                        retryable: error.retryable,
                    }),
            });
        }
        if self
            .policy(context)?
            .as_ref()
            .map_or(0, |value| value.revision)
            != revision
        {
            return Err(DomainError::new(
                "POLICY_CHANGED",
                "Access changed while reading the batch proposal; retry under current policy",
            ));
        }
        Ok(BatchPlanReview {
            schema_version: 1,
            id: plan.id.clone(),
            hash: plan.hash.clone(),
            created_at: plan.created_at.clone(),
            expires_at: plan.expires_at.clone(),
            failure_policy: plan.failure_policy,
            unresolved_policy: plan.unresolved_policy,
            unresolved: plan.unresolved.clone(),
            items,
        })
    }
    pub(super) fn read_batch(
        &self,
        context: &Context,
        id: &str,
        action: ObjectAction,
    ) -> Result<(Batch, BatchPlan)> {
        let batch: Batch = self
            .store
            .get(&named("batch", id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object_action(context, ObjectKind::Job, id, &batch.creator, action)?;
        if batch.id != id {
            return Err(DomainError::new(
                "INVALID_BATCH",
                "Stored batch identity differs",
            ));
        }
        let plan: BatchPlan = self
            .store
            .get(&named("batch-plan", &batch.plan_id)?)?
            .ok_or_else(|| {
                DomainError::new("INVALID_BATCH", "Accepted batch has no retained plan")
            })?;
        batch.validate(&plan)?;
        if plan.data_root != self.store.root_id() {
            return Err(DomainError::new("INVALID_BATCH", "Batch root differs"));
        }
        Ok((batch, plan))
    }
    pub(super) fn inspect_batch(&self, context: &Context, id: &str) -> Result<BatchResult> {
        let (batch, plan) = self.read_batch(context, id, ObjectAction::Read)?;
        let policy = self.policy(context)?;
        let revision = policy.as_ref().map_or(0, |value| value.revision);
        let owner = policy.is_none();
        let mut items = Vec::with_capacity(plan.items.len());
        for (member, item) in batch.members.iter().zip(&plan.items) {
            let mut result = ItemResult {
                index: item.index,
                asset_id: item.asset_id.clone(),
                output_name: item.output_name.clone(),
                job_id: member.job_id.clone(),
                state: None,
                action: item.action.clone(),
                artifact_id: None,
                source_bytes: item.source_bytes.clone(),
                output_bytes: None,
                error: item
                    .error
                    .as_ref()
                    .map(|error| nw_domain::views::ErrorView {
                        code: error.code.clone(),
                        message: owner.then(|| error.message.clone()),
                        retryable: error.retryable,
                    }),
            };
            if let Some(id) = &member.job_id {
                let job = self.store.inspect_job(id)?;
                self.object_action(
                    context,
                    ObjectKind::Job,
                    id,
                    &job.creator,
                    ObjectAction::Read,
                )?;
                if job.creator != batch.creator
                    || item.plan.as_ref().is_none_or(|reference| {
                        job.plan.id != reference.id || job.plan.hash != reference.hash
                    })
                {
                    return Err(DomainError::new(
                        "INVALID_BATCH",
                        "Accepted batch member differs from its captured job",
                    ));
                }
                let view = self.job_view(context, &job)?;
                result.state = Some(view.state);
                result.output_bytes = view
                    .artifact
                    .as_ref()
                    .map(|artifact| artifact.bytes.clone());
                result.artifact_id = view.artifact.map(|_| format!("job-{id}"));
                result.error = view.error.map(|mut error| {
                    // Full job details remain available through its separately authorized inspection.
                    // Keep group summaries bounded even when a worker records large diagnostics.
                    error.message = error.message.filter(|message| message.len() <= 2048);
                    error
                });
            }
            items.push(result);
        }
        let mut result = BatchResult {
            schema_version: 1,
            id: batch.id,
            plan_id: batch.plan_id,
            plan_hash: batch.plan_hash,
            total: items.len(),
            finished: 0,
            succeeded: 0,
            skipped: 0,
            failed: 0,
            cancelled: 0,
            interrupted: 0,
            complete: false,
            outcome: String::new(),
            successful_video_input_bytes: String::new(),
            successful_video_output_bytes: String::new(),
            successful_video_size_delta_bytes: String::new(),
            items,
        };
        result.finish_counts()?;
        if self
            .policy(context)?
            .as_ref()
            .map_or(0, |value| value.revision)
            != revision
        {
            return Err(DomainError::new(
                "POLICY_CHANGED",
                "Access changed while inspecting batch members; retry under current policy",
            ));
        }
        if serde_json::to_vec(&result)
            .map_err(|_| DomainError::new("INVALID_BATCH", "Cannot encode batch result"))?
            .len()
            > 224 * 1024
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Batch summary exceeds its bounded response budget; inspect individual member jobs",
            ));
        }
        Ok(result)
    }
    pub(super) fn prepare_batch_submission(
        &self,
        context: &Context,
        plan: &BatchPlan,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<BatchPreflight> {
        self.batch_child_command(context, "jobs.submit")?;
        plan.admits_submission()?;
        if plan.expires_at.parse::<u64>().unwrap_or(0) <= now {
            return Err(DomainError::new(
                "PLAN_EXPIRED",
                "Review a fresh cleanup batch",
            ));
        }
        let mut components = Vec::new();
        let mut members = Vec::new();
        if plan.items.iter().any(|item| item.plan.is_some()) {
            let engine = self.engine()?;
            for component in [&engine.ffmpeg, &engine.ffprobe] {
                let stamp = files::SourceStamp::capture_resolved(Path::new(&component.path))?;
                files::revalidate(component, cancel)?;
                stamp.revalidate()?;
                components.push(stamp);
            }
        }
        for item in &plan.items {
            let Some(reference) = &item.plan else {
                continue;
            };
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Batch preflight cancelled"));
            }
            let child = self.read_plan(context, &reference.id, ObjectAction::Submit)?;
            if child.hash != reference.hash || child.expires_at.parse::<u64>().unwrap_or(0) <= now {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "A batch child changed or expired",
                ));
            }
            self.engine()?.validate_plan(&child)?;
            self.admit_execution_limits(
                context,
                &child.execution_limits.clone().unwrap_or_default(),
            )?;
            self.plan_source_path(context, &child)?;
            self.path(context, Path::new(&child.output), true)?;
            let output_parent = nw_engine::output_parent_stamp(&child)?;
            let source =
                files::SourceStamp::capture_resolved(Path::new(&child.source.identity.path))?;
            source.require_object(child.source.source_object.as_ref().ok_or_else(|| {
                DomainError::new(
                    "INVALID_PLAN",
                    "Batch source has no frozen physical identity",
                )
            })?)?;
            files::revalidate(&child.source.identity, cancel)?;
            source.revalidate()?;
            output_parent.revalidate()?;
            members.push(MemberLease {
                index: item.index,
                reference: reference.clone(),
                source,
                output_parent,
            });
        }
        for component in &components {
            component.revalidate()?;
        }
        Ok(BatchPreflight {
            components,
            members,
        })
    }
    pub(super) fn revalidate_batch_submission(
        &self,
        context: &Context,
        plan: &BatchPlan,
        prepared: &BatchPreflight,
        now: u64,
    ) -> Result<()> {
        self.batch_child_command(context, "jobs.submit")?;
        plan.admits_submission()?;
        if plan.expires_at.parse::<u64>().unwrap_or(0) <= now
            || prepared.members.len()
                != plan.items.iter().filter(|item| item.plan.is_some()).count()
        {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Batch preflight expired or lost a candidate",
            ));
        }
        for component in &prepared.components {
            component.revalidate()?;
        }
        for member in &prepared.members {
            let item = &plan.items[member.index as usize];
            if item.plan.as_ref() != Some(&member.reference) {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Batch candidate changed during preflight",
                ));
            }
            let child = self.read_plan(context, &member.reference.id, ObjectAction::Submit)?;
            if child.hash != member.reference.hash
                || child.expires_at.parse::<u64>().unwrap_or(0) <= now
            {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Current child differs from batch preflight",
                ));
            }
            self.engine()?.validate_plan(&child)?;
            self.admit_execution_limits(
                context,
                &child.execution_limits.clone().unwrap_or_default(),
            )?;
            self.plan_source_path(context, &child)?;
            self.path(context, Path::new(&child.output), true)?;
            member
                .source
                .require_object(child.source.source_object.as_ref().ok_or_else(|| {
                    DomainError::new("INVALID_PLAN", "Batch source identity is missing")
                })?)?;
            member.output_parent.require_object(
                nw_domain::plans::validate_requirements(&child)?
                    .output_create
                    .parent_object
                    .as_ref()
                    .ok_or_else(|| {
                        DomainError::new("INVALID_PLAN", "Batch output parent identity is missing")
                    })?,
            )?;
        }
        Ok(())
    }
    pub(super) fn batch_receipt_readable(
        &self,
        context: &Context,
        response: &Response,
    ) -> Result<bool> {
        let Some(id) = response
            .result
            .as_ref()
            .and_then(|result| result["batch_id"].as_str())
        else {
            return Err(DomainError::new(
                "STORAGE_ERROR",
                "Retained batch receipt has no batch identity",
            ));
        };
        let (batch, _) = match self.read_batch(context, id, ObjectAction::Read) {
            Ok(value) => value,
            Err(error) if error.code == "OBJECT_NOT_FOUND" => return Ok(false),
            Err(error) => return Err(error),
        };
        for member in &batch.members {
            if let Some(id) = &member.job_id {
                let job = self.store.inspect_job(id)?;
                match self.object_action(
                    context,
                    ObjectKind::Job,
                    id,
                    &job.creator,
                    ObjectAction::Read,
                ) {
                    Ok(()) => {}
                    Err(error) if error.code == "OBJECT_NOT_FOUND" => return Ok(false),
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(true)
    }
}
