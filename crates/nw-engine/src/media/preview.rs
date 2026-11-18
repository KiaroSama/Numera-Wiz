//! Decode a single registered frame through the owned CPU process supervisor.
use super::identities;
use crate::{Engine, admission, blocking};
use nw_domain::{DomainError, Result, mux::StreamKind, preview, records::Asset};
use nw_platform::{files::SourceStamp, process};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

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
        let phase = cancel.child_token();
        let request = self.decode_preview(asset, source, stream_index, frame, phase.clone());
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
        frame: u32,
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
        let (width, height) = (preview::WIDTH, preview::HEIGHT);
        let filters = format!(
            "trim=start_frame={frame}:end_frame={},setpts=PTS-STARTPTS,scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2:reset_sar=1,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,format=rgba",
            frame + 1,
        );
        let mut args = [
            "-hide_banner", "-nostdin", "-v", "error", "-xerror", "-max_alloc", "67108864",
            "-threads", "1", "-filter_threads", "1",
        ]
        .map(String::from)
        .to_vec();
        args.extend(input.options());
        args.extend([
            "-i".into(),
            "fd:".into(),
            "-map".into(),
            format!("0:{stream_index}"),
            "-vf".into(),
            filters,
        ]);
        args.extend([
            "-frames:v", "1", "-c:v", "rawvideo", "-threads", "1", "-pix_fmt", "rgba",
            "-f", "rawvideo", "pipe:1",
        ].map(String::from));
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
