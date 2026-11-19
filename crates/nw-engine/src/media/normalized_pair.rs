//! Bounded NUT candidates for registered windows and verified input pairs.
mod nut;
mod reference;
use super::{identities, verification::ratio};
use crate::{Engine, admission, blocking, preservation};
use nw_domain::{DomainError, Result, mux::StreamKind, records::Asset, time::Rational};
use nw_platform::{
    files::{self, DirectoryStamp, SourceStamp},
    process::{
        self, ByteInput, InvocationSpec, MediaProcessSession, PreparedRelays, RelayInputId,
        SessionArg, SessionOutput, SessionPolicy, StdoutMode,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, Write},
    ops::Range,
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const CANDIDATE_LIMIT: usize = 256 * 1024;
const NUT_VERIFICATION_BYTES: u64 = 8 * 1024 * 1024;
const NUT_VERIFICATION_PACKETS: usize = 8 * 1024;
const STREAM_FORMAT_FIELDS: &str = "codec_type,codec_name,width,height,pix_fmt,sample_rate,channels,channel_layout,sample_fmt,color_range,color_space,color_transfer,color_primaries";

/// Source positions on the selected stream's decoded frame or sample grid.
pub enum NormalizedWindow {
    VideoFrames(Range<u64>),
    AudioSamples(Range<u64>),
}

struct NormalizationPolicy {
    stdout_bytes: usize,
    require_stable_video_format: bool,
}

impl Engine {
    /// Decode a registered window through its held seekable source. The result
    /// is a single-stream candidate for independent verification before use.
    pub async fn normalize_registered_window_candidate(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        window: NormalizedWindow,
        cancel: CancellationToken,
    ) -> Result<Vec<u8>> {
        self.normalize_window(
            asset,
            source,
            stream_index,
            window,
            NormalizationPolicy {
                stdout_bytes: CANDIDATE_LIMIT,
                require_stable_video_format: false,
            },
            cancel,
        )
        .await
    }

    async fn normalize_window(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        window: NormalizedWindow,
        policy: NormalizationPolicy,
        cancel: CancellationToken,
    ) -> Result<Vec<u8>> {
        let (range, kind, filter_flag, filter, encoding) = match &window {
            NormalizedWindow::VideoFrames(range) => (
                range,
                StreamKind::Video,
                "-vf",
                format!(
                    "trim=start_frame={}:end_frame={},setpts=PTS-STARTPTS",
                    range.start, range.end
                ),
                &[
                    "-c:v",
                    "rawvideo",
                    "-pix_fmt",
                    "rgba",
                    "-fps_mode",
                    "passthrough",
                    "-enc_time_base:v",
                    "filter",
                ][..],
            ),
            NormalizedWindow::AudioSamples(range) => (
                range,
                StreamKind::Audio,
                "-af",
                format!(
                    "atrim=start_sample={}:end_sample={},asettb=1/sr,asetpts=N",
                    range.start, range.end
                ),
                &["-c:a", "pcm_s16le"][..],
            ),
        };
        if range.is_empty() || range.end > i64::MAX as u64 {
            return Err(DomainError::new(
                "INVALID_RANGE",
                "Invalid decoded source window",
            ));
        }
        let admission::RegisteredInput {
            input,
            source,
            expected,
            length,
        } = admission::registered_stream(asset, source, stream_index, kind, cancel.clone()).await?;

        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            if policy.require_stable_video_format {
                "warning"
            } else {
                "error"
            },
            "-xerror",
            "-max_alloc",
            "67108864",
            "-threads",
            "1",
            "-filter_threads",
            "1",
        ]
        .map(String::from)
        .to_vec();
        if policy.require_stable_video_format {
            // Preserve the trim counter. Dropped changed inputs produce a warning
            // that makes the requested frame identities inadmissible below.
            args.extend(["-drop_changed", "1"].map(String::from));
        }
        args.extend(input.options());
        args.extend([
            "-i".into(),
            "fd:".into(),
            "-map".into(),
            format!("0:{stream_index}"),
            filter_flag.into(),
            filter,
        ]);
        args.extend(encoding.iter().map(|option| String::from(*option)));
        args.extend(
            [
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-f",
                "nut",
                "pipe:1",
            ]
            .map(String::from),
        );
        let ticket = self.process_budget.acquire(&cancel).await?;
        let ffmpeg = self.ffmpeg.clone();
        let token = cancel.clone();
        let source = match blocking(move || {
            files::revalidate(&ffmpeg, &token)?;
            source.revalidate()?;
            Ok(source)
        })
        .await
        {
            Ok(source) => source,
            Err(error) => return ticket.finish(Err(error)),
        };
        let result = process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(30),
            policy.stdout_bytes,
            input.file,
            cancel.clone(),
        )
        .await;
        let output = ticket.finish(result)?;
        if policy.require_stable_video_format
            && (output.diagnostic_bytes > 4096
                || output
                    .stderr_tail
                    .contains("Avoiding reinit; dropping frame"))
        {
            return Err(DomainError::new(
                "REFERENCE_ALIGNMENT_REQUIRED",
                "The video format changed during frame selection or its diagnostic budget was exceeded",
            ));
        }
        if output.stdout.is_empty() {
            return Err(DomainError::new(
                "PROCESS_OUTPUT_REJECTED",
                "Empty normalized window",
            ));
        }
        let ffmpeg = self.ffmpeg.clone();
        blocking(move || {
            files::revalidate(&ffmpeg, &cancel)?;
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during normalization",
                ));
            }
            Ok(output.stdout)
        })
        .await
    }

    /// Mux prepared video and audio transports and verify that their packets and
    /// exact timestamps survive. Input admission still owns original decoding.
    pub async fn mux_normalized_pair_candidate(
        &self,
        inputs: [ByteInput; 2],
        private_root: DirectoryStamp,
        cancel: CancellationToken,
    ) -> Result<SessionOutput> {
        identities(self, cancel.clone()).await?;
        let ([video, audio], inputs) = blocking({
            let cancel = cancel.clone();
            move || {
                let mut inputs = inputs;
                let [video, audio] = &mut inputs;
                let video = video.verification_file(&cancel)?;
                let audio = audio.verification_file(&cancel)?;
                Ok(([video, audio], inputs))
            }
        })
        .await?;
        let video = self.inspect_nut(video, cancel.clone()).await?;
        let audio = self.inspect_nut(audio, cancel.clone()).await?;
        let expected = [
            single_stream(&video, StreamKind::Video)?,
            single_stream(&audio, StreamKind::Audio)?,
        ];
        let mut executable_sha256 = [0; 32];
        hex::decode_to_slice(&self.ffmpeg.sha256, &mut executable_sha256)
            .map_err(|_| DomainError::new("COMPONENT_UNAVAILABLE", "Invalid FFmpeg identity"))?;
        let mut args = Vec::from(literal_args([
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-xerror",
            "-max_alloc",
            "67108864",
            "-copyts",
        ]));
        for index in 0..2 {
            args.extend(literal_args([
                "-protocol_whitelist",
                "file",
                "-format_whitelist",
                "nut",
                "-f",
                "nut",
                "-i",
            ]));
            args.push(SessionArg::Input(RelayInputId(index)));
        }
        args.extend(literal_args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "disabled",
            "-f",
            "nut",
            "pipe:1",
        ]));
        let spec = InvocationSpec {
            program: self.ffmpeg.path.clone().into(),
            args,
            executable_sha256,
            profile_id: "normalized-nut-pair-v1".into(),
        };
        let ticket = self.process_budget.acquire(&cancel).await?;
        let prepared = PreparedRelays::prepare(
            inputs,
            private_root,
            SessionPolicy::byte_fixture(),
            Box::new(ticket),
            cancel.clone(),
        )
        .await
        .map_err(|failure| failure.cause)?;
        let session = MediaProcessSession::spawn(
            prepared,
            spec,
            StdoutMode::Capture {
                limit: CANDIDATE_LIMIT,
            },
        )
        .await
        .map_err(|failure| failure.cause)?;
        let mut output = session.finish().await.map_err(|failure| failure.cause)?;
        if output.output.stdout.is_empty()
            || !output
                .closed
                .pumps()
                .iter()
                .all(|pump| pump.transport_complete())
        {
            return Err(DomainError::new(
                "PROCESS_OUTPUT_REJECTED",
                "Incomplete normalized pair output",
            ));
        }
        let (candidate, bytes) = blocking({
            let bytes = std::mem::take(&mut output.output.stdout);
            move || {
                let mut file = tempfile::tempfile().map_err(files::io_error)?;
                file.write_all(&bytes).map_err(files::io_error)?;
                file.rewind().map_err(files::io_error)?;
                Ok((file, bytes))
            }
        })
        .await?;
        let report = self.inspect_nut(candidate, cancel.clone()).await?;
        if streams(&report)?.len() != expected.len()
            || stream_signature(&report, 0, StreamKind::Video)? != expected[0]
            || stream_signature(&report, 1, StreamKind::Audio)? != expected[1]
        {
            return Err(invalid_pair(
                "Pair packet data or timestamps differ from the inputs",
            ));
        }
        identities(self, cancel).await?;
        output.output.stdout = bytes;
        Ok(output)
    }

    async fn inspect_nut(&self, mut input: File, cancel: CancellationToken) -> Result<Value> {
        let (input, declarations, ledger) = blocking({
            let cancel = cancel.clone();
            move || {
                let length = input.metadata().map_err(files::io_error)?.len();
                if length > NUT_VERIFICATION_BYTES {
                    return Err(invalid_pair(
                        "NUT candidate exceeded the verification byte limit",
                    ));
                }
                let (declarations, ledger) = {
                    let bounded = BufReader::new((&mut input).take(length + 1));
                    let mut reader = nut::NutReader::new(bounded, length as usize)?;
                    let mut ledger = Vec::new();
                    loop {
                        if cancel.is_cancelled() {
                            return Err(DomainError::new(
                                "CANCELLED",
                                "NUT verification cancelled",
                            ));
                        }
                        let Some(packet) = reader.next_packet()? else {
                            break;
                        };
                        if ledger.len() == NUT_VERIFICATION_PACKETS {
                            return Err(invalid_pair(
                                "NUT candidate exceeded the packet inventory limit",
                            ));
                        }
                        ledger.push(WirePacket {
                            stream: packet.stream,
                            pts: packet.pts,
                            size: packet.bytes.len() as i64,
                            sha256: Sha256::digest(&packet.bytes).into(),
                        });
                    }
                    if reader.position() != length {
                        return Err(invalid_pair(
                            "NUT candidate changed length during verification",
                        ));
                    }
                    (reader.streams().to_vec(), ledger)
                };
                input.rewind().map_err(files::io_error)?;
                Ok((input, declarations, ledger))
            }
        })
        .await?;
        let fields = format!(
            "stream=index,time_base,{STREAM_FORMAT_FIELDS}:packet=stream_index,pts,size,data_hash"
        );
        let mut args = [
            "-v",
            "error",
            "-max_alloc",
            "67108864",
            "-show_streams",
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            fields.as_str(),
            "-of",
            "json=compact=1:string_validation=fail",
        ]
        .map(String::from)
        .to_vec();
        args.extend(admission::options("nut"));
        args.extend(["-i", "fd:"].map(String::from));
        let ticket = self.process_budget.acquire(&cancel).await?;
        let report = process::run_readonly(
            Path::new(&self.ffprobe.path),
            &args,
            Duration::from_secs(6),
            1024 * 1024,
            input,
            cancel,
        )
        .await;
        let report = ticket.finish(report)?;
        let report = serde_json::from_slice(&report.stdout)
            .map_err(|_| invalid_pair("Invalid NUT packet inventory"))?;
        verify_wire_report(&declarations, &ledger, &report)?;
        Ok(report)
    }
}

fn invalid_pair(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}

struct WirePacket {
    stream: usize,
    pts: i64,
    size: i64,
    sha256: [u8; 32],
}

fn verify_wire_report(
    declarations: &[nut::Stream],
    ledger: &[WirePacket],
    report: &Value,
) -> Result<()> {
    let probe_streams = streams(report)?;
    let probe_packets = report["packets"]
        .as_array()
        .ok_or_else(|| invalid_pair("Missing NUT packet inventory"))?;
    if declarations.len() != probe_streams.len() || ledger.len() != probe_packets.len() {
        return Err(invalid_pair("NUT wire and ffprobe inventories differ"));
    }
    for (index, (declared, probe)) in declarations.iter().zip(probe_streams).enumerate() {
        let format_matches = match declared.format {
            nut::Format::Rgba { width, height } => {
                probe["codec_type"] == "video"
                    && probe["codec_name"] == "rawvideo"
                    && probe["pix_fmt"] == "rgba"
                    && probe["width"].as_u64() == Some(u64::from(width))
                    && probe["height"].as_u64() == Some(u64::from(height))
            }
            nut::Format::PcmS16 {
                sample_rate,
                channels,
            } => probe_audio_matches(probe, "pcm_s16le", "s16", sample_rate, channels),
            nut::Format::StereoPcmF32 => probe_audio_matches(probe, "pcm_f32le", "flt", 48_000, 2),
        };
        if !format_matches
            || probe["index"].as_u64() != Some(index as u64)
            || ratio(probe["time_base"].as_str().unwrap_or_default())? != declared.clock
        {
            return Err(invalid_pair(
                "NUT wire and ffprobe stream declarations differ",
            ));
        }
    }
    for (packet, probe) in ledger.iter().zip(probe_packets) {
        if probe["stream_index"].as_u64() != Some(packet.stream as u64)
            || preservation::integer(&probe["pts"])? != Some(packet.pts)
            || preservation::integer(&probe["size"])? != Some(packet.size)
            || packet_digest(&probe["data_hash"])? != packet.sha256
        {
            return Err(invalid_pair("NUT wire and ffprobe packet data differ"));
        }
    }
    Ok(())
}

fn probe_audio_matches(
    stream: &Value,
    codec: &str,
    format: &str,
    sample_rate: u32,
    channels: u32,
) -> bool {
    stream["codec_type"] == "audio"
        && stream["codec_name"] == codec
        && stream["sample_fmt"] == format
        && stream["sample_rate"]
            .as_str()
            .and_then(|value| value.parse::<u32>().ok())
            == Some(sample_rate)
        && stream["channels"].as_u64() == Some(u64::from(channels))
}

fn packet_digest(value: &Value) -> Result<[u8; 32]> {
    let digest = preservation::packet_sha256(value)?;
    let mut hash = [0; 32];
    hex::decode_to_slice(&digest[7..], &mut hash)
        .map_err(|_| invalid_pair("Invalid packet digest"))?;
    Ok(hash)
}

#[derive(PartialEq, Eq)]
struct StreamSignature {
    format: Value,
    packets: Vec<(Rational, u64, [u8; 32])>,
}

fn streams(report: &Value) -> Result<&[Value]> {
    report["streams"]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid_pair("Missing NUT stream declarations"))
}

fn single_stream(report: &Value, role: StreamKind) -> Result<StreamSignature> {
    if streams(report)?.len() != 1 {
        return Err(invalid_pair(
            "Normalized transport must contain exactly one stream",
        ));
    }
    stream_signature(report, 0, role)
}

fn stream_signature(report: &Value, index: u32, role: StreamKind) -> Result<StreamSignature> {
    let streams = streams(report)?;
    let stream = streams
        .get(index as usize)
        .ok_or_else(|| invalid_pair("Missing normalized stream"))?;
    let shape = match role {
        StreamKind::Video
            if stream["codec_type"] == "video"
                && stream["codec_name"] == "rawvideo"
                && stream["pix_fmt"] == "rgba" =>
        {
            stream["width"]
                .as_u64()
                .zip(stream["height"].as_u64())
                .and_then(|(width, height)| width.checked_mul(height)?.checked_mul(4))
        }
        StreamKind::Audio
            if stream["codec_type"] == "audio"
                && stream["codec_name"] == "pcm_s16le"
                && stream["sample_fmt"] == "s16"
                && stream["sample_rate"]
                    .as_str()
                    .and_then(|n| n.parse::<u32>().ok())
                    .is_some_and(|n| n > 0) =>
        {
            stream["channels"]
                .as_u64()
                .and_then(|channels| channels.checked_mul(2))
        }
        StreamKind::Audio if probe_audio_matches(stream, "pcm_f32le", "flt", 48_000, 2) => Some(8),
        _ => None,
    }
    .filter(|size| *size > 0)
    .ok_or_else(|| invalid_pair("Unexpected normalized stream format"))?;
    if stream["index"].as_u64() != Some(u64::from(index)) {
        return Err(invalid_pair("Normalized stream index differs"));
    }
    let grid = ratio(stream["time_base"].as_str().unwrap_or_default())?;
    // FFprobe may emit nested tags even with a field list. The normalized mux
    // explicitly discards metadata, so compare the declared media fields only.
    let format = Value::Object(
        STREAM_FORMAT_FIELDS
            .split(',')
            .filter_map(|field| stream.get(field).map(|value| (field.into(), value.clone())))
            .collect(),
    );
    let mut packets = Vec::new();
    let entries = report["packets"]
        .as_array()
        .ok_or_else(|| invalid_pair("Missing normalized packet inventory"))?;
    let mut previous_timestamp = None;
    for packet in entries {
        let packet_index = packet["stream_index"]
            .as_u64()
            .filter(|value| *value < streams.len() as u64)
            .ok_or_else(|| invalid_pair("Packet refers to an unknown normalized stream"))?;
        if packet_index != u64::from(index) {
            continue;
        }
        let timestamp = preservation::timestamp(&packet["pts"], grid)?
            .ok_or_else(|| invalid_pair("Missing exact packet timestamp"))?;
        if previous_timestamp.is_some_and(|previous| previous >= timestamp) {
            return Err(invalid_pair("Normalized packet timestamps did not advance"));
        }
        previous_timestamp = Some(timestamp);
        let size = preservation::integer(&packet["size"])?
            .and_then(|size| u64::try_from(size).ok())
            .filter(|size| *size > 0)
            .ok_or_else(|| invalid_pair("Missing packet length"))?;
        if !size.is_multiple_of(shape) || (role == StreamKind::Video && size != shape) {
            return Err(invalid_pair(
                "Normalized packet length differs from its stream shape",
            ));
        }
        packets.push((timestamp, size, packet_digest(&packet["data_hash"])?));
    }
    if packets.is_empty() {
        return Err(invalid_pair(
            "Normalized transport has no packets for its stream",
        ));
    }
    Ok(StreamSignature { format, packets })
}

fn literal_args<const N: usize>(values: [&str; N]) -> [SessionArg; N] {
    values.map(|value| SessionArg::Literal(value.into()))
}
