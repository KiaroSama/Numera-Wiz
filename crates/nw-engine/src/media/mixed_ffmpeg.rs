//! Closed FFmpeg composition for reviewed opaque video and mixed PCM windows.
use super::{
    BrowserSegment, Listing,
    browser_preview::{append_browser_audio, append_browser_output},
    identities, listing, normalized_pair::presentation::NativePreviewProfile,
};
use crate::{
    Engine, Observer, OutputDestination, PublicationStaging, VerifiedOutput, admission, blocking,
    publish_output,
};
use nw_domain::{
    DomainError, Result,
    media::{
        AudioLayout, BoundSource, CapturedOccurrence, MediaPlanV2, MixedCapture, MixedGraph,
        MixedTools, OccurrenceSource, compile_mixed_graph,
    },
    mux::StreamKind,
    records::{Asset, FileIdentity, NativeObjectIdentity},
    time::Rational,
};
use nw_platform::{
    files::{self, DirectoryStamp, SourceStamp},
    process::{self, AsyncChunkSink, StreamLimits},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Seek, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub const MIXED_FILTERS: &[&str] = &[
    "adelay",
    "aformat",
    "amix",
    "anullsrc",
    "aresample",
    "asetpts",
    "atrim",
    "color",
    "concat",
    "format",
    "fps",
    "pad",
    "scale",
    "setpts",
    "setsar",
    "settb",
    "trim",
];

pub struct StagedMixedInput {
    stage: tempfile::NamedTempFile,
    original: SourceStamp,
    identity: FileIdentity,
    asset_id: String,
    stream_indices: Vec<u32>,
}

impl StagedMixedInput {
    pub fn path(&self) -> &Path {
        self.stage.path()
    }

    pub async fn revalidate(&self, cancel: &CancellationToken) -> Result<()> {
        self.original.revalidate()?;
        let path = self.stage.path().to_owned();
        let file = self.stage.as_file().try_clone().map_err(files::io_error)?;
        let identity = self.identity.clone();
        let cancel = cancel.clone();
        blocking(move || {
            let stamp = SourceStamp::capture_open_resolved(&path, file)?;
            let limit = nw_domain::media_execution::exact(&identity.bytes)?;
            if stamp.identity_with_limit(&cancel, limit)? != identity {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Normalized source differs from its retained bytes",
                ));
            }
            Ok(())
        })
        .await?;
        self.original.revalidate()
    }

    pub fn retain(self) -> Result<()> {
        self.stage
            .keep()
            .map_err(|problem| files::io_error(problem.error))?;
        Ok(())
    }
}

pub struct VerifiedMixedCandidate {
    file: tempfile::NamedTempFile,
    identity: FileIdentity,
    proof: MixedSignalProof,
}

pub struct MixedSignalProof {
    pub video_frames: u64,
    pub audio_samples: u64,
    pub video_sha256: String,
    pub pcm_sha256: String,
}

impl VerifiedMixedCandidate {
    pub fn proof(&self) -> &MixedSignalProof {
        &self.proof
    }
}

pub struct MixedProcessLimits<'a> {
    pub workspace: &'a DirectoryStamp,
    pub max_bytes: u64,
    pub deadline: Duration,
    pub cancel: CancellationToken,
}

pub struct NativePreviewOutput<'a> {
    pub profile: NativePreviewProfile,
    pub sink: Box<dyn AsyncChunkSink>,
    pub limits: MixedProcessLimits<'a>,
}

pub struct MixedPublishRequest<'a> {
    pub candidate: VerifiedMixedCandidate,
    pub originals: Vec<StagedMixedInput>,
    pub output: &'a Path,
    pub parent: &'a NativeObjectIdentity,
    pub job_id: &'a str,
    pub cancel: CancellationToken,
    pub observer: Observer,
}

struct AudioExtent {
    input: usize,
    index: usize,
    channels: u64,
    samples: u64,
}

struct MixedFiltergraph {
    script: String,
    audio_extents: Vec<AudioExtent>,
}

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("INVALID_PLAN", message)
}

fn uncertain(error: &DomainError) -> bool {
    matches!(
        error.code.as_str(),
        "TREE_TERMINATION_UNCERTAIN"
            | "PROCESS_WAIT_FAILED"
            | "PROCESS_CAPTURE_FAILED"
            | "PROCESS_LAUNCH_FAILED"
    )
}

fn retain_candidate_on_error<T>(
    result: Result<T>,
    candidate: tempfile::NamedTempFile,
) -> Result<(T, tempfile::NamedTempFile)> {
    match result {
        Ok(proof) => Ok((proof, candidate)),
        Err(error) => {
            if uncertain(&error) && candidate.keep().is_err() {
                tracing::error!(
                    "Could not retain the mixed render candidate after uncertain process settlement"
                );
            }
            Err(error)
        }
    }
}

fn retain_preview_inputs(sidecar: tempfile::NamedTempFile, staged: Vec<StagedMixedInput>) {
    if sidecar.keep().is_err() {
        tracing::error!("Could not retain the preview composition graph for process settlement");
    }
    for input in staged {
        if input.retain().is_err() {
            tracing::error!("Could not retain a preview composition input for process settlement");
        }
    }
}

async fn validate_staged_inputs(
    sources: &[BoundSource],
    assets: &[&Asset],
    staged: &[StagedMixedInput],
    cancel: &CancellationToken,
) -> Result<()> {
    if assets.len() != staged.len() {
        return Err(invalid(
            "Mixed original set differs from admitted staged inputs",
        ));
    }
    for (asset, stage) in assets.iter().zip(staged) {
        let streams = sources.iter().filter(|stream| stream.asset_id == asset.id);
        if asset.id != stage.asset_id
            || !streams
                .map(|stream| stream.stream_index)
                .eq(stage.stream_indices.iter().copied())
        {
            return Err(invalid(
                "Normalized stream order differs from the reviewed physical inputs",
            ));
        }
        stage.revalidate(cancel).await?;
    }
    Ok(())
}

fn composition_invocation(
    workspace: &DirectoryStamp,
    staged: &[StagedMixedInput],
    script: &str,
) -> Result<(tempfile::NamedTempFile, Vec<String>)> {
    let mut graph = tempfile::Builder::new()
        .prefix(".numera-owned-mixed-filter-")
        .tempfile_in(workspace.path())
        .map_err(files::io_error)?;
    graph
        .write_all(script.as_bytes())
        .map_err(files::io_error)?;
    graph.as_file().sync_all().map_err(files::io_error)?;
    let mut args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-xerror",
        "-y",
        "-copyts",
        "-max_alloc",
    ]
    .map(String::from)
    .to_vec();
    args.extend([
        nw_domain::media::MIXED_FFV1_ALLOCATION_BYTES.to_string(),
        "-filter_complex_threads".into(),
        "1".into(),
    ]);
    for stage in staged {
        args.extend(
            [
                "-protocol_whitelist",
                "file",
                "-format_whitelist",
                "nut",
                "-threads",
                "1",
                "-f",
                "nut",
                "-i",
            ]
            .map(String::from),
        );
        args.push(format!("file:{}", files::path_text(stage.path())?));
    }
    args.extend(["-/filter_complex".into(), files::path_text(graph.path())?]);
    Ok((graph, args))
}

impl Engine {
    pub async fn qualify_mixed_compiler(
        &self,
        gain_required: bool,
        cancel: CancellationToken,
    ) -> Result<MixedTools> {
        let qualified = self.qualify_media_compiler(cancel.clone()).await?;
        let available = listing(self, "-filters", Listing::Filters, cancel.clone()).await?;
        if MIXED_FILTERS.iter().any(|name| !available.contains(*name))
            || (gain_required && !available.contains("volume"))
        {
            return Err(DomainError::new(
                "COMPONENT_UNAVAILABLE",
                "Selected FFmpeg lacks a required mixed composition filter",
            ));
        }
        identities(self, cancel).await?;
        let mut profile = b"NumeraWiz.MixedOpaqueFfv1Pcm.v2\0".to_vec();
        for name in MIXED_FILTERS {
            profile.extend_from_slice(name.as_bytes());
            profile.push(0);
        }
        if gain_required {
            profile.extend_from_slice(b"volume\0");
        }
        Ok(MixedTools {
            ffmpeg: qualified.ffmpeg,
            ffprobe: qualified.ffprobe,
            profile_sha256: nw_domain::projects::hash(&profile),
        })
    }

    /// Create a private normalized NUT input from the retained original. The
    /// later graph executor accepts these staged objects as its only file inputs.
    pub async fn stage_mixed_original(
        &self,
        asset: &Asset,
        source: SourceStamp,
        streams: &[BoundSource],
        limits: MixedProcessLimits<'_>,
    ) -> Result<StagedMixedInput> {
        let MixedProcessLimits {
            workspace,
            max_bytes,
            deadline,
            cancel,
        } = limits;
        let first = streams
            .first()
            .ok_or_else(|| invalid("Mixed source has no reviewed streams"))?;
        if streams.iter().any(|stream| {
            stream.asset_id != asset.id || stream.content_sha256 != asset.identity.sha256
        }) || streams
            .windows(2)
            .any(|pair| pair[0].stream_index >= pair[1].stream_index)
            || max_bytes == 0
        {
            return Err(invalid("Staged mixed stream identities or ordering differ"));
        }
        let asset_id = asset.id.clone();
        let stream_indices = streams.iter().map(|stream| stream.stream_index).collect();
        let kind = match first.media_kind {
            nw_domain::projects::TrackKind::Video => nw_domain::mux::StreamKind::Video,
            nw_domain::projects::TrackKind::Audio => nw_domain::mux::StreamKind::Audio,
        };
        let admission::RegisteredInput { input, source, .. } =
            admission::registered_stream(asset, source, first.stream_index, kind, cancel.clone())
                .await?;
        workspace.revalidate()?;
        let stage = tempfile::Builder::new()
            .prefix(".numera-owned-mixed-input-")
            .suffix(".nut")
            .tempfile_in(workspace.path())
            .map_err(files::io_error)?;
        let path = files::path_text(stage.path())?;
        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-xerror",
            "-y",
            "-copyts",
            "-max_alloc",
        ]
        .map(String::from)
        .to_vec();
        args.extend([
            nw_domain::media::MIXED_FFV1_ALLOCATION_BYTES.to_string(),
            "-threads".into(),
            "1".into(),
        ]);
        args.extend(input.options());
        args.extend(["-i".into(), "fd:".into()]);
        for stream in streams {
            args.extend(["-map".into(), format!("0:{}", stream.stream_index)]);
        }
        args.extend(
            [
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-c:v",
                "ffv1",
                "-threads:v",
                "1",
                "-fps_mode",
                "passthrough",
                "-enc_time_base:v",
                "demux",
                "-c:a",
                "pcm_s16le",
                "-avoid_negative_ts",
                "disabled",
                "-f",
                "nut",
                "-fs",
            ]
            .map(String::from),
        );
        args.extend([max_bytes.to_string(), path]);
        identities(self, cancel.clone()).await?;
        let ticket = self.process_budget.acquire(&cancel).await?;
        let result = process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            deadline,
            4096,
            input.file,
            cancel.clone(),
        )
        .await;
        if let Err(error) = ticket.finish(result) {
            if uncertain(&error) && stage.keep().is_err() {
                tracing::error!(
                    "Could not retain the mixed input stage after uncertain process settlement"
                );
            }
            return Err(error);
        }
        workspace.revalidate()?;
        identities(self, cancel.clone()).await?;
        blocking(move || {
            source.revalidate()?;
            let file = stage.as_file();
            file.sync_all().map_err(files::io_error)?;
            let output = SourceStamp::capture_open_resolved(
                stage.path(),
                file.try_clone().map_err(files::io_error)?,
            )?;
            if output.byte_len()? == 0 || output.byte_len()? >= max_bytes {
                return Err(invalid(
                    "Staged mixed input is empty or reached its reserved limit",
                ));
            }
            let identity = output.identity_with_limit(&cancel, max_bytes)?;
            Ok(StagedMixedInput {
                stage,
                original: source,
                identity,
                asset_id,
                stream_indices,
            })
        })
        .await
    }

    /// Encode one exact program window using the export composition, followed
    /// by the browser's bounded SDR display and stereo audition filters.
    pub async fn browser_program_segment(
        &self,
        capture: &MixedCapture,
        sources: &[BoundSource],
        assets: &[&Asset],
        staged: Vec<StagedMixedInput>,
        limits: MixedProcessLimits<'_>,
    ) -> Result<BrowserSegment> {
        let MixedProcessLimits {
            workspace,
            max_bytes,
            deadline,
            cancel,
        } = limits;
        let started = Instant::now();
        let remaining = || {
            deadline
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or_else(|| {
                    DomainError::new(
                        "PROCESS_TIMEOUT",
                        "Browser program preview exceeded its complete deadline",
                    )
                })
        };
        let limit = max_bytes.min(nw_domain::preview::MAX_SOURCE_SEGMENT_BYTES as u64) as usize;
        let maximum = Rational::new(nw_domain::preview::SOURCE_SEGMENT_SECONDS, 1)?;
        if limit == 0
            || capture.range.end.checked_sub(capture.range.start)? > maximum
            || capture.settings.width > 1920
            || capture.settings.height > 1080
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Browser program window exceeds its size or duration budget",
            ));
        }
        let graph = compile_mixed_graph(capture)?;
        validate_staged_inputs(sources, assets, &staged, &cancel).await?;
        let MixedFiltergraph {
            mut script,
            audio_extents,
        } = compose_filtergraph(capture, &graph, sources, assets)?;
        self.verify_mixed_extents(&staged, audio_extents, remaining, &cancel)
            .await?;
        script.push_str(concat!(
            ";\n[outv]scale=320:180:force_original_aspect_ratio=decrease:force_divisible_by=2:",
            "flags=lanczos:out_color_matrix=bt709:in_range=full:out_range=limited,format=yuv420p,",
            "pad=320:180:(ow-iw)/2:(oh-ih)/2:black,setsar=1,",
            "setparams=range=limited:color_primaries=bt709:color_trc=bt709:colorspace=bt709[viewv]",
        ));
        let (sidecar, mut args) = composition_invocation(workspace, &staged, &script)?;
        args.extend(["-map", "[viewv]", "-map", "[outa]"].map(str::to_owned));
        append_browser_audio(&mut args);
        append_browser_output(&mut args);
        workspace.revalidate()?;
        identities(self, cancel.clone()).await?;
        let ticket = self.process_budget.acquire(&cancel).await?;
        let left = match remaining() {
            Ok(left) => left,
            Err(error) => return ticket.finish(Err(error)),
        };
        let result = process::run(
            Path::new(&self.ffmpeg.path),
            &args,
            left,
            limit,
            cancel.clone(),
        )
        .await;
        let output = match ticket.finish(result) {
            Ok(output) => output,
            Err(error) => {
                if uncertain(&error) {
                    retain_preview_inputs(sidecar, staged);
                }
                return Err(error);
            }
        };
        validate_staged_inputs(sources, assets, &staged, &cancel).await?;
        workspace.revalidate()?;
        identities(self, cancel.clone()).await?;
        BrowserSegment::verify(output.stdout, true)
    }

    /// Feed native presentation from the same exact sequence composition.
    pub async fn stream_native_program(
        &self,
        capture: &MixedCapture,
        sources: &[BoundSource],
        assets: &[&Asset],
        staged: Vec<StagedMixedInput>,
        output: NativePreviewOutput<'_>,
    ) -> Result<()> {
        let NativePreviewOutput { profile, sink, limits } = output;
        let MixedProcessLimits { workspace, max_bytes, deadline, cancel } = limits;
        let picture_bytes = profile.validate()?;
        if profile.range != capture.range || profile.frame_rate != capture.settings.frame_rate
            || capture.settings.width > 3840 || capture.settings.height > 2160
        {
            return Err(invalid("Native preview clock or raster differs from its captured limits"));
        }
        let started = Instant::now();
        let remaining = || deadline.checked_sub(started.elapsed()).filter(|left| !left.is_zero())
            .ok_or_else(|| DomainError::new("PROCESS_TIMEOUT", "Native program preview exceeded its complete deadline"));
        let graph = compile_mixed_graph(capture)?;
        let frames = nw_domain::media_execution::exact(&graph.output_frames)?;
        let duration = capture.range.end.checked_sub(capture.range.start)?;
        let sample_frames = if profile.audio {
            nw_domain::projects::grid_tick(duration, Rational::new(48_000, 1)?)?
        } else {
            0
        };
        let decoded_bytes = frames * picture_bytes as u64 + sample_frames * 8;
        if max_bytes <= decoded_bytes {
            return Err(DomainError::new("RESOURCE_LIMIT", "Native preview exceeds its stdout budget"));
        }
        let stdout_bytes = (decoded_bytes + 1024 * 1024).min(max_bytes);
        validate_staged_inputs(sources, assets, &staged, &cancel).await?;
        let MixedFiltergraph { mut script, audio_extents } =
            compose_filtergraph(capture, &graph, sources, assets)?;
        self.verify_mixed_extents(&staged, audio_extents, remaining, &cancel).await?;
        let NativePreviewProfile { width, height, .. } = profile;
        script.push_str(&format!(
            ";\n[outv]scale={width}:{height}:force_original_aspect_ratio=decrease:flags=lanczos,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,format=rgba[viewv]"
        ));
        if !profile.audio {
            script.push_str(";\n[outa]anullsink");
        }
        let (sidecar, mut args) = composition_invocation(workspace, &staged, &script)?;
        args.extend([
            "-map", "[viewv]", "-c:v", "rawvideo", "-threads:v", "1", "-pix_fmt", "rgba",
            "-fps_mode:v", "passthrough", "-enc_time_base:v", "filter",
        ].map(str::to_owned));
        if profile.audio {
            args.extend([
                "-map", "[outa]", "-c:a", "pcm_f32le", "-ar", "48000", "-ac", "2",
                "-bsf:a", "pcm_rechunk=n=480:p=0",
            ].map(str::to_owned));
        }
        args.extend([
            "-map_metadata", "-1", "-map_chapters", "-1", "-avoid_negative_ts", "disabled",
            "-f", "nut", "-write_index", "0", "-syncpoints", "default", "pipe:1",
        ].map(str::to_owned));
        workspace.revalidate()?;
        identities(self, cancel.clone()).await?;
        let ticket = self.process_budget.acquire(&cancel).await?;
        let left = match remaining() {
            Ok(left) => left,
            Err(error) => return ticket.finish(Err(error)),
        };
        let result = process::stream_with_sink(
            Path::new(&self.ffmpeg.path), &args, left,
            StreamLimits { stdout_bytes, stderr_bytes: 4096 }, sink, cancel.clone(),
        ).await;
        if let Err(error) = ticket.finish(result) {
            if uncertain(&error) {
                retain_preview_inputs(sidecar, staged);
            }
            return Err(error);
        }
        validate_staged_inputs(sources, assets, &staged, &cancel).await?;
        workspace.revalidate()?;
        identities(self, cancel).await
    }

    /// Compose verified normalized inputs into one owned candidate. The caller
    /// commits its independent signal proof before attempting publication.
    pub async fn verify_mixed_candidate(
        &self,
        plan: &MediaPlanV2,
        assets: &[&Asset],
        staged: &[StagedMixedInput],
        limits: MixedProcessLimits<'_>,
    ) -> Result<VerifiedMixedCandidate> {
        let MixedProcessLimits {
            workspace,
            max_bytes,
            deadline,
            cancel,
        } = limits;
        let started = Instant::now();
        let remaining = || {
            deadline
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or_else(|| {
                    DomainError::new(
                        "PROCESS_TIMEOUT",
                        "Mixed output verification exceeded its complete deadline",
                    )
                })
        };
        let gain_required = plan
            .payload
            .graph
            .audio
            .iter()
            .any(|span| span.gain_centidb != 0);
        let current = self
            .qualify_mixed_compiler(gain_required, cancel.clone())
            .await?;
        if current != plan.payload.tools
            || max_bytes == 0
            || current.ffmpeg.sha256 != self.ffmpeg.sha256
        {
            return Err(invalid(
                "Mixed components or output reservation differ from the review",
            ));
        }
        validate_staged_inputs(&plan.payload.sources, assets, staged, &cancel).await?;
        workspace.revalidate()?;
        let MixedFiltergraph {
            script,
            audio_extents,
        } = mixed_filtergraph(plan, assets)?;
        self.verify_mixed_extents(staged, audio_extents, remaining, &cancel)
            .await?;
        let file = tempfile::Builder::new()
            .prefix("payload")
            .rand_bytes(0)
            .suffix(".mkv")
            .tempfile_in(workspace.path())
            .map_err(files::io_error)?;
        let (graph, mut args) = composition_invocation(workspace, staged, &script)?;
        args.extend(
            [
                "-map",
                "[outv]",
                "-map",
                "[outa]",
                "-c:v",
                "ffv1",
                "-threads:v",
                "1",
                "-pix_fmt",
                "bgra",
                "-fps_mode",
                "passthrough",
                "-c:a",
                "pcm_s16le",
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-f",
                "matroska",
                "-fs",
            ]
            .map(String::from),
        );
        args.extend([max_bytes.to_string(), files::path_text(file.path())?]);
        let deadline = remaining()?;
        let ticket = self.process_budget.acquire(&cancel).await?;
        let run = process::run(
            Path::new(&self.ffmpeg.path),
            &args,
            deadline,
            4096,
            cancel.clone(),
        )
        .await;
        if let Err(error) = ticket.finish(run) {
            if uncertain(&error) {
                if file.keep().is_err() {
                    tracing::error!(
                        "Could not retain the mixed render candidate after uncertain process settlement"
                    );
                }
                if graph.keep().is_err() {
                    tracing::error!(
                        "Could not retain the mixed render graph after uncertain process settlement"
                    );
                }
            }
            return Err(error);
        }
        for input in staged {
            input.revalidate(&cancel).await?;
        }
        workspace.revalidate()?;
        identities(self, cancel.clone()).await?;
        let hash_cancel = cancel.clone();
        let (file, candidate, identity) = blocking(move || {
            file.as_file().sync_all().map_err(files::io_error)?;
            let candidate = SourceStamp::capture_open_resolved(
                file.path(),
                file.as_file().try_clone().map_err(files::io_error)?,
            )?;
            let identity = candidate.identity_with_limit(&hash_cancel, max_bytes)?;
            Ok((file, candidate, identity))
        })
        .await?;
        let settings = &plan.payload.capture.settings;
        let video_frames = nw_domain::media_execution::exact(&plan.payload.graph.output_frames)?;
        let audio_samples = nw_domain::media_execution::exact(&plan.payload.graph.output_samples)?;
        let channels = match settings.audio_layout {
            AudioLayout::Mono => 1,
            AudioLayout::Stereo => 2,
        };
        let output = admission::from_file(candidate.try_clone_file()?, &cancel)?;
        let probe = self
            .probe_input(
                output,
                nw_domain::execution::CleanupLimits::default().probe,
                cancel.clone(),
            )
            .await;
        let (probe, file) = retain_candidate_on_error(probe, file)?;
        let video = probe
            .stream_details
            .get(&0)
            .ok_or_else(|| invalid("Mixed output has no video declaration"))?;
        let audio = probe
            .stream_details
            .get(&1)
            .ok_or_else(|| invalid("Mixed output has no audio declaration"))?;
        if probe
            .streams
            .iter()
            .map(|stream| (stream.index, &stream.kind, stream.codec.as_str()))
            .ne([
                (0, &StreamKind::Video, "ffv1"),
                (1, &StreamKind::Audio, "pcm_s16le"),
            ])
            || probe.format != "matroska,webm"
            || video["time_base"] != "1/1000"
            || audio["time_base"] != "1/1000"
            || video["width"].as_u64() != Some(u64::from(settings.width))
            || video["height"].as_u64() != Some(u64::from(settings.height))
            || video["pix_fmt"].as_str() != Some("bgra")
            || audio["channels"].as_u64() != Some(channels)
            || audio["sample_rate"]
                .as_str()
                .and_then(|rate| rate.parse::<u32>().ok())
                != Some(settings.sample_rate)
        {
            return Err(invalid(
                "Mixed output stream declarations differ from the reviewed profile",
            ));
        }
        let clocks = self
            .verify_mixed_clocks(
                &candidate,
                settings,
                [video_frames, audio_samples],
                remaining()?,
                &cancel,
            )
            .await;
        let (_, file) = retain_candidate_on_error(clocks, file)?;
        let video_bytes = video_frames
            .checked_mul(u64::from(settings.width) * u64::from(settings.height) * 4)
            .ok_or_else(|| invalid("Decoded video counter overflow"))?;
        let audio_bytes = audio_samples
            .checked_mul(2 * channels)
            .ok_or_else(|| invalid("Decoded audio counter overflow"))?;
        let video = self
            .verify_mixed_decode(
                &candidate,
                ["0:v:0", "rawvideo", "-pix_fmt", "bgra", "rawvideo"],
                video_bytes,
                remaining()?,
                &cancel,
                None,
            )
            .await;
        let (video_sha256, file) = retain_candidate_on_error(video, file)?;
        let pcm = self
            .verify_mixed_decode(
                &candidate,
                ["0:a:0", "pcm_s16le", "-sample_fmt", "s16", "s16le"],
                audio_bytes,
                remaining()?,
                &cancel,
                None,
            )
            .await;
        let (pcm_sha256, file) = retain_candidate_on_error(pcm, file)?;
        Ok(VerifiedMixedCandidate {
            file,
            identity,
            proof: MixedSignalProof {
                video_frames,
                audio_samples,
                video_sha256,
                pcm_sha256,
            },
        })
    }

    async fn verify_mixed_extents(
        &self,
        staged: &[StagedMixedInput],
        audio_extents: Vec<AudioExtent>,
        remaining: impl Fn() -> Result<Duration>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        for extent in audio_extents {
            let stage = &staged[extent.input];
            let source = SourceStamp::capture_open_resolved(
                stage.path(),
                stage.stage.as_file().try_clone().map_err(files::io_error)?,
            )?;
            let index = format!("0:{}", extent.index);
            let stop = format!("atrim=end_sample={}", extent.samples);
            let expected = extent
                .samples
                .checked_mul(2 * extent.channels)
                .ok_or_else(|| invalid("Normalized audio sample verification overflow"))?;
            self.verify_mixed_decode(
                &source,
                [index.as_str(), "pcm_s16le", "-sample_fmt", "s16", "s16le"],
                expected,
                remaining()?,
                cancel,
                Some(&stop),
            )
            .await?;
        }
        Ok(())
    }

    async fn verify_mixed_clocks(
        &self,
        source: &SourceStamp,
        settings: &nw_domain::media::MediaSettings,
        [frames, samples]: [u64; 2],
        deadline: Duration,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let input = admission::from_file(source.try_clone_file()?, cancel)?;
        let mut args = [
            "-v",
            "error",
            "-max_alloc",
            "67108864",
            "-threads",
            "1",
            "-show_frames",
            "-show_entries",
            "frame=stream_index,pts,nb_samples",
            "-of",
            "compact=p=0:nk=0",
        ]
        .map(String::from)
        .to_vec();
        args.extend(input.options());
        args.extend(["-i", "fd:"].map(String::from));
        let received = Arc::new(Mutex::new(ClockFacts::default()));
        let consumer = Arc::clone(&received);
        let rate = settings.frame_rate;
        let video_clock = (1000 * rate.denominator() as u128, rate.numerator() as u128);
        let audio_clock = (1000, u128::from(settings.sample_rate));
        let consume = Box::new(move |chunk: &[u8]| {
            consumer
                .lock()
                .map_err(|_| invalid("Mixed clock verifier lock failed"))?
                .feed(chunk, video_clock, audio_clock)
        });
        let ticket = self.process_budget.acquire(cancel).await?;
        let result = process::stream_verified_readonly(
            Path::new(&self.ffprobe.path),
            &args,
            deadline,
            input.file,
            process::StreamLimits {
                stdout_bytes: 16 * 1024 * 1024,
                stderr_bytes: 4096,
            },
            consume,
            cancel.clone(),
        )
        .await;
        ticket.finish(result)?;
        source.revalidate()?;
        let actual = received
            .lock()
            .map_err(|_| invalid("Mixed clock verifier lock failed"))?;
        if !actual.row.is_empty() || (actual.frames, actual.samples) != (frames, samples) {
            return Err(invalid(
                "Mixed frame clocks do not cover the complete reviewed signal",
            ));
        }
        Ok(())
    }

    pub async fn publish_verified_mixed(
        &self,
        request: MixedPublishRequest<'_>,
    ) -> Result<FileIdentity> {
        let MixedPublishRequest {
            candidate,
            originals,
            output,
            parent,
            job_id,
            cancel,
            observer,
        } = request;
        let VerifiedMixedCandidate {
            file,
            identity,
            proof,
        } = candidate;
        let MixedSignalProof {
            video_frames,
            audio_samples,
            video_sha256,
            pcm_sha256,
        } = proof;
        nw_domain::projects::validate_id(job_id)?;
        let output_path = output
            .parent()
            .ok_or_else(|| invalid("Mixed publication parent is missing"))?;
        let directory = DirectoryStamp::capture_resolved(output_path)?;
        directory.require_object(parent)?;
        let recheck = cancel.clone();
        let (stage, proof, source, identity) = blocking(move || {
            let path = file.path().to_owned();
            let original = file.as_file().try_clone().map_err(files::io_error)?;
            let stage = nw_platform::publication::RetainedStaging::new(file)?;
            let proof_name = format!(".numera-proof-{}", uuid::Uuid::new_v4());
            let proof = stage.create_proof(proof_name.as_ref())?;
            let source = SourceStamp::capture_open_resolved(&path, original)?;
            let expected_bytes = nw_domain::media_execution::exact(&identity.bytes)?;
            if source.identity_with_limit(&recheck, expected_bytes)? != identity {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Verified mixed candidate changed before its publication fence",
                ));
            }
            Ok((stage, proof, source, identity))
        })
        .await?;
        identities(self, cancel.clone()).await?;
        let verification = format!(
            "FFV1/BGRA {video_frames} decoded frames ({video_sha256}); PCM {audio_samples} decoded samples ({pcm_sha256})"
        );
        let mut sources: Vec<_> = originals.into_iter().map(|input| input.original).collect();
        sources.push(source);
        publish_output(
            VerifiedOutput {
                file: PublicationStaging::Linked {
                    file: Box::new(stage),
                    proof,
                },
                identity,
                sources,
            },
            OutputDestination {
                path: files::path_text(output)?,
                parent: directory,
                parent_object: parent.clone(),
            },
            job_id,
            verification,
            cancel,
            observer,
        )
        .await
    }

    async fn verify_mixed_decode(
        &self,
        source: &SourceStamp,
        [stream, codec, option, format, muxer]: [&str; 5],
        expected: u64,
        deadline: Duration,
        cancel: &CancellationToken,
        normalized_audio_filter: Option<&str>,
    ) -> Result<String> {
        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-xerror",
            "-max_alloc",
            "67108864",
            "-threads",
            "1",
        ]
        .map(String::from)
        .to_vec();
        let input = if normalized_audio_filter.is_some() {
            args.extend(admission::options("nut"));
            let mut file = source.try_clone_file()?;
            file.rewind().map_err(files::io_error)?;
            file
        } else {
            let input = admission::from_file(source.try_clone_file()?, cancel)?;
            args.extend(input.options());
            input.file
        };
        args.extend(
            [
                "-i", "fd:", "-map", stream, "-c", codec, "-threads", "1", option, format,
            ]
            .map(String::from),
        );
        if muxer == "rawvideo" {
            args.extend(["-fps_mode", "passthrough"].map(String::from));
        }
        if let Some(filter) = normalized_audio_filter {
            args.extend(["-af", filter].map(String::from));
        }
        args.extend(["-f", muxer, "pipe:1"].map(String::from));
        let proof = Arc::new(Mutex::new((0_u64, Sha256::new())));
        let received = Arc::clone(&proof);
        let consumer = Box::new(move |bytes: &[u8]| {
            let mut actual = received
                .lock()
                .map_err(|_| invalid("Mixed signal verifier lock failed"))?;
            actual.0 = actual
                .0
                .checked_add(bytes.len() as u64)
                .filter(|bytes| *bytes <= expected)
                .ok_or_else(|| {
                    invalid("Decoded mixed output exceeds its reviewed signal length")
                })?;
            actual.1.update(bytes);
            Ok(())
        });
        let ticket = self.process_budget.acquire(cancel).await?;
        let result = process::stream_verified_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            deadline,
            input,
            process::StreamLimits {
                stdout_bytes: expected,
                stderr_bytes: 4096,
            },
            consumer,
            cancel.clone(),
        )
        .await;
        ticket.finish(result)?;
        source.revalidate()?;
        let actual = proof
            .lock()
            .map_err(|_| invalid("Mixed signal verifier lock failed"))?;
        if actual.0 != expected {
            return Err(invalid(
                "Decoded mixed output is shorter than its reviewed signal length",
            ));
        }
        Ok(hex::encode(actual.1.clone().finalize()))
    }
}

#[derive(Default)]
struct ClockFacts {
    row: Vec<u8>,
    frames: u64,
    samples: u64,
}

impl ClockFacts {
    fn feed(
        &mut self,
        mut chunk: &[u8],
        video_clock: (u128, u128),
        audio_clock: (u128, u128),
    ) -> Result<()> {
        while !chunk.is_empty() {
            let end = chunk.iter().position(|byte| *byte == b'\n');
            let count = end.unwrap_or(chunk.len());
            if self.row.len() + count > 96 {
                return Err(invalid("Mixed decoded clock row exceeds its finite format"));
            }
            self.row.extend_from_slice(&chunk[..count]);
            let Some(end) = end else {
                break;
            };
            let text = std::str::from_utf8(&self.row)
                .map_err(|_| invalid("Invalid mixed clock encoding"))?;
            let fields = text.trim_end_matches('\r').split('|').collect::<Vec<_>>();
            let rounded =
                |count, (num, den): (u128, u128)| (u128::from(count) * num + den / 2) / den;
            let value = |field: &str, key| {
                field
                    .strip_prefix(key)
                    .and_then(|text| text.parse::<u64>().ok())
                    .ok_or_else(|| invalid("Mixed clock row lacks a valid counter"))
            };
            match fields.as_slice() {
                ["stream_index=0", pts]
                    if u128::from(value(pts, "pts=")?) == rounded(self.frames, video_clock) =>
                {
                    self.frames += 1;
                }
                ["stream_index=1", pts, samples]
                    if u128::from(value(pts, "pts=")?) == rounded(self.samples, audio_clock) =>
                {
                    let samples = value(samples, "nb_samples=")?;
                    self.samples = self
                        .samples
                        .checked_add(samples)
                        .filter(|_| samples > 0)
                        .ok_or_else(|| {
                            invalid("Mixed PCM clock contains an invalid sample count")
                        })?;
                }
                _ => {
                    return Err(invalid(
                        "Decoded mixed stream has a shifted, duplicate or discontinuous clock",
                    ));
                }
            }
            self.row.clear();
            chunk = &chunk[end + 1..];
        }
        Ok(())
    }
}

struct Inputs<'a> {
    assets: BTreeMap<&'a str, (usize, &'a Asset)>,
    streams: BTreeMap<(&'a str, u32), (usize, &'a BoundSource)>,
    clips: BTreeMap<&'a str, &'a CapturedOccurrence>,
}

impl<'a> Inputs<'a> {
    fn new(
        capture: &'a MixedCapture,
        sources: &'a [BoundSource],
        assets: &'a [&'a Asset],
    ) -> Result<Self> {
        let indexed: BTreeMap<_, _> = assets
            .iter()
            .enumerate()
            .map(|(index, asset)| (asset.id.as_str(), (index, *asset)))
            .collect();
        let mut streams = BTreeMap::new();
        let mut next_stream = BTreeMap::new();
        for stream in sources {
            let (_, original) = indexed
                .get(stream.asset_id.as_str())
                .ok_or_else(|| invalid("Mixed asset is missing from the admitted inputs"))?;
            let observed_probe = serde_json::to_vec(&original.probe)
                .map_err(|_| invalid("Cannot encode original probe"))?;
            if stream.content_sha256 != original.identity.sha256
                || stream.probe_sha256 != nw_domain::projects::hash(&observed_probe)
            {
                return Err(invalid(
                    "Mixed graph original hash differs from the reviewed source",
                ));
            }
            let ordinal = next_stream.entry(stream.asset_id.as_str()).or_insert(0);
            streams.insert(
                (stream.asset_id.as_str(), stream.stream_index),
                (*ordinal, stream),
            );
            *ordinal += 1;
        }
        let expected = streams
            .keys()
            .map(|(id, _)| *id)
            .collect::<std::collections::BTreeSet<_>>();
        if indexed.len() != assets.len() || !indexed.keys().copied().eq(expected) {
            return Err(invalid(
                "Admitted rendering inputs differ from the exact reviewed asset set",
            ));
        }
        let clips = capture
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|clip| (clip.occurrence_id.as_str(), clip))
            .collect();
        Ok(Self {
            assets: indexed,
            streams,
            clips,
        })
    }

    fn clip(&self, id: &str) -> Result<&'a CapturedOccurrence> {
        self.clips
            .get(id)
            .copied()
            .ok_or_else(|| invalid("Mixed graph occurrence is missing"))
    }

    fn original(
        &self,
        clip: &CapturedOccurrence,
    ) -> Result<(usize, usize, &'a Asset, &'a BoundSource)> {
        let OccurrenceSource::Registered {
            asset_id,
            stream_index,
            ..
        } = &clip.source
        else {
            return Err(invalid(
                "Mixed graph window requires an admitted registered stream",
            ));
        };
        let (index, asset) = self
            .assets
            .get(asset_id.as_str())
            .copied()
            .ok_or_else(|| invalid("Mixed graph original asset is missing"))?;
        let (ordinal, stream) = self
            .streams
            .get(&(asset_id.as_str(), *stream_index))
            .copied()
            .ok_or_else(|| invalid("Mixed graph physical stream is missing"))?;
        Ok((index, ordinal, asset, stream))
    }
}

/// Produce only the application-owned filtergraph for a selected input order.
/// The execution phase freezes input handles, demuxers and component identity.
fn mixed_filtergraph(plan: &MediaPlanV2, assets: &[&Asset]) -> Result<MixedFiltergraph> {
    plan.validate_hash()?;
    compose_filtergraph(
        &plan.payload.capture,
        &plan.payload.graph,
        &plan.payload.sources,
        assets,
    )
}

fn compose_filtergraph(
    capture: &MixedCapture,
    graph: &MixedGraph,
    sources: &[BoundSource],
    assets: &[&Asset],
) -> Result<MixedFiltergraph> {
    let inputs = Inputs::new(capture, sources, assets)?;
    let (width, height) = (capture.settings.width, capture.settings.height);
    let rate = capture.settings.frame_rate;
    let fps = format!("{}/{}", rate.numerator(), rate.denominator());
    let clock = format!("{}/{}", rate.denominator(), rate.numerator());
    let sample_rate = capture.settings.sample_rate;
    let layout = capture.settings.audio_layout.name();
    let output_samples = &graph.output_samples;
    let mut chains = Vec::new();
    let mut video_labels = String::new();
    for (ordinal, span) in graph.video.iter().enumerate() {
        let count = &span.frame_count;
        let label = format!("[v{ordinal}]");
        let color = |rgb: &[u8; 3]| {
            format!(
                "color=c=0x{:02x}{:02x}{:02x}:s={width}x{height}:r={fps},format=bgra",
                rgb[0], rgb[1], rgb[2]
            )
        };
        let source = if let Some(window) = &span.window {
            let clip = inputs.clip(window.occurrence_id.as_str())?;
            match &clip.source {
                OccurrenceSource::SolidColor { rgb } => color(rgb),
                OccurrenceSource::Registered { .. } => {
                    let (input, index, _, stream) = inputs.original(clip)?;
                    let start = stream.original_tick(window.range.start)?;
                    let end = stream.original_tick(window.range.end)?;
                    let clock_num = stream.time_base.numerator();
                    let clock_den = stream.time_base.denominator();
                    let speed_num = clip.rate.numerator();
                    let speed_den = clip.rate.denominator();
                    format!(
                        "[{input}:{index}]settb={clock_num}/{clock_den},trim=start_pts={start}:end_pts={end},setpts=(PTS-STARTPTS)*{speed_den}/{speed_num},fps={fps}:round=near,scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,format=bgr0,format=bgra"
                    )
                }
            }
        } else {
            color(&[0, 0, 0])
        };
        chains.push(format!(
            "{source},trim=end_frame={count},settb={clock},setpts=N{label}"
        ));
        video_labels.push_str(&label);
    }
    chains.push(format!(
        "{video_labels}concat=n={}:v=1:a=0,settb={clock},setpts=N[outv]",
        graph.video.len()
    ));

    chains.push(format!("anullsrc=r={sample_rate}:cl={layout},atrim=end_sample={output_samples},asetpts=N/SR/TB[abase]"));
    let mut audio_labels = String::from("[abase]");
    let mut extents = BTreeMap::<_, AudioExtent>::new();
    let unit_speed = Rational::new(1, 1)?;
    for (ordinal, span) in graph.audio.iter().enumerate() {
        let clip = inputs.clip(span.window.occurrence_id.as_str())?;
        if clip.rate != unit_speed {
            return Err(invalid(
                "This PCM composition profile requires unit audio playback rate",
            ));
        }
        let (input, index, asset, stream) = inputs.original(clip)?;
        let details = asset
            .probe
            .stream_details
            .get(&stream.stream_index)
            .ok_or_else(|| invalid("Registered audio stream details are missing"))?;
        let source_rate = details["sample_rate"]
            .as_str()
            .and_then(|rate| rate.parse::<u32>().ok())
            .ok_or_else(|| invalid("Registered audio sample rate is absent"))?;
        let channels = match capture.settings.audio_layout {
            AudioLayout::Mono => 1,
            AudioLayout::Stereo => 2,
        };
        if details["channels"].as_u64() != Some(channels)
            || details["channel_layout"]
                .as_str()
                .is_some_and(|value| value != layout && value != "unknown")
        {
            return Err(invalid(
                "Reviewed output layout requires a separately specified channel mapping",
            ));
        }
        let start = stream.original_audio_sample(span.window.range.start, source_rate)?;
        let end = stream.original_audio_sample(span.window.range.end, source_rate)?;
        let extent = extents.entry((input, index)).or_insert(AudioExtent {
            input,
            index,
            channels,
            samples: end,
        });
        extent.samples = extent.samples.max(end);
        let delay = &span.start_sample;
        let count = &span.sample_count;
        let label = format!("[a{ordinal}]");
        let gain = span.gain_centidb;
        let mut volume = String::new();
        if gain != 0 {
            let magnitude = gain.unsigned_abs();
            let sign = if gain < 0 { "-" } else { "" };
            volume = format!(
                "volume={sign}{}.{:02}dB:precision=float,",
                magnitude / 100,
                magnitude % 100
            );
        }
        chains.push(format!("[{input}:{index}]atrim=start_sample={start}:end_sample={end},asetpts=N/SR/TB,aresample={sample_rate},aformat=sample_fmts=s16:channel_layouts={layout},atrim=end_sample={count},{volume}adelay={delay}S:all=1{label}"));
        audio_labels.push_str(&label);
    }
    chains.push(format!("{audio_labels}amix=inputs={}:normalize=0:dropout_transition=0:duration=longest,atrim=end_sample={output_samples},aformat=sample_fmts=s16,asetpts=N/SR/TB[outa]", graph.audio.len() + 1));
    let script = chains.join(";\n");
    if script.len() > nw_domain::media::MAX_PLAN_BYTES {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Mixed filtergraph exceeds the reviewed media budget",
        ));
    }
    Ok(MixedFiltergraph {
        script,
        audio_extents: extents.into_values().collect(),
    })
}
