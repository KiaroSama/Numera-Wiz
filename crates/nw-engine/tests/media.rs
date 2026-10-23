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
    let seek_source = directory.path().join("seek-index-é-索引.mp4");
    nw_platform::process::run(&ffmpeg, &[
        "-hide_banner".into(), "-nostdin".into(), "-v".into(), "error".into(),
        "-i".into(), source.to_str().unwrap().into(), "-map".into(), "0:v:0".into(),
        "-c:v".into(), "mpeg4".into(), "-f".into(), "mp4".into(), seek_source.to_str().unwrap().into(),
    ], std::time::Duration::from_secs(30), 4096, token.clone()).await.unwrap();
    let seek_bytes = std::fs::read(&seek_source).unwrap();
    let mdat = seek_bytes.windows(4).position(|bytes| bytes == b"mdat").unwrap();
    let moov = seek_bytes.windows(4).position(|bytes| bytes == b"moov").unwrap();
    assert!(mdat < moov, "The fixture must require a backward seek after its tail index");
    let seek_asset = engine.register(seek_source.clone(), "fixture-owner".into(), token.clone()).await.unwrap();
    assert_eq!(seek_asset.identity.path, std::fs::canonicalize(&seek_source).unwrap().to_str().unwrap());
    assert_eq!(seek_asset.probe.streams.len(), 1);
    assert_eq!(seek_asset.probe.streams[0].codec, "mpeg4");
    let disguised = directory.path().join("renamed-manifest.mp4");
    std::fs::write(&disguised, b"ffconcat version 1.0\nfile '/unauthorized/sibling.mkv'\n").unwrap();
    assert_eq!(engine.register(disguised, "fixture-owner".into(), token.clone()).await.unwrap_err().code, "INPUT_ROUTE_REQUIRED");
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
    let (_, verification, _) = engine.execute(&plan, token.clone()).await.unwrap();
    assert!(verification.contains("Full packet read"));
    let json = fixture::probe(&media.bin, &remux).await;
    let streams = json["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[1]["tags"]["language"], "fas");
    assert_eq!(streams[1]["tags"]["title"], "Persian first");
    assert_eq!(streams[2]["tags"]["language"], "eng");
    let corrupt = directory.path().join("corrupt.mkv");
    let corrupt_plan = engine.clean_plan(plan.source.clone(), &corrupt, &clean, "fixture-owner").unwrap();
    let staging = std::sync::Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
    let probe = ffprobe.clone();
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let staging = staging.clone();
        let probe = probe.clone();
        Box::pin(async move {
            match event {
                nw_engine::ExecutionEvent::Prepared(args) => {
                    let mut current = staging.lock().unwrap();
                    if current.is_none() { *current = args.last().map(std::path::PathBuf::from); }
                }
                nw_engine::ExecutionEvent::Verifying => {
                    let path = staging.lock().unwrap().clone().unwrap();
                    let result = nw_platform::process::run(&probe, &[
                        "-v".into(), "error".into(), "-show_packets".into(), "-show_entries".into(),
                        "packet=stream_index,pos,size".into(), "-of".into(), "json".into(),
                        path.to_str().unwrap().into(),
                    ], std::time::Duration::from_secs(30), 1024 * 1024, CancellationToken::new()).await.unwrap();
                    let observed: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
                    let packet = observed["packets"].as_array().unwrap().iter().find(|packet| packet["stream_index"] == 1).unwrap();
                    let number = |value: &serde_json::Value| value.as_u64().or_else(|| value.as_str().and_then(|text| text.parse().ok())).unwrap();
                    let offset = number(&packet["pos"]) + number(&packet["size"]) / 2;
                    use std::io::{Read, Seek, SeekFrom, Write};
                    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
                    file.seek(SeekFrom::Start(offset)).unwrap();
                    let mut byte = [0u8]; file.read_exact(&mut byte).unwrap(); byte[0] ^= 1;
                    file.seek(SeekFrom::Start(offset)).unwrap(); file.write_all(&byte).unwrap(); file.sync_all().unwrap();
                }
                _ => {}
            }
            Ok(())
        })
    });
    let rejected = engine.execute_observed(&corrupt_plan, token, "corrupt-fixture", observer).await.unwrap_err();
    assert_eq!(rejected.code, "VERIFICATION_FAILED");
    assert!(rejected.message.contains("packet"));
    assert!(!corrupt.exists(), "Structurally valid changed packet bytes must never publish");
}
