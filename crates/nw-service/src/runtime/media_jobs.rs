//! Media acceptance captures current authority; execution consumes only the accepted snapshot.
use super::*;
use nw_domain::media_execution::{
    self as domain, AcceptedMediaPayload, MediaArtifact, MediaArtifactState, MediaExecutionEvent,
    MediaExecutionFact, MediaInvocation, MediaJobKind, MediaJobState, MediaJobV1, MediaOutputClaim,
    MediaResourceReservation, MediaStage, StoredJob,
};

impl Service {
    fn recompile_media(
        &self,
        context: &Context,
        plan: &nw_domain::media::MediaPlanV1,
    ) -> Result<()> {
        let capture = &plan.payload.capture;
        let read = self.read_project(context, capture.project_id.as_str(), ObjectAction::Read)?;
        let actual = nw_domain::media::capture(
            &read,
            &nw_domain::media::RenderInput {
                sequence_id: capture.sequence_id.clone(),
                revision: capture.project_revision,
                range: capture.range,
                output: capture.output.clone(),
            },
        )?;
        let recompiled = nw_domain::media::compile(
            actual,
            plan.payload.tools.clone(),
            self.store.root_id(),
            &context.principal,
            &plan.payload.application_build,
            &plan.id,
            domain::exact(&plan.created_at)?,
        )?;
        // Recompute the complete graph from actual Store entities. Public SHA values are not authority.
        if value(&recompiled.payload)? != value(&plan.payload)? || recompiled.hash != plan.hash {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Reviewed media semantics differ from the authoritative project capture",
            ));
        }
        Ok(())
    }

    pub(super) fn submit_media(
        self: &Arc<Self>,
        context: Context,
        request: Request,
        cancellation: CancellationToken,
        guard: Option<std::sync::MutexGuard<'_, ()>>,
    ) -> Result<Response> {
        let input: commands::MediaSubmitInput = commands::parse(request.params.clone())?;
        let accepted = AcceptedMediaPayload::from_json(input.media_plan_json)?;
        let plan = accepted.plan()?;
        self.store.require_media_floor()?;
        if plan.creator != context.principal || plan.payload.root_id != self.store.root_id() {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Media review belongs to another creator or data root",
            ));
        }
        if !domain::review_current(&plan.created_at, &plan.expires_at, self.clock.now()?)? {
            return Err(DomainError::new(
                "PLAN_EXPIRED",
                "Media review expired before submission",
            ));
        }
        self.recompile_media(&context, &plan)?;
        let resources = MediaResourceReservation::estimate(&accepted)?;
        if domain::exact(&resources.estimated_memory_bytes)? > domain::MEDIA_RESERVATION_POOL_BYTES
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Media estimate exceeds the fixed1GiB worker reservation",
            ));
        }
        let id = Uuid::new_v4().to_string();
        let parent = nw_platform::security::private_directory(&self.root.join("exports"))?;
        let output = parent.join(format!("media-{id}.mkv"));
        self.path(&context, &output, true)?;
        let parent_stamp = files::DirectoryStamp::capture_resolved(&parent)?;
        let output_claim = MediaOutputClaim {
            artifact_id: domain::MEDIA_OUTPUT_ARTIFACT.into(),
            job_id: id.clone(),
            principal: context.principal.clone(),
            output: files::path_text(&output)?,
            parent: parent_stamp.object_identity()?,
        };
        drop(guard);
        #[cfg(test)]
        self.preflight_started();
        let prepared = (|| {
            self.store
                .require_media_space(&parent, domain::exact(&resources.scratch_bytes)?)?;
            let engine = self.engine()?;
            let tools = tokio::runtime::Handle::current()
                .block_on(engine.qualify_media_compiler(cancellation.clone()))?;
            if tools != plan.payload.tools {
                return Err(DomainError::new(
                    "COMPONENT_CHANGED",
                    "Reviewed media components changed",
                ));
            }
            let build = files::identity(
                &std::env::current_exe().map_err(files::io_error)?,
                &cancellation,
            )?;
            if build.sha256 != plan.payload.application_build {
                return Err(DomainError::new(
                    "COMPONENT_CHANGED",
                    "Media review belongs to another application build",
                ));
            }
            parent_stamp.revalidate()?;
            Ok(())
        })();
        let acceptance = self.mutation.lock().map_err(|_| lock_error())?;
        if let Some(replay) =
            self.refresh_acceptance(&context, &request, &cancellation, &acceptance)?
        {
            return Ok(replay);
        }
        // Replay wins before delayed preflight errors, expiry or changed project revisions.
        prepared?;
        self.store.require_media_floor()?;
        self.recompile_media(&context, &plan)?;
        if !domain::review_current(&plan.created_at, &plan.expires_at, self.clock.now()?)? {
            return Err(DomainError::new(
                "PLAN_EXPIRED",
                "Media review expired during preflight",
            ));
        }
        self.path(&context, &output, true)?;
        parent_stamp.require_object(&output_claim.parent)?;
        files::revalidate_output(&output_claim.output)?;
        if self.pending.load(Ordering::Acquire) >= domain::MAX_QUEUE {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Durable job queue budget reached",
            ));
        }
        let now = self.clock.now()?;
        let job = MediaJobV1 {
            envelope_version: 1,
            payload_kind: MediaJobKind::MediaJobV1,
            id: id.clone(),
            creator: context.principal.clone(),
            plan_id: plan.id,
            plan_hash: plan.hash,
            payload_sha256: accepted.raw_sha256.clone(),
            revision: "0".into(),
            accepted_at: now.to_string(),
            queue_ticket: format!("{now:020}/{id}"),
            state: MediaJobState::Queued,
            output: output_claim,
            resources,
            stages: [
                domain::MEDIA_RENDER_STAGE,
                domain::MEDIA_VERIFY_STAGE,
                domain::MEDIA_PUBLISH_STAGE,
            ]
            .into_iter()
            .map(|id| MediaStage {
                id: id.into(),
                state: MediaJobState::Queued,
                invocations: Vec::new(),
            })
            .collect(),
            artifact: MediaArtifact {
                id: domain::MEDIA_OUTPUT_ARTIFACT.into(),
                state: MediaArtifactState::Planned,
                identity: None,
                verification: None,
            },
            owned_staging: None,
            progress: None,
            error: None,
        };
        let policy = self.policy(&context)?;
        let authorization = ExecutionAuthorization {
            principal: context.principal.clone(),
            profile: context.profile,
            revision: policy.as_ref().map_or(0, |profile| profile.revision),
        };
        let response = self.store.accept_media_with_clock(
            &context.principal,
            &request,
            &job,
            &accepted,
            &authorization,
            || self.clock.now(),
        )?;
        if let Err(error) = self.schedule(id) {
            tracing::error!(code=%error.code, "Media acceptance is durable; restart resumes its immutable queued payload");
        }
        Ok(response)
    }

    pub(super) fn media_job_view(&self, job: &MediaJobV1) -> Value {
        json!({
            "envelope_version":1,"payload_kind":"media_job_v1","id":job.id,"state":job.state,
            "revision":job.revision,"plan_id":job.plan_id,"plan_hash":job.plan_hash,
            "payload_sha256":job.payload_sha256,"resources":job.resources,"progress":job.progress,
            "stages":job.stages.iter().map(|stage| json!({"id":stage.id,"state":stage.state,
                "invocations":stage.invocations.iter().map(|call| json!({"executable_sha256":call.executable_sha256,"argument_count":call.argv.len()})).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "artifact":{"id":format!("media-{}-output",job.id),"state":job.artifact.state,
                "bytes":job.artifact.identity.as_ref().map(|v|&v.bytes),"sha256":job.artifact.identity.as_ref().map(|v|&v.sha256),
                "verification":job.artifact.verification},
            "error":job.error.as_ref().map(|error| json!({"code":error.code,"retryable":error.retryable}))
        })
    }

    fn validate_media_job_paths(&self, job: &MediaJobV1) -> Result<files::DirectoryStamp> {
        self.store.media_execution_allowed(&job.id)?;
        let parent = Path::new(&job.output.output)
            .parent()
            .ok_or_else(|| DomainError::new("TARGET_CHANGED", "Media output parent missing"))?;
        if parent != self.root.join("exports")
            || Path::new(&job.output.output)
                .file_name()
                .and_then(|n| n.to_str())
                != Some(format!("media-{}.mkv", job.id).as_str())
        {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Media output escaped its accepted managed destination",
            ));
        }
        let stamp = files::DirectoryStamp::capture_resolved(parent)?;
        stamp.require_object(&job.output.parent)?;
        Ok(stamp)
    }

    pub(super) async fn run_media_job(
        self: &Arc<Self>,
        id: &str,
        token: CancellationToken,
    ) -> Result<()> {
        let service = self.clone();
        let job_id = id.to_owned();
        let started = tokio::task::spawn_blocking(move || {
            let _guard = service.mutation.lock().map_err(|_| lock_error())?;
            let StoredJob::MediaV1(job) = service.store.inspect_execution(&job_id)? else {
                return Err(DomainError::new("INVALID_RECORD", "Expected media job"));
            };
            if job.state.terminal() {
                return Ok(None);
            }
            service.validate_media_job_paths(&job)?;
            service.store.require_media_space(
                Path::new(&job.output.output)
                    .parent()
                    .expect("validated parent"),
                domain::exact(&job.resources.scratch_bytes)?,
            )?;
            let job = service.store.update_media_job(&job_id, |job| {
                if job.state != MediaJobState::Queued {
                    return Err(DomainError::new(
                        "STALE_JOB_STATE",
                        "Media job is not queued",
                    ));
                }
                job.state = MediaJobState::Preparing;
                job.stages[0].state = MediaJobState::Preparing;
                Ok(())
            })?;
            let payload = service.store.accepted_media_payload(&job_id)?;
            Ok(Some((job, payload.plan()?)))
        })
        .await
        .map_err(|error| DomainError::new("WORKER_FAILED", error.to_string()))??;
        let Some((job, plan)) = started else {
            return Ok(());
        };
        let weak = Arc::downgrade(self);
        let observer: nw_engine::MediaObserver = Arc::new(move |event| {
            let weak = weak.clone();
            Box::pin(async move {
                let service = weak
                    .upgrade()
                    .ok_or_else(|| DomainError::new("OWNER_STOPPING", "Media owner stopped"))?;
                tokio::task::spawn_blocking(move || service.media_execution_event(event))
                    .await
                    .map_err(|error| DomainError::new("WORKER_FAILED", error.to_string()))?
            })
        });
        self.engine()?
            .execute_media_observed(nw_engine::MediaExecutionRequest {
                plan: &plan,
                job_id: id,
                output: Path::new(&job.output.output),
                expected_parent: &job.output.parent,
                maximum_output_bytes: domain::exact(&job.resources.maximum_output_bytes)?,
                cancel: token,
                observer,
            })
            .await
            .map(|_| ())
    }

    fn media_execution_event(&self, event: MediaExecutionEvent) -> Result<()> {
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        let StoredJob::MediaV1(current) = self.store.inspect_execution(&event.job_id)? else {
            return Err(DomainError::new(
                "INVALID_RECORD",
                "Media observer addressed a legacy job",
            ));
        };
        let stage = [
            domain::MEDIA_RENDER_STAGE,
            domain::MEDIA_VERIFY_STAGE,
            domain::MEDIA_PUBLISH_STAGE,
        ]
        .iter()
        .position(|id| *id == event.stage_id)
        .ok_or_else(|| DomainError::new("INVALID_JOB_TRANSITION", "Unknown media stage"))?;
        if event.artifact_id != domain::MEDIA_OUTPUT_ARTIFACT || current.state.terminal() {
            return Err(DomainError::new(
                "INVALID_JOB_TRANSITION",
                "Media event identity/state differs",
            ));
        }
        if !matches!(event.fact, MediaExecutionFact::Published { .. }) {
            self.validate_media_job_paths(&current)?;
            if current.state == MediaJobState::Cancelling {
                return Err(DomainError::new(
                    "CANCELLED",
                    "Media cancellation committed before this event",
                ));
            }
        }
        match event.fact {
            MediaExecutionFact::Fence(publication) => {
                if stage != 2 || publication.job_id != current.id {
                    return Err(DomainError::new(
                        "INVALID_JOB_TRANSITION",
                        "Media fence identity differs",
                    ));
                }
                self.store.publication_fence(&publication)?;
            }
            MediaExecutionFact::Published {
                identity,
                verification,
            } => {
                if stage != 2 {
                    return Err(DomainError::new(
                        "INVALID_JOB_TRANSITION",
                        "Publication used another stage",
                    ));
                }
                self.store
                    .media_published(&current.id, identity, verification)?;
            }
            fact => {
                let accepted = self.store.accepted_media_payload(&current.id)?.plan()?;
                self.store.update_media_job(&current.id, |job| {
                    match fact {
                        MediaExecutionFact::StagingIntent(intent) => {
                            if stage != 0
                                || job.state != MediaJobState::Preparing
                                || job.owned_staging.is_some()
                                || intent.job_id != job.id
                                || Path::new(&intent.directory).parent()
                                    != Path::new(&job.output.output).parent()
                                || Path::new(&intent.payload).parent()
                                    != Some(Path::new(&intent.directory))
                                || !Path::new(&intent.directory)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .and_then(|n| n.strip_prefix(".numera-job-"))
                                    .is_some_and(|n| Uuid::parse_str(n).is_ok())
                                || Path::new(&intent.payload)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    != Some("payload.mkv")
                            {
                                return Err(DomainError::new(
                                    "INVALID_JOB_TRANSITION",
                                    "Media staging escaped its accepted owner",
                                ));
                            }
                            job.owned_staging = Some(intent);
                            job.artifact.state = MediaArtifactState::Staged;
                        }
                        MediaExecutionFact::Prepared {
                            executable_sha256,
                            argv,
                        } => {
                            if stage > 1
                                || job.stages[stage].invocations.len() >= domain::MAX_INVOCATIONS
                                || argv.is_empty()
                                || argv.len() > 256
                                || argv.iter().map(String::len).sum::<usize>() > 128 * 1024
                                || ![
                                    &accepted.payload.tools.ffmpeg.sha256,
                                    &accepted.payload.tools.ffprobe.sha256,
                                ]
                                .contains(&&executable_sha256)
                                || (stage == 0
                                    && !matches!(
                                        job.state,
                                        MediaJobState::Preparing | MediaJobState::Running
                                    ))
                                || (stage == 1 && job.state != MediaJobState::Verifying)
                            {
                                return Err(DomainError::new(
                                    "INVALID_JOB_TRANSITION",
                                    "Media invocation escaped its accepted stage/component budget",
                                ));
                            }
                            job.stages[stage].invocations.push(MediaInvocation {
                                executable_sha256,
                                argv,
                            });
                        }
                        MediaExecutionFact::Running => {
                            if stage != 0
                                || job.state != MediaJobState::Preparing
                                || job.stages[0].invocations.is_empty()
                            {
                                return Err(DomainError::new(
                                    "INVALID_JOB_TRANSITION",
                                    "Running media requires its prepared render",
                                ));
                            }
                            job.state = MediaJobState::Running;
                            job.stages[0].state = MediaJobState::Running;
                        }
                        MediaExecutionFact::Verifying => {
                            if stage != 1 || job.state != MediaJobState::Running {
                                return Err(DomainError::new(
                                    "INVALID_JOB_TRANSITION",
                                    "Media verification requires completed render",
                                ));
                            }
                            job.state = MediaJobState::Verifying;
                            job.stages[0].state = MediaJobState::Succeeded;
                            job.stages[1].state = MediaJobState::Verifying;
                        }
                        MediaExecutionFact::Progress(progress) => {
                            progress.validate()?;
                            if stage != 0 || job.state != MediaJobState::Running {
                                return Err(DomainError::new(
                                    "INVALID_JOB_TRANSITION",
                                    "Media progress requires running render",
                                ));
                            }
                            job.progress = Some(progress);
                        }
                        MediaExecutionFact::Fence(_) | MediaExecutionFact::Published { .. } => {
                            unreachable!("handled transactionally above")
                        }
                    }
                    Ok(())
                })?;
            }
        }
        let _ = self.events.send(event.job_id);
        Ok(())
    }

    pub(super) fn reconcile_media(&self, job: &MediaJobV1) -> Result<()> {
        let confirmed = (|| {
            let parent = files::DirectoryStamp::capture_resolved(
                Path::new(&job.output.output)
                    .parent()
                    .ok_or_else(|| DomainError::new("TARGET_CHANGED", "Media parent missing"))?,
            )?;
            parent.require_object(&job.output.parent)?;
            let destination = nw_platform::claims::Destination::reviewed(
                Path::new(&job.output.output),
                &job.output.parent,
            )?;
            let claims = nw_platform::claims::DestinationClaims::try_acquire(vec![destination])?;
            let publication: Publication = self
                .store
                .get(&format!("publication/{}/output", job.id))?
                .ok_or_else(DomainError::object_not_found)?;
            if publication.output != job.output.output
                || !files::same_object(
                    Path::new(&publication.proof),
                    Path::new(&publication.output),
                )?
            {
                return Err(DomainError::new(
                    "PUBLICATION_UNCONFIRMED",
                    "Media output is not the retained verified object",
                ));
            }
            let actual =
                files::identity(Path::new(&publication.output), &CancellationToken::new())?;
            if actual != publication.identity {
                return Err(DomainError::new(
                    "PUBLICATION_UNCONFIRMED",
                    "Media output bytes differ from the durable fence",
                ));
            }
            claims.revalidate()?;
            parent.revalidate()?;
            self.store
                .media_published(&job.id, actual, publication.verified)?;
            Ok(())
        })();
        if let Err(error) = confirmed {
            self.store.update_media_job(&job.id, |job| {
                job.state = MediaJobState::Interrupted;
                job.error = Some(error);
                Ok(())
            })?;
        }
        Ok(())
    }
}
