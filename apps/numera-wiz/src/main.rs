//! Thin development CLI/MCP/GUI composition; domain behavior stays in the service.
mod mcp;
mod server;
use anyhow::{Context,Result,bail};
use clap::{Parser,Subcommand};
use nw_domain::commands::Request;
use nw_service::{CommandService,Principal};
use std::{io::Read,path::PathBuf};

#[derive(Parser)]
#[command(name="numera-wiz",version,about="Numera Wiz development command interface")]
struct Args {#[command(subcommand)] command:Option<Mode>}
#[derive(Subcommand)]
enum Mode {
    /// Discover active development commands and their qualification limits.
    Commands,
    /// Submit a bounded canonical JSON query from a file or stdin.
    Request {#[arg(long)] file:Option<PathBuf>},
    /// Official MCP adapter; default agent access remains discovery-only.
    Bridge {#[arg(long,default_value="stdio")] transport:String,#[arg(long,default_value_t=1578)] port:u16,#[arg(long)] token_file:Option<PathBuf>},
    /// Start a loopback development application API and direct HTTP MCP.
    Serve {#[arg(long,default_value_t=1578)] port:u16,#[arg(long)] token_file:PathBuf},
    /// Report current scope without initializing a media GPU or display.
    Doctor,
}
#[tokio::main]
async fn main()->std::process::ExitCode{
    nw_platform::init_logging();
    match run().await{Ok(())=>std::process::ExitCode::SUCCESS,Err(error)=>{tracing::error!(%error,"Operation failed");std::process::ExitCode::FAILURE}}
}
async fn run()->Result<()> {
    let args=Args::parse();tracing::info!("Numera Wiz development entry point started");
    match args.command {
        Some(Mode::Commands)=>print_response(Request::query("commands.list",serde_json::json!({}))),
        Some(Mode::Request{file})=>{
            let input:Box<dyn Read>=match file{Some(path)=>Box::new(std::fs::File::open(path)?),None=>Box::new(std::io::stdin())};
            let mut bytes=Vec::new();input.take(256*1024+1).read_to_end(&mut bytes)?;
            if bytes.len()>256*1024{bail!("Request exceeds 256 KiB");}
            print_response(nw_domain::commands::decode_request(&bytes)?)
        },
        Some(Mode::Doctor)=>{
            tracing::warn!("WARNING: Complete runtime, portable payload and GPU qualification remain pending");
            println!("{}",serde_json::to_string_pretty(&serde_json::json!({"product":"Numera Wiz","version":env!("CARGO_PKG_VERSION"),"scope":"development command/resolver; media owner integration pending","media_gpu_initialized":false,"portable_release":"not_qualified","default_mcp_profile":"discovery_only","default_http_port":1578}))?);Ok(())
        },
        Some(Mode::Bridge{transport,port,token_file})=>match transport.as_str(){
            "stdio"=>{use rmcp::ServiceExt;let running=mcp::Bridge::stdio().serve(rmcp::transport::stdio()).await?;running.waiting().await?;Ok(())},
            "http"=>server::serve(port,token_file.context("HTTP bridge requires --token-file; credentials are never printed")?).await,
            _=>bail!("Unsupported transport; choose stdio or http"),
        },
        Some(Mode::Serve{port,token_file})=>server::serve(port,token_file).await,
        None=>desktop().await,
    }
}
fn print_response(request:Request)->Result<()> {
    let result=CommandService.dispatch(Principal::LocalUser,request);
    println!("{}",serde_json::to_string_pretty(&result)?);
    if !result.ok{bail!("Command returned a domain error; inspect the JSON result");}Ok(())
}
#[cfg(not(feature="desktop"))]
async fn desktop()->Result<()>{bail!("This CI artifact is headless. Use commands/request/bridge/doctor, or select the separately built desktop artifact.")}
#[cfg(feature="desktop")]
async fn desktop()->Result<()>{
    let (endpoint,bearer,task)=server::native_session().await?;
    let result=eframe::run_native("Numera Wiz",eframe::NativeOptions{renderer:eframe::Renderer::Glow,viewport:eframe::egui::ViewportBuilder::default().with_inner_size([1280.0,800.0]),..Default::default()},Box::new(move|cc|Ok(Box::new(nw_ui::WizApp::new(cc,endpoint,bearer)))));
    task.abort();result.map_err(|e|anyhow::anyhow!("Desktop failed: {e}"))
}
