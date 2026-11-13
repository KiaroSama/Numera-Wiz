//! Shared CLI/stdio credential delivery. Secret bytes never enter printed results.
use crate::client::NativeClient;
use anyhow::{Context as _, Result};
use nw_domain::{DomainError, commands::{Request, Response}};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) fn is_issue(id: &str) -> bool {
    matches!(id, "access.tokens.issue" | "access.credentials.issue")
}

pub(crate) async fn deliver(client: &NativeClient, request: &Request, out: &Path) -> Result<Response> {
    if client.profile.is_some() {
        return Ok(Response::error(&request.request_id, DomainError::new(
            "PERMISSION_DENIED", "Credential file delivery requires explicit native owner authority",
        )));
    }
    if !is_issue(&request.command.id) {
        return Ok(Response::error(&request.request_id, DomainError::new(
            "VALIDATION_ERROR", "Credential file delivery requires an issuance command",
        )));
    }
    if out.try_exists()? {
        return Ok(Response::error(&request.request_id, DomainError::new(
            "OUTPUT_COLLISION", "Token file already exists; no credential was issued",
        )));
    }
    // Establish a representable metadata path before any credential is issued.
    let file = nw_platform::files::path_text(out)?;
    let mut issued = client.request(request).await?;
    if !issued.ok { return Ok(issued); }
    let result = issued.result.take().context("Issued credential metadata is missing")?;
    let bearer = result["bearer"].as_str()
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .context("Issued credential omitted its bounded value")?;
    let token_id = hex::encode(Sha256::digest(bearer.as_bytes()));
    if let Err(error) = nw_platform::security::write_private(out, bearer.as_bytes(), false) {
        let revoked = super::durable(client, "access.tokens.revoke", json!({"token_id":token_id})).await;
        let revoked = revoked.as_ref().is_ok_and(|response| response.ok);
        tracing::error!(token_id, revoked, code=%error.code, "Credential file delivery failed; no bearer was printed");
        return Ok(Response::error(&request.request_id, DomainError::new(
            "CREDENTIAL_DELIVERY_FAILED", if revoked {
                "Token file publication failed; the issued token was revoked"
            } else {
                "Token file publication failed; issued-token revocation could not be confirmed"
            },
        )));
    }
    let mut metadata = Map::new();
    for name in ["principal", "profile", "expires_at", "expires_in_seconds"] {
        if let Some(value) = result.get(name) { metadata.insert(name.into(), value.clone()); }
    }
    metadata.insert("token_id".into(), json!(token_id));
    metadata.insert("file".into(), json!(file));
    issued.result = Some(Value::Object(metadata));
    Ok(issued)
}
