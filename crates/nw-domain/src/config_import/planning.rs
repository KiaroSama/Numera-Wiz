//! Compile the admitted FFmWiz packet-copy settings into the ordinary stream plan.
use super::{FIELDS, ImportedJob, ScaleMode, Setting, TrackChoice};
use crate::{
    DomainError, Result,
    avc_export::{Crop, Detail, PictureTransform, Resize, ResizeAspect, Rotation, Strength},
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
        "This setting requires a different media profile",
    )
}

fn even_crop_axis(extent: u32, near: u32, far: u32) -> Option<(u32, u32)> {
    let total = near.checked_add(far).filter(|total| *total < extent)?;
    let lower = |value| value & !1;
    let near_options = [lower(near), lower(near) + 2];
    let far_options = [lower(far), lower(far) + 2];
    // The source and output use the same two-pixel grid. Searching the adjacent
    // even margins suffices. FFmWiz breaks ties by size, center, and retained area.
    near_options
        .into_iter()
        .flat_map(|left| far_options.into_iter().map(move |right| (left, right)))
        .filter(|(left, right)| left + right <= extent - 2)
        .min_by_key(|&(left, right)| {
            let trimmed = left + right;
            (
                left.abs_diff(near) + right.abs_diff(far),
                trimmed.abs_diff(total),
                (i64::from(left) - i64::from(right)).abs_diff(i64::from(near) - i64::from(far)),
                trimmed,
                left,
            )
        })
        .map(|(left, right)| (left, extent - left - right))
}

fn nearest_even(num: u64, den: u64) -> u64 {
    // The donor rounds half to even on the half-dimension, then doubles it.
    let quotient = num / (den * 2);
    let remainder = num % (den * 2);
    let up = remainder > den || (remainder == den && !quotient.is_multiple_of(2));
    ((quotient + up as u64) * 2).max(2)
}

fn cropped_display_size(
    details: &serde_json::Value,
    picture: &PictureTransform,
) -> Result<(u64, u64)> {
    let dimension = |field: &str| {
        details[field]
            .as_u64()
            .filter(|value| (2..=4096).contains(value))
            .ok_or_else(|| unsupported("resolution"))
    };
    let (width, height) = if let Some(crop) = picture.crop {
        (u64::from(crop.width), u64::from(crop.height))
    } else {
        (dimension("width")?, dimension("height")?)
    };
    let aspect = match details["sample_aspect_ratio"].as_str() {
        None | Some("0:1" | "N/A") => (1, 1),
        Some(aspect) => {
            let (num, den) = aspect
                .split_once(':')
                .ok_or_else(|| unsupported("resolution"))?;
            let integer = |value: &str| {
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| *value != 0)
                    .map(u64::from)
                    .ok_or_else(|| unsupported("resolution"))
            };
            (integer(num)?, integer(den)?)
        }
    };
    let width = nearest_even(width * aspect.0, aspect.1);
    let height = nearest_even(height, 1);
    Ok(
        if matches!(
            picture.rotation,
            Rotation::Clockwise | Rotation::Counterclockwise
        ) {
            (height, width)
        } else {
            (width, height)
        },
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
        self.cpu_encode_codec().is_some()
    }

    fn cpu_encode_codec(&self) -> Option<crate::avc_export::VideoCodec> {
        use crate::avc_export::VideoCodec;
        let Some(Setting::Encoder { name }) = self.get("video_codec") else {
            return None;
        };
        match name.as_str() {
            "h264" | "libx264" => Some(VideoCodec::H264),
            "hevc" | "libx265" => Some(VideoCodec::Hevc),
            "av1" | "libsvtav1" => Some(VideoCodec::Av1),
            "vp9" | "libvpx-vp9" => Some(VideoCodec::Vp9),
            "mpeg4" | "mpeg-4" | "mp4v" => Some(VideoCodec::Mpeg4),
            _ => None,
        }
    }

    fn avc_crop(&self, details: &serde_json::Value) -> Result<Option<Crop>> {
        let margin = |key| match self.get(key) {
            Some(Setting::Integer { value }) => *value,
            _ => 0,
        };
        let (top, left, right, bottom) = match self.get("crop") {
            None | Some(Setting::Enabled { value: false }) => return Ok(None),
            Some(Setting::Enabled { value: true }) => (
                margin("crop_top"),
                margin("crop_left"),
                margin("crop_right"),
                margin("crop_bottom"),
            ),
            Some(Setting::CropMargins {
                top,
                left,
                right,
                bottom,
            }) => (*top, *left, *right, *bottom),
            _ => return Err(unsupported("crop")),
        };
        let axis = |field: &str, max: u32, near, far| {
            let extent = details[field]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (2..=max).contains(value) && value.is_multiple_of(2));
            extent
                .and_then(|extent| even_crop_axis(extent, near, far))
                .ok_or_else(|| {
                    field_error(
                        "JOB_PROFILE_UNSUPPORTED",
                        "crop",
                        "Crop margins must leave a nonempty picture on the admitted 4:2:0 grid",
                    )
                })
        };
        let (x, width) = axis("width", 4096, left, right)?;
        let (y, height) = axis("height", 2160, top, bottom)?;
        Ok(Some(Crop {
            width,
            height,
            x,
            y,
        }))
    }

    fn apply_avc_look(&self, picture: &mut PictureTransform) -> Result<()> {
        let value = match self.get("video_look") {
            None | Some(Setting::Disabled) => return Ok(()),
            Some(Setting::Effects { value }) if (1..=32).contains(&value.len()) => value,
            _ => return Err(unsupported("video_look")),
        };
        for effect in value {
            let effect = (
                effect.name.as_str(),
                effect.amount,
                effect.strength.as_deref(),
            );
            match effect {
                ("90cw", None, None) => picture.rotation = Rotation::Clockwise,
                ("90ccw", None, None) => picture.rotation = Rotation::Counterclockwise,
                ("180", None, None) => picture.rotation = Rotation::HalfTurn,
                ("hflip", None, None) => picture.flip_horizontal = true,
                ("vflip", None, None) => picture.flip_vertical = true,
                ("gray", None, None) => picture.look.get_or_insert_default().grayscale = true,
                ("bright", Some(value), None) => {
                    picture.look.get_or_insert_default().brightness = Some(value);
                }
                ("contrast", Some(value), None) => {
                    picture.look.get_or_insert_default().contrast = Some(value);
                }
                ("sat", Some(value), None) => {
                    picture.look.get_or_insert_default().saturation = Some(value);
                }
                ("gamma", Some(value), None) => {
                    picture.look.get_or_insert_default().gamma = Some(value);
                }
                (name @ ("denoise" | "sharpen" | "blur"), None, Some(strength)) => {
                    let strength = match strength {
                        "light" => Strength::Light,
                        "medium" => Strength::Medium,
                        "heavy" => Strength::Heavy,
                        _ => return Err(unsupported("video_look")),
                    };
                    let look = picture.look.get_or_insert_default();
                    match name {
                        "denoise" => look.denoise = Some(strength),
                        "sharpen" if !matches!(look.detail, Some(Detail::Blur { .. })) => {
                            look.detail = Some(Detail::Sharpen { strength });
                        }
                        "blur" if !matches!(look.detail, Some(Detail::Sharpen { .. })) => {
                            look.detail = Some(Detail::Blur { strength });
                        }
                        _ => return Err(unsupported("video_look")),
                    }
                }
                _ => return Err(unsupported("video_look")),
            }
        }
        Ok(())
    }

    fn avc_resize(
        &self,
        details: &serde_json::Value,
        picture: &PictureTransform,
    ) -> Result<Option<Resize>> {
        let (mode, width, height) = match self.get("resolution") {
            None | Some(Setting::Source) => return Ok(None),
            Some(Setting::Geometry {
                mode,
                width,
                height,
            }) => (*mode, *width, *height),
            _ => return Err(unsupported("resolution")),
        };
        if !matches!(
            (mode, width, height),
            (ScaleMode::Fit | ScaleMode::Stretch, Some(_), Some(_))
                | (ScaleMode::Width, Some(_), None)
                | (ScaleMode::Height | ScaleMode::ClosestEdge, None, Some(_))
        ) {
            return Err(unsupported("resolution"));
        }
        let required = |value: Option<u32>| {
            value
                .filter(|value| (1..=16384).contains(value))
                .map(u64::from)
                .ok_or_else(|| unsupported("resolution"))
        };
        let (width, height) = match mode {
            ScaleMode::Fit | ScaleMode::Stretch => (
                nearest_even(required(width)?, 1),
                nearest_even(required(height)?, 1),
            ),
            _ => {
                let (display_width, display_height) = cropped_display_size(details, picture)?;
                let with_width = |width| {
                    let width = nearest_even(width, 1);
                    (width, nearest_even(width * display_height, display_width))
                };
                let with_height = |height| {
                    let height = nearest_even(height, 1);
                    (nearest_even(height * display_width, display_height), height)
                };
                match mode {
                    ScaleMode::Width => with_width(required(width)?),
                    ScaleMode::Height => with_height(required(height)?),
                    ScaleMode::ClosestEdge => {
                        let (mut target_width, mut target_height) = match required(height)? {
                            144 => (256, 144),
                            240 => (426, 240),
                            360 => (640, 360),
                            480 => (720, 480),
                            576 => (720, 576),
                            720 => (1280, 720),
                            1080 => (1920, 1080),
                            1440 => (2560, 1440),
                            2160 => (3840, 2160),
                            4320 => (7680, 4320),
                            _ => return Err(unsupported("resolution")),
                        };
                        if display_width < display_height {
                            std::mem::swap(&mut target_width, &mut target_height);
                        }
                        if display_width.abs_diff(target_width)
                            <= display_height.abs_diff(target_height)
                        {
                            with_width(target_width)
                        } else {
                            with_height(target_height)
                        }
                    }
                    _ => unreachable!("Only aspect-preserving scale modes need a source raster"),
                }
            }
        };
        Ok(Some(Resize {
            width: u32::try_from(width).map_err(|_| unsupported("resolution"))?,
            height: u32::try_from(height).map_err(|_| unsupported("resolution"))?,
            aspect: if matches!(mode, ScaleMode::Stretch) {
                ResizeAspect::SquarePixels
            } else {
                ResizeAspect::Fit
            },
        }))
    }

    pub fn avc_encode(
        &self,
        asset: &crate::records::Asset,
    ) -> Result<crate::avc_export::EncodeInput> {
        use crate::avc_export::{Audio, Bitrate, Container, Encode, EncodeInput, Preset, RateMode};
        let Some(codec) = self
            .cpu_encode_codec()
            .filter(|_| self.version == 1 && self.settings.len() <= FIELDS.len())
        else {
            return Err(DomainError::new(
                "INVALID_JOB_DOCUMENT",
                "Use a version 1 FFmWiz profile with CPU H.264, HEVC, AV1, VP9 or MPEG-4",
            ));
        };
        let source = self.imported_source()?;
        let output = self.path("output_path")?;
        let extension = output
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| missing("output_path"))?;
        let container = match extension.to_ascii_lowercase().as_str() {
            "mp4" => Container::Mp4,
            "mkv" => Container::Matroska,
            _ => return Err(unsupported("output_path")),
        };
        for (name, field) in &self.settings {
            let name = name.as_str();
            if !FIELDS.contains(&name) || !(1..=super::MAX_LINES).contains(&field.line) {
                return Err(field_error(
                    "INVALID_JOB_DOCUMENT",
                    name,
                    "Unknown field or invalid imported line number",
                ));
            }
            let supported = match (name, &field.value) {
                (
                    "input_path" | "output_path" | "video_codec" | "video_bitrate_kbps"
                    | "video_bitrate_mode" | "audio_tracks" | "audio_codec" | "audio_bitrate_kbps",
                    _,
                ) => true,
                ("output_format", Setting::Source) => same_extension(source, output),
                ("output_format", Setting::Choice { value }) => {
                    value.eq_ignore_ascii_case(extension)
                }
                (
                    "use_gpu"
                    | "cpu_two_pass"
                    | "reverse_video"
                    | "reverse_audio"
                    | "keep_source_metadata"
                    | "keep_embedded_attachments"
                    | "detect_duplicate_audio",
                    Setting::Enabled { value: false },
                ) => true,
                ("crop", Setting::Enabled { .. } | Setting::CropMargins { .. })
                | (
                    "crop_top" | "crop_left" | "crop_bottom" | "crop_right",
                    Setting::Integer { .. },
                ) => true,
                (
                    "subtitle_tracks",
                    Setting::Tracks {
                        selection: TrackChoice::None,
                    },
                ) => true,
                (
                    "resolution" | "fps" | "color_range" | "audio_sample_rate" | "video_speed"
                    | "audio_speed",
                    Setting::Source,
                ) => true,
                ("resolution", Setting::Geometry { .. }) => true,
                ("video_speed" | "audio_speed", Setting::Scalar { value }) => {
                    value.numerator() == value.denominator()
                }
                ("audio_speed", Setting::Choice { value }) => value == "match_video",
                ("video_look", Setting::Effects { .. }) => true,
                (
                    "video_look"
                    | "video_quick"
                    | "audio_volume"
                    | "raw_ffmpeg_args"
                    | "raw_ffmpeg_valued_args",
                    Setting::Disabled,
                ) => true,
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
        if !matches!(
            self.get("subtitle_tracks"),
            Some(Setting::Tracks {
                selection: TrackChoice::None
            })
        ) {
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
        let mut pictures = asset
            .probe
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Video && !stream.attached_picture());
        let picture = pictures.next().ok_or_else(|| unsupported("video_codec"))?;
        if pictures.next().is_some() {
            return Err(unsupported("video_codec"));
        }
        let details = asset
            .probe
            .stream_details
            .get(&picture.index)
            .ok_or_else(|| missing("input_path"))?;
        let crop = self.avc_crop(details)?;
        let audio = match self.get("audio_tracks") {
            Some(Setting::Tracks {
                selection: TrackChoice::None,
            }) => Audio::Drop {},
            Some(Setting::Tracks { selection }) => {
                let mut tracks: Vec<_> = asset
                    .probe
                    .streams
                    .iter()
                    .filter(|track| track.kind == StreamKind::Audio)
                    .collect();
                tracks.sort_unstable_by_key(|track| track.index);
                let track = match selection {
                    TrackChoice::All if tracks.len() == 1 => Some(tracks[0]),
                    TrackChoice::Ordinals { values } if values.len() == 1 => {
                        tracks.get(values[0] as usize).copied()
                    }
                    _ => None,
                }
                .ok_or_else(|| unsupported("audio_tracks"))?;
                if !matches!(self.get("audio_codec"), Some(Setting::Encoder { name }) if name == "aac")
                {
                    return Err(unsupported("audio_codec"));
                }
                let bitrate_kbps = match self.get("audio_bitrate_kbps") {
                    Some(Setting::Integer { value }) => {
                        u16::try_from(*value).map_err(|_| unsupported("audio_bitrate_kbps"))?
                    }
                    _ => return Err(missing("audio_bitrate_kbps")),
                };
                Audio::Aac {
                    stream_index: track.index,
                    bitrate_kbps,
                }
            }
            _ => return Err(missing("audio_tracks")),
        };
        let mut transform = PictureTransform {
            crop,
            ..Default::default()
        };
        self.apply_avc_look(&mut transform)?;
        transform.resize = self.avc_resize(details, &transform)?;
        let changed = crop.is_some()
            || transform.resize.is_some()
            || transform.rotation != Rotation::None
            || transform.flip_horizontal
            || transform.flip_vertical
            || transform.look.is_some();
        let settings = Encode {
            codec,
            video_stream: picture.index,
            container,
            crf: None,
            quantizer: None,
            bitrate: Some(Bitrate { average_kbps, mode }),
            preset: Preset::Balanced,
            audio,
            picture: changed.then_some(transform),
        };
        settings.validate()?;
        Ok(EncodeInput {
            asset_id: asset.id.clone(),
            output: output
                .to_str()
                .ok_or_else(|| missing("output_path"))?
                .to_owned(),
            settings,
        })
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
