//! Repository automation. Ordinary validation is executed by hosted CI.
use anyhow::{Context, Result, bail};
use sha2::{Digest,Sha256};
use std::{fs, path::{Path,PathBuf}};
fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).with_max_level(tracing::Level::DEBUG).init();
    match run() { Ok(()) => {tracing::info!("Automation completed");std::process::ExitCode::SUCCESS},Err(error) => {tracing::error!(%error,"Automation failed");std::process::ExitCode::FAILURE} }
}
fn run() -> Result<()> {
    let task=std::env::args().nth(1).unwrap_or_else(||"help".into());
    tracing::info!(%task,"Starting repository automation");
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).parent().context("workspace root")?;
    tracing::debug!(root=%root.display(),"Resolved automation workspace");
    match task.as_str() {
        "verify-research" => verify_research(root),
        "schemas" => schemas(root),
        "release-gate" => release_gate(root),
        "help" => {println!("xtask verify-research | schemas | release-gate");Ok(())},
        _ => bail!("Unknown task: {task}"),
    }
}
fn verify_research(root:&Path) -> Result<()> {
    let pack=root.join("docs/research-pack");
    let manifest:serde_json::Value=serde_json::from_slice(&fs::read(pack.join("research-pack-manifest.json"))?)?;
    let rows=manifest["files"].as_array().context("manifest files")?;
    for row in rows {
        let name=row["path"].as_str().context("manifest path")?;
        let relative=PathBuf::from(name);
        if relative.is_absolute() || relative.components().any(|c|!matches!(c,std::path::Component::Normal(_))) {bail!("Unsafe manifest path: {name}");}
        let bytes=fs::read(pack.join(relative)).with_context(||format!("Read archive member {name}"))?;
        let digest=hex::encode(Sha256::digest(&bytes));
        if row["bytes"].as_u64()!=Some(bytes.len() as u64) || row["sha256"].as_str()!=Some(digest.as_str()) {bail!("Research integrity mismatch: {name}");}
    }
    tracing::info!(files=rows.len(),"All preserved research members match their original hashes");Ok(())
}
fn schemas(root:&Path) -> Result<()> {
    let path=root.join("docs/generated");fs::create_dir_all(&path)?;
    fs::write(path.join("commands.json"),serde_json::to_vec_pretty(&nw_domain::commands::registry())?)?;
    fs::write(path.join("request.schema.json"),serde_json::to_vec_pretty(&schemars::schema_for!(nw_domain::commands::Request))?)?;
    fs::write(path.join("streams-clean.schema.json"),serde_json::to_vec_pretty(&schemars::schema_for!(nw_domain::mux::CleanIntent))?)?;
    tracing::info!("Generated schemas from authoritative Rust types");Ok(())
}
fn release_gate(root:&Path) -> Result<()> {
    let ledger:serde_json::Value=serde_json::from_slice(&fs::read(root.join("docs/development/completion.json"))?)?;
    let rows=ledger["records"].as_array().context("completion records")?;
    let pending=rows.iter().filter(|row|row["status"]!="qualified").count();
    if pending>0 {tracing::warn!(pending,"WARNING: Required release evidence remains incomplete");bail!("Release blocked by {pending} incomplete requirement/workflow records");}
    if rows.is_empty() {bail!("An empty ledger cannot qualify a release");}
    Ok(())
}
