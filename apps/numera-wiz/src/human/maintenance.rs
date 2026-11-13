//! Explicit owner maintenance lowers to the same reviewed durable command port.
use super::{MutationIdentity, document, query};
use anyhow::Result;
use clap::Subcommand;
use nw_domain::{
    DomainError,
    commands::{self, Request},
    maintenance::{
        MigrationPlan,
        media_lifecycle::{GenerationRestorationPlan, MediaInitializationPlan, RestorationKind},
        promotion::GenerationPromotionPlan,
    },
};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Maintenance {
    /// Review an idle schema2 store; issue and retain a replay scope first.
    Plan,
    /// Submit the reviewed migration with a retained mutation identity.
    Migrate {
        #[arg(long)]
        plan_file: PathBuf,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    Inspect {
        id: String,
    },
    #[command(subcommand)]
    Promotion(Enablement),
    #[command(subcommand)]
    Initialization(Enablement),
}

#[derive(Subcommand)]
pub enum Enablement {
    /// Review eligibility and required space without changing the reader floor.
    Plan,
    /// Accept the reviewed plan; retain the scope and key for retries.
    Accept {
        #[arg(long)]
        plan_file: PathBuf,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    Inspect {
        id: String,
    },
    #[command(subcommand)]
    Restore(Restoration),
}

#[derive(Subcommand)]
pub enum Restoration {
    /// Review a stopped source restoration from an accepted enablement identity.
    Plan {
        id: String,
    },
    /// Accept restoration of the original source; never replace its database.
    Accept {
        #[arg(long)]
        plan_file: PathBuf,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    Inspect {
        id: String,
    },
}

fn accepted(id: &str, plan: Value, identity: MutationIdentity) -> Result<Request> {
    let mut request = query(id, json!({"plan":plan}));
    identity.apply(&mut request)?;
    Ok(request)
}

impl Maintenance {
    pub(super) fn prepare(self) -> Result<Request> {
        match self {
            Self::Plan => Ok(query("maintenance.plan", json!({}))),
            Self::Inspect { id } => Ok(query("maintenance.inspect", json!({"id":id}))),
            Self::Migrate {
                plan_file,
                identity,
            } => {
                let plan: MigrationPlan = commands::parse(document(&plan_file)?)?;
                plan.validate_hash()?;
                accepted("maintenance.migrate", serde_json::to_value(plan)?, identity)
            }
            Self::Promotion(command) => command.prepare(false),
            Self::Initialization(command) => command.prepare(true),
        }
    }
}

impl Enablement {
    fn prepare(self, initialization: bool) -> Result<Request> {
        let prefix = if initialization {
            "maintenance.initialization"
        } else {
            "maintenance.promotion"
        };
        match self {
            Self::Plan => Ok(query(&format!("{prefix}.plan"), json!({}))),
            Self::Inspect { id } => Ok(query(&format!("{prefix}.inspect"), json!({"id":id}))),
            Self::Accept {
                plan_file,
                identity,
            } => {
                let plan = document(&plan_file)?;
                if initialization {
                    commands::parse::<MediaInitializationPlan>(plan.clone())?.validate_hash()?;
                } else {
                    commands::parse::<GenerationPromotionPlan>(plan.clone())?.validate_hash()?;
                }
                accepted(&format!("{prefix}.accept"), plan, identity)
            }
            Self::Restore(command) => match command {
                Restoration::Plan { id } => {
                    Ok(query(&format!("{prefix}.restore.plan"), json!({"id":id})))
                }
                Restoration::Inspect { id } => Ok(query(
                    &format!("{prefix}.restore.inspect"),
                    json!({"id":id}),
                )),
                Restoration::Accept {
                    plan_file,
                    identity,
                } => {
                    let plan: GenerationRestorationPlan = commands::parse(document(&plan_file)?)?;
                    plan.validate_hash()?;
                    if !matches!(
                        (initialization, plan.kind),
                        (true, RestorationKind::InitializationRestorationV1)
                            | (false, RestorationKind::GenerationPromotionRestorationV1)
                    ) {
                        return Err(DomainError::new(
                            "INVALID_GENERATION_PLAN",
                            "Restoration kind differs from the chosen maintenance route",
                        )
                        .into());
                    }
                    accepted(
                        &format!("{prefix}.restore.accept"),
                        serde_json::to_value(plan)?,
                        identity,
                    )
                }
            },
        }
    }
}
