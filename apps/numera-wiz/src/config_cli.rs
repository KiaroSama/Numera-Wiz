//! Read a caller-selected configuration file and pass its contents to the owner.
use crate::client::NativeClient;
use anyhow::{Context, Result};
use clap::Subcommand;
use nw_domain::{
    DomainError,
    commands::Request,
    config_import::{CONFIG_BYTES, CompatibilityProfile},
};
use std::{fs::File, io::{Read, Write}, path::{Path, PathBuf}};

#[derive(Subcommand)]
pub enum Command {
    /// Decode FFmWiz settings and return a versioned imported job with field diagnostics.
    ImportFfmwiz {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

pub async fn run(client: &NativeClient, command: Command) -> Result<()> {
    let Command::ImportFfmwiz { input, out } = command;
    let input = input.canonicalize()?;
    let base = nw_platform::files::path_text(input.parent().context("Configuration directory")?)?;
    let file = File::open(&input)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > CONFIG_BYTES as u64 {
        return Err(DomainError::new("RESOURCE_LIMIT", "Select a regular FFmWiz configuration within 64 KiB").into());
    }
    let mut bytes = Vec::new();
    file.take(CONFIG_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > CONFIG_BYTES {
        return Err(DomainError::new("RESOURCE_LIMIT", "FFmWiz configuration exceeded the read bound").into());
    }
    let source = String::from_utf8(bytes)
        .map_err(|_| DomainError::new("VALIDATION_ERROR", "FFmWiz configuration must be UTF-8"))?;
    let request = Request::query("config.import.ffmwiz", serde_json::json!({
        "source": source,
        "base_directory": base,
        "compatibility_profile": CompatibilityProfile::FfmwizV1,
    }));
    let response = client.request(&request).await?;
    if response.ok && let Some(out) = out {
        let parent = out.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut pending = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut pending, &response.result)?;
        pending.write_all(b"\n")?;
        pending.as_file().sync_all()?;
        pending.persist_noclobber(&out).map_err(|error| error.error)?;
    }
    println!("{}", serde_json::to_string_pretty(&response)?);
    if !response.ok {
        return Err(response.error.unwrap_or_else(|| DomainError::new("INTERNAL_ERROR", "Configuration import failed")).into());
    }
    Ok(())
}
