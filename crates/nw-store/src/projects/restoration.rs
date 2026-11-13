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
fn validate_entity(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    header: &ProjectHeader,
    entity: &EntityEnvelope,
) -> Result<()> {
    entity.validate()?;
    if !entity.known() {
        return Ok(());
    }
    let get = |id: &EntityId| -> Result<EntityEnvelope> {
        let row: EntityEnvelope =
            table_value(table, &entity_key(&header.id, id))?.ok_or_else(refused)?;
        row.validate()?;
        if row.id != *id || row.project_id != header.id {
            return Err(refused());
        }
        Ok(row)
    };
    let owner_kind = match entity.kind.as_str() {
        "bin" | "sequence" | "source_binding" => "bin",
        "track" => "sequence",
        "clip" => "track",
        _ => return Err(refused()),
    };
    if entity.id == header.root_bin_id {
        if entity.kind != "bin" || entity.owner_id.is_some() {
            return Err(refused());
        }
    } else {
        let owner = entity.owner_id.as_ref().ok_or_else(refused)?;
        if get(owner)?.kind != owner_kind
            || !entity
                .references
                .iter()
                .any(|reference| reference.role == "ownership" && reference.target == *owner)
        {
            return Err(refused());
        }
    }
    let mut ancestor = entity.owner_id.clone();
    let mut seen = BTreeSet::from([entity.id.clone()]);
    while let Some(id) = ancestor {
        if !seen.insert(id.clone()) || seen.len() > MAX_CLOSURE {
            return Err(refused());
        }
        let parent = get(&id)?;
        if parent.owner_id.is_none() && parent.id != header.root_bin_id {
            return Err(refused());
        }
        ancestor = parent.owner_id;
    }
    match entity.kind.as_str() {
        "bin" => {
            let value: Bin = entity.payload("bin")?;
            if value.member_count > MAX_ENTITIES {
                return Err(refused());
            }
        }
        "sequence" => {
            let value: Sequence = entity.payload("sequence")?;
            value.settings.validate()?;
            if value.track_count > MAX_ENTITIES {
                return Err(refused());
            }
        }
        "track" => {
            let value: Track = entity.payload("track")?;
            let sequence: Sequence = get(&value.sequence_id)?.payload("sequence")?;
            sequence.settings.validate()?;
            let grid = match value.kind {
                TrackKind::Video => sequence.settings.frame_rate,
                TrackKind::Audio => {
                    nw_domain::time::Rational::new(i64::from(sequence.settings.sample_rate), 1)?
                }
            };
            if entity.owner_id.as_ref() != Some(&value.sequence_id)
                || value.grid != grid
                || value.clip_count > MAX_ENTITIES
            {
                return Err(refused());
            }
        }
        "clip" => {
            let value: Clip = entity.payload("clip")?;
            let track: Track = get(&value.track_id)?.payload("track")?;
            let _: Sequence = get(&value.sequence_id)?.payload("sequence")?;
            let source: SourceBinding = get(&value.source_id)?.payload("source_binding")?;
            if entity.owner_id.as_ref() != Some(&value.track_id)
                || track.sequence_id != value.sequence_id
                || value.rate != nw_domain::time::Rational::new(1, 1)?
                || value.duration != value.source_range.duration()?
            {
                return Err(refused());
            }
            grid_tick(value.sequence_start, track.grid)?;
            grid_tick(value.end()?, track.grid)?;
            domain::edit::source_valid(&source, &value.source_range, track.kind)?;
            if !entity
                .references
                .iter()
                .any(|reference| reference.role == "source" && reference.target == value.source_id)
            {
                return Err(refused());
            }
            for reference in entity
                .references
                .iter()
                .filter(|reference| reference.role == "link")
            {
                let linked: Clip = get(&reference.target)?.payload("clip")?;
                if linked.sequence_id != value.sequence_id {
                    return Err(refused());
                }
            }
            if let Some(group) = &value.link_group_id {
                if table
                    .get(entity_key(&header.id, group).as_str())
                    .map_err(storage)?
                    .is_some()
                {
                    return Err(refused());
                }
                let prefix = format!("project-dependency/{}/{group}/", header.id);
                let mut members = 0usize;
                for row in table.range(prefix.as_str()..).map_err(storage)? {
                    let (key, _) = row.map_err(storage)?;
                    if !key.value().starts_with(&prefix) {
                        break;
                    }
                    members += 1;
                    if members > MAX_CLOSURE {
                        return Err(refused());
                    }
                    let member =
                        EntityId::parse(key.value().rsplit('/').next().ok_or_else(refused)?)?;
                    let other = get(&member)?;
                    if other.known() && other.kind == "clip" {
                        let linked: Clip = other.payload("clip")?;
                        if linked.link_group_id.as_ref() == Some(group)
                            && linked.sequence_id != value.sequence_id
                        {
                            return Err(refused());
                        }
                    }
                }
            }
        }
        "source_binding" => match entity.payload::<SourceBinding>("source_binding")? {
            SourceBinding::SolidColor { rgba, duration } => {
                if duration <= nw_domain::time::Rational::ZERO
                    || rgba
                        .iter()
                        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    return Err(refused());
                }
            }
            SourceBinding::Registered {
                asset_id,
                content_sha256,
                probe_sha256,
                ..
            } => {
                domain::validate_id(&asset_id)?;
                nw_domain::media_execution::hash(&content_sha256)?;
                nw_domain::media_execution::hash(&probe_sha256)?;
            }
        },
        _ => return Err(refused()),
    }
    Ok(())
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
            validate_entity(table, &header, &entity)?;
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
            validate_entity(table, &header, &entity)?;
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
    validate_checkpoint(table, &header)?;
    let mut entities = 0usize;
    let mut sequences = 0usize;
    let mut rows = 0usize;
    let mut counts: BTreeMap<EntityId, usize> = BTreeMap::new();
    let mut declared: BTreeMap<EntityId, usize> = BTreeMap::new();
    let mut opaque_owners = BTreeSet::new();
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
                if entity.known() {
                    if let Some(owner) = &entity.owner_id {
                        *counts.entry(owner.clone()).or_default() += 1;
                    }
                    let count = match entity.kind.as_str() {
                        "bin" => Some(entity.payload::<Bin>("bin")?.member_count),
                        "sequence" => Some(entity.payload::<Sequence>("sequence")?.track_count),
                        "track" => Some(entity.payload::<Track>("track")?.clip_count),
                        _ => None,
                    };
                    if let Some(count) = count {
                        declared.insert(entity.id, count);
                    }
                } else if let Some(owner) = entity.owner_id {
                    // Opaque baseline children keep their exact bytes; do not
                    // invent a membership/count meaning for an unknown codec.
                    opaque_owners.insert(owner);
                }
            }
        }
    }
    if declared.iter().any(|(id, count)| {
        !opaque_owners.contains(id) && counts.get(id).copied().unwrap_or(0) != *count
    }) {
        return Err(refused());
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

#[cfg(test)]
pub(super) fn exercise_known_corruption(store: &Store, project: &ProjectId) {
    let key = format!("project/{project}");
    let validate = || {
        let read = store.db.begin_read().unwrap();
        let table = read.open_table(RECORDS).unwrap();
        let row = table.get(key.as_str()).unwrap().unwrap();
        validate_project(&table, &key, row.value())
    };
    validate().unwrap();
    let rows = store
        .rows::<EntityEnvelope>(&format!("project-entity/{project}/"), 10000)
        .unwrap();
    let (entity_key, mut sequence) = rows
        .into_iter()
        .find(|(_, entity)| entity.kind == "sequence")
        .unwrap();
    let original = store.get::<EntityEnvelope>(&entity_key).unwrap().unwrap();
    sequence.payload_json = "{}".into();
    sequence.payload_sha256 = domain::hash(sequence.payload_json.as_bytes());
    sequence.validate().unwrap();
    store.put(&entity_key, &sequence).unwrap();
    assert!(
        validate().is_err(),
        "Valid envelope/hash cannot stand in for Sequence payload validation"
    );
    store.put(&entity_key, &original).unwrap();
    let header: ProjectHeader = store.get(&key).unwrap().unwrap();
    let journal_key = format!("project-op/{project}/{:020}", header.revision.0);
    let journal: Value = store.get(&journal_key).unwrap().unwrap();
    store.remove(&journal_key).unwrap();
    assert!(
        validate().is_err(),
        "Latest checkpoint journal is required even when every remaining journal parses"
    );
    store.put(&journal_key, &journal).unwrap();
    let mut wrong = journal.clone();
    wrong["change_digest"] = json!("ab".repeat(32));
    wrong["forward_sha256"] = wrong["change_digest"].clone();
    store.put(&journal_key, &wrong).unwrap();
    assert!(
        validate().is_err(),
        "Latest journal must bind the actual header change digest"
    );
    store.put(&journal_key, &journal).unwrap();
    validate().unwrap();
}
