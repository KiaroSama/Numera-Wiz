//! Actual CI-built native window and human input, exercised under hosted X11.
#![cfg(target_os = "linux")]
#[allow(
    dead_code,
    reason = "This shared fixture also supports deeper output oracles"
)]
#[path = "../../../test-support/media.rs"]
mod fixture;
use nw_domain::commands::Request;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
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
    click(&window, "331", "570").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    screenshot(&window, &evidence.join("native-reviewed-workspace.png")).await;
    input(&["key", "--clearmodifiers", "ctrl+Return"]).await;
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
