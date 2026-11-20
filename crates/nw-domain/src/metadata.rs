//! Bounded tag changes on an authorized container or original stream.
use crate::{DomainError, Result, mux::Stream};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Global {},
    Stream { index: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub target: Target,
    #[serde(default)]
    pub set: BTreeMap<String, String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchInput {
    pub asset_id: String,
    pub output: String,
    pub patch: Patch,
}

impl PatchInput {
    pub fn into_cleanup(self) -> crate::commands::CleanInput {
        let mut intent = crate::mux::CleanIntent::preserving_all();
        intent.metadata_patch = Some(self.patch);
        crate::commands::CleanInput {
            execution_limits: Default::default(),
            file_times: Default::default(),
            asset_id: self.asset_id,
            output: self.output,
            intent,
        }
    }
}

impl Patch {
    pub fn validate(&self) -> Result<()> {
        if self.set.len() + self.remove.len() > 32 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "At most 32 tag changes are allowed",
            ));
        }
        let mut names = BTreeSet::new();
        if self.set.is_empty() && self.remove.is_empty()
            || self.set.keys().chain(&self.remove).any(|key| {
                key.is_empty()
                    || key.len() > 64
                    || matches!(key.as_str(), "encoder" | "duration")
                    || !key.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                    || !names.insert(key)
            })
            || self
                .set
                .values()
                .any(|value| value.is_empty() || value.len() > 1024 || value.contains('\0'))
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Tag changes must be nonempty and disjoint, with bounded lowercase names and nonempty UTF-8 values without NUL",
            ));
        }
        if self
            .set
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > 8192
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Tag values exceed the 8 KiB edit budget",
            ));
        }
        Ok(())
    }

    pub fn apply(&self, tags: &mut BTreeMap<String, String>) {
        for name in &self.remove {
            tags.remove(name);
        }
        tags.extend(
            self.set
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }

    pub(crate) fn apply_to_streams(&self, streams: &mut [Stream]) -> Result<()> {
        if let Target::Stream { index } = self.target {
            let stream = streams
                .iter_mut()
                .find(|stream| {
                    stream.index == index
                        && matches!(
                            stream.kind,
                            crate::mux::StreamKind::Video
                                | crate::mux::StreamKind::Audio
                                | crate::mux::StreamKind::Subtitle
                        )
                        && !stream.attached_picture()
                })
                .ok_or_else(|| {
                    DomainError::new(
                        "UNRESOLVED_SOURCE_INDEX",
                        "Metadata patch requires a retained video, audio or subtitle stream",
                    )
                })?;
            self.apply(&mut stream.tags);
        }
        Ok(())
    }
}
