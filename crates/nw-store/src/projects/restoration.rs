//! Cross-row invariants for project writes made by the compatible legacy writer.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceCounts {
    live: usize,
    history: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationJournal {
    schema_version: u32,
    operation_id: OperationId,
    original_patch_id: OperationId,
    actor: String,
    command: nw_domain::commands::CommandRef,
    before_revision: Revision,
    after_revision: Revision,
    change_digest: String,
    forward_sha256: String,
    inverse_sha256: String,
    changed_ids: Vec<EntityId>,
}
fn refused() -> DomainError {
    DomainError::new(
        "RESTORATION_REFUSED",
        "Changed project record violates its typed or derived index invariants",
    )
}
pub(crate) fn validate(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    raw: &[u8],
) -> Result<()> {
    let parts: Vec<_> = key.split('/').collect();
    if parts.len() < 2 {
        return Err(refused());
    }
    let project = ProjectId::parse(parts[1])?;
    let header: ProjectHeader =
        table_value(table, &format!("project/{project}"))?.ok_or_else(refused)?;
    header.writable()?;
    domain::label(&header.name)?;
    if header.id != project
        || header.creator.is_empty()
        || header.entity_count > MAX_ENTITIES
        || header.sequence_count > header.entity_count
        || header.history_root != format!("project-history/{project}")
    {
        return Err(refused());
    }
    match parts[0] {
        "project" if parts.len() == 2 => {
            if raw != table_raw(table, key)?.ok_or_else(refused)?.as_bytes() {
                return Err(refused());
            }
            if !header.change_digest.is_empty() {
                nw_domain::media_execution::hash(&header.change_digest)?;
            }
        }
        "project-entity" if parts.len() == 3 => {
            let entity: EntityEnvelope = decode(raw)?;
            entity.validate()?;
            if entity.id.as_str() != parts[2] || entity.project_id != project {
                return Err(refused());
            }
            for (index, expected) in index_rows(table, &header, &entity, &BTreeMap::new(), true)? {
                if table
                    .get(index.as_str())
                    .map_err(storage)?
                    .as_ref()
                    .map(|row| row.value())
                    != Some(expected.as_slice())
                {
                    return Err(refused());
                }
            }
        }
        "project-bin" | "project-track" | "project-clip" | "project-dependency" => {
            let entity_id = EntityId::parse(*parts.last().ok_or_else(refused)?)?;
            let entity: EntityEnvelope =
                table_value(table, &entity_key(&project, &entity_id))?.ok_or_else(refused)?;
            entity.validate()?;
            if !index_rows(table, &header, &entity, &BTreeMap::new(), true)?
                .iter()
                .any(|(expected_key, expected)| expected_key == key && expected == raw)
            {
                return Err(refused());
            }
        }
        "project-reference" if parts.len() == 3 => {
            EntityId::parse(parts[2])?;
            let counts: ReferenceCounts = decode(raw)?;
            if counts.live > MAX_ENTITIES
                || counts.history > HISTORY_OPERATIONS
                || counts.live.saturating_add(counts.history) == 0
            {
                return Err(refused());
            }
        }
        "project-history" if parts.len() == 2 => {
            let history: ProjectHistory = decode(raw)?;
            if history.undo.len() + history.redo.len() > HISTORY_OPERATIONS
                || history.retained_bytes > HISTORY_BYTES
                || history.history_floor > header.revision.0
                || history.journal_floor > header.revision.0.saturating_add(1)
            {
                return Err(refused());
            }
            let mut ids = BTreeSet::new();
            let mut total = 0usize;
            for entry in history.undo.iter().chain(&history.redo) {
                if !ids.insert(&entry.id)
                    || entry.revision > header.revision
                    || entry.source_ids.len() > HISTORY_SOURCES
                {
                    return Err(refused());
                }
                let patch = table_raw(table, &format!("project-patch/{project}/{}", entry.id))?
                    .ok_or_else(refused)?;
                if patch.len() != entry.bytes {
                    return Err(refused());
                }
                total = total.checked_add(entry.bytes).ok_or_else(refused)?;
            }
            if total != history.retained_bytes {
                return Err(refused());
            }
            let summary = table_raw(table, &format!("project-history-summary/{project}"))?
                .ok_or_else(refused)?;
            if summary.as_bytes() != encode(&history.summary())? {
                return Err(refused());
            }
        }
        "project-history-summary" if parts.len() == 2 => {
            let history: ProjectHistory =
                table_value(table, &format!("project-history/{project}"))?.ok_or_else(refused)?;
            if raw != encode(&history.summary())? {
                return Err(refused());
            }
        }
        "project-patch" if parts.len() == 3 => {
            let operation = OperationId::parse(parts[2])?;
            let patch: StoredPatch = decode(raw)?;
            patch.forward.validate()?;
            patch.inverse.validate()?;
            if encode(&patch.inverse)? != encode(&patch.forward.inverse())? {
                return Err(refused());
            }
            let history: ProjectHistory =
                table_value(table, &format!("project-history/{project}"))?.ok_or_else(refused)?;
            if !history
                .undo
                .iter()
                .chain(&history.redo)
                .any(|entry| entry.id == operation)
            {
                return Err(refused());
            }
        }
        "project-op" if parts.len() == 3 => {
            let value: OperationJournal = decode(raw)?;
            if raw.len() > 16384
                || value.schema_version != 1
                || value.actor.is_empty()
                || value.command.version != 1
                || nw_domain::commands::descriptor(&value.command.id).is_none()
                || value.before_revision.next()? != value.after_revision
                || value.after_revision > header.revision
                || parts[2] != format!("{:020}", value.after_revision.0)
                || value.changed_ids.is_empty()
                || value.changed_ids.len() > MAX_CHANGES
            {
                return Err(refused());
            }
            nw_domain::media_execution::hash(&value.change_digest)?;
            nw_domain::media_execution::hash(&value.forward_sha256)?;
            nw_domain::media_execution::hash(&value.inverse_sha256)?;
            if value.change_digest != value.forward_sha256
                || value.changed_ids.iter().collect::<BTreeSet<_>>().len()
                    != value.changed_ids.len()
            {
                return Err(refused());
            }
            let _ = (value.operation_id, value.original_patch_id);
        }
        _ => return Err(refused()),
    }
    Ok(())
}

pub(crate) fn validate_project(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    raw: &[u8],
) -> Result<()> {
    validate(table, key, raw)?;
    let header: ProjectHeader = decode(raw)?;
    let mut entities = 0usize;
    let mut sequences = 0usize;
    let mut rows = 0usize;
    for namespace in [
        "project-entity",
        "project-bin",
        "project-track",
        "project-clip",
        "project-dependency",
        "project-reference",
        "project-patch",
        "project-op",
    ] {
        let prefix = format!("{namespace}/{}/", header.id);
        for row in table.range(prefix.as_str()..).map_err(storage)? {
            let (name, value) = row.map_err(storage)?;
            if !name.value().starts_with(&prefix) {
                break;
            }
            rows += 1;
            if rows > 50000 {
                return Err(refused());
            }
            validate(table, name.value(), value.value())?;
            if namespace == "project-entity" {
                entities += 1;
                let entity: EntityEnvelope = decode(value.value())?;
                sequences += usize::from(entity.kind == "sequence");
            }
        }
    }
    if entities != header.entity_count || sequences != header.sequence_count {
        return Err(refused());
    }
    for namespace in ["project-history", "project-history-summary"] {
        let name = format!("{namespace}/{}", header.id);
        let value = table
            .get(name.as_str())
            .map_err(storage)?
            .ok_or_else(refused)?;
        validate(table, &name, value.value())?;
    }
    Ok(())
}
