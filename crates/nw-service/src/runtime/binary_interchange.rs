//! Linked-media AAF and OMF 2.0 single-track cut interchange.
use craft_aaf::{Identity, Report, aaf, model, omf};
use craft_timeline::time::{FrameRate, TICKS_PER_SECOND, Tick};
use nw_domain::{
    DomainError, Result,
    interchange::{ImportedOtio, MAX_BINARY_BYTES, MAX_BINARY_IMPORT_BYTES, OtioSource},
    media::{CapturedOccurrence, MixedCapture, OccurrenceSource, bind_mixed_source, declared_relative_stream_window},
    mux::StreamKind,
    projects::{self, EntityId, PlaceInput, PlaceSource, SourceSpan, TrackKind, grid_tick, hash},
    records::Asset,
    time::{Rational, TimeRange},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

mod bento;
mod preflight;

const MAX_CUTS: usize = (projects::MAX_CHANGES - 2) / 2;
const MAX_ITEMS: usize = MAX_CUTS * 2 + 1;
const IDENTITY: Identity<'static> = Identity {
    company: "Numera Wiz",
    product: "Numera Wiz",
    version: env!("CARGO_PKG_VERSION"),
    platform: "Numera Wiz",
    product_id: [0x17, 0xb1, 0xc7, 0xac, 0x16, 0x16, 0x48, 0xa2, 0xae, 0xf0, 0x59, 0x20, 0xb1, 0xc4, 0x3e, 0x15],
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Aaf,
    Omf,
}

impl Format {
    fn invalid(self, message: impl Into<String>) -> DomainError {
        let code = match self {
            Self::Aaf => "AAF_INVALID",
            Self::Omf => "OMF_INVALID",
        };
        DomainError::new(code, message)
    }

    fn unsupported(self, message: impl Into<String>) -> DomainError {
        let code = match self {
            Self::Aaf => "AAF_UNREPRESENTABLE",
            Self::Omf => "OMF_UNREPRESENTABLE",
        };
        DomainError::new(code, message)
    }
}

pub(crate) struct BinaryExport {
    pub document: Vec<u8>,
    pub document_sha256: String,
    pub sources: Vec<OtioSource>,
}

fn exact_tick(format: Format, time: Rational) -> Result<Tick> {
    let ticks = grid_tick(time, Rational::new(TICKS_PER_SECOND, 1)?)
        .map_err(|_| format.unsupported("Time exceeds the exchange clock's exact grid"))?;
    i64::try_from(ticks)
        .ok()
        .filter(|ticks| *ticks < model::MAX_TIME.0)
        .map(Tick)
        .ok_or_else(|| format.unsupported("Time exceeds the exchange clock's admitted extent"))
}

fn exact_time(format: Format, ticks: Tick) -> Result<Rational> {
    if !(0..model::MAX_TIME.0).contains(&ticks.0) {
        return Err(format.unsupported("Time exceeds the exchange clock's admitted extent"));
    }
    Rational::new(ticks.0, TICKS_PER_SECOND)
}

fn check_notes(format: Format, report: &Report) -> Result<()> {
    if let Some(note) = report.entries.first() {
        return Err(format.unsupported(format!("Document requires a lossy conversion: {}", note.message)));
    }
    Ok(())
}

fn validate_bindings(format: Format, sources: &[OtioSource]) -> Result<BTreeMap<&str, &OtioSource>> {
    if sources.len() > MAX_CUTS {
        return Err(projects::limit("Too many registered media bindings"));
    }
    let mut bindings = BTreeMap::new();
    for source in sources {
        if source.target_url.is_empty()
            || source.target_url.len() > 4096
            || source.target_url.chars().any(char::is_control)
            || source.content_sha256.len() != 64
            || !source.content_sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format.invalid("Invalid media locator or SHA-256 binding"));
        }
        projects::validate_id(&source.asset_id)?;
        if bindings.insert(source.target_url.as_str(), source).is_some() {
            return Err(format.invalid("Duplicate media locator binding"));
        }
    }
    Ok(bindings)
}

pub(crate) fn import(
    format: Format,
    bytes: &[u8],
    sequence_id: &EntityId,
    track_id: &EntityId,
    start: Option<Rational>,
    sources: &[OtioSource],
) -> Result<ImportedOtio> {
    if bytes.len() > MAX_BINARY_IMPORT_BYTES {
        return Err(projects::limit("Binary interchange exceeds the bounded input size"));
    }
    let bindings = validate_bindings(format, sources)?;
    let start = start.unwrap_or(Rational::ZERO);
    if start < Rational::ZERO {
        return Err(format.invalid("The destination start must be nonnegative"));
    }
    let mut report = Report::default();
    let document = match format {
        Format::Aaf => {
            preflight::linked_aaf(bytes)?;
            aaf::read(bytes, &mut report)
        }
        Format::Omf => omf::read(&bento::linked_codec_input(bytes)?, &mut report),
    }
    .map_err(|_| format.invalid("Invalid or unsupported binary interchange container"))?;
    check_notes(format, &report)?;
    let [composition] = document.compositions.as_slice() else {
        return Err(format.unsupported("Import requires one top-level composition"));
    };
    let [track] = composition.tracks.as_slice() else {
        return Err(format.unsupported("Import requires one editorial track"));
    };
    if !composition.markers.is_empty() || !document.nested.is_empty() {
        return Err(format.unsupported("Markers and nested compositions are outside this cut list"));
    }
    if track.items.len() > MAX_ITEMS {
        return Err(projects::limit("Binary interchange exceeds the bounded cut and gap count"));
    }
    let kind = match track.kind {
        model::CKind::Picture => TrackKind::Video,
        model::CKind::Sound => TrackKind::Audio,
    };
    let mut used = BTreeSet::new();
    let mut cuts = Vec::new();
    let mut cursor = start;
    for item in &track.items {
        let duration = exact_time(format, item.len())?;
        match item {
            model::CItem::Filler(_) if duration == Rational::ZERO => continue,
            model::CItem::Filler(_) => {}
            model::CItem::Clip(clip) if duration > Rational::ZERO && clip.gain.is_none() => {
                if cuts.len() == MAX_CUTS {
                    return Err(projects::limit("Binary interchange exceeds the atomic edit limit"));
                }
                let source = document.sources.get(clip.source)
                    .ok_or_else(|| format.invalid("Cut references a missing source"))?;
                if format == Format::Omf && source.sample_rate != composition.sample_rate {
                    return Err(format.unsupported("OMF requires matching source and track sample rates"));
                }
                let source_range = linked_source_range(format, track.kind, source, clip, duration)?;
                let (locator, binding) = source.path.as_deref()
                    .and_then(|path| bindings.get_key_value(path))
                    .ok_or_else(|| format.invalid("Every linked media locator needs an explicit registered source binding"))?;
                used.insert(*locator);
                cuts.push((PlaceInput {
                    sequence_id: sequence_id.clone(),
                    track_id: track_id.clone(),
                    source: PlaceSource::Registered { asset_id: binding.asset_id.clone(), stream_index: binding.stream_index },
                    source_range,
                    sequence_start: cursor,
                    collision: "reject".into(),
                }, Some(binding.content_sha256.clone())));
            }
            _ => return Err(format.unsupported("Import requires plain positive-duration cuts and gaps")),
        }
        cursor = cursor.checked_add(duration)?;
    }
    if cuts.is_empty() || used.len() != bindings.len() {
        return Err(format.invalid("Import requires at least one cut and exactly the used media bindings"));
    }
    Ok(ImportedOtio {
        kind,
        range: TimeRange { start, end: cursor },
        document_sha256: hash(bytes),
        cuts,
    })
}

fn linked_source_range(
    format: Format,
    kind: model::CKind,
    source: &model::Source,
    clip: &model::CClip,
    duration: Rational,
) -> Result<SourceSpan> {
    if source.kind != kind
        || source.channel.is_some()
        || source.embedded.is_some()
        || source.nested.is_some()
        || source.offset != Tick::ZERO
        || !source.markers.is_empty()
    {
        return Err(format.unsupported("Import requires an unmodified, linked full-stream source"));
    }
    let start = exact_time(format, clip.start)?;
    let end = start.checked_add(duration)?;
    if source.length > Tick::ZERO && end > exact_time(format, source.length)? {
        return Err(format.invalid("Cut exceeds the declared linked source extent"));
    }
    Ok(SourceSpan { start, end })
}

fn number(metadata: &Value, field: &str) -> Option<u32> {
    metadata[field].as_u64().and_then(|value| u32::try_from(value).ok())
        .or_else(|| metadata[field].as_str()?.parse().ok())
}

fn frame_rate(format: Format, rate: Rational) -> Result<FrameRate> {
    if rate <= Rational::ZERO
        || rate > Rational::new(120, 1)?
        || rate.numerator() > i64::from(i32::MAX)
        || rate.denominator() > i64::from(i32::MAX)
    {
        return Err(format.unsupported("Picture edit rate exceeds the admitted exchange grid"));
    }
    exact_tick(format, Rational::new(rate.denominator(), rate.numerator())?)?;
    Ok(FrameRate::new(rate.numerator(), rate.denominator()))
}

fn picture_rate(format: Format, metadata: &Value) -> Result<FrameRate> {
    let rate = |key| -> Option<Rational> {
        let (num, den) = metadata[key].as_str()?.split_once('/')?;
        Rational::new(num.parse().ok()?, den.parse().ok()?).ok()
    };
    let confirmed = rate("avg_frame_rate")
        .filter(|value| Some(*value) == rate("r_frame_rate"))
        .ok_or_else(|| format.unsupported("Picture export requires matching registered frame rate observations"))?;
    frame_rate(format, confirmed)
}

fn pcm_bits(codec: &str, container: &str) -> Option<u16> {
    match (container, codec) {
        ("wav", "pcm_u8") | ("aiff", "pcm_s8") => Some(8),
        ("wav", "pcm_s16le") | ("aiff", "pcm_s16be") => Some(16),
        ("wav", "pcm_s24le") | ("aiff", "pcm_s24be") => Some(24),
        ("wav", "pcm_s32le") | ("aiff", "pcm_s32be") => Some(32),
        _ => None,
    }
}

fn clip_locator(format: Format, asset: &Asset) -> Result<String> {
    let name = asset.identity.path.rsplit(['/', '\\']).next().unwrap_or_default();
    if name.is_empty() || name.len() > 255 || name == "." || name == ".."
        || name.chars().any(char::is_control)
        || name.contains([':', '?', '*', '"', '<', '>', '|'])
    {
        return Err(format.unsupported("Linked media requires a portable file name"));
    }
    Ok(name.into())
}

fn linked_asset(
    format: Format,
    occurrence: &CapturedOccurrence,
    asset: &Asset,
    binding: &OtioSource,
    sequence_rate: FrameRate,
) -> Result<model::Source> {
    let OccurrenceSource::Registered { media_kind, probe_sha256, .. } = &occurrence.source else {
        return Err(format.unsupported("Binary interchange exports registered media cuts"));
    };
    let probe = serde_json::to_vec(&asset.probe)
        .map_err(|_| format.invalid("Could not verify the registered source identity"))?;
    if asset.id != binding.asset_id || asset.identity.sha256 != binding.content_sha256 || hash(&probe) != *probe_sha256 {
        return Err(DomainError::new("SOURCE_CHANGED", "Registered asset or probe differs from the captured binding"));
    }
    let stream_kind = match media_kind {
        TrackKind::Video => StreamKind::Video,
        TrackKind::Audio => StreamKind::Audio,
    };
    let mut streams = asset.probe.streams.iter().filter(|stream| stream.kind == stream_kind && !stream.attached_picture());
    let stream = streams.next().filter(|stream| stream.index == binding.stream_index)
        .ok_or_else(|| format.invalid("Linked source stream differs from the registered probe"))?;
    if streams.next().is_some() {
        return Err(format.unsupported("Linked export requires one stream of the selected media kind per file"));
    }
    let metadata = asset.probe.stream_details.get(&binding.stream_index)
        .ok_or_else(|| format.unsupported("Registered stream has no descriptor metadata"))?;
    let mut source = model::Source {
        key: format!("{}:{}", binding.asset_id, binding.stream_index),
        name: binding.target_url.clone(),
        kind: model::CKind::Picture,
        path: Some(binding.target_url.clone()),
        channel: None,
        channels: 0,
        file_channels: 0,
        width: 0,
        height: 0,
        frame_rate: sequence_rate,
        sample_rate: 0,
        bits: 0,
        length: Tick::ZERO,
        offset: Tick::ZERO,
        start_tc: None,
        tc_rate: sequence_rate,
        embedded: None,
        markers: Vec::new(),
        nested: None,
    };
    let declared_end = if *media_kind == TrackKind::Video {
        let bound = bind_mixed_source(occurrence, asset, hash(&serde_json::to_vec(asset)
            .map_err(|_| format.invalid("Could not verify the registered source identity"))?))?;
        if bound.stream_start != bound.asset_origin {
            return Err(format.unsupported("Linked export requires a source starting at the asset origin"));
        }
        source.frame_rate = picture_rate(format, metadata)?;
        source.width = number(metadata, "width").filter(|value| *value > 0)
            .ok_or_else(|| format.unsupported("Registered picture width is unavailable"))?;
        source.height = number(metadata, "height").filter(|value| *value > 0)
            .ok_or_else(|| format.unsupported("Registered picture height is unavailable"))?;
        if asset.probe.is_still_image() {
            return Err(format.unsupported("Linked picture export requires moving media"));
        }
        declared_relative_stream_window(asset, binding.stream_index)?.map(|range| range.end)
    } else {
        source.kind = model::CKind::Sound;
        source.bits = pcm_bits(&stream.codec, &asset.probe.format)
            .ok_or_else(|| format.unsupported("Linked sound export requires registered integer PCM WAVE or AIFF"))?;
        let aiff_name = binding.target_url.rsplit_once('.').is_some_and(|(_, extension)| {
            ["aif", "aiff", "aifc"].iter().any(|candidate| extension.eq_ignore_ascii_case(candidate))
        });
        if (asset.probe.format == "aiff") != aiff_name {
            return Err(format.unsupported("The linked sound file name must match its PCM container"));
        }
        source.channels = number(metadata, "channels").filter(|value| (1..=2).contains(value))
            .ok_or_else(|| format.unsupported("Linked sound export requires mono or stereo PCM"))?;
        source.file_channels = source.channels;
        source.sample_rate = number(metadata, "sample_rate").filter(|value| (8_000..=384_000).contains(value))
            .ok_or_else(|| format.unsupported("Registered sound sample rate is unavailable"))?;
        pcm_extent(format, asset, metadata, source.sample_rate)?
    };
    let source_grid = match source.kind {
        model::CKind::Picture => Rational::new(source.frame_rate.num, source.frame_rate.den)?,
        model::CKind::Sound => Rational::new(source.sample_rate.into(), 1)?,
    };
    if declared_end.is_some_and(|end| end < occurrence.source_range.end) {
        return Err(format.invalid("Cut exceeds the registered source extent"));
    }
    let declared_end = declared_end.or(asset.probe.container_duration);
    if let Some(end) = declared_end.filter(|end| *end >= occurrence.source_range.end)
        && grid_tick(end, source_grid).is_ok()
        && let Ok(length) = exact_tick(format, end)
    {
        source.length = length;
    }
    Ok(source)
}

fn pcm_extent(format: Format, asset: &Asset, metadata: &Value, rate: u32) -> Result<Option<Rational>> {
    let integer = |name: &str| metadata[name].as_i64().or_else(|| metadata[name].as_str()?.parse().ok());
    // WAVE and AIFF are single-stream sample files. FFprobe can omit the start
    // PTS because the first sample is implicitly zero.
    if asset.probe.streams.len() != 1 || integer("start_pts").is_some_and(|start| start != 0) {
        return Err(format.unsupported("Linked PCM requires a single stream starting at sample zero"));
    }
    let base = metadata["time_base"].as_str().and_then(|base| base.split_once('/'));
    let Some(("1", denominator)) = base else {
        return Err(format.unsupported("Registered PCM requires an exact sample clock"));
    };
    if denominator.parse::<u32>().ok() != Some(rate) {
        return Err(format.unsupported("Registered PCM time base differs from its sample rate"));
    }
    integer("duration_ts").filter(|duration| *duration > 0)
        .map(|duration| Rational::new(duration, i64::from(rate))).transpose()
}

pub(crate) fn export(
    format: Format,
    capture: &MixedCapture,
    track_id: &EntityId,
    assets: &BTreeMap<String, Asset>,
) -> Result<BinaryExport> {
    capture.range.validate()?;
    let track = capture.tracks.iter()
        .find(|track| track.id == *track_id && track.gain_centidb == 0)
        .ok_or_else(|| format.unsupported("Select an enabled unity-gain track"))?;
    if track.clips.is_empty() || track.clips.len() > MAX_CUTS {
        return Err(format.unsupported("Export requires one to 63 registered media cuts"));
    }
    let kind = match (format, track.kind) {
        (Format::Omf, TrackKind::Video) => return Err(format.unsupported("OMF exports an audio track")),
        (_, TrackKind::Video) => model::CKind::Picture,
        (_, TrackKind::Audio) => model::CKind::Sound,
    };
    let rate = frame_rate(format, capture.settings.frame_rate)?;
    let track_grid = match kind {
        model::CKind::Picture => capture.settings.frame_rate,
        model::CKind::Sound => Rational::new(capture.settings.sample_rate.into(), 1)?,
    };
    let mut document = model::Document::default();
    let mut bindings = BTreeMap::<String, (OtioSource, usize)>::new();
    let mut items = Vec::new();
    let mut cursor = capture.range.start;
    let unity = Rational::new(1, 1)?;
    for cut in &track.clips {
        let range = cut.sequence_range;
        if range.start < cursor || range.end > capture.range.end || range.end <= range.start
            || cut.rate != unity || cut.source_range.end.checked_sub(cut.source_range.start)? != range.end.checked_sub(range.start)?
        {
            return Err(format.unsupported("Export requires ordered unit-rate cuts inside the selected range"));
        }
        if range.start > cursor {
            items.push(model::CItem::Filler(exact_tick(format, range.start.checked_sub(cursor)?)?));
        }
        let OccurrenceSource::Registered { asset_id, stream_index, content_sha256, media_kind, .. } = &cut.source else {
            return Err(format.unsupported("Binary interchange exports registered media cuts"));
        };
        if *media_kind != track.kind {
            return Err(format.invalid("Cut media kind differs from the selected track"));
        }
        let asset = assets.get(asset_id).ok_or_else(DomainError::object_not_found)?;
        let target_url = clip_locator(format, asset)?;
        let key = target_url.to_lowercase();
        let binding = OtioSource { target_url, asset_id: asset_id.clone(), stream_index: *stream_index, content_sha256: content_sha256.clone() };
        let index = match bindings.get(&key) {
            Some((prior, index)) if *prior == binding => *index,
            Some(_) => return Err(format.unsupported("Different sources need distinct portable linked file names")),
            None => {
                let index = document.sources.len();
                document.sources.push(linked_asset(format, cut, asset, &binding, rate)?);
                bindings.insert(key, (binding, index));
                index
            }
        };
        let source = &mut document.sources[index];
        if format == Format::Omf && source.sample_rate != capture.settings.sample_rate {
            return Err(format.unsupported("OMF requires matching source and track sample rates"));
        }
        let source_grid = match kind {
            model::CKind::Picture => Rational::new(source.frame_rate.num, source.frame_rate.den)?,
            model::CKind::Sound => Rational::new(source.sample_rate.into(), 1)?,
        };
        for point in [cut.source_range.start, cut.source_range.end] {
            grid_tick(point, source_grid)?;
        }
        for point in [range.start, range.end] {
            grid_tick(point.checked_sub(capture.range.start)?, track_grid)?;
        }
        source.length = source.length.max(exact_tick(format, cut.source_range.end)?);
        items.push(model::CItem::Clip(model::CClip {
            len: exact_tick(format, range.end.checked_sub(range.start)?)?,
            source: index,
            start: exact_tick(format, cut.source_range.start)?,
            gain: None,
            name: String::new(),
        }));
        cursor = range.end;
    }
    if cursor < capture.range.end {
        items.push(model::CItem::Filler(exact_tick(format, capture.range.end.checked_sub(cursor)?)?));
    }
    let channels = document.sources.iter().map(|source| source.channels).max().unwrap_or(0);
    document.compositions.push(model::Composition {
        name: "Numera Wiz".into(),
        aaf_mob_id: None,
        rate,
        sample_rate: capture.settings.sample_rate,
        width: capture.settings.width,
        height: capture.settings.height,
        start_tc: 0,
        drop: false,
        tracks: vec![model::CTrack {
            kind,
            name: match kind { model::CKind::Picture => "V1", model::CKind::Sound => "A1" }.into(),
            number: 1,
            channels,
            items,
        }],
        markers: Vec::new(),
    });
    let mut report = Report::default();
    let mut bytes = match format {
        Format::Aaf => aaf::write(&document, &aaf::WriteOptions { identity: IDENTITY, small_sectors: true }, &mut report),
        Format::Omf => omf::write(&document, &IDENTITY, &mut report),
    }.map_err(|_| format.unsupported("Could not serialize the linked cut list"))?;
    check_notes(format, &report)?;
    if format == Format::Omf {
        bytes = bento::standard_output(&bytes)?;
    }
    if bytes.len() > MAX_BINARY_BYTES {
        return Err(projects::limit("Binary interchange exceeds the bounded output size"));
    }
    Ok(BinaryExport { document_sha256: hash(&bytes), document: bytes, sources: bindings.into_values().map(|(binding, _)| binding).collect() })
}
