//! Explicit disposition changes on retained audio and subtitle streams.
use crate::{DomainError, Result, mux::{Stream, StreamKind}};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const PATCH_FLAGS: &[&str] = &[
    "default", "dub", "original", "comment", "lyrics", "karaoke", "forced",
    "hearing_impaired", "visual_impaired", "clean_effects", "non_diegetic",
    "captions", "descriptions", "metadata", "dependent",
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub stream: u32,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default)]
    pub clear_all: bool,
    #[serde(default)]
    pub exclusive_default: bool,
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
        intent.disposition_patches.push(self.patch);
        crate::commands::CleanInput {
            execution_limits: Default::default(),
            file_times: Default::default(),
            asset_id: self.asset_id,
            output: self.output,
            intent,
        }
    }
}

pub(crate) fn validate(patches: &[Patch]) -> Result<()> {
    if patches.len() > 256 {
        return Err(DomainError::new("RESOURCE_LIMIT", "Too many disposition patches"));
    }
    for patch in patches {
        let flag_count = patch.add.len() + patch.remove.len();
        if flag_count > PATCH_FLAGS.len() * 2
            || (flag_count == 0 && !patch.clear_all && !patch.exclusive_default)
            || patch.add.iter().chain(&patch.remove).any(|flag| !PATCH_FLAGS.contains(&flag.as_str()))
            || patch.remove.iter().any(|flag| {
                patch.add.contains(flag) || (patch.exclusive_default && flag == "default")
            })
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR", "Disposition patch is empty, unsupported or contradictory",
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply(streams: &mut [Stream], patches: &[Patch]) -> Result<()> {
    for patch in patches {
        let target = streams.iter_mut().find(|stream| stream.index == patch.stream)
            .filter(|stream| matches!(stream.kind, StreamKind::Audio | StreamKind::Subtitle))
            .ok_or_else(|| DomainError::new("UNRESOLVED_SOURCE_INDEX", "Disposition patch requires a retained audio or subtitle stream"))?;
        if patch.clear_all {
            target.dispositions.clear();
        }
        for flag in &patch.remove {
            target.dispositions.remove(flag);
        }
        target.dispositions.extend(patch.add.iter().cloned());
        if patch.exclusive_default {
            target.dispositions.insert("default".into());
            let kind = target.kind.clone();
            for stream in streams.iter_mut().filter(|s| s.kind == kind && s.index != patch.stream) {
                stream.dispositions.remove("default");
            }
        }
    }
    Ok(())
}
