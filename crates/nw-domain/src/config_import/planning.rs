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
