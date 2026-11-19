//! Decode a single registered frame through the owned CPU process supervisor.
use super::identities;
use crate::{Engine, admission, blocking};
use nw_domain::{
    DomainError, Result, media::BoundSource, mux::StreamKind, preview, projects::TrackKind,
    records::Asset, time::Rational,
};
use nw_platform::{files::SourceStamp, process};
use std::{io::Cursor, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

enum FrameSelection {
    Index(u32),
    OriginalTick(i64),
}

#[derive(Clone, Copy)]
enum OutputKind {
    Raw,
    Png,
}

pub struct ObservedPng {
    pub bytes: Vec<u8>,
    pub timing: Option<preview::FrameTiming>,
    pub graph_sha256: String,
}

fn timing_unavailable() -> DomainError {
    DomainError::new(
        "FRAME_UNAVAILABLE",
        "The emitted frame's original timing could not be verified",
    )
}

fn frame_timing(log: &str) -> Result<preview::FrameTiming> {
    let mut clock = None;
    let mut frame = None;
    for line in log.lines() {
        let Some(line) = line
            .strip_prefix("[Parsed_showinfo_1 @ ")
            .and_then(|line| line.split_once("] "))
            .map(|(_, text)| text)
        else {
            continue;
        };
        if let Some(time_base) = line.strip_prefix("config in time_base: ") {
            let (ratio, _) = time_base.split_once(',').ok_or_else(timing_unavailable)?;
            let (num, den) = ratio.split_once('/').ok_or_else(timing_unavailable)?;
            let ratio = Rational::new(
                num.parse().map_err(|_| timing_unavailable())?,
                den.parse().map_err(|_| timing_unavailable())?,
            )?;
            if ratio <= Rational::ZERO || clock.replace(ratio).is_some() {
                return Err(timing_unavailable());
            }
        } else if let Some(body) = line.strip_prefix("n:")
            && frame.replace(body).is_some()
        {
            return Err(timing_unavailable());
        }
    }
    let clock = clock.ok_or_else(timing_unavailable)?;
    let (ordinal, fields) = frame
        .and_then(|line| line.split_once(" pts:"))
        .ok_or_else(timing_unavailable)?;
    let (pts, fields) = fields
        .split_once(" pts_time:")
        .ok_or_else(timing_unavailable)?;
    let (_, fields) = fields
        .split_once(" duration:")
        .ok_or_else(timing_unavailable)?;
    let (duration, _) = fields
        .split_once(" duration_time:")
        .ok_or_else(timing_unavailable)?;
    if ordinal.trim() != "0" {
        return Err(timing_unavailable());
    }
    let timestamp = |ticks: &str| -> Result<Rational> {
        let ticks = ticks.trim().parse().map_err(|_| timing_unavailable())?;
        clock.checked_mul(Rational::new(ticks, 1)?)
    };
    let pts = (pts.trim() != "NOPTS")
        .then(|| timestamp(pts))
        .transpose()?;
    let duration = timestamp(duration)?;
    if duration < Rational::ZERO {
        return Err(timing_unavailable());
    }
    Ok(preview::FrameTiming {
        time_base: clock,
        pts,
        duration: (duration > Rational::ZERO).then_some(duration),
    })
}

struct PreviewBuffer {
    bytes: Vec<u8>,
    timing: Option<preview::FrameTiming>,
    graph_sha256: Option<String>,
}

impl Engine {
    pub async fn preview_registered_frame(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        frame: u32,
        cancel: CancellationToken,
    ) -> Result<Vec<u8>> {
        Ok(self
            .registered_frame(asset, source, stream_index, frame, OutputKind::Raw, cancel)
            .await?
            .bytes)
    }

    pub async fn observe_registered_png(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        frame: u32,
        cancel: CancellationToken,
    ) -> Result<ObservedPng> {
        let result = self
            .registered_frame(asset, source, stream_index, frame, OutputKind::Png, cancel)
            .await?;
        Ok(ObservedPng {
            bytes: result.bytes,
            timing: result.timing,
            graph_sha256: result.graph_sha256.ok_or_else(timing_unavailable)?,
        })
    }

    async fn registered_frame(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        frame: u32,
        format: OutputKind,
        cancel: CancellationToken,
    ) -> Result<PreviewBuffer> {
        if frame > preview::MAX_FRAME {
            return Err(DomainError::new(
                "INVALID_RANGE",
                "Preview frame exceeds the supported index bound",
            ));
        }
        self.preview_selected_frame(
            asset,
            source,
            stream_index,
            FrameSelection::Index(frame),
            format,
            cancel,
        )
        .await
    }

    pub async fn preview_bound_frame(
        &self,
        asset: &Asset,
        source: SourceStamp,
        binding: &BoundSource,
        at: Rational,
        cancel: CancellationToken,
    ) -> Result<Vec<u8>> {
        if binding.media_kind != TrackKind::Video
            || binding.asset_id != asset.id
            || binding.content_sha256 != asset.identity.sha256
            || binding.time_base <= Rational::ZERO
            || at < Rational::ZERO
        {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Timeline frame requires the bound original video stream",
            ));
        }
        let point = binding.asset_origin.checked_add(at)?;
        let tick = point.checked_mul(Rational::new(
            binding.time_base.denominator(),
            binding.time_base.numerator(),
        )?)?;
        if point < binding.stream_start || tick.denominator() != 1 || tick.numerator() == i64::MAX {
            return Err(DomainError::new(
                "FRAME_UNAVAILABLE",
                "No exact original video tick covers the selected timeline point",
            ));
        }
        Ok(self
            .preview_selected_frame(
                asset,
                source,
                binding.stream_index,
                FrameSelection::OriginalTick(tick.numerator()),
                OutputKind::Raw,
                cancel,
            )
            .await?
            .bytes)
    }

    async fn preview_selected_frame(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        selection: FrameSelection,
        format: OutputKind,
        cancel: CancellationToken,
    ) -> Result<PreviewBuffer> {
        let phase = cancel.child_token();
        let request = self.decode_preview(
            asset,
            source,
            stream_index,
            selection,
            format,
            phase.clone(),
        );
        tokio::pin!(request);
        tokio::select! {
            result = &mut request => result,
            () = tokio::time::sleep(Duration::from_secs(15)) => {
                phase.cancel();
                match request.await {
                    Err(error) if error.code != "CANCELLED" => Err(error),
                    _ if cancel.is_cancelled() => Err(DomainError::new("CANCELLED", "Preview request cancelled")),
                    _ => Err(DomainError::new("PROCESS_TIMEOUT", "Preview exceeded its total budget")),
                }
            }
        }
    }

    async fn decode_preview(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        selection: FrameSelection,
        format: OutputKind,
        cancel: CancellationToken,
    ) -> Result<PreviewBuffer> {
        let admission::RegisteredInput {
            input,
            source,
            expected,
            length,
        } = admission::registered_stream(
            asset,
            source,
            stream_index,
            StreamKind::Video,
            cancel.clone(),
        )
        .await?;
        let still_image = asset.probe.is_still_image();
        if still_image && !matches!(selection, FrameSelection::Index(0)) {
            return Err(DomainError::new(
                "FRAME_UNAVAILABLE",
                "A registered still has only source frame zero",
            ));
        }
        let (width, height) = (preview::WIDTH, preview::HEIGHT);
        let (preserve_clock, trim) = match selection {
            FrameSelection::Index(frame) => (
                false,
                format!("trim=start_frame={frame}:end_frame={}", frame + 1),
            ),
            FrameSelection::OriginalTick(tick) => {
                (true, format!("trim=start_pts={tick}:end_pts={}", tick + 1))
            }
        };
        let scale = if still_image {
            // The RGBA display can represent a one-pixel span in very narrow stills.
            format!(
                "scale=w='max(1,min({width},round({height}*dar)))':h='max(1,min({height},round({width}/dar)))':reset_sar=1"
            )
        } else {
            format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2:reset_sar=1"
            )
        };
        let observe_time = matches!(format, OutputKind::Png) && !still_image;
        let showinfo = if observe_time {
            ",showinfo=checksum=0"
        } else {
            ""
        };
        // PNG can carry source EXIF and ICC side data. The bounded monitor
        // image carries the prepared pixels and encoder-generated signaling.
        let remove_side_data = if matches!(format, OutputKind::Png) {
            ",sidedata=mode=delete"
        } else {
            ""
        };
        let filters = format!("{trim}{showinfo},setpts=PTS-STARTPTS,{scale},pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,format=rgba{remove_side_data}");
        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            if observe_time { "info" } else { "error" },
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
        if observe_time {
            args.push("-nostats".into());
        }
        if preserve_clock || observe_time {
            args.push("-copyts".into());
        }
        if still_image {
            args.extend(["-max_pixels", admission::MAX_STILL_PIXELS].map(String::from));
        }
        args.extend(input.options());
        args.extend([
            "-i".into(),
            "fd:".into(),
            "-map".into(),
            format!("0:{stream_index}"),
            "-vf".into(),
            filters,
        ]);
        let codec = match format {
            OutputKind::Raw => "rawvideo",
            OutputKind::Png => "png",
        };
        args.extend(
            ["-frames:v", "1", "-c:v", codec, "-threads", "1", "-pix_fmt", "rgba"]
                .map(String::from),
        );
        let (container, output_budget) = match format {
            OutputKind::Raw => ("rawvideo", preview::RGBA_BYTES),
            OutputKind::Png => {
                args.extend(
                    ["-map_metadata", "-1", "-map_chapters", "-1", "-fps_mode", "passthrough"]
                        .map(String::from),
                );
                ("image2pipe", preview::MAX_INLINE_PNG_BYTES)
            }
        };
        args.extend(["-f", container, "pipe:1"].map(String::from));
        let graph_sha256 = if matches!(format, OutputKind::Png) {
            let graph = serde_json::to_vec(&args).map_err(|_| {
                DomainError::new("INVALID_PLAN", "Observation graph identity is unavailable")
            })?;
            Some(nw_domain::projects::hash(&graph))
        } else {
            None
        };
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone())
            .await
            .and_then(|_| source.revalidate())
        {
            return ticket.finish(Err(error));
        }
        let result = process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(7),
            output_budget,
            input.file,
            cancel.clone(),
        )
        .await;
        let output = ticket.finish(result)?;
        match format {
            OutputKind::Raw if output.stdout.len() != preview::RGBA_BYTES => {
                return Err(DomainError::new(
                    "FRAME_UNAVAILABLE",
                    "The registered stream returned no complete preview frame",
                ));
            }
            OutputKind::Png => verify_png(&output.stdout, &cancel)?,
            OutputKind::Raw => {}
        }
        // Timing evidence must include the entire bounded diagnostic, including
        // any early filter graph reinitialization that could reset frame counts.
        if observe_time && output.diagnostic_bytes > 64 * 1024 {
            return Err(timing_unavailable());
        }
        let timing = observe_time
            .then(|| frame_timing(&output.stderr_tail))
            .transpose()?;
        identities(self, cancel.clone()).await?;
        blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during frame preview",
                ));
            }
            Ok(PreviewBuffer {
                bytes: output.stdout,
                timing,
                graph_sha256,
            })
        })
        .await
    }
}

fn verify_png(bytes: &[u8], cancel: &CancellationToken) -> Result<()> {
    let invalid = || {
        DomainError::new(
            "FRAME_UNAVAILABLE",
            "The encoder returned no complete preview PNG",
        )
    };
    let width = (preview::WIDTH as u32).to_be_bytes();
    let height = (preview::HEIGHT as u32).to_be_bytes();
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        || bytes.get(16..20) != Some(width.as_slice())
        || bytes.get(20..24) != Some(height.as_slice())
        || bytes.get(24..26) != Some(b"\x08\x06".as_slice())
        || !bytes.ends_with(b"\0\0\0\0IEND\xaeB\x60\x82")
    {
        return Err(invalid());
    }
    admission::png_chunks(&mut Cursor::new(bytes), bytes.len() as u64, cancel).map_err(|error| {
        if error.code == "CANCELLED" {
            error
        } else {
            invalid()
        }
    })
}
