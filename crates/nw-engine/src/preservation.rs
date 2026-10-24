//! Full packet reads use bounded record framing and per-stream incremental digests.
use crate::{Engine, ExecutionEvent, Observer, admission};
use nw_domain::{
    DomainError, Result,
    records::{MediaProbe, Plan},
    time::Rational,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub const SCOPE: &str = "Full byte digest for unchanged copy. Remux requires full per-stream packet payload, exact original-clock timestamps, flags, codec/extradata, exposed side data and all authored tags. Matroska duration tags are regenerated from the verified packet clock; a generated container encoder tag is allowed where no authored value is retained. Canonical decoded reframing and unexposed raw side-data qualification remain pending.";

fn invalid(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}

#[derive(Default)]
struct StreamDigest {
    hash: Sha256,
    packets: u64,
    bytes: u64,
}

struct Ledger {
    phase: u8,
    prefix: Vec<u8>,
    record: Vec<u8>,
    depth: usize,
    quoted: bool,
    escaped: bool,
    clocks: BTreeMap<u32, Rational>,
    streams: BTreeMap<u32, StreamDigest>,
}

impl Ledger {
    fn new(probe: &MediaProbe) -> Result<Self> {
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
            phase: 0,
            prefix: Vec::new(),
            record: Vec::new(),
            depth: 0,
            quoted: false,
            escaped: false,
            clocks,
            streams: BTreeMap::new(),
        })
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        for &byte in bytes {
            match self.phase {
                0 => {
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    self.prefix.push(byte);
                    if self.prefix.len() > 32 {
                        return Err(invalid("Unexpected packet JSON header"));
                    }
                    if byte == b'[' {
                        if self.prefix != b"{\"packets\":[" {
                            return Err(invalid("Unexpected packet JSON root"));
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
                        return Err(invalid("Expected a packet JSON object"));
                    }
                }
                2 => {
                    if self.record.len() >= 64 * 1024 {
                        return Err(DomainError::new(
                            "RESOURCE_LIMIT",
                            "One packet verification record exceeded 64 KiB",
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
                                    return Err(invalid("Packet JSON nesting exceeded 64"));
                                }
                            }
                            b'}' | b']' => {
                                self.depth = self
                                    .depth
                                    .checked_sub(1)
                                    .ok_or_else(|| invalid("Unbalanced packet JSON"))?;
                                if self.depth == 0 {
                                    let value = serde_json::from_slice(&self.record)
                                        .map_err(|_| invalid("Invalid packet JSON record"))?;
                                    self.packet(value)?;
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
                        _ => return Err(invalid("Unexpected bytes after a packet")),
                    }
                }
                4 => {
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    if byte != b'}' {
                        return Err(invalid("Unexpected packet JSON suffix"));
                    }
                    self.phase = 6;
                }
                6 if byte.is_ascii_whitespace() => {}
                _ => return Err(invalid("Trailing packet verification bytes")),
            }
        }
        Ok(())
    }

    fn packet(&mut self, value: Value) -> Result<()> {
        let index = value["stream_index"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("Invalid packet stream index"))?;
        let clock = *self
            .clocks
            .get(&index)
            .ok_or_else(|| invalid("Packet references an unobserved stream"))?;
        let hash = value["data_hash"]
            .as_str()
            .ok_or_else(|| invalid("Missing packet SHA-256"))?;
        if !hash.starts_with("SHA256:")
            || hash.len() != 71
            || !hash[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid("Invalid packet SHA-256"));
        }
        let size = integer(&value["size"])?.ok_or_else(|| invalid("Missing packet size"))?;
        let size = u64::try_from(size).map_err(|_| invalid("Negative packet size"))?;
        let pts = timestamp(&value["pts"], clock)?;
        let dts = timestamp(&value["dts"], clock)?;
        let duration = timestamp(&value["duration"], clock)?;
        let flags = value["flags"]
            .as_str()
            .ok_or_else(|| invalid("Missing packet flags"))?;
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
        let stream = self.streams.entry(index).or_default();
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
        if self.phase != 6 {
            return Err(invalid("Truncated packet verification JSON"));
        }
        Ok(())
    }
}

fn integer(value: &Value) -> Result<Option<i64>> {
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

fn timestamp(value: &Value, clock: Rational) -> Result<Option<Rational>> {
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
    let ledger = Arc::new(Mutex::new(Ledger::new(probe)?));
    let consumer = ledger.clone();
    let ticket = engine.process_budget.acquire(&cancel).await?;
    let process = nw_platform::process::stream_readonly(
        Path::new(&engine.ffprobe.path),
        &args,
        Duration::from_secs(10800),
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

pub async fn verify(
    engine: &Engine,
    plan: &Plan,
    path: &Path,
    observed: &MediaProbe,
    cancel: CancellationToken,
    observer: &Observer,
) -> Result<String> {
    let source = scan(
        engine,
        Path::new(&plan.source.identity.path),
        &plan.source.probe,
        cancel.clone(),
        observer,
    )
    .await?;
    let output = scan(engine, path, observed, cancel, observer).await?;
    let mut packets = 0u64;
    let mut bytes = 0u64;
    for (position, stream) in plan.resolution.streams.iter().enumerate() {
        let output_index = observed
            .streams
            .get(position)
            .ok_or_else(|| invalid("Missing verified output stream"))?
            .index;
        let left = source.streams.get(&stream.index);
        let right = output.streams.get(&output_index);
        match (left, right) {
            (Some(left), Some(right))
                if left.packets == right.packets
                    && left.bytes == right.bytes
                    && left.hash.clone().finalize() == right.hash.clone().finalize() =>
            {
                packets = packets
                    .checked_add(left.packets)
                    .ok_or_else(|| invalid("Packet count overflow"))?;
                bytes = bytes
                    .checked_add(left.bytes)
                    .ok_or_else(|| invalid("Packet byte count overflow"))?;
            }
            (None, None) => {}
            _ => {
                return Err(invalid(&format!(
                    "Retained stream {} packet payload, timing, flags or exposed side data changed",
                    stream.index
                )));
            }
        }
        let left = plan
            .source
            .probe
            .stream_details
            .get(&stream.index)
            .ok_or_else(|| invalid("Missing frozen source codec parameters"))?;
        let right = observed
            .stream_details
            .get(&output_index)
            .ok_or_else(|| invalid("Missing output codec parameters"))?;
        for field in [
            "codec_name",
            "codec_type",
            "profile",
            "level",
            "width",
            "height",
            "pix_fmt",
            "sample_aspect_ratio",
            "sample_fmt",
            "sample_rate",
            "channels",
            "channel_layout",
            "bits_per_sample",
            "color_range",
            "color_space",
            "color_transfer",
            "color_primaries",
            "chroma_location",
            "extradata_hash",
            "side_data_list",
        ] {
            if canonical(left[field].clone()) != canonical(right[field].clone()) {
                return Err(invalid(&format!(
                    "Retained stream {} codec parameter {field} changed; no decoded reframing oracle is admitted",
                    stream.index
                )));
            }
        }
    }
    Ok(format!(
        "Full packet read: {packets} retained packets and {bytes} payload bytes; SHA-256 payloads, exact original-clock PTS/DTS/duration, flags, codec parameters/extradata and ffprobe-exposed side data matched. No decoded fallback or unexposed raw side-data identity claim."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ledger() -> Ledger {
        Ledger {
            phase: 0,
            prefix: Vec::new(),
            record: Vec::new(),
            depth: 0,
            quoted: false,
            escaped: false,
            clocks: [(0, Rational::new(1, 1000).unwrap())].into(),
            streams: BTreeMap::new(),
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
