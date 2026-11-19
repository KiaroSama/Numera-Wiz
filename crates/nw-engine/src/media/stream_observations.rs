//! Stream ffprobe records while retaining only the requested observation rows.
use super::identities;
use crate::{Engine, admission, blocking};
use nw_domain::{
    DomainError, Result,
    mux::StreamKind,
    records::Asset,
    stream_observations::{Detail, Input, Kind, Report, Row},
    time::Rational,
};
use nw_platform::{files::SourceStamp, process};
use std::{
    collections::BTreeMap,
    ops::Range,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const MAX_RECORD_BYTES: usize = 1024;
const MAX_SCAN_BYTES: u64 = 128 * 1024 * 1024;
const PACKET_FIELDS: &str = "packet=stream_index,pts,dts,duration,size,flags,data_hash:packet_tags=:packet_side_data_list=:packet_side_data=";
const FRAME_FIELDS: &str = "frame=stream_index,pts,pkt_dts,duration,key_frame,pict_type,width,height,nb_samples:frame_tags=:frame_side_data_list=:frame_side_data=:frame_side_data_components=:frame_side_data_component=:frame_side_data_pieces=:frame_side_data_piece=";

fn malformed() -> DomainError {
    DomainError::new("INVALID_PROBE", "Invalid ffprobe stream observation")
}

struct Scan {
    kind: Kind,
    stream_index: u32,
    clock: Rational,
    window: Range<u64>,
    record: Vec<u8>,
    observed: u64,
    rows: Vec<Row>,
}

impl Scan {
    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            if self.record.len() + part.len() > MAX_RECORD_BYTES {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "An observation row exceeded 1 KiB",
                ));
            }
            self.record.extend_from_slice(part);
            if part.ends_with(b"\n") {
                let line = std::str::from_utf8(&self.record).map_err(|_| malformed())?;
                let row = self.row(line.trim_end_matches(['\r', '\n']))?;
                if self.window.contains(&self.observed) {
                    self.rows.push(row);
                }
                self.observed += 1;
                self.record.clear();
            }
        }
        Ok(())
    }

    fn row(&self, line: &str) -> Result<Row> {
        let prefix = match self.kind {
            Kind::Packets => "packet|",
            Kind::Frames => "frame|",
        };
        let mut fields = BTreeMap::new();
        // The compact writer can leave a separator for an empty side-data section.
        let fields_text = line.strip_prefix(prefix).ok_or_else(malformed)?;
        for field in fields_text.trim_end_matches('|').split('|') {
            let (key, value) = field.split_once('=').ok_or_else(malformed)?;
            if fields.insert(key, value).is_some() {
                return Err(malformed());
            }
        }
        let read = |key| fields.get(key).copied().filter(|value| *value != "N/A");
        if read("stream_index").and_then(|value| value.parse().ok()) != Some(self.stream_index) {
            return Err(malformed());
        }
        let count = |key| {
            read(key)
                .map(|value| value.parse::<u32>().map_err(|_| malformed()))
                .transpose()
        };
        let timestamp = |key| {
            read(key)
                .map(|value| {
                    Rational::new(value.parse::<i64>().map_err(|_| malformed())?, 1)?
                        .checked_mul(self.clock)
                })
                .transpose()
        };
        let detail = match self.kind {
            Kind::Packets => {
                let bytes = read("size")
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(malformed)?;
                let flags = read("flags")
                    .filter(|value| {
                        (1..=16).contains(&value.len())
                            && value.bytes().all(|byte| byte == b'_' || byte.is_ascii_uppercase())
                    })
                    .ok_or_else(malformed)?;
                let hash = read("data_hash")
                    .and_then(|value| value.strip_prefix("SHA256:"))
                    .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    .ok_or_else(malformed)?;
                Detail::Packet {
                    bytes: bytes.to_string(),
                    flags: flags.into(),
                    data_sha256: hash.to_ascii_lowercase(),
                }
            }
            Kind::Frames => Detail::Frame {
                key_frame: match read("key_frame") {
                    Some("0") => false,
                    Some("1") => true,
                    _ => return Err(malformed()),
                },
                picture_type: read("pict_type").map(str::to_owned),
                width: count("width")?,
                height: count("height")?,
                samples_per_channel: count("nb_samples")?,
            },
        };
        Ok(Row {
            ordinal: self.observed.to_string(),
            pts: timestamp("pts")?,
            dts: timestamp(match self.kind {
                Kind::Packets => "dts",
                Kind::Frames => "pkt_dts",
            })?,
            duration: timestamp("duration")?,
            detail,
        })
    }
}

impl Engine {
    pub async fn observe_stream(
        &self,
        asset: &Asset,
        source: SourceStamp,
        selection: Input,
        cancel: CancellationToken,
    ) -> Result<Report> {
        let window = selection.window()?;
        let role = asset
            .probe
            .streams
            .iter()
            .find(|stream| stream.index == selection.stream_index)
            .map(|stream| stream.kind.clone())
            .ok_or_else(malformed)?;
        if role == StreamKind::Attachment {
            return Err(DomainError::new(
                "INPUT_DENIED",
                "Attachments have no timed observation stream",
            ));
        }
        if selection.kind == Kind::Frames && !matches!(role, StreamKind::Video | StreamKind::Audio) {
            return Err(DomainError::new(
                "INPUT_DENIED",
                "Decoded frame observation requires a video or audio stream",
            ));
        }
        let (num, den) = asset
            .probe
            .stream_details
            .get(&selection.stream_index)
            .and_then(|stream| stream["time_base"].as_str())
            .and_then(|clock| clock.split_once('/'))
            .ok_or_else(malformed)?;
        let clock = Rational::new(
            num.parse().map_err(|_| malformed())?,
            den.parse().map_err(|_| malformed())?,
        )?;
        if clock <= Rational::ZERO {
            return Err(malformed());
        }
        let admission::RegisteredInput {
            input,
            source,
            expected,
            length,
        } = admission::registered_stream(asset, source, selection.stream_index, role, cancel.clone())
            .await?;
        let mut args = ["-v", "error", "-max_alloc", "67108864", "-select_streams"]
            .map(str::to_owned)
            .to_vec();
        args.push(selection.stream_index.to_string());
        let options: &[&str] = match selection.kind {
            Kind::Packets => &[
                "-show_packets",
                "-show_data_hash",
                "sha256",
                "-show_entries",
                PACKET_FIELDS,
            ],
            Kind::Frames => &[
                "-threads",
                "1",
                "-show_frames",
                "-show_entries",
                FRAME_FIELDS,
            ],
        };
        args.extend(options.iter().map(|option| (*option).to_owned()));
        args.extend(["-of", "compact=print_section=1:nokey=0"].map(str::to_owned));
        args.extend(input.options());
        args.extend(["-i", "fd:"].map(str::to_owned));
        let all = Arc::new(Mutex::new(Scan {
            kind: selection.kind,
            stream_index: selection.stream_index,
            clock,
            window,
            record: Vec::new(),
            observed: 0,
            rows: Vec::new(),
        }));
        let consumer = all.clone();
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone())
            .await
            .and_then(|_| source.revalidate())
        {
            return ticket.finish(Err(error));
        }
        let output = process::stream_verified_readonly(
            Path::new(&self.ffprobe.path),
            &args,
            Duration::from_secs(45),
            input.file,
            process::StreamLimits {
                stdout_bytes: MAX_SCAN_BYTES,
                stderr_bytes: 4096,
            },
            Box::new(move |bytes| consumer.lock().map_err(|_| malformed())?.feed(bytes)),
            cancel.clone(),
        )
        .await;
        if !ticket.finish(output)?.stderr.is_empty() {
            return Err(DomainError::new(
                "INVALID_PROBE",
                "ffprobe reported an error during the scan",
            ));
        }
        let scan = Arc::try_unwrap(all)
            .map_err(|_| malformed())?
            .into_inner()
            .map_err(|_| malformed())?;
        if !scan.record.is_empty() {
            return Err(malformed());
        }
        identities(self, cancel.clone()).await?;
        blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during observation",
                ));
            }
            Ok(())
        })
        .await?;
        Ok(Report {
            selection,
            source_sha256: asset.identity.sha256.clone(),
            ffprobe_sha256: self.ffprobe.sha256.clone(),
            time_base: clock,
            observed: scan.observed.to_string(),
            rows: scan.rows,
        })
    }
}
