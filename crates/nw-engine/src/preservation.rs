//! Full packet reads use bounded record framing and per-stream incremental digests.
use crate::{Engine, ExecutionEvent, Observer, admission};
use nw_domain::{
    DomainError, Result,
    records::{MediaProbe, Plan},
    time::{Rational, TimeRange},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

pub const SCOPE: &str = "Full byte digest for unchanged copy. Remux requires full per-stream packet payload, exact original-clock timestamps, flags, codec/extradata, exposed side data and all authored tags. Matroska omits its explicit undefined stream language (und) from probe tags; this omission is the only admitted language normalization. Matroska duration tags are regenerated from the verified packet clock; a generated container encoder tag is allowed where no authored value is retained. Canonical decoded reframing and unexposed raw side-data qualification remain pending.";

fn invalid(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}

#[derive(Default)]
struct StreamDigest {
    hash: Sha256,
    packets: u64,
    bytes: u64,
    first: Option<(Rational, bool)>,
}

/// Frame one bounded ffprobe JSON object at a time, including nested side data.
pub(super) struct ProbeRecords {
    root: &'static [u8],
    phase: u8,
    prefix: Vec<u8>,
    record: Vec<u8>,
    depth: usize,
    quoted: bool,
    escaped: bool,
}

struct Ledger {
    records: ProbeRecords,
    clocks: BTreeMap<u32, Rational>,
    streams: BTreeMap<u32, StreamDigest>,
    window: Option<TimeRange>,
}

impl Ledger {
    fn new(probe: &MediaProbe, window: Option<TimeRange>) -> Result<Self> {
        let clocks = probe
            .streams
            .iter()
            .map(|stream| {
                let text = probe
                    .stream_details
                    .get(&stream.index)
                    .and_then(|row| row["time_base"].as_str())
                    .ok_or_else(|| invalid("A frozen stream time base is required"))?;
                let (num, den) = text
                    .split_once('/')
                    .ok_or_else(|| invalid("Invalid stream time base"))?;
                let clock = Rational::new(
                    num.parse()
                        .map_err(|_| invalid("Invalid clock numerator"))?,
                    den.parse()
                        .map_err(|_| invalid("Invalid clock denominator"))?,
                )?;
                if clock <= Rational::ZERO {
                    return Err(invalid("A positive stream time base is required"));
                }
                Ok((stream.index, clock))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Self {
            records: ProbeRecords::new(b"{\"packets\":["),
            clocks,
            streams: BTreeMap::new(),
            window,
        })
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        let Self {
            records,
            clocks,
            streams,
            window,
        } = self;
        records.feed(bytes, |value| Self::packet(clocks, streams, *window, value))
    }

    fn packet(
        clocks: &BTreeMap<u32, Rational>,
        streams: &mut BTreeMap<u32, StreamDigest>,
        window: Option<TimeRange>,
        value: Value,
    ) -> Result<()> {
        let index = value["stream_index"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("Invalid packet stream index"))?;
        let clock = *clocks
            .get(&index)
            .ok_or_else(|| invalid("Packet references an unobserved stream"))?;
        let hash = packet_sha256(&value["data_hash"])?;
        let size = integer(&value["size"])?.ok_or_else(|| invalid("Missing packet size"))?;
        let size = u64::try_from(size).map_err(|_| invalid("Negative packet size"))?;
        let pts = timestamp(&value["pts"], clock)?;
        let dts = timestamp(&value["dts"], clock)?;
        let duration = timestamp(&value["duration"], clock)?;
        let flags = value["flags"]
            .as_str()
            .ok_or_else(|| invalid("Missing packet flags"))?;
        if let Some(window) = window {
            let pts = pts.ok_or_else(|| invalid("Packet cuts require an original presentation timestamp"))?;
            if !window.contains(pts) {
                return Ok(());
            }
            if dts != Some(pts) {
                return Err(DomainError::new("COPY_BOUNDARY_UNQUALIFIED", "This packet cut needs identical presentation and decode times"));
            }
        }
        let record = canonical(json!([
            hash,
            size.to_string(),
            flags,
            pts,
            dts,
            duration,
            value.get("side_data_list")
        ]));
        let bytes =
            serde_json::to_vec(&record).map_err(|_| invalid("Packet digest encoding failed"))?;
        let stream = streams.entry(index).or_default();
        if stream.packets == 0 {
            stream.first = pts.map(|pts| (pts, flags.starts_with('K')));
        }
        stream.packets = stream
            .packets
            .checked_add(1)
            .ok_or_else(|| invalid("Packet count overflow"))?;
        stream.bytes = stream
            .bytes
            .checked_add(size)
            .ok_or_else(|| invalid("Packet byte count overflow"))?;
        stream.hash.update((bytes.len() as u64).to_be_bytes());
        stream.hash.update(bytes);
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        self.records.finish()
    }
}

impl ProbeRecords {
    pub(super) fn new(root: &'static [u8]) -> Self {
        Self {
            root,
            phase: 0,
            prefix: Vec::new(),
            record: Vec::new(),
            depth: 0,
            quoted: false,
            escaped: false,
        }
    }

    pub(super) fn feed(
        &mut self,
        bytes: &[u8],
        mut accept: impl FnMut(Value) -> Result<()>,
    ) -> Result<()> {
        for &byte in bytes {
            match self.phase {
                0 => {
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    self.prefix.push(byte);
                    if self.prefix.len() > 32 {
                        return Err(invalid("Unexpected ffprobe JSON header"));
                    }
                    if byte == b'[' {
                        if self.prefix != self.root {
                            return Err(invalid("Unexpected ffprobe JSON root"));
                        }
                        self.phase = 1;
                    }
                }
                1 | 5 => {
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    if byte == b']' && self.phase == 1 {
                        self.phase = 4;
                    } else if byte == b'{' {
                        self.record.push(byte);
                        self.depth = 1;
                        self.phase = 2;
                    } else {
                        return Err(invalid("Expected a ffprobe JSON object"));
                    }
                }
                2 => {
                    if self.record.len() >= 64 * 1024 {
                        return Err(DomainError::new(
                            "RESOURCE_LIMIT",
                            "One ffprobe record exceeded 64 KiB",
                        ));
                    }
                    self.record.push(byte);
                    if self.quoted {
                        if self.escaped {
                            self.escaped = false;
                        } else if byte == b'\\' {
                            self.escaped = true;
                        } else if byte == b'"' {
                            self.quoted = false;
                        }
                    } else {
                        match byte {
                            b'"' => self.quoted = true,
                            b'{' | b'[' => {
                                self.depth += 1;
                                if self.depth > 64 {
                                    return Err(invalid("ffprobe JSON nesting exceeded 64"));
                                }
                            }
                            b'}' | b']' => {
                                self.depth = self
                                    .depth
                                    .checked_sub(1)
                                    .ok_or_else(|| invalid("Unbalanced ffprobe JSON"))?;
                                if self.depth == 0 {
                                    let value = serde_json::from_slice(&self.record)
                                        .map_err(|_| invalid("Invalid ffprobe JSON record"))?;
                                    accept(value)?;
                                    self.record.clear();
                                    self.phase = 3;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                3 => {
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    match byte {
                        b',' => self.phase = 5,
                        b']' => self.phase = 4,
                        _ => return Err(invalid("Unexpected bytes after an ffprobe record")),
                    }
                }
                4 => {
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    if byte != b'}' {
                        return Err(invalid("Unexpected ffprobe JSON suffix"));
                    }
                    self.phase = 6;
                }
                6 if byte.is_ascii_whitespace() => {}
                _ => return Err(invalid("Trailing ffprobe output bytes")),
            }
        }
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<()> {
        if self.phase != 6 {
            return Err(invalid("Truncated ffprobe JSON"));
        }
        Ok(())
    }
}

pub(crate) fn packet_sha256(value: &Value) -> Result<&str> {
    let hash = value
        .as_str()
        .ok_or_else(|| invalid("Missing packet SHA-256"))?;
    if !hash.starts_with("SHA256:")
        || hash.len() != 71
        || !hash[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid("Invalid packet SHA-256"));
    }
    Ok(hash)
}

pub(crate) fn integer(value: &Value) -> Result<Option<i64>> {
    if value.is_null() || value.as_str() == Some("N/A") {
        return Ok(None);
    }
    if let Some(number) = value.as_i64() {
        return Ok(Some(number));
    }
    value
        .as_str()
        .ok_or_else(|| invalid("Expected an integer packet field"))?
        .parse()
        .map(Some)
        .map_err(|_| invalid("Invalid integer packet field"))
}

pub(crate) fn timestamp(value: &Value, clock: Rational) -> Result<Option<Rational>> {
    integer(value)?
        .map(|ticks| Rational::new(ticks, 1)?.checked_mul(clock))
        .transpose()
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        other => other,
    }
}

async fn scan(
    engine: &Engine,
    path: &Path,
    probe: &MediaProbe,
    cancel: CancellationToken,
    observer: &Observer,
    deadline: nw_domain::execution::DeadlinePolicy,
    window: Option<TimeRange>,
) -> Result<Ledger> {
    let source = path.to_owned();
    let admission_cancel = cancel.clone();
    let input = crate::blocking(move || admission::open(&source, &admission_cancel)).await?;
    let mut args = [
        "-v",
        "error",
        "-max_alloc",
        "67108864",
        "-show_packets",
        "-show_data_hash",
        "sha256",
        "-show_entries",
        "packet=stream_index,pts,dts,duration,size,flags,data_hash:packet_side_data",
        "-of",
        "json=string_validation=fail",
    ]
    .map(str::to_owned)
    .to_vec();
    args.extend(input.options());
    args.extend(["-i".into(), "fd:".into()]);
    let mut prepared = vec![engine.ffprobe.path.clone()];
    prepared.extend(args.clone());
    observer(ExecutionEvent::Prepared(prepared)).await?;
    let ledger = Arc::new(Mutex::new(Ledger::new(probe, window)?));
    let consumer = ledger.clone();
    let ticket = engine.process_budget.acquire(&cancel).await?;
    let process = nw_platform::process::stream_readonly_with_policy(
        Path::new(&engine.ffprobe.path),
        &args,
        deadline,
        input.file,
        Box::new(move |bytes| {
            consumer
                .lock()
                .map_err(|_| invalid("Packet parser lock poisoned"))?
                .feed(bytes)
        }),
        cancel,
    )
    .await;
    ticket.finish(process)?;
    let ledger = Arc::try_unwrap(ledger)
        .map_err(|_| invalid("Packet consumer did not finish"))?
        .into_inner()
        .map_err(|_| invalid("Packet parser lock poisoned"))?;
    ledger.finish()?;
    Ok(ledger)
}

pub(super) async fn check_cut(
    engine: &Engine,
    source: &nw_domain::records::Asset,
    window: TimeRange,
    cancel: CancellationToken,
) -> Result<()> {
    let observer: Observer = Arc::new(|_| Box::pin(async { Ok(()) }));
    let ledger = scan(engine, Path::new(&source.identity.path), &source.probe,
        cancel, &observer, nw_domain::execution::CleanupLimits::default().operation, Some(window)).await?;
    validate_cut(&ledger, &source.probe, window)
}

fn validate_cut(ledger: &Ledger, probe: &MediaProbe, range: TimeRange) -> Result<()> {
    for stream in &probe.streams {
        if stream.kind == nw_domain::mux::StreamKind::Video && !stream.attached_picture()
            && ledger.streams.get(&stream.index).and_then(|observed| observed.first)
                != Some((range.start, true))
        {
            return Err(DomainError::new("COPY_BOUNDARY_UNQUALIFIED", "Start must match an original keyframe on each retained picture stream"));
        }
    }
    Ok(())
}

pub async fn verify(
    engine: &Engine,
    plan: &Plan,
    path: &Path,
    observed: &MediaProbe,
    additional_paths: &[&Path],
    cancel: CancellationToken,
    observer: &Observer,
) -> Result<String> {
    if additional_paths.len() != plan.added_tracks.len() {
        return Err(invalid("Staged input count differs from the frozen stream map"));
    }
    let operation = plan.execution_limits.clone().unwrap_or_default().operation;
    let primary = (
        Path::new(&plan.source.identity.path),
        &plan.source.probe,
        plan.resolution.streams.as_slice(),
    );
    let additional = additional_paths.iter().zip(&plan.added_tracks).map(|(path, input)| {
        (*path, &input.source.probe, input.tracks.as_slice())
    });
    let mut inputs = Vec::with_capacity(1 + additional_paths.len());
    for (source_path, probe, streams) in std::iter::once(primary).chain(additional) {
        let ledger = scan(engine, source_path, probe, cancel.clone(), observer, operation, plan.packet_cut).await?;
        if let Some(range) = plan.packet_cut {
            validate_cut(&ledger, probe, range)?;
        }
        inputs.push((ledger, probe, streams));
    }
    let output = scan(engine, path, observed, cancel.clone(), observer, operation, None).await?;
    let mut packets = 0u64;
    let mut bytes = 0u64;
    let mut mapped_output = observed.streams.iter();
    for (slot, (source, probe, streams)) in inputs.into_iter().enumerate() {
        for stream in streams {
            let output_index = mapped_output.next()
                .ok_or_else(|| invalid("Missing verified output stream"))?.index;
            let name = format!("Input {slot} stream {}", stream.index);
            let left = source.streams.get(&stream.index);
            let right = output.streams.get(&output_index);
            match (left, right) {
                (Some(left), Some(right)) if left.packets == right.packets
                    && left.bytes == right.bytes
                    && left.hash.clone().finalize() == right.hash.clone().finalize() => {
                    packets = packets.checked_add(left.packets)
                        .ok_or_else(|| invalid("Packet count overflow"))?;
                    bytes = bytes.checked_add(left.bytes)
                        .ok_or_else(|| invalid("Packet byte count overflow"))?;
                }
                (None, None) => {}
                _ => return Err(invalid(&format!(
                    "{name} packet payload, timing, flags or exposed side data changed"
                ))),
            }
            verify_codec(probe, stream.index, observed, output_index, &name)?;
        }
    }
    if mapped_output.next().is_some() {
        return Err(invalid("Unmapped stream appeared in the verified output"));
    }
    Ok(format!(
        "Full packet read: {packets} retained packets and {bytes} payload bytes; SHA-256 payloads, exact original-clock PTS/DTS/duration, flags, codec parameters/extradata and ffprobe-exposed side data matched. No decoded fallback or unexposed raw side-data identity claim."
    ))
}

fn verify_codec(
    source: &MediaProbe,
    source_index: u32,
    output: &MediaProbe,
    output_index: u32,
    source_name: &str,
) -> Result<()> {
    let left = source.stream_details.get(&source_index)
        .ok_or_else(|| invalid("Missing frozen source codec parameters"))?;
    let right = output.stream_details.get(&output_index)
        .ok_or_else(|| invalid("Missing output codec parameters"))?;
    for field in [
        "codec_name", "codec_type", "profile", "level", "width", "height", "pix_fmt",
        "sample_aspect_ratio", "sample_fmt", "sample_rate", "channels", "channel_layout",
        "bits_per_sample", "color_range", "color_space", "color_transfer", "color_primaries",
        "chroma_location", "extradata_hash", "side_data_list",
    ] {
        if canonical(left[field].clone()) != canonical(right[field].clone()) {
            return Err(invalid(&format!(
                "{source_name} codec parameter {field} changed; no decoded reframing oracle is admitted"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ledger() -> Ledger {
        Ledger {
            records: ProbeRecords::new(b"{\"packets\":["),
            clocks: [(0, Rational::new(1, 1000).unwrap())].into(),
            streams: BTreeMap::new(),
            window: None,
        }
    }
    #[test]
    fn packet_json_is_incremental_bounded_and_complete() {
        let text = format!(
            r#"{{"packets":[{{"stream_index":0,"pts":-1,"dts":-2,"duration":10,"size":"12","flags":"K__","data_hash":"SHA256:{}","side_data_list":[{{"side_data_type":"a}}b\\\"c"}}]}}]}}"#,
            "a".repeat(64)
        );
        for width in [1, 2, 7, 8192] {
            let mut parser = ledger();
            for chunk in text.as_bytes().chunks(width) {
                parser.feed(chunk).unwrap();
            }
            parser.finish().unwrap();
            assert_eq!(parser.streams[&0].packets, 1);
        }
        let mut parser = ledger();
        parser.feed(b"{\"packets\":[{").unwrap();
        assert!(parser.finish().is_err());
        assert_eq!(
            parser.feed(&vec![b' '; 65536]).unwrap_err().code,
            "RESOURCE_LIMIT"
        );
        let mut parser = ledger();
        assert!(parser.feed(b"{\"packets\":[{},]}").is_err());
        let mut parser = ProbeRecords::new(b"{\"frames\":[");
        let recording = br#"{"frames":[{"stream_index":0,"pts":503,"side_data_list":[{"timecodes":[{"value":"01:02:03:04"}]}]}]}"#;
        let mut frames = Vec::new();
        for chunk in recording.chunks(3) {
            parser
                .feed(chunk, |frame| {
                    frames.push(frame);
                    Ok(())
                })
                .unwrap();
        }
        parser.finish().unwrap();
        assert_eq!(frames[0]["pts"], 503);
    }
    #[test]
    fn packet_clock_comparison_uses_exact_rationals() {
        assert_eq!(
            timestamp(&json!(9000), Rational::new(1, 90000).unwrap()).unwrap(),
            timestamp(&json!(100), Rational::new(1, 1000).unwrap()).unwrap()
        );
        assert_ne!(
            timestamp(&json!(9001), Rational::new(1, 90000).unwrap()).unwrap(),
            timestamp(&json!(100), Rational::new(1, 1000).unwrap()).unwrap()
        );
    }
}
