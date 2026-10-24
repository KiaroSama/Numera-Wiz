//! Human access setup lowers to the canonical owner commands; secrets go only to a protected file.
use crate::{client::NativeClient, print_response};
use anyhow::{Context as _, Result};
use clap::Subcommand;
use nw_domain::{
    DomainError,
    commands::{self, Request, Response},
};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};

#[derive(Subcommand)]
pub enum AccessCommand {
    ReplayScope {
        #[command(subcommand)]
        command: ReplayScope,
    },
    Profiles {
        #[command(subcommand)]
        command: Profiles,
    },
    Tokens {
        #[command(subcommand)]
        command: Tokens,
    },
}
#[derive(Subcommand)]
pub enum ReplayScope {
    Create,
}
#[derive(Subcommand)]
pub enum Profiles {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long = "read-root")]
        read_roots: Vec<PathBuf>,
        #[arg(long = "write-root")]
        write_roots: Vec<PathBuf>,
    },
    Inspect {
        #[arg(long)]
        name: String,
        #[arg(long)]
        grants_out: Option<PathBuf>,
    },
    List,
    Update {
        #[arg(long)]
        name: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        grants_file: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum Tokens {
    Issue {
        #[arg(long)]
        for_access_profile: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1578)]
        port: u16,
    },
    Revoke {
        #[arg(long)]
        token_id: String,
    },
}
fn checked(response: Response) -> Result<Response> {
    if response.ok {
        Ok(response)
    } else {
        Err(response
            .error
            .unwrap_or_else(|| {
                DomainError::new("INTERNAL_ERROR", "Command failed without a typed error")
            })
            .into())
    }
}
fn print_checked(response: Response) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    checked(response).map(|_| ())
}
async fn durable(client: &NativeClient, id: &str, params: Value) -> Result<Response> {
    let scope = client
        .request(&Request::query("access.replay_scope.create", json!({})))
        .await?;
    if !scope.ok {
        return Ok(scope);
    }
    let mut request = Request::query(id, params);
    request.replay_scope = Some(
        scope
            .result
            .as_ref()
            .and_then(|result| result["replay_scope"].as_str())
            .context("Scope response omitted its token")?
            .to_owned(),
    );
    request.idempotency_key = Some(uuid::Uuid::new_v4().to_string());
    request.request_id = uuid::Uuid::new_v4().to_string();
    client.request(&request).await
}
pub async fn run(client: &NativeClient, command: AccessCommand) -> Result<()> {
    tracing::info!("Access setup command started; credentials are excluded from diagnostics");
    match command {
        AccessCommand::ReplayScope { command: ReplayScope::Create } => {
            print_response(client, Request::query("access.replay_scope.create", json!({}))).await
        }
        AccessCommand::Profiles { command } => match command {
            Profiles::Create {
                name,
                read_roots,
                write_roots,
            } => {
                let roots = |paths: Vec<PathBuf>| -> Result<Vec<String>> {
                    paths
                        .iter()
                        .map(|path| Ok(nw_platform::files::path_text(path)?))
                        .collect()
                };
                print_checked(durable(client, "access.profiles.create", json!({"name":name,"read_roots":roots(read_roots)?,"write_roots":roots(write_roots)?})).await?)
            }
            Profiles::Inspect { name, grants_out } => {
                let response = client
                    .request(&Request::query(
                        "access.profiles.inspect",
                        json!({"name":name}),
                    ))
                    .await?;
                if response.ok
                    && let Some(path) = grants_out
                {
                    let profile: nw_domain::access::AccessProfile = commands::parse(
                        response
                            .result
                            .clone()
                            .context("Profile inspection omitted its result")?,
                    )?;
                    let grants = nw_domain::access::GrantsDocument::from(&profile);
                    nw_platform::security::write_private(
                        &path,
                        &serde_json::to_vec_pretty(&grants)?,
                        false,
                    )?;
                    tracing::info!(
                        "Replacement grant document saved without overwriting an existing file"
                    );
                }
                print_checked(response)
            }
            Profiles::List => {
                print_response(client, Request::query("access.profiles.list", json!({}))).await
            }
            Profiles::Update {
                name,
                expected_revision,
                grants_file,
            } => {
                let mut bytes = Vec::new();
                std::fs::File::open(grants_file)?
                    .take((commands::MAX_COMMAND_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > commands::MAX_COMMAND_BYTES {
                    return Err(
                        DomainError::new("RESOURCE_LIMIT", "Grant document exceeds 1 MiB").into(),
                    );
                }
                let grants: nw_domain::access::GrantsDocument =
                    commands::parse(commands::decode_json(&bytes)?)?;
                let input = grants.replace(name, expected_revision);
                input.validate()?;
                print_checked(
                    durable(
                        client,
                        "access.profiles.update",
                        serde_json::to_value(input)?,
                    )
                    .await?,
                )
            }
        },
        AccessCommand::Tokens { command } => match command {
            Tokens::Revoke { token_id } => print_checked(
                durable(client, "access.tokens.revoke", json!({"token_id":token_id})).await?,
            ),
            Tokens::Issue {
                for_access_profile,
                out,
                port,
            } => {
                if out.try_exists()? {
                    return print_checked(Response::error(
                        "token-file",
                        DomainError::new(
                            "OUTPUT_COLLISION",
                            "Token file already exists; no credential was issued",
                        ),
                    ));
                }
                let issued = client
                    .request(&Request::query(
                        "access.tokens.issue",
                        json!({"for_access_profile":for_access_profile,"port":port}),
                    ))
                    .await?;
                if !issued.ok {
                    return print_checked(issued);
                }
                let result = issued
                    .result
                    .context("Issued credential metadata is missing")?;
                let bearer = result["bearer"]
                    .as_str()
                    .context("Issued credential omitted its value")?;
                let token_id = result["token_id"]
                    .as_str()
                    .context("Issued credential omitted its identifier")?;
                if let Err(error) =
                    nw_platform::security::write_private(&out, bearer.as_bytes(), false)
                {
                    let revoked =
                        durable(client, "access.tokens.revoke", json!({"token_id":token_id})).await;
                    let revoked = revoked.as_ref().is_ok_and(|response| response.ok);
                    tracing::error!(token_id, revoked, code=%error.code, "Credential file delivery failed; no bearer was printed");
                    return print_checked(Response::error(
                        "token-file",
                        DomainError::new(
                            "CREDENTIAL_DELIVERY_FAILED",
                            if revoked {
                                "Token file publication failed; the issued token was revoked"
                            } else {
                                "Token file publication failed; issued-token revocation could not be confirmed"
                            },
                        ),
                    ));
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"token_id":token_id,"principal":result["principal"],"expires_at":result["expires_at"],"file":nw_platform::files::path_text(&out)?})
                    )?
                );
                Ok(())
            }
        },
    }
}
