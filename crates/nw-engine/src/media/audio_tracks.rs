//! Compare hashes over the selected decoded PCM streams.
use super::identities;
use crate::{Engine, admission, blocking};
use nw_domain::{
    DomainError, Result,
    audio_analysis::{AudioSignature, AudioTrackReport, MAX_TRACKS},
    mux::StreamKind,
    records::Asset,
};
use nw_platform::{files::SourceStamp, process};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn unavailable() -> DomainError {
    DomainError::new(
        "MEASUREMENT_UNAVAILABLE",
        "The selected audio tracks do not have complete decoded signatures",
    )
}

fn track_signatures(asset: &Asset, indices: &[u32], stdout: &[u8]) -> Result<Vec<AudioSignature>> {
    let observations = std::str::from_utf8(stdout).map_err(|_| unavailable())?;
    if observations.lines().count() != indices.len() {
        return Err(unavailable());
    }
    observations
        .lines()
        .zip(indices)
        .enumerate()
        .map(|(output_index, (row, index))| {
            let checksum = row
                .strip_prefix(&format!("{output_index},a,SHA256="))
                .filter(|value| {
                    value.len() == 64
                        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                        && *value
                            != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                })
                .ok_or_else(unavailable)?;
            let format = asset
                .probe
                .stream_details
                .get(index)
                .ok_or_else(unavailable)?;
            if !matches!(
                format["sample_fmt"].as_str(),
                Some(
                    "u8" | "u8p"
                        | "s16"
                        | "s16p"
                        | "s32"
                        | "s32p"
                        | "flt"
                        | "fltp"
                        | "dbl"
                        | "dblp"
                )
            ) {
                return Err(unavailable());
            }
            let channels = format["channels"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(unavailable)?;
            let sample_rate = format["sample_rate"]
                .as_str()
                .and_then(|rate| rate.parse::<u32>().ok())
                .filter(|rate| *rate > 0)
                .ok_or_else(unavailable)?;
            let channel_layout = format["channel_layout"]
                .as_str()
                .filter(|layout| !matches!(*layout, "" | "unknown" | "unspecified"))
                .or(match channels {
                    1 => Some("mono"),
                    2 => Some("stereo"),
                    _ => None,
                })
                .map(str::to_owned);
            Ok(AudioSignature {
                stream_index: *index,
                channels,
                sample_rate,
                channel_layout,
                decoded_pcm_sha256: checksum.to_ascii_lowercase(),
            })
        })
        .collect()
}

fn duplicate_groups(tracks: &[AudioSignature]) -> Vec<Vec<u32>> {
    let mut by_signal = BTreeMap::new();
    for track in tracks {
        if let Some(layout) = &track.channel_layout {
            by_signal
                .entry((
                    track.sample_rate,
                    track.channels,
                    layout,
                    &track.decoded_pcm_sha256,
                ))
                .or_insert_with(Vec::new)
                .push(track.stream_index);
        }
    }
    let mut groups: Vec<_> = by_signal
        .into_values()
        .filter(|indices| indices.len() > 1)
        .collect();
    for group in &mut groups {
        group.sort_unstable();
    }
    groups.sort_unstable();
    groups
}

impl Engine {
    pub async fn compare_audio_tracks(
        &self,
        asset: &Asset,
        source: SourceStamp,
        indices: &[u32],
        cancel: CancellationToken,
    ) -> Result<AudioTrackReport> {
        if !(2..=MAX_TRACKS).contains(&indices.len())
            || indices.iter().collect::<BTreeSet<_>>().len() != indices.len()
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Select between two and sixteen distinct original audio streams",
            ));
        }
        if !indices.iter().all(|index| {
            asset
                .probe
                .streams
                .iter()
                .any(|s| s.index == *index && s.kind == StreamKind::Audio)
        }) {
            return Err(DomainError::new(
                "INPUT_DENIED",
                "Select only registered audio streams",
            ));
        }
        let admission::RegisteredInput {
            input,
            source,
            expected,
            length,
        } = admission::registered_stream(
            asset,
            source,
            indices[0],
            StreamKind::Audio,
            cancel.clone(),
        )
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
        ]
        .map(str::to_owned)
        .to_vec();
        args.extend(input.options());
        args.extend(["-i".to_owned(), "fd:".to_owned()]);
        for index in indices {
            args.extend(["-map".to_owned(), format!("0:{index}")]);
        }
        args.extend(
            [
                "-c:a",
                "pcm_f64le",
                "-threads",
                "1",
                "-f",
                "streamhash",
                "-hash",
                "sha256",
                "pipe:1",
            ]
            .map(str::to_owned),
        );
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone())
            .await
            .and_then(|_| source.revalidate())
        {
            return ticket.finish(Err(error));
        }
        let output = process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(45),
            4096,
            input.file,
            cancel.clone(),
        )
        .await;
        let observations = ticket.finish(output)?.stdout;
        let tracks = track_signatures(asset, indices, &observations)?;
        let duplicate_groups = duplicate_groups(&tracks);
        identities(self, cancel.clone()).await?;
        let asset_id = asset.id.clone();
        blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during audio comparison",
                ));
            }
            Ok(AudioTrackReport {
                asset_id,
                source_sha256: expected.sha256,
                tracks,
                duplicate_groups,
            })
        })
        .await
    }
}
