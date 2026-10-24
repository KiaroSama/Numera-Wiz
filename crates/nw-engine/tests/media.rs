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
    for extension in ["mov", "mp4"] {
        let original = directory
            .path()
            .join(format!("original-two-video.{extension}"));
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
                "-map".into(),
                "0:v:0".into(),
                "-c:v".into(),
                "mpeg4".into(),
                "-map_metadata".into(),
                "-1".into(),
                "-map_metadata:s".into(),
                "-1".into(),
                "-f".into(),
                extension.into(),
                original.to_str().unwrap().into(),
            ],
            std::time::Duration::from_secs(30),
            4096,
            token.clone(),
        )
        .await
        .unwrap();
        let original_asset = engine
            .register(original.clone(), "fixture-owner".into(), token.clone())
            .await
            .unwrap();
        assert_eq!(original_asset.probe.streams.len(), 2);
        let mut unchanged = rules();
        unchanged.audio_selection.selector = Selector::None;
        unchanged.subtitle_selection.selector = Selector::None;
        let same_container = directory.path().join(format!(
            "original-case-copy.{}",
            extension.to_ascii_uppercase()
        ));
        let copy_plan = engine
            .clean_plan(
                original_asset.clone(),
                &same_container,
                &unchanged,
                "fixture-owner",
            )
            .unwrap();
        assert_eq!(copy_plan.resolution.action, CleanAction::CopyUnchanged);
        engine.execute(&copy_plan, token.clone()).await.unwrap();
        assert_eq!(
            std::fs::read(&original).unwrap(),
            std::fs::read(&same_container).unwrap()
        );
        let mut intent = rules();
        intent.audio_selection.selector = Selector::None;
        intent.subtitle_selection.selector = Selector::None;
        intent.preservation.additional_video = false;
        intent.stream_tag_patches = vec![StreamTagPatch {
            kind: StreamKind::Video,
            source_indices: vec![0],
            original_languages: vec![],
            language: TagPatch::Set {
                value: "eng".into(),
            },
            title: TagPatch::Set {
                value: "Reviewed original-container video".into(),
            },
        }];
        let output = directory
            .path()
            .join(format!("original-family-clean.{extension}"));
        let original_plan = engine
            .clean_plan(original_asset, &output, &intent, "fixture-owner")
            .unwrap();
        assert_eq!(original_plan.resolution.action, CleanAction::Remux);
        assert!(
            original_plan
                .argv_template
                .windows(2)
                .any(|args| args == ["-f", extension])
        );
        assert!(
            original_plan
                .verification_scope
                .contains("Original-family MOV/MP4")
        );
        let result = engine.execute(&original_plan, token.clone()).await.unwrap();
        assert!(result.1.contains("Full packet read"));
        let observed = fixture::probe(&media.bin, &output).await;
        assert_eq!(observed["streams"].as_array().unwrap().len(), 1);
        assert_eq!(observed["streams"][0]["codec_name"], "mpeg4");
        assert_eq!(observed["streams"][0]["tags"]["language"], "eng");
        assert_eq!(
            observed["streams"][0]["tags"]["name"],
            "Reviewed original-container video"
        );
        assert_eq!(
            observed["format"]["tags"]["major_brand"],
            original_plan.source.probe.global_tags["major_brand"]
        );
        let second_asset = engine.register(output.clone(), "fixture-owner".into(), token.clone()).await.unwrap();
        assert_eq!(second_asset.probe.streams[0].tags["title"], "Reviewed original-container video");
        assert_eq!(second_asset.probe.stream_details[&0]["tags"]["name"], "Reviewed original-container video");
        let mut second_intent = intent.clone();
        second_intent.stream_tag_patches[0].language = TagPatch::Set { value: "jpn".into() };
        second_intent.stream_tag_patches[0].title = TagPatch::Keep;
        let second_output = directory.path().join(format!("original-family-roundtrip.{extension}"));
        let second_plan = engine.clean_plan(second_asset, &second_output, &second_intent, "fixture-owner").unwrap();
        engine.execute(&second_plan, token.clone()).await.unwrap();
        let second_probe = fixture::probe(&media.bin, &second_output).await;
        assert_eq!(second_probe["streams"][0]["tags"]["language"], "jpn");
        assert_eq!(second_probe["streams"][0]["tags"]["name"], "Reviewed original-container video");
        let mut unsupported = intent.clone();
        unsupported.preservation.stream_tags = false;
        assert_eq!(
            engine
                .clean_plan(
                    original_plan.source.clone(),
                    &directory
                        .path()
                        .join(format!("unqualified-strip.{extension}")),
                    &unsupported,
                    "fixture-owner"
                )
                .unwrap_err()
                .code,
            "INCOMPATIBLE_PRESERVATION",
        );
    }
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
    let publication_links = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let retained_links = publication_links.clone();
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let retained = retained_links.clone();
        Box::pin(async move {
            match event {
                nw_engine::ExecutionEvent::StagingIntent(intent) => {
                    retained
                        .lock()
                        .unwrap()
                        .push(std::path::PathBuf::from(intent.payload));
                }
                nw_engine::ExecutionEvent::Fence(publication) => {
                    retained
                        .lock()
                        .unwrap()
                        .push(std::path::PathBuf::from(publication.proof));
                }
                _ => {}
            }
            Ok(())
        })
    });
    let (artifact, verification, _) = engine
        .execute_observed(&plan, token.clone(), "held-directory-copy", observer)
        .await
        .unwrap();
    {
        let links = publication_links.lock().unwrap();
        assert_eq!(links.len(), 2);
        for link in links.iter() {
            assert!(nw_platform::files::same_object(link, &copy).unwrap());
            assert_eq!(
                std::fs::read(link).unwrap(),
                std::fs::read(&source).unwrap()
            );
        }
    }
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
    assert!(
        plan.argv_template
            .windows(2)
            .any(|pair| pair == ["-i", "fd:"])
    );
    assert!(
        plan.argv_template
            .windows(2)
            .any(|pair| pair == ["-protocol_whitelist", "fd"])
    );
    assert!(!plan.argv_template.contains(&plan.source.identity.path));
    let mut old_route = plan.clone();
    for argument in &mut old_route.argv_template {
        if argument == "fd:" {
            *argument = old_route.source.identity.path.clone();
        }
    }
    for option in ["-fd", "-blocksize"] {
        let index = old_route
            .argv_template
            .iter()
            .position(|argument| argument == option)
            .unwrap();
        old_route.argv_template.drain(index..index + 2);
    }
    let whitelist = old_route
        .argv_template
        .iter()
        .position(|argument| argument == "-protocol_whitelist")
        .unwrap();
    old_route.argv_template[whitelist + 1] = "file".into();
    old_route.hash = nw_engine::plan_digest(&old_route).unwrap();
    assert_eq!(
        engine
            .execute(&old_route, token.clone())
            .await
            .unwrap_err()
            .code,
        "INVALID_PLAN"
    );
    assert!(
        !remux.exists(),
        "A current digest does not reactivate an obsolete pathname-input route"
    );
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
    let persisted_source = directory.path().join("registered-source.mkv");
    std::fs::copy(&source, &persisted_source).unwrap();
    let registered = engine
        .register(
            persisted_source.clone(),
            "fixture-owner".into(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let persisted_output = directory.path().join("registered-copy.mkv");
    let reviewed = engine
        .clean_plan(
            registered.clone(),
            &persisted_output,
            &rules(),
            "fixture-owner",
        )
        .unwrap();
    let retained = directory.path().join("registered-original.mkv");
    std::fs::rename(&persisted_source, &retained).unwrap();
    std::fs::copy(&retained, &persisted_source).unwrap();
    assert_eq!(
        std::fs::read(&retained).unwrap(),
        std::fs::read(&persisted_source).unwrap()
    );
    assert_eq!(
        engine
            .clean_plan(registered, &persisted_output, &rules(), "fixture-owner")
            .unwrap_err()
            .code,
        "SOURCE_CHANGED"
    );
    let restarted_engine = Engine::new(ffmpeg.clone(), ffprobe.clone()).await.unwrap();
    assert_eq!(
        restarted_engine
            .execute(&reviewed, CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "SOURCE_CHANGED"
    );
    assert!(!persisted_output.exists());
    let fresh = restarted_engine
        .register(
            persisted_source.clone(),
            "fixture-owner".into(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(fresh.identity, reviewed.source.identity);
    assert_ne!(fresh.source_object, reviewed.source.source_object);
    let fresh_plan = restarted_engine
        .clean_plan(fresh, &persisted_output, &rules(), "fixture-owner")
        .unwrap();
    assert_ne!(fresh_plan.hash, reviewed.hash);
    restarted_engine
        .execute(&fresh_plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&persisted_source).unwrap(),
        std::fs::read(&persisted_output).unwrap()
    );
    let reviewed_parent = directory.path().join("reviewed-exports");
    std::fs::create_dir(&reviewed_parent).unwrap();
    let parent_output = reviewed_parent.join("original-parent-copy.mkv");
    let parent_plan = engine
        .clean_plan(
            plan.source.clone(),
            &parent_output,
            &rules(),
            "fixture-owner",
        )
        .unwrap();
    let retained_parent = directory.path().join("original-reviewed-exports");
    std::fs::rename(&reviewed_parent, &retained_parent).unwrap();
    let missing_parent = engine
        .execute(&parent_plan, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(missing_parent.code, "TARGET_CHANGED");
    std::fs::create_dir(&reviewed_parent).unwrap();
    let changed_parent = engine
        .execute(&parent_plan, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(changed_parent.code, "TARGET_CHANGED");
    assert!(!parent_output.exists());
    assert!(
        std::fs::read_dir(&reviewed_parent)
            .unwrap()
            .next()
            .is_none()
    );
    assert!(
        std::fs::read_dir(&retained_parent)
            .unwrap()
            .next()
            .is_none()
    );
    let fresh_parent_plan = engine
        .clean_plan(
            plan.source.clone(),
            &parent_output,
            &rules(),
            "fixture-owner",
        )
        .unwrap();
    assert_ne!(fresh_parent_plan.hash, parent_plan.hash);
    engine
        .execute(&fresh_parent_plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&source).unwrap(),
        std::fs::read(&parent_output).unwrap()
    );

    let claim_output = reviewed_parent.join("claim-race.mkv");
    let claim_plan = engine
        .clean_plan(
            plan.source.clone(),
            &claim_output,
            &rules(),
            "fixture-owner",
        )
        .unwrap();
    let fence_entered = std::sync::Arc::new(tokio::sync::Notify::new());
    let resume = std::sync::Arc::new(tokio::sync::Notify::new());
    let entered = fence_entered.clone();
    let resume_first = resume.clone();
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let entered = entered.clone();
        let resume = resume_first.clone();
        Box::pin(async move {
            if matches!(event, nw_engine::ExecutionEvent::Fence(_)) {
                entered.notify_one();
                resume.notified().await;
            }
            Ok(())
        })
    });
    let first_engine = engine.clone();
    let first_plan = claim_plan.clone();
    let first = tokio::spawn(async move {
        first_engine
            .execute_observed(
                &first_plan,
                CancellationToken::new(),
                "claim-owner-a",
                observer,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), fence_entered.notified())
        .await
        .expect("First publication fence deadline");
    let expected_parent = claim_plan
        .required_rights
        .as_ref()
        .unwrap()
        .output_create
        .parent_object
        .as_ref()
        .unwrap();
    let probe = nw_platform::claims::Destination::reviewed(&claim_output, expected_parent).unwrap();
    match nw_platform::claims::DestinationClaims::try_acquire(vec![probe]) {
        Err(error) => assert_eq!(error.code, "PUBLICATION_BUSY"),
        Ok(_) => panic!("The first publication fence must retain its cross-root destination claim"),
    }
    let second_verifying = std::sync::Arc::new(tokio::sync::Notify::new());
    let reached = second_verifying.clone();
    let fences = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let second_fences = fences.clone();
    let observer: nw_engine::Observer = std::sync::Arc::new(move |event| {
        let reached = reached.clone();
        let fences = second_fences.clone();
        Box::pin(async move {
            match event {
                nw_engine::ExecutionEvent::Verifying => reached.notify_one(),
                nw_engine::ExecutionEvent::Fence(_) => {
                    fences.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                }
                _ => {}
            }
            Ok(())
        })
    });
    let second_engine = engine.clone();
    let second_plan = claim_plan.clone();
    let cancellation = CancellationToken::new();
    let second_token = cancellation.clone();
    let second = tokio::spawn(async move {
        second_engine
            .execute_observed(&second_plan, second_token, "claim-owner-b", observer)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        second_verifying.notified(),
    )
    .await
    .expect("Contending verification deadline");
    cancellation.cancel();
    let cancelled = tokio::time::timeout(std::time::Duration::from_secs(5), second)
        .await
        .expect("Contending publication cancellation deadline")
        .unwrap()
        .unwrap_err();
    assert_eq!(cancelled.code, "CANCELLED");
    assert_eq!(fences.load(std::sync::atomic::Ordering::Acquire), 0);
    assert!(!claim_output.exists());
    resume.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(10), first)
        .await
        .expect("Winning publication deadline")
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read(&source).unwrap(),
        std::fs::read(&claim_output).unwrap()
    );

    for (label, mut intent) in [("copy", rules()), ("remux", rules())] {
        if label == "remux" {
            intent.stream_tag_patches = vec![StreamTagPatch {
                kind: StreamKind::Audio,
                source_indices: vec![2],
                original_languages: vec![],
                language: TagPatch::Keep,
                title: TagPatch::Set {
                    value: "Reviewed file times remux".into(),
                },
            }];
        }
        let output = reviewed_parent.join(format!("required-times-{label}.mkv"));
        let time_plan = engine
            .clean_plan_with_file_times_at(
                plan.source.clone(),
                &output,
                &intent,
                nw_domain::file_times::FileTimeRequest::RequireReviewed,
                "fixture-owner",
                nw_platform::epoch_seconds(),
            )
            .unwrap();
        let expected = match time_plan.file_times.as_ref().unwrap() {
            nw_domain::file_times::FileTimePlan::RequireReviewed { times } => times.clone(),
            _ => unreachable!(),
        };
        engine
            .execute(&time_plan, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            nw_platform::file_times::capture_metadata(&std::fs::metadata(&output).unwrap())
                .unwrap(),
            expected,
        );
        if label == "copy" {
            assert_eq!(
                std::fs::read(&source).unwrap(),
                std::fs::read(&output).unwrap()
            );
        }
    }
    let replaced_output = directory.path().join("physical-replacement.mkv");
    let replacement_plan = engine
        .clean_plan(
            plan.source.clone(),
            &replaced_output,
            &clean,
            "fixture-owner",
        )
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
                assert_eq!(
                    std::fs::read(&source).unwrap(),
                    std::fs::read(&retained).unwrap()
                );
            }
            Ok(())
        })
    });
    let rejected = engine
        .execute_observed(
            &replacement_plan,
            CancellationToken::new(),
            "source-replacement-fixture",
            observer,
        )
        .await
        .unwrap_err();
    assert_eq!(rejected.code, "SOURCE_CHANGED");
    assert!(!replaced_output.exists());
    assert_eq!(
        engine.media_process_capacity(),
        (2, 2, 0),
        "No process was spawned; changed-input admission must return capacity"
    );
}
