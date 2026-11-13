//! Typed old-writer continuity after an irreversible restoration exposure.
use crate::{decode, storage};
use nw_domain::{
    DomainError, Result,
    media_execution::{exact, hash},
};
use redb::ReadableTable;
use serde::Deserialize;

fn refused() -> DomainError {
    DomainError::new(
        "RESTORATION_REFUSED",
        "Changed legacy record violates its typed identity or bounds",
    )
}
fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(refused());
    }
    Ok(())
}
fn identity(value: &nw_domain::records::FileIdentity) -> Result<()> {
    hash(&value.sha256)?;
    exact(&value.bytes)?;
    if value.path.is_empty() || value.path.len() > 4096 {
        return Err(refused());
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    principal: String,
    profile: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    context: Context,
    epoch: String,
    expires: u64,
    #[serde(default)]
    last_active: u64,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    audience: u64,
    #[serde(default)]
    revoked: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    id: String,
    principal: String,
    profile: Option<String>,
    display_name: String,
    expected: u64,
    expected_hash: Option<String>,
    expires: u64,
    phase: String,
    staging: String,
    sealed: Option<nw_domain::records::FileIdentity>,
}
pub(crate) fn validate(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    raw: &[u8],
) -> Result<()> {
    let (namespace, id) = key.split_once('/').ok_or_else(refused)?;
    if namespace.starts_with("project-") {
        return crate::projects::validate_restored_record(table, key, raw);
    }
    identifier(id)?;
    match namespace {
        "credential" => {
            hash(id)?;
            let value: Credential = decode(raw)?;
            if value.context.principal.is_empty()
                || value.context.principal.len() > 256
                || value
                    .context
                    .profile
                    .as_ref()
                    .is_some_and(|profile| identifier(profile).is_err())
                || uuid::Uuid::parse_str(&value.epoch).is_err()
                || value.expires == 0
                || value.last_active > value.expires
                || value
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.len() > 4096)
            {
                return Err(refused());
            }
            // Revocation is a typed boolean; both issuance and revocation are valid.
            let _ = (value.revoked, value.audience);
        }
        "upload" => {
            let value: Upload = decode(raw)?;
            if value.id != id
                || value.principal.is_empty()
                || value.principal.len() > 256
                || value
                    .profile
                    .as_ref()
                    .is_some_and(|profile| identifier(profile).is_err())
                || value.display_name.is_empty()
                || value.display_name.len() > 1024
                || value.expected == 0
                || value.expected > 1024 * 1024 * 1024
                || value.expires == 0
                || value.staging.len() > 4096
                || !value.staging.ends_with(&format!("{id}.part"))
                || !["reserved", "receiving", "ready", "interrupted"]
                    .contains(&value.phase.as_str())
            {
                return Err(refused());
            }
            if let Some(expected) = &value.expected_hash {
                hash(expected)?;
            }
            if let Some(sealed) = &value.sealed {
                identity(sealed)?;
                if exact(&sealed.bytes)? != value.expected
                    || value
                        .expected_hash
                        .as_ref()
                        .is_some_and(|expected| expected != &sealed.sha256)
                {
                    return Err(refused());
                }
            }
            if value.phase == "ready" && value.sealed.is_none() {
                return Err(refused());
            }
        }
        "folder" => {
            let value: nw_domain::ingest::FolderSnapshot = decode(raw)?;
            value.validate_hash()?;
            if value.id != id {
                return Err(refused());
            }
        }
        "batch-plan" => {
            let value: nw_domain::batches::BatchPlan = decode(raw)?;
            if value.id != id || value.digest()? != value.hash {
                return Err(refused());
            }
        }
        "batch" => {
            let value: nw_domain::batches::Batch = decode(raw)?;
            if value.schema_version != 1
                || value.id != id
                || value.creator.is_empty()
                || value.members.is_empty()
                || value.members.len() > nw_domain::batches::MAX_BATCH_ITEMS
            {
                return Err(refused());
            }
            identifier(&value.plan_id)?;
            hash(&value.plan_hash)?;
            exact(&value.accepted_at)?;
            let plan_row = table
                .get(format!("batch-plan/{}", value.plan_id).as_str())
                .map_err(storage)?
                .ok_or_else(refused)?;
            let plan: nw_domain::batches::BatchPlan = decode(plan_row.value())?;
            if plan.digest()? != value.plan_hash
                || plan.creator != value.creator
                || plan.items.len() != value.members.len()
            {
                return Err(refused());
            }
            let mut jobs = std::collections::BTreeSet::new();
            for (index, member) in value.members.iter().enumerate() {
                if member.index as usize != index {
                    return Err(refused());
                }
                if let Some(job_id) = &member.job_id {
                    identifier(job_id)?;
                    if !jobs.insert(job_id) {
                        return Err(refused());
                    }
                    let row = table
                        .get(format!("job/{job_id}").as_str())
                        .map_err(storage)?
                        .ok_or_else(refused)?;
                    let job = crate::records::decode_job(row.value())?;
                    if job.id != *job_id || job.creator != value.creator {
                        return Err(refused());
                    }
                }
            }
        }
        _ => return Err(refused()),
    }
    Ok(())
}
pub(crate) fn removable(key: &str) -> bool {
    matches!(
        key.split('/').next(),
        Some(
            "credential"
                | "upload"
                | "project-entity"
                | "project-bin"
                | "project-track"
                | "project-clip"
                | "project-dependency"
                | "project-reference"
                | "project-patch"
                | "project-op"
        )
    )
}

pub(crate) fn validate_removed(
    before: &impl ReadableTable<&'static str, &'static [u8]>,
    current: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    raw: &[u8],
) -> Result<()> {
    if !removable(key) {
        return Err(refused());
    }
    if key.starts_with("credential/") || key.starts_with("upload/") {
        return validate(before, key, raw);
    }
    // Only a newer committed project checkpoint can remove an old derived row,
    // history eviction or a known entity. Opaque unknown entities stay exact.
    let parts: Vec<_> = key.split('/').collect();
    let project = *parts.get(1).ok_or_else(refused)?;
    nw_domain::projects::validate_id(project)?;
    let header_key = format!("project/{project}");
    let previous = before
        .get(header_key.as_str())
        .map_err(storage)?
        .ok_or_else(refused)?;
    let next = current
        .get(header_key.as_str())
        .map_err(storage)?
        .ok_or_else(refused)?;
    let previous: nw_domain::projects::ProjectHeader = decode(previous.value())?;
    let next: nw_domain::projects::ProjectHeader = decode(next.value())?;
    previous.writable()?;
    next.writable()?;
    if next.revision <= previous.revision {
        return Err(refused());
    }
    if parts[0] == "project-entity" {
        let entity: nw_domain::projects::EntityEnvelope = decode(raw)?;
        entity.validate()?;
        if !entity.known()
            || entity.project_id.as_str() != project
            || parts.get(2).copied() != Some(entity.id.as_str())
        {
            return Err(refused());
        }
    } else {
        crate::projects::validate_restored_record(before, key, raw)?;
    }
    Ok(())
}

pub(crate) fn validate_transition(key: &str, before: &[u8], after: &[u8]) -> Result<()> {
    match key.split('/').next().ok_or_else(refused)? {
        // These accepted objects have no legacy editing operation.
        "plan" | "asset" | "folder" | "batch-plan" | "batch" | "job-auth" | "cancel" => {
            return Err(refused());
        }
        "profile" => {
            let old: nw_domain::access::AccessProfile = decode(before)?;
            let new: nw_domain::access::AccessProfile = decode(after)?;
            if new.id != old.id || new.principal != old.principal || new.revision <= old.revision {
                return Err(refused());
            }
        }
        "job" => {
            let old = crate::records::decode_job(before)?;
            let new = crate::records::decode_job(after)?;
            if old.state.terminal()
                || old.id != new.id
                || old.creator != new.creator
                || crate::encode(&old.plan)? != crate::encode(&new.plan)?
            {
                return Err(refused());
            }
        }
        "publication" => {
            let mut old: nw_domain::access::Publication = decode(before)?;
            let new: nw_domain::access::Publication = decode(after)?;
            if old.published || !new.published {
                return Err(refused());
            }
            old.published = true;
            if crate::encode(&old)? != crate::encode(&new)? {
                return Err(refused());
            }
        }
        "project" => {
            let old: nw_domain::projects::ProjectHeader = decode(before)?;
            let new: nw_domain::projects::ProjectHeader = decode(after)?;
            old.writable()?;
            if new.id != old.id
                || new.creator != old.creator
                || new.root_bin_id != old.root_bin_id
                || new.revision <= old.revision
                || crate::encode(&new.extensions)? != crate::encode(&old.extensions)?
            {
                return Err(refused());
            }
        }
        "project-entity" => {
            let old: nw_domain::projects::EntityEnvelope = decode(before)?;
            let new: nw_domain::projects::EntityEnvelope = decode(after)?;
            if !old.known()
                || !new.known()
                || new.id != old.id
                || new.project_id != old.project_id
                || new.kind != old.kind
                || new.kind_version != old.kind_version
                || crate::encode(&new.extensions)? != crate::encode(&old.extensions)?
            {
                return Err(refused());
            }
        }
        "credential" => {
            let old: Credential = decode(before)?;
            let new: Credential = decode(after)?;
            if old.context.principal != new.context.principal
                || old.context.profile != new.context.profile
                || old.epoch != new.epoch
                || old.expires != new.expires
                || old.origin != new.origin
                || old.audience != new.audience
                || (old.revoked && !new.revoked)
                || new.last_active < old.last_active
            {
                return Err(refused());
            }
        }
        "upload" => {
            let old: Upload = decode(before)?;
            let new: Upload = decode(after)?;
            if old.id != new.id
                || old.principal != new.principal
                || old.profile != new.profile
                || old.display_name != new.display_name
                || old.expected != new.expected
                || old.expected_hash != new.expected_hash
                || old.expires != new.expires
                || old.staging != new.staging
                || (old.sealed.is_some() && old.sealed != new.sealed)
            {
                return Err(refused());
            }
        }
        // Their complete derived/key validation uses the current committed project.
        namespace if namespace.starts_with("project-") => {}
        _ => return Err(refused()),
    }
    Ok(())
}
