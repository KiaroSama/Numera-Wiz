//! Independent, offline JSON Schema conformance child. Diagnostics contain fixed codes only.
mod protocol;
mod validation;
use protocol::{Batch, Failure, INPUT_LIMIT, Summary};
use std::io::{Read, Write};

fn main() {
    std::panic::set_hook(Box::new(|_| eprintln!("SCHEMA_VALIDATOR_PANIC")));
    let mut summary = Summary::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut input = Vec::new();
        std::io::stdin()
            .take((INPUT_LIMIT + 1) as u64)
            .read_to_end(&mut input)
            .map_err(|_| ())?;
        if input.len() > INPUT_LIMIT || std::env::args_os().len() != 1 {
            return Err(());
        }
        protocol::preflight(&input)?;
        let batch: Batch = serde_json::from_slice(&input).map_err(|_| ())?;
        validation::run(batch, &mut summary)
    }));
    if !matches!(result, Ok(Ok(()))) {
        summary.failure(
            Failure {
                code: "PROTOCOL",
                schema_id: None,
                case_id: None,
                control_id: None,
            },
            true,
        );
    }
    let exit = if summary.infrastructure {
        2
    } else if summary.failed != 0 {
        1
    } else {
        0
    };
    let output = serde_json::to_vec(&summary).unwrap_or_else(|_| b"{}".to_vec());
    if output.len() > 16_384 || std::io::stdout().write_all(&output).is_err() {
        std::process::exit(2);
    }
    std::process::exit(exit);
}
