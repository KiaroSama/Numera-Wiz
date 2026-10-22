//! Versioned execution facts shared by the store, engine and clients.
use crate::mux::{CleanResolution, Stream};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema,PartialEq,Eq)]
pub struct FileIdentity { pub path:String,pub bytes:String,pub sha256:String }
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
pub struct MediaProbe {
    pub streams:Vec<Stream>,
    pub format:String,
    pub global_tags:BTreeMap<String,String>,
    pub chapters:serde_json::Value,
    pub verification_scope:String,
}
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
pub struct Asset { pub id:String,pub creator:String,pub identity:FileIdentity,pub probe:MediaProbe }
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
pub struct Plan {
    pub id:String,pub creator:String,pub hash:String,pub source:Asset,
    pub output:String,pub ffmpeg:FileIdentity,pub ffprobe:FileIdentity,
    pub resolution:CleanResolution,pub argv_template:Vec<String>,
    pub created_at:String,pub expires_at:String,pub verification_scope:String,
}
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum JobState { Queued,Running,Verifying,Publishing,Succeeded,Failed,Cancelled,Interrupted }
impl JobState { pub fn terminal(&self)->bool {matches!(self,Self::Succeeded|Self::Failed|Self::Cancelled|Self::Interrupted)} }
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
pub struct Job {
    pub id:String,pub creator:String,pub plan:Plan,pub state:JobState,
    pub prepared_argv:Option<Vec<String>>,pub artifact:Option<FileIdentity>,
    pub verification:Option<String>,pub error:Option<crate::DomainError>,
}
