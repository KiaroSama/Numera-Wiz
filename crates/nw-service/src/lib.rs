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

        project_family_result_schemas_are_published_and_self_contained();
    }

    fn project_family_result_schemas_are_published_and_self_contained() {
        let ids = [
            "projects.create",
            "projects.list",
            "projects.inspect",
            "sequences.create",
            "sequences.inspect",
            "clips.place",
            "clips.move",
            "clips.link",
            "clips.unlink",
            "clips.split",
            "clips.remove",
            "history.undo",
            "history.redo",
        ];
        let list_response = CommandService.dispatch(
            Principal::DiscoveryAgent,
            Request::query("commands.list", serde_json::json!({})),
        );
        assert!(
            list_response.ok,
            "Project family commands.list must succeed"
        );
        assert!(
            list_response.error.is_none(),
            "Project family commands.list must have no error"
        );
        assert!(
            list_response.receipt_id.is_none(),
            "Project family commands.list must have no receipt"
        );
        let list_bytes = serde_json::to_vec(&list_response)
            .expect("complete commands.list Response serializes")
            .len();
        eprintln!("Project family discovery: commands.list Response bytes={list_bytes}");
        assert!(
            list_bytes <= commands::MAX_RESULT_BYTES,
            "Project family commands.list Response exceeds the byte budget: {list_bytes}"
        );
        let rows = list_response
            .result
            .as_ref()
            .and_then(|result| result.get("commands"))
            .and_then(serde_json::Value::as_array)
            .expect("Project family commands.list must contain command rows");
        for id in ids {
            assert_eq!(
                rows.iter()
                    .filter(|row| row.get("id").and_then(serde_json::Value::as_str) == Some(id))
                    .count(),
                1,
                "Project family commands.list must contain exactly one row for {id}"
            );
        }

        let mut largest_schema_bytes = 0;
        for id in ids {
            let schema_response = CommandService.dispatch(
                Principal::DiscoveryAgent,
                Request::query("commands.schema", serde_json::json!({"id": id})),
            );
            assert!(
                schema_response.ok,
                "Project family commands.schema must succeed for {id}"
            );
            assert!(
                schema_response.error.is_none(),
                "Project family commands.schema must have no error for {id}"
            );
            assert!(
                schema_response.receipt_id.is_none(),
                "Project family commands.schema must have no receipt for {id}"
            );
            let response_bytes = serde_json::to_vec(&schema_response)
                .expect("complete commands.schema Response serializes")
                .len();
            eprintln!(
                "Project family discovery: commands.schema Response bytes={response_bytes} for {id}"
            );
            assert!(
                response_bytes <= commands::MAX_RESULT_BYTES,
                "Project family commands.schema Response exceeds the byte budget for {id}: {response_bytes}"
            );
            let descriptor = schema_response
                .result
                .as_ref()
                .filter(|value| value.is_object())
                .expect("Project family commands.schema must return a descriptor object");
            assert_eq!(
                descriptor.get("id").and_then(serde_json::Value::as_str),
                Some(id),
                "Project family descriptor ID must match {id}"
            );
            assert_eq!(
                descriptor.get("version"),
                Some(&serde_json::json!(1)),
                "Project family command version must remain 1 for {id}"
            );
            assert_eq!(
                descriptor.get("descriptor_schema_version"),
                Some(&serde_json::json!(2)),
                "Project family descriptor format must be 2 for {id}"
            );
            let row = rows
                .iter()
                .find(|row| row.get("id").and_then(serde_json::Value::as_str) == Some(id))
                .expect("Project family descriptor row was counted above");
            assert_eq!(
                descriptor, row,
                "Project family commands.schema must equal its commands.list row for {id}"
            );
            assert!(
                descriptor
                    .get("result_schema")
                    .is_some_and(serde_json::Value::is_object),
                "Project family result_schema must be published for {id}"
            );
            let result_schema = descriptor
                .get("result_schema")
                .expect("Project family result_schema presence was checked above");
            assert!(
                result_schema
                    .as_object()
                    .is_some_and(|object| !object.is_empty()),
                "Project family result_schema must be a nonempty object for {id}"
            );
            let schema_bytes = serde_json::to_vec(result_schema)
                .expect("Project family result_schema serializes")
                .len();
            assert!(
                schema_bytes <= commands::MAX_RESULT_BYTES,
                "Project family result_schema exceeds the byte budget for {id}: {schema_bytes}"
            );
            largest_schema_bytes = largest_schema_bytes.max(schema_bytes);
            let mut pending = vec![result_schema];
            let mut visited = 0;
            while let Some(node) = pending.pop() {
                visited += 1;
                assert!(
                    visited <= schema_bytes,
                    "Project family result_schema traversal exceeds its byte-derived node budget for {id}"
                );
                match node {
                    serde_json::Value::Object(object) => {
                        if let Some(reference) = object.get("$ref") {
                            let reference = reference.as_str().unwrap_or_else(|| {
                                panic!(
                                    "Project family result_schema $ref must be a string for {id}"
                                )
                            });
                            assert!(
                                reference == "#" || reference.starts_with("#/"),
                                "Project family result_schema $ref must be local for {id}"
                            );
                            assert!(
                                result_schema.pointer(&reference[1..]).is_some(),
                                "Project family result_schema $ref must resolve against its own root for {id}"
                            );
                        }
                        pending.extend(object.values());
                    }
                    serde_json::Value::Array(values) => pending.extend(values),
                    _ => {}
                }
            }
        }
        eprintln!("Project family discovery: largest result_schema bytes={largest_schema_bytes}");

        // Independent finite shape observations. This is not a JSON Schema validator
        // or evidence that every real response conforms to its descriptor.
        use serde_json::Value;

        fn resolve<'a>(root: &'a Value, mut node: &'a Value) -> &'a Value {
            for _ in 0..64 {
                let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
                    return node;
                };
                node = root
                    .pointer(&reference[1..])
                    .expect("local schema reference");
            }
            panic!("Result schema reference cycle");
        }
        fn fields(root: &Value, node: &Value, expected: &[&str]) {
            let node = resolve(root, node);
            assert_eq!(node["type"], "object");
            assert_eq!(node["additionalProperties"], false);
            let properties = node["properties"].as_object().expect("finite properties");
            let actual: std::collections::BTreeSet<_> =
                properties.keys().map(String::as_str).collect();
            let expected: std::collections::BTreeSet<_> = expected.iter().copied().collect();
            assert_eq!(actual, expected);
            let required: std::collections::BTreeSet<_> = node["required"]
                .as_array()
                .expect("required fields")
                .iter()
                .map(|value| value.as_str().expect("required field name"))
                .collect();
            assert_eq!(required, expected);
        }
        // Observe only object-key admission, with no claim about scalar validation.
        fn permits_keys(root: &Value, node: &Value, keys: &[&str]) -> bool {
            let node = resolve(root, node);
            let properties = node["properties"].as_object().expect("object properties");
            keys.iter().all(|key| properties.contains_key(*key))
                && node["required"]
                    .as_array()
                    .expect("required keys")
                    .iter()
                    .all(|key| keys.contains(&key.as_str().expect("required key")))
        }
        let schema = |id: &str| -> &Value {
            &rows
                .iter()
                .find(|row| row["id"] == id)
                .expect("listed command")["result_schema"]
        };
        let catalog = schema("projects.list");
        fields(
            catalog,
            catalog,
            &[
                "schema_version",
                "project_revision",
                "snapshot_id",
                "snapshot_expires_at",
                "projection",
                "items",
                "next_cursor",
                "event_cursor",
                "total_count",
                "count_accuracy",
            ],
        );
        assert_eq!(
            catalog["properties"]["schema_version"],
            serde_json::json!({"const":1})
        );
        assert_eq!(
            catalog["properties"]["projection"],
            serde_json::json!({"const":"projects.summary.v1"})
        );
        assert_eq!(
            catalog["properties"]["count_accuracy"],
            serde_json::json!({"const":"exact"})
        );
        for key in ["project_revision", "event_cursor"] {
            assert_eq!(
                catalog["properties"][key],
                serde_json::json!({"type":"null"})
            );
        }
        assert_eq!(catalog["properties"]["items"]["maxItems"], 64);
        let row = resolve(catalog, &catalog["properties"]["items"]["items"]);
        fields(
            catalog,
            row,
            &[
                "id",
                "name",
                "revision",
                "schema_major",
                "envelope_version",
                "compatibility",
            ],
        );
        assert_eq!(
            row["properties"]["compatibility"]["enum"],
            serde_json::json!(["supported", "read_only_unsupported"])
        );

        let detail_common = [
            "schema_version",
            "project_revision",
            "snapshot_id",
            "snapshot_expires_at",
            "projection",
            "event_cursor",
        ];
        for (id, projections) in [
            ("projects.inspect", ["summary", "entity", "bin_page"]),
            ("sequences.inspect", ["entity", "track_page", "viewport"]),
        ] {
            let root = schema(id);
            let branches = root["oneOf"].as_array().expect("projection alternatives");
            assert_eq!(branches.len(), 3);
            for projection in projections {
                let branch = branches
                    .iter()
                    .find(|branch| branch["properties"]["projection"]["const"] == projection)
                    .expect("literal projection");
                let mut keys = detail_common.to_vec();
                match projection {
                    "summary" => keys.extend([
                        "project_id",
                        "root_bin_id",
                        "name",
                        "entity_count",
                        "sequence_count",
                        "schema_major",
                        "envelope_version",
                        "compatibility",
                        "history",
                        "writer_status",
                        "read_only",
                    ]),
                    "entity" => keys.push("entity"),
                    _ => keys.extend(["items", "next_cursor", "total_count", "count_accuracy"]),
                }
                fields(root, branch, &keys);
                assert_eq!(
                    branch["properties"]["schema_version"],
                    serde_json::json!({"const":1})
                );
                assert_eq!(
                    branch["properties"]["event_cursor"],
                    serde_json::json!({"type":"null"})
                );
                assert!(
                    resolve(root, &branch["properties"]["project_revision"])["pattern"].is_string()
                );
                if projection == "summary" {
                    assert_eq!(
                        branch["properties"]["writer_status"]["enum"],
                        serde_json::json!([
                            "this_owner",
                            "available",
                            "other_owner_read_only",
                            "writer_lease_unavailable_read_only",
                        ])
                    );
                    assert_eq!(branch["properties"]["read_only"]["type"], "boolean");
                } else if projection != "entity" {
                    let next = branch["properties"]["next_cursor"]["anyOf"]
                        .as_array()
                        .unwrap();
                    assert_eq!(
                        next,
                        &vec![
                            serde_json::json!({"type":"string"}),
                            serde_json::json!({"type":"null"})
                        ]
                    );
                    assert_eq!(branch["properties"]["items"]["maxItems"], 512);
                    let item = &branch["properties"]["items"]["items"];
                    match projection {
                        "bin_page" => fields(root, item, &["id", "kind", "name"]),
                        "track_page" => {
                            fields(root, item, &["id", "kind", "name", "order", "clip_count"]);
                            assert_eq!(
                                resolve(root, item)["properties"]["kind"]["enum"],
                                serde_json::json!(["video", "audio"])
                            );
                        }
                        "viewport" => fields(
                            root,
                            item,
                            &["id", "track_id", "source_id", "sequence_start", "duration"],
                        ),
                        _ => unreachable!(),
                    }
                    if projection == "viewport" {
                        assert_eq!(
                            branch["properties"]["count_accuracy"]["enum"],
                            serde_json::json!(["exact", "lower_bound"])
                        );
                    } else {
                        assert_eq!(
                            branch["properties"]["count_accuracy"],
                            serde_json::json!({"const":"exact"})
                        );
                    }
                }
            }
        }
        let inspect = schema("projects.inspect");
        let defs = &inspect["$defs"];
        let entity_common = [
            "id",
            "kind",
            "kind_version",
            "envelope_version",
            "owner_id",
            "enabled",
            "references",
            "semantic_scope",
            "name",
            "payload_sha256",
            "compatibility",
        ];
        for (name, extra) in [
            ("BinEntity", vec!["member_count", "locked"]),
            ("SequenceEntity", vec!["settings", "track_count", "locked"]),
            (
                "TrackEntity",
                vec!["sequence_id", "order", "grid", "clip_count", "locked"],
            ),
            (
                "ClipEntity",
                vec![
                    "sequence_id",
                    "track_id",
                    "source_id",
                    "occurrence_id",
                    "source_range",
                    "sequence_start",
                    "duration",
                    "rate",
                    "link_group_id",
                    "locked",
                ],
            ),
            ("SolidSourceEntity", vec!["source_kind", "rgba", "duration"]),
            (
                "RegisteredSourceEntity",
                vec![
                    "source_kind",
                    "asset_id",
                    "content_sha256",
                    "probe_sha256",
                    "stream_index",
                    "media_kind",
                    "extent_verification",
                ],
            ),
            ("OpaqueEntity", vec!["payload_json"]),
        ] {
            let node = &defs[name];
            let mut keys = entity_common.to_vec();
            keys.extend(extra);
            fields(inspect, node, &keys);
            assert_eq!(
                node["properties"]["compatibility"]["enum"],
                serde_json::json!([
                    "supported_or_optional_annotation",
                    "read_only_affected_scope"
                ])
            );
            assert_eq!(
                node["properties"]["owner_id"]["anyOf"][1],
                serde_json::json!({"type":"null"})
            );
            if name != "OpaqueEntity" {
                assert_eq!(
                    node["properties"]["kind_version"],
                    serde_json::json!({"const":1})
                );
                assert_eq!(
                    node["properties"]["envelope_version"],
                    serde_json::json!({"const":1})
                );
                assert!(node["properties"].get("payload_json").is_none());
            }
        }
        fields(inspect, &defs["EntityReference"], &["role", "target"]);
        fields(
            inspect,
            &defs["SequenceSettings"],
            &[
                "width",
                "height",
                "frame_rate",
                "sample_rate",
                "audio_layout",
            ],
        );
        fields(inspect, &defs["SourceSpan"], &["start", "end"]);
        assert_eq!(
            defs["EntityReference"]["properties"]["role"]["type"],
            "string"
        );
        assert_eq!(
            defs["TrackEntity"]["properties"]["kind"]["enum"],
            serde_json::json!(["video", "audio"])
        );
        assert_eq!(
            defs["SolidSourceEntity"]["properties"]["kind"]["const"],
            "source_binding"
        );
        assert_eq!(
            defs["SolidSourceEntity"]["properties"]["source_kind"]["const"],
            "solid_color"
        );
        assert_eq!(
            defs["RegisteredSourceEntity"]["properties"]["source_kind"]["const"],
            "registered"
        );
        assert_eq!(
            defs["RegisteredSourceEntity"]["properties"]["extent_verification"]["type"],
            "string"
        );
        assert_eq!(
            defs["RegisteredSourceEntity"]["properties"]["media_kind"]["enum"],
            serde_json::json!(["video", "audio"])
        );
        let rgba = &defs["SolidSourceEntity"]["properties"]["rgba"];
        assert_eq!(rgba["minItems"], 4);
        assert_eq!(rgba["maxItems"], 4);
        assert_eq!(rgba["items"]["type"], "number");
        assert_eq!(
            defs["OpaqueEntity"]["properties"]["payload_json"],
            serde_json::json!({"type":"string"})
        );
        assert_ne!(
            defs["OpaqueEntity"]["properties"]["payload_json"]["type"],
            "object"
        );
        assert_eq!(
            defs["OpaqueEntity"]["not"]["properties"]["kind"]["enum"],
            serde_json::json!(["bin", "sequence", "track", "clip", "source_binding"])
        );
        assert_eq!(defs["Entity"]["anyOf"].as_array().unwrap().len(), 7);
        assert!(defs["Entity"].get("oneOf").is_none());
        let mut opaque_keys = entity_common.to_vec();
        assert!(!permits_keys(inspect, &defs["OpaqueEntity"], &opaque_keys));
        opaque_keys.push("payload_json");
        assert!(permits_keys(inspect, &defs["OpaqueEntity"], &opaque_keys));
        opaque_keys.push("project_id");
        assert!(!permits_keys(inspect, &defs["OpaqueEntity"], &opaque_keys));
        let sequence = schema("sequences.inspect");
        let sequence_entities = &sequence["$defs"]["SequenceProjectionEntity"]["anyOf"];
        assert_eq!(
            sequence_entities,
            &serde_json::json!([
                {"$ref":"#/$defs/SequenceEntity"},{"$ref":"#/$defs/OpaqueSequenceEntity"},
            ])
        );
        assert_eq!(
            sequence["$defs"]["OpaqueSequenceEntity"]["properties"]["kind"]["const"],
            "sequence"
        );
        fields(inspect, &defs["Rational"], &["num", "den"]);
        assert_eq!(
            defs["Rational"]["properties"]["num"]["pattern"],
            "^(0|-?[1-9][0-9]*)$"
        );
        assert_eq!(
            defs["Rational"]["properties"]["den"]["pattern"],
            "^[1-9][0-9]*$"
        );

        let edit_keys = [
            "project_id",
            "base_revision",
            "revision",
            "operation_id",
            "created",
            "changed_ids",
            "history",
        ];
        for (id, created_keys, noop) in [
            ("sequences.create", vec!["sequence_id", "tracks"], false),
            ("clips.place", vec!["source_id", "clip_ids"], false),
            ("clips.move", vec![], true),
            ("clips.link", vec!["link_group_id", "clip_ids"], true),
            ("clips.unlink", vec!["clip_ids"], true),
            ("clips.split", vec!["clip_ids", "no_change"], true),
            ("clips.remove", vec![], false),
            ("history.undo", vec![], false),
            ("history.redo", vec![], false),
        ] {
            let root = schema(id);
            let changed = if noop { &root["oneOf"][0] } else { root };
            fields(root, changed, &edit_keys);
            assert!(permits_keys(root, changed, &edit_keys));
            let mut marked_keys = edit_keys.to_vec();
            marked_keys.push("no_change");
            assert!(!permits_keys(root, changed, &marked_keys));
            assert!(changed["properties"].get("no_change").is_none());
            assert_eq!(changed["properties"]["changed_ids"]["minItems"], 1);
            fields(root, &changed["properties"]["created"], &created_keys);
            fields(
                root,
                &changed["properties"]["history"],
                &[
                    "can_undo",
                    "can_redo",
                    "retained_operations",
                    "retained_bytes",
                    "history_floor",
                    "journal_floor",
                ],
            );
            for floor in ["history_floor", "journal_floor"] {
                assert_eq!(
                    resolve(root, &root["$defs"]["History"]["properties"][floor])["type"],
                    "string"
                );
            }
            if noop {
                assert_eq!(root["oneOf"].as_array().unwrap().len(), 2);
                let unchanged = &root["oneOf"][1];
                let mut keys = edit_keys.to_vec();
                keys.push("no_change");
                fields(root, unchanged, &keys);
                assert!(permits_keys(root, unchanged, &keys));
                assert!(!permits_keys(root, unchanged, &edit_keys));
                assert_eq!(
                    unchanged["properties"]["no_change"],
                    serde_json::json!({"const":true})
                );
                assert_ne!(
                    unchanged["properties"]["no_change"],
                    serde_json::json!({"const":false})
                );
                assert_eq!(unchanged["properties"]["changed_ids"]["maxItems"], 0);
                fields(root, &unchanged["properties"]["created"], &created_keys);
            } else {
                assert!(root.get("oneOf").is_none());
            }
            if id == "clips.split" {
                assert_eq!(
                    changed["properties"]["created"]["properties"]["no_change"],
                    serde_json::json!({"const":false})
                );
                assert_eq!(
                    root["oneOf"][1]["properties"]["created"]["properties"]["no_change"],
                    serde_json::json!({"const":true})
                );
                assert_eq!(
                    root["oneOf"][1]["properties"]["created"]["properties"]["clip_ids"]["maxItems"],
                    0
                );
            }
        }
        let sequence_create = schema("sequences.create");
        let tracks = &sequence_create["$defs"]["Created"]["properties"]["tracks"];
        fields(sequence_create, &tracks["items"], &["id", "kind"]);
        assert_eq!(
            tracks["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["video", "audio"])
        );
        let link = schema("clips.link");
        assert_eq!(
            link["$defs"]["Created"]["properties"]["link_group_id"]["$ref"],
            "#/$defs/OpaqueGroup"
        );
        assert_eq!(link["$defs"]["OpaqueGroup"]["type"], "string");
        for (id, minimum) in [("clips.link", 2), ("clips.unlink", 1)] {
            let clips = &schema(id)["$defs"]["Created"]["properties"]["clip_ids"];
            assert_eq!(clips["minItems"], minimum);
            assert_eq!(clips["maxItems"], 128);
            assert_eq!(clips["uniqueItems"], true);
        }
    }
}
