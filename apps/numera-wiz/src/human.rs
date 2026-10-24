//! Ordinary composable CLI commands lower to the existing canonical request port.
use crate::{client::NativeClient, print_response};
use anyhow::Result;
use clap::{Args, Subcommand};
use nw_domain::{
    DomainError,
    commands::{self, Request, Response},
    mux::{CleanIntent, Stream},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Args)]
pub struct MutationIdentity {
    /// Retain this principal-bound scope when retrying the operation.
    #[arg(long)]
    replay_scope: String,
    /// Reuse this exact key and parameters after a lost response.
    #[arg(long)]
    idempotency_key: String,
    #[arg(long)]
    request_id: Option<String>,
}
impl MutationIdentity {
    fn apply(self, request: &mut Request) -> Result<()> {
        request.request_id = match self.request_id {
            Some(id) => id,
            None => {
                let bytes = serde_json::to_vec(&(
                    &request.command.id,
                    &self.replay_scope,
                    &self.idempotency_key,
                ))?;
                format!("cli-{}", hex::encode(Sha256::digest(bytes)))
            }
        };
        request.replay_scope = Some(self.replay_scope);
        request.idempotency_key = Some(self.idempotency_key);
        Ok(())
    }
}
#[derive(Args)]
pub struct Page {
    #[arg(long, default_value_t = 128)]
    limit: u32,
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long)]
    replace_snapshot: Option<String>,
}
impl Page {
    pub(crate) fn params(self) -> Value {
        json!({"limit":self.limit,"cursor":self.cursor,"replace_snapshot":self.replace_snapshot})
    }
}
#[derive(Args)]
pub struct ProbePolicy {
    #[arg(long, conflicts_with = "disable_probe_timeout")]
    probe_timeout_seconds: Option<u32>,
    #[arg(long)]
    disable_probe_timeout: bool,
}
impl ProbePolicy {
    fn apply(self, params: &mut Value) -> Result<()> {
        let policy = match (self.probe_timeout_seconds, self.disable_probe_timeout) {
            (Some(seconds), false) => Some(nw_domain::execution::DeadlinePolicy::Finite { seconds }),
            (None, true) => Some(nw_domain::execution::DeadlinePolicy::Disabled),
            (None, false) => None,
            _ => return Err(DomainError::new("VALIDATION_ERROR", "Choose one explicit probe timeout policy").into()),
        };
        if let Some(policy) = policy {
            policy.duration()?;
            params["probe_timeout"] = serde_json::to_value(policy)?;
        }
        Ok(())
    }
}
#[derive(Subcommand)]
pub enum Media {
    /// Inspect an authorized media file without creating an asset or job.
    Probe {
        path: PathBuf,
        #[command(flatten)]
        probe: ProbePolicy,
    },
}
#[derive(Subcommand)]
pub enum Assets {
    Register {
        path: PathBuf,
        #[command(flatten)]
        probe: ProbePolicy,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    List {
        #[command(flatten)]
        page: Page,
    },
    Inspect {
        id: String,
    },
    /// Suggest a new output without creating it.
    Output {
        id: String,
    },
}
#[derive(Subcommand)]
pub enum Streams {
    /// Resolve exact original streams from bounded JSON documents.
    Resolve {
        #[arg(long)]
        streams_file: PathBuf,
        #[arg(long)]
        intent_file: PathBuf,
    },
    /// Prepare an immutable cleanup plan; submit it separately after review.
    Clean {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        intent_file: PathBuf,
        #[arg(long, value_parser = ["new", "preserve_best_effort", "require_reviewed"])]
        file_times: Option<String>,
        /// Explicit shared probe/operation policy document.
        #[arg(long)]
        limits_file: Option<PathBuf>,
    },
    /// Preview candidate and retained stream counts for 1-64 assets.
    Report {
        #[arg(long = "asset", required = true)]
        assets: Vec<String>,
        #[arg(long)]
        intent_file: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum Plans {
    Inspect { id: String },
}
#[derive(Subcommand)]
pub enum Jobs {
    Submit {
        #[arg(long)]
        plan: String,
        #[arg(long)]
        plan_hash: String,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    Inspect {
        id: String,
    },
    List {
        #[command(flatten)]
        page: Page,
    },
    Cancel {
        id: String,
        #[command(flatten)]
        identity: MutationIdentity,
    },
}
#[derive(Subcommand)]
pub enum Logs {
    Status,
}
#[derive(Subcommand)]
pub enum Discovery {
    List,
    Schema { id: String },
}
pub enum Command {
    Media(Media),
    Assets(Assets),
    Streams(Streams),
    Plans(Plans),
    Jobs(Jobs),
    Logs(Logs),
    Discovery(Option<Discovery>),
}
fn native_path(path: &Path) -> Result<String> {
    Ok(nw_platform::files::path_text(&std::path::absolute(path)?)?)
}
fn document(path: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((commands::MAX_COMMAND_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > commands::MAX_COMMAND_BYTES {
        return Err(DomainError::new("RESOURCE_LIMIT", "CLI document exceeds 1 MiB").into());
    }
    Ok(commands::decode_json(&bytes)?)
}
fn intent(path: &Path) -> Result<Value> {
    let value: CleanIntent = commands::parse(document(path)?)?;
    Ok(serde_json::to_value(value)?)
}
fn query(id: &str, params: Value) -> Request {
    let mut request = Request::query(id, params);
    request.request_id = format!("cli-{}", uuid::Uuid::new_v4());
    request
}
pub fn prepare(command: Command) -> Result<Request> {
    let request = match command {
        Command::Media(Media::Probe { path, probe }) => {
            let mut params = json!({"path":native_path(&path)?});
            probe.apply(&mut params)?;
            query("media.probe", params)
        }
        Command::Assets(Assets::Register { path, probe, identity }) => {
            let mut params = json!({"path":native_path(&path)?});
            probe.apply(&mut params)?;
            let mut request = query("assets.register", params);
            identity.apply(&mut request)?;
            request
        }
        Command::Assets(Assets::List { page }) => query("assets.list", page.params()),
        Command::Assets(Assets::Inspect { id }) => query("assets.inspect", json!({"id":id})),
        Command::Assets(Assets::Output { id }) => query("assets.output_suggest", json!({"id":id})),
        Command::Streams(Streams::Resolve {
            streams_file,
            intent_file,
        }) => {
            let streams: Vec<Stream> = commands::parse(document(&streams_file)?)?;
            query(
                "streams.clean.resolve",
                json!({"streams":streams,"intent":intent(&intent_file)?}),
            )
        }
        Command::Streams(Streams::Clean {
            asset,
            out,
            intent_file,
            file_times,
            limits_file,
        }) => {
            let mut params = json!({"asset_id":asset,"output":native_path(&out)?,"intent":intent(&intent_file)?});
            if let Some(path) = limits_file {
                let limits: nw_domain::execution::CleanupLimits =
                    commands::parse(document(&path)?)?;
                limits.validate()?;
                params["execution_limits"] = serde_json::to_value(limits)?;
            }
            if let Some(policy) = file_times {
                let policy: nw_domain::file_times::FileTimeRequest =
                    commands::parse(json!(policy))?;
                params["file_times"] = serde_json::to_value(policy)?;
            }
            query("streams.clean", params)
        }
        Command::Streams(Streams::Report {
            assets,
            intent_file,
        }) => query(
            "streams.clean.report",
            json!({"asset_ids":assets,"intent":intent(&intent_file)?}),
        ),
        Command::Plans(Plans::Inspect { id }) => query("plans.inspect", json!({"id":id})),
        Command::Jobs(Jobs::Submit {
            plan,
            plan_hash,
            identity,
        }) => {
            let mut request = query("jobs.submit", json!({"plan_id":plan,"plan_hash":plan_hash}));
            identity.apply(&mut request)?;
            request
        }
        Command::Jobs(Jobs::Inspect { id }) => query("jobs.inspect", json!({"id":id})),
        Command::Jobs(Jobs::List { page }) => query("jobs.list", page.params()),
        Command::Jobs(Jobs::Cancel { id, identity }) => {
            let mut request = query("jobs.cancel", json!({"id":id}));
            identity.apply(&mut request)?;
            request
        }
        Command::Logs(Logs::Status) => query("logs.status", json!({})),
        Command::Discovery(None | Some(Discovery::List)) => query("commands.list", json!({})),
        Command::Discovery(Some(Discovery::Schema { id })) => {
            query("commands.schema", json!({"id":id}))
        }
    };
    // Validate the same strict envelope and aggregate request budget before transport.
    Ok(commands::decode_request(&serde_json::to_vec(&request)?)?)
}
pub async fn run(client: &NativeClient, command: Command) -> Result<()> {
    match prepare(command) {
        Ok(request) => print_response(client, request).await,
        Err(error) => {
            let public = error
                .downcast_ref::<DomainError>()
                .cloned()
                .unwrap_or_else(|| {
                    DomainError::new(
                        "CLI_INPUT_ERROR",
                        "CLI input document or native path is unavailable",
                    )
                });
            println!(
                "{}",
                serde_json::to_string_pretty(&Response::error("human-cli", public))?
            );
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_cli_retry_keeps_identity_and_never_allocates_a_fresh_scope() {
        let make = || {
            Command::Jobs(Jobs::Submit {
                plan: "reviewed-plan".into(),
                plan_hash: "ab".repeat(32),
                identity: MutationIdentity {
                    replay_scope: "retained-scope".into(),
                    idempotency_key: "same-operation".into(),
                    request_id: None,
                },
            })
        };
        let first = prepare(make()).unwrap();
        let second = prepare(make()).unwrap();
        assert_eq!(first.request_id, second.request_id);
        assert_eq!(first.replay_scope.as_deref(), Some("retained-scope"));
        assert_eq!(first.idempotency_key.as_deref(), Some("same-operation"));
        assert_eq!(first.params, second.params);
    }
}
