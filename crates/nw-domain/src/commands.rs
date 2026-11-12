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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "project_revision_field"
    )]
    pub expected_revision: Option<String>,
}
fn project_revision_field<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
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
        let descriptor = descriptor(&self.command.id)
            .ok_or_else(|| DomainError::new("UNKNOWN_COMMAND", "Command is not registered"))?;
        match (descriptor.project_context, &self.project) {
            (ProjectContextRule::Forbidden, None) => {}
            (ProjectContextRule::RequiredRead, Some(project))
                if project.expected_revision.is_none() =>
            {
                crate::projects::validate_id(&project.id)?;
            }
            (ProjectContextRule::RequiredWrite, Some(project)) => {
                crate::projects::validate_id(&project.id)?;
                crate::projects::Revision::parse(
                    project.expected_revision.as_deref().ok_or_else(|| {
                        DomainError::new(
                            "VALIDATION_ERROR",
                            "Project writes require expected_revision",
                        )
                    })?,
                )?;
            }
            _ => {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "Project context does not match this command's read/write contract",
                ));
            }
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
            Value::Object(
                pairs
                    .into_iter()
                    .map(|(key, value)| (key, canonical_order(value)))
                    .collect(),
            )
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
    pub fn bounded(self) -> Self {
        if serde_json::to_vec(&self).is_ok_and(|bytes| bytes.len() <= MAX_RESULT_BYTES) {
            self
        } else {
            Self::error(
                &self.request_id,
                DomainError::new(
                    "RESOURCE_LIMIT",
                    "Result exceeds the bounded projection; request a narrower page",
                ),
            )
        }
    }

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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectContextRule {
    #[default]
    Forbidden,
    RequiredRead,
    RequiredWrite,
}
impl ProjectContextRule {
    fn forbidden(&self) -> bool {
        *self == Self::Forbidden
    }
}
fn project_context(id: &str) -> ProjectContextRule {
    match id {
        "projects.inspect" | "sequences.inspect" => ProjectContextRule::RequiredRead,
        "sequences.create" | "clips.place" | "clips.move" | "clips.link" | "clips.unlink"
        | "clips.split" | "clips.remove" | "history.undo" | "history.redo" => {
            ProjectContextRule::RequiredWrite
        }
        _ => ProjectContextRule::Forbidden,
    }
}
fn descriptor_schema_version_default() -> u32 {
    1
}
fn result_schema(id: &str) -> Option<Value> {
    use crate::projects::contracts::{self, EditResult};
    match id {
        "projects.create" => Some(
            serde_json::to_value(schema_for!(crate::projects::views::ProjectCreateResult))
                .expect("schema"),
        ),
        "projects.list" => Some(contracts::catalog()),
        "projects.inspect" => Some(contracts::inspect(false)),
        "sequences.inspect" => Some(contracts::inspect(true)),
        "sequences.create" => Some(contracts::edit(EditResult::Sequence)),
        "clips.place" => Some(contracts::edit(EditResult::Place)),
        "clips.move" => Some(contracts::edit(EditResult::Move)),
        "clips.link" => Some(contracts::edit(EditResult::Link)),
        "clips.unlink" => Some(contracts::edit(EditResult::Unlink)),
        "clips.split" => Some(contracts::edit(EditResult::Split)),
        "clips.remove" => Some(contracts::edit(EditResult::Remove)),
        "history.undo" | "history.redo" => Some(contracts::edit(EditResult::History)),
        _ => None,
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Descriptor {
    #[serde(default = "descriptor_schema_version_default")]
    pub descriptor_schema_version: u32,
    pub id: String,
    pub version: u32,
    pub label: String,
    pub state_class: StateClass,
    pub qualification: String,
    pub params_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_requirements: Option<crate::projects::authorization::AuthorizationRequirements>,
    #[serde(default, skip_serializing_if = "ProjectContextRule::forbidden")]
    pub project_context: ProjectContextRule,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_timeout: Option<crate::execution::DeadlinePolicy>,
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
    #[serde(default)]
    pub execution_limits: crate::execution::CleanupLimits,
    #[serde(default)]
    pub file_times: crate::file_times::FileTimeRequest,
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
                descriptor_schema_version: 2,
                id: $id.into(),
                version: 1,
                label: $label.into(),
                state_class: StateClass::$class,
                qualification: "development_only; five-surface/package qualification pending"
                    .into(),
                params_schema: crate::projects::contracts::input_schema(
                    $id, serde_json::to_value(schema_for!($params)).expect("schema"),
                ),
                result_schema: result_schema($id),
                authorization_requirements: crate::projects::authorization::requirements($id),
                project_context: project_context($id),
            }
        };
    }
    vec![
        row!(
            "projects.list",
            "List readable project headers in ascending ID order",
            Query,
            crate::views::PageInput
        ),
        row!(
            "projects.create",
            "Create a durable project",
            DurableMutation,
            crate::projects::CreateInput
        ),
        row!(
            "projects.inspect",
            "Read a bounded captured project projection",
            Query,
            crate::projects::views::InspectInput
        ),
        row!(
            "sequences.inspect",
            "Read captured sequence tracks or intervals",
            Query,
            crate::projects::views::SequenceInspectInput
        ),
        row!(
            "sequences.create",
            "Create a sequence and explicit tracks",
            DurableMutation,
            crate::projects::SequenceInput
        ),
        row!(
            "clips.place",
            "Place a finite generator or authorized registered stream",
            DurableMutation,
            crate::projects::PlaceInput
        ),
        row!(
            "clips.move",
            "Move the complete linked clip selection",
            DurableMutation,
            crate::projects::MoveInput
        ),
        row!(
            "clips.link",
            "Link complete same-sequence clip components without changing geometry",
            DurableMutation,
            crate::projects::LinkInput
        ),
        row!(
            "clips.unlink",
            "Unlink complete same-sequence clip components without changing geometry",
            DurableMutation,
            crate::projects::UnlinkInput
        ),
        row!(
            "clips.split",
            "Split exact source and sequence intervals",
            DurableMutation,
            crate::projects::SplitInput
        ),
        row!(
            "clips.remove",
            "Lift the linked clip selection",
            DurableMutation,
            crate::projects::RemoveInput
        ),
        row!(
            "history.undo",
            "Compensate the latest retained project edit",
            DurableMutation,
            Empty
        ),
        row!(
            "history.redo",
            "Reapply a retained project edit",
            DurableMutation,
            Empty
        ),
        row!(
            "maintenance.plan",
            "Review an idle schema2 generation migration",
            Query,
            Empty
        ),
        row!(
            "maintenance.migrate",
            "Accept a reviewed generation migration exactly once",
            DurableMutation,
            crate::maintenance::MigrationSubmitInput
        ),
        row!(
            "maintenance.inspect",
            "Inspect an accepted owner migration",
            Query,
            IdInput
        ),
        row!(
            "streams.clean.batch_plan",
            "Review every explicit cleanup batch candidate",
            Query,
            crate::batches::CleanBatchInput
        ),
        row!(
            "batches.plan_inspect",
            "Inspect an authorized frozen cleanup batch",
            Query,
            IdInput
        ),
        row!(
            "batches.submit",
            "Atomically accept reviewed child jobs with one receipt",
            DurableMutation,
            crate::batches::SubmitBatchInput
        ),
        row!(
            "batches.list",
            "List authorized accepted batch identities",
            Query,
            crate::views::PageInput
        ),
        row!(
            "batches.inspect",
            "Inspect current authorized batch member outcomes",
            Query,
            IdInput
        ),
        row!(
            "batches.cancel",
            "Journal cancellation requests for controllable batch members",
            DurableMutation,
            IdInput
        ),
        row!(
            "artifacts.list",
            "List current job-readable artifact manifests",
            Query,
            crate::views::PageInput
        ),
        row!(
            "artifacts.manifest",
            "Inspect an authorized published artifact",
            Query,
            crate::artifacts::ManifestInput
        ),
        row!(
            "artifacts.read_chunk",
            "Read a bounded verified artifact byte range",
            Query,
            crate::artifacts::ChunkInput
        ),
        row!(
            "logs.status",
            "Inspect bounded protected owner logging status",
            Query,
            Empty
        ),
        row!(
            "access.profiles.create",
            "Create a named operational access profile",
            DurableMutation,
            crate::access::ProfileCreateInput
        ),
        row!(
            "access.profiles.update",
            "Replace an access policy at an exact revision",
            DurableMutation,
            crate::access::ProfileInput
        ),
        row!(
            "access.profiles.inspect",
            "Inspect one explicit access policy",
            Query,
            crate::access::ProfileNameInput
        ),
        row!(
            "access.tokens.issue",
            "Issue an audience-bound credential for private file delivery",
            Bootstrap,
            crate::access::TokenIssueInput
        ),
        row!(
            "access.tokens.revoke",
            "Revoke one credential without changing principal identity",
            DurableMutation,
            crate::access::TokenRevokeInput
        ),
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
            "Page authorized asset summaries in a bounded snapshot",
            Query,
            crate::views::PageInput
        ),
        row!(
            "assets.inspect",
            "Inspect one authorized registered asset",
            Query,
            IdInput
        ),
        row!(
            "assets.output_suggest",
            "Suggest an authorized new cleanup output",
            Query,
            IdInput
        ),
        row!(
            "jobs.list",
            "Page authorized job summaries in a bounded snapshot",
            Query,
            crate::views::PageInput
        ),
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
            "Register a native source or verified intake",
            DurableMutation,
            crate::ingest::RegisterInput
        ),
        row!(
            "assets.register_folder",
            "Register a bounded native folder snapshot with per-file outcomes",
            DurableMutation,
            crate::ingest::RegisterFolderInput
        ),
        row!(
            "assets.folder_inspect",
            "Inspect an immutable creator-scoped folder intake snapshot",
            Query,
            IdInput
        ),
        row!(
            "streams.clean.resolve",
            "Resolve MuxCls selection without I/O",
            Query,
            ResolveInput
        ),
        row!(
            "streams.clean.report",
            "Report authorized source inventory and cleanup selections",
            Query,
            crate::reports::ReportInput
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
        finite_input_descriptor_vocabularies();
    }

    fn finite_input_descriptor_vocabularies() {
        for id in ["clips.link", "clips.unlink"] {
            let command = finite_input_descriptor(id);
            finite_input_enum(
                finite_input_property(&command.params_schema, "link_policy"),
                &["expand"],
                "R1_FINITE_TYPED_LINK_POLICY_MISSING",
            );
        }

        let project = finite_input_descriptor("projects.inspect");
        finite_input_enum(
            finite_input_property(&project.params_schema, "projection"),
            &["summary", "entity", "bin_page"],
            "R1_FINITE_INSPECT_PROJECTION_MISSING",
        );

        // Later authored cases remain UNEXECUTED until the boundary above passes.
        finite_input_enum(
            finite_input_property(&project.params_schema, "sort"),
            &["label"],
            "R1_FINITE_INSPECT_SORT_MISSING",
        );
        let sequence = finite_input_descriptor("sequences.inspect");
        finite_input_enum(
            finite_input_property(&sequence.params_schema, "projection"),
            &["entity", "track_page", "viewport"],
            "R1_FINITE_SEQUENCE_PROJECTION_MISSING",
        );
        for (id, field, values) in [
            ("clips.place", "collision", &["reject"][..]),
            ("clips.move", "collision", &["reject"][..]),
            ("clips.move", "link_policy", &["expand"][..]),
            ("clips.split", "link_policy", &["expand"][..]),
            ("clips.remove", "mode", &["lift"][..]),
            ("clips.remove", "link_policy", &["expand"][..]),
        ] {
            let command = finite_input_descriptor(id);
            finite_input_enum(
                finite_input_property(&command.params_schema, field),
                values,
                "R1_FINITE_EDIT_VOCABULARY_MISSING",
            );
        }
        // Both projection fields are required Strings without generated defaults.
        assert!(
            finite_input_property(&project.params_schema, "projection")
                .get("default")
                .is_none(),
            "R1_FINITE_INSPECT_PROJECTION_DEFAULT_ADDED"
        );
        assert!(
            finite_input_property(&sequence.params_schema, "projection")
                .get("default")
                .is_none(),
            "R1_FINITE_SEQUENCE_PROJECTION_DEFAULT_ADDED"
        );
        assert!(
            finite_input_property(&project.params_schema, "sort")
                .get("default")
                .and_then(Value::as_str)
                == Some("label"),
            "R1_FINITE_INSPECT_SORT_DEFAULT_CHANGED"
        );
    }

    fn finite_input_descriptor(id: &str) -> Descriptor {
        let command = descriptor(id).expect("R1_FINITE_DESCRIPTOR_MISSING");
        assert!(
            !command.id.is_empty() && command.id == id && command.version == 1,
            "R1_FINITE_CANONICAL_COMMAND_CHANGED"
        );
        assert!(
            ACTIVE_COMMANDS.contains(&command.id.as_str()),
            "R1_FINITE_ACTIVE_HANDLER_MISSING"
        );
        command
    }

    fn finite_input_property<'a>(root: &'a Value, field: &str) -> &'a Value {
        let property = finite_input_node(root, root)
            .get("properties")
            .and_then(|properties| properties.get(field))
            .expect("R1_FINITE_PROPERTY_MISSING");
        finite_input_node(root, property)
    }

    fn finite_input_node<'a>(root: &'a Value, mut node: &'a Value) -> &'a Value {
        let mut seen = Vec::new();
        for hop in 0..=16 {
            let Some(reference) = node.get("$ref") else {
                return node;
            };
            let reference = reference.as_str().expect("R1_FINITE_REF_NOT_STRING");
            assert!(
                node.as_object().is_some_and(|object| object.len() == 1),
                "R1_FINITE_REF_NOT_WHOLE_NODE"
            );
            assert!(
                reference.starts_with("#/$defs/"),
                "R1_FINITE_REF_NOT_LOCAL_DEFS"
            );
            assert!(
                hop < 16 && !seen.contains(&reference),
                "R1_FINITE_REF_CYCLIC_OR_DEEP"
            );
            seen.push(reference);
            node = root
                .pointer(&reference[1..])
                .expect("R1_FINITE_REF_TARGET_MISSING");
        }
        unreachable!("R1_FINITE_REF_CYCLIC_OR_DEEP")
    }

    fn finite_input_enum(node: &Value, expected: &[&str], diagnostic: &str) {
        let actual = node
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|values| values.iter().map(Value::as_str).collect::<Option<Vec<_>>>())
            .and_then(|mut values| {
                let count = values.len();
                values.sort_unstable();
                values.dedup();
                (values.len() == count).then_some(values)
            });
        let mut expected = expected.to_vec();
        expected.sort_unstable();
        assert!(
            actual.as_deref() == Some(expected.as_slice()),
            "{diagnostic}"
        );
    }
    #[test]
    fn duplicate_context_is_rejected() {
        assert!(parse::<PathInput>(serde_json::json!({"path":"a","project_id":"wrong"})).is_err());
        use crate::projects::{Bin, EntityEnvelope, EntityId, Revision};
        for revision in ["", "+1", "01", "-1", "18446744073709551616"] {
            assert!(Revision::parse(revision).is_err());
        }
        assert!(serde_json::from_value::<Revision>(serde_json::json!(1)).is_err());
        assert!(EntityId::parse("not-a-server-uuid").is_err());
        let id = "11111111-1111-4111-8111-111111111111";
        let mut read = Request::query(
            "projects.inspect",
            serde_json::json!({"projection":"summary"}),
        );
        read.project = Some(ProjectRef {
            id: id.into(),
            expected_revision: None,
        });
        read.validate().unwrap();
        assert!(
            serde_json::to_value(&read).unwrap()["project"]
                .get("expected_revision")
                .is_none()
        );
        let mut null_revision = serde_json::to_value(&read).unwrap();
        null_revision["project"]["expected_revision"] = Value::Null;
        assert!(serde_json::from_value::<Request>(null_revision).is_err());
        read.project.as_mut().unwrap().expected_revision = Some("0".into());
        assert!(read.validate().is_err());
        read.command.id = "clips.move".into();
        read.validate().unwrap();
        read.command.id = "jobs.list".into();
        assert!(read.validate().is_err());
        read.command.id = "projects.list".into();
        assert!(read.validate().is_err());
        let catalog = Request::query("projects.list", serde_json::json!({"limit":64}));
        catalog.validate().unwrap();
        let descriptor = descriptor("projects.list").unwrap();
        assert_eq!(descriptor.project_context, ProjectContextRule::Forbidden);
        assert!(matches!(descriptor.state_class, StateClass::Query));
        assert!(ACTIVE_COMMANDS.contains(&"projects.list"));
        assert!(parse::<crate::views::PageInput>(serde_json::json!({"project_id":id})).is_err());
        let mut entity = EntityEnvelope::new(
            &crate::projects::ProjectId::parse(id).unwrap(),
            EntityId::parse(id).unwrap(),
            "bin",
            None,
            "Opaque extension".into(),
            &Bin {
                member_count: 0,
                locked: false,
            },
        )
        .unwrap();
        entity.payload_json = r#"{"member_count":0,"locked":false,"future":{"integer":900719925474099312345678901,"decimal":0.123456789012345678901}}"#.into();
        entity.payload_sha256 = crate::projects::hash(entity.payload_json.as_bytes());
        entity
            .update(&Bin {
                member_count: 1,
                locked: false,
            })
            .unwrap();
        assert!(entity.payload_json.contains("900719925474099312345678901"));
        assert!(entity.payload_json.contains("0.123456789012345678901"));
        assert_eq!(entity.payload::<Bin>("bin").unwrap().member_count, 1);
        let original = entity.bytes().unwrap();
        assert!(entity.update(&serde_json::json!({"oversized": "x".repeat(crate::projects::MAX_PAYLOAD_BYTES)})).is_err());
        assert_eq!(
            entity.bytes().unwrap(),
            original,
            "A rejected overlay must retain the exact original envelope"
        );
        // I5: opaque extensions must reject ambiguity before any known-field overlay.
        let mut duplicate_failures = Vec::new();
        for (case, payload) in [
            (
                "duplicate_unknown",
                r#"{"member_count":0,"locked":false,"future":1,"future":2}"#,
            ),
            (
                "nested_object",
                r#"{"member_count":0,"locked":false,"future":{"exact":900719925474099312345678901,"exact":2}}"#,
            ),
            (
                "nested_array",
                r#"{"member_count":0,"locked":false,"future":[{"a":1,"a":2}]}"#,
            ),
            (
                "escaped_duplicate",
                r#"{"member_count":0,"locked":false,"future":{"a":1,"\u0061":2}}"#,
            ),
        ] {
            let mut ambiguous = entity.clone();
            ambiguous.payload_json = payload.into();
            ambiguous.payload_sha256 = crate::projects::hash(payload.as_bytes());
            let original_payload = ambiguous.payload_json.clone();
            let original_hash = ambiguous.payload_sha256.clone();
            let validation = ambiguous.validate();
            let update = ambiguous.update(&Bin {
                member_count: 2,
                locked: false,
            });
            if validation.is_ok()
                || update.is_ok()
                || ambiguous.payload_json != original_payload
                || ambiguous.payload_sha256 != original_hash
            {
                duplicate_failures.push(format!("{case}: validation_rejected={}, update_rejected={}, original_bytes_preserved={}", validation.is_err(), update.is_err(), ambiguous.payload_json == original_payload && ambiguous.payload_sha256 == original_hash));
            }
        }
        assert!(
            duplicate_failures.is_empty(),
            "Opaque duplicate-key preservation failures: {duplicate_failures:#?}"
        );
        // Raw top-level fields must use the JSON reader directly, as payload overlays do.
        use crate::projects::{ProjectHeader, ProjectId};
        use serde_json::value::RawValue;
        use std::collections::BTreeMap;
        let opaque = r#"{ "integer":900719925474099312345678901,"decimal":0.123456789012345678901,"escaped":"\u0061","nested":[{"number":1e+03,"text":"\u0062"}] }"#;
        let empty_envelope = entity.bytes().unwrap();
        let empty_decoded: EntityEnvelope = serde_json::from_str(&empty_envelope).unwrap();
        assert!(empty_decoded.extensions.is_empty());
        assert_eq!(empty_decoded.bytes().unwrap(), empty_envelope);
        let mut raw_entity = entity.clone();
        raw_entity.extensions.insert(
            "future_optional".into(),
            serde_json::from_str(opaque).unwrap(),
        );
        let encoded = raw_entity.bytes().unwrap();
        assert!(encoded.contains(opaque));
        let mut decoded: EntityEnvelope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.extensions["future_optional"].get(), opaque);
        assert_eq!(decoded.bytes().unwrap(), encoded);
        assert_eq!(decoded.id.as_str(), id);
        assert_eq!(decoded.project_id.as_str(), id);
        assert_eq!(decoded.owner_id, None);
        decoded
            .update(&Bin {
                member_count: 3,
                locked: false,
            })
            .unwrap();
        assert_eq!(decoded.extensions["future_optional"].get(), opaque);
        assert_eq!(decoded.payload::<Bin>("bin").unwrap().member_count, 3);
        let fields: BTreeMap<String, Box<RawValue>> = serde_json::from_str(&encoded).unwrap();
        for required in [
            "envelope_version",
            "id",
            "project_id",
            "kind",
            "kind_version",
            "enabled",
            "references",
            "semantic_scope",
            "label",
            "payload_json",
            "payload_sha256",
        ] {
            let mut missing = fields.clone();
            assert!(missing.remove(required).is_some());
            let raw = serde_json::to_string(&missing).unwrap();
            assert!(
                serde_json::from_str::<EntityEnvelope>(&raw).is_err(),
                "Missing required envelope field {required}"
            );
        }
        for (field, invalid) in [
            ("envelope_version", "\"1\""),
            ("id", "1"),
            ("project_id", "false"),
            ("kind", "0"),
            ("kind_version", "null"),
            ("owner_id", "1"),
            ("enabled", "0"),
            ("references", "{}"),
            ("semantic_scope", "false"),
            ("label", "null"),
            ("payload_json", "0"),
            ("payload_sha256", "false"),
        ] {
            let mut malformed = fields.clone();
            malformed.insert(field.into(), serde_json::from_str(invalid).unwrap());
            let raw = serde_json::to_string(&malformed).unwrap();
            assert!(
                serde_json::from_str::<EntityEnvelope>(&raw).is_err(),
                "Malformed envelope field {field}"
            );
        }
        let mut missing_owner = fields.clone();
        missing_owner.remove("owner_id");
        let missing_owner: EntityEnvelope =
            serde_json::from_str(&serde_json::to_string(&missing_owner).unwrap()).unwrap();
        assert_eq!(missing_owner.owner_id, None);
        assert_eq!(missing_owner.extensions["future_optional"].get(), opaque);
        let mut explicit_owner = fields.clone();
        explicit_owner.insert(
            "owner_id".into(),
            serde_json::from_str(&format!("\"{id}\"")).unwrap(),
        );
        let explicit_owner: EntityEnvelope =
            serde_json::from_str(&serde_json::to_string(&explicit_owner).unwrap()).unwrap();
        assert_eq!(
            explicit_owner.owner_id.as_ref().map(EntityId::as_str),
            Some(id)
        );
        for duplicate in [
            format!("\"id\":\"{id}\""),
            "\"owner_id\":null".into(),
            format!("\"future_optional\":{opaque}"),
        ] {
            let raw = format!("{{{duplicate},{}", &encoded[1..]);
            assert!(serde_json::from_str::<EntityEnvelope>(&raw).is_err());
        }
        let mut future_entity = raw_entity.clone();
        future_entity.envelope_version = 2;
        let future_entity: EntityEnvelope =
            serde_json::from_str(&future_entity.bytes().unwrap()).unwrap();
        assert_eq!(future_entity.envelope_version, 2);
        assert_eq!(future_entity.extensions["future_optional"].get(), opaque);
        assert_eq!(
            future_entity.semantics().unwrap_err().code,
            "PROJECT_READ_ONLY"
        );

        let project = ProjectId::parse(id).unwrap();
        let mut header = ProjectHeader {
            envelope_version: 1,
            schema_major: 1,
            id: project.clone(),
            creator: "codec-owner".into(),
            name: "Raw header fields".into(),
            revision: Revision(0),
            root_bin_id: EntityId::parse(id).unwrap(),
            entity_count: 1,
            sequence_count: 0,
            history_root: format!("project-history/{project}"),
            materialized_revision: Revision(0),
            change_digest: String::new(),
            compatibility: "supported".into(),
            extensions: BTreeMap::new(),
        };
        let empty_header = serde_json::to_string(&header).unwrap();
        let empty_decoded: ProjectHeader = serde_json::from_str(&empty_header).unwrap();
        assert!(empty_decoded.extensions.is_empty());
        assert_eq!(serde_json::to_string(&empty_decoded).unwrap(), empty_header);
        header.extensions.insert(
            "future_optional".into(),
            serde_json::from_str(opaque).unwrap(),
        );
        let encoded = serde_json::to_string(&header).unwrap();
        assert!(encoded.contains(opaque));
        let decoded: ProjectHeader = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.extensions["future_optional"].get(), opaque);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);
        assert_eq!(decoded.id, project);
        assert_eq!(decoded.creator, "codec-owner");
        assert_eq!(decoded.root_bin_id.as_str(), id);
        assert_eq!(decoded.revision, Revision(0));
        assert_eq!(decoded.materialized_revision, Revision(0));
        assert_eq!(decoded.entity_count, 1);
        assert_eq!(decoded.sequence_count, 0);
        decoded.writable().unwrap();
        let fields: BTreeMap<String, Box<RawValue>> = serde_json::from_str(&encoded).unwrap();
        for required in [
            "envelope_version",
            "schema_major",
            "id",
            "creator",
            "name",
            "revision",
            "root_bin_id",
            "entity_count",
            "sequence_count",
            "history_root",
            "materialized_revision",
            "change_digest",
            "compatibility",
        ] {
            let mut missing = fields.clone();
            assert!(missing.remove(required).is_some());
            let raw = serde_json::to_string(&missing).unwrap();
            assert!(
                serde_json::from_str::<ProjectHeader>(&raw).is_err(),
                "Missing required header field {required}"
            );
        }
        for (field, invalid) in [
            ("envelope_version", "null"),
            ("schema_major", "\"1\""),
            ("id", "1"),
            ("creator", "false"),
            ("name", "0"),
            ("revision", "0"),
            ("root_bin_id", "false"),
            ("entity_count", "-1"),
            ("sequence_count", "\"0\""),
            ("history_root", "null"),
            ("materialized_revision", "0"),
            ("change_digest", "false"),
            ("compatibility", "[]"),
        ] {
            let mut malformed = fields.clone();
            malformed.insert(field.into(), serde_json::from_str(invalid).unwrap());
            let raw = serde_json::to_string(&malformed).unwrap();
            assert!(
                serde_json::from_str::<ProjectHeader>(&raw).is_err(),
                "Malformed header field {field}"
            );
        }
        for duplicate in [
            format!("\"id\":\"{id}\""),
            format!("\"future_optional\":{opaque}"),
        ] {
            let raw = format!("{{{duplicate},{}", &encoded[1..]);
            assert!(serde_json::from_str::<ProjectHeader>(&raw).is_err());
        }
        for raw in ["[]", "null", "true", "1"] {
            assert!(serde_json::from_str::<EntityEnvelope>(raw).is_err());
            assert!(serde_json::from_str::<ProjectHeader>(raw).is_err());
        }
        for (envelope, schema) in [(2, 1), (1, 2)] {
            let mut future = header.clone();
            future.envelope_version = envelope;
            future.schema_major = schema;
            let raw = serde_json::to_string(&future).unwrap();
            let future: ProjectHeader = serde_json::from_str(&raw).unwrap();
            assert_eq!(future.envelope_version, envelope);
            assert_eq!(future.schema_major, schema);
            assert_eq!(future.extensions["future_optional"].get(), opaque);
            assert_eq!(serde_json::to_string(&future).unwrap(), raw);
            assert_eq!(future.writable().unwrap_err().code, "PROJECT_READ_ONLY");
        }
        let mut inconsistent = header.clone();
        inconsistent.materialized_revision = Revision(1);
        let inconsistent: ProjectHeader =
            serde_json::from_str(&serde_json::to_string(&inconsistent).unwrap()).unwrap();
        assert_eq!(inconsistent.writable().unwrap_err().code, "PROJECT_CORRUPT");
    }
}

/// Parse bounded JSON without silently accepting duplicate object members.
pub fn decode_json(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > MAX_COMMAND_BYTES {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "JSON command exceeds 1 MiB",
        ));
    }
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > 64 {
                        return Err(DomainError::new(
                            "RESOURCE_LIMIT",
                            "JSON nesting exceeds 64 levels",
                        ));
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
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
        assert_eq!(
            super::decode_json(too_deep.as_bytes()).unwrap_err().code,
            "RESOURCE_LIMIT"
        );
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
    "projects.list",
    "projects.create",
    "projects.inspect",
    "sequences.inspect",
    "sequences.create",
    "clips.place",
    "clips.move",
    "clips.link",
    "clips.unlink",
    "clips.split",
    "clips.remove",
    "history.undo",
    "history.redo",
    "maintenance.plan",
    "maintenance.migrate",
    "maintenance.inspect",
    "streams.clean.batch_plan",
    "batches.plan_inspect",
    "batches.submit",
    "batches.list",
    "batches.inspect",
    "batches.cancel",
    "artifacts.list",
    "artifacts.manifest",
    "artifacts.read_chunk",
    "logs.status",
    "commands.list",
    "commands.schema",
    "streams.clean.resolve",
    "streams.clean.report",
    "access.replay_scope.create",
    "access.profiles.put",
    "access.profiles.create",
    "access.profiles.update",
    "access.profiles.inspect",
    "access.tokens.issue",
    "access.tokens.revoke",
    "access.profiles.list",
    "access.credentials.issue",
    "media.probe",
    "assets.register",
    "assets.register_folder",
    "assets.folder_inspect",
    "assets.list",
    "assets.inspect",
    "assets.output_suggest",
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
pub fn registry_fingerprint() -> String {
    use sha2::{Digest, Sha256};
    let value =
        canonical_order(serde_json::to_value(active_registry()).expect("registry serializes"));
    hex::encode(Sha256::digest(
        serde_json::to_vec(&value).expect("registry serializes"),
    ))
}

#[cfg(test)]
mod request_budget_tests {
    use super::*;
    #[test]
    fn strict_request_decoder_admits_the_documented_one_mib_boundary() {
        let input = format!(r#"{{"value":"{}"}}"#, "x".repeat(MAX_COMMAND_BYTES - 12));
        assert_eq!(input.len(), MAX_COMMAND_BYTES);
        assert_eq!(
            decode_json(input.as_bytes()).unwrap()["value"]
                .as_str()
                .unwrap()
                .len(),
            MAX_COMMAND_BYTES - 12
        );
        let mut oversized = input.into_bytes();
        oversized.push(b' ');
        assert_eq!(decode_json(&oversized).unwrap_err().code, "RESOURCE_LIMIT");
    }
}
