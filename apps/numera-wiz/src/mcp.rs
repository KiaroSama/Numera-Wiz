use nw_domain::commands::{self,Request};
use nw_service::{CommandService,Principal,ACTIVE_COMMANDS};
use rmcp::{ErrorData,RoleServer,ServerHandler,model::*,service::RequestContext};
use std::{borrow::Cow,sync::Arc};
#[derive(Clone)]
pub struct Bridge{stdio:bool}
impl Bridge{pub fn stdio()->Self{Self{stdio:true}}pub fn http()->Self{Self{stdio:false}}}
fn tool(name:&str)->Option<Tool>{
    if !ACTIVE_COMMANDS.contains(&name){return None;}
    let descriptor=commands::descriptor(name)?;
    let mut params=descriptor.params_schema;
    let definitions=params.as_object_mut()?.remove("$defs");
    let mut schema=serde_json::json!({"type":"object","properties":{"params":params},"required":["params"],"additionalProperties":false});
    if let Some(definitions)=definitions {schema.as_object_mut()?.insert("$defs".into(),definitions);}
    Some(Tool::new(descriptor.id,descriptor.label,Arc::new(schema.as_object()?.clone())))
}
impl ServerHandler for Bridge {
    fn get_info(&self)->ServerConfig{ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(Implementation::new("numera-wiz",env!("CARGO_PKG_VERSION"))).with_instructions("Development-only typed queries. The default agent profile is discovery-only. Complete media/runtime and operating access profiles are not qualified.")}
    fn supported_protocol_versions(&self)->Cow<'static,[ProtocolVersion]>{Cow::Owned(vec![ProtocolVersion::V_2025_11_25,ProtocolVersion::V_2026_07_28])}
    async fn list_tools(&self,_request:Option<PaginatedRequestParams>,_context:RequestContext<RoleServer>)->Result<ListToolsResult,ErrorData>{Ok(ListToolsResult::with_all_items(ACTIVE_COMMANDS.iter().filter_map(|id|tool(id)).collect()))}
    fn get_tool(&self,name:&str)->Option<Tool>{tool(name)}
    async fn call_tool(&self,request:CallToolRequestParams,context:RequestContext<RoleServer>)->Result<CallToolResponse,ErrorData>{
        let principal=if self.stdio{Principal::DiscoveryAgent}else{context.extensions.get::<axum::http::request::Parts>().and_then(|parts|parts.extensions.get::<Principal>()).copied().ok_or_else(||ErrorData::invalid_request("Authenticated request context is missing",None))?};
        let args=request.arguments.unwrap_or_default();
        if args.len()!=1||!args.contains_key("params"){return Err(ErrorData::invalid_params("Expected exactly one typed params object",None));}
        let response=CommandService.dispatch(principal,Request::query(request.name.as_ref(),args["params"].clone()));
        let value=serde_json::to_value(&response).map_err(|_|ErrorData::internal_error("Response serialization failed",None))?;
        Ok(if response.ok{CallToolResult::structured(value)}else{CallToolResult::structured_error(value)}.into())
    }
}
