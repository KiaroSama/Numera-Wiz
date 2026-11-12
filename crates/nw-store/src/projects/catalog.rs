//! Header-only catalog reads: no content, history, writer lease or retained MVCC read.
use super::*;

#[derive(Deserialize)]
struct Identity {
    id: ProjectId,
    creator: String,
}
#[derive(Deserialize)]
struct Common {
    name: String,
    revision: Revision,
    schema_major: u32,
    envelope_version: u32,
}
fn corrupt() -> DomainError {
    DomainError::new("PROJECT_CORRUPT", "Invalid project catalog header")
}
impl Store {
    /// Visit at most 64 headers in ascending ID order in one short read transaction.
    /// Authorization runs on identity first; unauthorized metadata is never projected.
    pub fn visit_project_summaries(
        &self,
        mut authorize: impl FnMut(&ProjectId, &str) -> Result<bool>,
        mut visit: impl FnMut(ProjectSummary, &str) -> Result<()>,
    ) -> Result<()> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        for (count, row) in table.range("project/"..).map_err(storage)?.enumerate() {
            let (key, data) = row.map_err(storage)?;
            let Some(id) = key.value().strip_prefix("project/") else {
                break;
            };
            if count >= 64 || data.value().len() > 64 * 1024 {
                return Err(domain::limit(
                    "Project catalog exceeds its header count or raw byte budget",
                ));
            }
            let identity: Identity = serde_json::from_slice(data.value()).map_err(|_| corrupt())?;
            if identity.id.as_str() != id || identity.creator.is_empty() {
                return Err(corrupt());
            }
            if !authorize(&identity.id, &identity.creator)? {
                continue;
            }
            // Common fields deliberately do not require this version's full header shape.
            let common: Common = serde_json::from_slice(data.value()).map_err(|_| corrupt())?;
            domain::label(&common.name).map_err(|_| corrupt())?;
            let compatibility = if common.schema_major == 1 && common.envelope_version == 1 {
                ProjectCompatibility::Supported
            } else {
                ProjectCompatibility::ReadOnlyUnsupported
            };
            visit(ProjectSummary {
                id: identity.id,
                name: common.name,
                revision: common.revision,
                schema_major: common.schema_major,
                envelope_version: common.envelope_version,
                compatibility,
                authorization: None,
            }, &identity.creator)?;
        }
        Ok(())
    }
}
