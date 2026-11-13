//! Observe actual identified components through the shared supervised process budget.
pub(super) mod execution;
mod verification;
use super::{Engine, blocking};
use nw_domain::{
    DomainError, Result,
    media::{ComponentIdentity, ENCODERS, FILTERS, MUXERS, MediaTools},
};
use nw_platform::{files, process};
use std::{collections::BTreeSet, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) async fn qualify(engine: &Engine, cancel: CancellationToken) -> Result<MediaTools> {
    let phase = cancel.child_token();
    let work = observe(engine, phase.clone());
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        _ = tokio::time::sleep(Duration::from_secs(15)) => {
            phase.cancel();
            // Drive the same supervisor to confirmed reap; dropping it would quarantine capacity.
            let result = work.await;
            if let Err(error) = result
                && matches!(error.code.as_str(), "TREE_TERMINATION_UNCERTAIN" | "PROCESS_WAIT_FAILED" | "PROCESS_CAPTURE_FAILED" | "PROCESS_LAUNCH_FAILED")
            {
                return Err(error);
            }
            Err(DomainError::new("PROCESS_TIMEOUT", "Media qualification exceeded its15-second total deadline"))
        }
    }
}
async fn identities(engine: &Engine, cancel: CancellationToken) -> Result<()> {
    let ffmpeg = engine.ffmpeg.clone();
    let ffprobe = engine.ffprobe.clone();
    blocking(move || {
        for identity in [&ffmpeg, &ffprobe] {
            files::revalidate(identity, &cancel).map_err(|error| {
                if error.code == "CANCELLED" {
                    error
                } else {
                    DomainError::new(
                        "COMPONENT_CHANGED",
                        "Media executable differs from the Engine's frozen identity",
                    )
                }
            })?;
        }
        Ok(())
    })
    .await
}
async fn observe(engine: &Engine, cancel: CancellationToken) -> Result<MediaTools> {
    identities(engine, cancel.clone()).await?;
    for (flag, required, kind) in [
        ("-filters", FILTERS, Listing::Filters),
        ("-encoders", ENCODERS, Listing::Encoders),
        ("-muxers", MUXERS, Listing::Muxers),
    ] {
        let args = ["-hide_banner", flag].map(String::from);
        let ticket = engine.process_budget.acquire(&cancel).await?;
        let result = process::run_with_output_limits(
            Path::new(&engine.ffmpeg.path),
            &args,
            Duration::from_secs(5),
            256 * 1024,
            4096,
            cancel.clone(),
        )
        .await;
        let result = ticket.finish(result)?;
        let stdout = std::str::from_utf8(&result.stdout).map_err(|_| {
            DomainError::new(
                "COMPONENT_UNAVAILABLE",
                "Media component listing must be bounded UTF-8",
            )
        })?;
        let names = names(stdout, kind);
        if required.iter().any(|name| !names.contains(*name)) {
            return Err(DomainError::new(
                "COMPONENT_UNAVAILABLE",
                format!("Identified FFmpeg lacks the exact required {flag} component names"),
            ));
        }
    }
    identities(engine, cancel.clone()).await?;
    if cancel.is_cancelled() {
        return Err(DomainError::new(
            "CANCELLED",
            "Media qualification cancelled",
        ));
    }
    let tools = MediaTools {
        ffmpeg: ComponentIdentity {
            sha256: engine.ffmpeg.sha256.clone(),
            bytes: engine.ffmpeg.bytes.clone(),
        },
        ffprobe: ComponentIdentity {
            sha256: engine.ffprobe.sha256.clone(),
            bytes: engine.ffprobe.bytes.clone(),
        },
        filters: FILTERS.iter().map(|name| (*name).into()).collect(),
        encoders: ENCODERS.iter().map(|name| (*name).into()).collect(),
        muxers: MUXERS.iter().map(|name| (*name).into()).collect(),
    };
    tools.validate()?;
    Ok(tools)
}
#[derive(Clone, Copy)]
enum Listing {
    Filters,
    Encoders,
    Muxers,
}
fn names(text: &str, kind: Listing) -> BTreeSet<&str> {
    text.lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let flags = columns.next()?;
            let name = columns.next()?;
            let description = columns.next()?;
            if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return None;
            }
            let admitted = match kind {
                Listing::Filters => {
                    // FFmpeg9 emits two flags; retain the older three-flag format too.
                    matches!(flags.len(), 2 | 3)
                        && flags.bytes().all(|b| b".TSC".contains(&b))
                        && description.contains("->")
                        && description.bytes().all(|b| b"AVN|->".contains(&b))
                }
                Listing::Encoders => {
                    flags.len() == 6
                        && matches!(flags.as_bytes()[0], b'V' | b'A' | b'S')
                        && flags
                            .bytes()
                            .skip(1)
                            .all(|b| b == b'.' || b.is_ascii_uppercase())
                }
                Listing::Muxers => matches!(flags, "E" | "DE"),
            };
            admitted.then_some(name)
        })
        .collect()
}

#[cfg(test)]
pub(super) fn assert_listing_admission() {
    // Two flag columns follow the pinned FFmpeg9 show_filters output, including
    // sources and dynamic input counts. Header/description text is never a name.
    let filters = names(
        "Filters:\n  T.. = Timeline support\n  .S. = Slice threading\n  ------\n .. color             |->V       Provide an opaque color input.\n TS trim              V->V       Pick a finite range.\n .. concat            N->N       Concatenate inputs.\n ... setpts           V->V       Legacy three-flag listing.\n .. made_up           invalid    color trim asetpts\n color appears only in prose\n",
        Listing::Filters,
    );
    assert_eq!(
        filters,
        BTreeSet::from(["color", "trim", "concat", "setpts"])
    );
    assert!(!filters.contains("asetpts"));
    let encoders = names(
        "Encoders:\n V..... = Video\n VF.... ffv1                 FFmpeg video codec #1\n A....D pcm_s16le            PCM signed 16-bit little-endian\n ...... invented             ffv1\n",
        Listing::Encoders,
    );
    assert_eq!(encoders, BTreeSet::from(["ffv1", "pcm_s16le"]));
    let muxers = names(
        "Muxers:\n .E = Muxing supported\n  E  matroska        Matroska\n D   input_only      Matroska\n",
        Listing::Muxers,
    );
    assert_eq!(muxers, BTreeSet::from(["matroska"]));
    verification::exercise();
}
