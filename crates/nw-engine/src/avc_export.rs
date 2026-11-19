//! Encode a reviewed SDR source using the identified offline CPU profile.
use crate::output_profile::OutputProfile;
use crate::{Engine, Result, lower_remux, plan_digest};
use nw_domain::{
    DomainError,
    avc_export::{Audio, Container, Encode, Preset},
    mux::{CleanAction, CleanResolution, Stream, StreamKind, StreamOutcome},
    records::{Asset, MediaProbe, Plan},
};
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub(super) const SCOPE: &str = "Verify source and AVC output picture counts and presentation clocks, raster, aspect and observed SDR signaling. Optional AAC output verifies original rate, channel count and bounded coded audio timing.";

fn unavailable(message: &str) -> DomainError {
    DomainError::new("AVC_EXPORT_UNQUALIFIED", message)
}

pub(super) fn profile(settings: &Encode, path: &Path) -> Result<OutputProfile> {
    match (
        settings.container,
        path.extension().and_then(|name| name.to_str()),
    ) {
        (Container::Matroska, Some(ext)) if ext.eq_ignore_ascii_case("mkv") => {
            Ok(OutputProfile::Matroska)
        }
        (Container::Mp4, Some(ext)) if ext.eq_ignore_ascii_case("mp4") => Ok(OutputProfile::AvcMp4),
        _ => Err(unavailable(
            "Select the filename extension of the reviewed Matroska or MP4 container",
        )),
    }
}

fn stream(probe: &MediaProbe, index: u32) -> Result<&serde_json::Value> {
    probe
        .stream_details
        .get(&index)
        .ok_or_else(|| unavailable("Missing original media facts"))
}

fn audio_format(probe: &MediaProbe, index: u32) -> Result<(u32, u64)> {
    let facts = stream(probe, index)?;
    let sample_rate = facts["sample_rate"]
        .as_str()
        .and_then(|rate| rate.parse::<u32>().ok())
        .filter(|rate| matches!(*rate, 44100 | 48000))
        .ok_or_else(|| unavailable("Use audio at 44.1 or 48 kHz"))?;
    let channels = facts["channels"]
        .as_u64()
        .filter(|channels| matches!(*channels, 1 | 2))
        .ok_or_else(|| unavailable("Use original mono or stereo audio for this AAC profile"))?;
    Ok((sample_rate, channels))
}

fn resolution(streams: &[Stream], settings: &Encode) -> Result<CleanResolution> {
    let mut resolved = nw_domain::mux::extract_one(streams, settings.video_stream)?;
    let picture = &resolved.streams[0];
    if picture.kind != StreamKind::Video || picture.attached_picture() || picture.codec != "ffv1" {
        return Err(unavailable("Choose an original SDR FFV1 picture"));
    }
    if let Audio::Aac { stream_index, .. } = settings.audio {
        let audio = streams
            .iter()
            .find(|track| {
                track.index == stream_index
                    && track.kind == StreamKind::Audio
                    && track.codec == "pcm_s16le"
            })
            .ok_or_else(|| unavailable("Choose an original PCM s16le audio stream for AAC"))?;
        resolved.streams.push(audio.clone());
    }
    resolved.outcomes = streams
        .iter()
        .map(|original| {
            let output_index = resolved
                .streams
                .iter()
                .position(|track| track.index == original.index)
                .map(|index| index as u32);
            StreamOutcome {
                source_index: original.index,
                output_index,
                reason: if output_index.is_some() {
                    "Encode the selected source stream"
                } else {
                    "Excluded by the export stream selection"
                }
                .into(),
            }
        })
        .collect();
    resolved.preservation.global_tags = false;
    resolved.preservation.stream_tags = false;
    resolved.preservation.dispositions = false;
    for stream in &mut resolved.streams {
        stream.tags.clear();
        stream.dispositions.clear();
    }
    resolved.reasons =
        vec!["Encode an original SDR picture using libx264 and the reviewed CPU preset".into()];
    Ok(resolved)
}

pub(super) fn admit(source: &Asset, resolved: &CleanResolution) -> Result<()> {
    if crate::admission::frozen_demuxer(&source.probe.format)? != "matroska"
        || resolved.preservation.chapters
        || resolved.preservation.global_tags
        || resolved.preservation.stream_tags
        || resolved.preservation.dispositions
        || resolved.action != CleanAction::Remux
    {
        return Err(unavailable(
            "This AVC route takes registered Matroska and publishes the selected picture and audio",
        ));
    }
    let picture = resolved
        .streams
        .first()
        .ok_or_else(|| unavailable("AVC export requires a picture stream"))?;
    let details = stream(&source.probe, picture.index)?;
    let even = |key: &str, max| {
        details[key]
            .as_u64()
            .is_some_and(|value| (2..=max).contains(&value) && value.is_multiple_of(2))
    };
    if details["pix_fmt"] != "yuv420p"
        || details["time_base"] != "1/1000"
        || !even("width", 4096)
        || !even("height", 2160)
        || details["side_data_list"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        || details["color_transfer"]
            .as_str()
            .is_some_and(|tag| !matches!(tag, "unknown" | "bt709" | "iec61966-2-1" | "smpte170m"))
    {
        return Err(unavailable(
            "Use an ordinary yuv420p SDR picture through 4096×2160 on the admitted millisecond clock",
        ));
    }
    if let Some(track) = resolved.streams.get(1) {
        audio_format(&source.probe, track.index)?;
    }
    Ok(())
}

impl Engine {
    pub fn avc_plan_at(
        &self,
        asset: Asset,
        output: &Path,
        settings: Encode,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        settings.validate()?;
        let output_profile = profile(&settings, output)?;
        let mut plan = self.review_resolved_output(
            asset,
            output,
            Default::default(),
            creator,
            now,
            |streams| resolution(streams, &settings).map(|outcome| (outcome, Some(output_profile))),
        )?;
        plan.avc_export = Some(settings);
        plan.verification_scope = SCOPE.into();
        plan.argv_template = invocation(&plan, output_profile)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }

    pub(super) async fn verify_avc(
        &self,
        plan: &Plan,
        path: &Path,
        observed: &MediaProbe,
        cancel: CancellationToken,
        observer: &crate::Observer,
    ) -> Result<String> {
        let encode = plan.avc_export.as_ref().expect("Reviewed AVC export");
        let invalid = || {
            DomainError::new(
                "VERIFICATION_FAILED",
                "Encoded output differs from the reviewed AVC/AAC stream format",
            )
        };
        let primary = observed.streams.first().ok_or_else(invalid)?;
        if primary.index != 0
            || primary.codec != "h264"
            || primary.kind != StreamKind::Video
            || observed.streams.len() != plan.resolution.streams.len()
        {
            return Err(invalid());
        }
        let source_picture = stream(&plan.source.probe, encode.video_stream)?;
        let output_picture = stream(observed, primary.index)?;
        let sar = |facts: &serde_json::Value| -> String {
            facts["sample_aspect_ratio"]
                .as_str()
                .filter(|value| *value != "0:1")
                .unwrap_or("1:1")
                .to_owned()
        };
        if sar(source_picture) != sar(output_picture) {
            return Err(invalid());
        }
        for field in [
            "width",
            "height",
            "pix_fmt",
            "color_space",
            "color_transfer",
            "color_primaries",
            "color_range",
        ] {
            let expected = &source_picture[field];
            let actual = &output_picture[field];
            let missing = |value: &serde_json::Value| value.is_null() || value == "unknown";
            if expected != actual && !(missing(expected) && missing(actual)) {
                return Err(invalid());
            }
        }
        if let Audio::Aac { stream_index, .. } = encode.audio {
            let actual = &observed.streams[1];
            if actual.index != 1
                || actual.codec != "aac"
                || actual.kind != StreamKind::Audio
                || audio_format(&plan.source.probe, stream_index)?
                    != audio_format(observed, actual.index)?
            {
                return Err(invalid());
            }
        }
        crate::preservation::verify_avc_timing(self, plan, path, observed, cancel, observer).await
    }
}

pub(super) fn invocation(plan: &Plan, output_profile: OutputProfile) -> Result<Vec<String>> {
    let settings = plan
        .avc_export
        .as_ref()
        .ok_or_else(|| unavailable("Missing reviewed AVC export settings"))?;
    settings.validate()?;
    if profile(settings, Path::new(&plan.output))? != output_profile
        || resolution(&plan.source.probe.streams, settings)?.streams != plan.resolution.streams
    {
        return Err(unavailable(
            "The AVC output container or selected source stream map changed",
        ));
    }
    admit(&plan.source, &plan.resolution)?;
    let preset = match settings.preset {
        Preset::Fast => "fast",
        Preset::Balanced => "medium",
        Preset::Slow => "slow",
    };
    let mut args = lower_remux(&plan.source, &plan.resolution, output_profile)?;
    let input = args
        .iter()
        .position(|flag| flag == "-i")
        .expect("Reviewed held source");
    args.splice(
        input..input,
        ["-noautorotate", "-threads", "2"].map(str::to_owned),
    );
    let end = args.len() - 3;
    let mut export = [
        "-c:v",
        "libx264",
        "-preset",
        preset,
        "-pix_fmt",
        "yuv420p",
        "-bf",
        "0",
        "-threads:v",
        "2",
        "-fps_mode:v",
        "passthrough",
        "-enc_time_base:v",
        "1/1000",
        "-crf",
    ]
    .map(str::to_owned)
    .to_vec();
    export.push(settings.crf.to_string());
    if let Audio::Aac { bitrate_kbps, .. } = settings.audio {
        export.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            format!("{bitrate_kbps}k"),
        ]);
    }
    if settings.container == Container::Mp4 {
        export.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.splice(end..end, export);
    Ok(args)
}
