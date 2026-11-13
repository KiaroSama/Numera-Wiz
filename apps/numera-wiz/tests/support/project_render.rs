//! A genuine finite public project compiler review using the actual hosted component profile.
use super::owner::Cli;
use nw_domain::commands::{ProjectRef, Request};
use serde_json::{Value, json};

async fn edit(
    cli: &Cli,
    scope: &str,
    project: &str,
    revision: &str,
    key: &str,
    id: &str,
    params: Value,
) -> Value {
    let mut request = Request::query(id, params);
    request.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: Some(revision.into()),
    });
    request.replay_scope = Some(scope.into());
    request.idempotency_key = Some(key.into());
    let response = cli.raw(&request).await;
    assert_eq!(
        response["ok"], true,
        "Public compiler fixture edit failed: {}",
        response["error"]["code"]
    );
    assert!(response["receipt_id"].is_string());
    response["result"].clone()
}

pub async fn review(cli: &Cli, scope: &str) {
    let created = cli
        .mutation(
            scope,
            "finite-render-create",
            "projects.create",
            json!({"name":"Finite CPU render"}),
        )
        .await;
    assert_eq!(created["ok"], true);
    let project = created["result"]["project_id"].as_str().unwrap();
    let sequence = edit(
        cli,
        scope,
        project,
        "0",
        "finite-render-sequence",
        "sequences.create",
        json!({
            "name":"Red gap blue","settings":{"width":64,"height":48,
                "frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
            "tracks":[{"kind":"video","name":"Video"}]
        }),
    )
    .await;
    assert_eq!(sequence["revision"], "1");
    let sequence_id = sequence["created"]["sequence_id"].as_str().unwrap();
    let track = sequence["created"]["tracks"][0]["id"].as_str().unwrap();
    for (revision, key, rgba, start) in [
        (
            "1",
            "finite-render-red",
            [1, 0, 0, 1],
            json!({"num":"0","den":"1"}),
        ),
        (
            "2",
            "finite-render-blue",
            [0, 0, 1, 1],
            json!({"num":"3","den":"2"}),
        ),
    ] {
        let placed = edit(
            cli,
            scope,
            project,
            revision,
            key,
            "clips.place",
            json!({
                "sequence_id":sequence_id,"track_id":track,
                "source":{"kind":"solid_color","rgba":rgba,"duration":{"num":"1","den":"1"}},
                "source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
                "sequence_start":start,"collision":"reject"
            }),
        )
        .await;
        assert_eq!(placed["base_revision"], revision);
    }
    let range = json!({"start":{"num":"0","den":"1"},"end":{"num":"5","den":"2"}});
    let mut request = Request::query(
        "timeline.render",
        json!({
            "sequence_id":sequence_id,"revision":"3","range":range,
            "output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le",
                "audio":"silence","destination":{"kind":"download"}}
        }),
    );
    request.project = Some(ProjectRef {
        id: project.into(),
        expected_revision: None,
    });
    let response = cli.raw(&request).await;
    assert_eq!(
        response["ok"], true,
        "TIMELINE_RENDER_PUBLIC_COMPILER_MISSING code={}",
        response["error"]["code"]
    );
    assert_eq!(response["request_id"], request.request_id);
    assert!(response["receipt_id"].is_null());
    let plan = &response["result"];
    assert_eq!(plan["envelope_version"], 1);
    assert_eq!(plan["payload_kind"], "media_v1");
    assert_eq!(plan["payload"]["capture"]["project_id"], project);
    assert_eq!(plan["payload"]["capture"]["project_revision"], "3");
    assert_eq!(plan["payload"]["capture"]["sequence_id"], sequence_id);
    assert_eq!(plan["payload"]["capture"]["range"], range);
    assert_eq!(plan["payload"]["output"]["video_frames"], "10");
    assert_eq!(plan["payload"]["output"]["audio_samples"], "120000");
    let decoded: nw_domain::media::MediaPlanV1 = serde_json::from_value(plan.clone()).unwrap();
    decoded.validate_hash().unwrap();
    let encoded = serde_json::to_string(plan).unwrap();
    assert!(encoded.len() <= 128 * 1024);
    assert!(!encoded.contains(cli.data.to_string_lossy().as_ref()));
    assert!(!encoded.contains(cli.components.to_string_lossy().as_ref()));
    for private in ["signing_key", "replay_scope", "credential"] {
        assert!(!encoded.contains(private));
    }
    let mut changed = plan.clone();
    changed["payload"]["output"]["video_frames"] = json!("11");
    let changed: nw_domain::media::MediaPlanV1 = serde_json::from_value(changed).unwrap();
    assert!(
        changed.digest().is_err(),
        "Declared frame count must match the exact graph range"
    );
    let schema = nw_domain::commands::descriptor("timeline.render")
        .unwrap()
        .result_schema
        .unwrap();
    let expected: Vec<u16> = (1..=14).collect();
    let mut batch =
        super::project_topology::schema_validation::Batch::for_schemas(18, &expected, vec![schema]);
    batch.whole(1, 0, encoded, true);
    for id in 2..=14 {
        let mut copy = plan.clone();
        match id {
            2 => copy["envelope_version"] = json!(2),
            3 => copy["payload_kind"] = json!("cleanup"),
            4 => copy["extra"] = json!(true),
            5 => copy["hash"] = json!("0".repeat(63)),
            6 => copy["payload"]["capture"]["project_id"] = json!("../not-a-project"),
            7 => copy["payload"]["capture"]["project_revision"] = json!("01"),
            8 => copy["payload"]["output"]["video_frames"] = json!("01"),
            9 => copy["payload"]["output"]["audio_samples"] = json!("-1"),
            10 => {
                copy.as_object_mut().unwrap().remove("payload");
            }
            11 => copy["payload_kind"] = Value::Null,
            12 => copy["payload"]["output"] = json!("unsupported"),
            13 => copy["created_at"] = json!("01"),
            14 => {
                copy.as_object_mut().unwrap().remove("expires_at");
            }
            _ => unreachable!(),
        }
        batch.whole(id, 0, serde_json::to_string(&copy).unwrap(), false);
    }
    batch.finish().await;
    let (typed, exit) = cli
        .human(&[
            "timeline",
            "render",
            "--project",
            project,
            "--revision",
            "3",
            "--sequence",
            sequence_id,
            "--start",
            "0",
            "--end",
            "5/2",
        ])
        .await;
    assert_eq!(
        exit, 0,
        "Typed finite media review failed: {}",
        typed["error"]["code"]
    );
    assert_eq!(typed["ok"], true);
    assert_eq!(typed["result"]["hash"], plan["hash"]);
    assert!(typed["receipt_id"].is_null());
    for (field, value) in [("revision", json!("2")), ("extra", json!(true))] {
        let mut invalid = request.clone();
        invalid.params[field] = value;
        let failed = cli.raw(&invalid).await;
        assert_eq!(failed["ok"], false);
        assert_eq!(
            failed["error"]["code"],
            if field == "revision" {
                "STALE_REVISION"
            } else {
                "VALIDATION_ERROR"
            }
        );
        assert!(failed["receipt_id"].is_null());
        assert!(failed["result"].is_null());
    }
    physical_output(cli, &decoded).await;
}

async fn physical_output(cli: &Cli, plan: &nw_domain::media::MediaPlanV1) {
    use nw_domain::time::Rational;
    use nw_platform::{files, process};
    use sha2::{Digest, Sha256};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    plan.validate_hash().unwrap();
    let ffmpeg = cli.components.join("ffmpeg");
    let ffprobe = cli.components.join("ffprobe");
    let cancel = CancellationToken::new();
    let engine_identity = [
        files::identity(&ffmpeg, &cancel).unwrap(),
        files::identity(&ffprobe, &cancel).unwrap(),
    ];
    for (actual, reviewed) in engine_identity
        .iter()
        .zip([&plan.payload.tools.ffmpeg, &plan.payload.tools.ffprobe])
    {
        assert_eq!(actual.sha256, reviewed.sha256);
        assert_eq!(actual.bytes, reviewed.bytes);
    }
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("finite-compiled-output.mkv");
    let mut argv = plan.payload.argv_template.clone();
    assert_eq!(
        argv.iter()
            .filter(|arg| arg.as_str() == "@owned_staging@")
            .count(),
        1
    );
    assert_eq!(argv.last().map(String::as_str), Some("@owned_staging@"));
    *argv.last_mut().unwrap() = output.to_str().unwrap().into();
    process::run_with_output_limits(
        &ffmpeg,
        &argv,
        Duration::from_secs(5),
        4096,
        4096,
        cancel.clone(),
    )
    .await
    .unwrap();
    let bytes = std::fs::metadata(&output).unwrap().len();
    assert!(
        bytes > 0 && bytes <= 1024 * 1024,
        "Finite compiler output exceeds its fixture byte budget"
    );
    let probe_args = [
        "-v",
        "error",
        "-show_streams",
        "-show_frames",
        "-show_format",
        "-of",
        "json",
        output.to_str().unwrap(),
    ]
    .map(String::from);
    let observed = process::run_with_output_limits(
        &ffprobe,
        &probe_args,
        Duration::from_secs(5),
        256 * 1024,
        4096,
        cancel.clone(),
    )
    .await
    .unwrap();
    let probe: Value = serde_json::from_slice(&observed.stdout).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);
    let video = &streams[0];
    let audio = &streams[1];
    assert_eq!(video["index"], 0);
    assert_eq!(video["codec_name"], "ffv1");
    assert_eq!(video["width"], 64);
    assert_eq!(video["height"], 48);
    assert_eq!(video["pix_fmt"], "bgr0");
    assert_eq!(video["color_range"], "pc");
    assert_eq!(video["color_space"], "gbr");
    assert_eq!(video["color_primaries"], "bt709");
    assert_eq!(video["color_transfer"], "iec61966-2-1");
    assert_eq!(audio["index"], 1);
    assert_eq!(audio["codec_name"], "pcm_s16le");
    assert_eq!(audio["sample_rate"], "48000");
    assert_eq!(audio["channels"], 2);
    assert_eq!(audio["sample_fmt"], "s16");
    assert_eq!(probe["format"]["duration"], "2.500000");
    let frames = probe["frames"].as_array().unwrap();
    let video_frames: Vec<_> = frames
        .iter()
        .filter(|frame| frame["media_type"] == "video")
        .collect();
    assert_eq!(video_frames.len(), 10);
    let (num, den) = video["time_base"]
        .as_str()
        .unwrap()
        .split_once('/')
        .unwrap();
    let time_base = Rational::new(num.parse().unwrap(), den.parse().unwrap()).unwrap();
    let mut video_pts = Vec::new();
    for (index, frame) in video_frames.iter().enumerate() {
        let pts = frame["pts"].as_i64().unwrap();
        let timestamp = Rational::new(pts, 1)
            .unwrap()
            .checked_mul(time_base)
            .unwrap();
        assert_eq!(timestamp, Rational::new(index as i64, 4).unwrap());
        video_pts.push(timestamp);
    }
    let audio_frames: Vec<_> = frames
        .iter()
        .filter(|frame| frame["media_type"] == "audio")
        .collect();
    assert_eq!(audio_frames[0]["pts"], 0);
    let samples: u64 = audio_frames
        .iter()
        .map(|frame| frame["nb_samples"].as_u64().unwrap())
        .sum();
    assert_eq!(samples, 120000);
    let pixels_args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        output.to_str().unwrap(),
        "-map",
        "0:v:0",
        "-c:v",
        "rawvideo",
        "-pix_fmt",
        "bgr0",
        "-f",
        "rawvideo",
        "pipe:1",
    ]
    .map(String::from);
    let pixels = process::run_with_output_limits(
        &ffmpeg,
        &pixels_args,
        Duration::from_secs(5),
        512 * 1024,
        4096,
        cancel.clone(),
    )
    .await
    .unwrap()
    .stdout;
    let stride = 64 * 48 * 4;
    assert_eq!(pixels.len(), stride * 10);
    for (index, frame) in pixels.chunks_exact(stride).enumerate() {
        let expected = match index {
            0..=3 => [0, 0, 255],
            4..=5 => [0, 0, 0],
            6..=9 => [255, 0, 0],
            _ => unreachable!(),
        };
        for pixel in frame.chunks_exact(4) {
            // bgr0's unused fourth byte has no signal meaning.
            assert_eq!(
                &pixel[..3],
                &expected,
                "Unexpected decoded color at frame{index}"
            );
        }
    }
    let pcm_args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        output.to_str().unwrap(),
        "-map",
        "0:a:0",
        "-c:a",
        "pcm_s16le",
        "-f",
        "s16le",
        "pipe:1",
    ]
    .map(String::from);
    let pcm = process::run_with_output_limits(
        &ffmpeg,
        &pcm_args,
        Duration::from_secs(5),
        512 * 1024,
        4096,
        cancel.clone(),
    )
    .await
    .unwrap()
    .stdout;
    assert_eq!(pcm.len(), 120000 * 2 * 2);
    assert!(pcm.iter().all(|sample_byte| *sample_byte == 0));
    for identity in &engine_identity {
        files::revalidate(identity, &cancel).unwrap();
    }
    let output_identity = files::identity(&output, &cancel).unwrap();
    let evidence = json!({"schema_version":1,"source":std::env::var("NW_SCHEMA_VALIDATOR_SOURCE").unwrap(),
        "target":std::env::var("NW_SCHEMA_VALIDATOR_TARGET").unwrap(),"plan_hash":plan.hash,
        "tools":plan.payload.tools,"output_sha256":output_identity.sha256,"output_bytes":bytes,
        "video_frames":video_frames.len(),"video_pts":video_pts,"decoded_bgr0_bytes":pixels.len(),
        "decoded_bgr0_sha256":format!("{:x}", Sha256::digest(&pixels)),"audio_samples":samples,
        "decoded_pcm_s16le_bytes":pcm.len(),"decoded_pcm_s16le_sha256":format!("{:x}", Sha256::digest(&pcm)),
        "coverage":"Hosted finite compiler graph only; no durable media job, receipt or publication"});
    let encoded = serde_json::to_vec_pretty(&evidence).unwrap();
    assert!(encoded.len() <= 16 * 1024);
    let evidence_root =
        std::path::PathBuf::from(std::env::var_os("NW_SCHEMA_VALIDATOR_EVIDENCE_DIR").unwrap());
    std::fs::create_dir_all(&evidence_root).unwrap();
    std::fs::write(evidence_root.join("finite-compiler-media.json"), encoded).unwrap();
    println!(
        "FINITE_COMPILER_PHYSICAL_OUTPUT: videoFrames=10 exactQuarterSecondPTS red4black2blue4 allPixelsChecked audioSamples=120000 stereo48000 allPcmZero; outputBytes={bytes}"
    );
}
