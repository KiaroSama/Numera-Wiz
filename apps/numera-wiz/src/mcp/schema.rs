//! Output schemas describe structuredContent's whole canonical Response.
use serde_json::{Value, json};

const SCHEMA_MAPS: [&str; 5] = [
    "$defs",
    "definitions",
    "properties",
    "patternProperties",
    "dependentSchemas",
];
const SCHEMA_ARRAYS: [&str; 4] = ["allOf", "anyOf", "oneOf", "prefixItems"];
const SCHEMA_VALUES: [&str; 10] = [
    "items",
    "additionalProperties",
    "unevaluatedProperties",
    "propertyNames",
    "contains",
    "not",
    "if",
    "then",
    "else",
    "unevaluatedItems",
];

// Traverse schema positions only: enum/default/example objects are data, not references.
fn rebase(node: &mut Value, prefix: &str) -> Option<()> {
    if node.is_boolean() {
        return Some(());
    }
    let object = node.as_object_mut()?;
    if ["$id", "$anchor", "$dynamicAnchor"]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return None;
    }
    object.remove("$schema");
    for key in ["$ref", "$dynamicRef"] {
        if let Some(reference) = object.get_mut(key) {
            let original = reference.as_str()?;
            let suffix = if original == "#" {
                ""
            } else {
                original
                    .strip_prefix('#')
                    .filter(|text| text.starts_with('/'))?
            };
            *reference = json!(format!("{prefix}{suffix}"));
        }
    }
    for key in SCHEMA_MAPS {
        if let Some(children) = object.get_mut(key) {
            for child in children.as_object_mut()?.values_mut() {
                rebase(child, prefix)?;
            }
        }
    }
    for key in SCHEMA_ARRAYS {
        if let Some(children) = object.get_mut(key) {
            for child in children.as_array_mut()? {
                rebase(child, prefix)?;
            }
        }
    }
    for key in SCHEMA_VALUES.into_iter().chain(["contentSchema"]) {
        if let Some(child) = object.get_mut(key) {
            rebase(child, prefix)?;
        }
    }
    Some(())
}

fn local_references(root: &Value, node: &Value) -> Option<()> {
    if node.is_boolean() {
        return Some(());
    }
    let object = node.as_object()?;
    for key in ["$ref", "$dynamicRef"] {
        if let Some(reference) = object.get(key) {
            let pointer = reference.as_str()?.strip_prefix('#')?;
            if !pointer.is_empty() && (!pointer.starts_with('/') || root.pointer(pointer).is_none())
            {
                return None;
            }
        }
    }
    for key in SCHEMA_MAPS {
        if let Some(children) = object.get(key) {
            for child in children.as_object()?.values() {
                local_references(root, child)?;
            }
        }
    }
    for key in SCHEMA_ARRAYS {
        if let Some(children) = object.get(key) {
            for child in children.as_array()? {
                local_references(root, child)?;
            }
        }
    }
    for key in SCHEMA_VALUES.into_iter().chain(["contentSchema"]) {
        if let Some(child) = object.get(key) {
            local_references(root, child)?;
        }
    }
    Some(())
}

pub(super) fn response(mut result: Value) -> Option<Value> {
    let mut error = serde_json::to_value(schemars::schema_for!(nw_domain::DomainError)).ok()?;
    // The output serializer always emits field, including an explicit null.
    error["required"] = json!(["code", "message", "field", "retryable"]);
    rebase(&mut result, "#/$defs/Result")?;
    rebase(&mut error, "#/$defs/Error")?;
    let root = json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object","additionalProperties":false,"required":["request_id","ok"],
        "properties":{
            "request_id":{"type":"string"},"ok":{"type":"boolean"},
            "result":{"$ref":"#/$defs/Result"},"error":{"$ref":"#/$defs/Error"},
            "receipt_id":{"type":"string"}
        },
        "oneOf":[
            {"required":["result"],"properties":{"ok":{"const":true}},"not":{"required":["error"]}},
            {"required":["error"],"properties":{"ok":{"const":false}},"not":{"required":["result"]}}
        ],
        "$defs":{"Result":result,"Error":error}
    });
    local_references(&root, &root)?;
    Some(root)
}
