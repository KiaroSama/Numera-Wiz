//! Versioned reviewed execution requirements, independent of creation credentials.
use crate::{
    DomainError, Result,
    records::{FileIdentity, Plan},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequiredRights {
    pub data_root: String,
    pub source_read: SourceRead,
    pub output_create: OutputCreate,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRead {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<crate::records::NativeObjectIdentity>,
    pub identity: FileIdentity,
    pub purpose: SourcePurpose,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourcePurpose {
    NativeFile,
    CreatorBoundManaged { principal: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputCreate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_object: Option<crate::records::NativeObjectIdentity>,
    pub path: String,
    pub collision_policy: CollisionPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    NoReplace,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreationAudit {
    pub principal: String,
    pub policy_revision: u64,
}
impl RequiredRights {
    pub fn native(source: FileIdentity, output: String) -> Self {
        Self {
            data_root: String::new(),
            source_read: SourceRead {
                object: None,
                identity: source,
                purpose: SourcePurpose::NativeFile,
            },
            output_create: OutputCreate {
                parent_object: None,
                path: output,
                collision_policy: CollisionPolicy::NoReplace,
            },
        }
    }
}
fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_PLAN", message)
}

/// No historical plan is silently upgraded or rehashed.
pub fn digest(plan: &Plan) -> Result<String> {
    match plan.schema_version {
        1 if plan.required_rights.is_none() && plan.creation_audit.is_none() => {
            let mut legacy = plan.clone();
            legacy.hash.clear();
            let bytes =
                serde_json::to_vec(&legacy).map_err(|_| invalid("Cannot encode legacy plan"))?;
            Ok(hex::encode(Sha256::digest(bytes)))
        }
        2 => {
            validate_requirements(plan)?;
            digest_value(serde_json::to_value(plan).map_err(|_| invalid("Cannot encode plan"))?)
        }
        _ => Err(invalid(
            "Unsupported execution plan version; review a fresh plan",
        )),
    }
}
pub fn validate_requirements(plan: &Plan) -> Result<&RequiredRights> {
    if plan.schema_version != 2 {
        return Err(invalid("Review a fresh version 2 execution plan"));
    }
    let rights = plan
        .required_rights
        .as_ref()
        .ok_or_else(|| invalid("Plan has no captured rights"))?;
    let audit = plan
        .creation_audit
        .as_ref()
        .ok_or_else(|| invalid("Plan has no creation audit"))?;
    if rights.source_read.object != plan.source.source_object
        || rights.source_read.identity != plan.source.identity
        || rights.output_create.path != plan.output
        || audit.principal != plan.creator
    {
        return Err(invalid(
            "Captured execution requirements do not match the frozen plan",
        ));
    }
    if let SourcePurpose::CreatorBoundManaged { principal } = &rights.source_read.purpose
        && (principal.is_empty() || principal != &plan.creator)
    {
        return Err(invalid(
            "Managed input purpose must remain bound to its planning principal",
        ));
    }
    Ok(rights)
}
fn digest_value(mut value: Value) -> Result<String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid("Plan must be an object"))?;
    for key in [
        "id",
        "hash",
        "creator",
        "created_at",
        "expires_at",
        "creation_audit",
    ] {
        object.remove(key);
    }
    let source = object
        .get_mut("source")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("Plan source must be an object"))?;
    for key in ["id", "creator", "display_name"] {
        source.remove(key);
    }
    let bytes =
        serde_json::to_vec(&ordered(value)).map_err(|_| invalid("Cannot encode semantic plan"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}
fn ordered(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let pairs: std::collections::BTreeMap<_, _> =
                object.into_iter().map(|(k, v)| (k, ordered(v))).collect();
            Value::Object(pairs.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(ordered).collect()),
        scalar => scalar,
    }
}
/// Verify the stored raw semantic payload before typed decoding can discard fields.
pub fn decode(value: Value) -> Result<Plan> {
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if version != 1 && version != 2 {
        return Err(invalid("Unsupported stored execution plan version"));
    }
    let raw_digest = if version == 2 {
        Some(digest_value(value.clone())?)
    } else {
        None
    };
    if let Some(raw) = &raw_digest
        && value.get("hash").and_then(Value::as_str) != Some(raw.as_str())
    {
        return Err(invalid("Stored raw semantic plan hash does not match"));
    }
    let plan: Plan = serde_json::from_value(value)
        .map_err(|_| invalid("Stored execution plan shape is invalid"))?;
    if let Some(raw) = raw_digest
        && digest(&plan)? != raw
    {
        return Err(invalid(
            "Stored semantic fields cannot be represented without loss",
        ));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mux::{CleanAction, CleanResolution, Preservation},
        records::{Asset, MediaProbe},
    };
    fn fixture() -> Plan {
        let identity = FileIdentity {
            path: "/media/source.mkv".into(),
            bytes: "17".into(),
            sha256: "ab".repeat(32),
        };
        let output = "/exports/output.mkv".to_owned();
        let mut plan = Plan {
            schema_version: 2,
            required_rights: Some(RequiredRights::native(identity.clone(), output.clone())),
            creation_audit: Some(CreationAudit {
                principal: "alice".into(),
                policy_revision: 7,
            }),
            id: "first-plan".into(),
            creator: "alice".into(),
            hash: String::new(),
            source: Asset {
                source_object: None,
                id: "first-asset".into(),
                creator: "alice".into(),
                display_name: "A label".into(),
                identity: identity.clone(),
                probe: MediaProbe {
                    streams: vec![],
                    stream_details: Default::default(),
                    format: "matroska".into(),
                    global_tags: Default::default(),
                    chapters: serde_json::json!([]),
                    verification_scope: "bounded probe".into(),
                },
            },
            output,
            ffmpeg: identity.clone(),
            ffprobe: identity,
            resolution: CleanResolution {
                action: CleanAction::CopyUnchanged,
                streams: vec![],
                outcomes: vec![],
                reasons: vec![],
                warnings: vec![],
                preservation: Preservation::default(),
            },
            argv_template: vec![],
            created_at: "100".into(),
            expires_at: "1900".into(),
            verification_scope: "full byte identity".into(),
        };
        plan.hash = digest(&plan).unwrap();
        plan
    }
    #[test]
    fn equivalent_native_requirements_have_one_hash_across_creators_and_reviews() {
        let first = fixture();
        let mut second = first.clone();
        second.id = "another-review".into();
        second.creator = "bob".into();
        second.source.id = "another-registration".into();
        second.source.creator = "bob".into();
        second.source.display_name = "Different label".into();
        second.created_at = "200".into();
        second.expires_at = "2000".into();
        second.creation_audit = Some(CreationAudit {
            principal: "bob".into(),
            policy_revision: 99,
        });
        assert_eq!(digest(&first).unwrap(), digest(&second).unwrap());
        assert_eq!(
            decode(serde_json::to_value(second).unwrap()).unwrap().hash,
            first.hash
        );
    }
    #[test]
    fn media_and_required_purposes_are_semantic_but_policy_revisions_are_not() {
        let first = fixture();
        let mut changed = first.clone();
        changed.required_rights.as_mut().unwrap().data_root = "other-root".into();
        assert_ne!(digest(&changed).unwrap(), first.hash);
        changed = first.clone();
        changed
            .required_rights
            .as_mut()
            .unwrap()
            .source_read
            .purpose = SourcePurpose::CreatorBoundManaged {
            principal: "alice".into(),
        };
        assert_ne!(digest(&changed).unwrap(), first.hash);
        changed = first.clone();
        changed.output = "/exports/other.mkv".into();
        assert!(digest(&changed).is_err());
        changed.required_rights.as_mut().unwrap().output_create.path = changed.output.clone();
        assert_ne!(digest(&changed).unwrap(), first.hash);
        changed = first.clone();
        changed.source.identity.sha256 = "cd".repeat(32);
        assert!(digest(&changed).is_err());
        changed
            .required_rights
            .as_mut()
            .unwrap()
            .source_read
            .identity = changed.source.identity.clone();
        assert_ne!(digest(&changed).unwrap(), first.hash);
    }
    #[test]
    fn original_native_object_is_a_frozen_semantic_requirement() {
        let mut plan = fixture();
        let before = plan.hash.clone();
        plan.source.source_object = Some(crate::records::NativeObjectIdentity {
            schema_version: 1,
            kind: crate::records::NativeObjectKind::UnixDeviceInode,
            volume: "10".into(),
            object: "20".into(),
            created_ns: Some("30".into()),
        });
        assert!(digest(&plan).is_err());
        plan.required_rights.as_mut().unwrap().source_read.object =
            plan.source.source_object.clone();
        plan.hash = digest(&plan).unwrap();
        assert_ne!(plan.hash, before);
        assert!(decode(serde_json::to_value(&plan).unwrap()).is_ok());
        let mut raw = serde_json::to_value(&plan).unwrap();
        raw["source"]["source_object"]["future_generation"] = 7.into();
        raw["hash"] = digest_value(raw.clone()).unwrap().into();
        assert!(decode(raw).is_err());
    }
    #[test]
    fn reviewed_output_parent_is_semantic_and_missing_historical_fields_stay_missing() {
        let mut plan = fixture();
        let before = plan.hash.clone();
        let raw = serde_json::to_value(&plan).unwrap();
        assert!(
            raw["required_rights"]["output_create"]
                .get("parent_object")
                .is_none()
        );
        plan.required_rights
            .as_mut()
            .unwrap()
            .output_create
            .parent_object = Some(crate::records::NativeObjectIdentity {
            schema_version: 1,
            kind: crate::records::NativeObjectKind::UnixDeviceInode,
            volume: "10".into(),
            object: "40".into(),
            created_ns: Some("50".into()),
        });
        plan.hash = digest(&plan).unwrap();
        assert_ne!(plan.hash, before);
        let mut raw = serde_json::to_value(&plan).unwrap();
        raw["required_rights"]["output_create"]["parent_object"]["future_rules"] = true.into();
        raw["hash"] = digest_value(raw.clone()).unwrap().into();
        assert!(decode(raw).is_err());
    }
    #[test]
    fn raw_unknown_semantics_cannot_disappear_during_typed_decoding() {
        let plan = fixture();
        let mut raw = serde_json::to_value(&plan).unwrap();
        raw["future_processing"] = serde_json::json!({"enabled":true});
        assert_eq!(decode(raw.clone()).unwrap_err().code, "INVALID_PLAN");
        raw["hash"] = digest_value(raw.clone()).unwrap().into();
        assert_eq!(decode(raw).unwrap_err().code, "INVALID_PLAN");
        let mut raw = serde_json::to_value(&plan).unwrap();
        raw["source"]["probe"]["future_clock"] = "different".into();
        raw["hash"] = digest_value(raw.clone()).unwrap().into();
        assert!(decode(raw).is_err());
    }
    #[test]
    fn legacy_hash_and_serialized_shape_are_not_upgraded() {
        let mut plan = fixture();
        plan.schema_version = 1;
        plan.required_rights = None;
        plan.creation_audit = None;
        plan.hash = digest(&plan).unwrap();
        let raw = serde_json::to_value(&plan).unwrap();
        assert!(raw.get("schema_version").is_none());
        assert!(raw.get("required_rights").is_none());
        assert!(raw["source"].get("source_object").is_none());
        let legacy = decode(raw).unwrap();
        assert_eq!(legacy.schema_version, 1);
        assert_eq!(digest(&legacy).unwrap(), plan.hash);
        assert!(validate_requirements(&legacy).is_err());
        plan.schema_version = 3;
        assert!(digest(&plan).is_err());
    }
}
