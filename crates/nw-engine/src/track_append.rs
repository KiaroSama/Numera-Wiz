//! Frozen strict-copy layouts and privately staged extra inputs.
use super::{
    Engine, Result, blocking, io_error, lower_with_additions, plan_digest, registered_object,
};
use nw_domain::{
    DomainError,
    execution::DeadlinePolicy,
    mux::{self, CleanAction, CleanIntent},
    records::{Asset, FileIdentity, NativeObjectIdentity, Plan},
    track_append::{AddedInput, validate_sources},
};
use nw_platform::files;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use crate::output_profile::OutputProfile;

pub(super) fn placeholder(index: usize) -> String {
    format!("@added_track_input_{index}@")
}

impl Engine {
    pub fn append_tracks_plan_at(
        &self,
        primary: Asset,
        additions: Vec<AddedInput>,
        output: &Path,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        validate_sources(&primary, &additions, &files::path_text(output)?)?;
        self.replace_tracks_plan_at(
            primary,
            additions,
            output,
            &CleanIntent::preserving_all(),
            creator,
            now,
        )
    }

    pub fn replace_tracks_plan_at(
        &self,
        primary: Asset,
        additions: Vec<AddedInput>,
        output: &Path,
        intent: &CleanIntent,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        if additions.is_empty() {
            return self.clean_plan_at(primary, output, intent, creator, now);
        }
        let mut plan = self.review_stream_copy(
            primary,
            output,
            Default::default(),
            creator,
            now,
            |streams| {
                let mut resolution = mux::resolve(streams, intent)?;
                resolution.action = CleanAction::Remux;
                resolution.reasons.push(
                    "Append the reviewed external audio and subtitles without conversion".into(),
                );
                Ok(resolution)
            },
        )?;
        plan.added_tracks = additions;
        validate_sources(&plan.source, &plan.added_tracks, &plan.output)?;
        validate_track_additions(&plan, OutputProfile::Matroska)?;
        plan.argv_template = lower_with_additions(
            &plan.source,
            &plan.resolution,
            OutputProfile::Matroska,
            &plan.added_tracks,
        )?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }
}

pub(super) fn validate_track_additions(plan: &Plan, profile: OutputProfile) -> Result<()> {
    if plan.added_tracks.is_empty() {
        return Ok(());
    }
    validate_sources(&plan.source, &plan.added_tracks, &plan.output)?;
    if profile != OutputProfile::Matroska
        || crate::admission::frozen_demuxer(&plan.source.probe.format)? != "matroska"
        || plan.resolution.action != CleanAction::Remux
        || plan.resolution.chapter_import.is_some()
        || plan.resolution.streams.iter().any(|selected| {
            !plan.source.probe.streams.iter().any(|source| {
                selected.index == source.index
                    && selected.kind == source.kind
                    && selected.codec == source.codec
            })
        })
    {
        return Err(DomainError::new(
            "INCOMPATIBLE_PRESERVATION",
            "Strict track replacement requires original Matroska stream codecs and a new .mkv destination",
        ));
    }
    for input in &plan.added_tracks {
        crate::admission::frozen_demuxer(&input.source.probe.format)?;
        crate::admit_matroska(Path::new(&plan.output), &input.tracks)?;
        let object = registered_object(&input.source)?;
        let stamp = files::SourceStamp::capture_resolved(Path::new(&input.source.identity.path))?;
        stamp.require_object(object)?;
    }
    Ok(())
}

pub(super) struct StagedInput {
    staged: files::SourceStamp,
    file: tempfile::NamedTempFile,
    source: files::SourceStamp,
}

impl StagedInput {
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn revalidate(&self) -> Result<()> {
        self.staged.revalidate()?;
        self.source.revalidate()
    }

    pub fn into_source(self) -> files::SourceStamp {
        self.source
    }
}

pub(super) async fn stage(
    inputs: &[AddedInput],
    parent: PathBuf,
    cancel: CancellationToken,
) -> Result<Vec<StagedInput>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let inputs = inputs.to_vec();
    blocking(move || {
        if nw_platform::security::private_directory(&parent)? != parent {
            return Err(DomainError::new(
                "TARGET_CHANGED",
                "Track input staging changed location",
            ));
        }
        let deadline = DeadlinePolicy::Finite { seconds: 600 };
        inputs
            .iter()
            .map(|input| {
                let source = &input.source;
                let object = registered_object(source)?;
                let file = tempfile::Builder::new()
                    .prefix("track-input-")
                    .tempfile_in(&parent)
                    .map_err(io_error)?;
                let (file, _, source) = files::copy_into_staging_observed(
                    &source.identity,
                    object,
                    file,
                    &cancel,
                    deadline,
                    &mut |_| Ok(()),
                )?;
                let staged = files::SourceStamp::capture_open_resolved(
                    file.path(),
                    file.as_file().try_clone().map_err(io_error)?,
                )?;
                Ok(StagedInput {
                    staged,
                    file,
                    source,
                })
            })
            .collect()
    })
    .await
}

pub(super) fn stage_dependency(
    identity: &FileIdentity,
    object: &NativeObjectIdentity,
    output: &Path,
    cancel: &CancellationToken,
    deadline: DeadlinePolicy,
) -> Result<files::SourceStamp> {
    let parent = output.parent().expect("Owned staging directory");
    let temp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    let (temp, _, stamp) = files::copy_into_staging_observed(identity, object, temp, cancel, deadline, &mut |_| Ok(()))?;
    temp.persist_noclobber(output).map_err(|error| io_error(error.error))?;
    stamp.revalidate()?;
    Ok(stamp)
}
