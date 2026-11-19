//! Decode a single registered frame through the owned CPU process supervisor.
use super::identities;
use crate::{Engine, admission, blocking};
use nw_domain::{
    DomainError, Result, media::BoundSource, mux::StreamKind, preview, projects::TrackKind,
    records::Asset, time::Rational,
};
use nw_platform::{files::SourceStamp, process};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

enum FrameSelection {
    Index(u32),
    OriginalTick(i64),
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
        self.preview_selected_frame(
            asset,
            source,
            binding.stream_index,
            FrameSelection::OriginalTick(tick.numerator()),
            cancel,
        )
        .await
    }

    async fn preview_selected_frame(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        selection: FrameSelection,
        cancel: CancellationToken,
    ) -> Result<Vec<u8>> {
        let phase = cancel.child_token();
        let request = self.decode_preview(asset, source, stream_index, selection, phase.clone());
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
        cancel: CancellationToken,
    ) -> Result<Vec<u8>> {
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
        let filters = format!(
            "{trim},setpts=PTS-STARTPTS,{scale},pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,format=rgba"
        );
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
        if preserve_clock {
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
        args.extend(
            [
                "-frames:v",
                "1",
                "-c:v",
                "rawvideo",
                "-threads",
                "1",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ]
            .map(String::from),
        );
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
            preview::RGBA_BYTES,
            input.file,
            cancel.clone(),
        )
        .await;
        let pixels = ticket.finish(result)?.stdout;
        if pixels.len() != preview::RGBA_BYTES {
            return Err(DomainError::new(
                "FRAME_UNAVAILABLE",
                "The registered stream returned no complete preview frame",
            ));
        }
        identities(self, cancel.clone()).await?;
        blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during frame preview",
                ));
            }
            Ok(pixels)
        })
        .await
    }
}
