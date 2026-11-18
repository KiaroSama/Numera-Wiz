//! Chapter inspection and lossless FFmetadata text export from a registered probe.
use crate::{DomainError, Result, records::Asset, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write};

const MAX_CHAPTERS: usize = 256;
const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportInput {
    pub asset_id: String,
    #[serde(default)]
    pub include_global_metadata: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveInput {
    pub asset_id: String,
    pub output: String,
}

impl RemoveInput {
    pub fn into_cleanup(self) -> crate::commands::CleanInput {
        let mut intent = crate::mux::CleanIntent::preserving_all();
        intent.preservation.chapters = false;
        crate::commands::CleanInput {
            execution_limits: Default::default(),
            file_times: Default::default(),
            asset_id: self.asset_id,
            output: self.output,
            intent,
        }
    }
}

#[derive(Serialize, JsonSchema)]
pub struct Inventory {
    pub asset_id: String,
    pub source_sha256: String,
    #[schemars(length(max = 256))]
    pub chapters: Vec<Chapter>,
}

#[derive(Serialize, JsonSchema)]
pub struct Chapter {
    pub time_base: Rational,
    pub start_ticks: String,
    pub end_ticks: String,
    pub tags: BTreeMap<String, String>,
}

#[derive(Serialize, JsonSchema)]
pub struct Export {
    pub asset_id: String,
    pub source_sha256: String,
    #[schemars(length(max = 65536))]
    pub ffmetadata: String,
    pub sha256: String,
}

#[derive(Deserialize)]
struct ProbedChapter {
    time_base: String,
    start: i64,
    end: i64,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

fn limit() -> DomainError {
    DomainError::new(
        "RESOURCE_LIMIT",
        "Chapter data exceeds its bounded text or count budget",
    )
}

pub fn inspect(asset: &Asset) -> Result<Inventory> {
    let raw = asset.probe.chapters.as_array().ok_or_else(|| {
        DomainError::new(
            "INVALID_PROBE",
            "Registered chapter inventory must be an array",
        )
    })?;
    if raw.len() > MAX_CHAPTERS {
        return Err(limit());
    }
    let mut chapters = Vec::with_capacity(raw.len());
    let mut tag_bytes = 0;
    for row in raw {
        let invalid = || {
            DomainError::new("INVALID_PROBE", "Invalid registered chapter extent or tags")
        };
        let observed = ProbedChapter::deserialize(row).map_err(|_| invalid())?;
        let (num, den) = observed.time_base.split_once('/').ok_or_else(invalid)?;
        let num = num.parse::<i32>().map_err(|_| invalid())?;
        let den = den.parse::<i32>().map_err(|_| invalid())?;
        if num <= 0 || den <= 0 || observed.start < 0 || observed.end <= observed.start {
            return Err(invalid());
        }
        for (key, value) in &observed.tags {
            tag_bytes += key.len() + value.len();
            if tag_bytes > MAX_TEXT_BYTES {
                return Err(limit());
            }
        }
        chapters.push(Chapter {
            time_base: Rational::new(i64::from(num), i64::from(den))?,
            start_ticks: observed.start.to_string(),
            end_ticks: observed.end.to_string(),
            tags: observed.tags,
        });
    }
    Ok(Inventory {
        asset_id: asset.id.clone(),
        source_sha256: asset.identity.sha256.clone(),
        chapters,
    })
}

pub fn export(asset: &Asset, include_global_metadata: bool) -> Result<Export> {
    let inventory = inspect(asset)?;
    let mut ffmetadata = String::from(";FFMETADATA1\n");
    if include_global_metadata {
        write_tags(&mut ffmetadata, &asset.probe.global_tags)?;
    }
    for chapter in inventory.chapters {
        let base = chapter.time_base;
        writeln!(
            ffmetadata,
            "[CHAPTER]\nTIMEBASE={}/{}\nSTART={}\nEND={}",
            base.numerator(),
            base.denominator(),
            chapter.start_ticks,
            chapter.end_ticks,
        )
        .expect("String writes are infallible");
        write_tags(&mut ffmetadata, &chapter.tags)?;
    }
    if ffmetadata.len() > MAX_TEXT_BYTES {
        return Err(limit());
    }
    Ok(Export {
        asset_id: inventory.asset_id,
        source_sha256: inventory.source_sha256,
        sha256: crate::projects::hash(ffmetadata.as_bytes()),
        ffmetadata,
    })
}

fn write_tags(out: &mut String, tags: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in tags {
        if key.is_empty() || key.contains('\0') || value.contains('\0') || value.ends_with('\\') {
            return Err(DomainError::new(
                "INVALID_PROBE",
                "FFmetadata tag contains an empty name, NUL or a trailing backslash in its value",
            ));
        }
        if key.len() + value.len() + out.len() > MAX_TEXT_BYTES {
            return Err(limit());
        }
        for (part, delimiter) in [(key, '='), (value, '\n')] {
            for character in part.chars() {
                if matches!(character, '=' | ';' | '#' | '\\' | '\n' | '\r') {
                    out.push('\\');
                }
                out.push(character);
            }
            out.push(delimiter);
        }
        if out.len() > MAX_TEXT_BYTES {
            return Err(limit());
        }
    }
    Ok(())
}
