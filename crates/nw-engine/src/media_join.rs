//! Review and assemble compatible Matroska segments without recoding packets.
use crate::output_profile::OutputProfile;
use crate::{Engine, Result, blocking, io_error, lower_remux, plan_digest, registered_object};
use nw_domain::{
    DomainError,
    media_join::{Audio, Join, Metadata, Segment},
    mux::{CleanAction, CleanResolution, Preservation, Stream, StreamKind, StreamOutcome},
    packet_cut::ffmpeg_time,
    records::{Asset, Plan},
    time::Rational,
};
use nw_platform::files;
use std::{io::Write, path::Path};
use tokio_util::sync::CancellationToken;

pub(super) const SCOPE: &str = "Compare complete packets, rebased presentation/decode clocks, codec parameters and exposed side data from an ordered compatible Matroska join. Chapters, subtitles and attachments are dropped.";
pub(super) const SIDECAR: &str = "@owned_concat_manifest@";

fn incompatible(message: &str) -> DomainError {
    DomainError::new("JOIN_INCOMPATIBLE", message)
}

fn retained(streams: &[Stream], audio: Audio) -> Result<Vec<Stream>> {
    let mut video_count = 0;
    let mut chosen = Vec::new();
    for stream in streams {
        if stream.kind == StreamKind::Video && !stream.attached_picture() {
            video_count += 1;
            if stream.codec != "ffv1" {
                return Err(incompatible(
                    "The exact copy profile requires FFV1 for each input picture",
                ));
            }
            chosen.push(stream.clone());
        } else if stream.kind == StreamKind::Audio && audio == Audio::CopyAll {
            if stream.codec != "pcm_s16le" {
                return Err(incompatible(
                    "The exact copy profile admits original PCM s16le audio tracks",
                ));
            }
            chosen.push(stream.clone());
        }
    }
    if video_count > 1 || chosen.is_empty() || chosen.len() > 16 {
        return Err(incompatible(
            "Each copy source requires the same one-picture or audio-only layout and at most sixteen retained tracks",
        ));
    }
    Ok(chosen)
}

fn resolution(streams: &[Stream], join: &Join) -> Result<CleanResolution> {
    let mut output = retained(streams, join.audio)?;
    let keep_tags = join.metadata == Metadata::FirstInput;
    if !keep_tags {
        for stream in &mut output {
            stream.tags.clear();
        }
    }
    let outcomes = streams
        .iter()
        .map(|stream| {
            let output_index = output
                .iter()
                .position(|retained| retained.index == stream.index)
                .map(|index| index as u32);
            StreamOutcome {
                source_index: stream.index,
                output_index,
                reason: if output_index.is_some() {
                    "Concatenate the compatible original packets"
                } else {
                    "Excluded by the join stream policy"
                }
                .into(),
            }
        })
        .collect();
    Ok(CleanResolution {
        action: CleanAction::Remux,
        streams: output,
        outcomes,
        reasons: vec!["Join compatible packet clocks in the reviewed input order".into()],
        warnings: Vec::new(),
        preservation: Preservation {
            additional_video: false,
            attached_pictures: false,
            attachments: false,
            data: false,
            unknown: false,
            global_tags: keep_tags,
            stream_tags: keep_tags,
            chapters: false,
            dispositions: true,
        },
        metadata_patch: None,
        chapter_import: None,
    })
}

fn validate_formats(primary: &Asset, join: &Join) -> Result<Vec<Stream>> {
    let expected = retained(&primary.probe.streams, join.audio)?;
    for (source, _) in join.all(primary) {
        if crate::admission::frozen_demuxer(&source.probe.format)? != "matroska" {
            return Err(incompatible(
                "The packet join profile admits original Matroska sources",
            ));
        }
        let streams = retained(&source.probe.streams, join.audio)?;
        if streams.len() != expected.len() {
            return Err(incompatible(
                "Copy joins require matching retained stream topology in every input",
            ));
        }
        for (original, other) in expected.iter().zip(&streams) {
            let detail = source
                .probe
                .stream_details
                .get(&other.index)
                .ok_or_else(|| incompatible("Missing original stream parameters"))?;
            if original.index != other.index
                || original.kind != other.kind
                || original.codec != other.codec
                || detail["time_base"] != "1/1000"
            {
                return Err(incompatible(
                    "Retained streams must match original physical indices, codecs and millisecond clocks",
                ));
            }
            crate::preservation::verify_codec(
                &primary.probe,
                original.index,
                &source.probe,
                other.index,
                "Joined input stream",
            )
            .map_err(|error| incompatible(&error.message))?;
        }
    }
    Ok(expected)
}

impl Engine {
    pub async fn inspect_media_join(
        &self,
        inputs: Vec<Asset>,
        output: &str,
        audio: Audio,
        metadata: Metadata,
        cancel: CancellationToken,
    ) -> Result<(Asset, Join, Vec<files::SourceStamp>)> {
        let mut inputs = inputs.into_iter();
        let primary = inputs
            .next()
            .ok_or_else(|| incompatible("Missing primary join input"))?;
        let duration = |asset: &Asset| {
            asset.probe.container_duration
            .ok_or_else(|| incompatible("A packet join requires each registered source to have a finite container duration"))
        };
        let join = Join {
            primary_duration: duration(&primary)?,
            following: inputs
                .map(|input| {
                    Ok(Segment {
                        duration: duration(&input)?,
                        input,
                    })
                })
                .collect::<Result<_>>()?,
            metadata,
            audio,
        };
        join.validate(&primary, output)?;
        let retained = validate_formats(&primary, &join)?;
        let mut stamps = Vec::with_capacity(join.following.len() + 1);
        for (asset, duration) in join.all(&primary) {
            let identity = asset.identity.clone();
            let object = registered_object(asset)?.clone();
            let token = cancel.clone();
            let stamp = blocking(move || {
                let stamp = files::SourceStamp::capture_resolved(Path::new(&identity.path))?;
                stamp.require_object(&object)?;
                files::revalidate(&identity, &token)?;
                stamp.revalidate()?;
                Ok(stamp)
            })
            .await?;
            crate::preservation::check_join_clock(self, asset, &retained, duration, cancel.clone())
                .await?;
            stamp.revalidate()?;
            stamps.push(stamp);
        }
        Ok((primary, join, stamps))
    }

    pub fn media_join_plan_at(
        &self,
        primary: Asset,
        output: &Path,
        join: Join,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        let mut plan = self.review_stream_copy(
            primary,
            output,
            Default::default(),
            creator,
            now,
            |streams| resolution(streams, &join),
        )?;
        plan.media_join = Some(join);
        plan.verification_scope = SCOPE.into();
        plan.argv_template = invocation(&plan, OutputProfile::Matroska)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }
}

pub(super) fn invocation(plan: &Plan, profile: OutputProfile) -> Result<Vec<String>> {
    let join = plan
        .media_join
        .as_ref()
        .ok_or_else(|| incompatible("Missing reviewed ordered sources"))?;
    if profile != OutputProfile::Matroska
        || plan.resolution.action != CleanAction::Remux
        || plan.resolution.streams != resolution(&plan.source.probe.streams, join)?.streams
        || plan.resolution.preservation.global_tags != (join.metadata == Metadata::FirstInput)
        || plan.resolution.preservation.stream_tags != (join.metadata == Metadata::FirstInput)
    {
        return Err(incompatible(
            "Use the reviewed ordered Matroska layout and a new .mkv destination",
        ));
    }
    validate_formats(&plan.source, join)?;
    let mut args = lower_remux(&plan.source, &plan.resolution, profile)?;
    let start = args
        .iter()
        .position(|arg| arg == "-protocol_whitelist")
        .expect("Shared input policy");
    let input = args
        .iter()
        .position(|arg| arg == "fd:")
        .expect("Shared held source");
    args.splice(
        start..=input,
        [
            "-safe",
            "1",
            "-auto_convert",
            "0",
            "-protocol_whitelist",
            "file",
            "-format_whitelist",
            "concat,matroska",
            "-f",
            "concat",
            "-i",
            SIDECAR,
        ]
        .map(str::to_owned),
    );
    Ok(args)
}

pub(super) struct StagedJoin {
    _directory: tempfile::TempDir,
    files: Vec<files::SourceStamp>,
    originals: Vec<files::SourceStamp>,
    manifest: String,
}

impl StagedJoin {
    pub fn manifest(&self) -> &str {
        &self.manifest
    }

    pub fn revalidate(&self) -> Result<()> {
        for source in self.files.iter().chain(&self.originals) {
            source.revalidate()?;
        }
        Ok(())
    }

    pub fn into_originals(self) -> Vec<files::SourceStamp> {
        self.originals
    }
}

pub(super) async fn stage(
    plan: &Plan,
    parent: &Path,
    cancel: CancellationToken,
) -> Result<StagedJoin> {
    let join = plan.media_join.as_ref().expect("Reviewed packet join");
    let inputs: Vec<_> = join
        .all(&plan.source)
        .map(|(asset, duration)| (asset.clone(), duration))
        .collect();
    let parent = parent.to_owned();
    blocking(move || {
        let directory = tempfile::Builder::new()
            .prefix("join-inputs-")
            .tempdir_in(&parent)
            .map_err(io_error)?;
        let mut files = Vec::new();
        let mut originals = Vec::new();
        let mut content = String::from("ffconcat version 1.0\n");
        let limit = nw_domain::execution::DeadlinePolicy::Finite { seconds: 600 };
        for (index, (asset, duration)) in inputs.into_iter().enumerate() {
            let name = format!("segment-{index}.mkv");
            let path = directory.path().join(&name);
            originals.push(crate::track_append::stage_dependency(
                &asset.identity,
                registered_object(&asset)?,
                &path,
                &cancel,
                limit,
            )?);
            files.push(files::SourceStamp::capture_resolved(&path)?);
            use std::fmt::Write;
            writeln!(
                &mut content,
                "file {name}\nduration {}",
                ffmpeg_time(duration)?
            )
            .expect("Write to a string");
        }
        let path = directory.path().join("source.ffconcat");
        let mut manifest = tempfile::NamedTempFile::new_in(directory.path()).map_err(io_error)?;
        manifest.write_all(content.as_bytes()).map_err(io_error)?;
        manifest.as_file().sync_all().map_err(io_error)?;
        manifest
            .persist_noclobber(&path)
            .map_err(|error| io_error(error.error))?;
        files.push(files::SourceStamp::capture_resolved(&path)?);
        Ok(StagedJoin {
            _directory: directory,
            files,
            originals,
            manifest: format!("file:{}", files::path_text(&path)?),
        })
    })
    .await
}
