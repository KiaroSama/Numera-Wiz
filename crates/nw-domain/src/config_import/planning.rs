//! Compile the admitted FFmWiz packet-copy settings into the ordinary stream plan.
use super::{FIELDS, ImportedJob, Setting, TrackChoice};
use crate::{
    DomainError, Result,
    mux::{CleanIntent, NoMatch, Selection, Selector, Stream, StreamKind},
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanInput {
    pub asset_id: String,
    pub job_document: ImportedJob,
}

fn field_error(code: &'static str, field: &str, message: &'static str) -> DomainError {
    let mut error = DomainError::new(code, message);
    error.field = Some(format!("job_document.settings.{field}"));
    error
}

fn missing(field: &str) -> DomainError {
    field_error(
        "JOB_FIELD_REQUIRED",
        field,
        "Provide an explicit setting for this job",
    )
}

fn unsupported(field: &str) -> DomainError {
    field_error(
        "JOB_PROFILE_UNSUPPORTED",
        field,
        "This setting requires a media profile beyond packet copy",
    )
}

impl ImportedJob {
    fn get(&self, name: &str) -> Option<&Setting> {
        self.settings.get(name).map(|field| &field.value)
    }

    fn path(&self, name: &str) -> Result<&Path> {
        match self.get(name) {
            Some(Setting::File { path }) if path.len() <= 16384 && !path.contains('\0') => {
                let path = Path::new(path);
                if path.is_absolute() {
                    Ok(path)
                } else {
                    Err(missing(name))
                }
            }
            _ => Err(missing(name)),
        }
    }

    pub fn selects_picture_retime(&self) -> bool {
        matches!(self.get("video_codec"), Some(Setting::Encoder { name }) if name == "ffv1")
    }

    pub fn selects_avc_encode(&self) -> bool {
        matches!(self.get("video_codec"), Some(Setting::Encoder { name }) if matches!(name.as_str(), "h264" | "libx264"))
    }

    pub fn avc_encode(&self, asset: &crate::records::Asset) -> Result<crate::avc_export::EncodeInput> {
        use crate::avc_export::{Audio, Bitrate, Container, Encode, EncodeInput, PictureTransform, Preset, RateMode, Resize, ResizeAspect};
        if self.version != 1 || self.settings.len() > FIELDS.len() || !self.selects_avc_encode() {
            return Err(DomainError::new("INVALID_JOB_DOCUMENT", "Use a version 1 FFmWiz profile with CPU H.264"));
        }
        let source = self.imported_source()?;
        let output = self.path("output_path")?;
        let extension = output.extension().and_then(|ext| ext.to_str()).ok_or_else(|| missing("output_path"))?;
        let container = match extension.to_ascii_lowercase().as_str() {
            "mp4" => Container::Mp4,
            "mkv" => Container::Matroska,
            _ => return Err(unsupported("output_path")),
        };
        for (name, field) in &self.settings {
            let name = name.as_str();
            if !FIELDS.contains(&name) || !(1..=super::MAX_LINES).contains(&field.line) {
                return Err(field_error("INVALID_JOB_DOCUMENT",name,"Unknown field or invalid imported line number"));
            }
            let supported = match (name, &field.value) {
                ("input_path" | "output_path" | "video_codec" | "video_bitrate_kbps" | "video_bitrate_mode"
                    | "audio_tracks" | "audio_codec" | "audio_bitrate_kbps", _) => true,
                ("output_format", Setting::Source) => same_extension(source, output),
                ("output_format", Setting::Choice { value }) => value.eq_ignore_ascii_case(extension),
                ("use_gpu" | "crop" | "cpu_two_pass" | "reverse_video" | "reverse_audio"
                    | "keep_source_metadata" | "keep_embedded_attachments" | "detect_duplicate_audio", Setting::Enabled { value: false }) => true,
                ("crop_top" | "crop_left" | "crop_bottom" | "crop_right", Setting::Integer { value: 0 }) => true,
                ("subtitle_tracks", Setting::Tracks { selection: TrackChoice::None }) => true,
                ("resolution" | "fps" | "color_range" | "audio_sample_rate" | "video_speed" | "audio_speed", Setting::Source) => true,
                ("resolution", Setting::Geometry { mode: super::ScaleMode::Stretch, width: Some(_), height: Some(_) }) => true,
                ("video_speed" | "audio_speed", Setting::Scalar { value }) => value.numerator() == value.denominator(),
                ("audio_speed", Setting::Choice { value }) => value == "match_video",
                ("video_look" | "video_quick" | "audio_volume" | "raw_ffmpeg_args" | "raw_ffmpeg_valued_args", Setting::Disabled) => true,
                ("loudnorm", Setting::Choice { value }) => value == "off",
                ("logging_enabled", Setting::Enabled { .. })
                | ("log_retention_days", Setting::Integer { .. })
                | ("log_level", Setting::Choice { .. })
                | ("gui_engine", Setting::LegacyPolicy { .. }) => true,
                _ => false,
            };
            if !supported {
                return Err(unsupported(name));
            }
        }
        for name in ["keep_source_metadata", "use_gpu"] {
            if !matches!(self.get(name), Some(Setting::Enabled { value: false })) {
                return Err(missing(name));
            }
        }
        if !matches!(self.get("subtitle_tracks"), Some(Setting::Tracks { selection: TrackChoice::None })) {
            return Err(missing("subtitle_tracks"));
        }
        let average_kbps = match self.get("video_bitrate_kbps") {
            Some(Setting::Integer { value }) => *value,
            _ => return Err(missing("video_bitrate_kbps")),
        };
        let mode = match self.get("video_bitrate_mode") {
            None => RateMode::QualityVbr,
            Some(Setting::Choice { value }) if value == "quality_vbr" => RateMode::QualityVbr,
            Some(Setting::Choice { value }) if value == "strict_size" => RateMode::StrictSize,
            _ => return Err(unsupported("video_bitrate_mode")),
        };
        let mut pictures = asset.probe.streams.iter().filter(|stream| stream.kind == StreamKind::Video && !stream.attached_picture());
        let picture = pictures.next().ok_or_else(|| unsupported("video_codec"))?;
        if pictures.next().is_some() {
            return Err(unsupported("video_codec"));
        }
        let details = asset.probe.stream_details.get(&picture.index).ok_or_else(|| missing("input_path"))?;
        let (width, height) = match self.get("resolution") {
            Some(Setting::Geometry { width: Some(width), height: Some(height), .. }) => (*width, *height),
            _ => {
                let dimension = |key: &str| details[key].as_u64().and_then(|number| u32::try_from(number).ok()).ok_or_else(|| missing("resolution"));
                (dimension("width")?, dimension("height")?)
            }
        };
        let audio = match self.get("audio_tracks") {
            Some(Setting::Tracks { selection: TrackChoice::None }) => Audio::Drop {},
            Some(Setting::Tracks { selection }) => {
                let mut tracks: Vec<_> = asset.probe.streams.iter().filter(|track| track.kind == StreamKind::Audio).collect();
                tracks.sort_unstable_by_key(|track| track.index);
                let track = match selection {
                    TrackChoice::All if tracks.len() == 1 => Some(tracks[0]),
                    TrackChoice::Ordinals { values } if values.len() == 1 => tracks.get(values[0] as usize).copied(),
                    _ => None,
                }.ok_or_else(|| unsupported("audio_tracks"))?;
                if !matches!(self.get("audio_codec"), Some(Setting::Encoder { name }) if name == "aac") {
                    return Err(unsupported("audio_codec"));
                }
                let bitrate_kbps = match self.get("audio_bitrate_kbps") {
                    Some(Setting::Integer { value }) => u16::try_from(*value).map_err(|_| unsupported("audio_bitrate_kbps"))?,
                    _ => return Err(missing("audio_bitrate_kbps")),
                };
                Audio::Aac { stream_index: track.index, bitrate_kbps }
            }
            _ => return Err(missing("audio_tracks")),
        };
        let settings = Encode { video_stream: picture.index, container, crf: None,
            bitrate: Some(Bitrate { average_kbps, mode }), preset: Preset::Balanced, audio,
            picture: Some(PictureTransform { resize: Some(Resize { width, height, aspect: ResizeAspect::SquarePixels }), ..Default::default() }),
        };
        settings.validate()?;
        Ok(EncodeInput { asset_id: asset.id.clone(), output: output.to_str().ok_or_else(|| missing("output_path"))?.to_owned(), settings })
    }

    pub fn imported_source(&self) -> Result<&Path> {
        self.path("input_path")
    }

    pub fn picture_retime(
        &self,
        asset: &crate::records::Asset,
    ) -> Result<crate::video_retime::RetimeInput> {
        use crate::{
            time::{Rational, TimeRange},
            video_retime::{AudioPolicy, RetimeInput},
        };
        if self.version != 1 || self.settings.len() > FIELDS.len() || !self.selects_picture_retime()
        {
            return Err(DomainError::new(
                "INVALID_JOB_DOCUMENT",
                "Use a version 1 FFmWiz profile with the FFV1 encoder",
            ));
        }
        let source = self.imported_source()?;
        let output = self.path("output_path")?;
        if !output
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("mkv"))
        {
            return Err(unsupported("output_path"));
        }
        for name in ["audio_tracks", "subtitle_tracks"] {
            if !matches!(
                self.get(name),
                Some(Setting::Tracks {
                    selection: TrackChoice::None
                })
            ) {
                return Err(field_error(
                    "JOB_PROFILE_UNSUPPORTED",
                    name,
                    "This lossless picture job requires an explicit none selection",
                ));
            }
        }
        for (name, field) in &self.settings {
            let name = name.as_str();
            if !FIELDS.contains(&name) || !(1..=super::MAX_LINES).contains(&field.line) {
                return Err(field_error(
                    "INVALID_JOB_DOCUMENT",
                    name,
                    "Unknown field or invalid imported line number",
                ));
            }
            let qualified = match (name, &field.value) {
                ("input_path" | "output_path" | "video_codec" | "video_speed", _) => true,
                (
                    "audio_tracks" | "subtitle_tracks",
                    Setting::Tracks {
                        selection: TrackChoice::None,
                    },
                ) => true,
                ("reverse_video" | "logging_enabled", Setting::Enabled { .. }) => true,
                ("output_format", Setting::Source) => same_extension(source, output),
                ("output_format", Setting::Choice { value }) => value.eq_ignore_ascii_case("mkv"),
                (
                    "use_gpu"
                    | "crop"
                    | "cpu_two_pass"
                    | "reverse_audio"
                    | "keep_embedded_attachments",
                    Setting::Enabled { value: false },
                ) => true,
                (
                    "video_look"
                    | "video_quick"
                    | "raw_ffmpeg_args"
                    | "raw_ffmpeg_valued_args"
                    | "audio_volume",
                    Setting::Disabled,
                ) => true,
                (
                    "fps" | "resolution" | "color_range" | "audio_sample_rate"
                    | "video_bitrate_kbps" | "audio_bitrate_kbps",
                    Setting::Source,
                ) => true,
                ("loudnorm", Setting::Choice { value }) => value == "off",
                ("audio_codec", Setting::Copy) => true,
                ("log_retention_days", Setting::Integer { .. })
                | ("log_level", Setting::Choice { .. })
                | ("gui_engine", Setting::LegacyPolicy { .. }) => true,
                _ => false,
            };
            if !qualified {
                return Err(unsupported(name));
            }
        }
        let tempo_hundredths = match self.get("video_speed") {
            None | Some(Setting::Source) => 100,
            Some(Setting::Scalar { value }) => {
                let value = value.checked_mul(Rational::new(100, 1)?)?;
                u16::try_from(value.numerator())
                    .ok()
                    .filter(|tempo| value.denominator() == 1 && (10..=800).contains(tempo))
                    .ok_or_else(|| unsupported("video_speed"))?
            }
            _ => return Err(unsupported("video_speed")),
        };
        let mut pictures = asset
            .probe
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Video && !stream.attached_picture());
        let picture = pictures.next().ok_or_else(|| unsupported("video_codec"))?;
        if pictures.next().is_some() {
            return Err(field_error(
                "JOB_PROFILE_UNSUPPORTED",
                "video_codec",
                "The imported speed and reverse profile needs one ordinary source picture",
            ));
        }
        let end = asset
            .probe
            .container_duration
            .ok_or_else(|| missing("input_path"))?;
        let range = TimeRange {
            start: Rational::ZERO,
            end,
        };
        range.validate()?;
        Ok(RetimeInput {
            asset_id: asset.id.clone(),
            output: output
                .to_str()
                .ok_or_else(|| missing("output_path"))?
                .to_owned(),
            video_stream: picture.index,
            range,
            tempo_hundredths,
            reverse: matches!(
                self.get("reverse_video"),
                Some(Setting::Enabled { value: true })
            ),
            audio: AudioPolicy::Drop,
        })
    }

    pub fn copy_paths(&self) -> Result<(&Path, &Path)> {
        if self.version != 1 || self.settings.len() > FIELDS.len() {
            return Err(DomainError::new(
                "INVALID_JOB_DOCUMENT",
                "Expected a bounded version 1 FFmWiz job",
            ));
        }
        let source = self.path("input_path")?;
        let output = self.path("output_path")?;
        for name in ["video_codec", "audio_codec"] {
            match self.get(name) {
                Some(Setting::Copy) => (),
                None | Some(Setting::Unresolved) => return Err(missing(name)),
                _ => return Err(unsupported(name)),
            }
        }
        for name in ["audio_tracks", "subtitle_tracks"] {
            match self.get(name) {
                Some(Setting::Tracks {
                    selection: TrackChoice::Analyze { .. },
                }) => return Err(unsupported(name)),
                Some(Setting::Tracks { .. }) => (),
                None | Some(Setting::Unresolved) => return Err(missing(name)),
                _ => return Err(unsupported(name)),
            }
        }
        for (name, entry) in &self.settings {
            let name = name.as_str();
            if !FIELDS.contains(&name) || !(1..=super::MAX_LINES).contains(&entry.line) {
                return Err(field_error(
                    "INVALID_JOB_DOCUMENT",
                    name,
                    "Unknown field or invalid imported line number",
                ));
            }
            let value = &entry.value;
            let supported = match (name, value) {
                (
                    "input_path" | "output_path" | "video_codec" | "audio_codec" | "audio_tracks"
                    | "subtitle_tracks",
                    _,
                ) => true,
                ("output_format", Setting::Source) => same_extension(source, output),
                ("output_format", Setting::Choice { value }) => output
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(value)),
                (
                    "use_gpu"
                    | "crop"
                    | "reverse_video"
                    | "reverse_audio"
                    | "cpu_two_pass"
                    | "detect_duplicate_audio",
                    Setting::Enabled { value: false },
                ) => true,
                (
                    "keep_source_metadata" | "keep_embedded_attachments" | "logging_enabled",
                    Setting::Enabled { .. },
                ) => true,
                (
                    "crop_top" | "crop_left" | "crop_right" | "crop_bottom",
                    Setting::Integer { .. },
                ) => !matches!(self.get("crop"), Some(Setting::Enabled { value: true })),
                (
                    "resolution" | "fps" | "color_range" | "video_bitrate_kbps"
                    | "audio_bitrate_kbps" | "audio_sample_rate" | "video_speed" | "audio_speed",
                    Setting::Source,
                ) => true,
                ("video_speed" | "audio_speed", Setting::Scalar { value }) => {
                    value.numerator() == value.denominator()
                }
                ("audio_speed", Setting::Choice { value }) => value == "match_video",
                ("video_bitrate_mode", Setting::Choice { value }) => {
                    matches!(value.as_str(), "quality_vbr" | "strict_size")
                }
                (
                    "video_look"
                    | "video_quick"
                    | "audio_volume"
                    | "raw_ffmpeg_args"
                    | "raw_ffmpeg_valued_args",
                    Setting::Disabled,
                ) => true,
                ("audio_volume", Setting::Gain { unit, value }) => match unit {
                    super::GainUnit::Factor => value.numerator() == value.denominator(),
                    super::GainUnit::Percent => {
                        i128::from(value.numerator()) == 100 * i128::from(value.denominator())
                    }
                    super::GainUnit::Decibels => value.numerator() == 0,
                },
                ("loudnorm", Setting::Choice { value }) => value == "off",
                ("loudnorm_target_i", Setting::Scalar { .. }) => {
                    matches!(self.get("loudnorm"), Some(Setting::Choice { value }) if value == "off")
                }
                ("nvenc_multipass", Setting::Choice { value }) => value == "disabled",
                ("log_retention_days", Setting::Integer { .. })
                | ("log_level", Setting::Choice { .. })
                | ("gui_engine", Setting::LegacyPolicy { .. }) => true,
                (_, Setting::Unresolved) => return Err(missing(name)),
                _ => false,
            };
            if !supported {
                return Err(unsupported(name));
            }
        }
        Ok((source, output))
    }

    pub fn copy_intent(&self, streams: &[Stream]) -> Result<CleanIntent> {
        let mut intent = CleanIntent::preserving_all();
        (intent.audio_selection, intent.audio_order) =
            self.selection("audio_tracks", StreamKind::Audio, streams)?;
        (intent.subtitle_selection, intent.subtitle_order) =
            self.selection("subtitle_tracks", StreamKind::Subtitle, streams)?;
        if matches!(
            self.get("keep_source_metadata"),
            Some(Setting::Enabled { value: false })
        ) {
            intent.preservation.global_tags = false;
            intent.preservation.stream_tags = false;
            intent.preservation.chapters = false;
            intent.preservation.dispositions = false;
        }
        if matches!(
            self.get("keep_embedded_attachments"),
            Some(Setting::Enabled { value: false })
        ) {
            intent.preservation.attachments = false;
            intent.preservation.attached_pictures = false;
        }
        Ok(intent)
    }

    fn selection(
        &self,
        name: &str,
        kind: StreamKind,
        streams: &[Stream],
    ) -> Result<(Selection, Vec<u32>)> {
        let (selector, order) = match self.get(name) {
            Some(Setting::Tracks {
                selection: TrackChoice::All,
            }) => (Selector::All, Vec::new()),
            Some(Setting::Tracks {
                selection: TrackChoice::None,
            }) => (Selector::None, Vec::new()),
            Some(Setting::Tracks {
                selection: TrackChoice::Ordinals { values },
            }) => {
                let mut indices = streams
                    .iter()
                    .filter(|stream| stream.kind == kind)
                    .map(|stream| stream.index)
                    .collect::<Vec<_>>();
                indices.sort_unstable();
                if values.is_empty()
                    || values.len() > indices.len()
                    || values.iter().copied().collect::<BTreeSet<_>>().len() != values.len()
                {
                    return Err(field_error(
                        "INVALID_TRACK_SELECTION",
                        name,
                        "Provide distinct existing track ordinals",
                    ));
                }
                let mut resolved = Vec::with_capacity(values.len());
                for ordinal in values {
                    let index = indices.get(*ordinal as usize).ok_or_else(|| {
                        field_error("STREAM_NOT_FOUND", name, "An imported zero-based track index is absent from this registered source")
                    })?;
                    resolved.push(*index);
                }
                (
                    Selector::SourceIndices {
                        values: resolved.clone(),
                    },
                    resolved,
                )
            }
            _ => return Err(unsupported(name)),
        };
        Ok((
            Selection {
                selector,
                no_match: if order.is_empty() {
                    NoMatch::AllowEmpty
                } else {
                    NoMatch::Reject
                },
            },
            order,
        ))
    }
}

fn same_extension(left: &Path, right: &Path) -> bool {
    left.extension()
        .and_then(|extension| extension.to_str())
        .zip(right.extension().and_then(|extension| extension.to_str()))
        .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
}
