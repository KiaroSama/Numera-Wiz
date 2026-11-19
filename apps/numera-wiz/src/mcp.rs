use nw_domain::{
    commands::{self, Request},
    preview,
};
use nw_service::{ACTIVE_COMMANDS, Context, Service};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use std::{borrow::Cow, sync::Arc};
mod schema;
const TOOL_CURSOR_PREFIX: &str = "tools-v1-";
// Keep page boundaries independent of request IDs and reserve the JSON-RPC envelope.
const TOOL_PAGE_BYTES: usize = commands::MAX_RESULT_BYTES - 1024;
#[derive(Clone)]
pub struct Bridge {
    backend: Backend,
}
#[derive(Clone)]
enum Backend {
    Native(crate::client::NativeClient),
    Http(Arc<Service>),
}
impl Bridge {
    pub fn stdio(client: crate::client::NativeClient) -> Self {
        Self {
            backend: Backend::Native(client),
        }
    }
    pub fn http(service: Arc<Service>) -> Self {
        Self {
            backend: Backend::Http(service),
        }
    }
    async fn execute(
        &self,
        canonical: Request,
        context: &RequestContext<RoleServer>,
    ) -> Result<commands::Response, ErrorData> {
        if crate::access::credential_file::is_issue(&canonical.command.id) {
            return Ok(commands::Response::error(
                &canonical.request_id,
                nw_domain::DomainError::new(
                    "VALIDATION_ERROR",
                    "Credential issuance requires protected credential_file delivery",
                ),
            ));
        }
        match &self.backend {
            Backend::Native(client) => {
                tokio::select! {
                    result=client.request(&canonical)=>match result {
                        Ok(response)=>Ok(response),
                        Err(error)=>if let Some(error)=error.downcast_ref::<nw_domain::DomainError>() {
                            tracing::warn!(code=%error.code, "Native owner request failed before canonical dispatch");
                            Ok(commands::Response::error(&canonical.request_id,error.clone()).bounded())
                        } else {
                            Err(ErrorData::internal_error("Owner attachment/request failed; retain durable retry identity",None))
                        },
                    },
                    _=context.ct.cancelled()=>Ok(commands::Response::error(&canonical.request_id,nw_domain::DomainError::new("CANCELLED","MCP request cancelled; retain durable request identity to inspect a possibly accepted mutation"))),
                }
            }
            Backend::Http(service) => {
                let principal = context
                    .extensions
                    .get::<axum::http::request::Parts>()
                    .and_then(|parts| parts.extensions.get::<Context>())
                    .cloned()
                    .ok_or_else(|| {
                        ErrorData::invalid_request("Authenticated request context is missing", None)
                    })?;
                Ok(service
                    .dispatch_cancel(principal, canonical, context.ct.clone())
                    .await)
            }
        }
    }
}
fn tool(name: &str) -> Option<Tool> {
    if !ACTIVE_COMMANDS.contains(&name) {
        return None;
    }
    let descriptor = commands::descriptor(name)?;
    let mut params = descriptor.params_schema;
    let definitions = params.as_object_mut()?.remove("$defs");
    let mut schema = serde_json::json!({"type":"object","properties":{"params":params},"required":["params"],"additionalProperties":false});
    schema["properties"]["context"] = serde_json::json!({"type":"object","properties":{"request_id":{"type":"string","maxLength":128},"replay_scope":{"type":"string","maxLength":2048},"idempotency_key":{"type":"string","maxLength":128}},"additionalProperties":false});
    if descriptor.project_context != commands::ProjectContextRule::Forbidden {
        let write = descriptor.project_context == commands::ProjectContextRule::RequiredWrite;
        let mut project = serde_json::json!({"type":"object","properties":{"id":{"type":"string","format":"uuid"}},"required":["id"],"additionalProperties":false});
        if write {
            project["properties"]["expected_revision"] =
                serde_json::json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$"});
            project["required"] = serde_json::json!(["id", "expected_revision"]);
        }
        schema["properties"]["context"]["properties"]["project"] = project;
        schema["properties"]["context"]["required"] = serde_json::json!(["project"]);
        schema["required"] = serde_json::json!(["params", "context"]);
    }
    if let Some(definitions) = definitions {
        schema.as_object_mut()?.insert("$defs".into(), definitions);
    }
    if crate::access::credential_file::is_issue(name) {
        schema["properties"]["credential_file"] = serde_json::json!({
            "type":"string","minLength":1,"maxLength":32767,
            "description":"Absolute local protected output file; explicit native-owner stdio only; no overwrite"
        });
        schema["required"]
            .as_array_mut()?
            .push(serde_json::json!("credential_file"));
    }
    let mut tool = Tool::new(
        descriptor.id,
        descriptor.label,
        Arc::new(schema.as_object()?.clone()),
    );
    let result_schema = if name == preview::OBSERVE_COMMAND {
        Some(serde_json::to_value(schemars::schema_for!(preview::MediaObservationMetadata)).ok()?)
    } else {
        descriptor.result_schema
    };
    if let Some(result) = result_schema {
        let output = schema::response(result)?;
        tool = tool.with_raw_output_schema(Arc::new(output.as_object()?.clone()));
    }
    if let Some(requirements) = descriptor.authorization_requirements {
        tool = tool.with_meta(
            serde_json::from_value(serde_json::json!({
                "numera-wiz/authorization_requirements":requirements,
            }))
            .ok()?,
        );
    }
    Some(tool)
}
impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build()).with_server_info(Implementation::new("numera-wiz",env!("CARGO_PKG_VERSION"))).with_instructions("Typed commands use the shared owner service. The default agent profile is discovery-only. Durable mutations require an issued replay_scope and idempotency_key in context; retain them on retry.")
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2026_07_28,
        ])
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let offset = match request.and_then(|params| params.cursor) {
            None => 0,
            Some(cursor) => cursor
                .strip_prefix(TOOL_CURSOR_PREFIX)
                .filter(|offset| {
                    offset.len() <= 8 && offset.bytes().all(|byte| byte.is_ascii_digit())
                })
                .and_then(|offset| offset.parse::<usize>().ok())
                .filter(|offset| (1..ACTIVE_COMMANDS.len()).contains(offset))
                .ok_or_else(|| ErrorData::invalid_params("Invalid tool discovery cursor", None))?,
        };
        let mut result = ListToolsResult::with_all_items(Vec::new());
        // Measure an empty page with the longest possible continuation cursor.
        result.next_cursor = Some(format!("{TOOL_CURSOR_PREFIX}{}", ACTIVE_COMMANDS.len()));
        let serialization_error =
            || ErrorData::internal_error("Tool registry serialization failed", None);
        let mut page_bytes = serde_json::to_vec(&result)
            .map_err(|_| serialization_error())?
            .len();
        result.next_cursor = None;
        for (index, id) in ACTIVE_COMMANDS.iter().enumerate().skip(offset) {
            let entry = tool(id).ok_or_else(|| {
                ErrorData::internal_error("The active tool could not be constructed", None)
            })?;
            let entry_bytes = serde_json::to_vec(&entry)
                .map_err(|_| serialization_error())?
                .len();
            let bytes = page_bytes + entry_bytes + usize::from(!result.tools.is_empty());
            if bytes > TOOL_PAGE_BYTES {
                if result.tools.is_empty() {
                    return Err(ErrorData::internal_error(
                        "An active tool exceeds the discovery page byte budget",
                        None,
                    ));
                }
                result.next_cursor = Some(format!("{TOOL_CURSOR_PREFIX}{index}"));
                break;
            }
            result.tools.push(entry);
            page_bytes = bytes;
        }
        let envelope = serde_json::json!({"jsonrpc":"2.0","id":&context.id,"result":&result});
        let envelope_bytes = serde_json::to_vec(&envelope)
            .map_err(|_| ErrorData::internal_error("Tool response serialization failed", None))?
            .len();
        // Reserve the actual stdio newline in addition to the complete JSON-RPC envelope.
        if envelope_bytes >= commands::MAX_RESULT_BYTES {
            return Err(ErrorData::internal_error(
                "Tool discovery page exceeds the response byte budget",
                None,
            ));
        }
        tracing::debug!(
            envelope_bytes,
            tools = result.tools.len(),
            "MCP tool discovery page byte budget verified"
        );
        Ok(result)
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tool(name)
    }
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let cursor = request.and_then(|r| r.cursor);
        let response = self
            .execute(
                Request::query(
                    "artifacts.list",
                    serde_json::json!({"limit":64,"cursor":cursor}),
                ),
                &context,
            )
            .await?;
        if !response.ok {
            return Err(resource_error(response));
        }
        let page: nw_domain::views::SnapshotPage<nw_domain::artifacts::ArtifactSummary> =
            commands::parse(response.result.unwrap_or_default())
                .map_err(|_| ErrorData::internal_error("Artifact page decoding failed", None))?;
        let mut result = ListResourcesResult::with_all_items(
            page.items
                .into_iter()
                .map(|a| {
                    Resource::new(a.manifest_uri, a.artifact_id).with_mime_type("application/json")
                })
                .collect(),
        );
        result.next_cursor = page.next_cursor;
        Ok(result)
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Err(ErrorData::invalid_params(
                "Artifact reads do not accept input responses or request state",
                None,
            ));
        }
        let id = request
            .uri
            .strip_prefix("numera://artifact/")
            .and_then(|u| u.strip_suffix("/manifest"))
            .filter(|id| id.len() <= 132 && !id.contains('/'))
            .ok_or_else(|| {
                ErrorData::invalid_params("Expected a Numera artifact manifest URI", None)
            })?;
        let response = self
            .execute(
                Request::query("artifacts.manifest", serde_json::json!({"artifact_id":id})),
                &context,
            )
            .await?;
        if !response.ok {
            return Err(resource_error(response));
        }
        let text = serde_json::to_string(&response.result.unwrap_or_default())
            .map_err(|_| ErrorData::internal_error("Manifest serialization failed", None))?;
        if text.len() > 64 * 1024 {
            return Err(ErrorData::internal_error(
                "Manifest exceeded its 64 KiB budget",
                None,
            ));
        }
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, request.uri).with_mime_type("application/json"),
        ])
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)
        .into())
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let mut args = request.arguments.unwrap_or_default();
        let credential_file = args.remove("credential_file");
        let metadata = args
            .remove("context")
            .unwrap_or_else(|| serde_json::json!({}));
        #[derive(serde::Deserialize, Default)]
        #[serde(deny_unknown_fields)]
        struct Metadata {
            project: Option<commands::ProjectRef>,
            request_id: Option<String>,
            replay_scope: Option<String>,
            idempotency_key: Option<String>,
        }
        let metadata: Metadata = commands::parse(metadata)
            .map_err(|_| ErrorData::invalid_params("Invalid canonical command context", None))?;
        if args.len() != 1 || !args.contains_key("params") {
            return Err(ErrorData::invalid_params(
                "Expected typed params and optional canonical context",
                None,
            ));
        }
        let mut canonical = Request::query(request.name.as_ref(), args["params"].clone());
        if let Some(id) = metadata.request_id {
            canonical.request_id = id;
        }
        canonical.project = metadata.project;
        canonical.replay_scope = metadata.replay_scope;
        canonical.idempotency_key = metadata.idempotency_key;
        let observation = canonical.command.id == preview::OBSERVE_COMMAND;
        let issuance = crate::access::credential_file::is_issue(&canonical.command.id);
        if credential_file.is_some() && !issuance {
            return Err(ErrorData::invalid_params(
                "credential_file is only valid for credential issuance",
                None,
            ));
        }
        let response = if issuance {
            match &self.backend {
                Backend::Native(client)
                    if client.profile.is_none() && context.ct.is_cancelled() =>
                {
                    commands::Response::error(
                        &canonical.request_id,
                        nw_domain::DomainError::new(
                            "CANCELLED",
                            "Credential delivery cancelled before issuance",
                        ),
                    )
                }
                Backend::Native(client) if client.profile.is_none() => {
                    let path = credential_file
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        .filter(|path| {
                            !path.is_empty() && path.chars().take(32768).count() <= 32767
                        })
                        .map(std::path::Path::new)
                        .filter(|path| path.is_absolute());
                    if let Some(path) = path {
                        crate::access::credential_file::deliver(client, &canonical, path, &context.ct).await
                            .map_err(|_| ErrorData::internal_error("Protected credential file delivery failed; no credential value returned", None))?
                    } else {
                        commands::Response::error(
                            &canonical.request_id,
                            nw_domain::DomainError::new(
                                "VALIDATION_ERROR",
                                "Choose an explicit absolute credential_file; raw credentials are never returned",
                            ),
                        )
                    }
                }
                _ => commands::Response::error(
                    &canonical.request_id,
                    nw_domain::DomainError::new(
                        "PERMISSION_DENIED",
                        "Credential file delivery requires explicit native-owner stdio",
                    ),
                ),
            }
        } else {
            self.execute(canonical, &context).await?
        };
        if observation && response.ok {
            return Ok(observation_result(response, &context)?.into());
        }
        let value = serde_json::to_value(&response)
            .map_err(|_| ErrorData::internal_error("Response serialization failed", None))?;
        let mut result = if response.ok {
            CallToolResult::structured(value)
        } else {
            CallToolResult::structured_error(value)
        };
        result.content = vec![ContentBlock::text(if response.ok {
            "Command completed. Typed result is in structuredContent."
        } else {
            "Command failed. Typed error is in structuredContent."
        })];
        Ok(result.into())
    }
}

fn observation_result(
    mut response: commands::Response,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    let invalid = || ErrorData::internal_error("Invalid inline observation result", None);
    let media: preview::ObservationResult =
        commands::parse(response.result.take().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let (metadata, data, bytes, advertised_mime, mime, ceiling) = match media {
        preview::ObservationResult::Still(still) => (
            serde_json::to_value(&still.metadata),
            still.png_base64,
            still.metadata.encoded_byte_count,
            still.metadata.mime_type,
            preview::PNG_MIME_TYPE,
            preview::MAX_INLINE_PNG_BYTES,
        ),
        preview::ObservationResult::Audio(audio) => (
            serde_json::to_value(&audio.metadata),
            audio.audio_base64,
            audio.metadata.encoded_byte_count,
            audio.metadata.mime_type,
            preview::WAVE_MIME_TYPE,
            preview::MAX_INLINE_WAVE_BYTES,
        ),
    };
    if !(1..=ceiling).contains(&bytes)
        || data.len() != bytes.div_ceil(3) * 4
        || advertised_mime != mime
    {
        return Err(invalid());
    }
    response.result = Some(metadata.map_err(|_| invalid())?);
    let structured = serde_json::to_value(&response).map_err(|_| invalid())?;
    if serde_json::to_vec(&structured)
        .map_err(|_| invalid())?
        .len()
        > preview::MAX_OBSERVATION_METADATA_BYTES
    {
        return Err(invalid());
    }
    let mut result = CallToolResult::structured(structured);
    result.content = vec![if mime == preview::WAVE_MIME_TYPE {
        ContentBlock::audio(data, mime)
    } else {
        ContentBlock::image(data, mime)
    }];
    let envelope = serde_json::json!({"jsonrpc":"2.0","id":&context.id,"result":&result});
    if serde_json::to_vec(&envelope).map_err(|_| invalid())?.len()
        >= preview::MAX_MCP_OBSERVATION_BYTES
    {
        return Err(invalid());
    }
    Ok(result)
}

fn resource_error(response: commands::Response) -> ErrorData {
    let diagnostic = serde_json::to_value(response.error).ok();
    ErrorData::invalid_params(
        "Artifact resource is unavailable under current authority",
        diagnostic,
    )
}
