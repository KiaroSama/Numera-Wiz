//! Package the same-source Linux app with the already qualified CPU component.
use crate::package::{digest, output, source};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const TARGET: &str = "x86_64-unknown-linux-gnu";
const COMPONENT: &str = "linux-x64-ffmpeg-component.tar.gz";
const CORRESPONDING: &str = "corresponding-source.tar";
const TOOLS: [&str; 2] = ["ffmpeg", "ffprobe"];

fn input_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+_-".contains(&byte))
}

fn extract_component(archive: &Path, profile: &Path) -> Result<()> {
    let listing = output(Command::new("/usr/bin/tar").args(["-tzf"]).arg(archive))?;
    let members = std::str::from_utf8(&listing)?;
    ensure!(
        (1..=256).contains(&members.lines().count()),
        "Invalid Linux component archive member count"
    );
    for member in members.lines() {
        let member = member.trim_end_matches('/');
        let (group, name) = member.split_once('/').unwrap_or((member, ""));
        ensure!(
            matches!(group, "bin" | "fonts" | "notices") && (name.is_empty() || input_name(name)),
            "Unexpected Linux component archive member"
        );
    }
    output(
        Command::new("/usr/bin/tar")
            .current_dir(profile)
            .args(["--extract", "--gzip", "--no-same-owner", "--file"])
            .arg(archive),
    )?;
    Ok(())
}

fn files(proof: &Value, profile: &Path) -> Result<Vec<Value>> {
    let mut files = Vec::new();
    let runtime = proof["offline_install"]["ffmpeg"]["runtime"]["files"]
        .as_object()
        .context("Qualified Linux runtime closure")?;
    ensure!(
        (3..=127).contains(&runtime.len()) && TOOLS.iter().all(|name| runtime.contains_key(*name)),
        "Incomplete qualified Linux media helpers"
    );
    for (name, expected) in runtime {
        ensure!(input_name(name), "Unexpected Linux media filename");
        let relative = format!("bin/{name}");
        let path = profile.join(&relative);
        ensure!(
            fs::symlink_metadata(&path)?.is_file() && digest(&path)? == *expected,
            "Qualified Linux media bytes differ: {name}"
        );
        let mut entry = expected.clone();
        entry["path"] = json!(relative);
        files.push(entry);
    }
    let font_path = "fonts/DejaVuSans.ttf";
    let mut font = proof["offline_install"]["ffmpeg"]["font"]["file"].clone();
    let installed_font = profile.join(font_path);
    ensure!(
        fs::symlink_metadata(&installed_font)?.is_file() && digest(&installed_font)? == font,
        "Qualified Linux font identity differs"
    );
    font["path"] = json!(font_path);
    files.push(font);
    Ok(files)
}

pub fn build(root: &Path) -> Result<()> {
    let app_source = source(root)?;
    let supplied = root.join("target/qualified-linux-component");
    let candidates = fs::read_dir(supplied)?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        candidates.len() == 1 && candidates[0].file_type()?.is_dir(),
        "Expected one same-run Linux component profile"
    );
    let input = candidates[0].path();
    let metadata = input.join("verified-build.json");
    ensure!(
        fs::metadata(&metadata)?.len() <= 1024 * 1024,
        "Unbounded Linux component proof"
    );
    let proof: Value = serde_json::from_slice(&fs::read(&metadata)?)?;
    ensure!(
        proof["schema_version"] == 1
            && proof["app_source"] == app_source
            && proof["target"] == "linux-x64"
            && proof["offline_relocation"]["network"] == "none",
        "Linux component provenance differs from the requested app source"
    );
    for archive in [COMPONENT, CORRESPONDING] {
        ensure!(
            digest(&input.join(archive))? == proof["archives"][archive],
            "Linux component archive identity differs"
        );
    }
    let parent = nw_platform::security::private_directory(&root.join("target/linux-portable"))?;
    let work = parent.join(format!("build-{}", std::process::id()));
    let profile = work.join("profile");
    fs::create_dir_all(&profile)?;
    extract_component(&input.join(COMPONENT), &profile)?;
    let identity = json!({"target":TARGET,"schema_version":1,"app_source":app_source,"files":files(&proof, &profile)?});
    fs::write(
        profile.join("embed-profile.json"),
        serde_json::to_vec_pretty(&identity)?,
    )?;
    let binary = crate::package::build_portable_executable(root, &profile)?;
    let bundle = work.join("bundle");
    let delivery = work.join("delivery");
    fs::create_dir(&bundle)?;
    fs::create_dir(&delivery)?;
    let executable = bundle.join("numera-wiz");
    fs::copy(binary, &executable)?;
    fs::rename(profile.join("notices"), bundle.join("notices"))?;
    fs::copy(root.join("LICENSE"), bundle.join("LICENSE"))?;
    fs::copy(
        root.join("THIRD-PARTY-NOTICES.md"),
        bundle.join("THIRD-PARTY-NOTICES.md"),
    )?;
    let state = work.join("first-launch-state");
    fs::create_dir(&state)?;
    let first_launch: Value = serde_json::from_slice(&output(
        Command::new(&executable)
            .current_dir(&bundle)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &work)
            .arg("--data-dir")
            .arg(&state)
            .args(["media", "capabilities", "refresh", "--force"]),
    )?)?;
    ensure!(
        first_launch["ok"] == true && first_launch["result"]["cpu_round_trip"] == "exercised",
        "Embedded Linux media did not perform the application FFV1 round trip"
    );
    for name in TOOLS {
        ensure!(
            first_launch["result"][name]["sha256"]
                == proof["offline_install"]["ffmpeg"]["runtime"]["files"][name]["sha256"],
            "The application used a different Linux media helper"
        );
    }
    let archive = delivery.join("numera-wiz-linux-x64.tar.gz");
    output(
        Command::new("/usr/bin/tar")
            .current_dir(bundle)
            .args(["-czf"])
            .arg(&archive)
            .arg("."),
    )?;
    fs::copy(input.join(CORRESPONDING), delivery.join(CORRESPONDING))?;
    let receipt = json!({"source":app_source,"target":TARGET,"executable":digest(&executable)?,
        "application_archive":digest(&archive)?,"component":proof,"first_launch":first_launch,"release_ready":false});
    fs::write(
        delivery.join("linux-portable-package.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}
