//! FFmpeg-only media work. Utilities share the same planner and verifier.
mod admission;
mod budget;
mod output_profile;
mod preservation;
use nw_domain::{
    DomainError, Result,
    mux::{self, CleanAction, CleanIntent, Stream, StreamKind},
    records::{Asset, FileIdentity, MediaProbe, Plan},
};
use nw_platform::files::{self, io_error};
use output_profile::OutputProfile;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
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
    FileTimes(nw_domain::file_times::FileTimeOutcome),
    Progress(nw_domain::execution::StageProgress),
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
        self.probe_with_policy(path, nw_domain::execution::CleanupLimits::default().probe, cancel).await
    }
    pub async fn probe_with_policy(
        &self, path: &Path, deadline: nw_domain::execution::DeadlinePolicy, cancel: CancellationToken,
    ) -> Result<MediaProbe> {
        deadline.duration()?;
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
        let result = nw_platform::process::run_readonly_with_policy(
            Path::new(&self.ffprobe.path),
            &args,
            deadline,
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
        let (initial, stamp) = blocking(move || {
            let path = files::source_path(&path)?;
            let stamp = files::SourceStamp::capture_resolved(&path)?;
            let identity = files::identity(&path, &token)?;
            stamp.revalidate()?;
            Ok((identity, stamp))
        })
        .await?;
        let probe = self.probe(Path::new(&initial.path), cancel.clone()).await?;
        let expected = initial.clone();
        let source_object = blocking(move || {
            files::revalidate(&expected, &cancel)?;
            stamp.object_identity()
        })
        .await?;
        Ok(Asset {
            source_object: Some(source_object),
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
        self.clean_plan_with_file_times_at(
            asset,
            output,
            intent,
            nw_domain::file_times::FileTimeRequest::default(),
            creator,
            now,
        )
    }
    pub fn clean_plan_with_file_times_at(
        &self,
        asset: Asset,
        output: &Path,
        intent: &CleanIntent,
        file_times: nw_domain::file_times::FileTimeRequest,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        let object = registered_object(&asset)?.clone();
        let source_stamp = files::SourceStamp::capture_resolved(Path::new(&asset.identity.path))?;
        source_stamp.require_object(&object)?;
        let output = files::output_path(output)?;
        if output == Path::new(&asset.identity.path) {
            return Err(DomainError::new(
                "SOURCE_OUTPUT_ALIAS",
                "Output must be distinct from the source",
            ));
        }
        let resolution = mux::resolve(&asset.probe.streams, intent)?;
        let profile = if resolution.action == CleanAction::Remux {
            let profile = OutputProfile::from_output(&output)?;
            profile.admit(&asset, &resolution)?;
            if profile == OutputProfile::Matroska {
                admit_matroska(&output, &resolution.streams)?;
            }
            profile
        } else {
            OutputProfile::Matroska
        };
        if resolution.action == CleanAction::CopyUnchanged
            && Path::new(&asset.identity.path)
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                != output
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(str::to_ascii_lowercase)
        {
            return Err(DomainError::new(
                "CONTAINER_CHANGE_REQUIRED",
                "Byte copy requires the original container extension",
            ));
        }
        let args = if resolution.action == CleanAction::Remux {
            lower_remux(&asset, &resolution, profile)?
        } else {
            vec![]
        };
        let mut required_rights = nw_domain::plans::RequiredRights::native(
            asset.identity.clone(),
            files::path_text(&output)?,
        );
        required_rights.source_read.object = Some(object);
        required_rights.output_create.parent_object = Some(
            files::DirectoryStamp::capture_resolved(
                output.parent().expect("Resolved output parent"),
            )?
            .object_identity()?,
        );
        let file_times = if resolution.action == CleanAction::Skip {
            None
        } else {
            Some(nw_platform::file_times::plan(&source_stamp, file_times)?)
        };
        source_stamp.revalidate()?;
        let mut plan = Plan {
            execution_limits: Some(nw_domain::execution::CleanupLimits::default()),
            file_times,
            schema_version: 2,
            required_rights: Some(required_rights),
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
            verification_scope: profile.scope().into(),
        };
        plan.hash = plan_digest(&plan)?;
        Ok(plan)
    }
    pub fn validate_plan(&self, plan: &Plan) -> Result<()> {
        nw_domain::plans::validate_requirements(plan)?;
        if let Some(policy) = &plan.file_times {
            policy.validate()?;
        }
        registered_object(&plan.source)?;
        reviewed_parent(plan)?;
        if plan_digest(plan)? != plan.hash {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Stored plan hash mismatch",
            ));
        }
        let profile = if plan.resolution.action == CleanAction::Remux {
            OutputProfile::from_output(Path::new(&plan.output))?
        } else {
            OutputProfile::Matroska
        };
        if plan.verification_scope != profile.scope() {
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
        if plan.resolution.action == CleanAction::Remux {
            profile.admit(&plan.source, &plan.resolution)?;
            if profile == OutputProfile::Matroska {
                admit_matroska(Path::new(&plan.output), &plan.resolution.streams)?;
            }
        }
        if plan.resolution.action == CleanAction::Remux
            && plan.argv_template != lower_remux(&plan.source, &plan.resolution, profile)?
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
        let output_parent = output_parent_stamp(plan)?;
        let source = plan.source.identity.clone();
        let object = registered_object(&plan.source)?.clone();
        let ffmpeg = plan.ffmpeg.clone();
        let ffprobe = plan.ffprobe.clone();
        let token = cancel.clone();
        blocking(move || {
            let stamp = files::SourceStamp::capture_resolved(Path::new(&source.path))?;
            stamp.require_object(&object)?;
            files::revalidate(&source, &token)?;
            stamp.require_object(&object)?;
            files::revalidate(&ffmpeg, &token)?;
            files::revalidate(&ffprobe, &token)
        })
        .await?;
        output_parent.revalidate()?;
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
                let object = registered_object(&plan.source)?.clone();
                let token = cancel.clone();
                let deadline = plan.execution_limits.as_ref().map_or(
                    nw_domain::execution::DeadlinePolicy::Disabled, |limits| limits.operation,
                );
                let progress_observer = observer.clone();
                let runtime = tokio::runtime::Handle::current();
                let (staging, actual, source_stamp) = blocking(move || {
                    files::copy_into_staging_observed(&source, &object, staging, &token, deadline,
                        &mut |progress| runtime.block_on(progress_observer(ExecutionEvent::Progress(progress))))
                })
                .await?;
                observer(ExecutionEvent::Verifying).await?;
                let (staging, source_stamp) = apply_file_times(
                    staging,
                    source_stamp,
                    plan.file_times.clone(),
                    &cancel,
                    &observer,
                )
                .await?;
                let verification =
                    "Full byte SHA-256 and length verified before no-replace publication"
                        .to_owned();
                let artifact = publish(
                    VerifiedStaging {
                        file: staging,
                        identity: actual,
                        source: source_stamp,
                    },
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
        let object = registered_object(&plan.source)?.clone();
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
            stamp.require_object(&object)?;
            files::revalidate(&source, &token)?;
            stamp.require_object(&object)?;
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
        let process = nw_platform::process::run_readonly_with_policy(
            Path::new(&self.ffmpeg.path),
            &args,
            plan.execution_limits.clone().unwrap_or_default().operation,
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
        let observed = self.probe_with_policy(
            staging.path(), plan.execution_limits.clone().unwrap_or_default().probe, cancel.clone(),
        ).await?;
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
        let source_stamp = blocking(move || {
            files::revalidate(&source, &token)?;
            source_stamp.revalidate()?;
            Ok(source_stamp)
        })
        .await?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Cancelled before publication fence",
            ));
        }
        let (staging, source_stamp) = apply_file_times(
            staging,
            source_stamp,
            plan.file_times.clone(),
            &cancel,
            &observer,
        )
        .await?;
        let artifact = publish(
            VerifiedStaging {
                file: staging,
                identity: actual,
                source: source_stamp,
            },
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
async fn apply_file_times(
    staging: tempfile::NamedTempFile,
    source: files::SourceStamp,
    policy: Option<nw_domain::file_times::FileTimePlan>,
    cancel: &CancellationToken,
    observer: &Observer,
) -> Result<(tempfile::NamedTempFile, files::SourceStamp)> {
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Cancelled before filesystem metadata verification",
        ));
    }
    if let Some(policy) = &policy
        && !matches!(policy, nw_domain::file_times::FileTimePlan::New)
    {
        observer(ExecutionEvent::Prepared(vec![
            "rust:preserve-file-times".into(),
            files::path_text(staging.path())?,
            serde_json::to_string(policy).map_err(io_error)?,
        ]))
        .await?;
    }
    let (staging, source, outcome) = blocking(move || {
        source.revalidate()?;
        let outcome = policy
            .as_ref()
            .map(|policy| nw_platform::file_times::apply(&source, staging.as_file(), policy))
            .transpose()?;
        source.revalidate()?;
        Ok((staging, source, outcome))
    })
    .await?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Cancelled before filesystem metadata publication",
        ));
    }
    if let Some(outcome) = outcome {
        observer(ExecutionEvent::FileTimes(outcome)).await?;
    }
    Ok((staging, source))
}
async fn prepare_staging(
    plan: &Plan,
    job_id: &str,
    observer: &Observer,
) -> Result<tempfile::NamedTempFile> {
    let parent_stamp = output_parent_stamp(plan)?;
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
        parent_stamp.revalidate()?;
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
struct VerifiedStaging {
    file: tempfile::NamedTempFile,
    identity: FileIdentity,
    source: files::SourceStamp,
}
async fn publish(
    verified: VerifiedStaging,
    plan: &Plan,
    job_id: &str,
    verification: String,
    cancel: CancellationToken,
    observer: Observer,
) -> Result<FileIdentity> {
    let VerifiedStaging {
        file: staging,
        identity: actual,
        source: source_stamp,
    } = verified;
    let parent_stamp = output_parent_stamp(plan)?;
    let destination = nw_platform::claims::Destination::reviewed(
        Path::new(&plan.output),
        reviewed_parent(plan)?,
    )?;
    let claims =
        nw_platform::claims::DestinationClaims::acquire(vec![destination], cancel.clone()).await?;
    let output = files::revalidate_output(&plan.output)?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Cancelled before publication fence",
        ));
    }
    let staging = nw_platform::publication::RetainedStaging::new(staging)?;
    let proof_name = format!(".numera-proof-{}", Uuid::new_v4());
    let proof = staging.create_proof(std::ffi::OsStr::new(&proof_name))?;
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
    claims.revalidate()?;
    parent_stamp.revalidate()?;
    source_stamp.revalidate()?;
    observer(ExecutionEvent::Fence(journal)).await?;
    // The committed fence is the cancellation boundary. The retained hard link proves the published object for later job-scoped byte reads.
    parent_stamp.revalidate()?;
    staging.publish_no_replace(&parent_stamp, &output)?;
    claims.revalidate()?;
    parent_stamp.revalidate()?;
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
    let format = value["format"]["format_name"]
        .as_str()
        .ok_or_else(|| DomainError::new("INVALID_PROBE", "Missing container format"))?
        .to_owned();
    let movie = matches!(format.as_str(), "mov" | "mov,mp4,m4a,3gp,3g2,mj2");
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
            tags: {
                let mut tags = parse_tags(&row["tags"]);
                if movie
                    && !tags.contains_key("title")
                    && let Some(name) = tags.remove("name")
                {
                    tags.insert("title".into(), name);
                }
                tags
            },
            dispositions,
        });
    }

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
        verification_scope: if movie {
            "Bounded metadata probe; not a full decode or packet scan. Unambiguous MOV/MP4 stream name is the canonical title; exact raw tags remain in stream_details."
        } else {
            "Bounded metadata probe; not a full decode or packet scan"
        }.into(),
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
fn reviewed_parent(plan: &Plan) -> Result<&nw_domain::records::NativeObjectIdentity> {
    let rights = nw_domain::plans::validate_requirements(plan)?;
    rights
        .output_create
        .parent_object
        .as_ref()
        .filter(|object| object.schema_version == 1)
        .ok_or_else(|| {
            DomainError::new(
                "INVALID_PLAN",
                "Review a fresh plan with the original output directory identity",
            )
        })
}
/// Bind the current output directory to the identity frozen in the reviewed plan.
pub fn output_parent_stamp(plan: &Plan) -> Result<files::DirectoryStamp> {
    let parent = Path::new(&plan.output)
        .parent()
        .ok_or_else(|| DomainError::new("PATH_DENIED", "Output parent required"))?;
    let stamp = files::DirectoryStamp::capture_resolved(parent)?;
    stamp.require_object(reviewed_parent(plan)?)?;
    Ok(stamp)
}

fn registered_object(asset: &Asset) -> Result<&nw_domain::records::NativeObjectIdentity> {
    let object = asset.source_object.as_ref().ok_or_else(|| {
        DomainError::new(
            "INVALID_PLAN",
            "Register the source again to capture its original native object identity",
        )
    })?;
    if object.schema_version != 1 {
        return Err(DomainError::new(
            "INVALID_PLAN",
            "The native object identity version is not supported",
        ));
    }
    Ok(object)
}
fn lower_remux(
    asset: &Asset,
    resolution: &mux::CleanResolution,
    profile: OutputProfile,
) -> Result<Vec<String>> {
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
        let movie_name_alias = demuxer == "mov"
            && !original.tags.contains_key("name")
            && original.tags.contains_key("title")
            && asset
                .probe
                .stream_details
                .get(&original.index)
                .is_some_and(|row| {
                    row["tags"].get("name").is_some() && row["tags"].get("title").is_none()
                });
        if movie_name_alias {
            args.extend([format!("-metadata:s:{index}"), "name=".into()]);
        }
        for tag in original
            .tags
            .keys()
            .chain(stream.tags.keys())
            .collect::<BTreeSet<_>>()
        {
            if original.tags.get(tag) != stream.tags.get(tag)
                || !resolution.preservation.stream_tags
                || (movie_name_alias && tag.as_str() == "title")
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
    args.extend([
        "-f".into(),
        profile.muxer().into(),
        "@owned_staging@".into(),
    ]);
    Ok(args)
}
fn verify_structure(plan: &Plan, actual: &MediaProbe) -> Result<()> {
    let profile = OutputProfile::from_output(Path::new(&plan.output))?;
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
            profile,
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
    verify_tags(expected, &actual.global_tags, "Container", true, profile)?;
    Ok(())
}

fn verify_tags(
    expected: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
    target: &str,
    generated_writer: bool,
    profile: OutputProfile,
) -> Result<()> {
    let matroska = profile == OutputProfile::Matroska;
    for (key, value) in expected {
        // Matroska regenerates its duration tags from the independently verified packet clock.
        let undefined_stream_language = matroska
            && !generated_writer
            && key == "language"
            && value == "und"
            && !actual.contains_key(key);
        if !(matroska && key == "duration")
            && !undefined_stream_language
            && actual.get(key) != Some(value)
        {
            return Err(DomainError::new(
                "VERIFICATION_FAILED",
                format!("{target} tag {key} did not match its frozen value"),
            ));
        }
    }
    for key in actual.keys() {
        if !expected.contains_key(key)
            && !(matroska && key == "duration")
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
    fn lowering_distinguishes_current_title_projection_from_a_legacy_raw_name_snapshot() {
        let probe = parse_probe(serde_json::json!({
            "format":{"format_name":"mov"},
            "streams":[{"index":0,"codec_type":"video","codec_name":"mpeg4","tags":{"name":"Original label","language":"und"}}]
        })).unwrap();
        let mut source = Asset {
            source_object: None,
            display_name: "source.mov".into(),
            id: "a".into(),
            creator: "owner".into(),
            identity: FileIdentity {
                path: "/source.mov".into(),
                bytes: "1".into(),
                sha256: "0".repeat(64),
            },
            probe,
        };
        let mut resolved = mux::CleanResolution {
            action: CleanAction::Remux,
            streams: source.probe.streams.clone(),
            outcomes: vec![],
            reasons: vec![],
            warnings: vec![],
            preservation: nw_domain::mux::Preservation::default(),
        };
        let current = lower_remux(&source, &resolved, OutputProfile::Matroska).unwrap();
        assert!(
            current
                .windows(2)
                .any(|args| args == ["-metadata:s:0", "name="])
        );
        assert!(
            current
                .windows(2)
                .any(|args| args == ["-metadata:s:0", "title=Original label"])
        );
        let title = source.probe.streams[0].tags.remove("title").unwrap();
        source.probe.streams[0].tags.insert("name".into(), title);
        resolved.streams = source.probe.streams.clone();
        let legacy = lower_remux(&source, &resolved, OutputProfile::Matroska).unwrap();
        assert!(
            !legacy
                .iter()
                .any(|argument| argument == "name=" || argument == "title=Original label")
        );
        assert!(
            legacy
                .windows(2)
                .any(|args| args == ["-map_metadata:s:0", "0:s:0"])
        );
    }
    #[test]
    fn movie_track_name_is_an_exact_title_projection_with_raw_metadata_retained() {
        for format in ["mov,mp4,m4a,3gp,3g2,mj2", "matroska,webm"] {
            let input = serde_json::json!({
                "format":{"format_name":format},
                "streams":[{"index":0,"codec_type":"video","codec_name":"mpeg4","tags":{"name":"Exact mixed-case title"}}]
            });
            let probe = parse_probe(input).unwrap();
            assert_eq!(
                probe.stream_details[&0]["tags"]["name"],
                "Exact mixed-case title"
            );
            if format.starts_with("mov") {
                assert_eq!(
                    probe.streams[0].tags.get("title").unwrap(),
                    "Exact mixed-case title"
                );
                assert!(!probe.streams[0].tags.contains_key("name"));
            } else {
                assert_eq!(
                    probe.streams[0].tags.get("name").unwrap(),
                    "Exact mixed-case title"
                );
                assert!(!probe.streams[0].tags.contains_key("title"));
            }
        }
        let ambiguous = parse_probe(serde_json::json!({
            "format":{"format_name":"mov"},
            "streams":[{"index":0,"codec_type":"video","codec_name":"mpeg4","tags":{"name":"Original name","title":"Separate title"}}]
        })).unwrap();
        assert_eq!(
            ambiguous.streams[0].tags.get("name").unwrap(),
            "Original name"
        );
        assert_eq!(
            ambiguous.streams[0].tags.get("title").unwrap(),
            "Separate title"
        );
    }
    #[test]
    fn mov_mp4_never_borrow_matroska_language_or_duration_exceptions() {
        let empty = BTreeMap::new();
        for profile in [OutputProfile::QuickTimeMpeg4, OutputProfile::IsoMp4Mpeg4] {
            for (key, value) in [("language", "und"), ("duration", "authored duration")] {
                let expected = BTreeMap::from([(key.into(), value.into())]);
                assert!(verify_tags(&expected, &empty, "Stream", false, profile).is_err());
                assert!(verify_tags(&empty, &expected, "Stream", false, profile).is_err());
            }
        }
    }
    #[test]
    fn only_matroska_undefined_stream_language_has_the_declared_omission() {
        let undefined = BTreeMap::from([("language".into(), "und".into())]);
        let english = BTreeMap::from([("language".into(), "eng".into())]);
        let empty = BTreeMap::new();
        assert!(verify_tags(&undefined, &empty, "Stream", false, OutputProfile::Matroska).is_ok());
        assert!(
            verify_tags(
                &undefined,
                &empty,
                "Container",
                true,
                OutputProfile::Matroska
            )
            .is_err()
        );
        assert!(verify_tags(&english, &empty, "Stream", false, OutputProfile::Matroska).is_err());
        assert!(
            verify_tags(
                &undefined,
                &english,
                "Stream",
                false,
                OutputProfile::Matroska
            )
            .is_err()
        );
        assert!(verify_tags(&empty, &english, "Stream", false, OutputProfile::Matroska).is_err());
        let arbitrary = BTreeMap::from([("review_note".into(), "und".into())]);
        assert!(verify_tags(&arbitrary, &empty, "Stream", false, OutputProfile::Matroska).is_err());
    }
}
