//! Final Cut Pro XML document exchange for one flat editorial track.
use crate::{
    DomainError, Result,
    media::{MixedCapture, OccurrenceSource},
    projects::{EntityId, PlaceInput, PlaceSource, SourceSpan, TrackKind, hash},
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
    DomainError::new("FCPXML_INVALID", message)
}

fn unsupported(message: &'static str) -> DomainError {
    DomainError::new("FCPXML_UNREPRESENTABLE", message)
}

fn attr<'a>(node: &'a Element, name: &str) -> Option<&'a str> {
    node.attrs.get(name).map(String::as_str)
}

fn required_attr<'a>(node: &'a Element, name: &str) -> Result<&'a str> {
    attr(node, name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("Missing required FCPXML attribute"))
}

fn child<'a>(node: &'a Element, name: &str) -> Result<Option<&'a Element>> {
    node.child(name)
        .map_err(|()| invalid("Duplicate FCPXML element"))
}

fn required<'a>(node: &'a Element, name: &str) -> Result<&'a Element> {
    child(node, name)?.ok_or_else(|| invalid("Missing required FCPXML element"))
}

fn time(value: &str) -> Result<Rational> {
    let value = value
        .strip_suffix('s')
        .ok_or_else(|| invalid("FCPXML times require a seconds suffix"))?;
    let (numerator, denominator) = value.split_once('/').unwrap_or((value, "1"));
    let numerator = numerator
        .parse::<i64>()
        .map_err(|_| invalid("FCPXML time numerator must fit in a signed 64-bit integer"))?;
    let denominator = denominator
        .parse::<i32>()
        .map_err(|_| invalid("FCPXML time denominator must fit in a positive 32-bit integer"))?;
    Rational::new(numerator, i64::from(denominator))
}

fn time_attr(node: &Element, name: &str) -> Result<Option<Rational>> {
    attr(node, name).map(time).transpose()
}

fn seconds(value: Rational) -> Result<String> {
    let numerator = value.numerator();
    let denominator = value.denominator();
    if denominator > i64::from(i32::MAX) {
        return Err(unsupported("Time exceeds the FCPXML denominator limit"));
    }
    if denominator == 1 {
        Ok(format!("{numerator}s"))
    } else {
        Ok(format!("{numerator}/{denominator}s"))
    }
}

fn append_gap(story: &mut String, start: Rational, end: Rational) -> Result<()> {
    if end > start {
        let offset = seconds(start)?;
        let duration = seconds(end.checked_sub(start)?)?;
        write!(
            story,
            "<gap name=\"Gap\" offset=\"{offset}\" start=\"0s\" duration=\"{duration}\"/>"
        )
        .expect("write string");
    }
    Ok(())
}

pub fn export(capture: &MixedCapture, track_id: &EntityId) -> Result<OtioExport> {
    capture.range.validate()?;
    let track = capture
        .tracks
        .iter()
        .find(|track| track.id == *track_id && track.gain_centidb == 0)
        .ok_or_else(|| unsupported("Select an enabled unity-gain video or audio track"))?;
    if track.clips.is_empty() || track.clips.len() > MAX_CUTS {
        return Err(unsupported("FCPXML exports one track with one to 63 cuts"));
    }
    let rate = capture.settings.frame_rate;
    let frame_duration = seconds(Rational::new(rate.denominator(), rate.numerator())?)?;
    let duration = capture.range.end.checked_sub(capture.range.start)?;
    let (enabled, video, audio) = match track.kind {
        TrackKind::Video => ("video", 1, 0),
        TrackKind::Audio => ("audio", 0, 1),
    };
    let mut story = String::new();
    let mut cursor = Rational::ZERO;
    let mut resources = BTreeMap::<String, (usize, Rational, OtioSource)>::new();
    for clip in &track.clips {
        let OccurrenceSource::Registered {
            asset_id,
            stream_index,
            content_sha256,
            media_kind,
            ..
        } = &clip.source
        else {
            return Err(unsupported("FCPXML supports registered media cuts"));
        };
        let record = clip.sequence_range;
        let source = clip.source_range;
        let start = record.start.checked_sub(capture.range.start)?;
        let end = record.end.checked_sub(capture.range.start)?;
        let length = end.checked_sub(start)?;
        if *media_kind != track.kind
            || start < cursor
            || end > duration
            || length <= Rational::ZERO
            || source.start < Rational::ZERO
            || clip.rate != Rational::new(1, 1)?
            || length != source.end.checked_sub(source.start)?
        {
            return Err(unsupported(
                "FCPXML requires ordered unit-rate media cuts inside the range",
            ));
        }
        append_gap(&mut story, cursor, start)?;
        let url = format!("file://localhost/__numera_wiz_assets__/{asset_id}/{stream_index}");
        let binding = OtioSource {
            target_url: url.clone(),
            asset_id: asset_id.clone(),
            stream_index: *stream_index,
            content_sha256: content_sha256.clone(),
        };
        let next = resources.len() + 2;
        let (id, asset_end, identity) = resources
            .entry(url)
            .or_insert_with(|| (next, source.end, binding.clone()));
        if *identity != binding {
            return Err(unsupported(
                "Cuts disagree on the registered media identity",
            ));
        }
        *asset_end = (*asset_end).max(source.end);
        write!(
            story,
            "<asset-clip name=\"Registered source\" ref=\"r{id}\" offset=\"{}\" start=\"{}\" duration=\"{}\" srcEnable=\"{enabled}\"/>",
            seconds(start)?,
            seconds(source.start)?,
            seconds(length)?,
        )
        .expect("write string");
        cursor = end;
    }
    append_gap(&mut story, cursor, duration)?;
    let mut document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.10\"><resources><format id=\"r1\" frameDuration=\"{frame_duration}\" width=\"{}\" height=\"{}\"/>",
        capture.settings.width, capture.settings.height,
    );
    let mut sources = Vec::with_capacity(resources.len());
    for (id, asset_end, identity) in resources.into_values() {
        write!(
            document,
            "<asset id=\"r{id}\" name=\"Registered source\" start=\"0s\" duration=\"{}\" hasVideo=\"{video}\" hasAudio=\"{audio}\"><media-rep kind=\"original-media\" src=\"{}\"/></asset>",
            seconds(asset_end)?,
            quick_xml::escape::escape(identity.target_url.as_str()),
        )
        .expect("write string");
        sources.push(identity);
    }
    writeln!(
        document,
        "</resources><project name=\"Numera Wiz\"><sequence format=\"r1\" tcStart=\"0s\" duration=\"{}\"><spine>{story}</spine></sequence></project></fcpxml>",
        seconds(duration)?,
    )
    .expect("write string");
    if document.len() > MAX_OTIO_BYTES {
        return Err(crate::projects::limit("FCPXML document exceeds 96 KiB"));
    }
    Ok(OtioExport {
        project_id: capture.project_id.clone(),
        sequence_id: capture.sequence_id.clone(),
        track_id: track_id.clone(),
        revision: capture.project_revision,
        range: capture.range,
        document_sha256: hash(document.as_bytes()),
        document,
        sources,
    })
}

fn find_project<'a>(container: &'a Element, project: &mut Option<&'a Element>) -> Result<()> {
    for node in &container.elements {
        match (container.name.as_str(), node.name.as_str()) {
            ("fcpxml" | "event", "project") => {
                if project.replace(node).is_some() {
                    return Err(unsupported("FCPXML import requires one project"));
                }
            }
            ("fcpxml", "library" | "event") | ("library", "event") => find_project(node, project)?,
            _ => {}
        }
    }
    Ok(())
}

fn media_flag(asset: &Element, name: &str) -> Result<bool> {
    match attr(asset, name).unwrap_or("0") {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(invalid("FCPXML media flags must be 0 or 1")),
    }
}

fn clip_kind(asset: &Element, clip: &Element) -> Result<TrackKind> {
    let video = media_flag(asset, "hasVideo")?;
    let audio = media_flag(asset, "hasAudio")?;
    match (attr(clip, "srcEnable").unwrap_or("all"), video, audio) {
        ("video", true, _) | ("all", true, false) => Ok(TrackKind::Video),
        ("audio", _, true) | ("all", false, true) => Ok(TrackKind::Audio),
        _ => Err(unsupported(
            "Select a single video or audio component in FCPXML",
        )),
    }
}

fn original_url(asset: &Element) -> Result<&str> {
    let mut locations = asset
        .elements
        .iter()
        .filter(|node| node.name == "media-rep" && attr(node, "kind") == Some("original-media"));
    match (locations.next(), locations.next(), attr(asset, "src")) {
        (Some(location), None, None) => required_attr(location, "src"),
        (None, None, Some(source)) => Ok(source),
        _ => Err(invalid(
            "A referenced FCPXML asset needs one original-media source",
        )),
    }
}

pub fn import(input: OtioImportInput) -> Result<ImportedOtio> {
    if input.document.len() > MAX_OTIO_BYTES || input.sources.len() > MAX_CUTS {
        return Err(crate::projects::limit(
            "FCPXML exceeds the bounded document or mapping size",
        ));
    }
    let root = xml::parse(&input.document, "fcpxml")
        .map_err(|()| invalid("Expected bounded UTF-8 FCPXML without external declarations"))?;
    let version = required_attr(&root, "version")?;
    if root.name != "fcpxml"
        || !version
            .strip_prefix("1.")
            .and_then(|minor| minor.parse::<u8>().ok())
            .is_some_and(|minor| (8..=14).contains(&minor))
    {
        return Err(unsupported("Choose an FCPXML version from 1.8 to 1.14"));
    }
    let mut project = None;
    find_project(&root, &mut project)?;
    let sequence = required(
        project.ok_or_else(|| invalid("FCPXML requires a project"))?,
        "sequence",
    )?;
    let spine = required(sequence, "spine")?;
    if attr(spine, "offset").is_some() || attr(spine, "lane").is_some() {
        return Err(unsupported(
            "FCPXML import requires a primary story with no connected lanes",
        ));
    }
    let resources = required(&root, "resources")?;
    let mut resource_ids = BTreeMap::new();
    for node in &resources.elements {
        let id = required_attr(node, "id")?;
        if resource_ids.insert(id, node).is_some() {
            return Err(invalid("Duplicate FCPXML resource identifier"));
        }
    }
    if let Some(id) = attr(sequence, "format") {
        let format = resource_ids
            .get(id)
            .filter(|format| format.name == "format")
            .ok_or_else(|| invalid("FCPXML sequence format resource is missing"))?;
        if time_attr(format, "frameDuration")?.is_some_and(|time| time <= Rational::ZERO) {
            return Err(invalid("FCPXML frame duration must be positive"));
        }
    }

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
            return Err(invalid("Invalid or duplicate FCPXML media binding"));
        }
    }
    let base = input.start.unwrap_or(Rational::ZERO);
    if base < Rational::ZERO {
        return Err(invalid("The destination start must be nonnegative"));
    }
    let origin = time_attr(sequence, "tcStart")?.unwrap_or(Rational::ZERO);
    let duration = time_attr(sequence, "duration")?;
    if duration.is_some_and(|duration| duration <= Rational::ZERO) {
        return Err(invalid("The FCPXML sequence duration must be positive"));
    }
    let mut cursor = Rational::ZERO;
    let mut cuts = Vec::new();
    let mut kind = None;
    let mut used = BTreeSet::new();
    for clip in &spine.elements {
        if !matches!(clip.name.as_str(), "gap" | "asset-clip")
            || !clip.elements.is_empty()
            || attr(clip, "lane").is_some_and(|lane| lane != "0")
            || attr(clip, "enabled").is_some_and(|enabled| enabled != "1")
            || attr(clip, "audioStart").is_some()
            || attr(clip, "audioDuration").is_some()
        {
            return Err(unsupported(
                "FCPXML supports one enabled primary story with cuts and gaps",
            ));
        }
        let start = match time_attr(clip, "offset")? {
            Some(offset) => offset.checked_sub(origin)?,
            None => cursor,
        };
        let length = time(required_attr(clip, "duration")?)?;
        let end = start.checked_add(length)?;
        if start < cursor || length <= Rational::ZERO || duration.is_some_and(|limit| end > limit) {
            return Err(unsupported(
                "FCPXML requires ordered positive cuts and gaps within duration",
            ));
        }
        cursor = end;
        if clip.name == "gap" {
            continue;
        }
        if cuts.len() == MAX_CUTS {
            return Err(crate::projects::limit(
                "FCPXML exceeds the atomic edit limit",
            ));
        }
        let asset = resource_ids
            .get(required_attr(clip, "ref")?)
            .filter(|asset| asset.name == "asset")
            .ok_or_else(|| invalid("FCPXML clip refers to an undeclared asset"))?;
        let this_kind = clip_kind(asset, clip)?;
        if kind
            .replace(this_kind)
            .is_some_and(|previous| previous != this_kind)
        {
            return Err(unsupported(
                "Place one video or audio track per FCPXML import",
            ));
        }
        let asset_origin = time_attr(asset, "start")?.unwrap_or(Rational::ZERO);
        let source_start = time_attr(clip, "start")?
            .unwrap_or(asset_origin)
            .checked_sub(asset_origin)?;
        let source_end = source_start.checked_add(length)?;
        if source_start < Rational::ZERO
            || time_attr(asset, "duration")?.is_some_and(|limit| source_end > limit)
        {
            return Err(invalid("FCPXML cut lies outside the declared asset range"));
        }
        let url = original_url(asset)?;
        let binding = bindings.get(url).ok_or_else(|| {
            invalid("Every FCPXML original-media URL requires a registered binding")
        })?;
        used.insert(url);
        cuts.push((
            PlaceInput {
                sequence_id: input.sequence_id.clone(),
                track_id: input.track_id.clone(),
                source: PlaceSource::Registered {
                    asset_id: binding.asset_id.clone(),
                    stream_index: binding.stream_index,
                },
                source_range: SourceSpan {
                    start: source_start,
                    end: source_end,
                },
                sequence_start: base.checked_add(start)?,
                collision: "reject".into(),
            },
            Some(binding.content_sha256.clone()),
        ));
    }
    let kind = kind.ok_or_else(|| invalid("FCPXML requires a media cut"))?;
    if used.len() != bindings.len() {
        return Err(invalid("Every FCPXML binding must be used"));
    }
    Ok(ImportedOtio {
        kind,
        range: TimeRange {
            start: base,
            end: base.checked_add(duration.unwrap_or(cursor))?,
        },
        document_sha256: hash(input.document.as_bytes()),
        cuts,
    })
}
