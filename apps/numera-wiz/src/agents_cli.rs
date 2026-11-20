//! Generate local stdio configuration from the installed Rust executable.
use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use nw_domain::DomainError;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Client {
    Claude,
    Codex,
    Cursor,
    Generic,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Generate a client configuration for the selected installation and access profile.
    Setup {
        #[arg(long, value_enum, default_value = "generic")]
        client: Client,
        /// Write to a new file under a private directory.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

impl Command {
    pub(crate) fn run(
        &self,
        data_dir: Option<&Path>,
        components_dir: Option<&Path>,
        profile: Option<&str>,
    ) -> Result<()> {
        let Self::Setup { client, output } = self;
        let executable = std::env::current_exe()?;
        let root = match data_dir {
            Some(path) => std::path::absolute(path)?,
            None => super::install_root::default_data_directory(
                &executable,
                cfg!(target_os = "macos"),
                std::env::home_dir,
            )?,
        };
        let profile = profile.unwrap_or("agent");
        if profile.is_empty()
            || profile.len() > 128
            || !profile
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(DomainError::new("VALIDATION_ERROR", "Invalid access profile name").into());
        }
        let mut args = vec!["--data-dir".into(), config_path(&root)?];
        if let Some(directory) = components_dir {
            args.extend([
                "--components-dir".into(),
                config_path(&std::path::absolute(directory)?)?,
            ]);
        }
        args.extend(
            [
                "--access-profile",
                profile,
                "bridge",
                "--transport",
                "stdio",
            ]
            .map(str::to_owned),
        );
        let document = client.configuration(&config_path(&executable)?, &args)?;
        if let Some(path) = output {
            nw_platform::security::write_private(
                &std::path::absolute(path)?,
                document.as_bytes(),
                false,
            )?;
        } else {
            std::io::stdout().lock().write_all(document.as_bytes())?;
        }
        Ok(())
    }
}

impl Client {
    fn configuration(self, executable: &str, args: &[String]) -> Result<String> {
        let template = match self {
            Self::Codex => {
                return Ok(format!(
                    "[mcp_servers.numera-wiz]\ncommand = {}\nargs = {}\n",
                    serde_json::to_string(executable)?,
                    serde_json::to_string(args)?,
                ));
            }
            Self::Claude => include_str!("../../../integrations/templates/claude-mcp.json"),
            Self::Cursor => include_str!("../../../integrations/templates/cursor-mcp.json"),
            Self::Generic => include_str!("../../../integrations/templates/generic-mcp.json"),
        };
        let mut document: serde_json::Value = serde_json::from_str(template)?;
        let server = document["mcpServers"]["numera-wiz"]
            .as_object_mut()
            .expect("Packaged MCP template has its named server");
        server.insert("command".into(), executable.into());
        server.insert("args".into(), serde_json::to_value(args)?);
        Ok(serde_json::to_string_pretty(&document)? + "\n")
    }
}

fn config_path(path: &Path) -> Result<String> {
    let text = nw_platform::files::path_text(path)?;
    if !path.is_absolute() || text.chars().any(char::is_control) {
        return Err(DomainError::new(
            "VALIDATION_ERROR",
            "Configuration uses absolute text paths without control characters",
        )
        .into());
    }
    Ok(text)
}
