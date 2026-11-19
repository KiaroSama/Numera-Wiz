//! Render one original PCM track with ordered sample-domain edits.
use crate::output_profile::OutputProfile;
use crate::{Engine, blocking, lower_remux, plan_digest, registered_object};
use nw_domain::{
    DomainError, Result,
    audio_edit::{Edit, Piece},
    mux::{CleanResolution, Stream, StreamKind},
    records::{Asset, MediaProbe, Plan},
};
use nw_platform::files;
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub(super) const SCOPE: &str = "Verify the edited PCM sample count, rate, channel layout and 16-bit precision. Unity-tempo ranges use exact sample boundaries. Tempo processing uses the reviewed overlap-add duration tolerance.";
const MAX_SOURCE_BYTES: u64 = 1024 * 1024 * 1024;

fn unavailable(message: &str) -> DomainError {
    DomainError::new("AUDIO_EDIT_UNQUALIFIED", message)
}

pub(super) fn format(probe: &MediaProbe, index: u32) -> Result<(u32, u8, &str)> {
    if probe.streams.iter().all(|track| {
        track.index != index || track.kind != StreamKind::Audio || track.codec != "pcm_s16le"
    }) {
        return Err(unavailable("Choose an original PCM s16le audio track"));
    }
    let details = probe
        .stream_details
        .get(&index)
        .ok_or_else(|| unavailable("Missing original audio parameters"))?;
    let rate = details["sample_rate"]
        .as_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| unavailable("Missing original sample rate"))?;
    let channels = details["channels"]
        .as_u64()
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| unavailable("Missing original channel count"))?;
    let layout = match (channels, details["channel_layout"].as_str()) {
        (1, None | Some("mono")) => "mono",
        (2, None | Some("stereo")) => "stereo",
        (6, Some(layout @ ("5.1" | "5.1(side)"))) => layout,
        _ => {
            return Err(unavailable(
                "Choose mono, stereo or an original explicitly labeled 5.1 channel layout",
            ));
        }
    };
    Ok((rate, channels, layout))
}

fn resolution(streams: &[Stream], index: u32) -> Result<CleanResolution> {
    let mut resolution = nw_domain::mux::extract_one(streams, index)?;
    resolution.preservation.global_tags = false;
    resolution.preservation.stream_tags = false;
    resolution.preservation.dispositions = false;
    resolution.streams[0].tags.clear();
    resolution.streams[0].dispositions.clear();
    resolution.reasons = vec!["Render the ordered sample intervals into a PCM WAVE file".into()];
    if let Some(outcome) = resolution
        .outcomes
        .iter_mut()
        .find(|outcome| outcome.source_index == index)
    {
        outcome.reason = "Render the selected original audio stream".into();
    }
    Ok(resolution)
}

impl Engine {
    pub async fn inspect_audio_edit(
        &self,
        source: &Asset,
        index: u32,
        pieces: Vec<Piece>,
        cancel: CancellationToken,
    ) -> Result<(files::SourceStamp, Edit)> {
        if !(1..=16).contains(&pieces.len()) {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Select one to sixteen ordered sample intervals",
            ));
        }
        if !matches!(
            crate::admission::frozen_demuxer(&source.probe.format)?,
            "wav" | "matroska"
        ) || !source
            .identity
            .bytes
            .parse::<u64>()
            .is_ok_and(|bytes| bytes <= MAX_SOURCE_BYTES)
        {
            return Err(unavailable(
                "Sample edits admit registered WAVE or Matroska sources up to 1 GiB",
            ));
        }
        let (sample_rate, channels, layout) = format(&source.probe, index)?;
        let path = Path::new(&source.identity.path);
        let identity = source.identity.clone();
        let object = registered_object(source)?.clone();
        let token = cancel.clone();
        let stamp = blocking(move || {
            let stamp = files::SourceStamp::capture_resolved(Path::new(&identity.path))?;
            stamp.require_object(&object)?;
            files::revalidate(&identity, &token)?;
            stamp.revalidate()?;
            Ok(stamp)
        })
        .await?;
        let source_samples =
            crate::preservation::pcm_frames(self, path, &source.probe, index, channels, cancel)
                .await?;
        stamp.revalidate()?;
        let edit = Edit {
            stream_index: index,
            source_samples,
            sample_rate,
            channels,
            channel_layout: layout.into(),
            pieces,
        };
        edit.output_samples()?;
        Ok((stamp, edit))
    }

    pub fn audio_edit_plan_at(
        &self,
        source: Asset,
        output: &Path,
        edit: Edit,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        let mut plan = self.review_stream_copy(
            source,
            output,
            Default::default(),
            creator,
            now,
            |streams| resolution(streams, edit.stream_index),
        )?;
        plan.audio_edit = Some(edit);
        plan.verification_scope = SCOPE.into();
        plan.argv_template = invocation(&plan, OutputProfile::WavePcm)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }

    pub(super) async fn verify_audio_edit(
        &self,
        plan: &Plan,
        path: &Path,
        probe: &MediaProbe,
        cancel: CancellationToken,
    ) -> Result<String> {
        let edit = plan.audio_edit.as_ref().expect("Reviewed audio edit");
        let failed = || {
            DomainError::new(
                "VERIFICATION_FAILED",
                "Edited PCM differs from the reviewed sample count or channel layout",
            )
        };
        let (rate, channels, layout) = format(probe, 0)?;
        let (nominal, tolerance) = edit.output_samples()?;
        if probe.format != "wav"
            || probe.streams.len() != 1
            || rate != edit.sample_rate
            || channels != edit.channels
            || layout != edit.channel_layout
            || probe.stream_details[&0]["bits_per_sample"] != 16
        {
            return Err(failed());
        }
        let samples =
            crate::preservation::pcm_frames(self, path, probe, 0, channels, cancel).await?;
        if u64::from(samples).abs_diff(nominal) > tolerance {
            return Err(failed());
        }
        Ok(format!(
            "Verified {samples} PCM sample frames at {rate} Hz, signed 16 bit, {layout}; reviewed nominal {nominal}, allowed deviation {tolerance} sample frames."
        ))
    }
}

fn tempo_filters(hundredths: u16) -> Vec<String> {
    if hundredths == 100 {
        return Vec::new();
    }
    let mut tempo = f64::from(hundredths) / 100.0;
    let mut filters = Vec::new();
    while tempo < 0.5 {
        filters.push("atempo=0.5".into());
        tempo *= 2.0;
    }
    while tempo > 2.0 {
        filters.push("atempo=2".into());
        tempo /= 2.0;
    }
    filters.push(format!("atempo={tempo:.6}"));
    filters
}

fn graph(edit: &Edit) -> String {
    let count = edit.pieces.len();
    let mut chains = Vec::new();
    if count > 1 {
        let branches = (0..count)
            .map(|index| format!("[in{index}]"))
            .collect::<String>();
        chains.push(format!("[0:{}]asplit={count}{branches}", edit.stream_index));
    }
    for (index, piece) in edit.pieces.iter().enumerate() {
        let input = if count == 1 {
            format!("[0:{}]", edit.stream_index)
        } else {
            format!("[in{index}]")
        };
        let mut parts = vec![format!(
            "atrim=start_sample={}:end_sample={}",
            piece.start_sample, piece.end_sample
        )];
        if piece.reverse {
            parts.push("areverse".into());
        }
        parts.extend(tempo_filters(piece.tempo_hundredths));
        parts.push("asetpts=N/SR/TB".into());
        let output = if count == 1 {
            "[edited]".to_owned()
        } else {
            format!("[piece{index}]")
        };
        chains.push(format!("{input}{}{output}", parts.join(",")));
    }
    if count > 1 {
        let pieces = (0..count)
            .map(|index| format!("[piece{index}]"))
            .collect::<String>();
        chains.push(format!("{pieces}concat=n={count}:v=0:a=1[edited]"));
    }
    chains.join(";")
}

pub(super) fn invocation(plan: &Plan, profile: OutputProfile) -> Result<Vec<String>> {
    let edit = plan
        .audio_edit
        .as_ref()
        .ok_or_else(|| unavailable("Missing reviewed audio edit"))?;
    let (rate, channels, layout) = format(&plan.source.probe, edit.stream_index)?;
    if profile != OutputProfile::WavePcm
        || OutputProfile::from_output(Path::new(&plan.output))? != profile
        || (rate, channels, layout)
            != (
                edit.sample_rate,
                edit.channels,
                edit.channel_layout.as_str(),
            )
        || plan.resolution.streams
            != resolution(&plan.source.probe.streams, edit.stream_index)?.streams
        || plan.resolution.preservation.chapters
        || plan.resolution.preservation.global_tags
        || plan.resolution.preservation.stream_tags
        || plan.resolution.preservation.dispositions
    {
        return Err(unavailable(
            "Review the original sample layout and a new .wav destination",
        ));
    }
    edit.output_samples()?;
    let mut args = lower_remux(&plan.source, &plan.resolution, profile)?;
    let input = args
        .iter()
        .position(|argument| argument == "fd:")
        .expect("Shared held source");
    args.truncate(input + 1);
    args.extend([
        "-filter_complex_threads".into(),
        "1".into(),
        "-filter_complex".into(),
        graph(edit),
        "-map".into(),
        "[edited]".into(),
        "-map_chapters".into(),
        "-1".into(),
        "-map_metadata".into(),
        "-1".into(),
        "-fflags".into(),
        "+bitexact".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "wav".into(),
        "@owned_staging@".into(),
    ]);
    Ok(args)
}
