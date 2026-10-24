//! Stable principals and explicit command, object and path rights.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessProfile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub principal: String,
    pub revision: u64,
    pub enabled: bool,
    #[serde(default)]
    pub client_sessions: bool,
    pub mode: ProfileMode,
    pub owned_object_access: bool,
    pub allowed_command_ids: BTreeSet<String>,
    pub object_grants: Vec<ObjectGrant>,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub name: String,
    pub expected_revision: Option<u64>,
    pub enabled: bool,
    #[serde(default)]
    pub client_sessions: bool,
    pub mode: ProfileMode,
    pub owned_object_access: bool,
    pub allowed_command_ids: BTreeSet<String>,
    pub object_grants: Vec<ObjectGrant>,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileMode { Discovery, Operate }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind { Project, Plan, Job }
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectAction { Read, Edit, Submit, Control }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectGrant {
    pub kind: ObjectKind,
    pub id: String,
    pub actions: BTreeSet<ObjectAction>,
}
pub const DISCOVERY_COMMANDS: &[&str] = &["commands.list", "commands.schema", "access.replay_scope.create"];
impl AccessProfile {
    pub fn allows_command(&self, id: &str) -> bool {
        self.enabled && self.allowed_command_ids.contains(id)
            && (self.mode == ProfileMode::Operate || DISCOVERY_COMMANDS.contains(&id))
    }
    pub fn allows_asset(&self, creator: &str) -> bool {
        self.enabled && self.mode == ProfileMode::Operate && self.owned_object_access && self.principal == creator
    }
    pub fn allows_object(&self, kind: ObjectKind, id: &str, creator: &str, action: ObjectAction) -> bool {
        self.enabled && self.mode == ProfileMode::Operate &&
            ((self.owned_object_access && self.principal == creator)
                || self.object_grants.iter().any(|grant| grant.kind == kind && grant.id == id && grant.actions.contains(&action)))
    }
}
impl ProfileInput {
    pub fn validate(&self) -> crate::Result<()> {
        use crate::{DomainError, commands};
        let invalid = || DomainError::new("VALIDATION_ERROR", "Profile grants are invalid, excessive or outside the declared operational policy");
        if self.allowed_command_ids.len() > 1024 || self.object_grants.len() > 256
            || self.read_roots.len() > 32 || self.write_roots.len() > 32
            || self.allowed_command_ids.iter().any(|id| commands::descriptor(id).is_none() || id.starts_with("access.profiles") || id == "access.credentials.issue") {
            return Err(invalid());
        }
        if self.mode == ProfileMode::Discovery && (self.owned_object_access || self.client_sessions
            || !self.object_grants.is_empty() || !self.read_roots.is_empty() || !self.write_roots.is_empty()
            || self.allowed_command_ids.iter().any(|id| !DISCOVERY_COMMANDS.contains(&id.as_str()))) { return Err(invalid()); }
        let mut identities = BTreeSet::new();
        for grant in &self.object_grants {
            if grant.id.is_empty() || grant.id.len() > 128 || !identities.insert(format!("{:?}:{}", grant.kind, grant.id))
                || grant.actions.is_empty() || !grant.actions.contains(&ObjectAction::Read)
                || grant.actions.iter().any(|action| !matches!((grant.kind, action),
                    (_, ObjectAction::Read) | (ObjectKind::Project, ObjectAction::Edit) | (ObjectKind::Plan, ObjectAction::Submit) | (ObjectKind::Job, ObjectAction::Control))) { return Err(invalid()); }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialInput {
    pub profile: String,
    pub port: Option<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAuthorization {
    pub principal: String,
    pub profile: Option<String>,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Publication {
    pub job_id: String,
    pub proof: String,
    pub output: String,
    pub identity: crate::records::FileIdentity,
    pub verified: String,
    pub published: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_grants_do_not_confer_other_actions_or_inherited_creator_access() {
        let mut profile = AccessProfile {
            schema_version: 2, id: "operator".into(), name: "operator".into(), principal: "reader".into(), revision: 1,
            enabled: true, client_sessions: false, mode: ProfileMode::Operate, owned_object_access: false,
            allowed_command_ids: ["jobs.inspect".into()].into(), object_grants: vec![ObjectGrant { kind: ObjectKind::Job, id: "delegated".into(), actions: [ObjectAction::Read].into() }],
            read_roots: Vec::new(), write_roots: Vec::new(),
        };
        assert!(profile.allows_object(ObjectKind::Job, "delegated", "other", ObjectAction::Read));
        assert!(!profile.allows_object(ObjectKind::Job, "delegated", "other", ObjectAction::Control));
        assert!(!profile.allows_object(ObjectKind::Plan, "delegated", "other", ObjectAction::Read));
        assert!(!profile.allows_object(ObjectKind::Job, "own", "reader", ObjectAction::Read));
        assert!(!profile.allows_asset("reader"));
        assert!(!profile.allows_command("jobs.cancel"));
        profile.owned_object_access = true;
        assert!(profile.allows_asset("reader"));
        assert!(profile.allows_object(ObjectKind::Job, "own", "reader", ObjectAction::Control));
        profile.mode = ProfileMode::Discovery;
        assert!(!profile.allows_object(ObjectKind::Job, "delegated", "other", ObjectAction::Read));
        assert!(!profile.allows_command("jobs.inspect"));
    }
    #[test]
    fn write_actions_require_read_and_must_match_the_resource_kind() {
        let mut input = ProfileInput { name: "unit".into(), expected_revision: None, enabled: true, client_sessions: false,
            mode: ProfileMode::Operate, owned_object_access: true, allowed_command_ids: ["plans.inspect".into()].into(),
            object_grants: vec![ObjectGrant { kind: ObjectKind::Plan, id: "one".into(), actions: [ObjectAction::Submit].into() }], read_roots: Vec::new(), write_roots: Vec::new() };
        assert!(input.validate().is_err());
        input.object_grants[0].actions.insert(ObjectAction::Read);
        assert!(input.validate().is_ok());
        input.object_grants[0].actions.insert(ObjectAction::Control);
        assert!(input.validate().is_err());
        input.object_grants.clear();
        input.mode = ProfileMode::Discovery;
        assert!(input.validate().is_err());
    }
}
