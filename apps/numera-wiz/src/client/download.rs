//! Native artifact copies use the same principal and private authenticated byte route.
use super::*;
use nw_domain::{
    artifacts::{ARTIFACT_CHUNK_BYTES, ArtifactManifest},
    download::{DownloadVerifier, SaveControl, SaveReceipt, SaveSession},
};
use nw_platform::download::ClientCopy;

impl NativeClient {
    pub async fn save_artifact(
        &self,
        selected: ArtifactManifest,
        output: PathBuf,
        control: Arc<SaveControl>,
    ) -> Result<SaveReceipt> {
        let _session = SaveSession(control.clone());
        let admitted = DownloadVerifier::new(&selected)?;
        // The selected manifest is already authorized. GET reauthorizes and fully
        // verifies its strong If-Match identity; a second manifest scan is redundant.
        let preflight = std::time::Instant::now();
        tracing::info!(artifact_id = %selected.artifact_id, "artifact.client_copy.preflight.started");
        let record = control.interruptible(self.attach()).await??;
        let token = control.interruptible(self.scoped_token(&record)).await??;
        let expected_etag = format!("\"{}\"", selected.sha256);
        let request = self
            .http
            .get(format!(
                "http://127.0.0.1:{}{}",
                record.port,
                selected
                    .bytes_route
                    .as_deref()
                    .context("Artifact byte route missing")?
            ))
            .bearer_auth(token)
            .header(reqwest::header::IF_MATCH, &expected_etag)
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        let mut response = control.interruptible(request.send()).await??;
        if response.status() != reqwest::StatusCode::OK {
            bounded_json(response).await?;
            return Err(nw_domain::DomainError::new(
                "TRANSPORT_ERROR",
                "Artifact bytes did not return a complete response",
            )
            .into());
        }
        if response.content_length() != Some(admitted.expected_bytes())
            || response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                != Some(expected_etag.as_str())
            || response
                .headers()
                .contains_key(reqwest::header::CONTENT_RANGE)
            || response
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
        {
            return Err(nw_domain::DomainError::new(
                "ARTIFACT_CHANGED",
                "Artifact byte headers do not match the selected identity",
            )
            .into());
        }
        tracing::info!(
            elapsed_ms = preflight.elapsed().as_millis(),
            "artifact.client_copy.bytes.accepted"
        );
        let mut copy =
            tokio::task::spawn_blocking(move || ClientCopy::create(&output, &selected)).await??;
        while let Some(frame) = control.interruptible(response.chunk()).await?? {
            for bytes in frame.chunks(ARTIFACT_CHUNK_BYTES) {
                control.check()?;
                let bytes = bytes.to_vec();
                // Drain each admitted filesystem write before accepting cancellation.
                copy = tokio::task::spawn_blocking(move || {
                    copy.write(&bytes)?;
                    Ok::<_, nw_domain::DomainError>(copy)
                })
                .await??;
                control.set_copied(copy.copied());
                control.check()?;
            }
        }
        let finishing = control.clone();
        tokio::task::spawn_blocking(move || copy.finish(&finishing))
            .await?
            .map_err(Into::into)
    }
}
