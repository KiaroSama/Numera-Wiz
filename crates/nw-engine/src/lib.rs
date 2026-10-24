//! FFmpeg-only media work. Utilities share the same planner and verifier.
mod admission;
mod budget;
mod preservation;
use nw_domain::{
    DomainError, Result,
    mux::{self, CleanAction, CleanIntent, Stream, StreamKind},
    records::{Asset, FileIdentity, MediaProbe, Plan},
};
use nw_platform::files::{self, io_error};
use serde_json::Value;
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
    StagingIntent(nw_domain::records::StagingIntent),
    Prepared(Vec<String>),
    Verifying,
    Fence(nw_domain::access::Publication),
    Published(FileIdentity, String),
}
pub type Observer =
    Arc<dyn Fn(ExecutionEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct Engine {
    pub ffmpeg: FileIdentity,
    pub ffprobe: FileIdentity,
    process_budget: budget::ProcessBudget,
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
            process_budget: budget::ProcessBudget::default(),
        })
    }
    pub async fn probe(&self, path: &Path, cancel: CancellationToken) -> Result<MediaProbe> {
        let canonical = files::source_path(path)?;
        let admission_cancel = cancel.clone();
        let input = blocking(move || admission::open(&canonical, &admission_cancel)).await?;
        let mut args = [
            "-v",
            "error",
            "-max_alloc",
            "67108864",
            "-probesize",
            "5242880",
            "-analyzeduration",
            "5000000",
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-show_data_hash",
            "sha256",
            "-of",
            "json=string_validation=fail",
        ]
        .map(String::from)
        .to_vec();
        args.extend(input.options());
        args.extend(["-i".into(), "fd:".into()]);
        let ticket = self.process_budget.acquire(&cancel).await?;
        let result = nw_platform::process::run_readonly(
            Path::new(&self.ffprobe.path),
            &args,
            Duration::from_secs(120),
            4 * 1024 * 1024,
            input.file,
            cancel,
        )
        .await;
        let result = ticket.finish(result)?;
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
    pub fn media_process_capacity(&self) -> (usize, usize, usize) {
        self.process_budget.status()
    }
    pub fn clean_plan(
        &self,
        asset: Asset,
        output: &Path,
        intent: &CleanIntent,
        creator: &str,
    ) -> Result<Plan> {
        self.clean_plan_at(asset, output, intent, creator, nw_platform::epoch_seconds())
    }
    pub fn clean_plan_at(
        &self,
        asset: Asset,
        output: &Path,
        intent: &CleanIntent,
        creator: &str,
        now: u64,
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
        let mut plan = Plan {
            schema_version: 2,
            required_rights: Some(nw_domain::plans::RequiredRights::native(
                asset.identity.clone(),
                files::path_text(&output)?,
            )),
            creation_audit: Some(nw_domain::plans::CreationAudit {
                principal: creator.into(),
                policy_revision: 0,
            }),
            id: Uuid::new_v4().to_string(),
            creator: creator.into(),
            hash: String::new(),
            source: asset,
            output: files::path_text(&output)?,
            ffmpeg: self.ffmpeg.clone(),
            ffprobe: self.ffprobe.clone(),
            resolution,
            argv_template: args,
            created_at: now.to_string(),
            expires_at: now
                .checked_add(1800)
                .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Plan clock range exhausted"))?
                .to_string(),
            verification_scope: preservation::SCOPE.into(),
        };
        plan.hash = plan_digest(&plan)?;
        Ok(plan)
    }
    pub fn validate_plan(&self, plan: &Plan) -> Result<()> {
        if plan_digest(plan)? != plan.hash {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Stored plan hash mismatch",
            ));
        }
        if plan.verification_scope != preservation::SCOPE {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Review a fresh plan for the current verification contract",
            ));
        }
        if self.ffmpeg != plan.ffmpeg || self.ffprobe != plan.ffprobe {
            return Err(DomainError::new(
                "COMPONENT_CHANGED",
                "Execution engine does not match the reviewed component identities",
            ));
        }
        if plan.resolution.action == CleanAction::Remux
            && plan.argv_template != lower_remux(&plan.source, &plan.resolution)?
        {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Review a fresh plan for the admitted held-file remux route",
            ));
        }
        if plan.resolution.action != CleanAction::Remux && !plan.argv_template.is_empty() {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Copy/skip plans cannot contain a media subprocess",
            ));
        }
        Ok(())
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
        self.validate_plan(plan)?;
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
        files::revalidate_output(&plan.output)?;
        match plan.resolution.action {
            CleanAction::Skip => Ok((
                None,
                "Explicit no-match skip; no output artifact was produced".into(),
                None,
            )),
            CleanAction::CopyUnchanged => {
                let staging = prepare_staging(plan, job_id, &observer).await?;
                observer(ExecutionEvent::Prepared(vec![
                    "rust:verified-byte-copy".into(),
                    plan.source.identity.path.clone(),
                    plan.output.clone(),
                ]))
                .await?;
                let source = plan.source.identity.clone();
                let token = cancel.clone();
                let (staging, actual) =
                    blocking(move || files::copy_into_staging(&source, staging, &token)).await?;
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
        let source = plan.source.identity.clone();
        let token = cancel.clone();
        let frozen_demuxer = admission::frozen_demuxer(&plan.source.probe.format)?;
        let (input, source_stamp) = blocking(move || {
            let path = Path::new(&source.path);
            let input = admission::open(path, &token)?;
            if input.demuxer != frozen_demuxer {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Held input format differs from the reviewed source",
                ));
            }
            let stamp = files::SourceStamp::capture_open_resolved(
                path,
                input.file.try_clone().map_err(io_error)?,
            )?;
            files::revalidate(&source, &token)?;
            stamp.revalidate()?;
            Ok((input, stamp))
        })
        .await?;
        let staging = prepare_staging(plan, job_id, &observer).await?;
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
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = source_stamp.revalidate() {
            return ticket.finish(Err(error));
        }
        let process = nw_platform::process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(10800),
            8 * 1024 * 1024,
            input.file,
            cancel.clone(),
        )
        .await;
        if let Err(error) = ticket.finish(process) {
            let (_, retained) = staging.keep().map_err(|e| io_error(e.error))?;
            tracing::warn!(staging=%retained.display(),"Owned staging retained after process failure; no output was published");
            return Err(error);
        }
        source_stamp.revalidate()?;
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
        let verification = preservation::verify(
            self,
            plan,
            staging.path(),
            &observed,
            cancel.clone(),
            &observer,
        )
        .await?;
        let stage_path = staging.path().to_owned();
        let token = cancel.clone();
        let actual = blocking(move || files::identity(&stage_path, &token)).await?;
        let source = plan.source.identity.clone();
        let token = cancel.clone();
        blocking(move || {
            files::revalidate(&source, &token)?;
            source_stamp.revalidate()
        })
        .await?;
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
            verification.clone(),
            cancel,
            observer,
        )
        .await?;
        Ok((Some(artifact), verification, Some(args)))
    }
}
async fn prepare_staging(
    plan: &Plan,
    job_id: &str,
    observer: &Observer,
) -> Result<tempfile::NamedTempFile> {
    let output = files::revalidate_output(&plan.output)?;
    let parent = output
        .parent()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Output parent required"))?;
    let directory = parent.join(format!(".numera-job-{}", Uuid::new_v4()));
    let payload = directory.join("payload.mkv");
    observer(ExecutionEvent::StagingIntent(
        nw_domain::records::StagingIntent {
            job_id: job_id.into(),
            directory: files::path_text(&directory)?,
            payload: files::path_text(&payload)?,
        },
    ))
    .await?;
    blocking(move || {
        // The unpredictable workspace is journaled before any directory/file is created.
        if directory.try_exists().map_err(io_error)? {
            return Err(DomainError::new(
                "OUTPUT_COLLISION",
                "Staging workspace already exists",
            ));
        }
        let directory = nw_platform::security::private_directory(&directory)?;
        #[cfg(unix)]
        std::fs::File::open(directory.parent().expect("Staging parent"))
            .and_then(|file| file.sync_all())
            .map_err(io_error)?;
        let file = tempfile::Builder::new()
            .prefix("payload")
            .rand_bytes(0)
            .suffix(".mkv")
            .tempfile_in(&directory)
            .map_err(io_error)?;
        if file.path() != payload {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Staging workspace resolved to a different parent",
            ));
        }
        Ok(file)
    })
    .await
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
    let output = files::revalidate_output(&plan.output)?;
    #[cfg(unix)]
    let parent = output
        .parent()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Output parent required"))?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Cancelled before publication fence",
        ));
    }
    let workspace = staging
        .path()
        .parent()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Owned staging workspace required"))?
        .to_owned();
    let proof = workspace.join(format!(".numera-proof-{}", Uuid::new_v4()));
    std::fs::hard_link(staging.path(), &proof).map_err(io_error)?;
    #[cfg(unix)]
    std::fs::File::open(&workspace)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    let artifact = FileIdentity {
        path: plan.output.clone(),
        bytes: actual.bytes,
        sha256: actual.sha256,
    };
    let journal = nw_domain::access::Publication {
        read_purpose_version: 1,
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
    // The committed fence is the cancellation boundary. The retained hard link proves the published object for later job-scoped byte reads.
    staging
        .persist_noclobber(&output)
        .map_err(|error| DomainError::new("OUTPUT_COLLISION", error.error.to_string()))?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    observer(ExecutionEvent::Published(artifact.clone(), verification)).await?;
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
        stream_details: rows
            .iter()
            .map(|row| {
                (
                    row["index"].as_u64().unwrap_or(u64::MAX) as u32,
                    row.clone(),
                )
            })
            .collect(),
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
    nw_domain::plans::digest(plan)
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
    let demuxer = admission::frozen_demuxer(&asset.probe.format)?;
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
        "-avoid_negative_ts",
        "disabled",
    ]
    .map(String::from)
    .to_vec();
    args.extend(admission::options(demuxer));
    args.extend(["-i".into(), "fd:".into()]);
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
        verify_tags(
            &expected.tags,
            &observed.tags,
            &format!("Output stream {}", observed.index),
            false,
        )?;
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
    let empty = BTreeMap::new();
    let expected = if plan.resolution.preservation.global_tags {
        &plan.source.probe.global_tags
    } else {
        &empty
    };
    verify_tags(expected, &actual.global_tags, "Container", true)?;
    Ok(())
}

fn verify_tags(
    expected: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
    target: &str,
    generated_writer: bool,
) -> Result<()> {
    for (key, value) in expected {
        // Matroska regenerates its duration tags from the independently verified packet clock.
        let undefined_stream_language =
            !generated_writer && key == "language" && value == "und" && !actual.contains_key(key);
        if key != "duration" && !undefined_stream_language && actual.get(key) != Some(value) {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                format!("{target} tag {key} did not match its frozen value"),
            ));
        }
    }
    for key in actual.keys() {
        if !expected.contains_key(key)
            && key != "duration"
            && !(generated_writer && key == "encoder")
        {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                format!("{target} unexpectedly retained or introduced tag {key}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tag_tests {
    use super::*;
    #[test]
    fn only_matroska_undefined_stream_language_has_the_declared_omission() {
        let undefined = BTreeMap::from([("language".into(), "und".into())]);
        let english = BTreeMap::from([("language".into(), "eng".into())]);
        let empty = BTreeMap::new();
        assert!(verify_tags(&undefined, &empty, "Stream", false).is_ok());
        assert!(verify_tags(&undefined, &empty, "Container", true).is_err());
        assert!(verify_tags(&english, &empty, "Stream", false).is_err());
        assert!(verify_tags(&undefined, &english, "Stream", false).is_err());
        assert!(verify_tags(&empty, &english, "Stream", false).is_err());
        let arbitrary = BTreeMap::from([("review_note".into(), "und".into())]);
        assert!(verify_tags(&arbitrary, &empty, "Stream", false).is_err());
    }
}
