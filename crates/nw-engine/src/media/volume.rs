//! Measure one admitted and hash-bound audio signal with the FFmpeg astats filter.
use super::identities;
use crate::{Engine, admission, blocking};
use nw_domain::{
    DomainError, Result,
    audio_analysis::{Decibels, VolumeMeasurement},
    mux::StreamKind,
    records::Asset,
};
use nw_platform::{files::SourceStamp, process};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

const FILTER: &str = "astats=metadata=0:reset=0:measure_perchannel=none:measure_overall=Peak_level+RMS_level+Number_of_samples";

fn level(text: &str) -> Option<Decibels> {
    if text == "-inf" {
        return Some(Decibels::Silence);
    }
    let (negative, magnitude) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let (whole, fraction) = magnitude.split_once('.')?;
    if whole.is_empty()
        || fraction.len() != 6
        || !magnitude
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    let micro_db = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(1_000_000)?
        .checked_add(fraction.parse::<i64>().ok()?)?;
    Some(Decibels::Finite {
        micro_db: if negative { -micro_db } else { micro_db },
    })
}

fn measurements(stderr: &str) -> Option<(Decibels, Decibels, u64)> {
    let mut rows = stderr.lines().rev().filter_map(|row| {
        let (logger, message) = row
            .strip_prefix("[Parsed_astats_0 @ 0x")?
            .split_once("] ")?;
        if logger.is_empty() || !logger.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some((logger, message))
    });
    let (logger, samples) = rows.next()?;
    let (rms_logger, rms) = rows.next()?;
    let (peak_logger, peak) = rows.next()?;
    let (header_logger, header) = rows.next()?;
    if logger != rms_logger
        || logger != peak_logger
        || logger != header_logger
        || header != "Overall"
    {
        return None;
    }
    let samples = samples
        .strip_prefix("Number of samples: ")?
        .parse::<u64>()
        .ok()?;
    if samples == 0 {
        return None;
    }
    Some((
        level(peak.strip_prefix("Peak level dB: ")?)?,
        level(rms.strip_prefix("RMS level dB: ")?)?,
        samples,
    ))
}

impl Engine {
    pub async fn measure_volume(
        &self,
        asset: &Asset,
        source: SourceStamp,
        stream_index: u32,
        cancel: CancellationToken,
    ) -> Result<VolumeMeasurement> {
        let admission::RegisteredInput {
            input,
            source,
            expected,
            length,
        } = admission::registered_stream(
            asset,
            source,
            stream_index,
            StreamKind::Audio,
            cancel.clone(),
        )
        .await?;
        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-v",
            "info",
            "-xerror",
            "-max_alloc",
            "67108864",
            "-threads",
            "1",
            "-filter_threads",
            "1",
        ]
        .map(str::to_owned)
        .to_vec();
        args.extend(input.options());
        args.extend([
            "-i".to_owned(),
            "fd:".to_owned(),
            "-map".to_owned(),
            format!("0:{stream_index}"),
        ]);
        args.extend(["-af", FILTER, "-map_metadata", "-1", "-f", "null", "-"].map(str::to_owned));
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone())
            .await
            .and_then(|_| source.revalidate())
        {
            return ticket.finish(Err(error));
        }
        let report = process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(45),
            1024,
            input.file,
            cancel.clone(),
        )
        .await;
        let report = ticket.finish(report)?;
        let (peak, rms, samples) = measurements(&report.stderr_tail).ok_or_else(|| {
            DomainError::new(
                "MEASUREMENT_UNAVAILABLE",
                "FFmpeg returned no complete volume measurement",
            )
        })?;
        identities(self, cancel.clone()).await?;
        let asset_id = asset.id.clone();
        blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during volume measurement",
                ));
            }
            Ok(VolumeMeasurement {
                asset_id,
                source_sha256: expected.sha256,
                stream_index,
                peak,
                rms,
                samples_per_channel: samples.to_string(),
            })
        })
        .await
    }
}
