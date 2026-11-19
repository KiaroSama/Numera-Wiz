//! OpenTimelineIO single-track cut lists with exact time and separate media bindings.
use crate::{
    DomainError, Result, commands,
    media::{MixedCapture, OccurrenceSource},
    projects::{
        EntityId, MAX_CHANGES, PlaceInput, PlaceSource, ProjectId, Revision, SourceSpan, TrackKind,
        hash,
    },
    time::{Rational, TimeRange},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_OTIO_BYTES: usize = 96 * 1024;
const MAX_CUTS: usize = (MAX_CHANGES - 2) / 2;
const MAX_ITEMS: usize = MAX_CUTS * 2 + 1;
const MAX_EXACT_INTEGER: i64 = (1 << 53) - 1;
const DEFAULT_MEDIA: &str = "DEFAULT_MEDIA";
const SOLID_COLOR: &str = "NumeraWiz.SolidColor";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OtioSource {
    #[schemars(length(min = 1, max = 4096))]
    pub target_url: String,
    pub asset_id: String,
    pub stream_index: u32,
    pub content_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OtioExport {
    pub project_id: ProjectId,
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    pub revision: Revision,
    pub range: TimeRange,
    pub document: String,
    pub document_sha256: String,
    pub sources: Vec<OtioSource>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OtioImportInput {
    pub sequence_id: EntityId,
    pub track_id: EntityId,
    /// Placement offset in the destination sequence. Default is zero.
    pub start: Option<Rational>,
    #[schemars(length(min = 1, max = MAX_OTIO_BYTES))]
    pub document: String,
    #[schemars(length(max = MAX_CUTS))]
    pub sources: Vec<OtioSource>,
}

pub struct ImportedOtio {
    pub kind: TrackKind,
    pub range: TimeRange,
    pub document_sha256: String,
    pub cuts: Vec<(PlaceInput, Option<String>)>,
}

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("OTIO_INVALID", message)
}

fn unsupported(message: &'static str) -> DomainError {
    DomainError::new("OTIO_UNREPRESENTABLE", message)
}

fn bounded_integer(value: f64) -> Option<i64> {
    ((0.0..=MAX_EXACT_INTEGER as f64).contains(&value) && value.fract() == 0.0)
        .then_some(value as i64)
}

fn rate(value: f64) -> Option<Rational> {
    if let Some(integer) = bounded_integer(value) {
        return (integer != 0).then(|| Rational::new(integer, 1).expect("positive integer rate"));
    }
    [24000, 30000, 60000, 120000]
        .into_iter()
        .find(|numerator| {
            let standard = f64::from(*numerator) / 1001.0;
            // Other OTIO serializers can round the decimal rate by one ULP.
            (value - standard).abs() <= standard * f64::EPSILON
        })
        .map(|numerator| Rational::new(i64::from(numerator), 1001).expect("constant rate"))
}

fn encode_time(time: Rational, preferred_rate: Rational) -> Result<Value> {
    let ticks = time.checked_mul(preferred_rate)?;
    let preferred = preferred_rate.numerator() as f64 / preferred_rate.denominator() as f64;
    let (value, time_rate) = if ticks.denominator() == 1 && rate(preferred) == Some(preferred_rate)
    {
        (ticks.numerator(), json!(preferred))
    } else {
        if time.denominator() > MAX_EXACT_INTEGER {
            return Err(unsupported(
                "Time denominator exceeds OTIO's exact numeric range",
            ));
        }
        (time.numerator(), json!(time.denominator()))
    };
    if !(0..=MAX_EXACT_INTEGER).contains(&value) {
        return Err(unsupported("Time ticks exceed OTIO's exact numeric range"));
    }
    Ok(json!({"OTIO_SCHEMA":"RationalTime.1","rate":time_rate,"value":value}))
}

fn encode_range(range: TimeRange, grid: Rational) -> Result<Value> {
    range.validate()?;
    Ok(json!({"OTIO_SCHEMA":"TimeRange.1",
        "start_time":encode_time(range.start, grid)?,
        "duration":encode_time(range.end.checked_sub(range.start)?, grid)?}))
}

fn gap(duration: Rational, grid: Rational) -> Result<Value> {
    Ok(
        json!({"OTIO_SCHEMA":"Gap.1","name":"Gap","metadata":{},"enabled":true,
        "effects":[],"markers":[],"source_range":encode_range(TimeRange {
            start:Rational::ZERO, end:duration
        }, grid)?}),
    )
}

pub fn export_otio(capture: &MixedCapture, track_id: &EntityId) -> Result<OtioExport> {
    capture.range.validate()?;
    let track = capture
        .tracks
        .iter()
        .find(|track| track.id == *track_id && track.gain_centidb == 0)
        .ok_or_else(|| {
            unsupported("Select an enabled unity-gain track in the captured sequence")
        })?;
    if track.clips.len() > MAX_CUTS {
        return Err(crate::projects::limit(
            "OTIO exceeds the reversible cut count",
        ));
    }
    let (kind, grid) = match track.kind {
        TrackKind::Video => ("Video", capture.settings.frame_rate),
        TrackKind::Audio => (
            "Audio",
            Rational::new(capture.settings.sample_rate.into(), 1)?,
        ),
    };
    let mut cursor = capture.range.start;
    let mut children = Vec::new();
    let mut sources = BTreeMap::new();
    for clip in &track.clips {
        let record = clip.sequence_range;
        if record.start < cursor
            || record.end > capture.range.end
            || record.end <= record.start
            || clip.rate != Rational::new(1, 1)?
            || clip.source_range.end.checked_sub(clip.source_range.start)?
                != record.end.checked_sub(record.start)?
        {
            return Err(unsupported(
                "OTIO requires ordered unit-rate cuts in the selected range",
            ));
        }
        if record.start > cursor {
            children.push(gap(record.start.checked_sub(cursor)?, grid)?);
        }
        let reference = match &clip.source {
            OccurrenceSource::Registered {
                asset_id,
                stream_index,
                content_sha256,
                media_kind,
                ..
            } if *media_kind == track.kind => {
                let target_url = format!("urn:numera-wiz:asset:{asset_id}:{stream_index}");
                let binding = OtioSource {
                    target_url: target_url.clone(),
                    asset_id: asset_id.clone(),
                    stream_index: *stream_index,
                    content_sha256: content_sha256.clone(),
                };
                if sources
                    .insert(target_url.clone(), binding.clone())
                    .is_some_and(|old| old != binding)
                {
                    return Err(unsupported(
                        "Cuts disagree on the registered media identity",
                    ));
                }
                json!({"OTIO_SCHEMA":"ExternalReference.1","name":"Registered media",
                    "metadata":{},"available_range":null,"target_url":target_url})
            }
            OccurrenceSource::SolidColor { rgb } if track.kind == TrackKind::Video => {
                json!({"OTIO_SCHEMA":"GeneratorReference.1","name":"Solid color","metadata":{},
                    "available_range":encode_range(TimeRange { start:Rational::ZERO, end:clip.source_range.end }, grid)?,
                    "generator_kind":SOLID_COLOR,"parameters":{"rgb":rgb}})
            }
            _ => {
                return Err(unsupported(
                    "OTIO cut media differs from the selected track kind",
                ));
            }
        };
        children.push(json!({"OTIO_SCHEMA":"Clip.2","name":"Cut","metadata":{},"enabled":true,
            "effects":[],"markers":[],"source_range":encode_range(clip.source_range,grid)?,
            "media_references":{(DEFAULT_MEDIA):reference},"active_media_reference_key":DEFAULT_MEDIA}));
        cursor = record.end;
    }
    if cursor < capture.range.end {
        children.push(gap(capture.range.end.checked_sub(cursor)?, grid)?);
    }
    let document = json!({"OTIO_SCHEMA":"Timeline.1","name":"Numera Wiz","metadata":{},
    "global_start_time":encode_time(capture.range.start, capture.settings.frame_rate)?,
    "tracks":{"OTIO_SCHEMA":"Stack.1","name":"Tracks","metadata":{},"enabled":true,
        "effects":[],"markers":[],"source_range":null,"children":[{
            "OTIO_SCHEMA":"Track.1","name":kind,"kind":kind,"metadata":{},"enabled":true,
            "effects":[],"markers":[],"source_range":null,"children":children
        }]}})
    .to_string();
    if document.len() > MAX_OTIO_BYTES {
        return Err(crate::projects::limit("OTIO document exceeds 96 KiB"));
    }
    Ok(OtioExport {
        project_id: capture.project_id.clone(),
        sequence_id: capture.sequence_id.clone(),
        track_id: track_id.clone(),
        revision: capture.project_revision,
        range: capture.range,
        document_sha256: hash(document.as_bytes()),
        document,
        sources: sources.into_values().collect(),
    })
}

fn time(value: &Value) -> Result<Rational> {
    if value["OTIO_SCHEMA"] != "RationalTime.1" {
        return Err(invalid("Expected an OTIO RationalTime.1"));
    }
    let ticks = value["value"]
        .as_f64()
        .and_then(bounded_integer)
        .ok_or_else(|| invalid("OTIO ticks must be nonnegative exact integers"))?;
    let grid = value["rate"]
        .as_f64()
        .and_then(rate)
        .ok_or_else(|| invalid("OTIO rate must be a positive integer or a supported NTSC rate"))?;
    Rational::new(ticks, grid.numerator())?.checked_mul(Rational::new(grid.denominator(), 1)?)
}

fn range(value: &Value) -> Result<SourceSpan> {
    if value["OTIO_SCHEMA"] != "TimeRange.1" {
        return Err(invalid("Expected an OTIO TimeRange.1"));
    }
    let start = time(&value["start_time"])?;
    let duration = time(&value["duration"])?;
    if duration <= Rational::ZERO {
        return Err(invalid("OTIO ranges require a positive duration"));
    }
    Ok(SourceSpan {
        start,
        end: start.checked_add(duration)?,
    })
}

fn validate_item(item: &Value) -> Result<()> {
    if item
        .get("enabled")
        .is_some_and(|enabled| enabled.as_bool() != Some(true))
        || ["markers", "effects"].iter().any(|key| {
            item.get(*key)
                .is_some_and(|values| !values.as_array().is_some_and(Vec::is_empty))
        })
    {
        return Err(unsupported(
            "Disabled OTIO items, effects and markers need explicit conversion",
        ));
    }
    Ok(())
}

fn children<'a>(item: &'a Value, schema: &str) -> Result<&'a [Value]> {
    if item["OTIO_SCHEMA"] != schema || !item["source_range"].is_null() {
        return Err(unsupported(
            "OTIO requires an untrimmed Stack.1 containing one untrimmed Track.1",
        ));
    }
    validate_item(item)?;
    item["children"]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid("OTIO composition requires a child array"))
}

fn media_reference(item: &Value) -> Result<&Value> {
    match item["OTIO_SCHEMA"].as_str() {
        Some("Clip.1") => Ok(&item["media_reference"]),
        Some("Clip.2") => {
            let media = item["media_references"]
                .as_object()
                .filter(|references| references.len() == 1)
                .ok_or_else(|| unsupported("OTIO clips require one media reference"))?;
            item["active_media_reference_key"]
                .as_str()
                .and_then(|key| media.get(key))
                .ok_or_else(|| invalid("OTIO clip has no matching active media reference"))
        }
        _ => Err(unsupported("OTIO tracks admit clips and gaps only")),
    }
}

pub fn parse_otio(input: OtioImportInput) -> Result<ImportedOtio> {
    if input.document.len() > MAX_OTIO_BYTES || input.sources.len() > MAX_CUTS {
        return Err(crate::projects::limit(
            "OTIO exceeds the bounded document or media mapping size",
        ));
    }
    let document = commands::decode_json(input.document.as_bytes())
        .map_err(|_| invalid("Expected a bounded OTIO JSON document without duplicate keys"))?;
    if document["OTIO_SCHEMA"] != "Timeline.1" {
        return Err(invalid("Expected an OTIO Timeline.1 document"));
    }
    if !document["global_start_time"].is_null() {
        time(&document["global_start_time"])?;
    }
    let [track] = children(&document["tracks"], "Stack.1")? else {
        return Err(unsupported(
            "Import one OTIO track into a selected project track",
        ));
    };
    let kind = match track["kind"].as_str() {
        Some("Video") => TrackKind::Video,
        Some("Audio") => TrackKind::Audio,
        _ => return Err(unsupported("OTIO track kind must be Video or Audio")),
    };
    let items = children(track, "Track.1")?;
    if items.len() > MAX_ITEMS {
        return Err(crate::projects::limit(
            "OTIO exceeds the bounded cut and gap count",
        ));
    }
    let mut bindings = BTreeMap::new();
    for source in &input.sources {
        if source.target_url.is_empty()
            || source.target_url.len() > 4096
            || source.target_url.chars().any(char::is_control)
            || source.content_sha256.len() != 64
            || !source
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("Invalid OTIO media URL or SHA-256 binding"));
        }
        crate::projects::validate_id(&source.asset_id)?;
        if bindings
            .insert(source.target_url.as_str(), source)
            .is_some()
        {
            return Err(invalid("Duplicate OTIO media binding"));
        }
    }
    let start = input.start.unwrap_or(Rational::ZERO);
    if start < Rational::ZERO {
        return Err(invalid("The destination start must be nonnegative"));
    }
    let mut cursor = start;
    let mut cuts = Vec::new();
    let mut used = BTreeSet::new();
    for item in items {
        validate_item(item)?;
        if item["OTIO_SCHEMA"] == "Gap.1" {
            cursor = cursor.checked_add(range(&item["source_range"])?.duration()?)?;
            continue;
        }
        if cuts.len() == MAX_CUTS {
            return Err(crate::projects::limit("OTIO exceeds the atomic edit limit"));
        }
        let reference = media_reference(item)?;
        let available = (!reference["available_range"].is_null())
            .then(|| range(&reference["available_range"]))
            .transpose()?;
        let source_range = if item["source_range"].is_null() {
            available.ok_or_else(|| invalid("An OTIO clip requires a source or available range"))?
        } else {
            range(&item["source_range"])?
        };
        if available.is_some_and(|bounds| {
            source_range.start < bounds.start || source_range.end > bounds.end
        }) {
            return Err(invalid("OTIO cut exceeds the declared available range"));
        }
        let (source, expected_sha256) = match reference["OTIO_SCHEMA"].as_str() {
            Some("ExternalReference.1") => {
                let (url, binding) = reference["target_url"]
                    .as_str()
                    .and_then(|url| bindings.get_key_value(url))
                    .ok_or_else(|| {
                        invalid("Every OTIO media URL needs an explicit registered source binding")
                    })?;
                used.insert(*url);
                (
                    PlaceSource::Registered {
                        asset_id: binding.asset_id.clone(),
                        stream_index: binding.stream_index,
                    },
                    Some(binding.content_sha256.clone()),
                )
            }
            Some("GeneratorReference.1")
                if kind == TrackKind::Video && reference["generator_kind"] == SOLID_COLOR =>
            {
                let [r, g, b]: [u8; 3] = commands::parse(reference["parameters"]["rgb"].clone())?;
                (
                    PlaceSource::SolidColor {
                        rgba: [
                            f64::from(r) / 255.0,
                            f64::from(g) / 255.0,
                            f64::from(b) / 255.0,
                            1.0,
                        ],
                        duration: available.map_or(source_range.end, |bounds| bounds.end),
                    },
                    None,
                )
            }
            _ => {
                return Err(unsupported(
                    "Choose mapped external media or a Numera Wiz solid color",
                ));
            }
        };
        let duration = source_range.duration()?;
        cuts.push((
            PlaceInput {
                sequence_id: input.sequence_id.clone(),
                track_id: input.track_id.clone(),
                source,
                source_range,
                sequence_start: cursor,
                collision: "reject".into(),
            },
            expected_sha256,
        ));
        cursor = cursor.checked_add(duration)?;
    }
    if cuts.is_empty() || used.len() != bindings.len() {
        return Err(invalid(
            "OTIO requires a cut and every media binding must be used",
        ));
    }
    Ok(ImportedOtio {
        kind,
        range: TimeRange { start, end: cursor },
        document_sha256: hash(input.document.as_bytes()),
        cuts,
    })
}
