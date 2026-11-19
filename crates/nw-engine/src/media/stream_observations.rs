//! Stream ffprobe records while retaining only the requested observation rows.
use super::identities;
use crate::{
    Engine, admission, blocking,
    preservation::{self, ProbeRecords},
};
use nw_domain::{
    DomainError, Result,
    mux::StreamKind,
    records::Asset,
    stream_observations::{Detail, Input, Kind, Report, Row},
    time::Rational,
};
use nw_platform::{files::SourceStamp, process};
use serde_json::Value;
use std::{
    ops::Range,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

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
    observed: u64,
    rows: Vec<Row>,
}

impl Scan {
    fn accept(&mut self, record: Value) -> Result<()> {
        if record["stream_index"].as_u64() != Some(u64::from(self.stream_index)) {
            return Err(malformed());
        }
        if self.window.contains(&self.observed) {
            let row = self.row(&record)?;
            self.rows.push(row);
        }
        self.observed += 1;
        Ok(())
    }

    fn row(&self, record: &Value) -> Result<Row> {
        let count = |key: &str| {
            preservation::integer(&record[key])?
                .map(|number| u32::try_from(number).map_err(|_| malformed()))
                .transpose()
        };
        let timestamp = |key: &str| preservation::timestamp(&record[key], self.clock);
        let detail = match self.kind {
            Kind::Packets => {
                let bytes = preservation::integer(&record["size"])?
                    .and_then(|bytes| u64::try_from(bytes).ok())
                    .ok_or_else(malformed)?;
                let flags = record["flags"]
                    .as_str()
                    .filter(|value| {
                        (1..=16).contains(&value.len())
                            && value
                                .bytes()
                                .all(|byte| byte == b'_' || byte.is_ascii_uppercase())
                    })
                    .ok_or_else(malformed)?;
                let hash = preservation::packet_sha256(&record["data_hash"])?;
                Detail::Packet {
                    bytes: bytes.to_string(),
                    flags: flags.into(),
                    data_sha256: hash[7..].to_ascii_lowercase(),
                }
            }
            Kind::Frames => Detail::Frame {
                key_frame: match record["key_frame"].as_u64() {
                    Some(0) => false,
                    Some(1) => true,
                    _ => return Err(malformed()),
                },
                picture_type: record["pict_type"].as_str().map(str::to_owned),
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
        if selection.kind == Kind::Frames && !matches!(role, StreamKind::Video | StreamKind::Audio)
        {
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
        } = admission::registered_stream(
            asset,
            source,
            selection.stream_index,
            role,
            cancel.clone(),
        )
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
        args.extend(["-of", "json=compact=1:string_validation=fail"].map(str::to_owned));
        args.extend(input.options());
        args.extend(["-i", "fd:"].map(str::to_owned));
        let root: &'static [u8] = match selection.kind {
            Kind::Packets => b"{\"packets\":[",
            Kind::Frames => b"{\"frames\":[",
        };
        let all = Arc::new(Mutex::new((
            ProbeRecords::new(root),
            Scan {
                kind: selection.kind,
                stream_index: selection.stream_index,
                clock,
                window,
                observed: 0,
                rows: Vec::new(),
            },
        )));
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
            Box::new(move |bytes| {
                let mut state = consumer.lock().map_err(|_| malformed())?;
                let (records, scan) = &mut *state;
                records.feed(bytes, |record| scan.accept(record))
            }),
            cancel.clone(),
        )
        .await;
        if ticket.finish(output)?.diagnostic_bytes != 0 {
            return Err(DomainError::new(
                "INVALID_PROBE",
                "ffprobe reported an error during the scan",
            ));
        }
        let (records, scan) = Arc::try_unwrap(all)
            .map_err(|_| malformed())?
            .into_inner()
            .map_err(|_| malformed())?;
        records.finish()?;
        if selection.kind == Kind::Frames && scan.observed == 0 {
            return Err(DomainError::new(
                "FRAME_UNAVAILABLE",
                "ffprobe returned no decoded frames for the registered stream",
            ));
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
