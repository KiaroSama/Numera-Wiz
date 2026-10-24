//! Thin development CLI/MCP/GUI composition; domain behavior stays in the service.
mod access;
mod artifacts_cli;
mod client;
mod human;
mod mcp;
mod server;
mod web_assets;
use anyhow::Result;
use clap::{Parser, Subcommand};
use client::NativeClient;
use nw_domain::commands::Request;
use std::{io::Read, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "numera-wiz",
    version,
    about = "Numera Wiz development command interface"
)]
struct Args {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    components_dir: Option<PathBuf>,
    /// Select the effective profile for CLI/stdio, or the profile hosted by serve/HTTP bridge.
    #[arg(long, global = true)]
    access_profile: Option<String>,
    #[command(subcommand)]
    command: Option<Mode>,
}
#[derive(Subcommand)]
enum Mode {
    /// Create and inspect explicit policies, or issue/revoke file-delivered credentials.
    Access {
        #[arg(long, global = true)]
        json: bool,
        #[command(subcommand)]
        command: access::AccessCommand,
    },
    #[command(name = "__owner", hide = true)]
    Owner,
    /// Save an authorized verified artifact to a new client-side file.
    Artifacts {
        #[command(subcommand)]
        command: artifacts_cli::Command,
    },
    Media {
        #[command(subcommand)]
        command: human::Media,
    },
    Assets {
        #[command(subcommand)]
        command: human::Assets,
    },
    Streams {
        #[command(subcommand)]
        command: human::Streams,
    },
    Plans {
        #[command(subcommand)]
        command: human::Plans,
    },
    Jobs {
        #[command(subcommand)]
        command: human::Jobs,
    },
    Batches {
        #[command(subcommand)]
        command: human::Batches,
    },
    /// Review and execute explicit owner data-generation maintenance.
    Maintenance {
        #[command(subcommand)]
        command: human::Maintenance,
    },
    Logs {
        #[command(subcommand)]
        command: human::Logs,
    },
    /// Discover active development commands and their qualification limits.
    Commands {
        #[command(subcommand)]
        command: Option<human::Discovery>,
    },
    /// Submit a bounded canonical JSON request from a file or stdin.
    Request {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Official MCP adapter; default agent access remains discovery-only.
    Bridge {
        #[arg(long, default_value = "stdio", value_parser = ["stdio", "http"])]
        transport: String,
        #[arg(long, default_value_t = 1578)]
        port: u16,
        #[arg(long)]
        token_file: Option<PathBuf>,
    },
    /// Start a loopback development application API and direct HTTP MCP.
    Serve {
        #[arg(long, default_value_t = 1578)]
        port: u16,
        #[arg(long)]
        token_file: Option<PathBuf>,
        #[arg(long)]
        open_browser: bool,
    },
    /// Report current scope without initializing a media GPU or display.
    Doctor,
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    nw_platform::init_logging();
    let status = match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let domain_code = error
                .downcast_ref::<nw_domain::DomainError>()
                .map_or("ADAPTER_ERROR", |error| error.code.as_str());
            tracing::error!(code=domain_code, %error,"Operation failed");
            let code = error
                .downcast_ref::<nw_domain::DomainError>()
                .map(nw_domain::DomainError::exit_code)
                .unwrap_or_else(|| {
                    if error.downcast_ref::<reqwest::Error>().is_some() {
                        8
                    } else {
                        6
                    }
                });
            std::process::ExitCode::from(code)
        }
    };
    let _ = nw_platform::logging::flush();
    status
}
async fn run() -> Result<()> {
    let args = Args::parse();
    tracing::info!("Numera Wiz entry point started");
    let root = args.data_dir.unwrap_or_else(|| {
        std::env::current_exe()
            .expect("executable path")
            .parent()
            .expect("executable directory")
            .to_owned()
    });
    std::fs::create_dir_all(&root)?;
    let root = std::fs::canonicalize(root)?;
    if matches!(&args.command, Some(Mode::Owner)) {
        nw_platform::logging::activate(&root)?;
        tracing::info!("Protected structured owner logging activated");
        return server::run_owner(root, args.components_dir).await;
    }
    let profile = match &args.command {
        Some(Mode::Bridge { transport, .. }) if transport == "stdio" => Some(
            args.access_profile
                .clone()
                .unwrap_or_else(|| "agent".into()),
        ),
        Some(Mode::Bridge { .. } | Mode::Serve { .. }) => None,
        _ => args.access_profile.clone(),
    };
    let client = NativeClient::new(root, args.components_dir, profile).await?;
    match args.command {
        Some(Mode::Owner) => unreachable!("owner was dispatched before frontend initialization"),
        Some(Mode::Access { command, json }) => {
            tracing::debug!(
                json_output = json,
                "Access command uses structured JSON output"
            );
            access::run(&client, command).await
        }
        Some(Mode::Artifacts { command }) => artifacts_cli::run(&client, command).await,
        Some(Mode::Media { command }) => human::run(&client, human::Command::Media(command)).await,
        Some(Mode::Assets { command }) => {
            human::run(&client, human::Command::Assets(command)).await
        }
        Some(Mode::Streams { command }) => {
            human::run(&client, human::Command::Streams(command)).await
        }
        Some(Mode::Plans { command }) => human::run(&client, human::Command::Plans(command)).await,
        Some(Mode::Jobs { command }) => human::run(&client, human::Command::Jobs(command)).await,
        Some(Mode::Batches { command }) => {
            human::run(&client, human::Command::Batches(command)).await
        }
        Some(Mode::Maintenance { command }) => {
            human::run(&client, human::Command::Maintenance(command)).await
        }
        Some(Mode::Logs { command }) => human::run(&client, human::Command::Logs(command)).await,
        Some(Mode::Commands { command }) => {
            human::run(&client, human::Command::Discovery(command)).await
        }
        Some(Mode::Request { file }) => {
            let input: Box<dyn Read> = match file {
                Some(path) => Box::new(std::fs::File::open(path)?),
                None => Box::new(std::io::stdin()),
            };
            let mut bytes = Vec::new();
            input
                .take((nw_domain::commands::MAX_COMMAND_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > nw_domain::commands::MAX_COMMAND_BYTES {
                return Err(
                    nw_domain::DomainError::new("RESOURCE_LIMIT", "Request exceeds 1 MiB").into(),
                );
            }
            print_response(&client, nw_domain::commands::decode_request(&bytes)?).await
        }
        Some(Mode::Doctor) => {
            tracing::warn!(
                "WARNING: Complete runtime, portable payload and GPU qualification remain pending"
            );
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"product":"Numera Wiz","version":env!("CARGO_PKG_VERSION"),"scope":"development cleanup owner and shared workspace","media_gpu_initialized":false,"portable_release":"not_qualified","default_mcp_profile":"discovery_only","default_http_port":1578})
                )?
            );
            Ok(())
        }
        Some(Mode::Bridge {
            transport,
            port,
            token_file,
        }) => match transport.as_str() {
            "stdio" => {
                use rmcp::ServiceExt;
                let running = mcp::Bridge::stdio(client)
                    .serve(rmcp::transport::stdio())
                    .await?;
                running.waiting().await?;
                Ok(())
            }
            "http" => {
                server::serve(
                    client,
                    port,
                    token_file.ok_or_else(|| {
                        nw_domain::DomainError::new(
                            "VALIDATION_ERROR",
                            "HTTP bridge requires --token-file; credentials are never printed",
                        )
                    })?,
                    args.access_profile.unwrap_or_else(|| "agent".into()),
                )
                .await
            }
            _ => Err(nw_domain::DomainError::new(
                "VALIDATION_ERROR",
                "Unsupported transport; choose stdio or http",
            )
            .into()),
        },
        Some(Mode::Serve {
            port,
            token_file,
            open_browser,
        }) => {
            server::serve_browser(
                client,
                port,
                token_file,
                args.access_profile.unwrap_or_else(|| "browser".into()),
                open_browser,
            )
            .await
        }
        None => desktop(client).await,
    }
}
async fn print_response(client: &NativeClient, request: Request) -> Result<()> {
    if matches!(
        request.command.id.as_str(),
        "access.tokens.issue" | "access.credentials.issue"
    ) {
        let error = nw_domain::DomainError::new(
            "VALIDATION_ERROR",
            "Use access tokens issue --for-access-profile NAME --out FILE; raw credential responses are never printed",
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&nw_domain::commands::Response::error(
                &request.request_id,
                error.clone()
            ))?
        );
        return Err(error.into());
    }
    let result = client.request(&request).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !result.ok {
        return Err(result
            .error
            .unwrap_or_else(|| {
                nw_domain::DomainError::new(
                    "INTERNAL_ERROR",
                    "Command failed without a typed error",
                )
            })
            .into());
    }
    Ok(())
}
#[cfg(not(feature = "desktop"))]
async fn desktop(_client: NativeClient) -> Result<()> {
    Err(nw_domain::DomainError::new("CAPABILITY_UNAVAILABLE", "This CI artifact is headless. Use commands/request/bridge/doctor, or select the desktop artifact.").into())
}
#[cfg(feature = "desktop")]
async fn desktop(client: NativeClient) -> Result<()> {
    let bootstrap_started = std::time::Instant::now();
    tracing::info!("Native desktop bootstrap started");
    client.attach().await?;
    let port: std::sync::Arc<dyn nw_ui::CommandClient> = std::sync::Arc::new(NativeUiClient {
        client: client.clone(),
        runtime: tokio::runtime::Handle::current(),
    });
    let session = uuid::Uuid::new_v4().simple().to_string();
    let lease = uuid::Uuid::new_v4().to_string();
    client.client_lease(&lease, false).await?;
    tracing::info!(
        elapsed_ms = bootstrap_started.elapsed().as_millis(),
        "Native desktop owner and lease are ready"
    );
    let heartbeat = client.clone();
    let heartbeat_id = lease.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            if heartbeat.client_lease(&heartbeat_id, false).await.is_err() {
                break;
            }
        }
    });
    let result = eframe::run_native(
        "Numera Wiz",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let workspace = nw_ui::WizApp::new(cc, port, session);
            tracing::info!(
                elapsed_ms = bootstrap_started.elapsed().as_millis(),
                "Shared native workspace constructed"
            );
            Ok(Box::new(workspace))
        }),
    );
    task.abort();
    let _ = client.client_lease(&lease, true).await;
    result.map_err(|e| anyhow::anyhow!("Desktop failed: {e}"))
}

#[cfg(feature = "desktop")]
struct NativeUiClient {
    client: NativeClient,
    runtime: tokio::runtime::Handle,
}
#[cfg(feature = "desktop")]
impl nw_ui::CommandClient for NativeUiClient {
    fn choose_media(&self, reply: nw_ui::Reply) {
        match rfd::FileDialog::new()
            .set_title("Add media to Numera Wiz")
            .pick_file()
        {
            Some(path) => reply(
                nw_platform::files::path_text(&path)
                    .map(|path| {
                        nw_domain::commands::Response::success(
                            "intake",
                            serde_json::json!({"path":path}),
                        )
                    })
                    .map_err(|error| error.to_string()),
            ),
            None => reply(Err("File selection cancelled".into())),
        }
    }
    fn save_artifact(
        &self,
        manifest: nw_domain::artifacts::ArtifactManifest,
        destination: Option<String>,
        control: std::sync::Arc<nw_domain::download::SaveControl>,
        reply: nw_ui::SaveReply,
    ) {
        let output = match destination {
            Some(path) => Some(PathBuf::from(path)),
            None => rfd::FileDialog::new()
                .set_title("Save verified artifact copy")
                .set_file_name(manifest.suggested_file_name())
                .save_file(),
        };
        let Some(output) = output else {
            drop(nw_domain::download::SaveSession(control));
            reply(Err(
                "Save selection cancelled; no client file was published".into(),
            ));
            return;
        };
        let client = self.client.clone();
        self.runtime.spawn(async move {
            reply(
                client
                    .save_artifact(manifest, output, control)
                    .await
                    .map_err(|error| error.to_string()),
            );
        });
    }
    fn send(&self, request: Request, reply: nw_ui::Reply) {
        let client = self.client.clone();
        self.runtime.spawn(async move {
            let response = client
                .request(&request)
                .await
                .map_err(|error| error.to_string());
            reply(response);
        });
    }
}
