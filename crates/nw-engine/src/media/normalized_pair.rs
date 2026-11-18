//! Bounded NUT candidates for registered windows and verified input pairs.
use super::{identities, verification::ratio};
use crate::{Engine, admission, blocking};
use nw_domain::{DomainError, Result, mux::StreamKind, records::Asset, time::Rational};
use nw_platform::{
    files::{self, DirectoryStamp, SourceStamp},
    process::{
        self, ByteInput, InvocationSpec, MediaProcessSession, PreparedRelays, RelayInputId,
        SessionArg, SessionOutput, SessionPolicy, StdoutMode,
    },
};
use serde_json::Value;
use std::{
    fs::File,
    io::{Seek, Write},
    ops::Range,
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const CANDIDATE_LIMIT: usize = 256 * 1024;

/// Source positions on the selected stream's decoded frame or sample grid.
pub enum NormalizedWindow {
    VideoFrames(Range<u64>),
    AudioSamples(Range<u64>),
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
        if !asset.probe.streams.iter().any(|stream| {
            stream.index == stream_index && stream.kind == kind && !stream.attached_picture()
        }) {
            return Err(DomainError::new(
                "INPUT_DENIED",
                "Selected stream differs from its registered role",
            ));
        }
        let expected = asset.identity.clone();
        let object = asset.source_object.clone().ok_or_else(|| {
            DomainError::new("INPUT_DENIED", "Registered source lacks a native identity")
        })?;
        let demuxer = admission::frozen_demuxer(&asset.probe.format)?;
        let length = expected.bytes.parse::<u64>().map_err(|_| {
            DomainError::new(
                "INVALID_IDENTITY",
                "Registered source has an invalid byte count",
            )
        })?;
        let token = cancel.clone();
        let (input, source, expected) = blocking(move || {
            source.require_object(&object)?;
            if source.byte_len()? != length
                || source.identity_with_limit(&token, length)? != expected
            {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source content changed",
                ));
            }
            let input = admission::from_file(source.try_clone_file()?, &token)?;
            if input.demuxer != demuxer {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source format changed",
                ));
            }
            Ok((input, source, expected))
        })
        .await?;

        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
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
            CANDIDATE_LIMIT,
            input.file,
            cancel.clone(),
        )
        .await;
        let output = ticket.finish(result)?;
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

    async fn inspect_nut(&self, input: File, cancel: CancellationToken) -> Result<Value> {
        let mut args = [
            "-v", "error", "-max_alloc", "67108864",
            "-show_streams", "-show_packets", "-show_data_hash", "sha256", "-show_entries",
            "stream=index,codec_type,codec_name,width,height,pix_fmt,sample_rate,channels,channel_layout,sample_fmt,time_base,color_range,color_space,color_transfer,color_primaries:packet=stream_index,pts,size,data_hash",
            "-of", "json=compact=1:string_validation=fail",
        ].map(String::from).to_vec();
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
        serde_json::from_slice(&report.stdout)
            .map_err(|_| invalid_pair("Invalid NUT packet inventory"))
    }
}

fn invalid_pair(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
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
        _ => None,
    }
    .filter(|size| *size > 0)
    .ok_or_else(|| invalid_pair("Unexpected normalized stream format"))?;
    if stream["index"].as_u64() != Some(u64::from(index)) {
        return Err(invalid_pair("Normalized stream index differs"));
    }
    let grid = ratio(stream["time_base"].as_str().unwrap_or_default())?;
    let mut format = stream.clone();
    let properties = format
        .as_object_mut()
        .ok_or_else(|| invalid_pair("Missing normalized stream properties"))?;
    properties.remove("index");
    properties.remove("time_base");
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
        let pts = packet["pts"]
            .as_i64()
            .ok_or_else(|| invalid_pair("Missing exact packet timestamp"))?;
        let timestamp = Rational::new(pts, 1)?.checked_mul(grid)?;
        if previous_timestamp.is_some_and(|previous| previous >= timestamp) {
            return Err(invalid_pair("Normalized packet timestamps did not advance"));
        }
        previous_timestamp = Some(timestamp);
        let size = packet["size"]
            .as_str()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|size| *size > 0)
            .ok_or_else(|| invalid_pair("Missing packet length"))?;
        if !size.is_multiple_of(shape) || (role == StreamKind::Video && size != shape) {
            return Err(invalid_pair(
                "Normalized packet length differs from its stream shape",
            ));
        }
        let raw_hash = packet["data_hash"]
            .as_str()
            .and_then(|hash| hash.strip_prefix("SHA256:"))
            .ok_or_else(|| invalid_pair("Missing normalized packet digest"))?;
        let mut hash = [0; 32];
        hex::decode_to_slice(raw_hash, &mut hash)
            .map_err(|_| invalid_pair("Invalid packet digest"))?;
        packets.push((timestamp, size, hash));
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
