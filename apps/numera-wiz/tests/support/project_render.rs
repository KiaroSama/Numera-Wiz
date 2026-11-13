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
}
