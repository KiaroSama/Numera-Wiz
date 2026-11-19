//! Short browser audition bytes remain tied to the current source and project authority.
use super::*;
use nw_domain::{
    media::{MixedCapture, OccurrenceSource, capture_mixed},
    preview::{
        MAX_PROGRAM_PLAYBACK_BYTES, MAX_SOURCE_SEGMENT_BYTES, PROGRAM_PLAYBACK_MAX_SECONDS, PROGRAM_SEGMENT_COMMAND, ProgramPlaybackManifest, ProgramPlaybackRequest, ProgramSegment, ProgramSegmentInput,
        SOURCE_SEGMENT_SECONDS,
        SegmentBytes, SegmentPlayback, SourceSegment, SourceSegmentInput,
    },
    time::{Rational, TimeRange},
};
use nw_engine::{BrowserMp4Part, MixedProcessLimits};
use std::{collections::BTreeSet, ops::Range};

const SOURCE_COMMAND: &str = "previews.source_segment";
const PROGRAM_ORIGINALS: usize = 4;
const PROGRAM_STAGE_BYTES: u64 = 128 * 1024 * 1024;
const PROGRAM_DEADLINE: Duration = Duration::from_secs(16);
const PLAYBACK_DEADLINE: Duration = Duration::from_secs(PROGRAM_PLAYBACK_MAX_SECONDS as u64 + 35);
const MAX_SEGMENTS: usize = 4;
const TIME_TO_LIVE: u32 = 45;

pub(in crate::runtime) struct BrowserSegmentLease {
    principal: String,
    profile: Option<String>,
    expires: Instant,
    segment: Arc<nw_engine::BrowserSegment>,
    authority: SegmentAuthority,
}

pub struct BrowserProgramTransfer {
    service: Arc<Service>,
    context: Context,
    authority: SegmentAuthority,
    receive: tokio::sync::mpsc::Receiver<BrowserMp4Part>,
    worker: Option<tokio::task::JoinHandle<Result<()>>>,
    cancel: CancellationToken,
}

impl BrowserProgramTransfer {
    pub async fn next_part(&mut self) -> Result<Option<BrowserMp4Part>> {
        let part = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Program playback stopped")),
            part = self.receive.recv() => part,
        };
        if part.is_none() {
            if let Some(worker) = self.worker.take() {
                worker.await.map_err(|_| DomainError::new("WORKER_FAILED", "Program playback worker failed"))??;
            }
            return Ok(None);
        }
        let service = Arc::clone(&self.service);
        let context = self.context.clone();
        let authority = self.authority.clone();
        tokio::task::spawn_blocking(move || {
            let _publication = service.mutation.lock().map_err(|_| lock_error())?;
            service.recheck_program_segment(&context, &authority)
        }).await.map_err(|_| DomainError::new("WORKER_FAILED", "Program playback authorization worker failed"))??;
        Ok(part)
    }
}

impl Drop for BrowserProgramTransfer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Clone)]
enum SegmentAuthority {
    Source {
        asset_id: String,
        sha256: String,
        stamp: Arc<files::SourceStamp>,
    },
    Program {
        project: String,
        selection: ProgramSegmentInput,
        capture_sha256: String,
        originals: Arc<super::mixed::MixedInputs>,
    },
}

impl BrowserSegmentLease {
    fn owned_by(&self, context: &Context) -> bool {
        self.principal == context.principal && self.profile == context.profile
    }
}

fn extent(range: &Range<usize>) -> SegmentBytes {
    SegmentBytes {
        offset: range.start as u32,
        length: range.len() as u32,
    }
}

fn no_space() -> DomainError {
    DomainError::new("RESOURCE_LIMIT", "Browser preview capture capacity is full")
}

impl Service {
    pub(in crate::runtime) fn browser_playback_manifest(
        &self, context: &Context, project: &str, selection: ProgramSegmentInput,
    ) -> Result<ProgramPlaybackManifest> {
        check_playback_range(selection.range)?;
        self.check_browser_program_permission(context)?;
        let capture = self.browser_program_capture(context, project, &selection)?;
        program_source_count(&capture)?;
        Ok(ProgramPlaybackManifest { request: ProgramPlaybackRequest { project_id: capture.project_id, selection },
            path: "/api/v1/previews/program", method: "POST" })
    }

    pub async fn open_browser_program(
        self: &Arc<Self>, context: Context, input: ProgramPlaybackRequest, listener: CancellationToken,
    ) -> Result<BrowserProgramTransfer> {
        let service = Arc::clone(self);
        let cancellation = listener.child_token();
        let startup = cancellation.clone().drop_guard();
        let preparation_cancel = cancellation.clone();
        let preparation_context = context.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            service.check_browser_program_permission(&preparation_context)?;
            let permit = service.browser_capture_slot(&preparation_context)?;
            let project = input.project_id.as_str();
            let selection = input.selection;
            check_playback_range(selection.range)?;
            let capture = service.browser_program_capture(&preparation_context, project, &selection)?;
            let (originals, workspace) = service.prepare_program_originals(&preparation_context, &capture, &preparation_cancel)?;
            let authority = SegmentAuthority::Program {
                project: project.to_owned(), selection, capture_sha256: capture_fingerprint(&capture)?, originals: Arc::clone(&originals),
            };
            let engine = service.engine()?.clone();
            let _publication = service.mutation.lock().map_err(|_| lock_error())?;
            service.recheck_program_segment(&preparation_context, &authority)?;
            Ok::<_, DomainError>((permit, authority, capture, originals, workspace, engine))
        }).await.map_err(|_| DomainError::new("WORKER_FAILED", "Program playback preparation failed"))??;
        let (permit, authority, capture, originals, workspace, engine) = prepared;
        let (sink, receive) = nw_engine::browser_mp4_stream();
        let cancel = cancellation.clone();
        let worker = tokio::spawn(async move {
            let _permit = permit;
            let (assets, staged) = stage_program_originals(&engine, &originals, &workspace, Instant::now(), PROGRAM_DEADLINE, &cancel).await?;
            engine.stream_browser_program(&capture, &originals.bindings, &assets, staged, sink, MixedProcessLimits {
                workspace: &workspace, max_bytes: MAX_PROGRAM_PLAYBACK_BYTES, deadline: PLAYBACK_DEADLINE, cancel,
            }).await
        });
        Ok(BrowserProgramTransfer { service: Arc::clone(self), context, authority, receive, worker: Some(worker), cancel: startup.disarm() })
    }

    fn browser_capture_slot(&self, context: &Context) -> Result<tokio::sync::OwnedSemaphorePermit> {
        let capacity = self
            .browser_captures
            .clone()
            .try_acquire_owned()
            .map_err(|_| no_space())?;
        let mut segments = self.browser_segments.lock().map_err(|_| lock_error())?;
        let now = Instant::now();
        segments.retain(|_, entry| now < entry.expires);
        if segments.len() >= MAX_SEGMENTS && !segments.values().any(|entry| entry.owned_by(context))
        {
            return Err(no_space());
        }
        Ok(capacity)
    }

    pub(in crate::runtime) fn prepare_browser_source_segment(
        &self,
        context: &Context,
        selection: SourceSegmentInput,
        cancel: CancellationToken,
    ) -> Result<SourceSegment> {
        let _capacity = self.browser_capture_slot(context)?;
        let (asset, source) = self.analysis_source(context, &selection.asset_id)?;
        let (segment, source) =
            tokio::runtime::Handle::current().block_on(self.engine()?.browser_source_segment(
                &asset,
                source,
                selection.video_stream,
                selection.audio_stream,
                cancel.clone(),
            ))?;
        self.complete_analysis(context, &[&asset], SOURCE_COMMAND, &cancel)?;
        let source_sha256 = asset.identity.sha256;
        let authority = SegmentAuthority::Source {
            asset_id: selection.asset_id.clone(),
            sha256: source_sha256.clone(),
            stamp: Arc::new(source),
        };
        let playback = self.retain_browser_segment(context, segment, authority)?;
        Ok(SourceSegment {
            selection,
            source_sha256,
            segment_id: playback.segment_id,
            sha256: playback.sha256,
            bytes: playback.bytes,
            mime_type: playback.mime_type,
            initialization: playback.initialization,
            fragments: playback.fragments,
            expires_in_seconds: playback.expires_in_seconds,
            requested_range: TimeRange {
                start: Rational::ZERO,
                end: Rational::new(SOURCE_SEGMENT_SECONDS, 1)?,
            },
        })
    }

    pub(in crate::runtime) fn prepare_browser_program_segment(
        &self,
        context: &Context,
        project: &str,
        selection: ProgramSegmentInput,
        cancel: CancellationToken,
    ) -> Result<ProgramSegment> {
        let started = Instant::now();
        let _capacity = self.browser_capture_slot(context)?;
        selection.range.validate()?;
        if selection.range.end.checked_sub(selection.range.start)?
            > Rational::new(SOURCE_SEGMENT_SECONDS, 1)?
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Choose at most two seconds for program preview",
            ));
        }
        let capture = self.browser_program_capture(context, project, &selection)?;
        let (originals, workspace) = self.prepare_program_originals(context, &capture, &cancel)?;
        let capture_sha256 = capture_fingerprint(&capture)?;
        let engine = self.engine()?;
        let segment = tokio::runtime::Handle::current().block_on(async {
            let (assets, staged) = stage_program_originals(&engine, &originals, &workspace, started, PROGRAM_DEADLINE, &cancel).await?;
            engine.browser_program_segment(
                &capture, &originals.bindings, &assets, staged, MixedProcessLimits {
                    workspace: &workspace, max_bytes: MAX_SOURCE_SEGMENT_BYTES as u64,
                    deadline: program_time_left(started, PROGRAM_DEADLINE)?, cancel: cancel.clone(),
                },
            ).await
        })?;
        let authority = SegmentAuthority::Program {
            project: project.into(),
            selection: selection.clone(),
            capture_sha256: capture_sha256.clone(),
            originals,
        };
        let _publication = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Program preview was cancelled before delivery",
            ));
        }
        self.recheck_program_segment(context, &authority)?;
        let playback = self.retain_browser_segment(context, segment, authority)?;
        Ok(ProgramSegment {
            project_id: capture.project_id,
            selection,
            capture_sha256,
            playback,
        })
    }

    fn prepare_program_originals(
        &self, context: &Context, capture: &MixedCapture, cancel: &CancellationToken,
    ) -> Result<(Arc<super::mixed::MixedInputs>, files::DirectoryStamp)> {
        let asset_count = program_source_count(capture)?;
        let clips = capture.tracks.iter().flat_map(|track| &track.clips);
        let originals = Arc::new(self.bind_mixed_inputs(context, clips, cancel)?);
        let path = nw_platform::security::private_directory(&self.root.join("runtime").join("browser-program"))?;
        self.store.require_media_space(&path, PROGRAM_STAGE_BYTES * asset_count as u64)?;
        Ok((originals, files::DirectoryStamp::capture_resolved(&path)?))
    }

    fn browser_program_capture(
        &self,
        context: &Context,
        project: &str,
        selection: &ProgramSegmentInput,
    ) -> Result<MixedCapture> {
        let read = self.read_project(context, project, ObjectAction::Read)?;
        capture_mixed(
            &read,
            &selection.sequence_id,
            selection.revision,
            selection.range,
        )
    }

    /// The caller holds the mutation guard through checked delivery.
    fn recheck_program_segment(
        &self,
        context: &Context,
        authority: &SegmentAuthority,
    ) -> Result<()> {
        let SegmentAuthority::Program {
            project,
            selection,
            capture_sha256,
            originals,
        } = authority
        else {
            return Err(DomainError::object_not_found());
        };
        self.check_browser_program_permission(context)?;
        let current = self.browser_program_capture(context, project, selection)?;
        if capture_fingerprint(&current)? != *capture_sha256 {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Program preview no longer matches the project",
            ));
        }
        originals.revalidate(self, context)
    }

    fn check_browser_program_permission(&self, context: &Context) -> Result<()> {
        if self.execution_quarantined.load(Ordering::Acquire) {
            return Err(DomainError::new("RESOURCE_LIMIT", "Program playback capacity remains quarantined"));
        }
        if self.policy(context)?.as_ref().is_some_and(|policy| !policy.allows_command(PROGRAM_SEGMENT_COMMAND)) {
            return Err(DomainError::new("PERMISSION_DENIED", "Program preview access changed"));
        }
        Ok(())
    }

    fn retain_browser_segment(
        &self,
        context: &Context,
        segment: nw_engine::BrowserSegment,
        authority: SegmentAuthority,
    ) -> Result<SegmentPlayback> {
        let playback = SegmentPlayback {
            segment_id: Uuid::new_v4().to_string(),
            sha256: nw_domain::projects::hash(&segment.bytes),
            bytes: segment.bytes.len() as u32,
            mime_type: segment.mime_type.clone(),
            initialization: extent(&segment.initialization),
            fragments: segment.fragments.iter().map(extent).collect(),
            expires_in_seconds: TIME_TO_LIVE,
        };
        let lease = BrowserSegmentLease {
            principal: context.principal.clone(),
            profile: context.profile.clone(),
            expires: Instant::now() + Duration::from_secs(u64::from(TIME_TO_LIVE)),
            segment: Arc::new(segment),
            authority,
        };
        let mut segments = self.browser_segments.lock().map_err(|_| lock_error())?;
        let now = Instant::now();
        segments.retain(|_, entry| now < entry.expires);
        if segments.len() >= MAX_SEGMENTS {
            let replace = segments
                .iter()
                .filter(|(_, entry)| entry.owned_by(context))
                .min_by_key(|(_, entry)| entry.expires)
                .map(|(id, _)| id.clone())
                .ok_or_else(no_space)?;
            segments.remove(&replace);
        }
        segments.insert(playback.segment_id.clone(), lease);
        Ok(playback)
    }

    pub fn browser_segment_bytes(
        &self,
        context: &Context,
        id: &str,
    ) -> Result<Arc<nw_engine::BrowserSegment>> {
        let (segment, authority) = {
            let mut segments = self.browser_segments.lock().map_err(|_| lock_error())?;
            let now = Instant::now();
            segments.retain(|_, entry| now < entry.expires);
            let entry = segments
                .get(id)
                .filter(|entry| entry.owned_by(context))
                .ok_or_else(DomainError::object_not_found)?;
            (entry.segment.clone(), entry.authority.clone())
        };
        match &authority {
            SegmentAuthority::Source {
                asset_id,
                sha256,
                stamp,
            } => {
                let asset: Asset = self
                    .store
                    .get(&named("asset", asset_id)?)?
                    .ok_or_else(DomainError::object_not_found)?;
                if asset.identity.sha256 != *sha256 {
                    return Err(DomainError::new(
                        "SOURCE_CHANGED",
                        "The registered preview source changed",
                    ));
                }
                self.complete_analysis(
                    context,
                    &[&asset],
                    SOURCE_COMMAND,
                    &CancellationToken::new(),
                )?;
                stamp.revalidate()?;
            }
            SegmentAuthority::Program { .. } => {
                let _delivery = self.mutation.lock().map_err(|_| lock_error())?;
                self.recheck_program_segment(context, &authority)?;
            }
        }
        Ok(segment)
    }
}

fn check_playback_range(range: TimeRange) -> Result<()> {
    range.validate()?;
    if range.end.checked_sub(range.start)? > Rational::new(PROGRAM_PLAYBACK_MAX_SECONDS, 1)? {
        return Err(DomainError::new("RESOURCE_LIMIT", "Choose at most ten minutes of program playback"));
    }
    Ok(())
}

fn program_source_count(capture: &MixedCapture) -> Result<usize> {
    let asset_count = capture.tracks.iter().flat_map(|track| &track.clips).filter_map(|clip| match &clip.source {
        OccurrenceSource::Registered { asset_id, .. } => Some(asset_id.as_str()),
        OccurrenceSource::SolidColor { .. } => None,
    }).collect::<BTreeSet<_>>().len();
    if asset_count > PROGRAM_ORIGINALS || capture.settings.width > 1920
        || capture.settings.height > 1080 || capture.settings.frame_rate > Rational::new(60, 1)?
    {
        return Err(DomainError::new("RESOURCE_LIMIT", "Program preview exceeds its source count, raster or frame rate limit"));
    }
    Ok(asset_count)
}

fn program_time_left(start: Instant, budget: Duration) -> Result<Duration> {
    budget.checked_sub(start.elapsed()).filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| DomainError::new("PROCESS_TIMEOUT", "Program preview preparation exceeded its deadline"))
}

async fn stage_program_originals<'a>(
    engine: &nw_engine::Engine,
    originals: &'a super::mixed::MixedInputs,
    workspace: &files::DirectoryStamp,
    started: Instant,
    budget: Duration,
    cancel: &CancellationToken,
) -> Result<(Vec<&'a Asset>, Vec<nw_engine::StagedMixedInput>)> {
    let mut assets = Vec::with_capacity(originals.originals.len());
    let mut staged = Vec::with_capacity(originals.originals.len());
    for held in originals.originals.values() {
        let asset = &held.asset;
        let source = files::SourceStamp::capture_open_resolved(Path::new(&asset.identity.path), held.stamp.try_clone_file()?)?;
        let streams = originals.bindings.iter().filter(|binding| binding.asset_id == asset.id).cloned().collect::<Vec<_>>();
        staged.push(engine.stage_mixed_original(asset, source, &streams, MixedProcessLimits {
            workspace, max_bytes: PROGRAM_STAGE_BYTES, deadline: program_time_left(started, budget)?, cancel: cancel.clone(),
        }).await?);
        assets.push(asset);
    }
    Ok((assets, staged))
}

fn capture_fingerprint(capture: &MixedCapture) -> Result<String> {
    let encoded = serde_json::to_vec(capture).map_err(|_| {
        DomainError::new(
            "SERIALIZATION_FAILED",
            "Program capture identity is unavailable",
        )
    })?;
    Ok(nw_domain::projects::hash(&encoded))
}
