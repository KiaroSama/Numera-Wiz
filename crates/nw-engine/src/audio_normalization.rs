//! Individually measured loudness and source-preserving publication checks.
use super::{Engine, Result, blocking, lower_remux, plan_digest};
use nw_domain::{
    DomainError,
    audio_normalization::{Track, TrackRequest, validate_requests},
    mux::{self, CleanAction, CleanIntent, StreamKind},
    records::{Asset, FileIdentity, MediaProbe, Plan},
};
use nw_platform::files;
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::output_profile::OutputProfile;

pub(super) const SCOPE: &str = "Selected PCM tracks receive independently measured linear two-pass loudness, verified output loudness and peak, sample counts and original starts. Other retained streams keep exact packets, timing, codec parameters and tags.";

fn unavailable(message: &str) -> DomainError {
    DomainError::new("NORMALIZATION_UNQUALIFIED", message)
}

fn rate(probe: &MediaProbe, index: u32) -> Result<u32> {
    let same_pcm = probe.streams.iter().any(|stream| {
        stream.index == index && stream.kind == StreamKind::Audio && stream.codec == "pcm_s16le"
    });
    if !same_pcm {
        return Err(unavailable("The linear normalization profile requires an original PCM s16le audio stream"));
    }
    let fields = probe.stream_details.get(&index).ok_or_else(|| unavailable("Missing source audio format"))?;
    let rate = fields["sample_rate"].as_str().and_then(|rate| rate.parse::<u32>().ok());
    let channels = fields["channels"].as_u64();
    if channels != Some(1) && channels != Some(2) {
        return Err(unavailable("This PCM normalization profile admits mono and stereo"));
    }
    rate.filter(|rate| matches!(rate, 44100 | 48000)).ok_or_else(|| unavailable("PCM normalization requires 44.1 or 48 kHz"))
}

impl Engine {
    pub async fn inspect_normalization(
        &self,
        input: &Asset,
        requests: &[TrackRequest],
        cancel: CancellationToken,
    ) -> Result<(Vec<Track>, files::SourceStamp)> {
        if crate::admission::frozen_demuxer(&input.probe.format)? != "matroska" {
            return Err(unavailable("PCM normalization requires a registered Matroska source"));
        }
        validate_requests(requests)?;
        for request in requests {
            rate(&input.probe, request.stream_index)?;
        }
        let path = Path::new(&input.identity.path);
        let stamp = files::SourceStamp::capture_resolved(path)?;
        let mut tracks = Vec::with_capacity(requests.len());
        for request in requests {
            stamp.revalidate()?;
            let held = files::SourceStamp::capture_open_resolved(path, stamp.try_clone_file()?)?;
            tracks.push(self.measure_normalization_track(input, held, *request, cancel.clone()).await?);
        }
        stamp.revalidate()?;
        Ok((tracks, stamp))
    }

    pub fn normalization_plan_at(
        &self,
        input: Asset,
        output: &Path,
        tracks: Vec<Track>,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        let mut plan = self.review_stream_copy(input, output, Default::default(), creator, now, |streams| {
            let mut resolution = mux::resolve(streams, &CleanIntent::preserving_all())?;
            resolution.action = CleanAction::Remux;
            resolution.reasons.push("Apply independently measured linear two-pass loudness to the selected audio".into());
            Ok(resolution)
        })?;
        plan.audio_normalization = tracks;
        plan.verification_scope = SCOPE.into();
        plan.argv_template = invocation(&plan, OutputProfile::Matroska)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }

    pub(super) async fn verify_normalization(
        &self,
        plan: &Plan,
        observed: &MediaProbe,
        identity: &FileIdentity,
        cancel: CancellationToken,
    ) -> Result<Vec<String>> {
        let path = Path::new(&identity.path);
        let source = files::SourceStamp::capture_resolved(path)?;
        let asset = Asset {
            source_object: Some(source.object_identity()?),
            display_name: String::new(),
            id: plan.source.id.clone(),
            creator: plan.creator.clone(),
            identity: identity.clone(),
            probe: observed.clone(),
        };
        let mut results = Vec::with_capacity(plan.audio_normalization.len());
        for track in &plan.audio_normalization {
            let position = output_position(plan, track.request.stream_index)?;
            let index = observed.streams[position].index;
            let held = files::SourceStamp::capture_open_resolved(path, source.try_clone_file()?)?;
            let fresh = self.measure_loudness(&asset, held, index, cancel.clone()).await?;
            let target = track.request.settings;
            let accepted = fresh.integrated_centi_lufs.is_some_and(|actual| {
                actual.abs_diff(target.integrated_centi_lufs) <= 40
            }) && fresh.true_peak_centi_dbtp.is_some_and(|actual| actual <= target.true_peak_centi_dbtp)
                && fresh.range_centi_lu <= target.range_centi_lu;
            if !accepted {
                return Err(DomainError::new("VERIFICATION_FAILED", "Independent output loudness did not meet the reviewed target and peak limit"));
            }
            results.push(format!("Output audio {index} measured {} centi-LUFS and {} centi-dBTP",
                fresh.integrated_centi_lufs.expect("verified loudness"), fresh.true_peak_centi_dbtp.expect("verified peak")));
        }
        blocking(move || source.revalidate()).await?;
        Ok(results)
    }
}

fn output_position(plan: &Plan, stream_index: u32) -> Result<usize> {
    plan.resolution.streams.iter().position(|stream| stream.index == stream_index)
        .ok_or_else(|| unavailable("Normalized audio must be retained by the reviewed stream map"))
}

pub(super) fn invocation(plan: &Plan, profile: OutputProfile) -> Result<Vec<String>> {
    if profile != OutputProfile::Matroska
        || crate::admission::frozen_demuxer(&plan.source.probe.format)? != "matroska"
        || plan.resolution.action != CleanAction::Remux
        || plan.resolution.streams != plan.source.probe.streams
    {
        return Err(unavailable("PCM normalization requires an original Matroska source and a new .mkv output"));
    }
    let mut args = lower_remux(&plan.source, &plan.resolution, profile)?;
    let output = args.len().checked_sub(3).expect("Shared copy output clause");
    let mut audio = Vec::new();
    for track in &plan.audio_normalization {
        let position = output_position(plan, track.request.stream_index)?;
        let rate = rate(&plan.source.probe, track.request.stream_index)?;
        let graph = track.second_pass(rate)?;
        audio.extend([format!("-c:{position}"), "pcm_s16le".into(), format!("-filter:{position}"), graph]);
    }
    args.splice(output..output, audio);
    Ok(args)
}
