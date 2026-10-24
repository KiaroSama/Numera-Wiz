//! Real CPU fixture against the identified CI-built FFmpeg source profile.
use nw_domain::mux::*;
use nw_engine::Engine;
use tokio_util::sync::CancellationToken;
#[path = "../../../test-support/media.rs"]
mod fixture;
use fixture::rules;
fn observed_tag<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    value
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))?
        .1
        .as_str()
}
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
    nw_platform::process::run(
        &ffmpeg,
        &[
            "-hide_banner".into(),
            "-nostdin".into(),
            "-v".into(),
            "error".into(),
            "-i".into(),
            source.to_str().unwrap().into(),
            "-map".into(),
            "0:v:0".into(),
            "-c:v".into(),
            "mpeg4".into(),
            "-f".into(),
            "mp4".into(),
            seek_source.to_str().unwrap().into(),
        ],
        std::time::Duration::from_secs(30),
        4096,
        token.clone(),
    )
    .await
    .unwrap();
    let seek_bytes = std::fs::read(&seek_source).unwrap();
    let mdat = seek_bytes
        .windows(4)
        .position(|bytes| bytes == b"mdat")
        .unwrap();
    let moov = seek_bytes
        .windows(4)
        .position(|bytes| bytes == b"moov")
        .unwrap();
    assert!(
        mdat < moov,
        "The fixture must require a backward seek after its tail index"
    );
    let seek_asset = engine
        .register(seek_source.clone(), "fixture-owner".into(), token.clone())
        .await
        .unwrap();
    assert_eq!(
        seek_asset.identity.path,
        std::fs::canonicalize(&seek_source)
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(seek_asset.probe.streams.len(), 1);
    assert_eq!(seek_asset.probe.streams[0].codec, "mpeg4");
    let seek_remux = directory.path().join("seek-index-remux.mkv");
    let mut seek_intent = rules();
    seek_intent.audio_selection.no_match = NoMatch::AllowEmpty;
    seek_intent.preservation.global_tags = false;
    let seek_plan = engine
        .clean_plan(seek_asset, &seek_remux, &seek_intent, "fixture-owner")
        .unwrap();
    assert_eq!(seek_plan.resolution.action, CleanAction::Remux);
    engine.execute(&seek_plan, token.clone()).await.unwrap();
    let observed = fixture::probe(&media.bin, &seek_remux).await;
    assert_eq!(observed["streams"][0]["codec_name"], "mpeg4");
    let disguised = directory.path().join("renamed-manifest.mp4");
    std::fs::write(
        &disguised,
        b"ffconcat version 1.0\nfile '/unauthorized/sibling.mkv'\n",
    )
    .unwrap();
    assert_eq!(
        engine
            .register(disguised, "fixture-owner".into(), token.clone())
            .await
            .unwrap_err()
            .code,
        "INPUT_ROUTE_REQUIRED"
    );
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
    assert!(plan.argv_template.windows(2).any(|pair| pair == ["-i", "fd:"]));
    assert!(plan.argv_template.windows(2).any(|pair| pair == ["-protocol_whitelist", "fd"]));
    assert!(!plan.argv_template.contains(&plan.source.identity.path));
    let mut old_route = plan.clone();
    for argument in &mut old_route.argv_template {
        if argument == "fd:" { *argument = old_route.source.identity.path.clone(); }
    }
    for option in ["-fd", "-blocksize"] {
        let index = old_route.argv_template.iter().position(|argument| argument == option).unwrap();
        old_route.argv_template.drain(index..index + 2);
    }
    let whitelist = old_route.argv_template.iter().position(|argument| argument == "-protocol_whitelist").unwrap();
    old_route.argv_template[whitelist + 1] = "file".into();
    old_route.hash = nw_engine::plan_digest(&old_route).unwrap();
    assert_eq!(engine.execute(&old_route, token.clone()).await.unwrap_err().code, "INVALID_PLAN");
    assert!(!remux.exists(), "A current digest does not reactivate an obsolete pathname-input route");
    let (_, verification, _) = engine.execute(&plan, token.clone()).await.unwrap();
    assert!(verification.contains("Full packet read"));
    let json = fixture::probe(&media.bin, &remux).await;
    let streams = json["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[1]["tags"]["language"], "fas");
    assert_eq!(streams[1]["tags"]["title"], "Persian first");
    assert_eq!(streams[2]["tags"]["language"], "eng");
    assert_eq!(
        observed_tag(&streams[2]["tags"], "review_note"),
        Some("PRESERVE-ME-STREAM-TAG")
    );
    assert_eq!(
        observed_tag(&json["format"]["tags"], "artist"),
        Some("Fixture artist")
    );
    assert!(
        observed_tag(&json["format"]["tags"], "creation_time")
            .unwrap()
            .starts_with("2024-01-02T03:04:05")
    );
    let stripped = directory.path().join("stripped.mkv");
    let mut strip = clean.clone();
    strip.preservation.stream_tags = false;
    strip.preservation.global_tags = false;
    let strip_plan = engine
        .clean_plan(plan.source.clone(), &stripped, &strip, "fixture-owner")
        .unwrap();
    engine.execute(&strip_plan, token.clone()).await.unwrap();
    let stripped_probe = fixture::probe(&media.bin, &stripped).await;
    for stream in stripped_probe["streams"].as_array().unwrap() {
        assert!(observed_tag(&stream["tags"], "review_note").is_none());
    }
    assert!(observed_tag(&stripped_probe["format"]["tags"], "artist").is_none());
    assert!(observed_tag(&stripped_probe["format"]["tags"], "creation_time").is_none());
    let changed_tag = directory.path().join("changed-tag.mkv");
    let tag_plan = engine
        .clean_plan(plan.source.clone(), &changed_tag, &clean, "fixture-owner")
        .unwrap();
    let staged_tag = std::sync::Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let staged = staged_tag.clone();
        Box::pin(async move {
            match event {
                nw_engine::ExecutionEvent::Prepared(args) => {
                    let mut path = staged.lock().unwrap();
                    if path.is_none() {
                        *path = args.last().map(std::path::PathBuf::from);
                    }
                }
                nw_engine::ExecutionEvent::Verifying => {
                    let path = staged.lock().unwrap().clone().unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let marker = b"PRESERVE-ME-STREAM-TAG";
                    let position = bytes
                        .windows(marker.len())
                        .position(|bytes| bytes == marker)
                        .unwrap();
                    use std::io::{Seek, SeekFrom, Write};
                    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
                    file.seek(SeekFrom::Start(position as u64)).unwrap();
                    file.write_all(b"X").unwrap();
                    file.sync_all().unwrap();
                }
                _ => {}
            }
            Ok(())
        })
    });
    let rejected = engine
        .execute_observed(&tag_plan, token.clone(), "tag-fixture", observer)
        .await
        .unwrap_err();
    assert_eq!(rejected.code, "VERIFICATION_FAILED");
    assert!(rejected.message.contains("review_note"));
    assert!(
        !changed_tag.exists(),
        "A changed authored tag must fail even when every media packet is unchanged"
    );
    let corrupt = directory.path().join("corrupt.mkv");
    let corrupt_plan = engine
        .clean_plan(plan.source.clone(), &corrupt, &clean, "fixture-owner")
        .unwrap();
    let staging = std::sync::Arc::new(std::sync::Mutex::new(None::<std::path::PathBuf>));
    let probe = ffprobe.clone();
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let staging = staging.clone();
        let probe = probe.clone();
        Box::pin(async move {
            match event {
                nw_engine::ExecutionEvent::StagingIntent(intent) => {
                    assert!(
                        !std::path::Path::new(&intent.directory).exists(),
                        "Durable staging intent must precede workspace creation"
                    );
                    assert!(!std::path::Path::new(&intent.payload).exists());
                }
                nw_engine::ExecutionEvent::Prepared(args) => {
                    let mut current = staging.lock().unwrap();
                    if current.is_none() {
                        *current = args.last().map(std::path::PathBuf::from);
                    }
                }
                nw_engine::ExecutionEvent::Verifying => {
                    let path = staging.lock().unwrap().clone().unwrap();
                    let result = nw_platform::process::run(
                        &probe,
                        &[
                            "-v".into(),
                            "error".into(),
                            "-show_packets".into(),
                            "-show_entries".into(),
                            "packet=stream_index,pos,size".into(),
                            "-of".into(),
                            "json".into(),
                            path.to_str().unwrap().into(),
                        ],
                        std::time::Duration::from_secs(30),
                        1024 * 1024,
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                    let observed: serde_json::Value =
                        serde_json::from_slice(&result.stdout).unwrap();
                    let packet = observed["packets"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|packet| packet["stream_index"] == 1)
                        .unwrap();
                    let number = |value: &serde_json::Value| {
                        value
                            .as_u64()
                            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                            .unwrap()
                    };
                    let offset = number(&packet["pos"]) + number(&packet["size"]) / 2;
                    use std::io::{Read, Seek, SeekFrom, Write};
                    let mut file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(path)
                        .unwrap();
                    file.seek(SeekFrom::Start(offset)).unwrap();
                    let mut byte = [0u8];
                    file.read_exact(&mut byte).unwrap();
                    byte[0] ^= 1;
                    file.seek(SeekFrom::Start(offset)).unwrap();
                    file.write_all(&byte).unwrap();
                    file.sync_all().unwrap();
                }
                _ => {}
            }
            Ok(())
        })
    });
    let rejected = engine
        .execute_observed(&corrupt_plan, token, "corrupt-fixture", observer)
        .await
        .unwrap_err();
    assert_eq!(rejected.code, "VERIFICATION_FAILED");
    assert!(rejected.message.contains("packet"));
    assert!(
        !corrupt.exists(),
        "Structurally valid changed packet bytes must never publish"
    );
    let replaced_output = directory.path().join("physical-replacement.mkv");
    let replacement_plan = engine
        .clean_plan(plan.source.clone(), &replaced_output, &clean, "fixture-owner")
        .unwrap();
    let original_source = source.clone();
    let retained_source = directory.path().join("retained-original.mkv");
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let source = original_source.clone();
        let retained = retained_source.clone();
        Box::pin(async move {
            if let nw_engine::ExecutionEvent::Prepared(_) = event {
                std::fs::rename(&source, &retained).unwrap();
                std::fs::copy(&retained, &source).unwrap();
                assert_eq!(std::fs::read(&source).unwrap(), std::fs::read(&retained).unwrap());
            }
            Ok(())
        })
    });
    let rejected = engine
        .execute_observed(&replacement_plan, CancellationToken::new(), "source-replacement-fixture", observer)
        .await.unwrap_err();
    assert_eq!(rejected.code, "SOURCE_CHANGED");
    assert!(!replaced_output.exists());
    assert_eq!(engine.media_process_capacity(), (2, 2, 0), "No process was spawned; changed-input admission must return capacity");
}
