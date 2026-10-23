//! Thin development CLI/MCP/GUI composition; domain behavior stays in the service.
mod client;
mod mcp;
mod server;
mod web_assets;
use anyhow::{Context, Result, bail};
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
    #[command(subcommand)]
    command: Option<Mode>,
}
#[derive(Subcommand)]
enum Mode {
    #[command(name = "__owner", hide = true)]
    Owner,
    /// Discover active development commands and their qualification limits.
    Commands,
    /// Submit a bounded canonical JSON query from a file or stdin.
    Request {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Official MCP adapter; default agent access remains discovery-only.
    Bridge {
        #[arg(long, default_value = "agent")]
        access_profile: String,
        #[arg(long, default_value = "stdio")]
        transport: String,
        #[arg(long, default_value_t = 1578)]
        port: u16,
        #[arg(long)]
        token_file: Option<PathBuf>,
    },
    /// Start a loopback development application API and direct HTTP MCP.
    Serve {
        #[arg(long, default_value = "browser")]
        access_profile: String,
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
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error,"Operation failed");
            std::process::ExitCode::FAILURE
        }
    }
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
        return server::run_owner(root, args.components_dir).await;
    }
    let profile = match &args.command {
        Some(Mode::Bridge {
            access_profile,
            transport,
            ..
        }) if transport == "stdio" => Some(access_profile.clone()),
        _ => None,
    };
    let client = NativeClient::new(root, args.components_dir, profile).await?;
    match args.command {
        Some(Mode::Owner) => unreachable!("owner was dispatched before frontend initialization"),
        Some(Mode::Commands) => {
            print_response(
                &client,
                Request::query("commands.list", serde_json::json!({})),
            )
            .await
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
                bail!("Request exceeds 1 MiB");
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
                    &serde_json::json!({"product":"Numera Wiz","version":env!("CARGO_PKG_VERSION"),"scope":"development command/resolver; media owner integration pending","media_gpu_initialized":false,"portable_release":"not_qualified","default_mcp_profile":"discovery_only","default_http_port":1578})
                )?
            );
            Ok(())
        }
        Some(Mode::Bridge {
            transport,
            port,
            token_file,
            access_profile,
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
                    token_file.context(
                        "HTTP bridge requires --token-file; credentials are never printed",
                    )?,
                    access_profile,
                )
                .await
            }
            _ => bail!("Unsupported transport; choose stdio or http"),
        },
        Some(Mode::Serve {
            port,
            token_file,
            access_profile,
            open_browser,
        }) => server::serve_browser(client, port, token_file, access_profile, open_browser).await,
        None => desktop(client).await,
    }
}
async fn print_response(client: &NativeClient, request: Request) -> Result<()> {
    let result = client.request(&request).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !result.ok {
        bail!("Command returned a domain error; inspect the JSON result");
    }
    Ok(())
}
#[cfg(not(feature = "desktop"))]
async fn desktop(_client: NativeClient) -> Result<()> {
    bail!(
        "This CI artifact is headless. Use commands/request/bridge/doctor, or select the separately built desktop artifact."
    )
}
#[cfg(feature = "desktop")]
async fn desktop(client: NativeClient) -> Result<()> {
    client.attach().await?;
    let port: std::sync::Arc<dyn nw_ui::CommandClient> = std::sync::Arc::new(NativeUiClient {
        client: client.clone(),
        runtime: tokio::runtime::Handle::current(),
    });
    let session = uuid::Uuid::new_v4().simple().to_string();
    let lease = uuid::Uuid::new_v4().to_string();
    client
        .internal("internal/lease", serde_json::json!({"id":lease}))
        .await?;
    let heartbeat = client.clone();
    let heartbeat_id = lease.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            if heartbeat
                .internal("internal/lease", serde_json::json!({"id":heartbeat_id}))
                .await
                .is_err()
            {
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
        Box::new(move |cc| Ok(Box::new(nw_ui::WizApp::new(cc, port, session)))),
    );
    task.abort();
    let _ = client
        .internal(
            "internal/lease",
            serde_json::json!({"id":lease,"release":true}),
        )
        .await;
    result.map_err(|e| anyhow::anyhow!("Desktop failed: {e}"))
}

#[cfg(feature = "desktop")]
struct NativeUiClient {
    client: NativeClient,
    runtime: tokio::runtime::Handle,
}
#[cfg(feature = "desktop")]
impl nw_ui::CommandClient for NativeUiClient {
    fn choose_media(&self,reply:nw_ui::Reply){
        match rfd::FileDialog::new().set_title("Add media to Numera Wiz").pick_file(){
            Some(path)=>reply(nw_platform::files::path_text(&path).map(|path|nw_domain::commands::Response::success("intake",serde_json::json!({"path":path}))).map_err(|error|error.to_string())),
            None=>reply(Err("File selection cancelled".into())),
        }
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
