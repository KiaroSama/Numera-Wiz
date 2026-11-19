//! Revalidate the authoritative project and original-file bindings for mixed work.
use super::*;
use crate::runtime::projects::HeldSource;
use nw_domain::{
    media::{
        BoundSource, CapturedOccurrence, MediaPlanV2, MixedReview, OccurrenceSource,
        RenderContractV2, bind_mixed_source, capture_mixed,
    },
    media_execution::{self as execution, MediaJobState},
    media_v2_execution::{
        AcceptedMixedPayload, MediaJobV2, MixedExecutionAuthority, MixedJobKind, MixedOriginal,
        MixedOriginalBindings, MixedReservation,
    },
    plans::SourcePurpose,
};
use std::collections::{BTreeMap, btree_map::Entry};

pub(in crate::runtime) struct MixedInputs {
    pub originals: BTreeMap<String, HeldSource>,
    pub bindings: Vec<BoundSource>,
}

struct MixedRun {
    accepted: nw_store::MixedExecution,
    originals: Vec<(Asset, files::SourceStamp, u64)>,
    workspace: files::DirectoryStamp,
    max_output_bytes: u64,
}

impl MixedInputs {
    pub(in crate::runtime) fn revalidate(
        &self,
        service: &Service,
        context: &Context,
    ) -> Result<()> {
        for held in self.originals.values() {
            let asset = &held.asset;
            service.object(context, &asset.id, &asset.creator)?;
            let path = Path::new(&asset.identity.path);
            if service.path(context, path, false)? != path
                || service
                    .store
                    .project_fact_hash(&named("asset", &asset.id)?)?
                    != held.record_hash
            {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Reviewed mixed source authorization changed",
                ));
            }
            held.stamp.revalidate()?;
        }
        Ok(())
    }
}

impl Service {
    pub(in crate::runtime) fn bind_mixed_inputs<'a>(
        &self,
        context: &Context,
        clips: impl IntoIterator<Item = &'a CapturedOccurrence>,
        cancel: &CancellationToken,
    ) -> Result<MixedInputs> {
        let mut originals = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        for clip in clips {
            let OccurrenceSource::Registered {
                asset_id,
                stream_index,
                ..
            } = &clip.source
            else {
                continue;
            };
            if cancel.is_cancelled() {
                return Err(DomainError::new(
                    "CANCELLED",
                    "Mixed source preparation was cancelled",
                ));
            }
            let held = match originals.entry(asset_id.clone()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let (_, held) =
                        self.registered_project_source(context, asset_id, *stream_index, cancel)?;
                    entry.insert(held)
                }
            };
            if held.asset.source_object.is_none() {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Register the original again to capture its native file identity",
                ));
            }
            let binding = bind_mixed_source(clip, &held.asset, held.record_hash.clone())?;
            match bindings.entry((asset_id.clone(), *stream_index)) {
                Entry::Vacant(entry) => {
                    entry.insert(binding);
                }
                Entry::Occupied(entry) if entry.get() != &binding => {
                    return Err(DomainError::new(
                        "SOURCE_CHANGED",
                        "Mixed occurrences disagree on their physical source",
                    ));
                }
                Entry::Occupied(_) => {}
            }
        }
        Ok(MixedInputs {
            originals,
            bindings: bindings.into_values().collect(),
        })
    }

    pub(in crate::runtime) fn prepare_mixed_review(
        &self,
        context: &Context,
        request: &Request,
        input: &RenderContractV2,
        cancel: &CancellationToken,
    ) -> Result<(MediaPlanV2, MixedInputs)> {
        let project = &request
            .project
            .as_ref()
            .ok_or_else(|| {
                DomainError::new(
                    "VALIDATION_ERROR",
                    "Mixed media review requires a project context",
                )
            })?
            .id;
        let read = self.read_project(context, project, ObjectAction::Read)?;
        self.store.require_mixed_media_floor()?;
        let capture = capture_mixed(&read, &input.sequence_id, input.revision, input.range)?;
        drop(read);
        let held = self.bind_mixed_inputs(
            context,
            capture.tracks.iter().flat_map(|track| &track.clips),
            cancel,
        )?;
        let engine = self.engine()?;
        let gain_required = capture.tracks.iter().any(|track| track.gain_centidb != 0 && !track.clips.is_empty());
        let tools = tokio::runtime::Handle::current()
            .block_on(engine.qualify_mixed_compiler(gain_required, cancel.clone()))?;
        let build =
            files::identity(&std::env::current_exe().map_err(files::io_error)?, cancel)?.sha256;
        let id = Uuid::new_v4().to_string();
        let plan = MediaPlanV2::compile(
            input,
            capture,
            held.bindings.clone(),
            tools,
            MixedReview {
                root_id: self.store.root_id(),
                application_build: &build,
                principal: &context.principal,
                id: &id,
                now: self.clock.now()?,
            },
        )?;
        let _delivery = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Mixed review was cancelled before delivery",
            ));
        }
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Mixed review command grant changed",
            ));
        }
        let read = self.read_project(context, project, ObjectAction::Read)?;
        let current = capture_mixed(&read, &input.sequence_id, input.revision, input.range)?;
        if value(&current)? != value(&plan.payload.capture)? {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Project changed while preparing the mixed review",
            ));
        }
        drop(read);
        held.revalidate(self, context)?;
        Ok((plan, held))
    }

    pub(in crate::runtime) fn submit_mixed_media(
        self: &Arc<Self>,
        context: Context,
        request: Request,
        raw: String,
        plan: MediaPlanV2,
        cancel: CancellationToken,
    ) -> Result<Response> {
        self.store.require_mixed_media_floor()?;
        if plan.payload.root_id != self.store.root_id()
            || plan.payload.principal != context.principal
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Mixed review belongs to another principal or root",
            ));
        }
        let input: RenderContractV2 = commands::parse(json!({
            "contract_kind":"media_v2","sequence_id":plan.payload.capture.sequence_id,
            "revision":plan.payload.capture.project_revision,"range":plan.payload.capture.range,
            "output":plan.payload.output,
        }))?;
        let mut project_request = request.clone();
        project_request.project = Some(commands::ProjectRef {
            id: plan.payload.capture.project_id.to_string(),
            expected_revision: None,
        });
        let (observed, inputs) =
            self.prepare_mixed_review(&context, &project_request, &input, &cancel)?;
        if observed.hash != plan.hash {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Mixed review differs from the actual runtime capture",
            ));
        }
        let id = Uuid::new_v4().to_string();
        let directory = nw_platform::security::private_directory(&self.root.join("exports"))?;
        let parent = files::DirectoryStamp::capture_resolved(&directory)?;
        let output = self.path(&context, &directory.join(format!("media-{id}.mkv")), true)?;
        let raw_plan_sha256 = nw_domain::projects::hash(raw.as_bytes());
        let mut originals = MixedOriginalBindings {
            schema_version: 2,
            job_id: id.clone(),
            root_id: self.store.root_id().to_owned(),
            principal: context.principal.clone(),
            raw_plan_sha256: raw_plan_sha256.clone(),
            plan_hash: plan.hash.clone(),
            sources: Vec::new(),
        };
        for stream in inputs.bindings.iter().cloned() {
            let held = inputs
                .originals
                .get(&stream.asset_id)
                .ok_or_else(DomainError::object_not_found)?;
            let path = Path::new(&held.asset.identity.path);
            let directory = path.parent().ok_or_else(|| {
                DomainError::new("PATH_DENIED", "Original source parent is missing")
            })?;
            let source_purpose = if path.starts_with(self.root.join("assets")) {
                SourcePurpose::CreatorBoundManaged {
                    principal: context.principal.clone(),
                }
            } else {
                SourcePurpose::NativeFile
            };
            originals.sources.push(MixedOriginal {
                source: stream,
                original: held.asset.identity.clone(),
                source_purpose,
                native_object: held.stamp.object_identity()?,
                native_parent: files::DirectoryStamp::capture_resolved(directory)?
                    .object_identity()?,
            });
        }
        let original_bindings_sha256 = originals.digest()?;
        let payload = AcceptedMixedPayload {
            schema_version: 2,
            job_id: id.clone(),
            root_id: originals.root_id.clone(),
            principal: context.principal.clone(),
            raw_plan: raw,
            raw_plan_sha256: raw_plan_sha256.clone(),
            plan_hash: plan.hash.clone(),
            original_bindings_sha256: original_bindings_sha256.clone(),
        };
        let authority = MixedExecutionAuthority {
            schema_version: 2,
            job_id: id.clone(),
            root_id: originals.root_id.clone(),
            principal: context.principal.clone(),
            profile: context.profile.clone(),
            plan_hash: plan.hash.clone(),
            raw_plan_sha256: raw_plan_sha256.clone(),
            original_bindings_sha256: original_bindings_sha256.clone(),
            output: files::path_text(&output)?,
            output_parent: parent.object_identity()?,
        };
        let settings = &plan.payload.capture.settings;
        let rgba = u64::from(settings.width) * u64::from(settings.height) * 4;
        // The FFV1 encoder reserves one worst-case packet before writing a frame.
        let retained = nw_domain::media::MIXED_FFV1_ALLOCATION_BYTES
            + 32 * 1024 * 1024
            + rgba * (4 * inputs.originals.len() as u64 + 4);
        let reservation = MixedReservation {
            schema_version: 2,
            decoded_output_bytes: plan.payload.resources.decoded_output_bytes.clone(),
            retained_buffer_bytes: retained.to_string(),
            source_count: originals.sources.len(),
        };
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Mixed rendering was cancelled before acceptance",
            ));
        }
        if self.pending.load(Ordering::Acquire) >= execution::MAX_QUEUE {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Durable job queue is full",
            ));
        }
        inputs.revalidate(self, &context)?;
        self.path(&context, &output, true)?;
        parent.require_object(&authority.output_parent)?;
        let job = MediaJobV2 {
            envelope_version: 2,
            payload_kind: MixedJobKind::MediaJobV2,
            id: id.clone(),
            creator: context.principal.clone(),
            plan_id: plan.id,
            state: MediaJobState::Queued,
            accepted_at: "0".into(),
            queue_ticket: String::new(),
            revision: "0".into(),
            output: authority.output.clone(),
            raw_plan_sha256,
            plan_hash: plan.hash,
            original_bindings_sha256,
            reservation,
            owned_staging: None,
            artifact: None,
            error: None,
        };
        let response = self.store.accept_mixed_with_clock(
            &context.principal,
            &request,
            nw_store::MixedAcceptance {
                job: &job,
                payload: &payload,
                originals: &originals,
                authority: &authority,
            },
            || self.clock.now(),
        )?;
        if let Err(error) = self.schedule(id) {
            tracing::error!(code=%error.code, "Accepted mixed media awaits durable dispatcher recovery");
        }
        Ok(response)
    }

    fn prepare_mixed_run(&self, id: &str) -> Result<MixedRun> {
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        let accepted = self.store.allow_mixed_execution(id)?;
        let context = Context {
            principal: accepted.authority.principal.clone(),
            profile: accepted.authority.profile.clone(),
        };
        let output = Path::new(&accepted.authority.output);
        let parent = output
            .parent()
            .ok_or_else(|| DomainError::new("TARGET_CHANGED", "Mixed output parent is missing"))?;
        if parent != self.root.join("exports") || self.path(&context, output, true)? != output {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Mixed output escaped its managed destination",
            ));
        }
        let destination = files::DirectoryStamp::capture_resolved(parent)?;
        destination.require_object(&accepted.authority.output_parent)?;
        let mut originals = BTreeMap::new();
        let decoded = execution::exact(&accepted.plan.payload.resources.decoded_output_bytes)?;
        let max_output_bytes = (decoded.saturating_mul(2).saturating_add(64 * 1024 * 1024))
            .min(execution::MAX_OUTPUT_BYTES);
        let mut disk = max_output_bytes;
        for original in &accepted.originals.sources {
            if originals.contains_key(&original.source.asset_id) {
                continue;
            }
            let key = named("asset", &original.source.asset_id)?;
            let (asset, hash) = self
                .store
                .registered_asset_record(&key)?
                .ok_or_else(DomainError::object_not_found)?;
            let path = self.path(&context, Path::new(&asset.identity.path), false)?;
            if hash != original.source.asset_row_sha256
                || asset.identity != original.original
                || path != Path::new(&asset.identity.path)
            {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Mixed source row changed before execution",
                ));
            }
            let parent = path.parent().ok_or_else(|| {
                DomainError::new("SOURCE_CHANGED", "Mixed source parent is missing")
            })?;
            files::DirectoryStamp::capture_resolved(parent)?
                .require_object(&original.native_parent)?;
            let stamp = files::SourceStamp::capture_resolved(&path)?;
            stamp.require_object(&original.native_object)?;
            let limit = execution::exact(&asset.identity.bytes)?
                .saturating_mul(24)
                .saturating_add(32 * 1024 * 1024)
                .min(execution::MAX_OUTPUT_BYTES);
            disk = disk.checked_add(limit).ok_or_else(|| {
                DomainError::new(
                    "RESOURCE_LIMIT",
                    "Mixed scratch reservation exceeds its integer range",
                )
            })?;
            originals.insert(asset.id.clone(), (asset, stamp, limit));
        }
        self.store.require_media_space(parent, disk)?;
        self.store
            .advance_mixed_job(id, MediaJobState::Preparing, None)?;
        let scratch = parent.join(format!(".numera-job-{}", Uuid::new_v4()));
        self.store.journal_mixed_workspace(id, &scratch)?;
        let path = nw_platform::security::private_directory(&scratch)?;
        destination.revalidate()?;
        Ok(MixedRun {
            accepted,
            originals: originals.into_values().collect(),
            workspace: files::DirectoryStamp::capture_resolved(&path)?,
            max_output_bytes,
        })
    }

    pub(in crate::runtime) async fn run_mixed_job(
        self: &Arc<Self>,
        id: &str,
        cancel: CancellationToken,
    ) -> Result<()> {
        let started = std::time::Instant::now();
        let service = self.clone();
        let job_id = id.to_owned();
        let MixedRun {
            accepted,
            originals,
            workspace,
            max_output_bytes,
        } = tokio::task::spawn_blocking(move || service.prepare_mixed_run(&job_id))
            .await
            .map_err(|error| DomainError::new("PROCESS_WAIT_FAILED", error.to_string()))??;
        let build_cancel = cancel.clone();
        let build = tokio::task::spawn_blocking(move || {
            files::identity(
                &std::env::current_exe().map_err(files::io_error)?,
                &build_cancel,
            )
        })
        .await
        .map_err(|error| DomainError::new("FILESYSTEM_ERROR", error.to_string()))??;
        if build.sha256 != accepted.plan.payload.application_build {
            return Err(DomainError::new(
                "COMPONENT_CHANGED",
                "Application build changed after mixed acceptance",
            ));
        }
        let engine = self.engine()?;
        let render_limit = Duration::from_secs(u64::from(execution::MEDIA_RENDER_TIMEOUT_SECONDS));
        let limits = |max_bytes| {
            let deadline = render_limit
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or_else(|| {
                    DomainError::new(
                        "PROCESS_TIMEOUT",
                        "Mixed rendering exceeded its complete deadline",
                    )
                })?;
            Ok::<_, DomainError>(nw_engine::MixedProcessLimits {
                workspace: &workspace,
                max_bytes,
                deadline,
                cancel: cancel.clone(),
            })
        };
        self.store
            .advance_mixed_job(id, MediaJobState::Running, None)?;
        let mut staged = Vec::new();
        let mut assets = Vec::new();
        for (asset, stamp, limit) in originals {
            let bindings = accepted
                .plan
                .payload
                .sources
                .iter()
                .filter(|stream| stream.asset_id == asset.id)
                .cloned()
                .collect::<Vec<_>>();
            staged.push(
                engine
                    .stage_mixed_original(&asset, stamp, &bindings, limits(limit)?)
                    .await?,
            );
            assets.push(asset);
        }
        let borrowed = assets.iter().collect::<Vec<_>>();
        let verified = engine
            .verify_mixed_candidate(
                &accepted.plan,
                &borrowed,
                &staged,
                limits(max_output_bytes)?,
            )
            .await;
        let candidate = match verified {
            Ok(candidate) => candidate,
            Err(error) => {
                if super::super::scheduler::process_lifetime_uncertain(&error) {
                    for input in staged {
                        if let Err(retention) = input.retain() {
                            tracing::error!(code=%retention.code, "Could not retain a mixed process input for settlement");
                        }
                    }
                }
                return Err(error);
            }
        };
        self.store
            .advance_mixed_job(id, MediaJobState::Verifying, None)?;
        let service = self.clone();
        let job_id = id.to_owned();
        let observer: nw_engine::Observer = Arc::new(move |event| {
            let service = service.clone();
            let id = job_id.clone();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let _guard = service.mutation.lock().map_err(|_| lock_error())?;
                    match event {
                        nw_engine::ExecutionEvent::Fence(fence) => {
                            service.store.fence_mixed_publication(&fence)
                        }
                        nw_engine::ExecutionEvent::Published(identity, verification) => {
                            service.store.complete_mixed_publication(
                                &id,
                                identity,
                                &verification,
                            )?;
                            Ok(())
                        }
                        _ => Ok(()),
                    }
                })
                .await
                .map_err(|error| DomainError::new("PROCESS_WAIT_FAILED", error.to_string()))?
            })
        });
        engine
            .publish_verified_mixed(nw_engine::MixedPublishRequest {
                candidate,
                originals: staged,
                output: Path::new(&accepted.authority.output),
                parent: &accepted.authority.output_parent,
                job_id: id,
                cancel,
                observer,
            })
            .await?;
        Ok(())
    }

    pub(in crate::runtime) fn mixed_job_view(&self, job: &MediaJobV2) -> Value {
        json!({
            "envelope_version":2,"payload_kind":"media_job_v2","id":job.id,"state":job.state,
            "revision":job.revision,"plan_id":job.plan_id,"plan_hash":job.plan_hash,"payload_sha256":job.raw_plan_sha256,
            "resources":job.reservation,"artifact":{"id":format!("media-{}-output",job.id),
                "bytes":job.artifact.as_ref().map(|file| &file.bytes),"sha256":job.artifact.as_ref().map(|file| &file.sha256)},
            "error":job.error.as_ref().map(|error| json!({"code":error.code,"retryable":error.retryable})),
        })
    }

    pub(in crate::runtime) fn reconcile_mixed_job(&self, job: &MediaJobV2) -> Result<()> {
        let accepted = self.store.inspect_mixed_execution(&job.id)?;
        let confirmed = self.confirmed_media_publication(
            &job.id,
            &job.output,
            &accepted.authority.output_parent,
            |artifact, verification| {
                self.store
                    .complete_mixed_publication(&job.id, artifact, &verification)?;
                Ok(())
            },
        );
        if let Err(error) = confirmed {
            self.store
                .advance_mixed_job(&job.id, MediaJobState::Interrupted, Some(error))?;
        }
        Ok(())
    }

    pub(in crate::runtime) fn recover_mixed_job(&self, job: &MediaJobV2) -> Result<()> {
        if job
            .error
            .as_ref()
            .is_some_and(super::super::scheduler::process_lifetime_uncertain)
        {
            self.quarantine_execution();
        } else if job.state == MediaJobState::Publishing {
            self.reconcile_mixed_job(job)?;
        } else if !job.state.terminal() && job.state != MediaJobState::Queued {
            let launched = matches!(
                job.state,
                MediaJobState::Running | MediaJobState::Verifying | MediaJobState::Cancelling
            );
            let error = if launched {
                self.quarantine_execution();
                DomainError::new(
                    "TREE_TERMINATION_UNCERTAIN",
                    "Mixed execution stopped without retained confirmation of process reap",
                )
            } else {
                DomainError::new(
                    "OWNER_INTERRUPTED",
                    "Owner stopped before launching mixed execution",
                )
            };
            self.store
                .advance_mixed_job(&job.id, MediaJobState::Interrupted, Some(error))?;
        }
        Ok(())
    }
}
