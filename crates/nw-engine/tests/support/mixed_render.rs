use crate::fixture;
use nw_domain::{
    media::{
        AudioLayout, CapturedOccurrence, CapturedTrack, MediaPlanV2, MediaSettings, MixedCapture,
        MixedReview, OccurrenceSource, RawFact, RenderContractV2, bind_mixed_source,
    },
    projects::{self, EntityId, ProjectId, Revision, TrackKind},
    records::Asset,
    time::{Rational, TimeRange},
};
use nw_engine::{Engine, MixedProcessLimits};
use nw_platform::{
    files::{DirectoryStamp, SourceStamp, source_path},
    process,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

const TEST_DEADLINE: Duration = Duration::from_secs(30);

async fn ffmpeg(engine: &Engine, args: &[&str], token: &CancellationToken) -> Vec<u8> {
    let mut command = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-filter_threads",
        "1",
    ]
    .map(str::to_owned)
    .to_vec();
    command.extend(args.iter().map(|arg| (*arg).to_owned()));
    process::run(
        Path::new(&engine.ffmpeg.path),
        &command,
        TEST_DEADLINE,
        512 * 1024,
        token.clone(),
    )
    .await
    .unwrap()
    .stdout
}

fn point(quarter_seconds: i64) -> Rational {
    Rational::new(quarter_seconds, 4).unwrap()
}

fn id() -> EntityId {
    EntityId::parse(uuid::Uuid::new_v4().to_string()).unwrap()
}

fn clip(source: OccurrenceSource, start: i64, end: i64, placement: i64) -> CapturedOccurrence {
    CapturedOccurrence {
        id: id(),
        occurrence_id: id(),
        source_id: id(),
        sequence_range: TimeRange {
            start: point(placement),
            end: point(placement + end - start),
        },
        source_range: TimeRange {
            start: point(start),
            end: point(end),
        },
        rate: point(4),
        source,
    }
}

fn registered(asset: &Asset, stream_index: u32, media_kind: TrackKind) -> OccurrenceSource {
    OccurrenceSource::Registered {
        asset_id: asset.id.clone(),
        content_sha256: asset.identity.sha256.clone(),
        probe_sha256: projects::hash(&serde_json::to_vec(&asset.probe).unwrap()),
        stream_index,
        media_kind,
    }
}

pub(crate) async fn exercise(engine: &Engine, media: &fixture::Media, token: &CancellationToken) {
    let workspace =
        nw_platform::security::private_directory(&media.directory.path().join("mixed-render"))
            .unwrap();
    let directory = DirectoryStamp::capture_resolved(&workspace).unwrap();
    let av = workspace.join("picture-origin.mkv");
    let audio = workspace.join("sound-origin.mkv");
    ffmpeg(
        engine,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x48:rate=4:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=440:r=48000:d=2",
            "-vf",
            "setpts=PTS+1/TB",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
            av.to_str().unwrap(),
        ],
        token,
    )
    .await;
    ffmpeg(
        engine,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=880:r=48000:d=2",
            "-af",
            "asetpts=PTS+2/TB",
            "-c:a",
            "pcm_s16le",
            audio.to_str().unwrap(),
        ],
        token,
    )
    .await;
    let picture = engine
        .register(av.clone(), "fixture".into(), token.clone())
        .await
        .unwrap();
    let sound = engine
        .register(audio.clone(), "fixture".into(), token.clone())
        .await
        .unwrap();
    let track = |kind, order, clips| CapturedTrack {
        id: id(),
        kind,
        order,
        gain_centidb: 0,
        clips,
    };
    let tracks = vec![
        track(
            TrackKind::Video,
            0,
            vec![
                clip(
                    OccurrenceSource::SolidColor {
                        rgb: [0x11, 0x22, 0x33],
                    },
                    0,
                    2,
                    0,
                ),
                clip(registered(&picture, 0, TrackKind::Video), 5, 8, 2),
            ],
        ),
        track(
            TrackKind::Audio,
            1,
            vec![clip(registered(&sound, 0, TrackKind::Audio), 1, 3, 2)],
        ),
        track(
            TrackKind::Audio,
            2,
            vec![clip(registered(&picture, 1, TrackKind::Audio), 0, 2, 3)],
        ),
    ];
    let originals = [&picture, &sound];
    let mut bindings = Vec::new();
    for (clip, asset) in [
        (&tracks[0].clips[1], &picture),
        (&tracks[1].clips[0], &sound),
        (&tracks[2].clips[0], &picture),
    ] {
        bindings.push(bind_mixed_source(clip, asset, "a".repeat(64)).unwrap());
    }
    bindings.sort_by(|a, b| (&a.asset_id, a.stream_index).cmp(&(&b.asset_id, b.stream_index)));
    let sequence = id();
    let fact = |id: EntityId| RawFact {
        sha256: projects::hash(id.as_str().as_bytes()),
        bytes: 36,
        id,
    };
    let mut facts = vec![fact(sequence.clone())];
    for track in &tracks {
        facts.push(fact(track.id.clone()));
        for clip in &track.clips {
            facts.extend([fact(clip.id.clone()), fact(clip.source_id.clone())]);
        }
    }
    facts.sort_by(|a, b| a.id.cmp(&b.id));
    let capture = MixedCapture {
        project_id: ProjectId::parse(uuid::Uuid::new_v4().to_string()).unwrap(),
        project_revision: Revision(4),
        sequence_id: sequence.clone(),
        range: TimeRange {
            start: point(0),
            end: point(8),
        },
        settings: MediaSettings {
            width: 64,
            height: 48,
            frame_rate: point(16),
            sample_rate: 48000,
            audio_layout: AudioLayout::Mono,
        },
        header_sha256: "b".repeat(64),
        header_bytes: 36,
        facts,
        tracks,
    };
    let mut records = capture.clone();
    records.settings.frame_rate = Rational::new(24, 1).unwrap();
    records.tracks[0].clips[0].source = OccurrenceSource::SolidColor { rgb: [0, 0, 0] };
    let edl = nw_domain::interchange::export_cmx3600(&records, &records.tracks[0].id).unwrap();
    assert_eq!(
        edl.document,
        concat!(
            "TITLE: NUMERA WIZ\nFCM: NON-DROP FRAME\n\n",
            "001  BL       V     C        00:00:00:00 00:00:00:12 00:00:00:00 00:00:00:12\n",
            "002  R0000001 V     C        00:00:01:06 00:00:02:00 00:00:00:12 00:00:01:06\n",
            "003  BL       V     C        00:00:00:00 00:00:00:18 00:00:01:06 00:00:02:00\n",
        )
    );
    assert_eq!(edl.reels.len(), 1);
    assert_eq!(edl.reels[0].name, "R0000001");
    assert_eq!(edl.reels[0].asset_id, picture.id);
    assert_eq!(edl.reels[0].stream_index, 0);
    assert!(nw_domain::interchange::export_cmx3600(&capture, &capture.tracks[0].id).is_err());
    let request: RenderContractV2 = serde_json::from_value(json!({
        "contract_kind":"media_v2","sequence_id":sequence,"revision":"4","range":capture.range,
        "output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le","destination":{"kind":"download"}},
    })).unwrap();
    let plan = MediaPlanV2::compile(
        &request,
        capture,
        bindings,
        engine
            .qualify_mixed_compiler(false, token.clone())
            .await
            .unwrap(),
        MixedReview {
            root_id: id().as_str(),
            application_build: &"c".repeat(64),
            principal: "fixture",
            id: id().as_str(),
            now: 10,
        },
    )
    .unwrap();
    let limits = || MixedProcessLimits {
        workspace: &directory,
        max_bytes: 16 * 1024 * 1024,
        deadline: TEST_DEADLINE,
        cancel: token.clone(),
    };
    let mut staged = Vec::new();
    for asset in originals {
        let original = source_path(Path::new(&asset.identity.path)).unwrap();
        let streams = plan
            .payload
            .sources
            .iter()
            .filter(|stream| stream.asset_id == asset.id)
            .cloned()
            .collect::<Vec<_>>();
        staged.push(
            engine
                .stage_mixed_original(
                    asset,
                    SourceStamp::capture_resolved(&original).unwrap(),
                    &streams,
                    limits(),
                )
                .await
                .unwrap(),
        );
    }
    let result = engine
        .verify_mixed_candidate(&plan, &originals, &staged, limits())
        .await
        .unwrap();
    let proof = result.proof();
    assert_eq!((proof.video_frames, proof.audio_samples), (8, 96000));

    let video = ffmpeg(
        engine,
        &[
            "-i",
            av.to_str().unwrap(),
            "-map",
            "0:0",
            "-vf",
            "select=between(n\\,1\\,3)",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "bgra",
            "-c:v",
            "rawvideo",
            "-f",
            "rawvideo",
            "pipe:1",
        ],
        token,
    )
    .await;
    let mut expected_pixels = [0x33, 0x22, 0x11, 255].repeat(2 * 64 * 48);
    expected_pixels.extend(video);
    expected_pixels.extend([0, 0, 0, 255].repeat(3 * 64 * 48));
    assert_eq!(
        proof.video_sha256,
        hex::encode(Sha256::digest(&expected_pixels))
    );

    let mut expected_audio = vec![0_i32; 96000];
    for (path, first, output) in [(&audio, 12000, 24000), (&av, 0, 36000)] {
        let bytes = ffmpeg(
            engine,
            &[
                "-i",
                path.to_str().unwrap(),
                "-map",
                "0:a:0",
                "-c:a",
                "pcm_s16le",
                "-f",
                "s16le",
                "pipe:1",
            ],
            token,
        )
        .await;
        let window = &bytes[2 * first..2 * (first + 24000)];
        for (index, sample) in window.as_chunks::<2>().0.iter().enumerate() {
            expected_audio[output + index] += i32::from(i16::from_le_bytes(*sample));
        }
    }
    let expected_pcm = expected_audio
        .into_iter()
        .flat_map(|value| (value.clamp(-32768, 32767) as i16).to_le_bytes())
        .collect::<Vec<_>>();
    assert_eq!(proof.pcm_sha256, hex::encode(Sha256::digest(&expected_pcm)));
    drop(result);

    let mut gained = plan.payload.capture.clone();
    gained.tracks[1].gain_centidb = -2000;
    let gained = MediaPlanV2::compile(
        &request,
        gained,
        plan.payload.sources.clone(),
        engine
            .qualify_mixed_compiler(true, token.clone())
            .await
            .unwrap(),
        MixedReview {
            root_id: id().as_str(),
            application_build: &"c".repeat(64),
            principal: "fixture",
            id: id().as_str(),
            now: 10,
        },
    )
    .unwrap();
    assert_eq!(gained.payload.graph.audio[0].gain_centidb, -2000);
    let audible = engine
        .verify_mixed_candidate(&gained, &originals, &staged, limits())
        .await
        .unwrap();
    let mut attenuated = vec![0_f32; 96000];
    for (path, first, output, scale) in [(&audio, 12000, 24000, 0.1_f32), (&av, 0, 36000, 1.0)] {
        let bytes = ffmpeg(
            engine,
            &[
                "-i",
                path.to_str().unwrap(),
                "-map",
                "0:a:0",
                "-c:a",
                "pcm_s16le",
                "-f",
                "s16le",
                "pipe:1",
            ],
            token,
        )
        .await;
        for (index, sample) in bytes[2 * first..2 * (first + 24000)]
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
        {
            attenuated[output + index] += f32::from(i16::from_le_bytes(*sample)) * scale;
        }
    }
    let independent = attenuated
        .into_iter()
        .flat_map(|sample| (sample.round_ties_even() as i16).to_le_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        audible.proof().pcm_sha256,
        hex::encode(Sha256::digest(&independent))
    );
    assert_ne!(
        audible.proof().pcm_sha256,
        hex::encode(Sha256::digest(&expected_pcm))
    );
    drop(audible);
    drop(staged);

    let mut capture = plan.payload.capture.clone();
    capture.tracks.clear();
    capture.facts.retain(|fact| fact.id == capture.sequence_id);
    capture.range.end = point(176);
    capture.settings.width = 1920;
    capture.settings.height = 1080;
    capture.settings.frame_rate = point(48);
    let request = RenderContractV2 {
        range: capture.range,
        ..request
    };
    let large = MediaPlanV2::compile(
        &request,
        capture,
        Vec::new(),
        plan.payload.tools.clone(),
        MixedReview {
            root_id: id().as_str(),
            application_build: &"c".repeat(64),
            principal: "fixture",
            id: id().as_str(),
            now: 10,
        },
    )
    .unwrap();
    let verified = engine
        .verify_mixed_candidate(
            &large,
            &[],
            &[],
            MixedProcessLimits {
                workspace: &directory,
                max_bytes: 64 * 1024 * 1024,
                deadline: Duration::from_secs(120),
                cancel: token.clone(),
            },
        )
        .await
        .unwrap();
    let proof = verified.proof();
    assert_eq!(proof.video_frames, 528);
    assert_eq!(
        proof.video_sha256,
        "1f7454531671611117b744f10621867a6ddfba1435b5a8c880fdeb098b10b7c7"
    );
    assert_eq!(proof.audio_samples, 2112000);
}
