//! MuxCls selection and metadata semantics translated into the common domain.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Attachment,
    Data,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Stream {
    pub index: u32,
    pub kind: StreamKind,
    pub codec: String,
    pub tags: BTreeMap<String, String>,
    pub dispositions: BTreeSet<String>,
}
impl Stream {
    pub fn attached_picture(&self) -> bool {
        self.kind == StreamKind::Video && self.dispositions.contains("attached_pic")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selector {
    All,
    None,
    SourceIndices { values: Vec<u32> },
    Languages { values: Vec<String> },
    TitleContainsAny { values: Vec<String> },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoMatch {
    SkipFile,
    AllowEmpty,
    Reject,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub selector: Selector,
    pub no_match: NoMatch,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
#[derive(Default)]
pub enum TagPatch {
    #[default]
    Keep,
    Set {
        value: String,
    },
    Clear,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamTagPatch {
    pub kind: StreamKind,
    #[serde(default)]
    pub source_indices: Vec<u32>,
    #[serde(default)]
    pub original_languages: Vec<String>,
    #[serde(default)]
    pub language: TagPatch,
    #[serde(default)]
    pub title: TagPatch,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Preservation {
    pub additional_video: bool,
    pub attached_pictures: bool,
    pub attachments: bool,
    pub data: bool,
    pub unknown: bool,
    pub global_tags: bool,
    pub stream_tags: bool,
    pub chapters: bool,
    pub dispositions: bool,
}
impl Default for Preservation {
    fn default() -> Self {
        Self {
            additional_video: true,
            attached_pictures: true,
            attachments: true,
            data: true,
            unknown: true,
            global_tags: true,
            stream_tags: true,
            chapters: true,
            dispositions: true,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CleanIntent {
    pub audio_selection: Selection,
    pub subtitle_selection: Selection,
    #[serde(default)]
    pub audio_order: Vec<u32>,
    #[serde(default)]
    pub subtitle_order: Vec<u32>,
    #[serde(default)]
    pub stream_tag_patches: Vec<StreamTagPatch>,
    pub preservation: Preservation,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CleanAction {
    CopyUnchanged,
    Remux,
    Skip,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamOutcome {
    pub source_index: u32,
    pub output_index: Option<u32>,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CleanResolution {
    pub action: CleanAction,
    pub streams: Vec<Stream>,
    pub outcomes: Vec<StreamOutcome>,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub preservation: Preservation,
}

pub fn normalize_language(value: &str) -> String {
    let text = value.trim().to_lowercase();
    match text.as_str() {
        "" | "unknown" | "unk" | "undefined" | "uknown" | "*uknown" | "none" | "null" | "n/a" | "und" => {
            "und".into()
        }
        _ => text,
    }
}
fn validate_selector(selector: &Selector) -> Result<()> {
    let valid = match selector {
        Selector::Languages { values } | Selector::TitleContainsAny { values } => {
            !values.is_empty()
                && values.len() <= 256
                && values
                    .iter()
                    .all(|v| !v.trim().is_empty() && v.len() <= 1024)
        }
        Selector::SourceIndices { values } => !values.is_empty() && values.len() <= 256,
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(DomainError::new(
            "VALIDATION_ERROR",
            "Selector values must be nonempty and bounded",
        ))
    }
}
fn matches(selector: &Selector, stream: &Stream) -> bool {
    match selector {
        Selector::All => true,
        Selector::None => false,
        Selector::SourceIndices { values } => values.contains(&stream.index),
        Selector::Languages { values } => values.iter().any(|v| {
            normalize_language(v)
                == normalize_language(stream.tags.get("language").map_or("und", String::as_str))
        }),
        Selector::TitleContainsAny { values } => {
            let title = stream
                .tags
                .get("title")
                .map_or("", String::as_str)
                .to_lowercase();
            values.iter().any(|v| title.contains(&v.to_lowercase()))
        }
    }
}
fn reorder(streams: &mut [Stream], kind: &StreamKind, order: &[u32], warnings: &mut Vec<String>) {
    let selected: Vec<_> = streams
        .iter()
        .filter(|s| &s.kind == kind)
        .cloned()
        .collect();
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for index in order {
        if !seen.insert(*index) {
            continue;
        }
        if let Some(s) = selected.iter().find(|s| s.index == *index) {
            ordered.push(s.clone());
        } else {
            warnings.push(format!(
                "Order index {index} is missing or dropped; it was not reintroduced"
            ));
        }
    }
    ordered.extend(
        selected
            .iter()
            .filter(|s| !seen.contains(&s.index))
            .cloned(),
    );
    let mut values = ordered.into_iter();
    for stream in streams {
        if &stream.kind == kind {
            *stream = values.next().expect("one value per retained slot");
        }
    }
}
fn patch_matches(patch: &StreamTagPatch, original: &Stream) -> bool {
    patch.kind == original.kind
        && ((patch.source_indices.is_empty() && patch.original_languages.is_empty())
            || patch.source_indices.contains(&original.index)
            || patch.original_languages.iter().any(|v| {
                normalize_language(v)
                    == normalize_language(
                        original.tags.get("language").map_or("und", String::as_str),
                    )
            }))
}
fn apply_tag(tags: &mut BTreeMap<String, String>, name: &str, patch: &TagPatch) {
    match patch {
        TagPatch::Keep => {}
        TagPatch::Set { value } => {
            tags.insert(
                name.into(),
                if name == "language" {
                    normalize_language(value)
                } else {
                    value.clone()
                },
            );
        }
        TagPatch::Clear => {
            tags.remove(name);
        }
    }
}
pub fn resolve(streams: &[Stream], intent: &CleanIntent) -> Result<CleanResolution> {
    if streams.is_empty()
        || streams.len() > 256
        || intent.stream_tag_patches.len() > 256
        || intent.audio_order.len() > 256
        || intent.subtitle_order.len() > 256
    {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "At most 256 streams, order entries and tag patches are admitted",
        ));
    }
    if streams
        .iter()
        .map(|s| s.index)
        .collect::<BTreeSet<_>>()
        .len()
        != streams.len()
    {
        return Err(DomainError::new(
            "INVALID_PROBE",
            "Duplicate source stream index",
        ));
    }
    if !streams
        .iter()
        .any(|s| s.kind == StreamKind::Video && !s.attached_picture())
    {
        return Err(DomainError::new(
            "VIDEO_REQUIRED",
            "Cleanup requires an ordinary video stream",
        ));
    }
    validate_selector(&intent.audio_selection.selector)?;
    validate_selector(&intent.subtitle_selection.selector)?;
    for patch in &intent.stream_tag_patches {
        if patch.source_indices.len() > 256 || patch.original_languages.len() > 256 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Tag matcher is too large",
            ));
        }
        for value in [&patch.language, &patch.title] {
            if let TagPatch::Set { value } = value
                && (value.len() > 4096 || value.contains('\0'))
            {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "Tag values must be bounded and contain no NUL",
                ));
            }
        }
    }
    let mut kept = Vec::new();
    let mut ordinary_video_seen = false;
    for stream in streams {
        let preserve = match stream.kind {
            StreamKind::Audio => matches(&intent.audio_selection.selector, stream),
            StreamKind::Subtitle => matches(&intent.subtitle_selection.selector, stream),
            StreamKind::Video if stream.attached_picture() => intent.preservation.attached_pictures,
            StreamKind::Video => {
                let keep = !ordinary_video_seen || intent.preservation.additional_video;
                ordinary_video_seen = true;
                keep
            }
            StreamKind::Attachment => intent.preservation.attachments,
            StreamKind::Data => intent.preservation.data,
            StreamKind::Unknown => intent.preservation.unknown,
        };
        if preserve {
            kept.push(stream.clone());
        }
    }
    let mut skip = false;
    let mut warnings = Vec::new();
    for (kind, selection) in [
        (StreamKind::Audio, &intent.audio_selection),
        (StreamKind::Subtitle, &intent.subtitle_selection),
    ] {
        if matches!(selection.selector, Selector::None) || kept.iter().any(|s| s.kind == kind) {
            continue;
        }
        let reason = if kind == StreamKind::Audio {
            "NO_AUDIO_MATCH"
        } else {
            "NO_SUBTITLE_MATCH"
        };
        match selection.no_match {
            NoMatch::SkipFile => {
                skip = true;
                warnings.push(reason.into());
            }
            NoMatch::AllowEmpty => warnings.push(reason.into()),
            NoMatch::Reject => {
                return Err(DomainError::new(
                    reason,
                    "No selected streams matched the original probe",
                ));
            }
        }
    }
    reorder(
        &mut kept,
        &StreamKind::Audio,
        &intent.audio_order,
        &mut warnings,
    );
    reorder(
        &mut kept,
        &StreamKind::Subtitle,
        &intent.subtitle_order,
        &mut warnings,
    );
    for output in &mut kept {
        let original = streams
            .iter()
            .find(|s| s.index == output.index)
            .expect("retained source exists");
        if !intent.preservation.stream_tags {
            output.tags.clear();
        }
        if !intent.preservation.dispositions {
            output.dispositions.clear();
        }
        for patch in &intent.stream_tag_patches {
            if patch_matches(patch, original) {
                apply_tag(&mut output.tags, "language", &patch.language);
                apply_tag(&mut output.tags, "title", &patch.title);
            }
        }
    }
    let mut reasons = Vec::new();
    if kept != streams {
        reasons.push("Stream selection, ordering, tags or dispositions change".into());
    }
    if !intent.preservation.global_tags {
        reasons.push("Global metadata stripping requested".into());
    }
    if !intent.preservation.chapters {
        reasons.push("Chapter removal requested".into());
    }
    let action = if skip {
        CleanAction::Skip
    } else if reasons.is_empty() {
        CleanAction::CopyUnchanged
    } else {
        CleanAction::Remux
    };
    let outcomes = streams
        .iter()
        .map(|s| StreamOutcome {
            source_index: s.index,
            output_index: if skip {
                None
            } else {
                kept.iter()
                    .position(|k| k.index == s.index)
                    .map(|i| i as u32)
            },
            reason: if skip {
                "File skipped by explicit no-match policy".into()
            } else if kept.iter().any(|k| k.index == s.index) {
                "Retained by resolved policy".into()
            } else {
                "Dropped by explicit selection/preservation policy".into()
            },
        })
        .collect();
    Ok(CleanResolution {
        action,
        streams: kept,
        outcomes,
        reasons,
        warnings,
        preservation: intent.preservation.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stream(index: u32, kind: StreamKind, language: &str) -> Stream {
        Stream {
            index,
            kind,
            codec: "test".into(),
            tags: BTreeMap::from([("language".into(), language.into())]),
            dispositions: BTreeSet::new(),
        }
    }
    fn intent() -> CleanIntent {
        CleanIntent {
            audio_selection: Selection {
                selector: Selector::All,
                no_match: NoMatch::SkipFile,
            },
            subtitle_selection: Selection {
                selector: Selector::All,
                no_match: NoMatch::AllowEmpty,
            },
            audio_order: vec![],
            subtitle_order: vec![],
            stream_tag_patches: vec![],
            preservation: Preservation::default(),
        }
    }
    #[test]
    fn index_zero_and_independent_attachments() {
        let source = vec![
            stream(0, StreamKind::Audio, "eng"),
            stream(1, StreamKind::Video, "und"),
            stream(2, StreamKind::Subtitle, "fas"),
            stream(3, StreamKind::Attachment, "und"),
        ];
        let mut rules = intent();
        rules.audio_selection.selector = Selector::SourceIndices { values: vec![0] };
        rules.subtitle_selection.selector = Selector::None;
        let result = resolve(&source, &rules).unwrap();
        assert_eq!(
            result.streams.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![0, 1, 3]
        );
        assert_eq!(result.action, CleanAction::Remux);
    }
    #[test]
    fn patches_use_original_language_and_last_explicit_field() {
        let source = vec![
            stream(0, StreamKind::Video, "und"),
            stream(1, StreamKind::Audio, " ENG "),
            stream(2, StreamKind::Audio, "eng"),
        ];
        let mut rules = intent();
        rules.audio_order = vec![2, 2, 77, 1];
        rules.stream_tag_patches = vec![
            StreamTagPatch {
                kind: StreamKind::Audio,
                source_indices: vec![],
                original_languages: vec!["eng".into()],
                language: TagPatch::Set {
                    value: "fas".into(),
                },
                title: TagPatch::Set {
                    value: "First".into(),
                },
            },
            StreamTagPatch {
                kind: StreamKind::Audio,
                source_indices: vec![2],
                original_languages: vec![],
                language: TagPatch::Keep,
                title: TagPatch::Clear,
            },
            StreamTagPatch {
                kind: StreamKind::Audio,
                source_indices: vec![],
                original_languages: vec!["fas".into()],
                language: TagPatch::Set {
                    value: "jpn".into(),
                },
                title: TagPatch::Keep,
            },
        ];
        let result = resolve(&source, &rules).unwrap();
        assert_eq!(result.streams[1].index, 2);
        assert_eq!(result.streams[1].tags["language"], "fas");
        assert!(!result.streams[1].tags.contains_key("title"));
        assert_eq!(result.streams[2].tags["title"], "First");
        assert!(result.warnings.iter().any(|v| v.contains("77")));
    }
    #[test]
    fn no_match_and_exact_language_are_explicit() {
        let source = vec![
            stream(0, StreamKind::Video, "und"),
            stream(1, StreamKind::Audio, "eng"),
        ];
        let mut rules = intent();
        rules.audio_selection.selector = Selector::Languages {
            values: vec!["en".into()],
        };
        assert_eq!(resolve(&source, &rules).unwrap().action, CleanAction::Skip);
        rules.audio_selection.selector = Selector::None;
        assert_eq!(resolve(&source, &rules).unwrap().action, CleanAction::Remux);
        rules.audio_selection.selector = Selector::Languages { values: vec![] };
        assert!(resolve(&source, &rules).is_err());
    }
    #[test]
    fn donor_unknown_language_labels_remain_selectable() {
        let source = vec![
            stream(0, StreamKind::Video, "und"),
            stream(1, StreamKind::Audio, " *UkNoWn "),
            stream(2, StreamKind::Audio, "uknown"),
        ];
        let mut rules = intent();
        rules.audio_selection.selector = Selector::Languages {
            values: vec!["und".into()],
        };
        let result = resolve(&source, &rules).unwrap();
        assert_eq!(result.action, CleanAction::CopyUnchanged);
        assert_eq!(
            result.streams.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(result.streams[1].tags["language"], " *UkNoWn ");
    }
    #[test]
    fn no_op_copy_and_metadata_only_remux_are_distinct() {
        let source = vec![
            stream(0, StreamKind::Video, "und"),
            stream(1, StreamKind::Audio, "eng"),
        ];
        let mut rules = intent();
        assert_eq!(
            resolve(&source, &rules).unwrap().action,
            CleanAction::CopyUnchanged
        );
        rules.preservation.global_tags = false;
        assert_eq!(resolve(&source, &rules).unwrap().action, CleanAction::Remux);
    }
}
