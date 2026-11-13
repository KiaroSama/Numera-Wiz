//! Real finite MediaV1 execution through shared supervision, staging and publication.
use super::verification::{self, AudioCheck, VideoCheck};
use crate::{
    Engine, ExecutionEvent, Observer, OutputDestination, PublicationStaging, VerifiedOutput,
    admission, blocking, prepare_owned_staging, progress, publish_output,
};
use nw_domain::{
    DomainError, Result,
    media::MediaPlanV1,
    media_execution::{
        AcceptedMediaPayload, MEDIA_OUTPUT_ARTIFACT, MEDIA_PUBLISH_STAGE, MEDIA_RENDER_STAGE,
        MEDIA_VERIFY_STAGE, MediaExecutionEvent, MediaExecutionFact, MediaExecutionOutcome,
        MediaResourceReservation, exact,
    },
    records::NativeObjectIdentity,
};
use nw_platform::{files, process};
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub type MediaObserver = Arc<
    dyn Fn(MediaExecutionEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync,
>;

pub struct MediaExecutionRequest<'a> {
    pub plan: &'a MediaPlanV1,
    pub job_id: &'a str,
    pub output: &'a Path,
    pub expected_parent: &'a NativeObjectIdentity,
    pub maximum_output_bytes: u64,
    pub cancel: CancellationToken,
    pub observer: MediaObserver,
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_PLAN", message)
}
fn failed(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}
async fn emit(
    observer: &MediaObserver,
    job_id: &str,
    stage_id: &str,
    fact: MediaExecutionFact,
) -> Result<()> {
    observer(MediaExecutionEvent {
        job_id: job_id.into(),
        stage_id: stage_id.into(),
        artifact_id: MEDIA_OUTPUT_ARTIFACT.into(),
        fact,
    })
    .await
}
fn shared_observer(observer: MediaObserver, job_id: &str) -> Observer {
    let job_id = job_id.to_owned();
    Arc::new(move |event| {
        let observer = observer.clone();
        let job_id = job_id.clone();
        Box::pin(async move {
            let (stage, fact) = match event {
                ExecutionEvent::StagingIntent(intent) => (
                    MEDIA_RENDER_STAGE,
                    MediaExecutionFact::StagingIntent(intent),
                ),
                ExecutionEvent::RemuxProgress(value) => {
                    (MEDIA_RENDER_STAGE, MediaExecutionFact::Progress(value))
                }
                ExecutionEvent::Fence(value) => {
                    (MEDIA_PUBLISH_STAGE, MediaExecutionFact::Fence(value))
                }
                ExecutionEvent::Published(identity, verification) => (
                    MEDIA_PUBLISH_STAGE,
                    MediaExecutionFact::Published {
                        identity,
                        verification,
                    },
                ),
                _ => return Err(invalid("Unexpected shared event for a generated media job")),
            };
            emit(&observer, &job_id, stage, fact).await
        })
    })
}

async fn components(engine: &Engine, cancel: &CancellationToken) -> Result<()> {
    let identities = [engine.ffmpeg.clone(), engine.ffprobe.clone()];
    let cancel = cancel.clone();
    blocking(move || {
        for identity in identities {
            files::revalidate(&identity, &cancel).map_err(|error| {
                if error.code == "CANCELLED" {
                    error
                } else {
                    DomainError::new(
                        "COMPONENT_CHANGED",
                        "Media component changed during execution",
                    )
                }
            })?;
        }
        Ok(())
    })
    .await
}

impl Engine {
    pub async fn execute_media_observed(
        &self,
        request: MediaExecutionRequest<'_>,
    ) -> Result<MediaExecutionOutcome> {
        let MediaExecutionRequest {
            plan,
            job_id,
            output,
            expected_parent,
            maximum_output_bytes,
            cancel,
            observer,
        } = request;
        plan.validate_hash()?;
        nw_domain::projects::validate_id(job_id)?;
        if expected_parent.schema_version != 1 {
            return Err(invalid(
                "Media output requires a current native parent identity",
            ));
        }
        // Recompute admission from the same typed payload; accepted raw bytes remain in Store.
        let payload = AcceptedMediaPayload::from_json(
            serde_json::to_string(plan)
                .map_err(|_| invalid("Media payload serialization failed"))?,
        )?;
        let resources = MediaResourceReservation::estimate(&payload)?;
        if maximum_output_bytes != exact(&resources.maximum_output_bytes)? {
            return Err(invalid(
                "Media output limit differs from its deterministic reservation",
            ));
        }
        let tools = self.qualify_media_compiler(cancel.clone()).await?;
        if serde_json::to_vec(&tools).map_err(|_| invalid("Component serialization failed"))?
            != serde_json::to_vec(&plan.payload.tools)
                .map_err(|_| invalid("Component serialization failed"))?
        {
            return Err(DomainError::new(
                "COMPONENT_CHANGED",
                "Qualified media tools differ from the accepted review",
            ));
        }
        let output_text = files::path_text(output)?;
        let parent_path = output
            .parent()
            .ok_or_else(|| invalid("Media output needs a parent"))?;
        let parent = files::DirectoryStamp::capture_resolved(parent_path)?;
        parent.require_object(expected_parent)?;
        let publication_observer = shared_observer(observer.clone(), job_id);
        let staging =
            prepare_owned_staging(&output_text, parent, job_id, &publication_observer).await?;
        let staging_name = files::path_text(staging.path())?;
        let mut args = plan.payload.argv_template.clone();
        if args
            .iter()
            .filter(|arg| arg.as_str() == "@owned_staging@")
            .count()
            != 1
            || args.last().map(String::as_str) != Some("@owned_staging@")
        {
            return Err(invalid(
                "Media lowering has an invalid owned output placeholder",
            ));
        }
        *args
            .last_mut()
            .ok_or_else(|| invalid("Missing media output"))? = staging_name;
        // Operational controls cannot introduce an input, semantic filter or alternate destination.
        args.splice(
            1..1,
            ["-progress", "pipe:1", "-stats_period", "0.2", "-fs"]
                .map(String::from)
                .into_iter()
                .chain([maximum_output_bytes.to_string()]),
        );
        emit(
            &observer,
            job_id,
            MEDIA_RENDER_STAGE,
            MediaExecutionFact::Prepared {
                executable_sha256: self.ffmpeg.sha256.clone(),
                argv: args.clone(),
            },
        )
        .await?;
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = components(self, &cancel).await {
            return ticket.finish(Err(error));
        }
        if let Err(error) = emit(
            &observer,
            job_id,
            MEDIA_RENDER_STAGE,
            MediaExecutionFact::Running,
        )
        .await
        {
            return ticket.finish(Err(error));
        }
        let run = progress::run_generated(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(u64::from(resources.render_timeout_seconds)),
            cancel.clone(),
            &publication_observer,
        )
        .await;
        if let Err(error) = ticket.finish(run) {
            let (_, retained) = staging
                .keep()
                .map_err(|error| files::io_error(error.error))?;
            tracing::warn!(staging=%retained.display(), "Owned media staging retained after process failure; no output was published");
            return Err(error);
        }
        components(self, &cancel).await?;
        let path = staging.path().to_owned();
        let original = staging.as_file().try_clone().map_err(files::io_error)?;
        // Hard-link creation changes the file's native change stamp. Create our
        // retained link before capturing and verifying the immutable read stamps.
        // Its name grants no read authority: only the later durable fence does.
        let (staging, proof, original_stamp, read_stamp) = blocking(move || {
            let staging = nw_platform::publication::RetainedStaging::new(staging)?;
            let proof_name = format!(".numera-proof-{}", uuid::Uuid::new_v4());
            let proof = staging.create_proof(std::ffi::OsStr::new(&proof_name))?;
            original.sync_all().map_err(files::io_error)?;
            let length = original.metadata().map_err(files::io_error)?.len();
            if length == 0 || length > maximum_output_bytes {
                return Err(failed(
                    "Generated media output is empty or exceeds its admitted byte limit",
                ));
            }
            let original_stamp = files::SourceStamp::capture_open_resolved(&path, original)?;
            let read_stamp = files::SourceStamp::capture_resolved(&path)?;
            read_stamp.require_object(&original_stamp.object_identity()?)?;
            Ok((staging, proof, original_stamp, read_stamp))
        })
        .await?;
        emit(
            &observer,
            job_id,
            MEDIA_VERIFY_STAGE,
            MediaExecutionFact::Verifying,
        )
        .await?;
        let timeout = Duration::from_secs(u64::from(resources.verify_timeout_seconds));
        let probe = metadata_probe(self, &read_stamp, job_id, timeout, &cancel, &observer).await?;
        verification::metadata(plan, &probe)?;
        let video = Arc::new(Mutex::new(VideoCheck::new(plan)?));
        let consumer = video.clone();
        decode_stream(
            self,
            &read_stamp,
            DecodeRequest {
                args: vec![
                    "-map",
                    "0:v:0",
                    "-c:v",
                    "rawvideo",
                    "-threads:v",
                    "1",
                    "-pix_fmt",
                    "bgr0",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
                consumer: Box::new(move |bytes| {
                    consumer
                        .lock()
                        .map_err(|_| failed("Video verifier lock poisoned"))?
                        .feed(bytes)
                }),
                job_id,
                timeout,
                cancel: &cancel,
                observer: &observer,
            },
        )
        .await?;
        video
            .lock()
            .map_err(|_| failed("Video verifier lock poisoned"))?
            .finish()?;
        let channels = match plan.payload.capture.settings.audio_layout {
            nw_domain::media::AudioLayout::Mono => 1,
            nw_domain::media::AudioLayout::Stereo => 2,
        };
        let audio = Arc::new(Mutex::new(AudioCheck::new(
            exact(&plan.payload.output.audio_samples)?,
            channels,
        )?));
        let consumer = audio.clone();
        decode_stream(
            self,
            &read_stamp,
            DecodeRequest {
                args: vec![
                    "-map",
                    "0:a:0",
                    "-c:a",
                    "pcm_s16le",
                    "-threads:a",
                    "1",
                    "-f",
                    "s16le",
                    "pipe:1",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
                consumer: Box::new(move |bytes| {
                    consumer
                        .lock()
                        .map_err(|_| failed("PCM verifier lock poisoned"))?
                        .feed(bytes)
                }),
                job_id,
                timeout,
                cancel: &cancel,
                observer: &observer,
            },
        )
        .await?;
        audio
            .lock()
            .map_err(|_| failed("PCM verifier lock poisoned"))?
            .finish()?;
        components(self, &cancel).await?;
        let hash_cancel = cancel.clone();
        let (identity, read_stamp) = blocking(move || {
            let identity = read_stamp.identity_with_limit(&hash_cancel, maximum_output_bytes)?;
            Ok((identity, read_stamp))
        })
        .await?;
        original_stamp.revalidate()?;
        let parent = files::DirectoryStamp::capture_resolved(parent_path)?;
        parent.require_object(expected_parent)?;
        let verification = format!(
            "Full FFV1/BGR0 corner-color and PCM silence decode: {} exact-grid frames, {} samples; stream/color/container/timestamps verified",
            plan.payload.output.video_frames, plan.payload.output.audio_samples,
        );
        let artifact = publish_output(
            VerifiedOutput {
                file: PublicationStaging::Linked {
                    file: staging,
                    proof,
                },
                identity,
                sources: vec![original_stamp, read_stamp],
            },
            OutputDestination {
                path: output_text,
                parent,
                parent_object: expected_parent.clone(),
            },
            job_id,
            verification.clone(),
            cancel,
            publication_observer,
        )
        .await?;
        Ok(MediaExecutionOutcome {
            artifact,
            verification,
        })
    }
}

fn decoder_args() -> Vec<String> {
    [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-max_alloc",
        "67108864",
        "-threads",
        "1",
        "-filter_threads",
        "1",
    ]
    .map(String::from)
    .to_vec()
}
fn held_input(stamp: &files::SourceStamp, cancel: &CancellationToken) -> Result<admission::Input> {
    let input = admission::from_file(stamp.try_clone_file()?, cancel)?;
    if input.demuxer != "matroska" {
        return Err(failed("Generated staging is not admitted Matroska"));
    }
    Ok(input)
}

async fn metadata_probe(
    engine: &Engine,
    stamp: &files::SourceStamp,
    job_id: &str,
    timeout: Duration,
    cancel: &CancellationToken,
    observer: &MediaObserver,
) -> Result<serde_json::Value> {
    let input = held_input(stamp, cancel)?;
    let mut args = [
        "-v", "error", "-max_alloc", "67108864", "-threads", "1",
        "-probesize", "5242880", "-analyzeduration", "5000000",
        "-show_streams", "-show_frames", "-show_format", "-show_entries",
        "stream=index,codec_type,codec_name,width,height,pix_fmt,sample_rate,channels,sample_fmt,time_base,color_range,color_space,color_transfer,color_primaries:frame=media_type,stream_index,pts,nb_samples:format=format_name,duration",
        "-of", "json=compact=1:string_validation=fail",
    ].map(String::from).to_vec();
    args.extend(input.options());
    args.extend(["-i", "fd:"].map(String::from));
    emit(
        observer,
        job_id,
        MEDIA_VERIFY_STAGE,
        MediaExecutionFact::Prepared {
            executable_sha256: engine.ffprobe.sha256.clone(),
            argv: args.clone(),
        },
    )
    .await?;
    let ticket = engine.process_budget.acquire(cancel).await?;
    if let Err(error) = components(engine, cancel)
        .await
        .and_then(|_| stamp.revalidate())
    {
        return ticket.finish(Err(error));
    }
    let metadata = Arc::new(Mutex::new(Vec::new()));
    let captured = metadata.clone();
    let result = process::stream_readonly_with_output_limits(
        Path::new(&engine.ffprobe.path),
        &args,
        timeout,
        input.file,
        4096,
        Box::new(move |bytes| {
            let mut captured = captured
                .lock()
                .map_err(|_| failed("Metadata verifier lock poisoned"))?;
            if captured
                .len()
                .checked_add(bytes.len())
                .is_none_or(|length| length > 4 * 1024 * 1024)
            {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Decoded metadata exceeds 4 MiB",
                ));
            }
            captured.extend_from_slice(bytes);
            Ok(())
        }),
        cancel.clone(),
    )
    .await;
    ticket.finish(result)?;
    stamp.revalidate()?;
    let metadata = metadata
        .lock()
        .map_err(|_| failed("Metadata verifier lock poisoned"))?;
    serde_json::from_slice(&metadata).map_err(|_| failed("Invalid bounded decoded metadata"))
}

struct DecodeRequest<'a> {
    args: Vec<String>,
    consumer: process::ChunkConsumer,
    job_id: &'a str,
    timeout: Duration,
    cancel: &'a CancellationToken,
    observer: &'a MediaObserver,
}
async fn decode_stream(
    engine: &Engine,
    stamp: &files::SourceStamp,
    request: DecodeRequest<'_>,
) -> Result<()> {
    let input = held_input(stamp, request.cancel)?;
    let mut args = decoder_args();
    args.extend(input.options());
    args.extend(["-i", "fd:"].map(String::from));
    args.extend(request.args);
    emit(
        request.observer,
        request.job_id,
        MEDIA_VERIFY_STAGE,
        MediaExecutionFact::Prepared {
            executable_sha256: engine.ffmpeg.sha256.clone(),
            argv: args.clone(),
        },
    )
    .await?;
    let ticket = engine.process_budget.acquire(request.cancel).await?;
    if let Err(error) = components(engine, request.cancel)
        .await
        .and_then(|_| stamp.revalidate())
    {
        return ticket.finish(Err(error));
    }
    let result = process::stream_readonly_with_output_limits(
        Path::new(&engine.ffmpeg.path),
        &args,
        request.timeout,
        input.file,
        4096,
        request.consumer,
        request.cancel.clone(),
    )
    .await;
    ticket.finish(result)?;
    stamp.revalidate()
}
