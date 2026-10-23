//! CI-only deterministic media construction and an independent bounded ffprobe oracle.
use nw_domain::mux::*;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;
pub struct Media {
    pub directory: tempfile::TempDir,
    pub source: PathBuf,
    pub bin: PathBuf,
}
pub fn rules() -> CleanIntent {
    CleanIntent {
        audio_selection: Selection {
            selector: Selector::All,
            no_match: NoMatch::SkipFile,
        },
        subtitle_selection: Selection {
            selector: Selector::All,
            no_match: NoMatch::AllowEmpty,
        },
        audio_order: vec![],
        subtitle_order: vec![],
        stream_tag_patches: vec![],
        preservation: Preservation::default(),
    }
}
pub async fn generate() -> Media {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let bin = PathBuf::from(std::env::var_os("NW_FIXTURE_COMPONENTS").expect("CI component path"));
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mkv");
    let mut args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x48:rate=4:duration=1",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=1:sample_rate=48000",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=880:duration=1:sample_rate=48000",
        "-map",
        "0:v:0",
        "-map",
        "1:a:0",
        "-map",
        "2:a:0",
        "-c:v",
        "ffv1",
        "-c:a",
        "pcm_s16le",
        "-metadata:s:a:0",
        "language=eng",
        "-metadata:s:a:1",
        "language=fas",
        "-metadata",
        "title=Fixture",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(source.to_str().unwrap().into());
    nw_platform::process::run(
        &bin.join("ffmpeg"),
        &args,
        std::time::Duration::from_secs(30),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    Media {
        directory,
        source,
        bin,
    }
}
pub async fn probe(bin: &Path, path: &Path) -> serde_json::Value {
    let mut args = [
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(path.to_str().unwrap().into());
    let observed = nw_platform::process::run(
        &bin.join("ffprobe"),
        &args,
        std::time::Duration::from_secs(30),
        4 * 1024 * 1024,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    serde_json::from_slice(&observed.stdout).unwrap()
}
