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

        let list_response = CommandService.dispatch(
            Principal::DiscoveryAgent,
            Request::query("commands.list", serde_json::json!({})),
        );
        assert!(list_response.ok, "{list_response:?}");
        assert!(list_response.error.is_none(), "{list_response:?}");
        assert!(list_response.receipt_id.is_none(), "{list_response:?}");
        let list = list_response
            .result
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("commands.list must return an object: {list_response:?}"));
        assert_eq!(
            list.get("release").and_then(serde_json::Value::as_str),
            Some("development_only"),
            "{list_response:?}"
        );
        assert_eq!(
            list.get("full_release").and_then(serde_json::Value::as_str),
            Some("blocked"),
            "{list_response:?}"
        );
        let listed_commands = list
            .get("commands")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("commands.list must contain commands: {list_response:?}"));
        for id in ["projects.create", "clips.link"] {
            assert_eq!(
                listed_commands
                    .iter()
                    .filter(
                        |descriptor| descriptor.get("id").and_then(serde_json::Value::as_str)
                            == Some(id)
                    )
                    .count(),
                1,
                "exactly one {id} descriptor is required: {list_response:?}"
            );
        }
        let unknown_id = "test.missing_metadata_probe";
        assert!(
            !listed_commands.iter().any(|descriptor| {
                descriptor.get("id").and_then(serde_json::Value::as_str) == Some(unknown_id)
            }),
            "unknown metadata ID must not be advertised: {list_response:?}"
        );
        let unknown_response = CommandService.dispatch(
            Principal::DiscoveryAgent,
            Request::query("commands.schema", serde_json::json!({"id": unknown_id})),
        );
        assert!(!unknown_response.ok, "{unknown_response:?}");
        assert_eq!(
            unknown_response
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("CAPABILITY_UNAVAILABLE"),
            "{unknown_response:?}"
        );
        assert!(unknown_response.result.is_none(), "{unknown_response:?}");
        assert!(
            unknown_response.receipt_id.is_none(),
            "{unknown_response:?}"
        );

        let schema_response = CommandService.dispatch(
            Principal::DiscoveryAgent,
            Request::query(
                "commands.schema",
                serde_json::json!({"id": "projects.create"}),
            ),
        );
        assert!(schema_response.ok, "{schema_response:?}");
        assert!(schema_response.error.is_none(), "{schema_response:?}");
        assert!(schema_response.receipt_id.is_none(), "{schema_response:?}");
        let descriptor = schema_response
            .result
            .as_ref()
            .filter(|descriptor| descriptor.is_object())
            .unwrap_or_else(|| {
                panic!("commands.schema must return a descriptor: {schema_response:?}")
            });
        assert_eq!(
            descriptor.get("id"),
            Some(&serde_json::json!("projects.create")),
            "{schema_response:?}"
        );
        assert_eq!(
            descriptor.get("version"),
            Some(&serde_json::json!(1)),
            "{schema_response:?}"
        );
        assert_eq!(
            descriptor.get("state_class"),
            Some(&serde_json::json!("durable_mutation")),
            "{schema_response:?}"
        );
        assert!(
            descriptor.get("project_context").is_none(),
            "{schema_response:?}"
        );
        let params_schema = descriptor
            .get("params_schema")
            .filter(|schema| schema.is_object())
            .unwrap_or_else(|| panic!("params_schema must be an object: {schema_response:?}"));
        assert_eq!(
            params_schema.get("type"),
            Some(&serde_json::json!("object")),
            "{schema_response:?}"
        );
        assert_eq!(
            params_schema.pointer("/properties/name/type"),
            Some(&serde_json::json!("string")),
            "{schema_response:?}"
        );
        assert!(
            params_schema
                .get("required")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|required| {
                    required.iter().any(|name| name.as_str() == Some("name"))
                }),
            "params_schema must require name: {schema_response:?}"
        );
        let listed_descriptor = listed_commands
            .iter()
            .find(|entry| {
                entry.get("id").and_then(serde_json::Value::as_str) == Some("projects.create")
            })
            .unwrap_or_else(|| panic!("projects.create descriptor missing: {list_response:?}"));
        assert_eq!(
            descriptor, listed_descriptor,
            "list={list_response:?}; schema={schema_response:?}"
        );

        assert!(
            descriptor
                .get("result_schema")
                .is_some_and(serde_json::Value::is_object),
            "projects.create must publish its finite result_schema: {schema_response:?}"
        );
        let result_schema = descriptor
            .get("result_schema")
            .unwrap_or_else(|| panic!("result_schema missing: {schema_response:?}"));
        let resolve = |segments: &[&str]| {
            let mut node = result_schema;
            for step in 0..=segments.len() {
                for _ in 0..16 {
                    let Some(reference) = node.get("$ref") else {
                        break;
                    };
                    let reference = reference.as_str().unwrap_or_else(|| {
                        panic!("schema reference must be a string: {schema_response:?}")
                    });
                    assert!(
                        reference.starts_with("#/$defs/"),
                        "schema reference must use its own $defs: {schema_response:?}"
                    );
                    node = result_schema.pointer(&reference[1..]).unwrap_or_else(|| {
                        panic!("schema reference must resolve locally: {schema_response:?}")
                    });
                }
                assert!(
                    node.get("$ref").is_none(),
                    "schema reference exceeds 16 hops: {schema_response:?}"
                );
                assert!(
                    node.is_object(),
                    "schema node must be an object: {schema_response:?}"
                );
                if let Some(segment) = segments.get(step) {
                    node = node.get(*segment).unwrap_or_else(|| {
                        panic!("schema path {segments:?} must exist: {schema_response:?}")
                    });
                }
            }
            node
        };
        let result_root = resolve(&[]);
        assert_eq!(
            result_root.get("type"),
            Some(&serde_json::json!("object")),
            "{schema_response:?}"
        );
        let result_properties = result_root
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| {
                panic!("result_schema must declare properties: {schema_response:?}")
            });
        let result_required = result_root
            .get("required")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| {
                panic!("result_schema must declare required fields: {schema_response:?}")
            });
        let result_fields = ["project_id", "root_bin_id", "revision", "history"];
        assert_eq!(
            result_properties.len(),
            result_fields.len(),
            "{schema_response:?}"
        );
        assert_eq!(
            result_required.len(),
            result_fields.len(),
            "{schema_response:?}"
        );
        for field in result_fields {
            assert!(
                result_properties.contains_key(field),
                "missing result property {field}: {schema_response:?}"
            );
            assert!(
                result_required
                    .iter()
                    .any(|name| name.as_str() == Some(field)),
                "missing required result field {field}: {schema_response:?}"
            );
        }
        for field in ["project_id", "root_bin_id", "revision"] {
            assert_eq!(
                resolve(&["properties", field]).get("type"),
                Some(&serde_json::json!("string")),
                "{field} must be a string: {schema_response:?}"
            );
        }
        let history = resolve(&["properties", "history"]);
        assert_eq!(
            history.get("type"),
            Some(&serde_json::json!("object")),
            "{schema_response:?}"
        );
        let history_properties = history
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("history must declare properties: {schema_response:?}"));
        let history_required = history
            .get("required")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("history must declare required fields: {schema_response:?}"));
        assert_eq!(
            history_properties.len(),
            2,
            "history must have two fields: {schema_response:?}"
        );
        assert_eq!(
            history_required.len(),
            2,
            "history must require two fields: {schema_response:?}"
        );
        for field in ["can_undo", "can_redo"] {
            assert!(
                history_properties.contains_key(field),
                "missing history property {field}: {schema_response:?}"
            );
            assert!(
                history_required
                    .iter()
                    .any(|name| name.as_str() == Some(field)),
                "missing required history field {field}: {schema_response:?}"
            );
            assert_eq!(
                resolve(&["properties", "history", "properties", field]).get("type"),
                Some(&serde_json::json!("boolean")),
                "{field} must be a boolean: {schema_response:?}"
            );
        }

        assert_eq!(
            descriptor.get("descriptor_schema_version"),
            Some(&serde_json::json!(2)),
            "{schema_response:?}"
        );
        let emitted: commands::Descriptor = serde_json::from_value(descriptor.clone())
            .unwrap_or_else(|error| {
                panic!("descriptor must deserialize: {error}; {schema_response:?}")
            });
        assert_eq!(emitted.descriptor_schema_version, 2, "{schema_response:?}");
        assert!(emitted.result_schema.is_some(), "{schema_response:?}");

        let historical = serde_json::json!({
            "id": "projects.create",
            "version": 1,
            "label": "Create a durable project",
            "state_class": "durable_mutation",
            "qualification": "development_only; five-surface/package qualification pending",
            "params_schema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            }
        });
        let legacy: commands::Descriptor = serde_json::from_value(historical.clone())
            .unwrap_or_else(|error| panic!("historical descriptor must deserialize: {error}"));
        assert_eq!(legacy.descriptor_schema_version, 1, "{legacy:?}");
        assert!(legacy.result_schema.is_none(), "{legacy:?}");
        assert_eq!(
            legacy.project_context,
            commands::ProjectContextRule::Forbidden,
            "{legacy:?}"
        );
        let serialized_legacy = serde_json::to_value(&legacy).expect("descriptor serializes");
        assert!(
            serialized_legacy.get("result_schema").is_none(),
            "{serialized_legacy:?}"
        );
        assert!(
            serialized_legacy.get("project_context").is_none(),
            "{serialized_legacy:?}"
        );
        let mut expected_legacy = historical;
        expected_legacy["descriptor_schema_version"] = serde_json::json!(1);
        assert_eq!(serialized_legacy, expected_legacy, "{legacy:?}");

        let other_response = CommandService.dispatch(
            Principal::DiscoveryAgent,
            Request::query(
                "commands.schema",
                serde_json::json!({"id": "commands.schema"}),
            ),
        );
        assert!(other_response.ok, "{other_response:?}");
        assert!(other_response.error.is_none(), "{other_response:?}");
        assert!(other_response.receipt_id.is_none(), "{other_response:?}");
        let other_descriptor = other_response
            .result
            .as_ref()
            .filter(|descriptor| descriptor.is_object())
            .unwrap_or_else(|| panic!("commands.schema descriptor missing: {other_response:?}"));
        assert_eq!(
            other_descriptor.get("id"),
            Some(&serde_json::json!("commands.schema")),
            "{other_response:?}"
        );
        assert_eq!(
            other_descriptor.get("descriptor_schema_version"),
            Some(&serde_json::json!(2)),
            "{other_response:?}"
        );
        assert!(
            other_descriptor.get("result_schema").is_none(),
            "{other_response:?}"
        );
    }
}
