//! Hosted process fault fixture, never part of the ordinary package.
use std::io::Write;
fn main() -> std::process::ExitCode {
    nw_platform::init_logging();
    tracing::info!("Hosted process fixture starting");
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("flood") => {
            tracing::debug!("Producing deterministic bounded fixture output");
            let block = [b'x'; 8192];
            for _ in 0..256 {
                if std::io::stdout().write_all(&block).is_err() {
                    break;
                }
                let _ = std::io::stderr().write_all(&block);
            }
            std::process::ExitCode::SUCCESS
        }
        Some("sleep") => {
            tracing::warn!("WARNING: Waiting for supervisor cancellation");
            std::thread::sleep(std::time::Duration::from_secs(20));
            std::process::ExitCode::SUCCESS
        }
        Some("stream-sleep") => {
            std::io::stdout().write_all(&[b'x'; 4096]).unwrap();
            std::io::stdout().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(20));
            std::process::ExitCode::SUCCESS
        }
        Some("hold-destination" | "try-destination") => {
            let Some(path) = args.get(2).map(std::path::PathBuf::from) else {
                tracing::error!("Destination fixture requires an absolute output");
                return std::process::ExitCode::FAILURE;
            };
            let result = (|| {
                let parent = nw_platform::files::DirectoryStamp::capture_resolved(path.parent().unwrap())?;
                let destination = nw_platform::claims::Destination::reviewed(&path, &parent.object_identity()?)?;
                nw_platform::claims::DestinationClaims::try_acquire(vec![destination])
            })();
            match result {
                Ok(_claims) => {
                    if args[1] == "hold-destination" {
                        println!("CLAIM_READY");
                        std::io::stdout().flush().unwrap();
                        // The supervisor can terminate this bounded fixture to test crash unlock.
                        std::thread::sleep(std::time::Duration::from_secs(50));
                    }
                    std::process::ExitCode::SUCCESS
                }
                Err(error) => {
                    println!("{}", error.code);
                    tracing::warn!(code = %error.code, "Destination fixture could not acquire publication");
                    std::process::ExitCode::from(4)
                }
            }
        }
        Some("failure") => {
            tracing::error!("Deterministic process failure");
            std::process::ExitCode::from(9)
        }
        Some("no-output") => std::process::ExitCode::SUCCESS,
        _ => {
            tracing::error!("Unknown fixture mode");
            std::process::ExitCode::FAILURE
        }
    }
}
