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

pub struct FiniteTimeline {
    project: String,
    sequence: String,
}

pub async fn registered_timeline_frame(
    cli: &Cli,
    scope: &str,
    media: &super::fixture::Media,
    finite: &FiniteTimeline,
    wave_asset: &str,
) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};

    let source = media
        .directory
        .path()
        .join("timeline-nonmatching-origins.mkv");
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
        "sine=440:r=48000:d=2",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-vf",
        "setpts=PTS+1/TB",
        "-c:v",
        "ffv1",
        "-c:a",
        "pcm_s16le",
    ]
    .map(String::from)
    .to_vec();
    args.push(source.to_str().unwrap().into());
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &args,
        std::time::Duration::from_secs(10),
        4096,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let asset = cli
        .mutation(
            scope,
            "timeline-preview-register",
            "assets.register",
            json!({"path":source}),
        )
        .await;
    assert_eq!(asset["ok"], true, "{asset}");
    let asset_id = asset["result"]["id"].as_str().unwrap();
    let asset = cli.query("assets.inspect", json!({"id":asset_id})).await;
    assert_eq!(asset["probe"]["stream_details"]["0"]["start_pts"], 1000);
    assert_eq!(asset["probe"]["stream_details"]["1"]["start_pts"], 0);
    let project = cli
        .mutation(
            scope,
            "timeline-preview-project",
            "projects.create",
            json!({"name":"Original clocks preview"}),
        )
        .await;
    assert_eq!(project["ok"], true, "{project}");
    let project_id = project["result"]["project_id"].as_str().unwrap();
    let sequence = edit(cli, scope, project_id, "0", "timeline-preview-sequence", "sequences.create", json!({
        "name":"Three tracks", "settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"mono"},
        "tracks":[{"kind":"video","name":"Background"},{"kind":"audio","name":"Audio"},{"kind":"video","name":"Picture"}]
    })).await;
    let sequence_id = sequence["created"]["sequence_id"].as_str().unwrap();
    let t = |num: u64, den: u64| json!({"num":num.to_string(),"den":den.to_string()});
    let zero = t(0, 1);
    for (index, (source, start, end, place)) in [
        (
            json!({"kind":"solid_color","rgba":[0.2,0.4,0.8,1],"duration":t(4,1)}),
            zero.clone(),
            t(4, 1),
            zero.clone(),
        ),
        (
            json!({"kind":"registered","asset_id":asset_id,"stream_index":1}),
            zero.clone(),
            t(2, 1),
            zero.clone(),
        ),
        (
            json!({"kind":"registered","asset_id":asset_id,"stream_index":0}),
            t(1, 1),
            t(2, 1),
            t(1, 2),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        edit(cli, scope, project_id, &(index+1).to_string(), &format!("timeline-preview-clip-{index}"), "clips.place", json!({
            "sequence_id":sequence_id,"track_id":sequence["created"]["tracks"][index]["id"],"source":source,
            "source_range":{"start":start,"end":end},"sequence_start":place,"collision":"reject"
        })).await;
    }
    let native = cli
        .human(&[
            "timeline",
            "preview-frame",
            "--project",
            project_id,
            "--revision",
            "4",
            "--sequence",
            sequence_id,
            "--at",
            "1",
        ])
        .await;
    assert_eq!(native.1, 0, "{}", native.0);
    let frame = &native.0["result"];
    let independent = cli
        .query(
            "media.preview_frame",
            json!({"asset_id":asset_id,"stream_index":0,"frame":2}),
        )
        .await;
    assert_eq!(frame["rgba_base64"], independent["rgba_base64"]);
    assert_eq!(frame["origin"]["asset_id"], asset_id);
    assert_eq!(frame["origin"]["stream_index"], 0);
    for (at, kind, rgba) in [
        ("1/4", "solid_color", [51, 102, 204, 255]),
        ("4", "gap", [0, 0, 0, 255]),
    ] {
        let (response, exit) = cli
            .human(&[
                "timeline",
                "preview-frame",
                "--project",
                project_id,
                "--revision",
                "4",
                "--sequence",
                sequence_id,
                "--at",
                at,
            ])
            .await;
        assert_eq!(exit, 0, "{response}");
        let pixels = STANDARD
            .decode(response["result"]["rgba_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(pixels.len(), nw_domain::preview::RGBA_BYTES);
        assert!(pixels.as_chunks::<4>().0.iter().all(|pixel| pixel == &rgba));
        assert_eq!(response["result"]["origin"]["kind"], kind);
    }
    let mut bridge = cli.command();
    bridge.args(["bridge", "--native-owner"]);
    let sdk = ().serve(TokioChildProcess::new(bridge).unwrap()).await.unwrap();
    let response = sdk.call_tool(CallToolRequestParams::new("timeline.preview_frame").with_arguments(
        rmcp::object!({"params":{"sequence_id":sequence_id,"revision":"4","at":t(1,1)},"context":{"project":{"id":project_id}}}),
    )).await.unwrap();
    let response = serde_json::to_value(response).unwrap();
    assert_eq!(response["structuredContent"]["ok"], true, "{response}");
    assert_eq!(&response["structuredContent"]["result"], frame);
    sdk.cancel().await.unwrap();
    accepted_mixed_timeline(cli, project_id, sequence_id, finite).await;
    sequence_markers(cli, project_id, sequence_id).await;
    let placed = edit(
        cli,
        scope,
        project_id,
        "8",
        "wave-cue-occurrence",
        "clips.place",
        json!({
            "sequence_id":sequence_id, "track_id":sequence["created"]["tracks"][1]["id"],
            "source":{"kind":"registered","asset_id":wave_asset,"stream_index":0},
            "source_range":{"start":t(1,4),"end":t(3,4)},
            "sequence_start":t(3,1),"collision":"reject"
        }),
    )
    .await;
    let clip = placed["created"]["clip_ids"][0].as_str().unwrap();
    assert_eq!(placed["revision"], "9");
    let (response, code) = cli
        .human(&[
            "markers",
            "wave-projection",
            "--project",
            project_id,
            "--clip",
            clip,
            "--revision",
            "9",
        ])
        .await;
    assert_eq!(code, 0, "{}", response["error"]);
    assert_eq!(response["result"]["asset_id"], wave_asset);
    assert_eq!(response["result"]["clip_id"], clip);
    assert_eq!(
        response["result"]["cues"],
        json!([{
            "original":{"id":7,"sample_offset":24000,"sample_count":12000,"label":"نشانهٔ موسیقی"},
            "source_start":t(1,2), "source_end":t(3,4),
            "sequence_start":t(13,4), "sequence_end":t(7,2),
        }])
    );
    let (stale, code) = cli
        .human(&[
            "markers",
            "wave-projection",
            "--project",
            project_id,
            "--clip",
            clip,
            "--revision",
            "8",
        ])
        .await;
    assert_ne!(code, 0);
    assert_eq!(stale["error"]["code"], "STALE_REVISION");
    let mut bridge = cli.command();
    bridge.args(["bridge", "--native-owner"]);
    let sdk = ().serve(TokioChildProcess::new(bridge).unwrap()).await.unwrap();
    let projected = sdk.call_tool(CallToolRequestParams::new("markers.wave_projection").with_arguments(
        rmcp::object!({"params":{"clip_id":clip,"revision":"9"},"context":{"project":{"id":project_id}}}),
    )).await.unwrap();
    let projected = serde_json::to_value(projected).unwrap();
    assert_eq!(projected["structuredContent"]["ok"], true, "{projected}");
    assert_eq!(projected["structuredContent"]["result"], response["result"]);
    sdk.cancel().await.unwrap();
}

async fn sequence_markers(cli: &Cli, project: &str, sequence: &str) {
    let (issued, code) = cli
        .human(&["access", "replay-scope", "create", "--json"])
        .await;
    assert_eq!(code, 0);
    let scope = issued["result"]["replay_scope"].as_str().unwrap();
    let (placed, code) = cli
        .human(&[
            "markers",
            "place",
            "--project",
            project,
            "--expected-revision",
            "4",
            "--sequence",
            sequence,
            "--name",
            "نشانه é",
            "--at",
            "1/4",
            "--length",
            "1/2",
            "--color",
            "violet",
            "--replay-scope",
            scope,
            "--idempotency-key",
            "sequence-grid-marker",
            "--request-id",
            "sequence-grid-marker",
        ])
        .await;
    assert_eq!(code, 0, "{}", placed["error"]);
    let id = &placed["result"]["created"]["marker_id"];
    assert_eq!(placed["result"]["revision"], "5");
    let query = |revision| {
        let mut request = Request::query(
            "sequences.inspect",
            json!({
                "projection":"marker_page","sequence_id":sequence,"revision":revision,
            }),
        );
        request.project = Some(ProjectRef {
            id: project.into(),
            expected_revision: None,
        });
        request
    };
    let first = cli.raw(&query("5")).await;
    assert_eq!(first["ok"], true, "{}", first["error"]);
    assert_eq!(
        first["result"]["items"],
        json!([{
            "id":id,"name":"نشانه é","at":{"num":"1","den":"4"},
            "length":{"num":"1","den":"2"},"color":"violet",
        }])
    );
    edit(
        cli,
        scope,
        project,
        "5",
        "sequence-marker-undo",
        "history.undo",
        json!({}),
    )
    .await;
    assert_eq!(cli.raw(&query("6")).await["result"]["items"], json!([]));
    edit(
        cli,
        scope,
        project,
        "6",
        "sequence-marker-redo",
        "history.redo",
        json!({}),
    )
    .await;
    assert_eq!(
        cli.raw(&query("7")).await["result"]["items"],
        first["result"]["items"]
    );
    edit(
        cli,
        scope,
        project,
        "7",
        "sequence-marker-remove",
        "markers.remove",
        json!({
            "sequence_id":sequence,"marker_id":id,
        }),
    )
    .await;
    assert_eq!(cli.raw(&query("8")).await["result"]["items"], json!([]));
}

async fn enable_mixed(cli: &Cli, scope: &str, key: &str) -> String {
    let (reviewed, code) = cli
        .human(&["maintenance", "media-v2", "promotion-plan"])
        .await;
    assert_eq!(code, 0, "{}", reviewed["error"]);
    let reviewed = &reviewed["result"];
    assert_eq!(reviewed["kind"], "generation_promotion_v2");
    assert!(
        reviewed["source"]["guard_revision"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    let review_file = cli.data.join("mixed-enablement-review.json");
    std::fs::write(&review_file, serde_json::to_vec(reviewed).unwrap()).unwrap();
    let accept = [
        "maintenance",
        "media-v2",
        "accept",
        "--plan-file",
        review_file.to_str().unwrap(),
        "--replay-scope",
        scope,
        "--idempotency-key",
        key,
        "--request-id",
        key,
    ];
    let (accepted, code) = cli.human(&accept).await;
    assert_eq!(code, 0, "{}", accepted["error"]);
    assert_eq!(accepted["result"]["enablement_id"], reviewed["id"]);
    assert!(accepted["receipt_id"].is_string());
    let (replayed, code) = cli.human(&accept).await;
    assert_eq!(code, 0, "{}", replayed["error"]);
    assert_eq!(replayed["receipt_id"], accepted["receipt_id"]);
    reviewed["id"].as_str().unwrap().to_owned()
}

async fn restore_unadopted_mixed(cli: &Cli, scope: &str, enablement: &str) {
    let (reviewed, code) = cli
        .human(&["maintenance", "media-v2", "restore", "plan", enablement])
        .await;
    assert_eq!(code, 0, "{}", reviewed["error"]);
    let review = &reviewed["result"];
    assert_eq!(review["kind"], "generation_promotion_restoration_v2");
    let file = cli.data.join("mixed-restoration-review.json");
    std::fs::write(&file, serde_json::to_vec(review).unwrap()).unwrap();
    let accept = [
        "maintenance",
        "media-v2",
        "restore",
        "accept",
        "--plan-file",
        file.to_str().unwrap(),
        "--replay-scope",
        scope,
        "--idempotency-key",
        "mixed-restore-owner",
        "--request-id",
        "mixed-restore-owner",
    ];
    let (accepted, code) = cli.human(&accept).await;
    assert_eq!(code, 0, "{}", accepted["error"]);
    assert_eq!(accepted["result"]["restoration_id"], review["id"]);
    let id = review["id"].as_str().unwrap();
    let (inspected, code) = cli
        .human(&["maintenance", "media-v2", "restore", "inspect", id])
        .await;
    assert_eq!(code, 0, "{}", inspected["error"]);
    assert_eq!(inspected["result"]["status"], "armed");
    assert_eq!(inspected["result"]["plan"], *review);
    let (replayed, code) = cli.human(&accept).await;
    assert_eq!(code, 0, "{}", replayed["error"]);
    assert_eq!(replayed["receipt_id"], accepted["receipt_id"]);
}

async fn accepted_mixed_timeline(
    cli: &Cli,
    project: &str,
    sequence: &str,
    finite: &FiniteTimeline,
) {
    use nw_domain::media::MediaPlanV2;
    use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};

    let settings = [
        "timeline",
        "render",
        "--mixed",
        "--project",
        project,
        "--revision",
        "4",
        "--sequence",
        sequence,
        "--end",
        "2",
    ];
    let (before, code) = cli.human(&settings).await;
    assert_ne!(code, 0);
    assert_eq!(before["error"]["code"], "STORE_VERSION_UNSUPPORTED");

    let (issued, code) = cli
        .human(&["access", "replay-scope", "create", "--json"])
        .await;
    assert_eq!(code, 0);
    let scope = issued["result"]["replay_scope"].as_str().unwrap();
    let first = enable_mixed(cli, scope, "mixed-enablement-owner").await;
    restore_unadopted_mixed(cli, scope, &first).await;
    let (restored, code) = cli.human(&settings).await;
    assert_ne!(code, 0);
    assert_eq!(restored["error"]["code"], "STORE_VERSION_UNSUPPORTED");
    let finite_job = submit_media_after_restoration(cli, scope, finite).await;
    let second = enable_mixed(cli, scope, "mixed-enablement-successor").await;
    assert_ne!(second, first);
    let preserved = cli
        .query("jobs.inspect", json!({"id":finite_job["id"]}))
        .await;
    assert_eq!(preserved, finite_job);

    let (review, code) = cli.human(&settings).await;
    assert_eq!(code, 0, "{}", review["error"]);
    let mixed: MediaPlanV2 = serde_json::from_value(review["result"].clone()).unwrap();
    mixed.validate_hash().unwrap();
    assert_eq!(mixed.payload.sources.len(), 2);
    let mut bridge = cli.command();
    bridge.args(["bridge", "--native-owner"]);
    let sdk = ().serve(TokioChildProcess::new(bridge).unwrap()).await.unwrap();
    let response = sdk.call_tool(CallToolRequestParams::new("timeline.render_mixed").with_arguments(
        rmcp::object!({"params":{"contract_kind":"media_v2","sequence_id":sequence,"revision":"4",
            "range":{"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}},
            "output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le","destination":{"kind":"download"}}},
            "context":{"project":{"id":project}}}),
    )).await.unwrap();
    let response = serde_json::to_value(response).unwrap();
    let body = &response["structuredContent"];
    assert_eq!(body["ok"], true, "{}", body["error"]);
    assert_eq!(body["result"]["hash"], mixed.hash);
    assert_eq!(body["result"]["payload"], review["result"]["payload"]);
    sdk.cancel().await.unwrap();

    let raw = format!("\n{}\n", serde_json::to_string_pretty(&mixed).unwrap());
    let review_file = cli.data.join("mixed-render-review.json");
    std::fs::write(&review_file, &raw).unwrap();
    let submit = [
        "jobs",
        "submit-media",
        "--plan-file",
        review_file.to_str().unwrap(),
        "--replay-scope",
        scope,
        "--idempotency-key",
        "mixed-owner-job",
        "--request-id",
        "mixed-owner-job",
    ];
    let (accepted, code) = cli.human(&submit).await;
    assert_eq!(code, 0, "{}", accepted["error"]);
    let id = accepted["result"]["job_id"].as_str().unwrap();
    let job = cli.completed(id).await;
    assert_eq!(job["state"], "succeeded", "{}", job["error"]);
    assert_eq!(job["payload_kind"], "media_job_v2");
    assert_eq!(
        job["payload_sha256"],
        nw_domain::projects::hash(raw.as_bytes())
    );
    let artifact = format!("media-{id}-output");
    let manifest = cli
        .query("artifacts.manifest", json!({"artifact_id":artifact}))
        .await;
    let actual = nw_platform::files::identity(
        &cli.data.join("exports").join(format!("media-{id}.mkv")),
        &tokio_util::sync::CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(manifest["sha256"], actual.sha256);
    assert_eq!(manifest["encoded_byte_count"], actual.bytes);
    let (replayed, code) = cli.human(&submit).await;
    assert_eq!(code, 0, "{}", replayed["error"]);
    assert_eq!(replayed["receipt_id"], accepted["receipt_id"]);
    assert_eq!(replayed["result"]["job_id"], id);
    let (refused, code) = cli
        .human(&["maintenance", "media-v2", "restore", "plan", &second])
        .await;
    assert_ne!(code, 0);
    assert_eq!(refused["error"]["code"], "RESTORATION_REFUSED");
}

async fn submit_media_after_restoration(cli: &Cli, scope: &str, finite: &FiniteTimeline) -> Value {
    let (review, code) = cli
        .human(&[
            "timeline",
            "render",
            "--project",
            &finite.project,
            "--revision",
            "4",
            "--sequence",
            &finite.sequence,
            "--end",
            "5/2",
        ])
        .await;
    assert_eq!(code, 0, "{}", review["error"]);
    assert_eq!(review["result"]["payload_kind"], "media_v1");
    let file = cli.data.join("finite-media-after-mixed-restoration.json");
    let raw = serde_json::to_vec(&review["result"]).unwrap();
    std::fs::write(&file, &raw).unwrap();
    let (accepted, code) = cli
        .human(&[
            "jobs",
            "submit-media",
            "--plan-file",
            file.to_str().unwrap(),
            "--replay-scope",
            scope,
            "--idempotency-key",
            "media-v1-after-mixed-restore",
            "--request-id",
            "media-v1-after-mixed-restore",
        ])
        .await;
    assert_eq!(code, 0, "{}", accepted["error"]);
    let job = cli
        .completed(accepted["result"]["job_id"].as_str().unwrap())
        .await;
    assert_eq!(job["state"], "succeeded", "{}", job["error"]);
    assert_eq!(job["payload_kind"], "media_job_v1");
    assert_eq!(job["payload_sha256"], nw_domain::projects::hash(&raw));
    job
}

pub async fn review(cli: &Cli, scope: &str) -> FiniteTimeline {
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
    accepted_output(cli, scope, project, &decoded).await;
    FiniteTimeline {
        project: project.into(),
        sequence: sequence_id.into(),
    }
}

async fn accepted_output(
    cli: &Cli,
    scope: &str,
    project: &str,
    plan: &nw_domain::media::MediaPlanV1,
) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};

    let added_started = std::time::Instant::now();
    let raw = format!("\n{}\n", serde_json::to_string_pretty(plan).unwrap());
    let raw_sha = hex::encode(Sha256::digest(raw.as_bytes()));
    let review_file = cli.data.join("finite-media-review.json");
    std::fs::write(&review_file, &raw).unwrap();
    let arguments = [
        "jobs",
        "submit-media",
        "--plan-file",
        review_file.to_str().unwrap(),
        "--replay-scope",
        scope,
        "--idempotency-key",
        "finite-media-v1-accepted",
        "--request-id",
        "finite-media-v1-request",
    ];
    let (accepted, code) = cli.human(&arguments).await;
    assert_eq!(
        code, 0,
        "Actual MediaV1 submission failed: {}",
        accepted["error"]["code"]
    );
    assert_eq!(accepted["ok"], true);
    assert!(accepted["receipt_id"].is_string());
    let id = accepted["result"]["job_id"].as_str().unwrap();
    let job = cli.completed(id).await;
    assert_eq!(
        job["state"], "succeeded",
        "Actual MediaV1 execution failed: {}",
        job["error"]
    );
    assert_eq!(job["envelope_version"], 1);
    assert_eq!(job["payload_kind"], "media_job_v1");
    assert_eq!(job["plan_id"], plan.id);
    assert_eq!(job["plan_hash"], plan.hash);
    assert_eq!(job["payload_sha256"], raw_sha);
    assert_eq!(job["artifact"]["state"], "published");
    let stages = job["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 3);
    for (stage, name) in stages.iter().zip(["render", "verify", "publish"]) {
        assert_eq!(stage["id"], name);
        assert_eq!(stage["state"], "succeeded");
    }
    assert_eq!(stages[0]["invocations"].as_array().unwrap().len(), 1);
    assert_eq!(stages[1]["invocations"].as_array().unwrap().len(), 3);
    assert_eq!(
        stages[0]["invocations"][0]["executable_sha256"],
        plan.payload.tools.ffmpeg.sha256
    );
    for (call, sha) in stages[1]["invocations"].as_array().unwrap().iter().zip([
        &plan.payload.tools.ffprobe.sha256,
        &plan.payload.tools.ffmpeg.sha256,
        &plan.payload.tools.ffmpeg.sha256,
    ]) {
        assert_eq!(call["executable_sha256"], *sha);
    }
    let (waited, code) = cli
        .human(&["jobs", "wait", id, "--timeout-seconds", "5"])
        .await;
    assert_eq!(code, 0);
    assert_eq!(waited["result"]["id"], id);
    assert_eq!(waited["result"]["state"], "succeeded");
    let artifact = job["artifact"]["id"].as_str().unwrap();
    assert_eq!(artifact, format!("media-{id}-output"));
    let manifest = cli
        .query("artifacts.manifest", json!({"artifact_id":artifact}))
        .await;
    assert_eq!(manifest["artifact_id"], artifact);
    assert_eq!(manifest["job_id"], id);
    assert_eq!(manifest["plan_id"], plan.id);
    assert_eq!(manifest["availability"], "retained_published_object");
    let bytes: u64 = manifest["encoded_byte_count"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(bytes > 0 && bytes <= 1024 * 1024);
    assert_eq!(manifest["sha256"], job["artifact"]["sha256"]);
    let mut offset = 0u64;
    let mut hash = Sha256::new();
    while offset < bytes {
        let length = (bytes - offset).min(128 * 1024) as u32;
        let chunk = cli
            .query(
                "artifacts.read_chunk",
                json!({
                    "artifact_id":artifact,"expected_sha256":manifest["sha256"],
                    "offset":offset.to_string(),"length":length,
                }),
            )
            .await;
        let decoded = STANDARD
            .decode(chunk["data_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(decoded.len(), length as usize);
        assert_eq!(chunk["actual_length"], length);
        assert_eq!(chunk["offset"], offset.to_string());
        offset += u64::from(length);
        assert_eq!(chunk["eof"], offset == bytes);
        hash.update(decoded);
    }
    assert_eq!(hex::encode(hash.finalize()), manifest["sha256"]);
    let exported = cli.data.join("exports").join(format!("media-{id}.mkv"));
    let actual =
        nw_platform::files::identity(&exported, &tokio_util::sync::CancellationToken::new())
            .unwrap();
    assert_eq!(actual.sha256, manifest["sha256"]);
    assert_eq!(actual.bytes, manifest["encoded_byte_count"]);
    let inspected = serde_json::to_string(&job).unwrap();
    for private in [
        "owned_staging",
        "argv",
        "proof",
        "signing_key",
        "replay_scope",
    ] {
        assert!(!inspected.contains(private));
    }
    assert!(!inspected.contains(cli.data.to_string_lossy().as_ref()));
    // Real project drift cannot turn an accepted retry into a second acceptance.
    let changed = edit(
        cli,
        scope,
        project,
        "3",
        "finite-media-after-acceptance",
        "sequences.create",
        json!({
            "name":"After accepted media","settings":{"width":64,"height":48,
                "frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},
            "tracks":[{"kind":"video","name":"Video"}]
        }),
    )
    .await;
    assert_eq!(changed["revision"], "4");
    let (replayed, code) = cli.human(&arguments).await;
    assert_eq!(code, 0);
    assert_eq!(replayed["receipt_id"], accepted["receipt_id"]);
    assert_eq!(replayed["result"], accepted["result"]);
    let mut different = Request::query("jobs.submit", json!({"media_plan_json":format!("{raw} ")}));
    different.request_id = "finite-media-v1-request".into();
    different.replay_scope = Some(scope.into());
    different.idempotency_key = Some("finite-media-v1-accepted".into());
    let conflict = cli.raw(&different).await;
    assert_eq!(conflict["ok"], false);
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_CONFLICT");
    assert!(conflict["receipt_id"].is_null());
    let mut stale = Request::query("jobs.submit", json!({"media_plan_json":raw}));
    stale.replay_scope = Some(scope.into());
    stale.idempotency_key = Some("finite-media-stale-unaccepted".into());
    let refused = cli.raw(&stale).await;
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["code"], "STALE_REVISION");
    assert!(refused["receipt_id"].is_null());
    if let Some(directory) = std::env::var_os("NW_SCHEMA_VALIDATOR_EVIDENCE_DIR") {
        let evidence = json!({
            "schema_version":1,"status":"passed","scope":"Real common MediaV1 job, full RGB/PCM verification, retained no-replace publication and authorized byte retrieval; not whole-v1 qualification",
            "source_commit":std::env::var("NW_SCHEMA_VALIDATOR_SOURCE").ok(),
            "target":std::env::var("NW_SCHEMA_VALIDATOR_TARGET").ok(),
            "plan_id":plan.id,"plan_hash":plan.hash,"exact_raw_payload_sha256":raw_sha,
            "job_id":id,"receipt_id":accepted["receipt_id"],"job":job,"manifest":manifest,
            "actual_bytes":actual.bytes,"actual_sha256":actual.sha256,
            "accepted_replay_after_project_drift":true,"changed_raw_bytes_conflict":true,
            "fresh_stale_acceptance_refused":true,
            "added_elapsed_ms":u64::try_from(added_started.elapsed().as_millis()).unwrap(),
        });
        let encoded = serde_json::to_vec_pretty(&evidence).unwrap();
        assert!(encoded.len() <= 16 * 1024);
        std::fs::write(
            std::path::PathBuf::from(directory).join("finite-media-job.json"),
            encoded,
        )
        .unwrap();
    }
}

async fn physical_output(cli: &Cli, plan: &nw_domain::media::MediaPlanV1) {
    use nw_domain::time::Rational;
    use nw_platform::{files, process};
    use sha2::{Digest, Sha256};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    let added_started = std::time::Instant::now();
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
        for pixel in frame.as_chunks::<4>().0 {
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
        "added_elapsed_ms":u64::try_from(added_started.elapsed().as_millis()).unwrap(),
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
