//! Actual CI-built native window and human input, exercised under hosted X11.
#![cfg(target_os = "linux")]
#[path = "support/evidence.rs"]
mod evidence;
#[allow(
    dead_code,
    reason = "This shared fixture also supports deeper output oracles"
)]
#[path = "../../../test-support/media.rs"]
mod fixture;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use evidence::RetainLogs;
use nw_domain::commands::Request;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
async fn xdo(args: &[&str]) -> std::process::Output {
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("/usr/bin/xdotool")
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("X11 input deadline")
    .expect("X11 input command")
}
async fn input(args: &[&str]) {
    assert!(
        xdo(args).await.status.success(),
        "X11 input operation failed"
    );
}
async fn click(window: &str, x: &str, y: &str) {
    input(&["mousemove", "--window", window, x, y]).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    input(&["mousedown", "1"]).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    input(&["mouseup", "1"]).await;
}
async fn screenshot(window: &str, path: &Path) {
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("/usr/bin/import")
            .args(["-window", window])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(status.success(), "Native screenshot failed");
}
async fn query(root: &Path, id: &str) -> Option<Value> {
    query_params(root, id, json!({})).await
}
async fn query_params(root: &Path, id: &str, params: Value) -> Option<Value> {
    let bytes =
        nw_platform::security::read_private(&root.join("runtime/owner.json"), 16384).ok()?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    let port = record["port"].as_u64()?;
    let bearer = record["bearer"].as_str()?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?
        .post(format!("http://127.0.0.1:{port}/api/v1/commands"))
        .bearer_auth(bearer)
        .json(&Request::query(id, params))
        .send()
        .await
        .ok()?;
    let value: Value = response.json().await.ok()?;
    if value["ok"] == true {
        Some(value["result"].clone())
    } else {
        None
    }
}
#[tokio::test]
#[ignore = "Actual desktop input runs only with the hosted matching executable and Xvfb"]
async fn native_window_imports_real_media_through_the_human_controls() {
    nw_platform::init_logging();
    tokio::time::timeout(Duration::from_secs(150), journey())
        .await
        .expect("Native UI journey deadline");
}
async fn journey() {
    let executable = PathBuf::from(
        std::env::var_os("NW_DESKTOP_EXE").expect("Matching CI-built desktop executable"),
    );
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let mut wm = tokio::process::Command::new("/usr/bin/openbox")
        .arg("--sm-disable")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("Hosted X11 window manager");
    let media = fixture::generate().await;
    let root = media.directory.path().join("native-data");
    let _owner_logs = RetainLogs {
        source: root.join("logs"),
        destination: evidence.join("native-owner-logs"),
    };
    let identity =
        nw_platform::files::identity(&executable, &tokio_util::sync::CancellationToken::new())
            .unwrap();
    let log = std::fs::File::create(evidence.join("native-stderr.log")).unwrap();
    let mut app = tokio::process::Command::new(&executable)
        .arg("--data-dir")
        .arg(&root)
        .arg("--components-dir")
        .arg(&media.bin)
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("WINIT_X11_SCALE_FACTOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let start = Instant::now();
    let window = loop {
        let output = xdo(&["search", "--onlyvisible", "--name", "^Numera Wiz$"]).await;
        let text = String::from_utf8(output.stdout).unwrap();
        if output.status.success()
            && let Some(window) = text.lines().next()
            && !window.is_empty()
            && window.bytes().all(|byte| byte.is_ascii_digit())
        {
            break window.to_owned();
        }
        if let Some(status) = app.try_wait().unwrap() {
            panic!("Native application exited before showing its workspace: {status}");
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "Native workspace did not open within the functional CI deadline; inspect retained startup timings"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    std::fs::write(evidence.join("native-startup.json"), serde_json::to_vec_pretty(&json!({"visible_window_ms":start.elapsed().as_millis().to_string(),"functional_deadline_seconds":60,"scope":"Unoptimized development executable under hosted X11/software GL; not packaged startup or memory qualification"})).unwrap()).unwrap();
    input(&["windowsize", &window, "1440", "861"]).await;
    input(&["windowactivate", "--sync", &window]).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    screenshot(&window, &evidence.join("native-workspace.png")).await;
    click(&window, "560", "116").await;
    input(&["key", "--clearmodifiers", "ctrl+a"]).await;
    input(&[
        "type",
        "--clearmodifiers",
        "--delay",
        "2",
        media.source.to_str().unwrap(),
    ])
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    click(&window, "995", "116").await;
    let start = Instant::now();
    let assets = loop {
        if let Some(assets) = query(&root, "assets.list").await
            && assets["items"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        {
            break assets["items"].clone();
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "Human input did not register its actual native media source"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let asset = query_params(&root, "assets.inspect", json!({"id":assets[0]["id"]}))
        .await
        .unwrap();
    assert_eq!(
        asset["identity"]["path"].as_str().unwrap(),
        std::fs::canonicalize(&media.source)
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(asset["probe"]["streams"].as_array().unwrap().len(), 3);
    tokio::time::sleep(Duration::from_millis(500)).await;
    screenshot(&window, &evidence.join("native-loaded-workspace.png")).await;
    click(&window, "331", "591").await;
    let started = Instant::now();
    let report_event = loop {
        let log = std::fs::read_to_string(root.join("logs/owner.jsonl")).unwrap();
        if let Some(event) = log
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| {
                event["fields"]["message"] == "command.completed"
                    && event["fields"]["command"] == "streams.clean.report"
                    && event["fields"]["ok"] == true
                    && event["fields"]["request_id"] != "local"
            })
        {
            break event;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Human Selection report input did not complete its canonical request"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    screenshot(&window, &evidence.join("native-selection-report.png")).await;
    {
        use std::io::Read as _;
        let mut png = Vec::new();
        std::fs::File::open(evidence.join("native-selection-report.png"))
            .unwrap()
            .take(256 * 1024 + 1)
            .read_to_end(&mut png)
            .unwrap();
        if png.len() <= 256 * 1024 {
            println!("NW_NATIVE_SELECTION_IMAGE:{}", STANDARD.encode(png));
        }
    }
    let report = query_params(
        &root,
        "streams.clean.report",
        json!({"asset_ids":[assets[0]["id"]],"intent":fixture::rules()}),
    )
    .await
    .unwrap();
    assert_eq!(report["resolved_files"], 1);
    assert_eq!(
        report["files"][0]["selected_original_indices"],
        json!([0, 1, 2])
    );
    std::fs::write(evidence.join("native-selection-report.json"), serde_json::to_vec_pretty(&json!({"ui_completion_event":report_event,"independent_report":report,"scope":"Actual Selection report pointer input completed before the harness query, retained window PNG, Escape closes the report"})).unwrap()).unwrap();
    input(&["key", "--clearmodifiers", "Escape"]).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    click(&window, "331", "570").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    screenshot(&window, &evidence.join("native-reviewed-workspace.png")).await;
    input(&["windowactivate", "--sync", &window]).await;
    input(&["key", "--clearmodifiers", "ctrl+Return"]).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    screenshot(&window, &evidence.join("native-run-requested.png")).await;
    let start = Instant::now();
    let job = loop {
        if let Some(jobs) = query(&root, "jobs.list").await
            && let Some(summary) = jobs["items"].as_array().and_then(|jobs| jobs.first())
            && let Some(job) =
                query_params(&root, "jobs.inspect", json!({"id":summary["id"]})).await
            && let Ok(job) = serde_json::from_value::<nw_domain::views::JobView>(job)
            && job.state.terminal()
        {
            break job;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "Native review/Run controls did not produce a terminal job"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert_eq!(job.state, nw_domain::records::JobState::Succeeded);
    let artifact = job
        .artifact
        .as_ref()
        .expect("Native UI must produce a verified artifact");
    assert_eq!(
        std::fs::read(&media.source).unwrap(),
        std::fs::read(
            artifact
                .path
                .as_ref()
                .expect("Owner inspection retains the authorized output path")
        )
        .unwrap()
    );
    assert_eq!(job.plan.source.id, assets[0]["id"].as_str().unwrap());
    std::fs::write(evidence.join("native-output.json"), serde_json::to_vec_pretty(&json!({"job_id":job.id,"plan_hash":job.plan.hash,"artifact":artifact,"verification":job.verification,"oracle":"Independent source/output byte equality after native input, Review and Ctrl+Enter Run"})).unwrap()).unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    screenshot(&window, &evidence.join("native-completed-workspace.png")).await;
    // Observe the canonical UI request before this harness asks for its own manifest oracle.
    click(&window, "533", "850").await;
    let started = Instant::now();
    let manifest_event = loop {
        let log = std::fs::read_to_string(root.join("logs/owner.jsonl")).unwrap();
        if let Some(event) = log
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| {
                event["fields"]["message"] == "command.completed"
                    && event["fields"]["command"] == "artifacts.manifest"
                    && event["fields"]["ok"] == true
                    && event["fields"]["request_id"] != "local"
            })
        {
            break event;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Human Output details input did not complete a canonical manifest request"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    tokio::time::sleep(Duration::from_millis(700)).await;
    screenshot(&window, &evidence.join("native-output-details.png")).await;
    // A bounded image transport keeps visual review possible without running a local GUI.
    {
        use std::io::Read as _;
        let mut png = Vec::new();
        std::fs::File::open(evidence.join("native-output-details.png"))
            .unwrap()
            .take(256 * 1024 + 1)
            .read_to_end(&mut png)
            .unwrap();
        if png.len() <= 256 * 1024 {
            println!("NW_NATIVE_MANIFEST_IMAGE:{}", STANDARD.encode(png));
        }
    }
    let manifest = query_params(
        &root,
        "artifacts.manifest",
        json!({"artifact_id":format!("job-{}",job.id)}),
    )
    .await
    .unwrap();
    assert_eq!(manifest["sha256"], artifact.sha256);
    assert_eq!(manifest["encoded_byte_count"], artifact.bytes);
    std::fs::write(evidence.join("native-manifest.json"), serde_json::to_vec_pretty(&json!({"ui_completion_event":manifest_event,"manifest":manifest,"scope":"Actual Output details mouse input, canonical completion before the independent manifest query, and retained dialog screenshot"})).unwrap()).unwrap();
    // Coordinates match the personally inspected CI75 Output details screenshot.
    // Real pointer/keyboard input edits the destination and activates Save.
    let saved_path = evidence.join("native-client-saved-copy.mkv");
    assert!(!saved_path.exists());
    click(&window, "300", "400").await;
    input(&["key", "--clearmodifiers", "ctrl+a"]).await;
    input(&[
        "type",
        "--clearmodifiers",
        "--delay",
        "1",
        saved_path.to_str().unwrap(),
    ])
    .await;
    click(&window, "100", "436").await;
    let started = Instant::now();
    loop {
        if saved_path.exists()
            && let Ok(saved) = nw_platform::files::identity(&saved_path, &CancellationToken::new())
            && saved.sha256 == artifact.sha256
            && saved.bytes == artifact.bytes
        {
            break;
        }
        if started.elapsed() >= Duration::from_secs(20) {
            screenshot(&window, &evidence.join("native-client-save-timeout.png")).await;
            let png = std::fs::read(evidence.join("native-client-save-timeout.png")).unwrap();
            if png.len() <= 256 * 1024 {
                println!("NW_NATIVE_SAVE_FAILURE_IMAGE:{}", STANDARD.encode(png));
            }
            panic!("Native human Save verified copy controls did not publish the selected bytes");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        std::fs::read(&saved_path).unwrap(),
        std::fs::read(&media.source).unwrap()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    screenshot(&window, &evidence.join("native-client-saved.png")).await;
    let png = std::fs::read(evidence.join("native-client-saved.png")).unwrap();
    if png.len() <= 256 * 1024 {
        println!("NW_NATIVE_SAVE_IMAGE:{}", STANDARD.encode(png));
    }
    let producer_job_count = query(&root, "jobs.list").await.unwrap()["items"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(producer_job_count, 1);
    std::fs::write(evidence.join("native-client-save.json"), serde_json::to_vec_pretty(&json!({
        "artifact_id":manifest["artifact_id"], "sha256":artifact.sha256, "bytes":artifact.bytes,
        "oracle":"Actual destination-field pointer/keyboard input and Save button pointer activation; independent saved/source byte equality",
        "producer_job_count":producer_job_count,
        "not_qualified":["native OS Save As picker", "other desktop platforms", "browser file handles"]
    })).unwrap()).unwrap();
    // A second actual Save uses the native OS picker, with an empty client field.
    click(&window, "300", "400").await;
    input(&["key", "--clearmodifiers", "ctrl+a", "BackSpace"]).await;
    click(&window, "100", "436").await;
    let started = Instant::now();
    let picker = loop {
        let found = xdo(&["search", "--onlyvisible", "--name", "^Save"]).await;
        if found.status.success() {
            let windows = String::from_utf8(found.stdout).unwrap();
            if let Some(window) = windows.lines().last() { break window.to_owned(); }
        }
        if started.elapsed() >= Duration::from_secs(20) {
            screenshot("root", &evidence.join("native-picker-timeout.png")).await;
            let png = std::fs::read(evidence.join("native-picker-timeout.png")).unwrap();
            if png.len() <= 256 * 1024 { println!("NW_NATIVE_PICKER_FAILURE_IMAGE:{}", STANDARD.encode(png)); }
            panic!("Actual native Save control did not open its OS file picker");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    screenshot(&picker, &evidence.join("native-save-picker.png")).await;
    let png = std::fs::read(evidence.join("native-save-picker.png")).unwrap();
    if png.len() <= 256 * 1024 { println!("NW_NATIVE_PICKER_IMAGE:{}", STANDARD.encode(png)); }
    let picker_saved_path = evidence.join("native-picker-saved-copy.mkv");
    assert!(!picker_saved_path.exists());
    input(&["windowactivate", "--sync", &picker]).await;
    input(&["key", "--clearmodifiers", "ctrl+l"]).await;
    input(&["type", "--clearmodifiers", "--delay", "1", picker_saved_path.to_str().unwrap()]).await;
    input(&["key", "--clearmodifiers", "Return"]).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    if xdo(&["search", "--onlyvisible", "--name", "^Save"]).await.status.success() {
        input(&["key", "--clearmodifiers", "alt+s"]).await;
    }
    let started = Instant::now();
    loop {
        if picker_saved_path.exists()
            && let Ok(saved) = nw_platform::files::identity(&picker_saved_path, &CancellationToken::new())
            && saved.sha256 == artifact.sha256 && saved.bytes == artifact.bytes { break; }
        if started.elapsed() >= Duration::from_secs(20) {
            screenshot("root", &evidence.join("native-picker-save-timeout.png")).await;
            let png = std::fs::read(evidence.join("native-picker-save-timeout.png")).unwrap();
            if png.len() <= 256 * 1024 { println!("NW_NATIVE_PICKER_FAILURE_IMAGE:{}", STANDARD.encode(png)); }
            panic!("Native OS picker and authenticated copy did not publish the selected bytes");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(std::fs::read(&picker_saved_path).unwrap(), std::fs::read(&media.source).unwrap());
    assert_eq!(query(&root,"jobs.list").await.unwrap()["items"].as_array().unwrap().len(), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    screenshot(&window, &evidence.join("native-picker-saved.png")).await;
    let png = std::fs::read(evidence.join("native-picker-saved.png")).unwrap();
    if png.len() <= 256 * 1024 { println!("NW_NATIVE_PICKER_SAVE_IMAGE:{}", STANDARD.encode(png)); }
    std::fs::write(evidence.join("native-picker-save.json"),serde_json::to_vec_pretty(&json!({
        "artifact_id":manifest["artifact_id"],"sha256":artifact.sha256,"bytes":artifact.bytes,
        "oracle":"Actual empty destination field, native Save pointer input, real OS file picker and independent saved/source byte equality",
        "producer_job_count":1,
        "not_qualified":["other desktop platforms", "crash recovery", "full product"]
    })).unwrap()).unwrap();
    let record: Value = serde_json::from_slice(
        &nw_platform::security::read_private(&root.join("runtime/owner.json"), 16384).unwrap(),
    )
    .unwrap();
    assert_eq!(record["build"], identity.sha256);
    std::fs::write(evidence.join("native.json"),serde_json::to_vec_pretty(&json!({"schema_version":1,"source":std::env::var("GITHUB_SHA").ok(),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"runner_image":std::env::var("ImageVersion").ok(),"executable":identity,"registry":record["registry"],"renderer":"Glow; LIBGL_ALWAYS_SOFTWARE=1","viewport":[1440,861],"observed":"Real native CI executable, window input, media registration, Review and keyboard Run, verified byte-identical output","not_qualified":["Windows or macOS native input","preview/playback","full five-surface qualification"]})).unwrap()).unwrap();
    input(&["windowactivate", "--sync", &window]).await;
    input(&["key", "--clearmodifiers", "alt+F4"]).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(10), app.wait())
            .await
            .expect("Native close did not drain its client lease")
            .unwrap()
            .success()
    );
    let _ = wm.kill().await;
    let _ = wm.wait().await;
}
