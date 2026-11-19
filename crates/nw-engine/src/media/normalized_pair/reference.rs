//! Compare equal presentation grids with fixed FFmpeg RGB metric filters.
use super::{NormalizationPolicy, NormalizedWindow, StreamSignature, single_stream};
use crate::{Engine, blocking, media::identities};
use nw_domain::{
    DomainError, Result,
    mux::StreamKind,
    records::{Asset, FileIdentity},
    reference_metrics::{CompareInput, ComparedWindow, Comparison, FrameScores, Psnr, SampleBasis},
    time::Rational,
};
use nw_platform::{
    files::{self, SourceStamp},
    process, security,
};
use serde_json::Value;
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const WINDOW_BYTES: usize = 8 * 1024 * 1024;
const WINDOW_POLICY: NormalizationPolicy = NormalizationPolicy {
    stdout_bytes: WINDOW_BYTES,
    require_stable_video_format: true,
};
const CONTAINER_RESERVE: u64 = 64 * 1024;
const GRAPH: &str = concat!(
    "[0:v]format=gbrp,split=2[rp][rs];",
    "[1:v]format=gbrp,split=2[cp][cs];",
    "[cp][rp]psnr=stats_file=-:shortest=1:repeatlast=0[ps];",
    "[cs][rs]ssim=stats_file=-:shortest=1:repeatlast=0[ss]",
);

fn misaligned(message: &str) -> DomainError {
    DomainError::new("REFERENCE_ALIGNMENT_REQUIRED", message)
}

fn geometry(reference: &Value, candidate: &Value, frames: u32) -> Result<(u32, u32)> {
    for field in [
        "width",
        "height",
        "sample_aspect_ratio",
        "color_range",
        "color_space",
        "color_transfer",
        "color_primaries",
        "field_order",
    ] {
        if reference.get(field) != candidate.get(field) {
            return Err(misaligned(
                "The registered dimensions, aspect ratio, color tags or field order differ",
            ));
        }
    }
    let dimension = |name: &str| {
        reference[name]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|dimension| *dimension != 0)
            .ok_or_else(|| misaligned("Both registered streams need known video dimensions"))
    };
    let width = dimension("width")?;
    let height = dimension("height")?;
    if width < 8 || height < 8 {
        return Err(misaligned("SSIM requires video at least 8 × 8 pixels"));
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(u64::from(frames) * 4));
    if bytes.is_none_or(|bytes| bytes > WINDOW_BYTES as u64 - CONTAINER_RESERVE) {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "The decoded comparison windows exceed the 8 MiB per-source budget",
        ));
    }
    Ok((width, height))
}

fn check_alignment(
    reference: &StreamSignature,
    candidate: &StreamSignature,
    count: usize,
) -> Result<Vec<Rational>> {
    if reference.format != candidate.format
        || reference.packets.len() != count
        || candidate.packets.len() != count
        || !reference
            .packets
            .iter()
            .zip(&candidate.packets)
            .all(|(left, right)| (left.0, left.1) == (right.0, right.1))
        || reference.packets.first().map(|packet| packet.0) != Some(Rational::ZERO)
    {
        return Err(misaligned(
            "Both normalized windows must contain every requested frame on the same relative presentation grid",
        ));
    }
    Ok(reference.packets.iter().map(|packet| packet.0).collect())
}

fn decimal(text: &str, digits: usize) -> Option<i32> {
    let (negative, number) = text
        .strip_prefix('-')
        .map_or((false, text), |value| (true, value));
    let (whole, fraction) = number.split_once('.')?;
    if whole.is_empty()
        || fraction.len() != digits
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let magnitude = whole
        .parse::<i32>()
        .ok()?
        .checked_mul(10_i32.pow(digits as u32))?
        .checked_add(fraction.parse::<i32>().ok()?)?;
    Some(if negative { -magnitude } else { magnitude })
}

fn parse_scores(bytes: &[u8], count: usize) -> Option<Vec<(Psnr, i32)>> {
    let mut psnr = (0..count).map(|_| None).collect::<Vec<_>>();
    let mut ssim = vec![None; count];
    for line in std::str::from_utf8(bytes).ok()?.lines() {
        let mut values = line.split_ascii_whitespace();
        let frame = values
            .next()?
            .strip_prefix("n:")?
            .parse::<usize>()
            .ok()?
            .checked_sub(1)?;
        let fields = values.collect::<Vec<_>>();
        if let Some(value) = fields
            .iter()
            .find_map(|field| field.strip_prefix("psnr_avg:"))
        {
            let score = if value == "inf" {
                Psnr::Identical
            } else {
                Psnr::Finite {
                    centi_db: decimal(value, 2)?,
                }
            };
            if psnr.get_mut(frame)?.replace(score).is_some() {
                return None;
            }
        } else if let Some(value) = fields.iter().find_map(|field| field.strip_prefix("All:")) {
            let score = decimal(value, 6)?;
            if !(-1_000_000..=1_000_000).contains(&score)
                || ssim.get_mut(frame)?.replace(score).is_some()
            {
                return None;
            }
        } else {
            return None;
        }
    }
    psnr.into_iter()
        .zip(ssim)
        .map(|(psnr, ssim)| Some((psnr?, ssim?)))
        .collect()
}

fn stage(
    scratch: &Path,
    windows: [Vec<u8>; 2],
    cancel: &CancellationToken,
) -> Result<(tempfile::TempDir, [FileIdentity; 2])> {
    let root = security::private_directory(scratch)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix("reference-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir_in(root).map_err(files::io_error)?;
    let paths = ["reference.nut", "candidate.nut"].map(|name| directory.path().join(name));
    for (path, bytes) in paths.iter().zip(windows) {
        security::write_private(path, &bytes, false)?;
    }
    Ok((
        directory,
        [
            files::identity(&paths[0], cancel)?,
            files::identity(&paths[1], cancel)?,
        ],
    ))
}

impl Engine {
    pub async fn compare_video_windows(
        &self,
        input: &CompareInput,
        assets: [&Asset; 2],
        stamps: [SourceStamp; 2],
        scratch: PathBuf,
        cancel: CancellationToken,
    ) -> Result<Comparison> {
        let count = input.frame_count as usize;
        let [reference_window, candidate_window] = input.frame_ranges()?;
        let [reference, candidate] = assets;
        let detail = |asset: &Asset, stream| {
            asset
                .probe
                .stream_details
                .get(&stream)
                .cloned()
                .ok_or_else(|| misaligned("A selected stream has no registered video properties"))
        };
        let (width, height) = geometry(
            &detail(reference, input.reference.stream_index)?,
            &detail(candidate, input.candidate.stream_index)?,
            input.frame_count,
        )?;
        let [reference_stamp, candidate_stamp] = stamps;
        let reference_bytes = self
            .normalize_window(
                reference,
                reference_stamp,
                input.reference.stream_index,
                NormalizedWindow::VideoFrames(reference_window),
                WINDOW_POLICY,
                cancel.clone(),
            )
            .await?;
        let candidate_bytes = self
            .normalize_window(
                candidate,
                candidate_stamp,
                input.candidate.stream_index,
                NormalizedWindow::VideoFrames(candidate_window),
                WINDOW_POLICY,
                cancel.clone(),
            )
            .await?;
        let (_directory, staged) = blocking({
            let cancel = cancel.clone();
            move || stage(&scratch, [reference_bytes, candidate_bytes], &cancel)
        })
        .await?;
        let mut declarations = Vec::with_capacity(2);
        for identity in &staged {
            let file = File::open(&identity.path).map_err(files::io_error)?;
            let report = self.inspect_nut(file, cancel.clone()).await?;
            let signature = single_stream(&report, StreamKind::Video)?;
            if signature.format["width"] != width || signature.format["height"] != height {
                return Err(misaligned(
                    "Decoded window geometry differs from the registered dimensions",
                ));
            }
            declarations.push(signature);
        }
        let timestamps = check_alignment(&declarations[0], &declarations[1], count)?;
        let scores = self.measure_quality(&staged, count, cancel.clone()).await?;
        identities(self, cancel.clone()).await?;
        let originals = assets.map(|asset| (asset.identity.clone(), asset.source_object.clone()));
        blocking(move || {
            for identity in &staged {
                files::revalidate(identity, &cancel)?;
            }
            for (identity, object) in originals {
                let stamp = SourceStamp::capture_resolved(Path::new(&identity.path))?;
                stamp.require_object(
                    object
                        .as_ref()
                        .ok_or_else(|| misaligned("Missing original file identity"))?,
                )?;
                files::revalidate(&identity, &cancel)?;
                stamp.revalidate()?;
            }
            Ok(())
        })
        .await?;
        let frames = scores
            .into_iter()
            .zip(timestamps)
            .enumerate()
            .map(|(index, ((psnr, ssim_millionths), offset))| FrameScores {
                reference_frame: input.reference.start_frame + index as u64,
                candidate_frame: input.candidate.start_frame + index as u64,
                offset,
                psnr,
                ssim_millionths,
            })
            .collect();
        Ok(Comparison {
            reference: ComparedWindow {
                selection: input.reference.clone(),
                source_sha256: reference.identity.sha256.clone(),
            },
            candidate: ComparedWindow {
                selection: input.candidate.clone(),
                source_sha256: candidate.identity.sha256.clone(),
            },
            sample_basis: SampleBasis::DecodedRgb8,
            width,
            height,
            frames,
        })
    }

    async fn measure_quality(
        &self,
        staged: &[FileIdentity; 2],
        count: usize,
        cancel: CancellationToken,
    ) -> Result<Vec<(Psnr, i32)>> {
        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-xerror",
            "-max_alloc",
            "67108864",
            "-filter_complex_threads",
            "1",
        ]
        .map(String::from)
        .to_vec();
        for input in staged {
            args.extend(
                [
                    "-protocol_whitelist",
                    "file",
                    "-format_whitelist",
                    "nut",
                    "-f",
                    "nut",
                ]
                .map(String::from),
            );
            args.extend(["-i".to_owned(), input.path.clone()]);
        }
        args.extend(
            [
                "-filter_complex",
                GRAPH,
                "-map",
                "[ps]",
                "-map",
                "[ss]",
                "-c:v",
                "rawvideo",
                "-threads",
                "1",
                "-f",
                "null",
                "-",
            ]
            .map(String::from),
        );
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone()).await {
            return ticket.finish(Err(error));
        }
        let result = process::run(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(15),
            8192,
            cancel,
        )
        .await;
        let output = ticket.finish(result)?;
        parse_scores(&output.stdout, count).ok_or_else(|| {
            DomainError::new(
                "MEASUREMENT_UNAVAILABLE",
                "FFmpeg did not report both quality metrics for every aligned frame",
            )
        })
    }
}
