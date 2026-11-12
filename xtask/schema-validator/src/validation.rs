use crate::protocol::{
    Batch, DRAFT, Failure, INSTANCE_LIMIT, SCHEMA_LIMIT, SchemaOutcome, Select, Summary, preflight,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn document(text: &str) -> Result<Value, ()> {
    preflight(text.as_bytes())?;
    serde_json::from_str(text).map_err(|_| ())
}
fn local_refs(root: &Value) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node {
            Value::Object(object) => {
                for name in ["$ref", "$dynamicRef"] {
                    if let Some(reference) = object.get(name) {
                        let Some(reference) = reference.as_str() else {
                            return false;
                        };
                        let Some(pointer) = reference.strip_prefix('#') else {
                            return false;
                        };
                        if !pointer.is_empty()
                            && (!pointer.starts_with('/') || root.pointer(pointer).is_none())
                        {
                            return false;
                        }
                    }
                }
                pending.extend(object.values());
            }
            Value::Array(array) => pending.extend(array),
            _ => {}
        }
    }
    true
}
fn build(schema: &Value) -> Result<jsonschema::Validator, ()> {
    jsonschema::draft202012::options()
        .offline()
        .should_validate_formats(true)
        .build(schema)
        .map_err(|_| ())
}
pub fn run(batch: Batch, summary: &mut Summary) -> Result<(), ()> {
    summary.suite = batch.suite;
    if batch.protocol != 1
        || ![1, 2, 3, 4, 5, 6, 11, 12, 13].contains(&batch.suite)
        || batch.schemas.is_empty()
        || batch.schemas.len() + batch.schema_controls.len() > 64
        || batch.cases.len() > 128
        || batch.cases.is_empty()
        || batch
            .schemas
            .iter()
            .map(|s| s.schema_json.len())
            .sum::<usize>()
            + batch
                .schema_controls
                .iter()
                .map(|s| s.schema_json.len())
                .sum::<usize>()
            > SCHEMA_LIMIT
    {
        return Err(());
    }
    let mut validators = BTreeMap::new();
    let mut all_schema_ids = BTreeSet::new();
    for schema in batch.schemas {
        if !all_schema_ids.insert(schema.id) {
            return Err(());
        }
        summary.schema_ids.push(schema.id);
        let value = document(&schema.schema_json)?;
        if value.get("$schema").and_then(Value::as_str) != Some(DRAFT)
            || jsonschema::meta::validate(&value).is_err()
            || !local_refs(&value)
        {
            summary.failure(
                Failure {
                    code: "SCHEMA_META_OR_REF",
                    schema_id: Some(schema.id),
                    case_id: None,
                    control_id: None,
                },
                true,
            );
            continue;
        }
        match build(&value) {
            Ok(validator) => {
                validators.insert(schema.id, validator);
                summary.passed += 1;
            }
            Err(()) => summary.failure(
                Failure {
                    code: "SCHEMA_BUILD",
                    schema_id: Some(schema.id),
                    case_id: None,
                    control_id: None,
                },
                true,
            ),
        }
    }
    let mut case_ids = BTreeSet::new();
    for case in batch.cases {
        if !case_ids.insert(case.id)
            || !all_schema_ids.contains(&case.schema_id)
            || case.instance_json.len() > INSTANCE_LIMIT
        {
            return Err(());
        }
        summary.case_ids.push(case.id);
        let value = document(&case.instance_json)?;
        let instance = match case.select {
            Select::Whole => &value,
            Select::Result => {
                if value.get("ok") != Some(&Value::Bool(true))
                    || value.get("error").is_some_and(|v| !v.is_null())
                {
                    return Err(());
                }
                value.get("result").filter(|v| !v.is_null()).ok_or(())?
            }
        };
        let Some(validator) = validators.get(&case.schema_id) else {
            summary.failure(
                Failure {
                    code: "SCHEMA_UNAVAILABLE",
                    schema_id: Some(case.schema_id),
                    case_id: Some(case.id),
                    control_id: None,
                },
                true,
            );
            continue;
        };
        if validator.is_valid(instance) == case.valid {
            summary.passed += 1;
        } else {
            summary.failure(
                Failure {
                    code: "OUTCOME",
                    schema_id: Some(case.schema_id),
                    case_id: Some(case.id),
                    control_id: None,
                },
                false,
            );
        }
    }
    for control in batch.schema_controls {
        if !all_schema_ids.insert(control.id) {
            return Err(());
        }
        summary.control_ids.push(control.id);
        let value = document(&control.schema_json)?;
        let meta_valid = value.get("$schema").and_then(Value::as_str) == Some(DRAFT)
            && jsonschema::meta::validate(&value).is_ok();
        let matches = match control.outcome {
            SchemaOutcome::MetaInvalid => !meta_valid,
            SchemaOutcome::BuildInvalid => meta_valid && build(&value).is_err(),
        };
        if matches {
            summary.passed += 1;
        } else {
            summary.failure(
                Failure {
                    code: "SCHEMA_CONTROL",
                    schema_id: None,
                    case_id: None,
                    control_id: Some(control.id),
                },
                false,
            );
        }
    }
    summary.schema_ids.sort_unstable();
    summary.case_ids.sort_unstable();
    summary.control_ids.sort_unstable();
    Ok(())
}
