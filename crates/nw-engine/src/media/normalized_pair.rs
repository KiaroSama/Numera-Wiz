//! Bounded NUT candidates for registered windows and prepared input pairs.
use crate::{Engine, admission, blocking};
use nw_domain::{DomainError, Result, mux::StreamKind, records::Asset};
use nw_platform::{
    files::{self, DirectoryStamp, SourceStamp},
    process::{
        self, ByteInput, InvocationSpec, MediaProcessSession, PreparedRelays, RelayInputId,
        SessionArg, SessionOutput, SessionPolicy, StdoutMode,
    },
};
use std::{ops::Range, path::Path, time::Duration};
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

    /// Mux prepared video and audio transports into a bounded candidate. The
    /// planner must supply the exact held transport inputs. The caller must
    /// verify the candidate's decoded media and timestamps before publication.
    pub async fn mux_normalized_pair_candidate(
        &self,
        inputs: [ByteInput; 2],
        private_root: DirectoryStamp,
        cancel: CancellationToken,
    ) -> Result<SessionOutput> {
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
            cancel,
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
        let output = session.finish().await.map_err(|failure| failure.cause)?;
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
        Ok(output)
    }
}

fn literal_args<const N: usize>(values: [&str; N]) -> [SessionArg; N] {
    values.map(|value| SessionArg::Literal(value.into()))
}
