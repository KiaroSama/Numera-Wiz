//! Encode a reviewed SDR source using the identified offline CPU profile.
use crate::output_profile::OutputProfile;
use crate::{Engine, Result, lower_remux, plan_digest};
use nw_domain::{
    DomainError,
    avc_export::{
        Audio, Container, Detail, Encode, PictureLook, Preset, RateMode, ResizeAspect, Rotation, Strength,
        VideoCodec,
    },
    mux::{CleanAction, CleanResolution, Stream, StreamKind, StreamOutcome},
    records::{Asset, MediaProbe, Plan},
    time::Rational,
};
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub(super) const SCOPE: &str = "Verify source and AVC output picture counts and presentation clocks, raster, aspect and observed SDR signaling. Optional AAC output verifies original rate, channel count and bounded coded audio timing.";
const HEVC_SCOPE: &str = "Verify source and HEVC output picture counts and presentation clocks, raster, aspect and observed SDR signaling. MP4 verifies hvc1 signaling. Optional AAC output verifies original rate, channel count and bounded coded audio timing.";
const AV1_SCOPE: &str = "Verify source and AV1 output picture counts and presentation clocks, raster, aspect and observed SDR signaling. MP4 verifies av01 signaling. Optional AAC output verifies original rate, channel count and bounded coded audio timing.";

pub(super) fn scope(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::H264 => SCOPE,
        VideoCodec::Hevc => HEVC_SCOPE,
        VideoCodec::Av1 => AV1_SCOPE,
    }
}

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

struct Raster {
    width: u64,
    height: u64,
    sar: Rational,
}

fn append_look(filters: &mut Vec<String>, look: PictureLook, raster: &Raster) -> Result<()> {
    let mut color = Vec::new();
    for (field, value) in [
        ("brightness", look.brightness),
        ("contrast", look.contrast),
        (
            "saturation",
            if look.grayscale {
                Some(Rational::ZERO)
            } else {
                look.saturation
            },
        ),
        ("gamma", look.gamma),
    ] {
        if let Some(value) = value {
            color.push(format!(
                "{field}={}/{}",
                value.numerator(),
                value.denominator()
            ));
        }
    }
    if !color.is_empty() {
        filters.push(format!("eq={}", color.join(":")));
    }
    if let Some(strength) = look.denoise {
        filters.push(
            match strength {
                Strength::Light => "hqdn3d=2:1.5:3:2.25",
                Strength::Medium => "hqdn3d=4:3:6:4.5",
                Strength::Heavy => "hqdn3d=8:6:12:9",
            }
            .into(),
        );
    }
    match look.detail {
        Some(Detail::Sharpen { strength }) => {
            let amount = match strength {
                Strength::Light => "0.5",
                Strength::Medium => "1.0",
                Strength::Heavy => "1.5",
            };
            filters.push(format!("unsharp=5:5:{amount}:5:5:0.0"));
        }
        Some(Detail::Blur { strength }) => {
            let radius = match strength {
                Strength::Light => 2,
                Strength::Medium => 5,
                Strength::Heavy => 10,
            };
            if raster.width.min(raster.height) < 4 * radius {
                return Err(unavailable(
                    "The reviewed output picture is too small for this blur strength",
                ));
            }
            filters.push(format!("boxblur={radius}:1"));
        }
        None => {}
    }
    Ok(())
}

fn raster(facts: &serde_json::Value) -> Result<Raster> {
    let invalid = || unavailable("Picture dimensions or sample aspect are missing or invalid");
    let dimension = |field: &str| {
        facts[field]
            .as_u64()
            .filter(|value| (2..=4096).contains(value))
            .ok_or_else(invalid)
    };
    let sar = match facts["sample_aspect_ratio"].as_str() {
        None | Some("0:1" | "N/A") => Rational::new(1, 1)?,
        Some(aspect) => {
            let (num, den) = aspect.split_once(':').ok_or_else(invalid)?;
            let num: u16 = num.parse().map_err(|_| invalid())?;
            let den: u16 = den.parse().map_err(|_| invalid())?;
            if num == 0 || den == 0 {
                return Err(invalid());
            }
            Rational::new(i64::from(num), i64::from(den))?
        }
    };
    Ok(Raster {
        width: dimension("width")?,
        height: dimension("height")?,
        sar,
    })
}

fn picture_filter(facts: &serde_json::Value, settings: &Encode) -> Result<(Raster, Vec<String>)> {
    let mut output = raster(facts)?;
    let Some(picture) = settings.picture else {
        return Ok((output, Vec::new()));
    };
    let mut filters = Vec::new();
    if let Some(crop) = picture.crop {
        if u64::from(crop.width) + u64::from(crop.x) > output.width
            || u64::from(crop.height) + u64::from(crop.y) > output.height
        {
            return Err(unavailable(
                "The crop must fit inside the registered original raster",
            ));
        }
        filters.push(format!(
            "crop={}:{}:{}:{}:exact=1",
            crop.width, crop.height, crop.x, crop.y
        ));
        output.width = u64::from(crop.width);
        output.height = u64::from(crop.height);
    }
    match picture.rotation {
        Rotation::None => {}
        Rotation::HalfTurn => filters.extend(["hflip".into(), "vflip".into()]),
        Rotation::Clockwise | Rotation::Counterclockwise => {
            let direction = match picture.rotation {
                Rotation::Clockwise => "clock",
                _ => "cclock",
            };
            filters.push(format!("transpose={direction}"));
            std::mem::swap(&mut output.width, &mut output.height);
            output.sar = Rational::new(output.sar.denominator(), output.sar.numerator())?;
        }
    }
    if picture.flip_horizontal {
        filters.push("hflip".into());
    }
    if picture.flip_vertical {
        filters.push("vflip".into());
    }
    if let Some(resize) = picture.resize {
        let width = i64::from(resize.width);
        let height = i64::from(resize.height);
        output.sar = match resize.aspect {
            ResizeAspect::PreserveDisplay => output.sar.checked_mul(Rational::new(
                output.width as i64 * height,
                output.height as i64 * width,
            )?)?,
            ResizeAspect::SquarePixels | ResizeAspect::Fit => Rational::new(1, 1)?,
        };
        output.width = u64::from(resize.width);
        output.height = u64::from(resize.height);
        if resize.aspect == ResizeAspect::Fit {
            filters.push(format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2:reset_sar=1:flags=lanczos"
            ));
            filters.push(format!("pad={width}:{height}:(ow-iw)/2:(oh-ih)/2"));
        } else {
            filters.push(format!("scale={width}:{height}:flags=lanczos"));
        }
    }
    if output.width > 4096
        || output.height > 2160
        || output.sar.numerator() > i64::from(u16::MAX)
        || output.sar.denominator() > i64::from(u16::MAX)
    {
        return Err(unavailable(
            "Choose an AVC raster and sample aspect within the verified bounds",
        ));
    }
    if !filters.is_empty() {
        filters.push(format!(
            "setsar={}/{}:max=65535",
            output.sar.numerator(),
            output.sar.denominator()
        ));
    }
    if let Some(look) = picture.look {
        append_look(&mut filters, look, &output)?;
    }
    Ok((output, filters))
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
    if picture.kind != StreamKind::Video
        || picture.attached_picture()
        || !matches!(picture.codec.as_str(), "ffv1" | "h264" | "hevc" | "av1")
    {
        return Err(unavailable(
            "Choose an original SDR FFV1, H.264, HEVC or AV1 picture",
        ));
    }
    if let Audio::Aac { stream_index, .. } = settings.audio {
        let audio = streams
            .iter()
            .find(|track| {
                track.index == stream_index
                    && track.kind == StreamKind::Audio
                    && matches!(track.codec.as_str(), "pcm_s16le" | "aac")
            })
            .ok_or_else(|| unavailable("Choose a PCM s16le or AAC audio stream"))?;
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
    resolved.reasons = vec![format!(
        "Encode an original SDR picture using {} and the reviewed CPU preset",
        settings.codec.encoder()
    )];
    Ok(resolved)
}

pub(super) fn admit(source: &Asset, resolved: &CleanResolution) -> Result<()> {
    if !matches!(
        crate::admission::frozen_demuxer(&source.probe.format)?,
        "matroska" | "mov"
    ) || resolved.preservation.chapters
        || resolved.preservation.global_tags
        || resolved.preservation.stream_tags
        || resolved.preservation.dispositions
        || resolved.action != CleanAction::Remux
    {
        return Err(unavailable(
            "The AVC route takes registered Matroska or self-contained MP4/MOV and publishes the selected picture and audio",
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
    let clock = crate::preservation::stream_clock(&source.probe, picture.index)?;
    if details["pix_fmt"] != "yuv420p"
        || clock.numerator() != 1
        || clock.denominator() > i64::from(i32::MAX)
        || !even("width", 4096)
        || !even("height", 2160)
        || details["field_order"]
            .as_str()
            .is_some_and(|order| !matches!(order, "unknown" | "progressive"))
        || details["side_data_list"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        || details["color_transfer"]
            .as_str()
            .is_some_and(|tag| !matches!(tag, "unknown" | "bt709" | "iec61966-2-1" | "smpte170m"))
    {
        return Err(unavailable(
            "Use a progressive yuv420p SDR picture through 4096×2160 on an exact representable stream clock",
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
        plan.verification_scope = scope(settings.codec).into();
        plan.avc_export = Some(settings);
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
                "Encoded output differs from the reviewed picture and audio format",
            )
        };
        let primary = observed.streams.first().ok_or_else(invalid)?;
        if primary.index != 0
            || primary.codec != encode.codec.probe_name()
            || primary.kind != StreamKind::Video
            || observed.streams.len() != plan.resolution.streams.len()
        {
            return Err(invalid());
        }
        let source_picture = stream(&plan.source.probe, encode.video_stream)?;
        let output_picture = stream(observed, primary.index)?;
        let original_is_compressed =
            matches!(plan.resolution.streams[0].codec.as_str(), "h264" | "hevc" | "av1");
        if let Some(tag) = mp4_codec_tag(encode)
            && output_picture["codec_tag_string"] != tag
        {
            return Err(invalid());
        }
        let (expected, _) = picture_filter(source_picture, encode)?;
        let actual = raster(output_picture)?;
        if (expected.width, expected.height, expected.sar)
            != (actual.width, actual.height, actual.sar)
        {
            return Err(invalid());
        }
        for field in [
            "pix_fmt",
            "color_space",
            "color_transfer",
            "color_primaries",
            "color_range",
        ] {
            let expected = &source_picture[field];
            let actual = &output_picture[field];
            let missing = |value: &serde_json::Value| value.is_null() || value == "unknown";
            // AVC defaults an absent video_full_range_flag to limited.
            // MP4 and Matroska can expose the same range differently.
            let implicit_limited_range = field == "color_range"
                && (expected == "tv" || (original_is_compressed || encode.codec == VideoCodec::Av1) && missing(expected))
                && (actual == "tv" || missing(actual));
            if !(expected == actual
                || missing(expected) && missing(actual)
                || implicit_limited_range)
            {
                return Err(DomainError::new(
                    "VERIFICATION_FAILED",
                    format!("Encoded picture differs from the reviewed {field}"),
                ));
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

fn mp4_codec_tag(settings: &Encode) -> Option<&'static str> {
    match (settings.container, settings.codec) {
        (Container::Mp4, VideoCodec::Hevc) => Some("hvc1"),
        (Container::Mp4, VideoCodec::Av1) => Some("av01"),
        _ => None,
    }
}

fn svt_parameters(original: &serde_json::Value, strict_size: bool) -> String {
    // SVT needs a nominal rate for its encoder model. Keep the exact PTS grid
    // independent, including irregular sources with a 1 kHz guessed rate.
    let (num, den) = ["avg_frame_rate", "r_frame_rate"].into_iter().find_map(|key| {
        let (num, den) = original[key].as_str()?.split_once('/')?;
        let (num, den) = (num.parse::<u32>().ok()?, den.parse::<u32>().ok()?);
        (num > 0 && (1..=1001).contains(&den) && num <= 240 * den).then_some((num, den))
    }).unwrap_or((25, 1));
    // FFmpeg selects SVT CBR when the reviewed peak equals the target. CBR
    // requires low delay and uses its buffer without the separate VBR maximum.
    let rate_control = if strict_size { ":pred-struct=1:mbr=0" } else { "" };
    format!("lp=1:fps-num={num}:fps-denom={den}{rate_control}")
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
    let clock = crate::preservation::stream_clock(&plan.source.probe, settings.video_stream)?;
    if settings.container == Container::Matroska && 1000 % clock.denominator() != 0 {
        return Err(unavailable(
            "Use MP4 for original stream timestamps that may require sub-millisecond precision",
        ));
    }
    let preset = match (settings.codec, settings.preset) {
        (VideoCodec::Av1, Preset::Fast) => "10",
        (VideoCodec::Av1, Preset::Balanced) => "8",
        (VideoCodec::Av1, Preset::Slow) => "6",
        (_, Preset::Fast) => "fast",
        (_, Preset::Balanced) => "medium",
        (_, Preset::Slow) => "slow",
    };
    let original = stream(&plan.source.probe, settings.video_stream)?;
    let (raster, filters) = picture_filter(original, settings)?;
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
        settings.codec.encoder(),
        "-preset",
        preset,
        "-pix_fmt",
        "yuv420p",
    ].map(str::to_owned).to_vec();
    if settings.codec != VideoCodec::Av1 {
        export.extend(["-bf".into(), "0".into()]);
    }
    export.extend([
        "-threads:v",
        "2",
        "-fps_mode:v",
        "passthrough",
        "-enc_time_base:v",
    ]
    .map(str::to_owned));
    export.push(format!("1/{}", clock.denominator()));
    if settings.codec == VideoCodec::Hevc {
        export.extend([
            "-x265-params".into(),
            "bframes=0:pools=none:frame-threads=2:log-level=error".into(),
        ]);
    }
    if settings.codec == VideoCodec::Av1 {
        if raster.width < 64 || raster.height < 64 {
            return Err(unavailable("SVT-AV1 needs an encoded raster of at least 64×64"));
        }
        let strict_size = settings.bitrate.is_some_and(|bitrate| bitrate.mode == RateMode::StrictSize);
        export.extend(["-svtav1-params".into(), svt_parameters(original, strict_size)]);
    }
    if let Some(crf) = settings.crf {
        export.extend(["-crf".into(), crf.to_string()]);
    }
    if let Some(bitrate) = settings.bitrate {
        let (peak, buffer) = bitrate.limits();
        export.extend([
            "-b:v".into(),
            format!("{}k", bitrate.average_kbps),
            "-maxrate:v".into(),
            format!("{peak}k"),
            "-bufsize:v".into(),
            format!("{buffer}k"),
        ]);
    }
    if !filters.is_empty() {
        export.extend(["-filter:v".into(), filters.join(",")]);
    }
    if let Audio::Aac { bitrate_kbps, .. } = settings.audio {
        export.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            format!("{bitrate_kbps}k"),
        ]);
    }
    if settings.container == Container::Mp4 {
        if let Some(tag) = mp4_codec_tag(settings) {
            export.extend(["-tag:v".into(), tag.into()]);
        }
        export.extend(["-movflags".into(), "+faststart".into()]);
        if clock.denominator() != 1000 {
            export.extend([
                "-video_track_timescale".into(),
                clock.denominator().to_string(),
            ]);
        }
    }
    args.splice(end..end, export);
    Ok(args)
}
