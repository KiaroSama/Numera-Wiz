use super::{Chapter, MAX_CHAPTERS, MAX_TEXT_BYTES, limit, write_chapters};
use crate::{DomainError, Result, time::{Rational, TimeRange}};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportInput {
    pub asset_id: String,
    pub output: String,
    #[schemars(length(max = 65536))]
    pub ffmetadata: String,
    #[serde(default)]
    pub include_global_metadata: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Import {
    #[schemars(length(min = 1, max = 256))]
    pub chapters: Vec<Chapter>,
    pub global_tags: BTreeMap<String, String>,
}

impl ImportInput {
    pub fn into_cleanup(self) -> Result<crate::commands::CleanInput> {
        let mut intent = crate::mux::CleanIntent::preserving_all();
        intent.chapter_import = Some(Import::parse(&self.ffmetadata, self.include_global_metadata)?);
        Ok(crate::commands::CleanInput {
            execution_limits: Default::default(),
            file_times: Default::default(),
            asset_id: self.asset_id,
            output: self.output,
            intent,
        })
    }
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_CHAPTERS", message)
}

fn integer(value: &str) -> Result<i64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("Chapter offsets and time-base fields must be unsigned decimal integers"));
    }
    value.parse().map_err(|_| invalid("Chapter time field is too large"))
}

fn range(chapter: &Chapter) -> Result<TimeRange> {
    let base = chapter.time_base;
    if base <= Rational::ZERO || base.numerator() > i64::from(i32::MAX)
        || base.denominator() > i64::from(i32::MAX) {
        return Err(invalid("Chapter time bases must have positive 32-bit terms"));
    }
    let points = |text: &str| {
        let tick = integer(text)?;
        let seconds = base.checked_mul(Rational::new(tick, 1)?)?;
        let nanoseconds = seconds.checked_mul(Rational::new(1_000_000_000, 1)?)?;
        if nanoseconds.denominator() != 1 {
            return Err(invalid("Chapter time does not fit the Matroska nanosecond grid"));
        }
        Ok(seconds)
    };
    let output = TimeRange { start: points(&chapter.start_ticks)?, end: points(&chapter.end_ticks)? };
    output.validate().map_err(|_| invalid("Chapter end must follow its start"))?;
    Ok(output)
}

fn chapter(mut fields: BTreeMap<String, String>) -> Result<Chapter> {
    let base = fields.remove("TIMEBASE").ok_or_else(|| invalid("TIMEBASE is required"))?;
    let (num, den) = base.split_once('/').ok_or_else(|| invalid("TIMEBASE must be a rational ratio"))?;
    let (num, den) = (integer(num)?, integer(den)?);
    if num == 0 || den == 0 || num > i64::from(i32::MAX) || den > i64::from(i32::MAX) {
        return Err(invalid("Chapter time bases must have positive 32-bit terms"));
    }
    let start = fields.remove("START").ok_or_else(|| invalid("START is required"))?;
    let end = fields.remove("END").ok_or_else(|| invalid("END is required"))?;
    Ok(Chapter {
        time_base: Rational::new(num, den)?,
        start_ticks: integer(&start)?.to_string(),
        end_ticks: integer(&end)?.to_string(),
        tags: fields,
    })
}

// Preserve escaped newlines and distinguish a tag's escaped equals sign from its delimiter.
fn field(row: &str) -> Result<(String, String)> {
    let mut key = String::new();
    let mut value = None::<String>;
    let mut chars = row.chars();
    while let Some(character) = chars.next() {
        if character == '=' && value.is_none() {
            value = Some(String::new());
            continue;
        }
        let character = if character == '\\' {
            chars.next().ok_or_else(|| invalid("Incomplete FFmetadata escape"))?
        } else {
            character
        };
        value.as_mut().unwrap_or(&mut key).push(character);
    }
    let value = value.filter(|_| !key.is_empty()).ok_or_else(|| invalid("FFmetadata row must be a key and value"))?;
    Ok((key, value))
}

impl Import {
    pub fn parse(text: &str, include_global_metadata: bool) -> Result<Self> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(limit());
        }
        if text.contains('\0') {
            return Err(invalid("FFmetadata must contain no NUL"));
        }
        let (header, text) = text.split_once('\n').ok_or_else(|| invalid("Missing FFmetadata header"))?;
        if header.trim_end_matches('\r') != ";FFMETADATA1" {
            return Err(invalid("Expected the FFmetadata version 1 header"));
        }
        let mut lines = text.split_inclusive('\n');
        let mut fields = BTreeMap::new();
        let mut imported = Self { chapters: Vec::new(), global_tags: BTreeMap::new() };
        let mut in_chapter = false;
        while let Some(physical) = lines.next() {
            let mut row = physical.trim_end_matches(['\r', '\n']).to_owned();
            if row.is_empty() || row.starts_with([';', '#']) {
                continue;
            }
            while row.bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 1 {
                let following = lines.next().ok_or_else(|| invalid("Incomplete FFmetadata line continuation"))?;
                row.push('\n');
                row.push_str(following.trim_end_matches(['\r', '\n']));
            }
            if row == "[CHAPTER]" {
                if in_chapter {
                    imported.chapters.push(chapter(std::mem::take(&mut fields))?);
                    if imported.chapters.len() == MAX_CHAPTERS {
                        return Err(limit());
                    }
                } else {
                    if include_global_metadata {
                        imported.global_tags = std::mem::take(&mut fields);
                    }
                    fields.clear();
                    in_chapter = true;
                }
            } else if row.starts_with('[') {
                return Err(invalid("Only chapter sections are admitted in this FFmetadata document"));
            } else {
                let (key, value) = field(&row)?;
                if fields.insert(key, value).is_some() {
                    return Err(invalid("FFmetadata section repeats a key"));
                }
            }
        }
        if in_chapter {
            imported.chapters.push(chapter(fields)?);
        }
        imported.validate(None)?;
        Ok(imported)
    }

    pub fn document(&self) -> Result<String> {
        let mut document = String::from(";FFMETADATA1\n");
        write_chapters(&mut document, &self.chapters)?;
        Ok(document)
    }

    pub fn validate(&self, extent: Option<Rational>) -> Result<()> {
        if self.chapters.is_empty() {
            return Err(invalid("Import requires at least one chapter"));
        }
        if self.chapters.len() > MAX_CHAPTERS {
            return Err(limit());
        }
        let mut end = Rational::ZERO;
        for chapter in &self.chapters {
            let timing = range(chapter)?;
            if timing.start < end || extent.is_some_and(|extent| timing.end > extent) {
                return Err(invalid("Chapters must be ordered, nonoverlapping and within the registered extent"));
            }
            let mut names = BTreeSet::new();
            if chapter.tags.keys().any(|key| {
                let key = key.to_lowercase();
                matches!(key.as_str(), "timebase" | "start" | "end") || !names.insert(key)
            }) {
                return Err(invalid("Chapter tags cannot repeat names or duplicate timing fields"));
            }
            end = timing.end;
        }
        self.document()?;
        if !self.global_tags.is_empty() {
            crate::metadata::Patch {
                target: crate::metadata::Target::Global {},
                set: self.global_tags.clone(),
                remove: Vec::new(),
            }.validate()?;
        }
        Ok(())
    }

    pub fn match_observed(&self, observed: &[Chapter]) -> Result<()> {
        if self.chapters.len() != observed.len() {
            return Err(invalid("Output chapter count differs from the reviewed import"));
        }
        for (expected, actual) in self.chapters.iter().zip(observed) {
            let observed_tags = actual.tags.iter()
                .map(|(name, value)| (name.to_lowercase(), value)).collect::<BTreeMap<_, _>>();
            if range(expected)? != range(actual)?
                || expected.tags.len() != actual.tags.len()
                || expected.tags.iter().any(|(name, value)| observed_tags.get(&name.to_lowercase()) != Some(&value))
            {
                return Err(invalid("Output chapter times or tags differ from the reviewed import"));
            }
        }
        Ok(())
    }
}
