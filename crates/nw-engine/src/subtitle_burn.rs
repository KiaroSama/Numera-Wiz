//! Burn a held text subtitle into video using FFmpeg's libass filter.
use super::{Engine, Result, blocking, io_error, lower_remux, plan_digest, registered_object};
use crate::output_profile::OutputProfile;
use nw_domain::{
    DomainError,
    execution::DeadlinePolicy,
    mux::{CleanAction, CleanResolution, Preservation, Stream, StreamKind, StreamOutcome},
    records::{Asset, FileIdentity, MediaProbe, Plan},
    subtitle_burn::{Audio, Burn, BurnInput, Color, SubtitleSource},
};
use nw_platform::{files, process};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub(super) const SCOPE: &str = "Libass renders registered SubRip and the reviewed offline font into lossless FFV1. Output picture geometry, frame presentation times and color signaling are checked. Retained audio uses independently verified original packets.";
pub(super) const FILTER: &str = "@owned_subtitle_filter@";
const FONT: &str = "DejaVuSans.ttf";
const MAX_SUBTITLE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FONT_BYTES: u64 = 4 * 1024 * 1024;
const COLOR_TAGS: [(&str, &str, &str); 4] = [
    ("color_range", "-color_range", "tv"),
    ("color_space", "-colorspace", "bt709"),
    ("color_transfer", "-color_trc", "bt709"),
    ("color_primaries", "-color_primaries", "bt709"),
];

fn unavailable(message: &str) -> DomainError {
    DomainError::new("SUBTITLE_BURN_UNQUALIFIED", message)
}
fn unsupported_text() -> DomainError {
    DomainError::new("SUBTITLE_TEXT_REQUIRED", "Choose a SubRip text track or registered UTF-8 SRT. Bitmap subtitles cannot be rendered by this text workflow.")
}
fn font_path(component: &FileIdentity) -> Result<PathBuf> {
    Path::new(&component.path)
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("fonts").join(FONT))
        .ok_or_else(|| unavailable("The component profile has no offline font directory"))
}

fn selected<'a>(streams: &'a [Stream], burn: &Burn) -> Result<Vec<&'a Stream>> {
    burn.audio.validate()?;
    let picture = streams.iter().find(|stream| {
        stream.index == burn.video_stream && stream.kind == StreamKind::Video && !stream.attached_picture()
    }).ok_or_else(|| unavailable("Choose a registered ordinary picture stream"))?;
    if picture.codec != "ffv1" {
        return Err(unavailable("The lossless original-clock caption route requires an FFV1 picture"));
    }
    let mut output = vec![picture];
    match &burn.audio {
        Audio::None {} => {}
        Audio::CopyAll {} => output.extend(streams.iter().filter(|stream| stream.kind == StreamKind::Audio)),
        Audio::CopySelected { indices } => {
            for index in indices {
                let stream = streams.iter().find(|stream| stream.index == *index && stream.kind == StreamKind::Audio)
                    .ok_or_else(|| unavailable("A selected audio index is absent from the original probe"))?;
                output.push(stream);
            }
        }
    }
    Ok(output)
}

fn resolution(streams: &[Stream], burn: &Burn) -> Result<CleanResolution> {
    let output = selected(streams, burn)?;
    let outcomes = streams.iter().map(|input| {
        let output_index = output.iter().position(|stream| stream.index == input.index).map(|index| index as u32);
        StreamOutcome {
            source_index: input.index,
            output_index,
            reason: match (input.index == burn.video_stream, output_index) {
                (true, _) => "Render the selected text track into the picture",
                (false, Some(_)) => "Preserve the selected original audio packets",
                (false, None) => "Excluded by the picture and audio selection",
            }.into(),
        }
    }).collect();
    Ok(CleanResolution {
        action: CleanAction::Remux,
        streams: output.into_iter().cloned().collect(),
        outcomes,
        reasons: vec!["Render SubRip through libass and encode lossless FFV1".into()],
        warnings: Vec::new(),
        preservation: Preservation {
            additional_video: false, attached_pictures: false, attachments: false,
            data: false, unknown: false, ..Default::default()
        },
        metadata_patch: None,
        chapter_import: None,
    })
}

fn source_picture<'a>(probe: &'a MediaProbe, burn: &Burn) -> Result<&'a Value> {
    let details = probe.stream_details.get(&burn.video_stream)
        .ok_or_else(|| unavailable("Missing registered picture format"))?;
    if details["width"].as_u64().is_none_or(|value| !(2..=4096).contains(&value) || value % 2 != 0)
        || details["height"].as_u64().is_none_or(|value| !(2..=2160).contains(&value) || value % 2 != 0)
        || details["side_data_list"].as_array().is_some_and(|rows| rows.iter().any(|row| {
            !matches!(row["side_data_type"].as_str(), Some("Mastering display metadata" | "Content light level metadata"))
        }))
    {
        return Err(unavailable("Caption burn requires even dimensions through 4096×2160 and an ordinary unrotated picture"));
    }
    Ok(details)
}

fn color_filter(details: &Value, color: Color) -> Result<String> {
    match color {
        Color::PreserveSdr {} => {
            if details["pix_fmt"] != "yuv420p"
                || details["side_data_list"].as_array().is_some_and(|rows| !rows.is_empty())
                || COLOR_TAGS.iter().any(|(key, _, expected)| {
                    details[*key].as_str().is_some_and(|value| value != *expected && value != "unknown")
                })
            {
                return Err(unavailable("The SDR route requires yuv420p and limited BT.709 or unspecified SDR signaling"));
            }
            Ok(String::new())
        }
        Color::ToneMapBt709 { peak_nits } => {
            color.validate()?;
            let transfer = details["color_transfer"].as_str().filter(|value| matches!(*value, "smpte2084" | "arib-std-b67"))
                .ok_or_else(|| unavailable("Tone mapping requires explicitly signaled PQ or HLG"))?;
            if details["color_space"] != "bt2020nc" || details["color_primaries"] != "bt2020"
                || details["color_range"] != "tv" || details["pix_fmt"] != "yuv420p10le"
            {
                return Err(unavailable("The SDR tone map admits limited-range 10-bit BT.2020 input"));
            }
            let peak = format!("{}.{:02}", peak_nits / 100, peak_nits % 100);
            Ok(format!(concat!(
                "zscale=matrixin=bt2020nc:transferin={transfer}:primariesin=bt2020:rangein=limited:",
                "matrix=gbr:primaries=bt2020:range=full:transfer=linear:npl=100,format=gbrpf32le,",
                "zscale=matrixin=0:transferin=linear:primariesin=bt2020:matrix=0:transfer=linear:primaries=bt709,",
                "tonemap=mobius:param=0.3:desat=2:peak={peak},",
                "zscale=matrixin=0:transferin=linear:primariesin=bt709:transfer=bt709:matrix=bt709:primaries=bt709:range=limited,",
                "format=yuv420p,sidedata=mode=delete:type=MASTERING_DISPLAY_METADATA,",
                "sidedata=mode=delete:type=CONTENT_LIGHT_LEVEL,"
            ), transfer=transfer, peak=peak))
        }
    }
}

fn verify_text_source(source: &Asset, burn: &Burn) -> Result<()> {
    match &burn.subtitle {
        SubtitleSource::Embedded { stream_index } => {
            if !source.probe.streams.iter().any(|stream| {
                stream.index == *stream_index && stream.kind == StreamKind::Subtitle && stream.codec == "subrip"
            }) {
                return Err(unsupported_text());
            }
        }
        SubtitleSource::External { source: captions } => {
            if captions.id == source.id || captions.source_object == source.source_object
                || captions.probe.format != "srt" || captions.probe.streams.len() != 1
                || captions.probe.streams[0].codec != "subrip"
                || !captions.identity.bytes.parse::<usize>().is_ok_and(|size| (1..=MAX_SUBTITLE_BYTES).contains(&size))
            {
                return Err(unsupported_text());
            }
            registered_object(captions)?;
        }
    }
    Ok(())
}

impl Engine {
    pub fn subtitle_burn_plan_at(
        &self,
        source: Asset,
        output: &Path,
        request: BurnInput,
        subtitle: SubtitleSource,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        let path = files::source_path(&font_path(&self.ffmpeg)?)?;
        let font_stamp = files::SourceStamp::capture_resolved(&path)?;
        let font = files::identity(&path, &CancellationToken::new())?;
        font_stamp.revalidate()?;
        let burn = Burn {
            video_stream: request.video_stream,
            subtitle,
            audio: request.audio,
            color: request.color,
            quality: request.quality,
            font,
            font_object: font_stamp.object_identity()?,
        };
        verify_text_source(&source, &burn)?;
        let mut plan = self.review_stream_copy(source, output, Default::default(), creator, now, |streams| resolution(streams, &burn))?;
        plan.subtitle_burn = Some(burn);
        plan.verification_scope = SCOPE.into();
        plan.argv_template = invocation(&plan, OutputProfile::Matroska)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }
}

pub(super) fn invocation(plan: &Plan, profile: OutputProfile) -> Result<Vec<String>> {
    let burn = plan.subtitle_burn.as_ref().ok_or_else(|| unavailable("Missing reviewed caption operation"))?;
    if profile != OutputProfile::Matroska || crate::admission::frozen_demuxer(&plan.source.probe.format)? != "matroska"
        || plan.resolution.action != CleanAction::Remux
        || !burn.font.bytes.parse::<u64>().is_ok_and(|size| (1..=MAX_FONT_BYTES).contains(&size))
        || files::source_path(&font_path(&plan.ffmpeg)?)? != Path::new(&burn.font.path)
    {
        return Err(unavailable("Review an FFV1 Matroska route with the installed Numera Wiz font"));
    }
    verify_text_source(&plan.source, burn)?;
    let expected = resolution(&plan.source.probe.streams, burn)?;
    let serialize = |resolution: &CleanResolution| {
        serde_json::to_value(resolution).map_err(|_| unavailable("Cannot evaluate the reviewed caption stream map"))
    };
    if serialize(&expected)? != serialize(&plan.resolution)? {
        return Err(unavailable("The caption stream map differs from the reviewed picture and audio"));
    }
    let details = source_picture(&plan.source.probe, burn)?;
    color_filter(details, burn.color)?;
    let mut args = lower_remux(&plan.source, &plan.resolution, profile)?;
    let input = args.iter().position(|arg| arg == "-i").expect("Shared held source");
    args.splice(input..input, ["-noautorotate", "-threads", "2"].map(str::to_owned));
    let offset = args.len() - 3;
    let mut video = [
        "-filter_threads", "1", "-c:v", "ffv1", "-level:v", "3", "-threads:v", "2",
        "-pix_fmt", "yuv420p", "-fps_mode:v", "passthrough", "-vf", FILTER,
    ].map(str::to_owned).to_vec();
    for (key, control, expected) in COLOR_TAGS {
        if matches!(burn.color, Color::ToneMapBt709 { .. }) || details[key] == expected {
            video.extend([control.into(), expected.into()]);
        }
    }
    args.splice(offset..offset, video);
    Ok(args)
}

pub(super) struct StagedCaption {
    _directory: tempfile::TempDir,
    subtitle: files::SourceStamp,
    font: files::SourceStamp,
    originals: Vec<files::SourceStamp>,
    filter: String,
}
impl StagedCaption {
    pub fn filter(&self) -> &str {
        &self.filter
    }
    pub fn revalidate(&self) -> Result<()> {
        self.subtitle.revalidate()?;
        self.font.revalidate()?;
        for original in &self.originals {
            original.revalidate()?;
        }
        Ok(())
    }
    pub fn into_sources(self) -> Vec<files::SourceStamp> {
        self.originals
    }
}

fn escape_option(value: &str) -> String {
    fn level(value: &str, escape: &str) -> String {
        let mut output = String::with_capacity(value.len());
        for ch in value.chars() {
            if escape.contains(ch) {
                output.push('\\');
            }
            output.push(ch);
        }
        output
    }
    level(&level(value, "\\':"), "\\'[],;")
}

impl Engine {
    async fn embedded_srt(&self, setup: &Plan, index: u32, cancel: CancellationToken) -> Result<Vec<u8>> {
        let canonical = PathBuf::from(&setup.source.identity.path);
        let cancel_open = cancel.clone();
        let object = registered_object(&setup.source)?.clone();
        let input = blocking(move || {
            let input = crate::admission::open(&canonical, &cancel_open)?;
            let held = files::SourceStamp::capture_open_resolved(&canonical, input.file.try_clone().map_err(io_error)?)?;
            held.require_object(&object)?;
            Ok(input)
        }).await?;
        let mut args = ["-v", "error", "-nostdin", "-copyts", "-max_alloc", "67108864"].map(str::to_owned).to_vec();
        args.extend(input.options());
        args.extend(["-i".into(), "fd:".into(), "-map".into(), format!("0:{index}"),
            "-c:s".into(), "copy".into(), "-f".into(), "srt".into(), "pipe:1".into()]);
        let ticket = self.process_budget.acquire(&cancel).await?;
        let result = process::run_readonly(Path::new(&self.ffmpeg.path), &args,
            Duration::from_secs(60), MAX_SUBTITLE_BYTES, input.file, cancel).await;
        let bytes = ticket.finish(result)?.stdout;
        if bytes.is_empty() || std::str::from_utf8(&bytes).is_err() {
            return Err(unsupported_text());
        }
        Ok(bytes)
    }

    pub(super) async fn stage_caption(&self, plan: &Plan, parent: &Path, cancel: CancellationToken) -> Result<StagedCaption> {
        let burn = plan.subtitle_burn.as_ref().expect("Reviewed caption");
        let prefix = color_filter(source_picture(&plan.source.probe, burn)?, burn.color)?;
        let directory = tempfile::Builder::new().prefix("caption-input-").tempdir_in(parent).map_err(io_error)?;
        let font_dir = nw_platform::security::private_directory(&directory.path().join("fonts"))?;
        let mut originals = Vec::new();
        let output = directory.path().join("captions.srt");
        let original = match &burn.subtitle {
            SubtitleSource::Embedded { stream_index } => {
                let bytes = self.embedded_srt(plan, *stream_index, cancel.clone()).await?;
                nw_platform::security::write_private(&output, &bytes, false)?;
                None
            }
            SubtitleSource::External { source } => {
                Some(stage_file(&source.identity, registered_object(source)?, &output, &cancel)?)
            }
        };
        originals.extend(original);
        let font = font_dir.join(FONT);
        originals.push(stage_file(&burn.font, &burn.font_object, &font, &cancel)?);
        let filter = format!("{prefix}subtitles=filename={}:fontsdir={}:force_style={}:wrap_unicode=1",
            escape_option(&files::path_text(&output)?), escape_option(&files::path_text(&font_dir)?),
            escape_option("FontName=DejaVu Sans"));
        Ok(StagedCaption {
            _directory: directory,
            originals,
            subtitle: files::SourceStamp::capture_resolved(&output)?,
            font: files::SourceStamp::capture_resolved(&font)?,
            filter,
        })
    }
}

fn stage_file(
    identity: &FileIdentity,
    object: &nw_domain::records::NativeObjectIdentity,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<files::SourceStamp> {
    let parent = output.parent().expect("Owned staging directory");
    let temp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    let (temp, _, stamp) = files::copy_into_staging_observed(identity, object, temp, cancel,
        DeadlinePolicy::Finite { seconds: 30 }, &mut |_| Ok(()))?;
    temp.persist_noclobber(output).map_err(|error| io_error(error.error))?;
    stamp.revalidate()?;
    Ok(stamp)
}

pub(super) fn verify_picture(plan: &Plan, probe: &MediaProbe, output: u32) -> Result<()> {
    let burn = plan.subtitle_burn.as_ref().expect("Reviewed caption");
    let source = source_picture(&plan.source.probe, burn)?;
    let result = probe.stream_details.get(&output).ok_or_else(|| unavailable("Missing rendered picture attributes"))?;
    let failed = || DomainError::new("VERIFICATION_FAILED", "Caption rendering changed the original geometry or missed the reviewed SDR signaling");
    if result["codec_name"] != "ffv1" || result["pix_fmt"] != "yuv420p"
        || ["width", "height", "sample_aspect_ratio"].into_iter().any(|key| source[key] != result[key])
        || COLOR_TAGS.into_iter().any(|(key, _, expected)| {
            if matches!(burn.color, Color::ToneMapBt709 { .. }) || source[key] == expected {
                result[key] != expected
            } else {
                result[key].as_str().is_some_and(|tag| tag != "unknown")
            }
        })
        || result["side_data_list"].as_array().is_some_and(|rows| !rows.is_empty())
    {
        return Err(failed());
    }
    Ok(())
}
