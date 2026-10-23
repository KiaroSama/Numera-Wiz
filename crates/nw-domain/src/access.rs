//! Stable principals and explicit command, object and path rights.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessProfile {
    pub id: String,
    pub name: String,
    pub principal: String,
    pub revision: u64,
    pub enabled: bool,
    pub commands: BTreeSet<String>,
    pub objects: BTreeSet<String>,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub name: String,
    pub expected_revision: Option<u64>,
    pub enabled: bool,
    pub commands: BTreeSet<String>,
    pub objects: BTreeSet<String>,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialInput {
    pub profile: String,
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
