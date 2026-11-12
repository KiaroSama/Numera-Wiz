//! Finite observations describe checks still required; they never authorize a request.
use super::Revision;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityState {
    Unavailable,
    RequiresRequestValidation,
}
impl AvailabilityState {
    pub fn for_reasons(reasons: &[BlockReason]) -> Self {
        if reasons.is_empty() {
            Self::RequiresRequestValidation
        } else {
            Self::Unavailable
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    CommandNotAllowed,
    ProjectReadNotGranted,
    ProjectEditNotGranted,
    UnsupportedProjectSchema,
    WriterObservedUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestGate {
    CurrentPolicy,
    ProjectContext,
    ProjectRights,
    RequestParameters,
    ExpectedRevision,
    WriterLease,
    AffectedScope,
    HistoryState,
    SourceObjectRights,
    SourcePathAndIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAction {
    Read,
    Edit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationRequirements {
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    pub command_id: String,
    pub project_action: Option<ProjectAction>,
    #[schemars(length(max = 10), extend("uniqueItems" = true))]
    pub request_gates: Vec<RequestGate>,
}

pub const PROJECT_TARGET_COMMANDS: [&str; 11] = [
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

/// The shared table contains static requirements only, independent of any caller.
pub fn requirements(command: &str) -> Option<AuthorizationRequirements> {
    let project_action = match command {
        "projects.create" | "projects.list" => None,
        "projects.inspect" | "sequences.inspect" => Some(ProjectAction::Read),
        "sequences.create" | "clips.place" | "clips.move" | "clips.link" | "clips.unlink"
        | "clips.split" | "clips.remove" | "history.undo" | "history.redo" => {
            Some(ProjectAction::Edit)
        }
        _ => return None,
    };
    let write = project_action == Some(ProjectAction::Edit);
    let mut request_gates = vec![RequestGate::CurrentPolicy];
    if project_action.is_some() {
        request_gates.push(RequestGate::ProjectContext);
    }
    if project_action.is_some() || command == "projects.list" {
        request_gates.push(RequestGate::ProjectRights);
    }
    request_gates.push(RequestGate::RequestParameters);
    if write {
        request_gates.push(RequestGate::ExpectedRevision);
    }
    if write || command == "projects.create" {
        request_gates.push(RequestGate::WriterLease);
    }
    if write {
        request_gates.push(RequestGate::AffectedScope);
    }
    if matches!(command, "history.undo" | "history.redo") {
        request_gates.push(RequestGate::HistoryState);
    }
    if command == "clips.place" {
        request_gates.extend([RequestGate::SourceObjectRights, RequestGate::SourcePathAndIdentity]);
    }
    Some(AuthorizationRequirements {
        schema_version: 1,
        command_id: command.to_owned(),
        project_action,
        request_gates,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurrentRight {
    pub state: AvailabilityState,
    #[schemars(length(max = 1), extend("uniqueItems" = true))]
    pub reasons: Vec<BlockReason>,
}
impl CurrentRight {
    pub fn observed(granted: bool, reason: BlockReason) -> Self {
        let reasons = if granted { Vec::new() } else { vec![reason] };
        Self { state: AvailabilityState::for_reasons(&reasons), reasons }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionAvailability {
    pub command_id: String,
    pub state: AvailabilityState,
    #[schemars(length(max = 5), extend("uniqueItems" = true))]
    pub reasons: Vec<BlockReason>,
    #[schemars(length(max = 10), extend("uniqueItems" = true))]
    pub unchecked_gates: Vec<RequestGate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectAuthorization {
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    pub policy_revision: String,
    pub observed_project_revision: String,
    pub current_read: CurrentRight,
    pub current_edit: CurrentRight,
    #[schemars(length(min = 11, max = 11))]
    pub actions: Vec<ActionAvailability>,
}

fn ordered<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

impl ProjectAuthorization {
    /// Unknown or malformed metadata remains unknown, including all newer markers.
    pub fn from_observation(value: Option<&Value>) -> Option<Self> {
        let value = value?;
        if value.get("schema_version")?.as_u64()? != 1
            || value.as_object()?.len() != 6
            || value.get("actions")?.as_array()?.len() != PROJECT_TARGET_COMMANDS.len()
        {
            return None;
        }
        for key in ["policy_revision", "observed_project_revision"] {
            let text = value.get(key)?.as_str()?;
            if text.len() > 20 || Revision::parse(text).is_err() {
                return None;
            }
        }
        for key in ["current_read", "current_edit"] {
            let right = value.get(key)?;
            if right.as_object()?.len() != 2 || right.get("reasons")?.as_array()?.len() > 1
                || !matches!(right.get("state")?.as_str()?, "unavailable" | "requires_request_validation")
                || right.get("reasons")?.as_array()?.iter().any(|reason| reason.as_str().is_none_or(|text| text.len() > 32))
            {
                return None;
            }
        }
        for action in value.get("actions")?.as_array()? {
            if action.as_object()?.len() != 4
                || action.get("command_id")?.as_str()?.len() > 32
                || action.get("reasons")?.as_array()?.len() > 5
                || action.get("unchecked_gates")?.as_array()?.len() > 10
                || !matches!(action.get("state")?.as_str()?, "unavailable" | "requires_request_validation")
                || action.get("reasons")?.as_array()?.iter().chain(action.get("unchecked_gates")?.as_array()?)
                    .any(|label| label.as_str().is_none_or(|text| text.len() > 32))
            {
                return None;
            }
        }
        let decoded: Self = serde_json::from_value(value.clone()).ok()?;
        for (right, reason) in [
            (&decoded.current_read, BlockReason::ProjectReadNotGranted),
            (&decoded.current_edit, BlockReason::ProjectEditNotGranted),
        ] {
            if (!right.reasons.is_empty() && right.reasons != [reason])
                || right.state != AvailabilityState::for_reasons(&right.reasons)
            {
                return None;
            }
        }
        for (action, command) in decoded.actions.iter().zip(PROJECT_TARGET_COMMANDS) {
            let requirement = requirements(command)?;
            let gates: Vec<_> = requirement.request_gates.into_iter()
                .filter(|gate| *gate != RequestGate::ProjectRights).collect();
            if action.command_id != command
                || action.state != AvailabilityState::for_reasons(&action.reasons)
                || !ordered(&action.reasons)
                || !ordered(&action.unchecked_gates)
                || action.unchecked_gates != gates
            {
                return None;
            }
            let read = requirement.project_action == Some(ProjectAction::Read);
            let denied = if read { &decoded.current_read } else { &decoded.current_edit };
            let reason = if read { BlockReason::ProjectReadNotGranted } else { BlockReason::ProjectEditNotGranted };
            let missing_right = !denied.reasons.is_empty();
            if action.reasons.contains(&reason) != missing_right
                || action.reasons.contains(&if read { BlockReason::ProjectEditNotGranted } else { BlockReason::ProjectReadNotGranted })
                || (read && action.reasons.iter().any(|reason| matches!(reason, BlockReason::UnsupportedProjectSchema | BlockReason::WriterObservedUnavailable)))
            {
                return None;
            }
        }
        Some(decoded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeAuthorization {
    #[schemars(extend("const" = 1))]
    pub schema_version: u32,
    pub policy_revision: String,
    #[schemars(length(min = 13, max = 13))]
    pub actions: Vec<ActionAvailability>,
}
