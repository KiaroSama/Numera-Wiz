//! Explicit headless artifact transfer; never opens a GUI or writes through server path grants.
use crate::client::NativeClient;
use anyhow::{Context as _, Result};
use clap::Subcommand;
use nw_domain::{
    DomainError,
    artifacts::ArtifactManifest,
    commands::{Request, Response},
    download::SaveControl,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Subcommand)]
pub enum Command {
    List {
        #[command(flatten)]
        page: crate::human::Page,
    },
    Manifest {
        #[arg(long)]
        artifact_id: String,
    },
    ReadChunk {
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long, default_value = "0")]
        offset: String,
        #[arg(long, default_value_t = 131072)]
        length: u32,
    },
    /// Stream a selected immutable artifact to an absent client destination.
    Save {
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        expected_sha256: String,
        #[arg(long)]
        out: PathBuf,
    },
}
pub async fn run(client: &NativeClient, command: Command) -> Result<()> {
    let request = match command {
        Command::Save { artifact_id, expected_sha256, out } => {
            return save(client, artifact_id, expected_sha256, out).await;
        }
        Command::List { page } => Request::query("artifacts.list", page.params()),
        Command::Manifest { artifact_id } => {
            Request::query("artifacts.manifest", serde_json::json!({"artifact_id":artifact_id}))
        }
        Command::ReadChunk { artifact_id, expected_sha256, offset, length } => Request::query(
            "artifacts.read_chunk",
            serde_json::json!({"artifact_id":artifact_id,"expected_sha256":expected_sha256,"offset":offset,"length":length}),
        ),
    };
    crate::print_response(client, request).await
}
async fn save(
    client: &NativeClient,
    artifact_id: String,
    expected_sha256: String,
    out: PathBuf,
) -> Result<()> {
    let control = Arc::new(SaveControl::default());
    let saving = async {
        let response = control
            .interruptible(client.request(&Request::query(
                "artifacts.manifest",
                serde_json::json!({"artifact_id":artifact_id}),
            )))
            .await??;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| {
                    DomainError::new("ARTIFACT_UNAVAILABLE", "Artifact manifest is unavailable")
                })
                .into());
        }
        let manifest: ArtifactManifest =
            serde_json::from_value(response.result.context("Artifact manifest missing")?)?;
        if manifest.sha256 != expected_sha256 {
            return Err(DomainError::new(
                "ARTIFACT_CHANGED",
                "Expected SHA-256 differs from the selected artifact",
            )
            .into());
        }
        client.save_artifact(manifest, out, control.clone()).await
    };
    tokio::pin!(saving);
    let result: Result<nw_domain::download::SaveReceipt> = tokio::select! {
        result = &mut saving => result,
        _ = tokio::signal::ctrl_c() => {
            control.cancel();
            // Observe manifest/network cancellation and drain admitted filesystem work.
            saving.await
        }
    };
    match result {
        Ok(receipt) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Response::success(
                    "client-artifact-save",
                    serde_json::to_value(receipt)?,
                ))?
            );
            Ok(())
        }
        Err(error) => {
            let public = error
                .downcast_ref::<DomainError>()
                .cloned()
                .unwrap_or_else(|| {
                    DomainError::new(
                        "CLIENT_COPY_FAILED",
                        "Client artifact transfer did not finish",
                    )
                });
            println!(
                "{}",
                serde_json::to_string_pretty(&Response::error("client-artifact-save", public))?
            );
            Err(error)
        }
    }
}
