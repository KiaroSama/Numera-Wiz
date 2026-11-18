//! Inspect component declarations and exercise fixed encode/decode paths.
use crate::{Engine, blocking, media};
use nw_domain::{
    DomainError, Result,
    capabilities::{
        AdvertisedComponents, CapabilityReport, ClearResult, NvencObservations, Observation,
        RefreshInput,
    },
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

#[derive(Clone, Copy)]
enum VideoProbe {
    Ffv1,
    H264Nvenc,
    HevcNvenc,
}

impl VideoProbe {
    fn codecs(self) -> (&'static str, &'static str) {
        match self {
            Self::Ffv1 => ("ffv1", "ffv1"),
            Self::H264Nvenc => ("h264_nvenc", "h264"),
            Self::HevcNvenc => ("hevc_nvenc", "hevc"),
        }
    }
}

impl Engine {
    pub async fn inspect_capabilities(
        &self,
        cancel: CancellationToken,
    ) -> Result<CapabilityReport> {
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
        input: RefreshInput,
        cancel: CancellationToken,
    ) -> Result<CapabilityReport> {
        let report = if input.components {
            self.refresh_components(input.force, cancel.clone()).await?
        } else {
            self.inspect_capabilities(cancel.clone()).await?
        };
        if input.devices {
            self.refresh_nvenc(report, cancel).await
        } else {
            Ok(report)
        }
    }

    async fn refresh_components(
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
        let hardware_accelerators =
            listing("-hwaccels", media::Listing::AcceleratorMethods).await?;
        report.cpu_round_trip = if encoders.contains("ffv1")
            && encoders.contains("rawvideo")
            && decoders.contains("ffv1")
            && filters.contains("color")
            && demuxers.contains("matroska")
            && muxers.contains("matroska")
            && muxers.contains("rawvideo")
        {
            self.observe_video(VideoProbe::Ffv1, cancel.clone()).await?
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

    async fn refresh_nvenc(
        &self,
        mut report: CapabilityReport,
        cancel: CancellationToken,
    ) -> Result<CapabilityReport> {
        let phase = cancel.child_token();
        let work = self.observe_nvenc(phase.clone());
        tokio::pin!(work);
        let nvenc = tokio::select! {
            result = &mut work => result?,
            () = tokio::time::sleep(Duration::from_secs(23)) => {
                phase.cancel();
                return match work.await {
                    Err(error) if error.code != "CANCELLED" => Err(error),
                    _ if cancel.is_cancelled() => Err(DomainError::new("CANCELLED", "NVENC probe cancelled")),
                    _ => Err(DomainError::new("PROCESS_TIMEOUT", "NVENC probe exceeded its total budget")),
                };
            }
        };
        let observations = [nvenc.h264_nvenc, nvenc.hevc_nvenc];
        report.hardware = if observations.contains(&Observation::Exercised) {
            Observation::Exercised
        } else if observations.contains(&Observation::Failed) {
            Observation::Failed
        } else {
            Observation::Unavailable
        };
        // Device and driver state can change independently of the executable digests.
        report.nvenc = Some(nvenc);
        Ok(report)
    }

    async fn observe_nvenc(&self, cancel: CancellationToken) -> Result<NvencObservations> {
        media::identities(self, cancel.clone()).await?;
        let encoders =
            media::listing(self, "-encoders", media::Listing::Codecs, cancel.clone()).await?;
        let mut observations = NvencObservations {
            h264_nvenc: Observation::Unavailable,
            hevc_nvenc: Observation::Unavailable,
        };
        for (probe, result) in [
            (VideoProbe::H264Nvenc, &mut observations.h264_nvenc),
            (VideoProbe::HevcNvenc, &mut observations.hevc_nvenc),
        ] {
            if encoders.contains(probe.codecs().0) {
                *result = self.observe_video(probe, cancel.clone()).await?;
            }
        }
        media::identities(self, cancel).await?;
        Ok(observations)
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
            nvenc: None,
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

    async fn observe_video(
        &self,
        probe: VideoProbe,
        cancel: CancellationToken,
    ) -> Result<Observation> {
        match self.exercise_video(probe, cancel).await {
            Ok(true) => Ok(Observation::Exercised),
            Ok(false) => Ok(Observation::Failed),
            Err(error) if matches!(error.code.as_str(), "PROCESS_FAILED" | "PROCESS_TIMEOUT") => {
                Ok(Observation::Failed)
            }
            Err(error) => Err(error),
        }
    }

    async fn exercise_video(&self, probe: VideoProbe, cancel: CancellationToken) -> Result<bool> {
        let ffmpeg = Path::new(&self.ffmpeg.path);
        let (encoder, decoder) = probe.codecs();
        let (source, luma_bytes, tolerance, encode_limit, decode_deadline) = match probe {
            VideoProbe::Ffv1 => ("color=black:s=16x16:r=1:d=1", 16 * 16, 0, 32 * 1024, 5),
            VideoProbe::H264Nvenc | VideoProbe::HevcNvenc => {
                ("color=black:s=256x256:r=1:d=1", 256 * 256, 2, 512 * 1024, 3)
            }
        };
        let encode = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            source,
            "-frames:v",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            encoder,
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
            encode_limit,
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
            "-c:v",
            decoder,
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
        let chroma_bytes = luma_bytes / 2;
        let decoded = process::run_readonly(
            ffmpeg,
            &decode,
            Duration::from_secs(decode_deadline),
            luma_bytes + chroma_bytes,
            input,
            cancel,
        )
        .await;
        let planes = ticket.finish(decoded)?.stdout;
        Ok(
            matches!(planes.split_at_checked(luma_bytes), Some((luma, chroma))
            if chroma.len() == chroma_bytes
                && luma.iter().all(|sample| sample.abs_diff(16) <= tolerance)
                && chroma.iter().all(|sample| sample.abs_diff(128) <= tolerance)),
        )
    }
}
