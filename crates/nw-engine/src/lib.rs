//! FFmpeg-only media work. Utilities share the same planner and verifier.
use nw_domain::{
    DomainError, Result,
    mux::{self, CleanAction, CleanIntent, Stream, StreamKind},
    records::{Asset, FileIdentity, MediaProbe, Plan},
};
use nw_platform::files::{self, io_error};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    Prepared(Vec<String>),
    Verifying,
    Fence(nw_domain::access::Publication),
    Published(FileIdentity, String),
}
pub type Observer =
    Arc<dyn Fn(ExecutionEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

const FORMATS: &str = "mov,matroska,webm,avi,wav,flac,mp3,ogg";
#[derive(Clone)]
pub struct Engine {
    pub ffmpeg: FileIdentity,
    pub ffprobe: FileIdentity,
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| DomainError::new("WORKER_FAILED", e.to_string()))?
}
impl Engine {
    pub async fn new(ffmpeg: PathBuf, ffprobe: PathBuf) -> Result<Self> {
        let pair = blocking(move || {
            Ok((
                files::identity(&ffmpeg, &CancellationToken::new())?,
                files::identity(&ffprobe, &CancellationToken::new())?,
            ))
        })
        .await?;
        Ok(Self {
            ffmpeg: pair.0,
            ffprobe: pair.1,
        })
    }
    pub async fn probe(&self, path: &Path, cancel: CancellationToken) -> Result<MediaProbe> {
        let canonical = files::source_path(path)?;
        let path = files::path_text(&canonical)?;
        let args = [
            "-v",
            "error",
            "-max_alloc",
            "67108864",
            "-probesize",
            "5242880",
            "-analyzeduration",
            "5000000",
            "-protocol_whitelist",
            "file",
            "-format_whitelist",
            FORMATS,
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-of",
            "json",
            "-i",
            &path,
        ]
        .map(String::from)
        .to_vec();
        let result = nw_platform::process::run(
            Path::new(&self.ffprobe.path),
            &args,
            Duration::from_secs(120),
            4 * 1024 * 1024,
            cancel,
        )
        .await?;
        let value: Value = serde_json::from_slice(&result.stdout)
            .map_err(|e| DomainError::new("INVALID_PROBE", e.to_string()))?;
        parse_probe(value)
    }
    pub async fn register(
        &self,
        path: PathBuf,
        creator: String,
        cancel: CancellationToken,
    ) -> Result<Asset> {
        let token = cancel.clone();
        let initial = blocking(move || files::identity(&path, &token)).await?;
        let probe = self.probe(Path::new(&initial.path), cancel.clone()).await?;
        let expected = initial.clone();
        blocking(move || files::revalidate(&expected, &cancel)).await?;
        Ok(Asset {
            display_name: String::new(),
            id: Uuid::new_v4().to_string(),
            creator,
            identity: initial,
            probe,
        })
    }
    pub fn clean_plan(
        &self,
        asset: Asset,
        output: &Path,
        intent: &CleanIntent,
        creator: &str,
    ) -> Result<Plan> {
        let output = files::output_path(output)?;
        if output == Path::new(&asset.identity.path) {
            return Err(DomainError::new(
                "SOURCE_OUTPUT_ALIAS",
                "Output must be distinct from the source",
            ));
        }
        let resolution = mux::resolve(&asset.probe.streams, intent)?;
        if resolution.action == CleanAction::Remux {
            admit_matroska(&output, &resolution.streams)?;
        }
        if resolution.action == CleanAction::CopyUnchanged
            && Path::new(&asset.identity.path).extension() != output.extension()
        {
            return Err(DomainError::new(
                "CONTAINER_CHANGE_REQUIRED",
                "Byte copy requires the original container extension",
            ));
        }
        let args = if resolution.action == CleanAction::Remux {
            lower_remux(&asset, &resolution)?
        } else {
            vec![]
        };
        let now = nw_platform::epoch_seconds();
        let mut plan=Plan{id:Uuid::new_v4().to_string(),creator:creator.into(),hash:String::new(),source:asset,output:files::path_text(&output)?,ffmpeg:self.ffmpeg.clone(),ffprobe:self.ffprobe.clone(),resolution,argv_template:args,created_at:now.to_string(),expires_at:(now+1800).to_string(),verification_scope:"Full byte digest for unchanged copy; stream/codec/tag/disposition/chapter structure for remux. Packet/decoded/timing preservation qualification remains pending.".into()};
        plan.hash = plan_digest(&plan)?;
        Ok(plan)
    }
    pub async fn execute(
        &self,
        plan: &Plan,
        cancel: CancellationToken,
    ) -> Result<(Option<FileIdentity>, String, Option<Vec<String>>)> {
        if plan.expires_at.parse::<u64>().unwrap_or(0) <= nw_platform::epoch_seconds() {
            return Err(DomainError::new(
                "PLAN_EXPIRED",
                "Review a fresh plan before accepting execution",
            ));
        }
        self.execute_observed(
            plan,
            cancel,
            "untracked",
            Arc::new(|_| Box::pin(async { Ok(()) })),
        )
        .await
    }
    pub async fn execute_observed(
        &self,
        plan: &Plan,
        cancel: CancellationToken,
        job_id: &str,
        observer: Observer,
    ) -> Result<(Option<FileIdentity>, String, Option<Vec<String>>)> {
        if plan_digest(plan)? != plan.hash {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Stored plan hash mismatch",
            ));
        }
        if self.ffmpeg != plan.ffmpeg || self.ffprobe != plan.ffprobe {
            return Err(DomainError::new(
                "COMPONENT_CHANGED",
                "Execution engine does not match the reviewed component identities",
            ));
        }
        let source = plan.source.identity.clone();
        let ffmpeg = plan.ffmpeg.clone();
        let ffprobe = plan.ffprobe.clone();
        let token = cancel.clone();
        blocking(move || {
            files::revalidate(&source, &token)?;
            files::revalidate(&ffmpeg, &token)?;
            files::revalidate(&ffprobe, &token)
        })
        .await?;
        files::output_path(Path::new(&plan.output))?;
        match plan.resolution.action {
            CleanAction::Skip => Ok((
                None,
                "Explicit no-match skip; no output artifact was produced".into(),
                None,
            )),
            CleanAction::CopyUnchanged => {
                observer(ExecutionEvent::Prepared(vec![
                    "rust:verified-byte-copy".into(),
                    plan.source.identity.path.clone(),
                    plan.output.clone(),
                ]))
                .await?;
                let source = plan.source.identity.clone();
                let output = PathBuf::from(&plan.output);
                let token = cancel.clone();
                let (staging, actual) =
                    blocking(move || files::prepare_verified_copy(&source, &output, &token))
                        .await?;
                observer(ExecutionEvent::Verifying).await?;
                let verification =
                    "Full byte SHA-256 and length verified before no-replace publication"
                        .to_owned();
                let artifact = publish(
                    staging,
                    actual,
                    plan,
                    job_id,
                    verification.clone(),
                    cancel,
                    observer,
                )
                .await?;
                Ok((Some(artifact), verification, None))
            }
            CleanAction::Remux => self.remux(plan, cancel, job_id, observer).await,
        }
    }
    async fn remux(
        &self,
        plan: &Plan,
        cancel: CancellationToken,
        job_id: &str,
        observer: Observer,
    ) -> Result<(Option<FileIdentity>, String, Option<Vec<String>>)> {
        let output = PathBuf::from(&plan.output);
        let parent = output
            .parent()
            .ok_or_else(|| DomainError::new("PATH_DENIED", "Missing output parent"))?;
        let staging = tempfile::Builder::new()
            .prefix(".numera-owned-")
            .suffix(".mkv")
            .tempfile_in(parent)
            .map_err(io_error)?;
        let staging_name = files::path_text(staging.path())?;
        let args = plan
            .argv_template
            .iter()
            .map(|v| {
                if v == "@owned_staging@" {
                    staging_name.clone()
                } else {
                    v.clone()
                }
            })
            .collect::<Vec<_>>();
        observer(ExecutionEvent::Prepared(args.clone())).await?;
        if let Err(error) = nw_platform::process::run(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(10800),
            8 * 1024 * 1024,
            cancel.clone(),
        )
        .await
        {
            let (_, retained) = staging.keep().map_err(|e| io_error(e.error))?;
            tracing::warn!(staging=%retained.display(),"Owned staging retained after process failure; no output was published");
            return Err(error);
        }
        staging.as_file().sync_all().map_err(io_error)?;
        if staging.as_file().metadata().map_err(io_error)?.len() == 0 {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                "This attempt produced no new staging bytes",
            ));
        }
        observer(ExecutionEvent::Verifying).await?;
        let observed = self.probe(staging.path(), cancel.clone()).await?;
        verify_structure(plan, &observed)?;
        let stage_path = staging.path().to_owned();
        let token = cancel.clone();
        let actual = blocking(move || files::identity(&stage_path, &token)).await?;
        let source = plan.source.identity.clone();
        let token = cancel.clone();
        blocking(move || files::revalidate(&source, &token)).await?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Cancelled before publication fence",
            ));
        }
        let artifact = publish(
            staging,
            actual,
            plan,
            job_id,
            plan.verification_scope.clone(),
            cancel,
            observer,
        )
        .await?;
        Ok((Some(artifact), plan.verification_scope.clone(), Some(args)))
    }
}
async fn publish(
    staging: tempfile::NamedTempFile,
    actual: FileIdentity,
    plan: &Plan,
    job_id: &str,
    verification: String,
    cancel: CancellationToken,
    observer: Observer,
) -> Result<FileIdentity> {
    let output = PathBuf::from(&plan.output);
    let parent = output
        .parent()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Output parent required"))?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Cancelled before publication fence",
        ));
    }
    let proof = parent.join(format!(".numera-proof-{}", Uuid::new_v4()));
    std::fs::hard_link(staging.path(), &proof).map_err(io_error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    let artifact = FileIdentity {
        path: plan.output.clone(),
        bytes: actual.bytes,
        sha256: actual.sha256,
    };
    let journal = nw_domain::access::Publication {
        job_id: job_id.into(),
        proof: files::path_text(&proof)?,
        output: plan.output.clone(),
        identity: artifact.clone(),
        verified: verification.clone(),
        published: false,
    };
    if let Err(error) = observer(ExecutionEvent::Fence(journal)).await {
        let _ = staging.keep();
        return Err(error);
    }
    // The committed fence is the cancellation boundary. Recovery retains the hard-link proof until durable acknowledgement.
    staging
        .persist_noclobber(&output)
        .map_err(|error| DomainError::new("OUTPUT_COLLISION", error.error.to_string()))?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    observer(ExecutionEvent::Published(artifact.clone(), verification)).await?;
    std::fs::remove_file(&proof).map_err(io_error)?;
    Ok(artifact)
}
fn parse_tags(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_lowercase(), s.into())))
                .collect()
        })
        .unwrap_or_default()
}
pub fn parse_probe(value: Value) -> Result<MediaProbe> {
    let rows = value["streams"]
        .as_array()
        .ok_or_else(|| DomainError::new("INVALID_PROBE", "Probe did not return stream records"))?;
    if rows.is_empty() || rows.len() > 256 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Probe must contain 1-256 streams",
        ));
    }
    let mut streams = Vec::new();
    for row in rows {
        let index = row["index"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| DomainError::new("INVALID_PROBE", "Invalid source index"))?;
        let kind = match row["codec_type"].as_str().unwrap_or("") {
            "video" => StreamKind::Video,
            "audio" => StreamKind::Audio,
            "subtitle" => StreamKind::Subtitle,
            "attachment" => StreamKind::Attachment,
            "data" => StreamKind::Data,
            _ => StreamKind::Unknown,
        };
        let dispositions = row["disposition"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| v.as_i64() == Some(1))
                    .map(|(k, _)| k.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        streams.push(Stream {
            index,
            kind,
            codec: row["codec_name"].as_str().unwrap_or("unknown").into(),
            tags: parse_tags(&row["tags"]),
            dispositions,
        });
    }
    let format = value["format"]["format_name"]
        .as_str()
        .ok_or_else(|| DomainError::new("INVALID_PROBE", "Missing container format"))?
        .into();
    Ok(MediaProbe {
        streams,
        format,
        global_tags: parse_tags(&value["format"]["tags"]),
        chapters: value
            .get("chapters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
        verification_scope: "Bounded metadata probe; not a full decode or packet scan".into(),
    })
}
pub fn plan_digest(plan: &Plan) -> Result<String> {
    let mut copy = plan.clone();
    copy.hash.clear();
    serde_json::to_vec(&copy)
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .map_err(|e| DomainError::new("INVALID_PLAN", e.to_string()))
}
fn admit_matroska(output: &Path, streams: &[Stream]) -> Result<()> {
    if output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        != Some("mkv".into())
    {
        return Err(DomainError::new(
            "CONTAINER_UNQUALIFIED",
            "Development remux profile currently admits Matroska .mkv only; additional muxers remain required",
        ));
    }
    for stream in streams {
        let admitted = match stream.kind {
            StreamKind::Video => {
                !stream.attached_picture()
                    && [
                        "h264",
                        "hevc",
                        "av1",
                        "vp8",
                        "vp9",
                        "mpeg4",
                        "mpeg2video",
                        "ffv1",
                        "mjpeg",
                    ]
                    .contains(&stream.codec.as_str())
            }
            StreamKind::Audio => [
                "aac",
                "mp3",
                "flac",
                "opus",
                "vorbis",
                "ac3",
                "eac3",
                "dts",
                "pcm_s16le",
                "pcm_s24le",
                "pcm_s32le",
                "pcm_f32le",
            ]
            .contains(&stream.codec.as_str()),
            StreamKind::Subtitle => [
                "subrip",
                "ass",
                "ssa",
                "webvtt",
                "hdmv_pgs_subtitle",
                "dvd_subtitle",
            ]
            .contains(&stream.codec.as_str()),
            StreamKind::Attachment => {
                stream.tags.contains_key("filename") && stream.tags.contains_key("mimetype")
            }
            StreamKind::Data | StreamKind::Unknown => false,
        };
        if !admitted {
            return Err(DomainError::new(
                "INCOMPATIBLE_PRESERVATION",
                format!(
                    "Retained source stream {} ({:?}/{}) has no admitted Matroska copy route",
                    stream.index, stream.kind, stream.codec
                ),
            ));
        }
    }
    Ok(())
}
fn lower_remux(asset: &Asset, resolution: &mux::CleanResolution) -> Result<Vec<String>> {
    let mut args = [
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-v",
        "error",
        "-max_alloc",
        "67108864",
        "-y",
        "-copyts",
        "-start_at_zero",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        FORMATS,
        "-i",
        &asset.identity.path,
    ]
    .map(String::from)
    .to_vec();
    for stream in &resolution.streams {
        args.extend(["-map".into(), format!("0:{}", stream.index)]);
    }
    args.extend([
        "-c".into(),
        "copy".into(),
        "-map_metadata".into(),
        if resolution.preservation.global_tags {
            "0".into()
        } else {
            "-1".into()
        },
        "-map_chapters".into(),
        if resolution.preservation.chapters {
            "0".into()
        } else {
            "-1".into()
        },
    ]);
    for (index, stream) in resolution.streams.iter().enumerate() {
        args.extend([
            format!("-map_metadata:s:{index}"),
            if resolution.preservation.stream_tags {
                format!("0:s:{}", stream.index)
            } else {
                "-1".into()
            },
        ]);
        let original = asset
            .probe
            .streams
            .iter()
            .find(|s| s.index == stream.index)
            .ok_or_else(|| DomainError::new("INVALID_PLAN", "Missing source stream"))?;
        for tag in original
            .tags
            .keys()
            .chain(stream.tags.keys())
            .collect::<BTreeSet<_>>()
        {
            if original.tags.get(tag) != stream.tags.get(tag)
                || !resolution.preservation.stream_tags
            {
                args.extend([
                    format!("-metadata:s:{index}"),
                    format!("{tag}={}", stream.tags.get(tag).map_or("", String::as_str)),
                ]);
            }
        }
        args.extend([
            format!("-disposition:{index}"),
            if stream.dispositions.is_empty() {
                "0".into()
            } else {
                stream
                    .dispositions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("+")
            },
        ]);
    }
    args.extend(["-f".into(), "matroska".into(), "@owned_staging@".into()]);
    Ok(args)
}
fn verify_structure(plan: &Plan, actual: &MediaProbe) -> Result<()> {
    if actual.streams.len() != plan.resolution.streams.len() {
        return Err(DomainError::new(
            "VERIFICATION_FAILED",
            "Output stream count differs from the frozen outcome manifest",
        ));
    }
    for (expected, observed) in plan.resolution.streams.iter().zip(&actual.streams) {
        if expected.kind != observed.kind
            || expected.codec != observed.codec
            || expected.dispositions != observed.dispositions
        {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                format!(
                    "Output stream {} kind/codec/disposition mismatch",
                    observed.index
                ),
            ));
        }
        for key in ["language", "title", "filename", "mimetype"] {
            if expected.tags.get(key) != observed.tags.get(key) {
                return Err(DomainError::new(
                    "VERIFICATION_FAILED",
                    format!("Output stream {} tag {key} mismatch", observed.index),
                ));
            }
        }
    }
    if !plan.resolution.preservation.chapters
        && actual.chapters.as_array().is_some_and(|v| !v.is_empty())
    {
        return Err(DomainError::new(
            "VERIFICATION_FAILED",
            "Chapter removal was not honored",
        ));
    }
    if plan.resolution.preservation.chapters && actual.chapters != plan.source.probe.chapters {
        return Err(DomainError::new(
            "VERIFICATION_FAILED",
            "Preserved chapters differ from the frozen source snapshot",
        ));
    }
    for (key, value) in &plan.source.probe.global_tags {
        if ["encoder", "duration", "creation_time"].contains(&key.as_str()) {
            continue;
        }
        if plan.resolution.preservation.global_tags && actual.global_tags.get(key) != Some(value) {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                format!("Global tag {key} was not retained"),
            ));
        }
        if !plan.resolution.preservation.global_tags && actual.global_tags.contains_key(key) {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                format!("Global tag {key} was not stripped"),
            ));
        }
    }
    Ok(())
}
