//! Bounded Final Cut Pro 7 XML (xmeml) track exchange.
use crate::{
    DomainError, Result,
    media::{MixedCapture, OccurrenceSource},
    projects::{EntityId, PlaceInput, PlaceSource, SourceSpan, TrackKind, grid_tick, hash},
    time::{Rational, TimeRange},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

use super::{
    ImportedOtio, MAX_OTIO_BYTES, OtioExport, OtioImportInput, OtioSource,
    xml::{self, Element},
};

const MAX_CUTS: usize = (crate::projects::MAX_CHANGES - 2) / 2;

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("FCP7_INVALID", message)
}

fn unsupported(message: &'static str) -> DomainError {
    DomainError::new("FCP7_UNREPRESENTABLE", message)
}

fn child<'a>(node: &'a Element, name: &str) -> Result<Option<&'a Element>> {
    node.child(name)
        .map_err(|()| invalid("Duplicate xmeml element"))
}

fn required<'a>(node: &'a Element, name: &str) -> Result<&'a Element> {
    child(node, name)?.ok_or_else(|| invalid("Missing required xmeml element"))
}

fn text(node: &Element) -> Result<&str> {
    if !node.elements.is_empty() {
        return Err(invalid("Expected plain text in an xmeml element"));
    }
    Ok(node.text.trim())
}

fn ticks(node: &Element, name: &str) -> Result<i64> {
    text(required(node, name)?)?
        .parse()
        .ok()
        .filter(|ticks| *ticks >= 0)
        .ok_or_else(|| invalid("xmeml time positions require nonnegative integer frames"))
}

fn read_rate(node: &Element) -> Result<Rational> {
    let nominal = ticks(node, "timebase")?;
    if !(1..=120).contains(&nominal) {
        return Err(unsupported(
            "xmeml nominal frame rate must be from 1 to 120",
        ));
    }
    match child(node, "ntsc")?
        .map(text)
        .transpose()?
        .unwrap_or("FALSE")
    {
        "FALSE" => Rational::new(nominal, 1),
        "TRUE" => Rational::new(nominal * 1000, 1001),
        _ => Err(invalid("xmeml ntsc must be TRUE or FALSE")),
    }
}

fn at(ticks: i64, rate: Rational) -> Result<Rational> {
    Rational::new(ticks, rate.numerator())?.checked_mul(Rational::new(rate.denominator(), 1)?)
}

fn rate_xml(rate: Rational) -> Result<String> {
    let (base, ntsc) = match rate.denominator() {
        1 => (rate.numerator(), "FALSE"),
        1001 if rate.numerator() % 1000 == 0 => (rate.numerator() / 1000, "TRUE"),
        _ => {
            return Err(unsupported(
                "xmeml requires an integer or 1000/1001 frame rate",
            ));
        }
    };
    if !(1..=120).contains(&base) {
        return Err(unsupported(
            "xmeml nominal frame rate must be from 1 to 120",
        ));
    }
    Ok(format!(
        "<rate><timebase>{base}</timebase><ntsc>{ntsc}</ntsc></rate>"
    ))
}

fn frame(time: Rational, rate: Rational) -> Result<u64> {
    grid_tick(time, rate).map_err(|_| unsupported("xmeml boundaries must be on their frame grid"))
}

pub fn export(capture: &MixedCapture, track_id: &EntityId) -> Result<OtioExport> {
    capture.range.validate()?;
    let track = capture
        .tracks
        .iter()
        .find(|track| track.id == *track_id && track.gain_centidb == 0)
        .ok_or_else(|| unsupported("Select an enabled unity-gain video or audio track"))?;
    if track.clips.is_empty() || track.clips.len() > MAX_CUTS {
        return Err(unsupported("xmeml exports one track with one to 63 cuts"));
    }
    let rate = capture.settings.frame_rate;
    let rate_text = rate_xml(rate)?;
    let duration = frame(capture.range.end.checked_sub(capture.range.start)?, rate)?;
    let format = match track.kind {
        TrackKind::Video => format!(
            "<format><samplecharacteristics>{rate_text}<width>{}</width><height>{}</height><pixelaspectratio>square</pixelaspectratio></samplecharacteristics></format>",
            capture.settings.width, capture.settings.height
        ),
        TrackKind::Audio => format!(
            "<format><samplecharacteristics><samplerate>{}</samplerate></samplecharacteristics></format>",
            capture.settings.sample_rate
        ),
    };
    let kind = match track.kind {
        TrackKind::Video => "video",
        TrackKind::Audio => "audio",
    };
    let mut document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"5\"><sequence><name>Numera Wiz</name><duration>{duration}</duration>{rate_text}<media><{kind}>{format}<track>"
    );
    let mut cursor = capture.range.start;
    let mut sources = BTreeMap::new();
    for (index, clip) in track.clips.iter().enumerate() {
        let OccurrenceSource::Registered {
            asset_id,
            stream_index,
            content_sha256,
            media_kind,
            ..
        } = &clip.source
        else {
            return Err(unsupported("xmeml supports registered media cuts"));
        };
        let record = clip.sequence_range;
        let source = clip.source_range;
        if *media_kind != track.kind
            || record.start < cursor
            || record.end > capture.range.end
            || clip.rate != Rational::new(1, 1)?
            || record.end.checked_sub(record.start)? != source.end.checked_sub(source.start)?
        {
            return Err(unsupported(
                "xmeml requires ordered unit-rate cuts of the selected track kind",
            ));
        }
        let start = frame(record.start.checked_sub(capture.range.start)?, rate)?;
        let end = frame(record.end.checked_sub(capture.range.start)?, rate)?;
        let source_in = frame(source.start, rate)?;
        let source_out = frame(source.end, rate)?;
        let target_url =
            format!("file://localhost/__numera_wiz_assets__/{asset_id}/{stream_index}");
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
        write!(document,
            "<clipitem id=\"cut-{index}\"><name>Registered source</name>{rate_text}<duration>{source_out}</duration><start>{start}</start><end>{end}</end><in>{source_in}</in><out>{source_out}</out><file><name>Registered source</name><pathurl>{}</pathurl>{rate_text}</file></clipitem>",
            quick_xml::escape::escape(target_url)
        ).expect("write string");
        cursor = record.end;
    }
    writeln!(document, "</track></{kind}></media></sequence></xmeml>").expect("write string");
    if document.len() > MAX_OTIO_BYTES {
        return Err(crate::projects::limit("xmeml document exceeds 96 KiB"));
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

pub fn import(input: OtioImportInput) -> Result<ImportedOtio> {
    if input.document.len() > MAX_OTIO_BYTES || input.sources.len() > MAX_CUTS {
        return Err(crate::projects::limit(
            "xmeml exceeds the bounded document or mapping size",
        ));
    }
    let root = xml::parse(&input.document, "xmeml")
        .map_err(|()| invalid("Expected bounded UTF-8 xmeml without external declarations"))?;
    if root.name != "xmeml"
        || !matches!(
            root.attrs.get("version").map(String::as_str),
            Some("4" | "5")
        )
    {
        return Err(unsupported(
            "Choose Final Cut Pro XML (xmeml version 4 or 5)",
        ));
    }
    let sequence = match root.elements.as_slice() {
        [sequence] if sequence.name == "sequence" => sequence,
        [project] if project.name == "project" => {
            required(required(project, "children")?, "sequence")?
        }
        _ => return Err(unsupported("xmeml import requires one sequence")),
    };
    let rate = read_rate(required(sequence, "rate")?)?;
    let media = required(sequence, "media")?;
    let mut tracks = Vec::new();
    for (kind, name) in [(TrackKind::Video, "video"), (TrackKind::Audio, "audio")] {
        if let Some(container) = child(media, name)? {
            tracks.extend(
                container
                    .elements
                    .iter()
                    .filter(|node| node.name == "track")
                    .map(|track| (kind, track)),
            );
        }
    }
    let [(kind, track)] = tracks.as_slice() else {
        return Err(unsupported(
            "xmeml import needs one flat video or audio track",
        ));
    };
    if child(track, "enabled")?
        .map(text)
        .transpose()?
        .is_some_and(|value| value != "TRUE")
    {
        return Err(unsupported("Enable the xmeml track before export"));
    }
    let start = input.start.unwrap_or(Rational::ZERO);
    if start < Rational::ZERO {
        return Err(invalid("The destination start must be nonnegative"));
    }
    let end = start.checked_add(at(ticks(sequence, "duration")?, rate)?)?;
    let mut bindings = BTreeMap::new();
    for binding in &input.sources {
        crate::projects::validate_id(&binding.asset_id)?;
        if binding.target_url.is_empty()
            || binding.target_url.len() > 4096
            || binding.target_url.chars().any(char::is_control)
            || binding.content_sha256.len() != 64
            || !binding
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || bindings
                .insert(binding.target_url.as_str(), binding)
                .is_some()
        {
            return Err(invalid("Invalid or duplicate xmeml media binding"));
        }
    }
    let mut files = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut cuts = Vec::new();
    let mut cursor = start;
    for item in &track.elements {
        if matches!(item.name.as_str(), "enabled" | "locked") {
            continue;
        }
        if item.name != "clipitem"
            || item.elements.iter().any(|node| {
                matches!(
                    node.name.as_str(),
                    "filter" | "link" | "sequence" | "marker" | "pproTicksIn" | "pproTicksOut"
                )
            })
            || child(item, "enabled")?
                .map(text)
                .transpose()?
                .is_some_and(|value| value != "TRUE")
        {
            return Err(unsupported(
                "xmeml requires enabled cuts without links, effects, markers or subframe edits",
            ));
        }
        if cuts.len() == MAX_CUTS {
            return Err(crate::projects::limit(
                "xmeml exceeds the atomic edit limit",
            ));
        }
        let record_start = start.checked_add(at(ticks(item, "start")?, rate)?)?;
        let record_end = start.checked_add(at(ticks(item, "end")?, rate)?)?;
        let file = required(item, "file")?;
        let id = file.attrs.get("id").map(String::as_str);
        let url = if let Some(url) = child(file, "pathurl")? {
            let url = text(url)?;
            if let Some(id) = id
                && files.insert(id, url).is_some_and(|old| old != url)
            {
                return Err(invalid("Conflicting xmeml file references"));
            }
            url
        } else {
            id.and_then(|id| files.get(id).copied())
                .ok_or_else(|| invalid("xmeml file reference has no previously declared pathurl"))?
        };
        let binding = bindings.get(url).ok_or_else(|| {
            invalid("Every xmeml pathurl requires an explicit registered media binding")
        })?;
        used.insert(url);
        let source_rate = child(item, "rate")?
            .or(child(file, "rate")?)
            .map(read_rate)
            .transpose()?
            .unwrap_or(rate);
        let source_range = SourceSpan {
            start: at(ticks(item, "in")?, source_rate)?,
            end: at(ticks(item, "out")?, source_rate)?,
        };
        if record_start < cursor
            || record_end <= record_start
            || record_end > end
            || source_range.duration()? != record_end.checked_sub(record_start)?
        {
            return Err(unsupported(
                "xmeml requires ordered unit-rate cuts inside the sequence duration",
            ));
        }
        cuts.push((
            PlaceInput {
                sequence_id: input.sequence_id.clone(),
                track_id: input.track_id.clone(),
                source: PlaceSource::Registered {
                    asset_id: binding.asset_id.clone(),
                    stream_index: binding.stream_index,
                },
                source_range,
                sequence_start: record_start,
                collision: "reject".into(),
            },
            Some(binding.content_sha256.clone()),
        ));
        cursor = record_end;
    }
    if cuts.is_empty() || used.len() != bindings.len() {
        return Err(invalid(
            "xmeml requires a cut and every media binding must be used",
        ));
    }
    Ok(ImportedOtio {
        kind: *kind,
        range: TimeRange { start, end },
        document_sha256: hash(input.document.as_bytes()),
        cuts,
    })
}
