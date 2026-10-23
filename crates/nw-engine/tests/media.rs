//! Real CPU fixture against the identified CI-built FFmpeg source profile.
use nw_domain::mux::*;
use nw_engine::Engine;
use std::{path::PathBuf, process::Command};
use tokio_util::sync::CancellationToken;
fn rules() -> CleanIntent {
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
#[tokio::test]
#[ignore = "Requires CI-built FFmpeg fixture profile; executed by the hosted media job"]
async fn actual_copy_and_order_metadata_remux() {
    nw_platform::init_logging();
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let bin = PathBuf::from(std::env::var_os("NW_FIXTURE_COMPONENTS").expect("CI component path"));
    let ffmpeg = bin.join("ffmpeg");
    let ffprobe = bin.join("ffprobe");
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mkv");
    let mut fixture = Command::new(&ffmpeg);
    fixture
        .args([
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
        ])
        .arg(&source);
    tracing::info!(argv=?fixture,"fixture.stage.prepared");
    let status = fixture.status().unwrap();
    assert!(status.success());
    let engine = Engine::new(ffmpeg.clone(), ffprobe.clone()).await.unwrap();
    let token = CancellationToken::new();
    let asset = engine
        .register(source.clone(), "fixture-owner".into(), token.clone())
        .await
        .unwrap();
    let copy = directory.path().join("copy.mkv");
    let plan = engine
        .clean_plan(asset.clone(), &copy, &rules(), "fixture-owner")
        .unwrap();
    assert_eq!(plan.resolution.action, CleanAction::CopyUnchanged);
    let (artifact, verification, _) = engine.execute(&plan, token.clone()).await.unwrap();
    assert!(artifact.is_some());
    assert!(verification.contains("Full byte"));
    assert_eq!(
        std::fs::read(&source).unwrap(),
        std::fs::read(&copy).unwrap()
    );
    let mut clean = rules();
    clean.audio_order = vec![2, 1];
    clean.stream_tag_patches = vec![StreamTagPatch {
        kind: StreamKind::Audio,
        source_indices: vec![2],
        original_languages: vec![],
        language: TagPatch::Keep,
        title: TagPatch::Set {
            value: "Persian first".into(),
        },
    }];
    let remux = directory.path().join("remux.mkv");
    let plan = engine
        .clean_plan(asset, &remux, &clean, "fixture-owner")
        .unwrap();
    assert_eq!(plan.resolution.action, CleanAction::Remux);
    engine.execute(&plan, token).await.unwrap();
    let mut observer = Command::new(ffprobe);
    observer
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(remux);
    tracing::info!(argv=?observer,"fixture.oracle.prepared");
    let observed = observer.output().unwrap();
    assert!(observed.status.success());
    let json: serde_json::Value = serde_json::from_slice(&observed.stdout).unwrap();
    let streams = json["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[1]["tags"]["language"], "fas");
    assert_eq!(streams[1]["tags"]["title"], "Persian first");
    assert_eq!(streams[2]["tags"]["language"], "eng");
}
