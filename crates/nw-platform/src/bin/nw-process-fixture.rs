//! Hosted process fault fixture, never part of the ordinary package.
use std::io::Write;
fn main()->std::process::ExitCode{
    nw_platform::init_logging();
    tracing::info!("Hosted process fixture starting");
    let args=std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str){
        Some("flood")=>{
            tracing::debug!("Producing deterministic bounded fixture output");
            let block=[b'x';8192];
            for _ in 0..256 {if std::io::stdout().write_all(&block).is_err(){break;}let _=std::io::stderr().write_all(&block);}
            std::process::ExitCode::SUCCESS
        },
        Some("sleep")=>{tracing::warn!("WARNING: Waiting for supervisor cancellation");std::thread::sleep(std::time::Duration::from_secs(20));std::process::ExitCode::SUCCESS},
        Some("failure")=>{tracing::error!("Deterministic process failure");std::process::ExitCode::from(9)},
        Some("no-output")=>std::process::ExitCode::SUCCESS,
        _=>{tracing::error!("Unknown fixture mode");std::process::ExitCode::FAILURE},
    }
}
