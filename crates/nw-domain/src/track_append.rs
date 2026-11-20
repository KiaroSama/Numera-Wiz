//! Strict stream-copy additions use original physical audio and subtitle indices.
use crate::{
    DomainError, Result,
    mux::{Stream, StreamKind},
    records::Asset,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_ADDED_INPUTS: usize = 4;
pub const MAX_ADDED_TRACKS: usize = 32;
pub const MAX_STAGED_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppendInput {
    pub target: String,
    #[schemars(length(min = 1, max = 4))]
    pub additions: Vec<Addition>,
    pub output: String,
    pub conversion: Conversion,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceInput {
    pub target: String,
    #[serde(default)]
    #[schemars(length(max = 32))]
    pub remove: Vec<Removal>,
    #[serde(default)]
    #[schemars(length(max = 4))]
    pub additions: Vec<Addition>,
    pub metadata: Metadata,
    pub output: String,
    pub conversion: Conversion,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Metadata {
    Preserve,
    StripGlobal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemovableTrack {
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Removal {
    OriginalIndex {
        index: u32,
    },
    TypeOrdinal {
        track_kind: RemovableTrack,
        ordinal: usize,
    },
}

impl ReplaceInput {
    pub fn validate(&self) -> Result<()> {
        if self.remove.len() > MAX_ADDED_TRACKS || self.additions.len() > MAX_ADDED_INPUTS {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Replacement exceeds the admitted track or input bound",
            ));
        }
        if self.remove.is_empty()
            && self.additions.is_empty()
            && matches!(self.metadata, Metadata::Preserve)
        {
            return Err(invalid("Select removals, additions or a metadata change"));
        }
        Ok(())
    }

    pub fn primary_intent(&self, streams: &[Stream]) -> Result<crate::mux::CleanIntent> {
        self.validate()?;
        let mut remove = BTreeSet::new();
        for removal in &self.remove {
            let source = match removal {
                Removal::OriginalIndex { index } => streams
                    .iter()
                    .find(|stream| stream.index == *index && eligible(stream)),
                Removal::TypeOrdinal {
                    track_kind,
                    ordinal,
                } => {
                    let kind = match track_kind {
                        RemovableTrack::Audio => StreamKind::Audio,
                        RemovableTrack::Subtitle => StreamKind::Subtitle,
                    };
                    streams
                        .iter()
                        .filter(|stream| stream.kind == kind)
                        .nth(*ordinal)
                }
            }
            .ok_or_else(|| {
                invalid("Removal does not resolve to an original audio or subtitle track")
            })?;
            if !remove.insert(source.index) {
                return Err(invalid(
                    "Two removal selectors address the same source track",
                ));
            }
        }
        let retained = |kind| {
            let values = streams
                .iter()
                .filter(|stream| stream.kind == kind && !remove.contains(&stream.index))
                .map(|stream| stream.index)
                .collect::<Vec<_>>();
            crate::mux::Selection {
                selector: if values.is_empty() {
                    crate::mux::Selector::None
                } else {
                    crate::mux::Selector::SourceIndices { values }
                },
                no_match: crate::mux::NoMatch::AllowEmpty,
            }
        };
        let mut intent = crate::mux::CleanIntent::preserving_all();
        intent.audio_selection = retained(StreamKind::Audio);
        intent.subtitle_selection = retained(StreamKind::Subtitle);
        intent.preservation.global_tags = matches!(self.metadata, Metadata::Preserve);
        Ok(intent)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Conversion {
    Forbid,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Addition {
    pub asset_id: String,
    pub selector: Selection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    All {},
    Explicit {
        #[schemars(length(min = 1, max = 32))]
        tracks: Vec<RequestedTrack>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestedTrack {
    pub stream_index: u32,
    pub title: Option<String>,
    pub language: Option<String>,
    pub disposition: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddedInput {
    pub source: Asset,
    pub tracks: Vec<Stream>,
}

fn eligible(stream: &Stream) -> bool {
    matches!(stream.kind, StreamKind::Audio | StreamKind::Subtitle)
}

impl Addition {
    pub fn resolve(self, source: Asset) -> Result<AddedInput> {
        if source.id != self.asset_id {
            return Err(invalid("External track source identity differs"));
        }
        let tracks = match self.selector {
            Selection::All {} => source
                .probe
                .streams
                .iter()
                .filter(|track| eligible(track))
                .cloned()
                .collect(),
            Selection::Explicit { tracks } => tracks
                .into_iter()
                .map(|request| resolve_track(&source, request))
                .collect::<Result<Vec<_>>>()?,
        };
        let added = AddedInput { source, tracks };
        added.validate()?;
        Ok(added)
    }
}

fn resolve_track(source: &Asset, request: RequestedTrack) -> Result<Stream> {
    let mut track = source
        .probe
        .streams
        .iter()
        .find(|stream| stream.index == request.stream_index && eligible(stream))
        .ok_or_else(|| invalid("Append selection requires an original audio or subtitle stream"))?
        .clone();
    if let Some(title) = request.title {
        if title.is_empty() || title.len() > 1024 || title.contains('\0') {
            return Err(invalid(
                "Track title requires 1–1024 UTF-8 bytes without NUL",
            ));
        }
        track.tags.insert("title".into(), title);
    }
    if let Some(language) = request.language {
        if language.len() != 3 || !language.bytes().all(|byte| byte.is_ascii_lowercase()) {
            return Err(invalid(
                "Track language requires a lowercase ISO 639-2 code",
            ));
        }
        track.tags.insert("language".into(), language);
    }
    if let Some(disposition) = request.disposition {
        if disposition
            .iter()
            .any(|value| !crate::dispositions::PATCH_FLAGS.contains(&value.as_str()))
        {
            return Err(invalid("Append disposition contains an unsupported flag"));
        }
        track.dispositions = disposition;
    }
    Ok(track)
}

impl AddedInput {
    fn validate(&self) -> Result<()> {
        let mut indices = BTreeSet::new();
        if self.source.source_object.is_none()
            || self.tracks.is_empty()
            || self.tracks.len() > MAX_ADDED_TRACKS
            || self.tracks.iter().any(|track| {
                !indices.insert(track.index)
                    || !eligible(track)
                    || !self.source.probe.streams.iter().any(|original| {
                        original.index == track.index
                            && original.kind == track.kind
                            && original.codec == track.codec
                    })
            })
        {
            return Err(invalid(
                "An addition needs distinct registered audio or subtitle tracks",
            ));
        }
        Ok(())
    }
}

pub fn validate_sources(primary: &Asset, additions: &[AddedInput], output: &str) -> Result<()> {
    if !(1..=MAX_ADDED_INPUTS).contains(&additions.len()) {
        return Err(invalid("Add 1–4 distinct external media inputs"));
    }
    let mut objects = vec![&primary.source_object];
    let mut tracks = 0usize;
    let mut bytes = 0u64;
    for addition in additions {
        addition.validate()?;
        if objects.contains(&&addition.source.source_object)
            || addition.source.identity.path == output
        {
            return Err(invalid(
                "All external inputs need distinct physical identities",
            ));
        }
        objects.push(&addition.source.source_object);
        tracks += addition.tracks.len();
        let length = addition
            .source
            .identity
            .bytes
            .parse::<u64>()
            .ok()
            .filter(|length| length.to_string() == addition.source.identity.bytes)
            .ok_or_else(|| invalid("External input length is invalid"))?;
        bytes = bytes
            .checked_add(length)
            .ok_or_else(|| invalid("External input length overflows"))?;
    }
    if tracks > MAX_ADDED_TRACKS || bytes > MAX_STAGED_BYTES {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Track addition exceeds the admitted track or scratch limit",
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_TRACK_ADDITION", message)
}
