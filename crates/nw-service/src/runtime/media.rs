//! Authenticated finite compiler reviews do not persist new media records on old reader floors.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nw_domain::{
    capabilities::{CapabilityScope, ClearInput, InspectInput, RefreshInput},
    media::{self as domain, RenderInput},
};
use std::time::{Duration, Instant};

pub(super) mod browser_segment;
pub(super) mod mixed;
pub(super) use browser_segment::BrowserSegmentLease;
mod timeline_preview;
pub(super) mod native_preview;

struct StillFile {
    bytes: Vec<u8>,
    actual_frame_index: Option<u32>,
    timing: Option<nw_domain::preview::FrameTiming>,
    source_sha256: Option<String>,
    component_sha256: String,
    graph_sha256: String,
    timeline: Option<nw_domain::preview::TimelineObservation>,
}

impl Service {
    fn analysis_source(
        &self,
        context: &Context,
        asset_id: &str,
    ) -> Result<(Asset, files::SourceStamp)> {
        let asset: Asset = self
            .store
            .get(&named("asset", asset_id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object(context, &asset.id, &asset.creator)?;
        let path = self.path(context, Path::new(&asset.identity.path), false)?;
        let source = files::SourceStamp::capture_resolved(&path)?;
        Ok((asset, source))
    }

    fn complete_analysis(
        &self,
        context: &Context,
        assets: &[&Asset],
        command: &str,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let _delivery = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Media analysis cancelled before delivery",
            ));
        }
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(command))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "The media analysis grant changed during measurement",
            ));
        }
        for asset in assets {
            self.object(context, &asset.id, &asset.creator)?;
            self.path(context, Path::new(&asset.identity.path), false)?;
        }
        Ok(())
    }

    pub(super) fn source_volume(
        &self,
        context: &Context,
        input: nw_domain::audio_analysis::VolumeInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::audio_analysis::VolumeMeasurement> {
        let (asset, source) = self.analysis_source(context, &input.asset_id)?;
        let result = tokio::runtime::Handle::current().block_on(self.engine()?.measure_volume(
            &asset,
            source,
            input.stream_index,
            cancel.clone(),
        ))?;
        self.complete_analysis(context, &[&asset], "media.analyze.volume", &cancel)?;
        Ok(result)
    }

    pub(super) fn source_loudness(
        &self,
        context: &Context,
        input: nw_domain::audio_analysis::VolumeInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::audio_analysis::LoudnessMeasurement> {
        let (asset, source) = self.analysis_source(context, &input.asset_id)?;
        let result = tokio::runtime::Handle::current().block_on(
            self.engine()?
                .measure_loudness(&asset, source, input.stream_index, cancel.clone()),
        )?;
        self.complete_analysis(context, &[&asset], "media.analyze.loudness", &cancel)?;
        Ok(result)
    }

    pub(super) fn compare_source_audio(
        &self,
        context: &Context,
        input: nw_domain::audio_analysis::TrackSelection,
        cancel: CancellationToken,
    ) -> Result<nw_domain::audio_analysis::AudioTrackReport> {
        let (asset, source) = self.analysis_source(context, &input.asset_id)?;
        let result =
            tokio::runtime::Handle::current().block_on(self.engine()?.compare_audio_tracks(
                &asset,
                source,
                &input.stream_indices,
                cancel.clone(),
            ))?;
        self.complete_analysis(context, &[&asset], "media.analyze.audio_tracks", &cancel)?;
        Ok(result)
    }

    pub(super) fn compare_source_video(
        &self,
        context: &Context,
        input: nw_domain::reference_metrics::CompareInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::reference_metrics::Comparison> {
        input.frame_ranges()?;
        let (reference, reference_stamp) =
            self.analysis_source(context, &input.reference.asset_id)?;
        let (candidate, candidate_stamp) =
            self.analysis_source(context, &input.candidate.asset_id)?;
        let assets = [&reference, &candidate];
        let result =
            tokio::runtime::Handle::current().block_on(self.engine()?.compare_video_windows(
                &input,
                assets,
                [reference_stamp, candidate_stamp],
                self.root.join("runtime").join("reference-quality"),
                cancel.clone(),
            ))?;
        self.complete_analysis(context, &assets, "media.analyze.reference", &cancel)?;
        Ok(result)
    }

    pub(super) fn observe_source_stream(
        &self,
        context: &Context,
        input: nw_domain::stream_observations::Input,
        cancel: CancellationToken,
    ) -> Result<nw_domain::stream_observations::Report> {
        let (asset, source) = self.analysis_source(context, &input.asset_id)?;
        let report = tokio::runtime::Handle::current().block_on(self.engine()?.observe_stream(
            &asset,
            source,
            input,
            cancel.clone(),
        ))?;
        self.complete_analysis(context, &[&asset], "media.analyze.stream", &cancel)?;
        Ok(report)
    }

    pub(super) fn source_preview(
        &self,
        context: &Context,
        input: nw_domain::preview::FrameInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::preview::Frame> {
        use nw_domain::preview;
        let asset: Asset = self
            .store
            .get(&named("asset", &input.asset_id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object(context, &asset.id, &asset.creator)?;
        let path = self.path(context, Path::new(&asset.identity.path), false)?;
        let source = files::SourceStamp::capture_resolved(&path)?;
        let pixels =
            tokio::runtime::Handle::current().block_on(self.engine()?.preview_registered_frame(
                &asset,
                source,
                input.stream_index,
                input.frame,
                cancel.clone(),
            ))?;
        let _delivery = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Preview cancelled before delivery",
            ));
        }
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command("media.preview_frame"))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Source preview grant changed during decoding",
            ));
        }
        self.object(context, &asset.id, &asset.creator)?;
        self.path(context, Path::new(&asset.identity.path), false)?;
        Ok(preview::Frame {
            asset_id: asset.id,
            source_sha256: asset.identity.sha256,
            stream_index: input.stream_index,
            frame: input.frame,
            width: preview::WIDTH,
            height: preview::HEIGHT,
            rgba_base64: STANDARD.encode(pixels),
        })
    }

    pub(super) fn observe_media(
        &self,
        context: &Context,
        input: nw_domain::preview::ObserveInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::preview::ObservationResult> {
        use nw_domain::preview::{
            self, InlineObservation, ObservationMetadata, ObservationResult, ObserveInput,
            ObserveTarget,
        };

        let (target, frame_selection, source_policy, display_profile, fit, encoding, delivery) =
            match input {
                ObserveInput::Still {
                    target,
                    frame_selection,
                    source_policy,
                    display_profile,
                    fit,
                    encoding,
                    delivery,
                } => (
                    target,
                    frame_selection,
                    source_policy,
                    display_profile,
                    fit,
                    encoding,
                    delivery,
                ),
                ObserveInput::Audio(input) => {
                    return self
                        .observe_audio(context, input, cancel)
                        .map(|observation| ObservationResult::Audio(Box::new(observation)));
                }
            };
        let file = match &target {
            ObserveTarget::RegisteredSource {
                asset_id,
                stream_index,
                frame_index,
            } if frame_selection == preview::FrameSelection::DecodedIndex
                && display_profile == preview::DisplayProfile::SourceMonitorRgba8 =>
            {
                let (asset, source) = self.analysis_source(context, asset_id)?;
                let engine = self.engine()?;
                let observed =
                    tokio::runtime::Handle::current().block_on(engine.observe_registered_png(
                        &asset,
                        source,
                        *stream_index,
                        *frame_index,
                        cancel.clone(),
                    ))?;
                self.complete_analysis(context, &[&asset], preview::OBSERVE_COMMAND, &cancel)?;
                StillFile {
                    bytes: observed.bytes,
                    actual_frame_index: Some(*frame_index),
                    timing: observed.timing,
                    source_sha256: Some(asset.identity.sha256),
                    component_sha256: engine.ffmpeg.sha256.clone(),
                    graph_sha256: observed.graph_sha256,
                    timeline: None,
                }
            }
            ObserveTarget::TimelineFrame {
                project_id,
                sequence_id,
                revision,
                at,
            } if frame_selection == preview::FrameSelection::ExactSequenceGrid
                && display_profile == preview::DisplayProfile::TimelineMonitorRgba8 =>
            {
                self.observe_timeline_frame(
                    context,
                    project_id,
                    preview::TimelineFrameInput {
                        sequence_id: sequence_id.clone(),
                        revision: *revision,
                        at: *at,
                    },
                    cancel,
                )?
            }
            _ => {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "Choose the frame selection and display profile for this observation target",
                ));
            }
        };
        let metadata = ObservationMetadata {
            observation_id: Uuid::new_v4().to_string(),
            target,
            actual_frame_index: file.actual_frame_index,
            timing: file.timing,
            source_sha256: file.source_sha256,
            component_sha256: file.component_sha256,
            graph_sha256: file.graph_sha256,
            timeline: file.timeline,
            frame_selection,
            source_policy,
            display_profile,
            fit,
            encoding,
            delivery,
            width: preview::WIDTH,
            height: preview::HEIGHT,
            mime_type: preview::PNG_MIME_TYPE.into(),
            encoded_byte_count: file.bytes.len(),
            encoded_sha256: nw_domain::projects::hash(&file.bytes),
        };
        if serde_json::to_vec(&metadata)
            .map_err(|_| DomainError::new("SERIALIZATION_FAILED", "Invalid observation metadata"))?
            .len()
            > preview::MAX_OBSERVATION_METADATA_BYTES
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Observation metadata exceeds its 64 KiB budget",
            ));
        }
        Ok(ObservationResult::Still(Box::new(InlineObservation {
            metadata,
            png_base64: STANDARD.encode(&file.bytes),
        })))
    }

    fn observe_audio(
        &self,
        context: &Context,
        input: nw_domain::preview::AudioObservationInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::preview::InlineAudioObservation> {
        use nw_domain::preview::{self, AudioObservationMetadata, InlineAudioObservation};
        let (asset, source) = self.analysis_source(context, &input.target.asset_id)?;
        let engine = self.engine()?;
        let wave = tokio::runtime::Handle::current().block_on(engine.observe_pcm_wave(
            &asset,
            source,
            &input,
            self.root.join("runtime").join("observe-audio"),
            cancel.clone(),
        ))?;
        self.complete_analysis(context, &[&asset], preview::OBSERVE_COMMAND, &cancel)?;
        Ok(InlineAudioObservation {
            metadata: AudioObservationMetadata {
                observation_id: Uuid::new_v4().to_string(),
                target: input.target,
                range: input.range,
                sample_rate: input.sample_rate,
                channel_layout: input.channel_layout,
                frames: wave.frames,
                source_sha256: asset.identity.sha256,
                component_sha256: engine.ffmpeg.sha256.clone(),
                graph_sha256: wave.graph_sha256,
                source_policy: input.source_policy,
                encoding: input.encoding,
                delivery: input.delivery,
                mime_type: preview::WAVE_MIME_TYPE.into(),
                encoded_byte_count: wave.bytes.len(),
                encoded_sha256: nw_domain::projects::hash(&wave.bytes),
            },
            audio_base64: STANDARD.encode(wave.bytes),
        })
    }

    pub(super) fn component_capabilities(
        &self,
        command: &str,
        params: Value,
        owner: bool,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let runtime = tokio::runtime::Handle::current();
        match command {
            "capabilities.inspect" => {
                let InspectInput {
                    scope: CapabilityScope::Components,
                } = commands::parse(params)?;
                value(&runtime.block_on(self.engine()?.inspect_capabilities(cancel))?)
            }
            "capabilities.refresh" => {
                let input: RefreshInput = commands::parse(params)?;
                if input.devices && !owner {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "An explicit NVENC device probe requires owner authority",
                    ));
                }
                if !input.components && !input.devices {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Select at least one capability source to refresh",
                    ));
                }
                value(&runtime.block_on(self.engine()?.refresh_capabilities(input, cancel))?)
            }
            "capabilities.clear" => {
                let ClearInput {
                    namespace: CapabilityScope::Components,
                    fingerprint,
                } = commands::parse(params)?;
                if fingerprint.len() != 64
                    || !fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Capability fingerprint must be a lowercase SHA-256 digest",
                    ));
                }
                value(&runtime.block_on(self.engine()?.clear_capabilities(&fingerprint, cancel))?)
            }
            _ => Err(DomainError::new(
                "UNKNOWN_COMMAND",
                "Unregistered capability command",
            )),
        }
    }

    pub(super) fn media_review(
        self: &Arc<Self>,
        context: &Context,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        let started = Instant::now();
        let input: RenderInput = commands::parse(request.params.clone())?;
        let reference = request.project.as_ref().ok_or_else(|| {
            DomainError::new(
                "VALIDATION_ERROR",
                "Media review requires a project context",
            )
        })?;
        let policy = self.policy(context)?;
        if policy
            .as_ref()
            .is_some_and(|p| !p.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Media review command is not granted",
            ));
        }
        let read = self.read_project(context, &reference.id, ObjectAction::Read)?;
        let header_sha256 = nw_domain::projects::hash(read.header_raw().as_bytes());
        let revision = read.header().revision;
        let capture = domain::capture(&read, &input)?;
        drop(read);
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Media review cancelled before component observation",
            ));
        }
        let engine = self.engine()?;
        let tools = tokio::runtime::Handle::current()
            .block_on(engine.qualify_media_compiler(cancel.clone()))?;
        let build =
            files::identity(&std::env::current_exe().map_err(files::io_error)?, cancel)?.sha256;
        let review_id = Uuid::new_v4().to_string();
        let plan = domain::compile(
            capture,
            tools,
            self.store.root_id(),
            &context.principal,
            &build,
            &review_id,
            self.clock.now()?,
        )?;
        let response = Response::success(&request.request_id, plan);
        if serde_json::to_vec(&response)
            .map_err(|_| {
                DomainError::new("SERIALIZATION_FAILED", "Media review serialization failed")
            })?
            .len()
            > commands::MAX_RESULT_BYTES
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Media review exceeds the response budget",
            ));
        }
        let current = self.read_project(context, &reference.id, ObjectAction::Read)?;
        if current.header().revision != revision
            || nw_domain::projects::hash(current.header_raw().as_bytes()) != header_sha256
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Project changed during media review",
            ));
        }
        let current_policy = self.policy(context)?;
        if current_policy.as_ref().map_or(0, |p| p.revision)
            != policy.as_ref().map_or(0, |p| p.revision)
            || current_policy
                .as_ref()
                .is_some_and(|p| !p.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Policy changed during media review",
            ));
        }
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Media review cancelled before publication",
            ));
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Media review exceeds its 30-second admission",
            ));
        }
        Ok(response)
    }
}
