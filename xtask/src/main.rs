//! Repository automation. Ordinary validation is executed by hosted CI.
use anyhow::{Context, Result, bail};
mod web;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn main() -> std::process::ExitCode {
    nw_platform::init_logging();
    match run() {
        Ok(()) => {
            tracing::info!("Automation completed");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(%error,"Automation failed");
            std::process::ExitCode::FAILURE
        }
    }
}
fn run() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    tracing::info!(%task,"Starting repository automation");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root")?;
    tracing::debug!(root=%root.display(),"Resolved automation workspace");
    match task.as_str() {
        "verify-research" => verify_research(root),
        "schemas" => schemas(root),
        "release-gate" => release_gate(root),
        "build-fixture-profile" => build_fixture_profile(root),
        "build-web" => web::build(root),
        "help" => {
            println!("xtask verify-research | schemas | release-gate");
            Ok(())
        }
        _ => bail!("Unknown task: {task}"),
    }
}
fn verify_research(root: &Path) -> Result<()> {
    let pack = root.join("docs/research-pack");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(pack.join("research-pack-manifest.json"))?)?;
    let rows = manifest["files"].as_array().context("manifest files")?;
    for row in rows {
        let name = row["path"].as_str().context("manifest path")?;
        let relative = PathBuf::from(name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            bail!("Unsafe manifest path: {name}");
        }
        let bytes =
            fs::read(pack.join(relative)).with_context(|| format!("Read archive member {name}"))?;
        let digest = hex::encode(Sha256::digest(&bytes));
        if row["bytes"].as_u64() != Some(bytes.len() as u64)
            || row["sha256"].as_str() != Some(digest.as_str())
        {
            bail!("Research integrity mismatch: {name}");
        }
    }
    tracing::info!(
        files = rows.len(),
        "All preserved research members match their original hashes"
    );
    Ok(())
}
fn schemas(root: &Path) -> Result<()> {
    let path = root.join("docs/generated");
    fs::create_dir_all(&path)?;
    fs::write(
        path.join("commands.json"),
        serde_json::to_vec_pretty(&nw_domain::commands::active_registry())?,
    )?;
    fs::write(
        path.join("request.schema.json"),
        serde_json::to_vec_pretty(&schemars::schema_for!(nw_domain::commands::Request))?,
    )?;
    fs::write(
        path.join("streams-clean.schema.json"),
        serde_json::to_vec_pretty(&schemars::schema_for!(nw_domain::mux::CleanIntent))?,
    )?;
    fs::write(path.join("artifact-transfer.schema.json"), serde_json::to_vec_pretty(&schemars::schema_for!(nw_domain::artifacts::ArtifactChunk))?)?;
    fs::write(path.join("artifact-manifest.schema.json"), serde_json::to_vec_pretty(&schemars::schema_for!(nw_domain::artifacts::ArtifactManifest))?)?;
    tracing::info!("Generated schemas from authoritative Rust types");
    Ok(())
}
fn release_gate(root: &Path) -> Result<()> {
    let ledger: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("docs/development/completion.json"))?)?;
    let rows = ledger["records"].as_array().context("completion records")?;
    for (prefix, count) in [("R", 22), ("F", 15), ("CD", 34), ("WP", 12)] {
        for index in 1..=count {
            let id = format!("{prefix}{index:02}");
            let matches = rows.iter().filter(|row| row["subject_id"] == id).count();
            if matches != 1 {
                bail!("Release requires exactly one evidence record for {id}");
            }
        }
    }
    for row in rows.iter().filter(|row| row["status"] == "qualified") {
        let evidence = row["evidence"]
            .as_array()
            .context("Qualified record requires actual evidence")?;
        if evidence.is_empty() {
            bail!("Qualified record has no evidence: {}", row["subject_id"]);
        }
        for item in evidence {
            let run = item["ci_run_url"]
                .as_str()
                .context("Actual CI run URL required")?;
            if !run.starts_with("https://github.com/KiaroSama/Numera-Wiz/actions/runs/") {
                bail!("Invalid CI evidence origin");
            }
            let hash = item["artifact_sha256"]
                .as_str()
                .context("Exact artifact hash required")?;
            if hash.len() != 64
                || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                || hash == "0".repeat(64)
            {
                bail!("Invalid artifact hash");
            }
            if item["asserted_domain"].as_str().is_none_or(str::is_empty)
                || item["observed_outcome"].as_str().is_none_or(str::is_empty)
            {
                bail!("Evidence requires domain and observed outcome");
            }
        }
    }
    let pending = rows
        .iter()
        .filter(|row| row["status"] != "qualified")
        .count();
    if pending > 0 {
        tracing::warn!(
            pending,
            "WARNING: Required release evidence remains incomplete"
        );
        bail!("Release blocked by {pending} incomplete requirement/workflow records");
    }
    if rows.is_empty() {
        bail!("An empty ledger cannot qualify a release");
    }
    Ok(())
}

fn build_fixture_profile(root: &Path) -> Result<()> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        bail!("This source build belongs in GitHub CI");
    }
    let source = root.join("target/fixture-ffmpeg-src");
    let output = root.join("target/fixture-components");
    let flags = [
        "--disable-doc",
        "--disable-debug",
        "--disable-network",
        "--disable-autodetect",
        "--disable-everything",
        "--disable-x86asm",
        "--enable-ffmpeg",
        "--enable-ffprobe",
        "--enable-protocol=file,pipe,fd",
        "--enable-demuxer=matroska,mov,wav,srt",
        "--enable-muxer=matroska,mov,mp4,null",
        "--enable-encoder=ffv1,mpeg4,pcm_s16le",
        "--enable-decoder=ffv1,mpeg4,pcm_s16le,subrip,wrapped_avframe",
        "--enable-indev=lavfi",
        "--enable-filter=testsrc2,sine,anull,null,aformat,format,aresample",
        "--enable-swscale",
        "--enable-swresample",
    ];
    let mut configure = std::process::Command::new("sh");
    configure
        .current_dir(&source)
        .arg("configure")
        .args(flags)
        .arg(format!("--prefix={}", output.display()));
    tracing::info!(argv=?configure,"Preparing the CI-only FFmpeg fixture build");
    if !configure.status()?.success() {
        bail!("FFmpeg fixture configure failed");
    }
    for arguments in [["-j2"].as_slice(), ["install"].as_slice()] {
        let mut make = std::process::Command::new("make");
        make.current_dir(&source).args(arguments);
        tracing::info!(argv=?make,"Running fixture build stage");
        if !make.status()?.success() {
            bail!("FFmpeg fixture build stage failed");
        }
    }
    tracing::warn!(
        "WARNING: This limited fixture component profile is not the required standard distribution profile"
    );
    Ok(())
}
