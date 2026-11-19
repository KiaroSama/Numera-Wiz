//! Decode the FFmWiz configuration format into versioned media intent data.
use crate::{DomainError, Result, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

mod planning;
pub use planning::PlanInput;

pub const CONFIG_BYTES: usize = 64 * 1024;
const MAX_LINES: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityProfile {
    FfmwizV1,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub source: String,
    pub base_directory: String,
    pub compatibility_profile: CompatibilityProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Diagnostic {
    pub line: usize,
    pub field: Option<String>,
    pub severity: Severity,
    pub code: &'static str,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedJob {
    pub version: u32,
    pub compatibility_profile: CompatibilityProfile,
    pub settings: BTreeMap<String, ImportedField>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportReport {
    pub job: ImportedJob,
    pub diagnostics: Vec<Diagnostic>,
    pub input_valid: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedField {
    pub line: usize,
    pub value: Setting,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Setting {
    Source,
    Copy,
    Disabled,
    Enabled {
        value: bool,
    },
    PreferNvidia,
    DirectoryTemplate {
        path: String,
    },
    File {
        path: String,
    },
    Unresolved,
    Encoder {
        name: String,
    },
    Choice {
        value: String,
    },
    Integer {
        value: u32,
    },
    CropMargins {
        top: u32,
        left: u32,
        right: u32,
        bottom: u32,
    },
    Scalar {
        value: Rational,
    },
    Gain {
        unit: GainUnit,
        value: Rational,
    },
    Geometry {
        mode: ScaleMode,
        width: Option<u32>,
        height: Option<u32>,
    },
    Effects {
        value: Vec<Effect>,
    },
    Quick {
        output: Option<QuickOutput>,
        repeat_count: u32,
    },
    Tracks {
        selection: TrackChoice,
    },
    LegacyPolicy {
        value: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GainUnit {
    Factor,
    Percent,
    Decibels,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    ClosestEdge,
    Width,
    Height,
    Fit,
    Stretch,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrackChoice {
    All,
    None,
    Ordinals { values: Vec<u32> },
    Analyze { duplicates: bool, silence: bool },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub name: String,
    pub amount: Option<Rational>,
    pub strength: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuickOutput {
    Gif { frame_rate: u32, width: u32 },
    Boomerang,
    Thumbnail { seconds: Rational },
}

const FIELDS: [&str; 40] = [
    "input_path",
    "output_path",
    "output_format",
    "video_codec",
    "use_gpu",
    "crop",
    "crop_top",
    "crop_left",
    "crop_right",
    "crop_bottom",
    "video_bitrate_kbps",
    "video_bitrate_mode",
    "resolution",
    "fps",
    "video_look",
    "video_quick",
    "audio_volume",
    "raw_ffmpeg_args",
    "raw_ffmpeg_valued_args",
    "audio_tracks",
    "audio_codec",
    "audio_bitrate_kbps",
    "audio_sample_rate",
    "keep_source_metadata",
    "subtitle_tracks",
    "keep_embedded_attachments",
    "detect_duplicate_audio",
    "loudnorm",
    "loudnorm_target_i",
    "nvenc_multipass",
    "cpu_two_pass",
    "color_range",
    "video_speed",
    "reverse_video",
    "audio_speed",
    "reverse_audio",
    "logging_enabled",
    "log_retention_days",
    "log_level",
    "gui_engine",
];

fn diagnostic(
    line: usize,
    field: Option<&str>,
    severity: Severity,
    code: &'static str,
) -> Diagnostic {
    Diagnostic {
        line,
        field: field.map(str::to_owned),
        severity,
        code,
    }
}

fn mapped(value: &str, choices: &[&str]) -> Option<Setting> {
    choices.contains(&value).then(|| Setting::Choice {
        value: value.into(),
    })
}

fn integer(value: &str, low: u32, high: u32) -> Option<u32> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| (low..=high).contains(value))
}

fn decimal(value: &str) -> Option<Rational> {
    if let Some((num, den)) = value.split_once('/') {
        return Rational::new(num.parse().ok()?, den.parse().ok()?).ok();
    }
    let (unsigned, sign) = value
        .strip_prefix('-')
        .map_or((value.strip_prefix('+').unwrap_or(value), 1), |rest| {
            (rest, -1)
        });
    let (whole, fractional) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty()
        || fractional.len() > 6
        || !whole
            .bytes()
            .chain(fractional.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let denominator = 10_i64.pow(fractional.len() as u32);
    let unsigned = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(denominator)?
        .checked_add(fractional.parse::<i64>().unwrap_or(0))?;
    Rational::new(unsigned.checked_mul(sign)?, denominator).ok()
}

fn scalar(value: &str, low: f64, high: f64) -> Option<Rational> {
    let rational = decimal(value)?;
    let count = rational.numerator() as f64 / rational.denominator() as f64;
    (low..=high).contains(&count).then_some(rational)
}

fn geometry(value: &str) -> Option<Setting> {
    let (body, mode) = value
        .strip_prefix("stretch:")
        .or_else(|| value.strip_prefix("exact:"))
        .map_or((value, ScaleMode::Fit), |body| (body, ScaleMode::Stretch));
    if let Some((width, height)) = body.split_once('x') {
        return Some(Setting::Geometry {
            mode,
            width: Some(integer(width.trim(), 1, 16384)?),
            height: Some(integer(height.trim(), 1, 16384)?),
        });
    }
    if matches!(mode, ScaleMode::Stretch) {
        return None;
    }
    if let Some(width) = body
        .strip_prefix("width=")
        .or_else(|| body.strip_prefix('w'))
        .or_else(|| body.strip_suffix('w'))
    {
        return Some(Setting::Geometry {
            mode: ScaleMode::Width,
            width: Some(integer(width, 1, 16384)?),
            height: None,
        });
    }
    if let Some(height) = body
        .strip_prefix("height=")
        .or_else(|| body.strip_prefix('h'))
        .or_else(|| body.strip_suffix('h'))
    {
        return Some(Setting::Geometry {
            mode: ScaleMode::Height,
            width: None,
            height: Some(integer(height, 1, 16384)?),
        });
    }
    Some(Setting::Geometry {
        mode: ScaleMode::ClosestEdge,
        width: None,
        height: Some(integer(body.strip_suffix('p').unwrap_or(body), 1, 16384)?),
    })
}

fn gain(value: &str) -> Option<Setting> {
    let (value, unit, min, max) = if let Some(value) = value.strip_suffix('%') {
        (value, GainUnit::Percent, 1.0, 1000.0)
    } else if let Some(value) = value.strip_suffix("db") {
        (value, GainUnit::Decibels, -40.0, 20.0)
    } else {
        (value, GainUnit::Factor, 0.01, 10.0)
    };
    Some(Setting::Gain {
        unit,
        value: scalar(value, min, max)?,
    })
}

fn tracks(value: &str) -> Option<Setting> {
    let selection = match value {
        "all" => TrackChoice::All,
        "none" | "clear" | "delete" => TrackChoice::None,
        "d" | "e" | "de" | "ed" => TrackChoice::Analyze {
            duplicates: value.contains('d'),
            silence: value.contains('e'),
        },
        value => {
            let values = value
                .split(',')
                .map(|part| integer(part.trim(), 0, 4095))
                .collect::<Option<Vec<_>>>()?;
            if values.len() > 128
                || values
                    .iter()
                    .enumerate()
                    .any(|(i, item)| values[..i].contains(item))
            {
                return None;
            }
            TrackChoice::Ordinals { values }
        }
    };
    Some(Setting::Tracks { selection })
}

fn effects(value: &str) -> Option<Setting> {
    let mut entries = Vec::new();
    for effect in value.split(',').map(str::trim) {
        if entries.len() == 32 {
            return None;
        }
        let (name, parameter) = effect.split_once('=').unwrap_or((effect, ""));
        let (amount, strength) = match name {
            "90cw" | "90ccw" | "180" | "hflip" | "vflip" | "gray" if parameter.is_empty() => {
                (None, None)
            }
            "bright" => (Some(scalar(parameter, -1.0, 1.0)?), None),
            "contrast" => (Some(scalar(parameter, 0.0, 3.0)?), None),
            "sat" => (Some(scalar(parameter, 0.0, 3.0)?), None),
            "gamma" => (Some(scalar(parameter, 0.1, 10.0)?), None),
            "fadein" | "fadeout" => (Some(scalar(parameter, 0.001, 600.0)?), None),
            "denoise" | "sharpen" | "blur"
                if matches!(parameter, "" | "light" | "medium" | "heavy") =>
            {
                (
                    None,
                    Some(
                        if parameter.is_empty() {
                            "medium"
                        } else {
                            parameter
                        }
                        .to_owned(),
                    ),
                )
            }
            _ => return None,
        };
        entries.push(Effect {
            name: name.to_owned(),
            amount,
            strength,
        });
    }
    if entries.iter().any(|entry| entry.name == "sharpen")
        && entries.iter().any(|entry| entry.name == "blur")
    {
        return None;
    }
    Some(Setting::Effects { value: entries })
}

fn clock(value: &str) -> Option<Rational> {
    if let [hours, minutes, seconds] = value.split(':').collect::<Vec<_>>().as_slice() {
        let hours = integer(hours, 0, 1000)?;
        let minutes = integer(minutes, 0, 59)?;
        let seconds = scalar(seconds, 0.0, 59.999_999)?;
        return Rational::new(i64::from(hours * 3600 + minutes * 60), 1)
            .ok()?
            .checked_add(seconds)
            .ok();
    }
    scalar(value, 0.0, 3_600_000.0)
}

fn quick(value: &str) -> Option<Setting> {
    let mut output = None;
    let mut repeat_count = None;
    for token in value.split(',').map(str::trim) {
        let (name, parameter) = token.split_once('=').unwrap_or((token, ""));
        let next = match name {
            "loop" if repeat_count.is_none() => {
                repeat_count = Some(integer(parameter, 0, 1000)?);
                continue;
            }
            "gif" => {
                let (fps, width) = parameter.split_once(':').unwrap_or((parameter, "480"));
                QuickOutput::Gif {
                    frame_rate: integer(if fps.is_empty() { "15" } else { fps }, 1, 60)?,
                    width: integer(width, 1, 8192)?,
                }
            }
            "boomerang" if parameter.is_empty() => QuickOutput::Boomerang,
            "thumb" => QuickOutput::Thumbnail {
                seconds: clock(if parameter.is_empty() { "0" } else { parameter })?,
            },
            _ => return None,
        };
        if output.replace(next).is_some() {
            return None;
        }
    }
    Some(Setting::Quick {
        output,
        repeat_count: repeat_count.unwrap_or(0),
    })
}

fn encoder(value: &str) -> Option<Setting> {
    if matches!(value, "n" | "copy") {
        return Some(Setting::Copy);
    }
    let normalized = match value {
        "h.264" | "h264" | "avc" => "h264",
        "h.265" | "h265" | "hevc" => "hevc",
        "opus" => "libopus",
        "mp3" => "libmp3lame",
        "vorbis" => "libvorbis",
        other => other,
    };
    (!normalized.is_empty()
        && normalized.len() <= 64
        && normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)))
    .then(|| Setting::Encoder {
        name: normalized.to_owned(),
    })
}

fn setting(field: &str, original: &str, base: &Path) -> Option<Setting> {
    if original.is_empty() && field != "audio_tracks" {
        return Some(Setting::Unresolved);
    }
    let value = original.to_ascii_lowercase();
    let value = value.as_str();
    let preserve = matches!(value, "n" | "source" | "keep");
    match field {
        "input_path" | "output_path" => {
            let native = Path::new(original);
            if !cfg!(windows)
                && (original.as_bytes().get(1) == Some(&b':') || original.starts_with("\\\\"))
            {
                return Some(Setting::Unresolved);
            }
            let path = if native.is_absolute() {
                native.to_owned()
            } else {
                base.join(native)
            };
            let path = path.to_str()?.to_owned();
            if field == "output_path" && original.ends_with(['/', '\\']) {
                Some(Setting::DirectoryTemplate { path })
            } else {
                Some(Setting::File { path })
            }
        }
        "video_codec" | "audio_codec" => encoder(value),
        "use_gpu" if value == "y" => Some(Setting::PreferNvidia),
        "crop" if value.contains(',') => {
            let mut fields = value.split(',');
            let mut margin = || integer(fields.next()?.trim(), 0, 16384);
            let [top, left, right, bottom] = [margin()?, margin()?, margin()?, margin()?];
            fields.next().is_none().then_some(Setting::CropMargins {
                top,
                left,
                right,
                bottom,
            })
        }
        "use_gpu"
        | "crop"
        | "keep_source_metadata"
        | "keep_embedded_attachments"
        | "detect_duplicate_audio"
        | "cpu_two_pass"
        | "reverse_video"
        | "reverse_audio"
        | "logging_enabled" => match value {
            "y" | "yes" | "true" => Some(Setting::Enabled { value: true }),
            "n" | "no" | "false" => Some(Setting::Enabled { value: false }),
            _ => None,
        },
        "crop_top" | "crop_left" | "crop_right" | "crop_bottom" => Some(Setting::Integer {
            value: integer(value, 0, 16384)?,
        }),
        "output_format" | "video_bitrate_kbps" | "audio_bitrate_kbps" | "audio_sample_rate"
        | "resolution" | "fps" | "video_speed" | "audio_speed" | "color_range"
            if preserve =>
        {
            Some(Setting::Source)
        }
        "output_format" => mapped(
            value.trim_start_matches('.'),
            &[
                "mkv", "mp4", "mov", "webm", "avi", "flv", "mpeg", "ts", "m4v", "wmv", "mpg",
            ],
        ),
        "video_bitrate_kbps" | "audio_bitrate_kbps" => Some(Setting::Integer {
            value: integer(value, 1, 1_000_000)?,
        }),
        "audio_sample_rate" => Some(Setting::Integer {
            value: integer(value, 8000, 192000)?,
        }),
        "video_bitrate_mode" => mapped(value, &["quality_vbr", "strict_size"]),
        "resolution" => geometry(value),
        "fps" => Some(Setting::Scalar {
            value: scalar(value, 0.1, 1000.0)?,
        }),
        "video_speed" | "audio_speed" => {
            if field == "audio_speed" && matches!(value, "match" | "match_video" | "video") {
                mapped("match_video", &["match_video"])
            } else {
                Some(Setting::Scalar {
                    value: scalar(value, 0.10, 8.0)?,
                })
            }
        }
        "video_look"
        | "video_quick"
        | "audio_volume"
        | "raw_ffmpeg_args"
        | "raw_ffmpeg_valued_args"
            if preserve =>
        {
            Some(Setting::Disabled)
        }
        "video_look" => effects(value),
        "video_quick" => quick(value),
        "audio_volume" => gain(value),
        "audio_tracks" | "subtitle_tracks" => {
            tracks(if field == "audio_tracks" && value.is_empty() {
                "de"
            } else {
                value
            })
        }
        "loudnorm" => mapped(value, &["off", "on", "dynamic", "two_pass"]),
        "loudnorm_target_i" => Some(Setting::Scalar {
            value: scalar(value, -30.0, -5.0)?,
        }),
        "nvenc_multipass" => mapped(value, &["disabled", "qres", "fullres"]),
        "color_range" => mapped(value, &["tv", "pc", "unspecified"]),
        "log_retention_days" => Some(Setting::Integer {
            value: integer(value, 0, 36525)?,
        }),
        "log_level" => mapped(
            value,
            &[
                "trace", "debug", "info", "warning", "warn", "error", "critical",
            ],
        ),
        "gui_engine" => Some(Setting::LegacyPolicy {
            value: if matches!(value, "classic" | "qml") {
                value
            } else {
                "unknown"
            }
            .into(),
        }),
        _ => None,
    }
}

pub fn import(request: ImportRequest) -> Result<ImportReport> {
    if request.source.len() > CONFIG_BYTES || request.source.lines().count() > MAX_LINES {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "FFmWiz configuration exceeds the import limit",
        ));
    }
    let base = Path::new(&request.base_directory);
    if !base.is_absolute()
        || request.base_directory.len() > 4096
        || request.base_directory.contains('\0')
    {
        return Err(DomainError::new(
            "VALIDATION_ERROR",
            "Configuration base directory must be an explicit absolute path",
        ));
    }
    let mut settings = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for (index, line) in request
        .source
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
    {
        let line_number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, original)) = line.split_once('=') else {
            diagnostics.push(diagnostic(
                line_number,
                None,
                Severity::Error,
                "invalid_assignment",
            ));
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || key.len() > 80
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            diagnostics.push(diagnostic(
                line_number,
                None,
                Severity::Error,
                "invalid_field_name",
            ));
            continue;
        }
        if !FIELDS.contains(&key) {
            diagnostics.push(diagnostic(
                line_number,
                Some(key),
                Severity::Error,
                "unknown_field",
            ));
            continue;
        }
        let original = original.trim();
        let original = original
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .or_else(|| {
                original
                    .strip_prefix('\'')
                    .and_then(|rest| rest.strip_suffix('\''))
            })
            .unwrap_or(original);
        let value = if original.len() > 4096 || original.contains('\0') {
            None
        } else {
            setting(key, original, base)
        };
        let Some(value) = value else {
            diagnostics.push(diagnostic(
                line_number,
                Some(key),
                Severity::Error,
                if matches!(key, "raw_ffmpeg_args" | "raw_ffmpeg_valued_args") {
                    "expert_options_require_typed_review"
                } else {
                    "invalid_field_value"
                },
            ));
            continue;
        };
        if matches!(value, Setting::Unresolved) {
            diagnostics.push(diagnostic(
                line_number,
                Some(key),
                Severity::Error,
                "unresolved_field",
            ));
        } else if key == "gui_engine"
            || matches!(key, "logging_enabled" | "log_retention_days" | "log_level")
        {
            diagnostics.push(diagnostic(
                line_number,
                Some(key),
                Severity::Warning,
                "legacy_application_policy",
            ));
        }
        if settings
            .insert(
                key.to_owned(),
                ImportedField {
                    line: line_number,
                    value,
                },
            )
            .is_some()
        {
            diagnostics.push(diagnostic(
                line_number,
                Some(key),
                Severity::Warning,
                "duplicate_field_last_value",
            ));
        }
    }
    for required in ["input_path", "output_path"] {
        if !settings.contains_key(required) {
            diagnostics.push(diagnostic(
                0,
                Some(required),
                Severity::Error,
                "required_path_missing",
            ));
        }
    }
    let input_valid = diagnostics
        .iter()
        .all(|item| item.severity != Severity::Error);
    Ok(ImportReport {
        job: ImportedJob {
            version: 1,
            compatibility_profile: request.compatibility_profile,
            settings,
        },
        diagnostics,
        input_valid,
    })
}
