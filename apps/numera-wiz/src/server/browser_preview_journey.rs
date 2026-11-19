//! Hosted Chrome decodes real source and program windows served by the protected Rust owner.
use super::tests::{driver, fixture_with_components, pairing};
use nw_domain::commands::{ProjectRef, Request};
use nw_service::{Context, Service};
use serde_json::{Value, json};
use std::{io::Write, path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const COMPONENTS: &str = "NW_BROWSER_PREVIEW_COMPONENTS";
const CHROME_CONTINUOUS: &str = r#"
const [bearer, input, finish] = arguments;
function waiting(video, name, start) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(name + ' deadline')), 4500);
        video.addEventListener(name, () => { clearTimeout(timer); resolve(); }, {once: true});
        video.addEventListener('error', () => { clearTimeout(timer); reject(new Error('video decode')); }, {once: true});
        start();
    });
}
(async () => {
    const response = await fetch('/api/v1/previews/program', {
        method: 'POST', redirect: 'error',
        headers: {'Authorization': `Bearer ${bearer}`, 'Content-Type': 'application/json'},
        body: JSON.stringify(input)
    });
    if (!response.ok || response.headers.get('Cache-Control') !== 'no-store') throw new Error('Program stream unavailable');
    const bytes = await response.blob();
    if (bytes.size < 1 || bytes.size > 2 * 1024 * 1024) throw new Error('Unexpected sample extent');
    const url = URL.createObjectURL(bytes);
    const video = document.createElement('video');
    video.muted = true;
    document.body.append(video);
    try {
        await waiting(video, 'loadeddata', () => video.src = url);
        await waiting(video, 'seeked', () => video.currentTime = 5.5);
        const canvas = document.createElement('canvas');
        canvas.width = 320;
        canvas.height = 180;
        const image = canvas.getContext('2d', {willReadFrequently: true});
        image.drawImage(video, 0, 0);
        finish({ok: true, duration: video.duration, at: video.currentTime,
            pixel: Array.from(image.getImageData(160, 90, 1, 1).data)});
    } finally {
        video.removeAttribute('src');
        video.load();
        video.remove();
        URL.revokeObjectURL(url);
    }
})().catch(error => finish({ok: false, code: error.message}));
"#;
const CHROME_SEGMENT: &str = r#"
const [bearer, asset, audio, program, finish] = arguments;
const headers = {'Authorization': `Bearer ${bearer}`};
function wait(target, event, operate) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(event + ' deadline')), 4500);
        target.addEventListener(event, () => { clearTimeout(timer); resolve(); }, {once: true});
        target.addEventListener('error', () => { clearTimeout(timer); reject(new Error(event + ' failed')); }, {once: true});
        operate();
    });
}
(async () => {
    const request = program ?? {api_version: 1, request_id: 'hosted-browser-preview',
        command: {id: 'previews.source_segment', version: 1},
        params: {asset_id: asset, video_stream: 0, audio_stream: audio}};
    const accepted = await fetch('/api/v1/commands', {
        method: 'POST', headers: {...headers, 'Content-Type': 'application/json'},
        body: JSON.stringify(request)
    }).then(response => response.json());
    if (!accepted.ok) throw new Error(accepted.error?.code ?? 'Segment request rejected');
    const manifest = program ? accepted.result.playback : accepted.result;
    if (program && (accepted.result.project_id !== request.project.id ||
        accepted.result.selection.revision !== request.params.revision ||
        !manifest.mime_type.includes('mp4a.40.2'))) throw new Error('Incorrect captured program');
    if ((!program && manifest.selection.audio_stream !== audio) ||
        !MediaSource.isTypeSupported(manifest.mime_type)) {
        throw new Error('Chrome cannot admit the accepted media profile');
    }
    const route = `/api/v1/previews/${manifest.segment_id}/mp4`;
    const response = await fetch(route, {headers, redirect: 'error'});
    if (!response.ok || response.headers.get('Content-Type') !== 'video/mp4' ||
        response.headers.get('Cache-Control') !== 'no-store') throw new Error('Incorrect segment delivery');
    const bytes = new Uint8Array(await response.arrayBuffer());
    const digest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)),
        value => value.toString(16).padStart(2, '0')).join('');
    if (digest !== manifest.sha256 || bytes.length !== manifest.bytes) throw new Error('Segment digest differs');
    const media = new MediaSource();
    const url = URL.createObjectURL(media);
    const video = document.createElement('video');
    video.muted = true;
    video.controls = true;
    video.style = 'position:fixed;bottom:10px;right:10px;z-index:1000;width:320px';
    document.body.append(video);
    try {
        await wait(media, 'sourceopen', () => video.src = url);
        const buffer = media.addSourceBuffer(manifest.mime_type);
        let consumed = 0;
        for (const unit of [manifest.initialization, ...manifest.fragments]) {
            if (unit.offset !== consumed || unit.length <= 0) throw new Error('Noncontiguous MP4 unit');
            consumed = unit.offset + unit.length;
            await wait(buffer, 'updateend', () => buffer.appendBuffer(bytes.subarray(unit.offset, consumed)));
        }
        if (consumed !== bytes.length) throw new Error('Extra bytes beyond the append units');
        if (video.readyState < 2) await wait(video, 'loadeddata', () => media.endOfStream());
        else media.endOfStream();
        const pixels = [];
        if (program) {
            const canvas = document.createElement('canvas');
            canvas.width = 320;
            canvas.height = 180;
            const view = canvas.getContext('2d', {willReadFrequently: true});
            await wait(video, 'seeked', () => video.currentTime = 0.25);
            view.drawImage(video, 0, 0);
            pixels.push(Array.from(view.getImageData(160, 90, 1, 1).data));
        }
        await wait(video, 'seeked', () => video.currentTime = 1.125);
        if (program) {
            const canvas = document.createElement('canvas');
            canvas.width = 320;
            canvas.height = 180;
            const view = canvas.getContext('2d', {willReadFrequently: true});
            view.drawImage(video, 0, 0);
            pixels.push(Array.from(view.getImageData(160, 90, 1, 1).data));
        }
        finish({ok: true, manifest, pixels, width: video.videoWidth, height: video.videoHeight,
            position: video.currentTime, frames: video.getVideoPlaybackQuality().totalVideoFrames,
            start: video.buffered.start(0), end: video.buffered.end(0), digest});
    } finally {
        video.removeAttribute('src');
        video.load();
        video.remove();
        URL.revokeObjectURL(url);
    }
})().catch(error => finish({ok: false, code: error.message}));
"#;

pub(super) async fn run(client: &reqwest::Client, base: &str, route: &str, evidence: &Path) {
    let components = std::env::var_os(COMPONENTS).expect("Same-source browser FFmpeg runtime");
    let fixture = fixture_with_components(COMPONENTS).await;
    let source = fixture.root.path().join("browser-playback.mkv");
    let program = Path::new(&components).join("ffmpeg");
    let mut args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=24:duration=3",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=650:sample_rate=48000:duration=3",
        "-map",
        "0:v:0",
        "-map",
        "1:a:0",
        "-vf",
        "setparams=range=limited:color_primaries=bt709:color_trc=bt709:colorspace=bt709",
        "-af",
        "aformat=channel_layouts=stereo",
        "-c:v",
        "ffv1",
        "-threads:v",
        "1",
        "-c:a",
        "flac",
        "-f",
        "matroska",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(source.to_str().unwrap().to_owned());
    nw_platform::process::run(
        &program,
        &args,
        Duration::from_secs(15),
        4096,
        CancellationToken::new(),
    )
    .await
    .expect("Generate selected stereo source for Chrome playback");

    let paired = pairing(&fixture).await;
    let proof = paired
        .split_once("#pair=")
        .expect("Fixture pairing proof")
        .1;
    let credential: Value = client
        .post(format!("{}/api/v1/pairing/exchange", fixture.endpoint))
        .header(reqwest::header::ORIGIN, &fixture.endpoint)
        .json(&json!({"proof":proof}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let bearer = credential["bearer"].as_str().unwrap();
    let source_bytes = std::fs::read(&source).unwrap();
    let source_hash = nw_domain::projects::hash(&source_bytes);
    let upload: nw_domain::ingest::UploadTicket = client
        .post(format!("{}/api/v1/uploads", fixture.endpoint))
        .bearer_auth(bearer)
        .json(&nw_domain::ingest::UploadInput {
            display_name: "browser-playback.mkv".into(),
            bytes: source_bytes.len().to_string(),
            sha256: Some(source_hash.clone()),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let completed: nw_domain::ingest::UploadTicket = client
        .put(format!(
            "{}/api/v1/uploads/{}",
            fixture.endpoint, upload.upload_id
        ))
        .bearer_auth(bearer)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(source_bytes)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(completed.upload_id, upload.upload_id);

    let service = &fixture.owner.service;
    let browser = service.profile_context("browser").unwrap();
    let scope = service
        .dispatch(
            browser.clone(),
            Request::query("access.replay_scope.create", json!({})),
        )
        .await;
    assert!(scope.ok);
    let mut registration =
        Request::query("assets.register", json!({"upload_id":completed.upload_id}));
    let scope = scope.result.unwrap()["replay_scope"]
        .as_str()
        .unwrap()
        .to_owned();
    registration.replay_scope = Some(scope.clone());
    registration.idempotency_key = Some("hosted-browser-playback-source".into());
    let registered = service.dispatch(browser.clone(), registration).await;
    assert!(
        registered.ok,
        "Registered preview source: {:?}",
        registered.error
    );
    let asset = registered.result.unwrap();
    assert_eq!(asset["identity"]["sha256"], source_hash);
    driver(
        client,
        base,
        reqwest::Method::POST,
        &format!("{route}/url"),
        json!({"url":format!("{}/", fixture.endpoint)}),
    )
    .await;

    let mut reports = Vec::new();
    for audio in [None, Some(1u32)] {
        let report = driver(
            client,
            base,
            reqwest::Method::POST,
            &format!("{route}/execute/async"),
            json!({"script":CHROME_SEGMENT,"args":[bearer,asset["id"],audio,null]}),
        )
        .await;
        assert_eq!(report["ok"], true, "Actual Chrome source preview: {report}");
        assert_eq!(report["width"], 320);
        assert_eq!(report["height"], 180);
        assert_eq!(report["position"], 1.125);
        assert_eq!(report["start"], 0.0);
        assert!(
            (1.9..=2.05).contains(&report["end"].as_f64().unwrap()),
            "{report}"
        );
        assert!(report["frames"].as_u64().unwrap() > 0, "{report}");
        assert_eq!(
            report["manifest"]["source_sha256"],
            asset["identity"]["sha256"]
        );
        reports.push(report);
    }

    let selected = program_fixture(service, &browser, &scope, &asset["id"]).await;
    let program_report = driver(
        client,
        base,
        reqwest::Method::POST,
        &format!("{route}/execute/async"),
        json!({"script":CHROME_SEGMENT,"args":[bearer,null,null,selected.request]}),
    )
    .await;
    assert_eq!(
        program_report["ok"], true,
        "Actual Chrome program preview: {program_report}"
    );
    assert_eq!(program_report["width"], 320);
    assert_eq!(program_report["height"], 180);
    assert_eq!(program_report["position"], 1.125);
    assert_eq!(program_report["start"], 0.0);
    assert!((1.9..=2.05).contains(&program_report["end"].as_f64().unwrap()));
    let plane =
        |frame: usize, color: usize| program_report["pixels"][frame][color].as_u64().unwrap();
    assert!(plane(0, 0) > 180 && plane(0, 2) < 20, "{program_report}");
    assert!(plane(1, 2) > 180 && plane(1, 0) < 20, "{program_report}");
    let program_url = format!(
        "{}/api/v1/previews/{}/mp4",
        fixture.endpoint,
        program_report["manifest"]["segment_id"].as_str().unwrap(),
    );
    let preview = client
        .get(&program_url)
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let saved = fixture.root.path().join("checked-browser-program.mp4");
    std::fs::write(&saved, &preview).unwrap();
    let mut decode = ["-hide_banner", "-nostdin", "-v", "error", "-i"]
        .map(str::to_owned)
        .to_vec();
    decode.push(saved.to_str().unwrap().into());
    decode.extend(
        [
            "-map",
            "0:a:0",
            "-c:a",
            "pcm_s16le",
            "-f",
            "s16le",
            "pipe:1",
        ]
        .map(str::to_owned),
    );
    let pcm = nw_platform::process::run(
        &program,
        &decode,
        Duration::from_secs(10),
        512 * 1024,
        CancellationToken::new(),
    )
    .await
    .unwrap()
    .stdout;
    let peak = |start: usize, end: usize| {
        pcm[start * 4..end * 4]
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs())
            .max()
            .unwrap()
    };
    let peaks = [
        peak(1_000, 20_000),
        peak(28_000, 68_000),
        peak(77_000, 95_000),
    ];
    assert!(
        peaks[0] < 90 && peaks[1] > 2_000 && peaks[2] < 90,
        "{peaks:?}"
    );

    let playback_url = format!("{}/api/v1/previews/program", fixture.endpoint);
    let playback_input = json!({"project_id":selected.project,"selection":{
        "sequence_id":selected.sequence,"revision":"4","range":{"start":t(0,1),"end":t(6,1)}
    }});
    assert_eq!(
        client
            .post(&playback_url)
            .json(&playback_input)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let streamed = client
        .post(&playback_url)
        .bearer_auth(bearer)
        .json(&playback_input)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert!(
        streamed.headers()[reqwest::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("avc1.")
    );
    let streamed = streamed.bytes().await.unwrap();
    assert!((1..=2 * 1024 * 1024).contains(&streamed.len()));
    let long = fixture.root.path().join("checked-browser-continuous.mp4");
    std::fs::write(&long, &streamed).unwrap();
    let mut extract_late = ["-hide_banner", "-nostdin", "-v", "error", "-i"]
        .map(str::to_owned)
        .to_vec();
    extract_late.push(long.to_str().unwrap().into());
    extract_late.extend(
        [
            "-map",
            "0:v:0",
            "-vf",
            "select=eq(n\\,132)",
            "-frames:v",
            "1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "pipe:1",
        ]
        .map(str::to_owned),
    );
    let late = nw_platform::process::run(
        &program,
        &extract_late,
        Duration::from_secs(10),
        320 * 180 * 3,
        CancellationToken::new(),
    )
    .await
    .unwrap()
    .stdout;
    assert_eq!(late.len(), 320 * 180 * 3);
    assert!(
        late.iter().all(|channel| *channel <= 8),
        "The gap at 5.5 seconds must decode as black"
    );
    let long_browser = driver(
        client,
        base,
        reqwest::Method::POST,
        &format!("{route}/execute/async"),
        json!({"script":CHROME_CONTINUOUS,"args":[bearer,playback_input]}),
    ).await;
    assert_eq!(long_browser["ok"], true, "Chrome continuous playback: {long_browser}");
    assert_eq!(long_browser["at"], 5.5);
    assert!((5.9..=6.1).contains(&long_browser["duration"].as_f64().unwrap()));
    assert!(long_browser["pixel"].as_array().unwrap()[..3].iter().all(|channel| channel.as_u64().unwrap() <= 8));

    let revision = project_edit(
        service,
        &browser,
        &scope,
        "browser-program-invalidate",
        "clips.place",
        Some(ProjectRef {
            id: selected.project,
            expected_revision: Some("4".into()),
        }),
        color_place(
            &selected.sequence,
            &selected.picture,
            [0.0, 0.65, 0.0, 1.0],
            2,
        ),
    )
    .await;
    assert_eq!(revision["revision"], "5", "{revision}");
    let stale = client
        .get(program_url)
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(
        stale.json::<Value>().await.unwrap()["error"]["code"],
        "STALE_REVISION"
    );
    let stale_stream = client
        .post(&playback_url)
        .bearer_auth(bearer)
        .json(&playback_input)
        .send()
        .await
        .unwrap();
    assert_eq!(
        stale_stream.json::<Value>().await.unwrap()["error"]["code"],
        "STALE_REVISION"
    );

    let last = reports.last().unwrap();
    let segment = format!(
        "{}/api/v1/previews/{}/mp4",
        fixture.endpoint,
        last["manifest"]["segment_id"].as_str().unwrap()
    );
    assert_eq!(
        client.get(&segment).send().await.unwrap().status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(asset["identity"]["path"].as_str().unwrap())
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    let denied = client
        .get(&segment)
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        denied.json::<Value>().await.unwrap()["error"]["code"],
        "SOURCE_CHANGED"
    );
    std::fs::write(
        evidence.join("source-preview-chrome.json"),
        serde_json::to_vec_pretty(
            &json!({"cases":reports,"missing_bearer_denied":true,"changed_source_denied":true,
                    "program":{"chrome":program_report,"decoded_audio_window_peaks":peaks,"stale_revision_denied":true,
                        "continuous":{"bytes":streamed.len(),"verified_frame_index":132,"stale_revision_denied":true,"chrome":long_browser}}}),
        )
        .unwrap(),
    )
    .unwrap();
}

struct ProgramFixture {
    project: String,
    sequence: String,
    picture: String,
    request: Request,
}

async fn program_fixture(
    service: &Arc<Service>,
    context: &Context,
    scope: &str,
    asset: &Value,
) -> ProgramFixture {
    let project = project_edit(
        service,
        context,
        scope,
        "browser-program-project",
        "projects.create",
        None,
        json!({"name":"Browser program window"}),
    )
    .await["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let edit_context = |revision: &str| {
        Some(ProjectRef {
            id: project.clone(),
            expected_revision: Some(revision.into()),
        })
    };
    let created = project_edit(
        service, context, scope, "browser-program-sequence", "sequences.create", edit_context("0"),
        json!({"name":"Picture and sound","settings":{
            "width":640,"height":360,"frame_rate":t(24,1),"sample_rate":48000,"audio_layout":"stereo"
        },"tracks":[{"kind":"video","name":"Picture"},{"kind":"audio","name":"Audio"}]}),
    ).await;
    let sequence = created["created"]["sequence_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let picture = created["created"]["tracks"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (revision, color, place) in [
        ("1", [0.82, 0.0, 0.0, 1.0], 0),
        ("2", [0.0, 0.0, 0.82, 1.0], 1),
    ] {
        project_edit(
            service,
            context,
            scope,
            &format!("browser-program-picture-{place}"),
            "clips.place",
            edit_context(revision),
            color_place(&sequence, &picture, color, place),
        )
        .await;
    }
    project_edit(
        service, context, scope, "browser-program-sound", "clips.place", edit_context("3"),
        json!({"sequence_id":sequence,"track_id":created["created"]["tracks"][1]["id"],
            "source":{"kind":"registered","asset_id":asset,"stream_index":1},
            "source_range":{"start":t(0,1),"end":t(1,1)},"sequence_start":t(1,2),"collision":"reject"}),
    ).await;
    let mut request = Request::query(
        nw_domain::preview::PROGRAM_SEGMENT_COMMAND,
        json!({"sequence_id":sequence,"revision":"4","range":{"start":t(0,1),"end":t(2,1)}}),
    );
    request.project = Some(ProjectRef {
        id: project.clone(),
        expected_revision: None,
    });
    ProgramFixture {
        project,
        sequence,
        picture,
        request,
    }
}

fn t(num: u32, den: u32) -> Value {
    json!({"num":num.to_string(),"den":den.to_string()})
}

fn color_place(sequence: &str, picture: &str, rgba: [f32; 4], at: u32) -> Value {
    json!({
        "sequence_id":sequence,"track_id":picture,
        "source":{"kind":"solid_color","rgba":rgba,"duration":t(1,1)},
        "source_range":{"start":t(0,1),"end":t(1,1)},"sequence_start":t(at,1),"collision":"reject"
    })
}

async fn project_edit(
    service: &Arc<Service>,
    context: &Context,
    scope: &str,
    key: &str,
    command: &str,
    project: Option<ProjectRef>,
    params: Value,
) -> Value {
    let mut request = Request::query(command, params);
    request.project = project;
    request.replay_scope = Some(scope.into());
    request.idempotency_key = Some(key.into());
    let response = service.dispatch(context.clone(), request).await;
    assert!(
        response.ok,
        "Program fixture edit {command}: {:?}",
        response.error
    );
    assert!(response.receipt_id.is_some());
    response.result.unwrap()
}
