//! Real CPU fixture against the identified CI-built FFmpeg source profile.
use nw_domain::mux::*;
use nw_engine::Engine;
use tokio_util::sync::CancellationToken;
#[path = "../../../test-support/media.rs"]
mod fixture;
use fixture::rules;
#[tokio::test]
#[ignore = "Requires CI-built FFmpeg fixture profile; executed by the hosted media job"]
async fn actual_copy_and_order_metadata_remux() {
    nw_platform::init_logging();
    let media = fixture::generate().await;
    let source = media.source.clone();
    let directory = &media.directory;
    let ffmpeg = media.bin.join("ffmpeg");
    let ffprobe = media.bin.join("ffprobe");
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
    let mut changed_engine = engine.clone();
    changed_engine.ffmpeg.sha256 = "different-component".into();
    assert_eq!(
        changed_engine
            .execute(&plan, token.clone())
            .await
            .unwrap_err()
            .code,
        "COMPONENT_CHANGED"
    );
    assert!(!copy.exists());
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
    let json = fixture::probe(&media.bin, &remux).await;
    let streams = json["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[1]["tags"]["language"], "fas");
    assert_eq!(streams[1]["tags"]["title"], "Persian first");
    assert_eq!(streams[2]["tags"]["language"], "eng");
}
