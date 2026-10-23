use nw_domain::commands::{self, Request};
use nw_service::{ACTIVE_COMMANDS, Service, Context};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use std::{borrow::Cow, sync::Arc};
#[derive(Clone)]
pub struct Bridge { backend: Backend }
#[derive(Clone)]
enum Backend { Native(crate::client::NativeClient), Http(Arc<Service>) }
impl Bridge {
    pub fn stdio(client: crate::client::NativeClient) -> Self { Self { backend: Backend::Native(client) } }
    pub fn http(service: Arc<Service>) -> Self { Self { backend: Backend::Http(service) } }
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
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(Implementation::new("numera-wiz",env!("CARGO_PKG_VERSION"))).with_instructions("Typed commands use the shared owner service. The default agent profile is discovery-only. Durable mutations require an issued replay_scope and idempotency_key in context; retain them on retry.")
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
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let mut args = request.arguments.unwrap_or_default();
        let metadata = args.remove("context").unwrap_or_else(|| serde_json::json!({}));
        #[derive(serde::Deserialize, Default)]
        #[serde(deny_unknown_fields)]
        struct Metadata { request_id: Option<String>, replay_scope: Option<String>, idempotency_key: Option<String> }
        let metadata: Metadata = commands::parse(metadata).map_err(|_| ErrorData::invalid_params("Invalid canonical command context", None))?;
        if args.len() != 1 || !args.contains_key("params") { return Err(ErrorData::invalid_params("Expected typed params and optional canonical context", None)); }
        let mut canonical = Request::query(request.name.as_ref(), args["params"].clone());
        if let Some(id) = metadata.request_id { canonical.request_id = id; }
        canonical.replay_scope = metadata.replay_scope;
        canonical.idempotency_key = metadata.idempotency_key;
        let response = match &self.backend {
            Backend::Native(client) => client.request(&canonical).await.map_err(|_| ErrorData::internal_error("Owner attachment/request failed; retain durable retry identity", None))?,
            Backend::Http(service) => {
                let principal = context.extensions.get::<axum::http::request::Parts>().and_then(|parts| parts.extensions.get::<Context>()).cloned().ok_or_else(|| ErrorData::invalid_request("Authenticated request context is missing", None))?;
                service.dispatch(principal, canonical).await
            }
        };
        let value = serde_json::to_value(&response)
            .map_err(|_| ErrorData::internal_error("Response serialization failed", None))?;
        Ok(if response.ok {
            CallToolResult::structured(value)
        } else {
            CallToolResult::structured_error(value)
        }
        .into())
    }
}
