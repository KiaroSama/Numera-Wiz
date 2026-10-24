use nw_domain::commands::{self, Request};
use nw_service::{ACTIVE_COMMANDS, Context, Service};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use std::{borrow::Cow, sync::Arc};
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
        match &self.backend {
            Backend::Native(client) => {
                tokio::select! {
                    result=client.request(&canonical)=>result.map_err(|_|ErrorData::internal_error("Owner attachment/request failed; retain durable retry identity",None)),
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
    if let Some(definitions) = definitions {
        schema.as_object_mut()?.insert("$defs".into(), definitions);
    }
    Some(Tool::new(
        descriptor.id,
        descriptor.label,
        Arc::new(schema.as_object()?.clone()),
    ))
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
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            ACTIVE_COMMANDS.iter().filter_map(|id| tool(id)).collect(),
        ))
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
        let metadata = args
            .remove("context")
            .unwrap_or_else(|| serde_json::json!({}));
        #[derive(serde::Deserialize, Default)]
        #[serde(deny_unknown_fields)]
        struct Metadata {
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
        canonical.replay_scope = metadata.replay_scope;
        canonical.idempotency_key = metadata.idempotency_key;
        let response = self.execute(canonical, &context).await?;
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

fn resource_error(response: commands::Response) -> ErrorData {
    let diagnostic = serde_json::to_value(response.error).ok();
    ErrorData::invalid_params(
        "Artifact resource is unavailable under current authority",
        diagnostic,
    )
}
