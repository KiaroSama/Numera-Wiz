//! Inspect component declarations and verify a fixed software encode/decode path.
use crate::{Engine, blocking, media};
use nw_domain::{
    DomainError, Result,
    capabilities::{AdvertisedComponents, CapabilityReport, ClearResult, Observation},
    media::ComponentIdentity,
    records::FileIdentity,
};
use nw_platform::{files::io_error, process};
use std::{
    io::{Seek, Write},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

impl Engine {
    pub async fn inspect_capabilities(&self, cancel: CancellationToken) -> Result<CapabilityReport> {
        let cache = self.lock_capabilities(&cancel).await?;
        media::identities(self, cancel).await?;
        let current = self.empty_capability_report();
        Ok(cache
            .as_ref()
            .filter(|report| report.fingerprint == current.fingerprint)
            .cloned()
            .unwrap_or(current))
    }

    pub async fn refresh_capabilities(
        &self,
        force: bool,
        cancel: CancellationToken,
    ) -> Result<CapabilityReport> {
        let phase = cancel.child_token();
        let refresh = self.observe_capabilities(force, phase.clone());
        tokio::pin!(refresh);
        tokio::select! {
            result = &mut refresh => result,
            () = tokio::time::sleep(Duration::from_secs(15)) => {
                phase.cancel();
                match refresh.await {
                    Err(error) if error.code != "CANCELLED" => Err(error),
                    _ => Err(DomainError::new("PROCESS_TIMEOUT", "Capability refresh exceeded its 15-second budget")),
                }
            }
        }
    }

    async fn observe_capabilities(
        &self,
        force: bool,
        cancel: CancellationToken,
    ) -> Result<CapabilityReport> {
        let mut cache = self.lock_capabilities(&cancel).await?;
        media::identities(self, cancel.clone()).await?;
        let mut report = self.empty_capability_report();
        if !force
            && let Some(cached) = cache.as_ref()
            && cached.fingerprint == report.fingerprint
        {
            return Ok(cached.clone());
        }
        let listing = |flag: &'static str, kind| media::listing(self, flag, kind, cancel.clone());
        let encoders = listing("-encoders", media::Listing::Codecs).await?;
        let decoders = listing("-decoders", media::Listing::Codecs).await?;
        let filters = listing("-filters", media::Listing::Filters).await?;
        let demuxers = listing("-demuxers", media::Listing::Demuxers).await?;
        let muxers = listing("-muxers", media::Listing::Muxers).await?;
        let pixel_formats = listing("-pix_fmts", media::Listing::PixelFormats).await?;
        let sample_formats = listing("-sample_fmts", media::Listing::SampleFormats).await?;
        let hardware_accelerators = listing("-hwaccels", media::Listing::AcceleratorMethods).await?;
        report.cpu_round_trip = if encoders.contains("ffv1")
            && encoders.contains("rawvideo")
            && decoders.contains("ffv1")
            && filters.contains("color")
            && demuxers.contains("matroska")
            && muxers.contains("matroska")
            && muxers.contains("rawvideo")
        {
            match self.exercise_software(cancel.clone()).await {
                Ok(true) => Observation::Exercised,
                Ok(false) => Observation::Failed,
                Err(error) if matches!(error.code.as_str(), "PROCESS_FAILED" | "PROCESS_TIMEOUT") => {
                    Observation::Failed
                }
                Err(error) => return Err(error),
            }
        } else {
            Observation::Unavailable
        };
        report.advertised = Some(AdvertisedComponents {
            encoders: encoders.into_iter().collect(),
            decoders: decoders.into_iter().collect(),
            filters: filters.into_iter().collect(),
            demuxers: demuxers.into_iter().collect(),
            muxers: muxers.into_iter().collect(),
            pixel_formats: pixel_formats.into_iter().collect(),
            sample_formats: sample_formats.into_iter().collect(),
            hardware_accelerators: hardware_accelerators.into_iter().collect(),
        });
        media::identities(self, cancel).await?;
        // A transient failure gives the caller evidence without poisoning later refreshes.
        *cache = (report.cpu_round_trip != Observation::Failed).then(|| report.clone());
        Ok(report)
    }

    pub async fn clear_capabilities(
        &self,
        fingerprint: &str,
        cancel: CancellationToken,
    ) -> Result<ClearResult> {
        let mut cache = self.lock_capabilities(&cancel).await?;
        let cleared = cache
            .as_ref()
            .is_some_and(|report| report.fingerprint == fingerprint);
        if cleared {
            *cache = None;
        }
        Ok(ClearResult { cleared })
    }

    fn empty_capability_report(&self) -> CapabilityReport {
        let fingerprint = nw_domain::projects::hash(
            format!(
                "numera-component-report-v1:{}:{}:{}:{}",
                std::env::consts::OS,
                std::env::consts::ARCH,
                self.ffmpeg.sha256,
                self.ffprobe.sha256
            )
            .as_bytes(),
        );
        let identity = |file: &FileIdentity| ComponentIdentity {
            sha256: file.sha256.clone(),
            bytes: file.bytes.clone(),
        };
        CapabilityReport {
            fingerprint,
            ffmpeg: identity(&self.ffmpeg),
            ffprobe: identity(&self.ffprobe),
            advertised: None,
            cpu_round_trip: Observation::Unknown,
            hardware: Observation::Unknown,
        }
    }

    async fn lock_capabilities(
        &self,
        cancel: &CancellationToken,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<CapabilityReport>>> {
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(DomainError::new("CANCELLED", "Capability request cancelled")),
            guard = self.component_capabilities.lock() => Ok(guard),
        }
    }

    async fn exercise_software(&self, cancel: CancellationToken) -> Result<bool> {
        let ffmpeg = Path::new(&self.ffmpeg.path);
        let encode = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=black:s=16x16:r=1:d=1",
            "-frames:v",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "ffv1",
            "-threads",
            "1",
            "-f",
            "matroska",
            "pipe:1",
        ]
        .map(String::from);
        let ticket = self.process_budget.acquire(&cancel).await?;
        let encoded = process::run_with_output_limits(
            ffmpeg,
            &encode,
            Duration::from_secs(5),
            32 * 1024,
            4096,
            cancel.clone(),
        )
        .await;
        let bytes = ticket.finish(encoded)?.stdout;
        let input = blocking(move || {
            let mut input = tempfile::tempfile().map_err(io_error)?;
            input.write_all(&bytes).map_err(io_error)?;
            input.rewind().map_err(io_error)?;
            Ok(input)
        })
        .await?;
        let decode = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "fd",
            "-fd",
            "0",
            "-f",
            "matroska",
            "-i",
            "fd:",
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "rawvideo",
            "-threads",
            "1",
            "-f",
            "rawvideo",
            "pipe:1",
        ]
        .map(String::from);
        let ticket = self.process_budget.acquire(&cancel).await?;
        let decoded =
            process::run_readonly(ffmpeg, &decode, Duration::from_secs(5), 1024, input, cancel)
                .await;
        let planes = ticket.finish(decoded)?.stdout;
        Ok(matches!(planes.split_at_checked(256), Some((luma, chroma))
            if chroma.len() == 128
                && luma.iter().all(|sample| *sample == 16)
                && chroma.iter().all(|sample| *sample == 128)))
    }
}
