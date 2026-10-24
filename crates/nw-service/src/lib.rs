//! Shared development dispatch. No adapter owns a second selector implementation.
use nw_domain::{
    DomainError,
    commands::{self, Empty, IdInput, Request, ResolveInput, Response},
};
mod runtime;
pub use runtime::{ArtifactTransfer, Context, Service};

pub use nw_domain::commands::ACTIVE_COMMANDS;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    DiscoveryAgent,
    LocalUser,
}
#[derive(Debug, Clone, Default)]
pub struct CommandService;
impl CommandService {
    pub fn dispatch(&self, principal: Principal, request: Request) -> Response {
        let id = request.request_id.clone();
        let result = (|| {
            request.validate()?;
            if !ACTIVE_COMMANDS.contains(&request.command.id.as_str()) {
                return Err(DomainError::new(
                    "CAPABILITY_UNAVAILABLE",
                    "This operation awaits the protected owner/store/runtime integration gate; it is not offered as an implemented tool",
                ));
            }
            if principal == Principal::DiscoveryAgent
                && !["commands.list", "commands.schema"].contains(&request.command.id.as_str())
            {
                return Err(DomainError::new(
                    "PERMISSION_DENIED",
                    "Default agent profile is discovery-only; operating profiles remain under implementation",
                ));
            }
            if request.replay_scope.is_some() || request.idempotency_key.is_some() {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "These pure query commands do not accept durable-mutation identity",
                ));
            }
            match request.command.id.as_str() {
                "commands.list" => {
                    let _: Empty = commands::parse(request.params)?;
                    Ok(
                        serde_json::json!({"commands":commands::registry().into_iter().filter(|d|ACTIVE_COMMANDS.contains(&d.id.as_str())).collect::<Vec<_>>(),"release":"development_only","full_release":"blocked"}),
                    )
                }
                "commands.schema" => {
                    let params: IdInput = commands::parse(request.params)?;
                    if !ACTIVE_COMMANDS.contains(&params.id.as_str()) {
                        return Err(DomainError::new(
                            "CAPABILITY_UNAVAILABLE",
                            "Command is not implemented by this development dispatch",
                        ));
                    }
                    Ok(serde_json::to_value(commands::descriptor(&params.id))
                        .expect("descriptor serializes"))
                }
                "streams.clean.resolve" => {
                    let params: ResolveInput = commands::parse(request.params)?;
                    serde_json::to_value(nw_domain::mux::resolve(&params.streams, &params.intent)?)
                        .map_err(|e| DomainError::new("SERIALIZATION_FAILED", e.to_string()))
                }
                _ => Err(DomainError::new("UNKNOWN_COMMAND", "No handler")),
            }
        })();
        match result {
            Ok(value) => Response::success(&id, value),
            Err(error) => Response::error(&id, error),
        }
        .bounded()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_agent_never_inherits_local_user_authority() {
        let request = Request::query("streams.clean.resolve", serde_json::json!({}));
        let result = CommandService.dispatch(Principal::DiscoveryAgent, request);
        assert_eq!(result.error.unwrap().code, "PERMISSION_DENIED");
    }
    #[test]
    fn inert_future_operations_are_not_advertised() {
        let response = CommandService.dispatch(
            Principal::DiscoveryAgent,
            Request::query("commands.list", serde_json::json!({})),
        );
        let commands = response.result.unwrap()["commands"]
            .as_array()
            .unwrap()
            .clone();
        assert!(commands.iter().any(|c| c["id"] == "jobs.submit"));
        assert!(!commands.iter().any(|c| c["id"] == "shell.execute"));
    }
}
