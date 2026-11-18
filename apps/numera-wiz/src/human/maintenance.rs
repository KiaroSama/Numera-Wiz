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
        media_v2::{EnablementKind, EnablementPlanV2},
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
    #[command(subcommand)]
    MediaV2(MixedEnablement),
}

#[derive(Subcommand)]
pub enum MixedEnablement {
    /// Review a MediaV1 generation that has not been adopted.
    InitializationPlan,
    /// Review a MediaV1 generation with committed user work.
    PromotionPlan,
    /// Accept the reviewed generation and retain the mutation identity.
    Accept {
        #[arg(long)]
        plan_file: PathBuf,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    #[command(subcommand)]
    Restore(Restoration),
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
            Self::MediaV2(command) => match command {
                MixedEnablement::InitializationPlan => Ok(query(
                    "maintenance.media_v2.plan",
                    json!({"kind":EnablementKind::MediaInitializationV2}),
                )),
                MixedEnablement::PromotionPlan => Ok(query(
                    "maintenance.media_v2.plan",
                    json!({"kind":EnablementKind::GenerationPromotionV2}),
                )),
                MixedEnablement::Accept {
                    plan_file,
                    identity,
                } => {
                    let plan: EnablementPlanV2 = commands::parse(document(&plan_file)?)?;
                    plan.validate_hash()?;
                    accepted(
                        "maintenance.media_v2.accept",
                        serde_json::to_value(plan)?,
                        identity,
                    )
                }
                MixedEnablement::Restore(command) => command.prepare("maintenance.media_v2", None),
            },
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
            Self::Restore(command) => command.prepare(
                prefix,
                Some(if initialization {
                    RestorationKind::InitializationRestorationV1
                } else {
                    RestorationKind::GenerationPromotionRestorationV1
                }),
            ),
        }
    }
}

impl Restoration {
    fn prepare(self, prefix: &str, kind: Option<RestorationKind>) -> Result<Request> {
        match self {
            Self::Plan { id } => Ok(query(&format!("{prefix}.restore.plan"), json!({"id":id}))),
            Self::Inspect { id } => Ok(query(
                &format!("{prefix}.restore.inspect"),
                json!({"id":id}),
            )),
            Self::Accept {
                plan_file,
                identity,
            } => {
                let plan: GenerationRestorationPlan = commands::parse(document(&plan_file)?)?;
                plan.validate_hash()?;
                if kind.map_or(plan.schema_version != 2, |kind| kind != plan.kind) {
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
        }
    }
}
