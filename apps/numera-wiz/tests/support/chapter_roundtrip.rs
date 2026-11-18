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
        .mutation(
            scope,
            "chapter-source",
            "assets.register",
            json!({"path":prepared}),
        )
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
        .human(&[
            "chapters",
            "export",
            "--asset",
            asset,
            "--include-global-metadata",
        ])
        .await;
    assert_eq!(status, 0, "{all_tags}");
    assert!(
        all_tags["result"]["ffmetadata"]
            .as_str()
            .unwrap()
            .contains("artist=Fixture artist\n")
    );
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
        .human(&[
            "chapters",
            "remove",
            "--asset",
            asset,
            "--out",
            output.to_str().unwrap(),
        ])
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
    assert_eq!(
        observe(media, &media.directory.path().join("chapter.mkv")).await["chapters"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

pub(super) async fn import(cli: &Cli, media: &fixture::Media, scope: &str, export: &Value) {
    let directory = media.directory.path();
    let registered = cli
        .mutation(scope, "chapter-import-source", "assets.register", json!({"path":directory.join("no-chapters.mkv")}))
        .await;
    assert_eq!(registered["ok"], true, "{registered}");
    let asset = registered["result"]["id"].as_str().unwrap();
    let ffmetadata = export["ffmetadata"].as_str().unwrap();
    let doc = format!(";FFMETADATA1\nartist=Sidecar artist\n{}", ffmetadata.strip_prefix(";FFMETADATA1\n").unwrap());
    let sidecar = directory.join("chapter-input.ffmetadata");
    std::fs::write(&sidecar, &doc).unwrap();
    let output = directory.join("chapters-imported.mkv");
    let (review, status) = cli
        .human(&[
            "chapters", "import", "--asset", asset, "--out", output.to_str().unwrap(),
            "--ffmetadata", sidecar.to_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 0, "{review}");
    submit(cli, scope, "import-registered-chapters", &review["result"]).await;
    let original = observe(media, &directory.join("chapter.mkv")).await;
    let observed = observe(media, &output).await;
    assert_eq!(observed["chapters"], original["chapters"]);
    assert_eq!(artist(&observed), "Fixture artist");

    let output = directory.join("chapters-with-global-tags.mkv");
    let plan = cli.query("chapters.import", json!({
        "asset_id":asset,"output":output,"ffmetadata":doc,"include_global_metadata":true
    })).await;
    submit(cli, scope, "import-chapter-tags", &plan).await;
    let observed = observe(media, &output).await;
    assert_eq!(observed["chapters"], original["chapters"]);
    assert_eq!(artist(&observed), "Sidecar artist");
    assert_eq!(tag(&observed["format"], "title"), tag(&original["format"], "title"));

    for document in [
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=99999\n",
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/3\nSTART=1\nEND=2\n",
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=500\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=499\nEND=600\n",
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=500\nEND=600\n",
    ] {
        let invalid = cli.raw(&nw_domain::commands::Request::query("chapters.import", json!({
            "asset_id":asset,"output":directory.join("rejected-chapters.mkv"),"ffmetadata":document
        }))).await;
        assert_eq!(invalid["error"]["code"], "INVALID_CHAPTERS", "{invalid}");
    }
}

pub(super) async fn verify_dispositions(
    cli: &Cli,
    media: &fixture::Media,
    scope: &str,
    asset: &str,
) {
    let output = media.directory.path().join("new-default-audio.mkv");
    let (review, status) = cli
        .human(&[
            "disposition",
            "patch",
            "--asset",
            asset,
            "--stream",
            "2",
            "--exclusive-default",
            "--out",
            output.to_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 0, "{review}");
    let plan = &review["result"];
    assert_eq!(plan["resolution"]["action"], "remux");
    assert_eq!(
        plan["resolution"]["streams"][1]["dispositions"],
        json!(["comment"])
    );
    assert_eq!(
        plan["resolution"]["streams"][2]["dispositions"],
        json!(["default", "original"])
    );
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

pub(super) async fn verify_metadata(cli: &Cli, media: &fixture::Media, scope: &str, asset: &str) {
    let directory = media.directory.path();
    let global = directory.join("global-metadata.mkv");
    let patch = directory.join("global-metadata.json");
    std::fs::write(
        &patch,
        r#"{"target":{"scope":"global"},"set":{"project_code":"ویرایش شماره ۲"},"remove":["title"]}"#,
    )
    .unwrap();
    let (review, status) = cli
        .human(&[
            "metadata", "patch", "--asset", asset, "--out", global.to_str().unwrap(),
            "--patch", patch.to_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 0, "{review}");
    let plan = &review["result"];
    assert_eq!(plan["resolution"]["action"], "remux");
    assert_eq!(plan["resolution"]["metadata_patch"]["target"]["scope"], "global");
    submit(cli, scope, "patch-container-metadata", plan).await;
    let global = observe(media, &global).await;
    let source = observe(media, &directory.join("chapter.mkv")).await;
    assert_eq!(tag(&global["format"], "project_code"), Some(&json!("ویرایش شماره ۲")));
    assert!(tag(&global["format"], "title").is_none());
    assert_eq!(artist(&global), artist(&source));
    assert_eq!(global["chapters"], source["chapters"]);

    let stream = directory.join("stream-metadata.mkv");
    let input = json!({"asset_id":asset, "output":stream, "patch":{
        "target":{"scope":"stream","index":2},
        "set":{"title":"گویندهٔ دوم","project_code":"cue=2"},"remove":["language"]
    }});
    let plan = cli.query("metadata.patch", input.clone()).await;
    submit(cli, scope, "patch-track-metadata", &plan).await;
    let observed = observe(media, &stream).await;
    for key in ["language", "review_note"] {
        assert_eq!(tag(&observed["streams"][1], key), tag(&source["streams"][1], key));
    }
    assert_eq!(observed["streams"][1]["disposition"], source["streams"][1]["disposition"]);
    assert_eq!(tag(&observed["streams"][2], "title"), Some(&json!("گویندهٔ دوم")));
    assert_eq!(tag(&observed["streams"][2], "project_code"), Some(&json!("cue=2")));
    assert!(tag(&observed["streams"][2], "language").is_none());
    assert_eq!(artist(&observed), artist(&source));

    let mut contradictory = input;
    contradictory["output"] = json!(directory.join("invalid-tag-change.mkv"));
    contradictory["patch"]["remove"] = json!(["title"]);
    let invalid = cli.raw(&nw_domain::commands::Request::query("metadata.patch", contradictory)).await;
    assert_eq!(invalid["error"]["code"], "VALIDATION_ERROR");
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
        "-v",
        "error",
        "-show_chapters",
        "-show_format",
        "-show_streams",
        "-of",
        "json",
        input.to_str().unwrap(),
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
    tag(&observed["format"], "artist").unwrap()
}

fn tag<'a>(row: &'a Value, name: &str) -> Option<&'a Value> {
    row["tags"].as_object()?.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(name).then_some(value)
    })
}
