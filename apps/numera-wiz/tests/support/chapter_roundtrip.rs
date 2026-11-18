use crate::{Cli, fixture};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) async fn verify(cli: &Cli, media: &fixture::Media, scope: &str) -> (String, Value) {
    async fn remux(media: &fixture::Media, source: &Path, sidecar: &Path, output: &Path) {
        let args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-i",
            source.to_str().unwrap(),
            "-f",
            "ffmetadata",
            "-i",
            sidecar.to_str().unwrap(),
            "-map",
            "0",
            "-map_metadata",
            "0",
            "-map_chapters",
            "1",
            "-c",
            "copy",
            "-disposition:a:0",
            "default+comment",
            "-disposition:a:1",
            "original",
            output.to_str().unwrap(),
        ]
        .map(str::to_owned);
        nw_platform::process::run(
            &media.bin.join("ffmpeg"),
            &args,
            Duration::from_secs(30),
            4096,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    }

    let directory = media.directory.path();
    let text = directory.join("chapter.txt");
    let prepared = directory.join("chapter.mkv");
    let remuxed = directory.join("chapter-roundtrip.mkv");
    let title = "آغاز شماره #۲; =\\\nپایان";
    std::fs::write(
        &text,
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=125\nEND=875\ntitle=آغاز شماره \\#۲\\; \\=\\\\\\\nپایان\n",
    )
    .unwrap();
    remux(media, &media.source, &text, &prepared).await;
    let first = cli
        .mutation(scope, "chapter-source", "assets.register", json!({"path":prepared}))
        .await;
    assert_eq!(first["ok"], true, "{first}");
    let asset = first["result"]["id"].as_str().unwrap();
    let (inspected, status) = cli.human(&["chapters", "inspect", "--asset", asset]).await;
    assert_eq!(status, 0, "{inspected}");
    let chapter = &inspected["result"]["chapters"][0];
    assert_eq!(chapter["time_base"], json!({"num":"1","den":"1000000000"}));
    assert_eq!(chapter["start_ticks"], "125000000");
    assert_eq!(chapter["end_ticks"], "875000000");
    assert_eq!(chapter["tags"]["title"], title);
    let (export, status) = cli.human(&["chapters", "export", "--asset", asset]).await;
    assert_eq!(status, 0, "{export}");
    let ffmetadata = export["result"]["ffmetadata"].as_str().unwrap();
    assert!(ffmetadata.starts_with(";FFMETADATA1\n[CHAPTER]\n"));
    assert!(!ffmetadata.contains("Fixture artist"));
    assert_eq!(
        export["result"]["sha256"],
        nw_domain::projects::hash(ffmetadata.as_bytes())
    );
    let (all_tags, status) = cli
        .human(&["chapters", "export", "--asset", asset, "--include-global-metadata"])
        .await;
    assert_eq!(status, 0, "{all_tags}");
    assert!(all_tags["result"]["ffmetadata"].as_str().unwrap().contains("artist=Fixture artist\n"));
    std::fs::write(&text, ffmetadata).unwrap();
    remux(media, &prepared, &text, &remuxed).await;
    let observed = observe(media, &remuxed).await;
    assert_eq!(observed["chapters"][0]["start"], 125_000_000);
    assert_eq!(observed["chapters"][0]["end"], 875_000_000);
    assert_eq!(observed["chapters"][0]["tags"]["title"], title);
    assert_eq!(artist(&observed), "Fixture artist");
    (asset.into(), export["result"].clone())
}

pub(super) async fn remove(cli: &Cli, media: &fixture::Media, scope: &str, asset: &str) {
    let output = media.directory.path().join("no-chapters.mkv");
    let (review, status) = cli
        .human(&["chapters", "remove", "--asset", asset, "--out", output.to_str().unwrap()])
        .await;
    assert_eq!(status, 0, "{review}");
    let plan = &review["result"];
    assert_eq!(plan["resolution"]["action"], "remux");
    assert_eq!(plan["resolution"]["preservation"]["chapters"], false);
    assert_eq!(plan["resolution"]["streams"].as_array().unwrap().len(), 3);
    submit(cli, scope, "remove-chapters", plan).await;
    let observed = observe(media, &output).await;
    assert_eq!(observed["chapters"], json!([]));
    assert_eq!(artist(&observed), "Fixture artist");
    assert_eq!(observe(media, &media.directory.path().join("chapter.mkv")).await["chapters"].as_array().unwrap().len(), 1);
}

pub(super) async fn verify_dispositions(cli: &Cli, media: &fixture::Media, scope: &str, asset: &str) {
    let output = media.directory.path().join("new-default-audio.mkv");
    let (review, status) = cli
        .human(&[
            "disposition", "patch", "--asset", asset, "--stream", "2",
            "--exclusive-default", "--out", output.to_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 0, "{review}");
    let plan = &review["result"];
    assert_eq!(plan["resolution"]["action"], "remux");
    assert_eq!(plan["resolution"]["streams"][1]["dispositions"], json!(["comment"]));
    assert_eq!(plan["resolution"]["streams"][2]["dispositions"], json!(["default", "original"]));
    submit(cli, scope, "change-default-audio", plan).await;
    let output = observe(media, &output).await;
    let previous = &output["streams"][1]["disposition"];
    let selected = &output["streams"][2]["disposition"];
    assert_eq!(previous["default"], 0);
    assert_eq!(previous["comment"], 1);
    assert_eq!(selected["default"], 1);
    assert_eq!(selected["original"], 1);
    assert_eq!(output["chapters"].as_array().unwrap().len(), 1);
    assert_eq!(artist(&output), "Fixture artist");
}

async fn submit(cli: &Cli, scope: &str, key: &str, plan: &Value) {
    let submission = cli
        .mutation(
            scope,
            key,
            "jobs.submit",
            json!({"plan_id":plan["id"],"plan_hash":plan["hash"]}),
        )
        .await;
    assert_eq!(submission["ok"], true, "{submission}");
    let id = submission["result"]["job_id"].as_str().unwrap();
    assert_eq!(cli.completed(id).await["state"], "succeeded");
}

async fn observe(media: &fixture::Media, input: &Path) -> Value {
    let args = [
        "-v", "error", "-show_chapters", "-show_format", "-show_streams", "-of", "json", input.to_str().unwrap(),
    ]
    .map(str::to_owned);
    let observed = nw_platform::process::run(
        &media.bin.join("ffprobe"),
        &args,
        Duration::from_secs(10),
        64 * 1024,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    serde_json::from_slice(&observed.stdout).unwrap()
}

fn artist(observed: &Value) -> &Value {
    observed["format"]["tags"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("artist"))
        .unwrap()
        .1
}
