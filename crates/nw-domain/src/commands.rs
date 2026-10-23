//! Canonical envelopes and headless discovery; no UI registration is required.
use crate::{
    DomainError, Result,
    mux::{CleanIntent, Stream},
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_COMMAND_BYTES: usize = 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandRef {
    pub id: String,
    pub version: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectRef {
    pub id: String,
    pub expected_revision: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub api_version: u32,
    pub request_id: String,
    pub command: CommandRef,
    #[serde(default)]
    pub project: Option<ProjectRef>,
    #[serde(default)]
    pub replay_scope: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    pub params: Value,
}
impl Request {
    pub fn query(id: &str, params: Value) -> Self {
        Self {
            api_version: 1,
            request_id: "local".into(),
            command: CommandRef {
                id: id.into(),
                version: 1,
            },
            project: None,
            replay_scope: None,
            idempotency_key: None,
            params,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.api_version != 1 || self.command.version != 1 {
            return Err(DomainError::new(
                "UNSUPPORTED_VERSION",
                "Only application and command version 1 is admitted",
            ));
        }
        if self.request_id.is_empty() || self.request_id.len() > 128 {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "request_id must contain 1-128 bytes",
            ));
        }
        if self.project.is_some() {
            return Err(DomainError::new(
                "CAPABILITY_UNAVAILABLE",
                "Project commands have not reached their implementation gate",
            ));
        }
        if descriptor(&self.command.id).is_none() {
            return Err(DomainError::new(
                "UNKNOWN_COMMAND",
                "Command is not registered",
            ));
        }
        if !self.params.is_object() {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "params must be an object",
            ));
        }
        Ok(())
    }
    pub fn structural_digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let canonical = serde_json::json!({"api_version":self.api_version,"command":self.command,"project":self.project,"params":self.params});
        hex::encode(Sha256::digest(
            serde_json::to_vec(&canonical_order(canonical)).expect("JSON value serializes"),
        ))
    }
}
fn canonical_order(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut pairs = object.into_iter().collect::<Vec<_>>();
            pairs.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(pairs.into_iter().map(|(key, value)| (key, canonical_order(value))).collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_order).collect()),
        scalar => scalar,
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Response {
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DomainError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
}
impl Response {
    pub fn success(id: &str, result: impl Serialize) -> Self {
        Self {
            request_id: id.into(),
            ok: true,
            result: Some(serde_json::to_value(result).expect("serializable result")),
            error: None,
            receipt_id: None,
        }
    }
    pub fn error(id: &str, error: DomainError) -> Self {
        Self {
            request_id: id.into(),
            ok: false,
            result: None,
            error: Some(error),
            receipt_id: None,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StateClass {
    Query,
    Bootstrap,
    DurableMutation,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Descriptor {
    pub id: String,
    pub version: u32,
    pub label: String,
    pub state_class: StateClass,
    pub qualification: String,
    pub params_schema: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathInput {
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdInput {
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveInput {
    pub streams: Vec<Stream>,
    pub intent: CleanIntent,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CleanInput {
    pub asset_id: String,
    pub output: String,
    pub intent: CleanIntent,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitInput {
    pub plan_id: String,
    pub plan_hash: String,
}
pub fn registry() -> Vec<Descriptor> {
    macro_rules! row {
        ($id:literal,$label:literal,$class:ident,$params:ty) => {
            Descriptor {
                id: $id.into(),
                version: 1,
                label: $label.into(),
                state_class: StateClass::$class,
                qualification: "development_only; five-surface/package qualification pending"
                    .into(),
                params_schema: serde_json::to_value(schema_for!($params)).expect("schema"),
            }
        };
    }
    vec![
        row!(
            "access.profiles.put",
            "Create or revise an explicit access profile",
            DurableMutation,
            crate::access::ProfileInput
        ),
        row!(
            "access.profiles.list",
            "Inspect configured access profiles",
            Query,
            Empty
        ),
        row!(
            "access.credentials.issue",
            "Issue a scoped credential",
            Bootstrap,
            crate::access::CredentialInput
        ),
        row!(
            "assets.list",
            "List authorized registered assets",
            Query,
            Empty
        ),
        row!("jobs.list", "List authorized retained jobs", Query, Empty),
        row!(
            "commands.list",
            "Discover implemented development commands",
            Query,
            Empty
        ),
        row!(
            "commands.schema",
            "Inspect a command parameter schema",
            Query,
            IdInput
        ),
        row!(
            "access.replay_scope.create",
            "Create own bounded replay scope",
            Bootstrap,
            Empty
        ),
        row!(
            "media.probe",
            "Probe an explicitly authorized native input",
            Query,
            PathInput
        ),
        row!(
            "assets.register",
            "Register a source identity",
            DurableMutation,
            PathInput
        ),
        row!(
            "streams.clean.resolve",
            "Resolve MuxCls selection without I/O",
            Query,
            ResolveInput
        ),
        row!(
            "streams.clean",
            "Plan cleanup of a registered asset",
            Query,
            CleanInput
        ),
        row!(
            "plans.inspect",
            "Inspect a frozen cleanup plan",
            Query,
            IdInput
        ),
        row!(
            "jobs.submit",
            "Accept the reviewed plan exactly once",
            DurableMutation,
            SubmitInput
        ),
        row!(
            "jobs.inspect",
            "Inspect durable execution and verification facts",
            Query,
            IdInput
        ),
        row!(
            "jobs.cancel",
            "Cancel an accepted job before publication",
            DurableMutation,
            IdInput
        ),
    ]
}
pub fn descriptor(id: &str) -> Option<Descriptor> {
    registry().into_iter().find(|d| d.id == id)
}
pub fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| DomainError::new("VALIDATION_ERROR", e.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digest_excludes_correlation_but_not_semantics() {
        let a = Request::query("media.probe", serde_json::json!({"path":"a"}));
        let mut b = a.clone();
        b.request_id = "different".into();
        assert_eq!(a.structural_digest(), b.structural_digest());
        b.params["path"] = Value::String("b".into());
        assert_ne!(a.structural_digest(), b.structural_digest());
    }
    #[test]
    fn duplicate_context_is_rejected() {
        assert!(parse::<PathInput>(serde_json::json!({"path":"a","project_id":"wrong"})).is_err());
    }
}

/// Parse bounded JSON without silently accepting duplicate object members.
pub fn decode_json(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > MAX_COMMAND_BYTES { return Err(DomainError::new("RESOURCE_LIMIT", "JSON command exceeds 1 MiB")); }
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for byte in bytes {
        if quoted {
            if escaped { escaped = false; }
            else if *byte == b'\\' { escaped = true; }
            else if *byte == b'"' { quoted = false; }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => { depth += 1; if depth > 64 { return Err(DomainError::new("RESOURCE_LIMIT", "JSON nesting exceeds 64 levels")); } }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {},
            }
        }
    }

    if bytes.len() > 256 * 1024 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "JSON request exceeds 256 KiB",
        ));
    }
    #[derive(Debug)]
    struct Strict(Value);
    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Strict;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("JSON with unique object members")
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Bool(v)))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                    serde_json::Number::from_f64(v)
                        .map(|n| Strict(Value::Number(n)))
                        .ok_or_else(|| E::custom("Nonfinite JSON number"))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::String(v.into())))
                }
                fn visit_string<E: serde::de::Error>(
                    self,
                    v: String,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::String(v)))
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut values = Vec::new();
                    while let Some(Strict(value)) = seq.next_element()? {
                        values.push(value);
                    }
                    Ok(Strict(Value::Array(values)))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut values = serde_json::Map::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if values.contains_key(&key) {
                            return Err(serde::de::Error::custom("Duplicate JSON object member"));
                        }
                        let Strict(value) = map.next_value()?;
                        values.insert(key, value);
                    }
                    Ok(Strict(Value::Object(values)))
                }
            }
            deserializer.deserialize_any(Visitor)
        }
    }
    serde_json::from_slice::<Strict>(bytes)
        .map(|v| v.0)
        .map_err(|e| DomainError::new("VALIDATION_ERROR", e.to_string()))
}
pub fn decode_request(bytes: &[u8]) -> Result<Request> {
    parse(decode_json(bytes)?)
}
#[cfg(test)]
mod strict_json_tests {
    #[test]
    fn structural_depth_budget_ignores_brackets_in_strings() {
        let too_deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
        assert_eq!(super::decode_json(too_deep.as_bytes()).unwrap_err().code, "RESOURCE_LIMIT");
        let quoted = serde_json::to_vec(&serde_json::json!({"literal":"[".repeat(100)})).unwrap();
        assert!(super::decode_json(&quoted).is_ok());
    }
    #[test]
    fn duplicate_nested_parameters_are_not_last_writer_wins() {
        assert!(super::decode_json(br#"{"params":{"index":0,"index":1}}"#).is_err());
        assert!(super::decode_json(br#"{"params":{"index":0}}"#).is_ok());
    }
}

/// The single active-handler set; planned DTOs remain separate from offered tools.
pub const ACTIVE_COMMANDS: &[&str] = &[
    "commands.list",
    "commands.schema",
    "streams.clean.resolve",
    "access.replay_scope.create",
    "access.profiles.put",
    "access.profiles.list",
    "access.credentials.issue",
    "media.probe",
    "assets.register",
    "assets.list",
    "streams.clean",
    "plans.inspect",
    "jobs.submit",
    "jobs.inspect",
    "jobs.list",
    "jobs.cancel",
];
pub fn active_registry() -> Vec<Descriptor> {
    registry()
        .into_iter()
        .filter(|descriptor| ACTIVE_COMMANDS.contains(&descriptor.id.as_str()))
        .collect()
}
