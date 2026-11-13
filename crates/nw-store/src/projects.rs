//! Indexed project MVCC reads and atomic project/history/receipt acceptance in versioned_records.
use super::*;
use nw_domain::projects::{self as domain, edit::ProjectRead, views::*, *};
use redb::ReadableTableMetadata;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
mod catalog;
mod render;
mod restoration;

pub(crate) use restoration::validate as validate_restored_record;
pub(crate) use restoration::validate_project as validate_restored_project;

const HISTORY_OPERATIONS: usize = 256;
const HISTORY_BYTES: usize = 32 * 1024 * 1024;
// Reserve three quarters of the unchanged 512 index/reference-write allowance
// for a branch edit. Evict complete oldest originals; never drop current sources.
const HISTORY_SOURCES: usize = MAX_INDEX_WRITES / 4;
const JOURNAL_OPERATIONS: u64 = 512;
const PROJECT_GENERATION_LIMIT: u64 = 48000;
const PAGE_BYTES: usize = 240 * 1024;

pub struct ProjectReadSnapshot {
    read: live::Read,
    header: ProjectHeader,
    header_raw: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: OperationId,
    pub revision: Revision,
    pub bytes: usize,
    pub source_ids: Vec<EntityId>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectHistory {
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
    pub retained_bytes: usize,
    pub history_floor: u64,
    pub journal_floor: u64,
}
impl ProjectHistory {
    pub fn summary(&self) -> Value {
        json!({"can_undo":!self.undo.is_empty(),"can_redo":!self.redo.is_empty(),"retained_operations":self.undo.len()+self.redo.len(),"retained_bytes":self.retained_bytes,"history_floor":self.history_floor.to_string(),"journal_floor":self.journal_floor.to_string()})
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPatch {
    pub forward: ProjectPatch,
    pub inverse: ProjectPatch,
}
#[derive(Debug, Clone)]
pub enum HistoryAction {
    Normal,
    Undo(OperationId),
    Redo(OperationId),
}
pub struct ProjectCommit {
    pub header: ProjectHeader,
    pub expected_header: Option<String>,
    pub patch: ProjectPatch,
    pub operation_id: OperationId,
    pub history_action: HistoryAction,
    pub created: Value,
    pub expected_records: Vec<(String, String)>,
}
pub struct IndexedPage {
    pub items: Vec<Value>,
    pub next_key: Option<String>,
    pub total_count: usize,
    pub count_accuracy: String,
    pub scanned: usize,
}
fn entity_key(project: &ProjectId, entity: &EntityId) -> String {
    format!("project-entity/{project}/{entity}")
}
fn table_raw(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
) -> Result<Option<String>> {
    bounded_table_raw(table, key, MAX_RECORD)
}
fn bounded_table_raw(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
    maximum: usize,
) -> Result<Option<String>> {
    table
        .get(key)
        .map_err(storage)?
        .map(|v| {
            if v.value().len() > maximum {
                return Err(domain::limit(
                    "Project detail record exceeds its bounded section",
                ));
            }
            std::str::from_utf8(v.value())
                .map(str::to_owned)
                .map_err(storage)
        })
        .transpose()
}
fn table_value<T: DeserializeOwned>(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
) -> Result<Option<T>> {
    table
        .get(key)
        .map_err(storage)?
        .map(|v| decode(v.value()))
        .transpose()
}
fn bounded_range(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    prefix: &str,
    after: Option<&str>,
    limit: usize,
    budget: usize,
) -> Result<(Vec<(String, Value)>, bool)> {
    if limit == 0 || limit > 513 || budget > PAGE_BYTES {
        return Err(domain::limit("Invalid indexed page budget"));
    }
    if after.is_some_and(|a| !a.starts_with(prefix)) {
        return Err(domain::invalid("Cursor key is outside the requested index"));
    }
    let mut rows = Vec::new();
    let mut bytes = 0;
    for row in table.range(after.unwrap_or(prefix)..).map_err(storage)? {
        let (key, data) = row.map_err(storage)?;
        let key = key.value();
        if !key.starts_with(prefix) {
            break;
        }
        if after == Some(key) {
            continue;
        }
        if rows.len() == limit || bytes + data.value().len() + 1 > budget {
            if rows.is_empty() {
                return Err(domain::limit("One summary exceeds the page byte budget"));
            }
            return Ok((rows, true));
        }
        bytes += data.value().len() + 1;
        rows.push((key.to_owned(), decode(data.value())?));
    }
    Ok((rows, false))
}
fn validate_checkpoint(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    header: &ProjectHeader,
) -> Result<()> {
    if header.revision.0 > 0 {
                let journal: Value = table_value(
                    table,
                    &format!("project-op/{}/{:020}", header.id, header.revision.0),
                )?
                .ok_or_else(|| {
                    DomainError::new(
                        "PROJECT_CORRUPT",
                        "Latest committed change journal is missing",
                    )
                })?;
                if journal["after_revision"] != header.revision.to_string()
                    || journal["change_digest"] != header.change_digest
                {
                    return Err(DomainError::new(
                        "PROJECT_CORRUPT",
                        "Latest committed change facts differ",
                    ));
                }
            }
    Ok(())
}
impl Store {
    pub fn project_creator(&self, id: &str) -> Result<String> {
        validate_id(id)?;
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let raw = bounded_table_raw(&table, &format!("project/{id}"), 64 * 1024)?
            .ok_or_else(DomainError::object_not_found)?;
        #[derive(Deserialize)]
        struct Identity {
            id: ProjectId,
            creator: String,
        }
        let identity: Identity = serde_json::from_str(&raw).map_err(storage)?;
        if identity.id.as_str() != id {
            return Err(DomainError::new(
                "PROJECT_CORRUPT",
                "Project key identity differs",
            ));
        }
        Ok(identity.creator)
    }
    pub fn project_fact_hash(&self, key: &str) -> Result<String> {
        let read = self.db.begin_read().map_err(storage)?;
        let table = read.open_table(RECORDS).map_err(storage)?;
        let raw = table_raw(&table, key)?.ok_or_else(DomainError::object_not_found)?;
        Ok(domain::hash(raw.as_bytes()))
    }
    pub fn project_read(&self, id: &str) -> Result<ProjectReadSnapshot> {
        let id = ProjectId::parse(id)?;
        let read = self.db.begin_read().map_err(storage)?;
        let (header_raw, header) = {
            let table = read.open_table(RECORDS).map_err(storage)?;
            let raw = bounded_table_raw(&table, &format!("project/{id}"), 64 * 1024)?
                .ok_or_else(DomainError::object_not_found)?;
            let header: ProjectHeader = serde_json::from_str(&raw).map_err(storage)?;
            if header.extensions.len() > 64 {
                return Err(domain::limit(
                    "Project header has too many opaque extension fields",
                ));
            }
            if header.id != id || header.materialized_revision != header.revision {
                return Err(DomainError::new(
                    "PROJECT_CORRUPT",
                    "Project identity/checkpoint mismatch",
                ));
            }
            validate_checkpoint(&table, &header)?;
            (raw, header)
        };
        Ok(ProjectReadSnapshot {
            read,
            header,
            header_raw,
        })
    }
    pub fn project_generation_bytes(&self) -> Result<u64> {
        std::fs::metadata(self.db.path()?.join("store.redb"))
            .map(|m| m.len())
            .map_err(storage)
    }
    pub fn commit_project_with_clock(
        &self,
        principal: &str,
        request: &Request,
        prepared: ProjectCommit,
        clock: impl FnOnce() -> Result<u64>,
    ) -> Result<Response> {
        prepared.patch.validate()?;
        self.commit_composed_with_clock(principal, request, clock, |table, _now| {
            let mut header = prepared.header.clone(); let project = header.id.clone();
            let header_key = format!("project/{project}");
            let current = table_raw(table, &header_key)?;
            if current != prepared.expected_header { return Err(DomainError::new("STALE_REVISION", "Project changed before atomic acceptance")); }
            let creating = current.is_none();
            if creating {
                if request.command.id != "projects.create" || header.revision != Revision(0) { return Err(domain::invalid("Invalid project creation acceptance")); }
                let mut count = 0;
                for row in table.range("project/"..).map_err(storage)? {
                    let (key, _) = row.map_err(storage)?; if !key.value().starts_with("project/") { break; }
                    count += 1; if count >= 64 { return Err(domain::limit("At most 64 projects are admitted")); }
                }
            } else {
                header = serde_json::from_str(current.as_deref().expect("Existing header checked above")).map_err(storage)?;
                if header.id != project || request.project.as_ref().map(|p| p.id.as_str()) != Some(project.as_str()) { return Err(domain::invalid("Project envelope and stored identity differ")); }
                let history_matches = matches!((&prepared.history_action, request.command.id.as_str()), (HistoryAction::Undo(_), "history.undo") | (HistoryAction::Redo(_), "history.redo"))
                    || (matches!(&prepared.history_action, HistoryAction::Normal) && ["sequences.create", "clips.place", "clips.move", "clips.link", "clips.unlink", "clips.split", "clips.remove"].contains(&request.command.id.as_str()));
                if !history_matches { return Err(domain::invalid("Project command and prepared history action differ")); }
                header.writable()?;
                let expected = request.project.as_ref().and_then(|p| p.expected_revision.as_deref()).ok_or_else(|| domain::invalid("Project write revision required"))?;
                if Revision::parse(expected)? != header.revision { return Err(DomainError::new("STALE_REVISION", "Expected revision differs from current project")); }
            }
            for (key, expected_hash) in &prepared.expected_records {
                let raw = table_raw(table, key)?.ok_or_else(DomainError::object_not_found)?;
                if domain::hash(raw.as_bytes()) != *expected_hash { return Err(DomainError::new("SOURCE_CHANGED", "Referenced acceptance fact changed")); }
            }
            if !creating && prepared.patch.changes.is_empty() && matches!(&prepared.history_action, HistoryAction::Normal) {
                let history: ProjectHistory = table_value(table, &format!("project-history/{project}"))?.ok_or_else(|| DomainError::new("PROJECT_CORRUPT", "Project history checkpoint missing"))?;
                return Ok(json!({"project_id":project,"base_revision":header.revision,"revision":header.revision,"operation_id":prepared.operation_id,"created":prepared.created,"changed_ids":[],"history":history.summary(),"no_change":true}));
            }
            let mut changes = BTreeMap::new();
            for change in &prepared.patch.changes {
                let key = entity_key(&project, &change.id);
                if table_raw(table, &key)? != change.before { return Err(DomainError::new("STALE_REVISION", "Expected entity before-image differs")); }
                let before: Option<EntityEnvelope> = change.before.as_ref().map(|raw| serde_json::from_str(raw).map_err(storage)).transpose()?;
                let after: Option<EntityEnvelope> = change.after.as_ref().map(|raw| serde_json::from_str(raw).map_err(storage)).transpose()?;
                for entity in before.iter().chain(after.iter()) { if entity.project_id != project || entity.id != change.id { return Err(domain::invalid("Cross-project entity change")); } }
                changes.insert(change.id.clone(), (before, after));
            }
            let mut rows: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
            for (before, after) in changes.values() {
                if let Some(entity) = before {
                    for (key, expected) in index_rows(table, &header, entity, &changes, false)? {
                        let actual = table.get(key.as_str()).map_err(storage)?;
                        if actual.as_ref().map(|v| v.value()) != Some(expected.as_slice()) { return Err(DomainError::new("PROJECT_CORRUPT", "Expected entity index before-image differs")); }
                        rows.insert(key, None);
                    }
                }
                if let Some(entity) = after {
                    for (key, data) in index_rows(table, &header, entity, &changes, true)? { rows.insert(key, Some(data)); }
                }
            }
            if rows.len() > MAX_INDEX_WRITES { return Err(domain::limit("Project edit exceeds 512 index writes")); }
            let mut entity_count = header.entity_count as i64; let mut sequence_count = header.sequence_count as i64;
            for (before, after) in changes.values() {
                entity_count += i64::from(after.is_some()) - i64::from(before.is_some());
                sequence_count += i64::from(after.as_ref().is_some_and(|e| e.kind == "sequence")) - i64::from(before.as_ref().is_some_and(|e| e.kind == "sequence"));
            }
            if !(0..=MAX_ENTITIES as i64).contains(&entity_count) || sequence_count < 0 { return Err(domain::limit("Project entity/count budget exceeded")); }
            for change in &prepared.patch.changes { rows.insert(entity_key(&project, &change.id), change.after.as_ref().map(|raw| raw.as_bytes().to_vec())); }
            let history_key = format!("project-history/{project}");
            let mut history: ProjectHistory = table_value(table, &history_key)?.unwrap_or_default();
            let base = header.revision;
            let mut retained_delta: BTreeMap<EntityId, i64> = BTreeMap::new();
            if !creating {
                header.revision = header.revision.next()?;
                let patch_id = match &prepared.history_action {
                    HistoryAction::Normal => {
                        for entry in history.redo.drain(..) {
                            history.retained_bytes = history.retained_bytes.saturating_sub(entry.bytes);
                            rows.insert(format!("project-patch/{project}/{}", entry.id), None);
                            for id in entry.source_ids { *retained_delta.entry(id).or_default() -= 1; }
                        }
                        let stored = StoredPatch { forward: prepared.patch.clone(), inverse: prepared.patch.inverse() };
                        let bytes = encode(&stored)?;
                        if bytes.len() > MAX_PATCH_BYTES { return Err(domain::limit("Original patch exceeds 128 KiB")); }
                        let source_ids = patch_sources(&prepared.patch)?;
                        if source_ids.len() > HISTORY_SOURCES { return Err(domain::limit("One original operation exceeds retained source headroom")); }
                        for id in &source_ids { *retained_delta.entry(id.clone()).or_default() += 1; }
                        history.retained_bytes += bytes.len();
                        history.undo.push(HistoryEntry { id: prepared.operation_id.clone(), revision: header.revision, bytes: bytes.len(), source_ids });
                        rows.insert(format!("project-patch/{project}/{}", prepared.operation_id), Some(bytes));
                        while history.undo.len() + history.redo.len() > HISTORY_OPERATIONS || history.retained_bytes > HISTORY_BYTES || retained_source_count(&history) > HISTORY_SOURCES {
                            let entry = history.undo.remove(0); history.retained_bytes -= entry.bytes; history.history_floor = header.revision.0;
                            rows.insert(format!("project-patch/{project}/{}", entry.id), None);
                            for id in entry.source_ids { *retained_delta.entry(id).or_default() -= 1; }
                        }
                        prepared.operation_id.clone()
                    }
                    HistoryAction::Undo(id) => {
                        let entry = history.undo.pop().ok_or_else(|| DomainError::new("NOTHING_TO_UNDO", "No retained undo operation"))?;
                        if entry.id != *id { return Err(DomainError::new("STALE_REVISION", "Undo stack changed")); }
                        history.redo.push(entry); id.clone()
                    }
                    HistoryAction::Redo(id) => {
                        let entry = history.redo.pop().ok_or_else(|| DomainError::new("NOTHING_TO_REDO", "No retained redo operation"))?;
                        if entry.id != *id { return Err(DomainError::new("STALE_REVISION", "Redo stack changed")); }
                        history.undo.push(entry); id.clone()
                    }
                };
                header.change_digest = domain::hash(&encode(&prepared.patch)?);
                let journal = json!({"schema_version":1,"operation_id":prepared.operation_id,"original_patch_id":patch_id,"actor":principal,"command":request.command,"before_revision":base,"after_revision":header.revision,"change_digest":header.change_digest,"forward_sha256":domain::hash(&encode(&prepared.patch)?),"inverse_sha256":domain::hash(&encode(&prepared.patch.inverse())?),"changed_ids":prepared.patch.changes.iter().map(|c|&c.id).collect::<Vec<_>>()});
                let journal_bytes = encode(&journal)?;
                // 512 * 16KiB is the independent 8MiB compensation metadata ceiling.
                if journal_bytes.len() > 16 * 1024 { return Err(domain::limit("Operation journal metadata exceeds 16 KiB")); }
                rows.insert(format!("project-op/{project}/{:020}", header.revision.0), Some(journal_bytes));
                if header.revision.0 > JOURNAL_OPERATIONS {
                    let expired = header.revision.0 - JOURNAL_OPERATIONS;
                    rows.insert(format!("project-op/{project}/{expired:020}"), None); history.journal_floor = expired + 1;
                }
            }
            history.history_floor = history.undo.iter().chain(history.redo.iter()).map(|entry| entry.revision.0).min().unwrap_or(header.revision.0);
            let mut live_delta: BTreeMap<EntityId, i64> = BTreeMap::new();
            for (before, after) in changes.values() {
                for (entity, direction) in [(before, -1), (after, 1)] {
                    if let Some(entity) = entity {
                        if entity.kind == "clip" { let clip: Clip = entity.payload("clip")?; *live_delta.entry(clip.source_id).or_default() += direction; }
                        if entity.kind == "source_binding" { *live_delta.entry(entity.id.clone()).or_default() += direction; }
                    }
                }
            }
            live_delta.retain(|_, delta| *delta != 0);
            retained_delta.retain(|_, delta| *delta != 0);
            let sources: BTreeSet<_> = live_delta.keys().chain(retained_delta.keys()).cloned().collect();
            for source in sources {
                let key = format!("project-reference/{project}/{source}");
                let current: Value = table_value(table, &key)?.unwrap_or(json!({"live":0,"history":0}));
                let live = current["live"].as_i64().unwrap_or(0) + live_delta.get(&source).copied().unwrap_or(0);
                let retained = current["history"].as_i64().unwrap_or(0) + retained_delta.get(&source).copied().unwrap_or(0);
                if live < 0 || retained < 0 { return Err(DomainError::new("PROJECT_CORRUPT", "Source reference counter underflow")); }
                rows.insert(key, if live == 0 && retained == 0 { None } else { Some(encode(&json!({"live":live,"history":retained}))?) });
            }
            header.entity_count = entity_count as usize; header.sequence_count = sequence_count as usize;
            header.materialized_revision = header.revision;
            let header_bytes = encode(&header)?;
            if header_bytes.len() > 64 * 1024 { return Err(domain::limit("Project header exceeds its bounded section")); }
            rows.insert(format!("project-history-summary/{project}"), Some(encode(&history.summary())?));
            rows.insert(history_key, Some(encode(&history)?)); rows.insert(header_key, Some(header_bytes));
            let index_reference_writes = rows.keys().filter(|key| ["project-bin/", "project-track/", "project-clip/", "project-dependency/", "project-reference/"].iter().any(|prefix| key.starts_with(prefix))).count();
            if index_reference_writes > MAX_INDEX_WRITES { return Err(domain::limit("Project index and retained reference writes exceed 512")); }
            let additions = rows.iter().filter(|(_, data)| data.is_some()).try_fold(0u64, |count, (key, _)| -> Result<u64> { Ok(count + u64::from(table.get(key.as_str()).map_err(storage)?.is_none())) })?;
            let removals = rows.iter().filter(|(_, data)| data.is_none()).try_fold(0u64, |count, (key, _)| -> Result<u64> { Ok(count + u64::from(table.get(key.as_str()).map_err(storage)?.is_some())) })?;
            if table.len().map_err(storage)?.saturating_add(additions).saturating_sub(removals) + 2 > PROJECT_GENERATION_LIMIT { return Err(domain::limit("Generation project headroom exhausted; no records were evicted")); }
            for (key, data) in rows {
                match data {
                    Some(data) => { if data.len() > MAX_RECORD { return Err(domain::limit("Project record exceeds 8 MiB")); } table.insert(key.as_str(), data.as_slice()).map_err(storage)?; }
                    None => { table.remove(key.as_str()).map_err(storage)?; }
                }
            }
            #[cfg(test)]
            if self.project_fail_after_rows.swap(false, std::sync::atomic::Ordering::SeqCst) {
                return Err(DomainError::new("INJECTED_PROJECT_FAILURE", "Injected after project rows and before receipt/clock commit"));
            }
            if creating { Ok(json!({"project_id":project,"root_bin_id":header.root_bin_id,"revision":header.revision,"history":{"can_undo":false,"can_redo":false}})) }
            else { Ok(json!({"project_id":project,"base_revision":base,"revision":header.revision,"operation_id":prepared.operation_id,"created":prepared.created,"changed_ids":prepared.patch.changes.iter().map(|c|&c.id).collect::<Vec<_>>(),"history":history.summary()})) }
        })
    }
}
fn retained_source_count(history: &ProjectHistory) -> usize {
    history
        .undo
        .iter()
        .chain(&history.redo)
        .flat_map(|entry| entry.source_ids.iter())
        .collect::<BTreeSet<_>>()
        .len()
}
fn patch_sources(patch: &ProjectPatch) -> Result<Vec<EntityId>> {
    let mut ids = BTreeSet::new();
    for change in &patch.changes {
        for raw in [&change.before, &change.after].into_iter().flatten() {
            let entity: EntityEnvelope = serde_json::from_str(raw).map_err(storage)?;
            if entity.kind == "clip" {
                ids.insert(entity.payload::<Clip>("clip")?.source_id);
            }
            if entity.kind == "source_binding" {
                ids.insert(entity.id);
            }
        }
    }
    Ok(ids.into_iter().collect())
}
type EntityPair = (Option<EntityEnvelope>, Option<EntityEnvelope>);
fn index_rows(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    header: &ProjectHeader,
    entity: &EntityEnvelope,
    changes: &BTreeMap<EntityId, EntityPair>,
    after: bool,
) -> Result<Vec<(String, Vec<u8>)>> {
    let project = &header.id;
    let mut rows = Vec::new();
    if let Some(owner) = &entity.owner_id
        && ["sequence", "source_binding", "bin"].contains(&entity.kind.as_str())
    {
        rows.push((
            format!(
                "project-bin/{project}/{owner}/{}/{id}",
                hex::encode(entity.label.as_bytes()),
                id = entity.id
            ),
            encode(&ProjectItemSummary {
                id: entity.id.clone(),
                kind: entity.kind.clone(),
                name: entity.label.clone(),
            })?,
        ));
    }
    if entity.kind == "track" {
        let track: Track = entity.payload("track")?;
        rows.push((
            format!(
                "project-track/{project}/{}/{:08x}/{}",
                track.sequence_id, track.order, entity.id
            ),
            encode(&TrackSummary {
                id: entity.id.clone(),
                kind: track.kind,
                name: entity.label.clone(),
                order: track.order,
                clip_count: track.clip_count,
            })?,
        ));
    }
    if entity.kind == "clip" {
        let clip: Clip = entity.payload("clip")?;
        let track_entity = changes
            .get(&clip.track_id)
            .and_then(|(before, next)| {
                if after {
                    next.as_ref()
                } else {
                    before.as_ref()
                }
            })
            .cloned()
            .or(table_value(table, &entity_key(project, &clip.track_id))?)
            .ok_or_else(DomainError::object_not_found)?;
        let track: Track = track_entity.payload("track")?;
        let tick = grid_tick(clip.sequence_start, track.grid)?;
        rows.push((
            format!(
                "project-clip/{project}/{}/{}/{tick:016x}/{}",
                clip.sequence_id, clip.track_id, entity.id
            ),
            encode(&ClipSummary {
                id: entity.id.clone(),
                track_id: clip.track_id,
                source_id: clip.source_id,
                sequence_start: clip.sequence_start,
                duration: clip.duration,
            })?,
        ));
        if let Some(group) = clip.link_group_id {
            rows.push((
                format!("project-dependency/{project}/{group}/{}", entity.id),
                encode(&entity.id)?,
            ));
        }
    }
    for reference in &entity.references {
        if ["semantic_input", "link"].contains(&reference.role.as_str()) {
            rows.push((
                format!(
                    "project-dependency/{project}/{}/{}",
                    reference.target, entity.id
                ),
                encode(&entity.id)?,
            ));
        }
    }
    let targets = if !entity.envelope_semantics_supported() {
        // Envelope support is independent of whether a payload codec is known.
        vec![header.root_bin_id.clone()]
    } else if !entity.known() && entity.enabled && entity.semantic_scope != "annotation" {
        match entity.semantic_scope.as_str() {
            "project" => vec![header.root_bin_id.clone()],
            "track" | "sequence" => {
                let mut targets = BTreeSet::new();
                let mut ancestors = BTreeMap::new();
                for target in entity
                    .owner_id
                    .iter()
                    .chain(entity.references.iter().map(|r| &r.target))
                {
                    targets.insert(scope_ancestor(
                        table,
                        header,
                        target,
                        &entity.semantic_scope,
                        changes,
                        after,
                        &mut ancestors,
                    )?);
                }
                if targets.is_empty() {
                    return Err(domain::invalid(
                        "Scoped semantic node requires an ancestor target",
                    ));
                }
                targets.into_iter().collect()
            }
            _ => entity
                .owner_id
                .iter()
                .cloned()
                .chain(entity.references.iter().map(|r| r.target.clone()))
                .collect(),
        }
    } else {
        Vec::new()
    };
    for target in targets {
        rows.push((
            format!("project-dependency/{project}/{target}/{}", entity.id),
            encode(&entity.id)?,
        ));
    }
    Ok(rows)
}
// Resolve declared scope through bounded ownership, including newly prepared ancestors.
fn scope_ancestor(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    header: &ProjectHeader,
    target: &EntityId,
    kind: &str,
    changes: &BTreeMap<EntityId, EntityPair>,
    after: bool,
    ancestors: &mut BTreeMap<EntityId, EntityEnvelope>,
) -> Result<EntityId> {
    let mut next = target.clone();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(next.clone()) {
            return Err(domain::invalid("Semantic scope ownership cycle"));
        }
        if seen.len() > MAX_CLOSURE {
            return Err(domain::limit("Semantic scope ancestor budget exceeded"));
        }
        let entity = if let Some(entity) = ancestors.get(&next) {
            entity.clone()
        } else {
            if ancestors.len() >= MAX_CLOSURE {
                return Err(domain::limit("Declared scope targets exceed 256 ancestors"));
            }
            let entity: EntityEnvelope = if let Some((before, following)) = changes.get(&next) {
                (if after { following } else { before })
                    .clone()
                    .ok_or_else(DomainError::object_not_found)?
            } else {
                table_value(table, &entity_key(&header.id, &next))?
                    .ok_or_else(DomainError::object_not_found)?
            };
            ancestors.insert(next.clone(), entity.clone());
            entity
        };
        if entity.id != next || entity.project_id != header.id {
            return Err(domain::invalid("Semantic scope target identity differs"));
        }
        entity.validate()?;
        if entity.kind == kind {
            return Ok(next);
        }
        next = entity
            .owner_id
            .ok_or_else(|| domain::invalid("Declared semantic scope has no matching ancestor"))?;
    }
}
impl ProjectReadSnapshot {
    pub fn header(&self) -> &ProjectHeader {
        &self.header
    }
    pub fn header_raw(&self) -> &str {
        &self.header_raw
    }
    pub fn history_summary(&self) -> Result<Value> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        let raw = bounded_table_raw(
            &table,
            &format!("project-history-summary/{}", self.header.id),
            4096,
        )?
        .ok_or_else(|| DomainError::new("PROJECT_CORRUPT", "Project history summary is missing"))?;
        serde_json::from_str(&raw).map_err(storage)
    }
    pub fn history(&self) -> Result<ProjectHistory> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        table_value(&table, &format!("project-history/{}", self.header.id))?.ok_or_else(|| {
            DomainError::new("PROJECT_CORRUPT", "Project history checkpoint missing")
        })
    }
    pub fn patch(&self, id: &OperationId) -> Result<StoredPatch> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        table_value(&table, &format!("project-patch/{}/{id}", self.header.id))?
            .ok_or_else(|| DomainError::new("PROJECT_CORRUPT", "Retained history patch missing"))
    }
    pub fn bin_page(
        &self,
        bin: &EntityId,
        after: Option<&str>,
        limit: usize,
        budget: usize,
    ) -> Result<IndexedPage> {
        let owner: Bin = self.entity(bin)?.payload("bin")?;
        self.index_page(
            &format!("project-bin/{}/{bin}/", self.header.id),
            after,
            limit,
            budget,
            owner.member_count,
        )
    }
    pub fn track_page(
        &self,
        sequence: &EntityId,
        after: Option<&str>,
        limit: usize,
        budget: usize,
    ) -> Result<IndexedPage> {
        let owner: Sequence = self.entity(sequence)?.payload("sequence")?;
        self.index_page(
            &format!("project-track/{}/{sequence}/", self.header.id),
            after,
            limit,
            budget,
            owner.track_count,
        )
    }
    fn index_page(
        &self,
        prefix: &str,
        after: Option<&str>,
        limit: usize,
        budget: usize,
        total: usize,
    ) -> Result<IndexedPage> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        let (rows, more) = bounded_range(&table, prefix, after, limit, budget)?;
        let next_key = if more {
            rows.last().map(|r| r.0.clone())
        } else {
            None
        };
        let scanned = rows.len() + usize::from(more);
        Ok(IndexedPage {
            items: rows.into_iter().map(|r| r.1).collect(),
            next_key,
            total_count: total,
            count_accuracy: "exact".into(),
            scanned,
        })
    }
    fn clip_rows(
        &self,
        sequence: &EntityId,
        track_id: &EntityId,
        start: nw_domain::time::Rational,
        end: nw_domain::time::Rational,
        after: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<(String, ClipSummary)>, bool)> {
        let track: Track = self.entity(track_id)?.payload("track")?;
        self.clip_rows_with_track(
            sequence,
            track_id,
            &track,
            nw_domain::time::TimeRange { start, end },
            (after, limit),
            None,
        )
    }
    // The legacy wrapper keeps its exact admission behavior. Compiler callers already
    // hold a budget-admitted Track and bound raw index bytes before summary decoding.
    fn clip_rows_with_track(
        &self,
        sequence: &EntityId,
        track_id: &EntityId,
        track: &Track,
        range: nw_domain::time::TimeRange,
        page: (Option<&str>, usize),
        summary_budget: Option<usize>,
    ) -> Result<(Vec<(String, ClipSummary)>, bool)> {
        let nw_domain::time::TimeRange { start, end } = range;
        let (after, limit) = page;
        let mut summary_bytes = 0;
        let mut admit_summary = |bytes: usize| -> Result<()> {
            if let Some(budget) = summary_budget {
                if bytes > 1024 || bytes > budget.saturating_sub(summary_bytes) {
                    return Err(domain::limit(
                        "Compiler clip index exceeds summary admission",
                    ));
                }
                summary_bytes += bytes;
            }
            Ok(())
        };
        if track.sequence_id != *sequence {
            return Err(domain::invalid("Viewport track is not in this sequence"));
        }
        let prefix = format!("project-clip/{}/{sequence}/{track_id}/", self.header.id);
        let scaled = start.checked_mul(track.grid)?;
        let tick = scaled.numerator() / scaled.denominator();
        let lower = format!("{prefix}{tick:016x}/");
        let upper_time = end.checked_mul(track.grid)?;
        let upper_tick = upper_time.numerator() / upper_time.denominator();
        let upper = format!(
            "{prefix}{:016x}/",
            upper_tick
                .checked_add(1)
                .ok_or_else(|| domain::limit("Viewport grid overflow"))?
        );
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        let mut rows = Vec::new();
        if after.is_none()
            && let Some(row) = table
                .range(prefix.as_str()..lower.as_str())
                .map_err(storage)?
                .next_back()
        {
            let (key, data) = row.map_err(storage)?;
            admit_summary(data.value().len())?;
            let clip: ClipSummary = decode(data.value())?;
            if clip.sequence_start.checked_add(clip.duration)? > start {
                rows.push((key.value().to_owned(), clip));
            }
        }
        if after.is_some_and(|a| !a.starts_with(&prefix)) {
            return Err(domain::invalid(
                "Clip continuation is outside the selected track",
            ));
        }
        for row in table
            .range(after.unwrap_or(&lower)..upper.as_str())
            .map_err(storage)?
        {
            let (key, data) = row.map_err(storage)?;
            if after == Some(key.value()) {
                continue;
            }
            admit_summary(data.value().len())?;
            let clip: ClipSummary = decode(data.value())?;
            if clip.sequence_start >= end {
                break;
            }
            if clip.sequence_start.checked_add(clip.duration)? <= start {
                continue;
            }
            if rows.len() == limit {
                return Ok((rows, true));
            }
            rows.push((key.value().to_owned(), clip));
        }
        Ok((rows, false))
    }
    pub fn clip_range(
        &self,
        sequence: &EntityId,
        track_ids: &[EntityId],
        range: nw_domain::time::TimeRange,
        after: Option<&str>,
        limit: usize,
        budget: usize,
    ) -> Result<IndexedPage> {
        range.validate()?;
        if track_ids.is_empty() || track_ids.len() > 64 || limit == 0 || limit > 512 {
            return Err(domain::limit(
                "Viewport requires 1-64 tracks and 1-512 items",
            ));
        }
        let _: Sequence = self.entity(sequence)?.payload("sequence")?;
        let mut tracks = Vec::new();
        let mut seen = BTreeSet::new();
        for id in track_ids {
            if !seen.insert(id.clone()) {
                return Err(domain::invalid("Duplicate viewport track"));
            }
            let track: Track = self.entity(id)?.payload("track")?;
            if track.sequence_id != *sequence {
                return Err(domain::invalid(
                    "Viewport track belongs to another sequence",
                ));
            }
            tracks.push((track.order, id));
        }
        tracks.sort();
        let continuation = after
            .map(|key| {
                key.split_once(':')
                    .ok_or_else(|| domain::invalid("Invalid viewport continuation"))
            })
            .transpose()?;
        let mut items = Vec::new();
        let mut bytes = 0;
        let mut last = None;
        let mut scanned = tracks.len();
        for (order, track) in tracks {
            let order_key = format!("{order:08x}");
            if continuation.is_some_and(|(saved, _)| saved > order_key.as_str()) {
                continue;
            }
            let track_after = continuation
                .filter(|(saved, _)| *saved == order_key.as_str())
                .map(|(_, key)| key);
            let (rows, more) = self.clip_rows(
                sequence,
                track,
                range.start,
                range.end,
                track_after,
                limit + 1,
            )?;
            scanned += rows.len() + 1;
            for (key, item) in rows {
                let encoded = encode(&item)?;
                if items.len() == limit || bytes + encoded.len() + 1 > budget {
                    if items.is_empty() {
                        return Err(domain::limit("Viewport summary exceeds page budget"));
                    }
                    return Ok(IndexedPage {
                        total_count: items.len(),
                        items,
                        next_key: last,
                        count_accuracy: "lower_bound".into(),
                        scanned,
                    });
                }
                bytes += encoded.len() + 1;
                items.push(serde_json::to_value(item).map_err(storage)?);
                last = Some(format!("{order_key}:{key}"));
            }
            if more {
                return Ok(IndexedPage {
                    total_count: items.len(),
                    items,
                    next_key: last,
                    count_accuracy: "lower_bound".into(),
                    scanned,
                });
            }
        }
        Ok(IndexedPage {
            total_count: items.len(),
            items,
            next_key: None,
            count_accuracy: "exact".into(),
            scanned,
        })
    }
}
impl ProjectRead for ProjectReadSnapshot {
    fn header(&self) -> &ProjectHeader {
        &self.header
    }
    fn raw_entity(&self, id: &EntityId) -> Result<Option<String>> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        bounded_table_raw(&table, &entity_key(&self.header.id, id), 192 * 1024)
    }
    fn has_dependency_target(&self, id: &EntityId) -> Result<bool> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        let prefix = format!("project-dependency/{}/{id}/", self.header.id);
        let mut rows = table.range(prefix.as_str()..).map_err(storage)?;
        match rows.next() {
            Some(row) => {
                let (key, _) = row.map_err(storage)?;
                Ok(key.value().starts_with(&prefix))
            }
            None => Ok(false),
        }
    }
    fn dependents(&self, id: &EntityId, limit: usize) -> Result<Vec<EntityId>> {
        let table = self.read.open_table(RECORDS).map_err(storage)?;
        let prefix = format!("project-dependency/{}/{id}/", self.header.id);
        let mut ids = Vec::new();
        for row in table.range(prefix.as_str()..).map_err(storage)? {
            let (key, value) = row.map_err(storage)?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            if ids.len() >= limit {
                return Err(domain::limit(
                    "Dependency closure exceeds its finite budget",
                ));
            }
            ids.push(decode(value.value())?);
        }
        Ok(ids)
    }
    fn intersecting(
        &self,
        sequence: &EntityId,
        track: &EntityId,
        start: nw_domain::time::Rational,
        end: nw_domain::time::Rational,
        limit: usize,
    ) -> Result<Vec<EntityEnvelope>> {
        // A newly-created track has no interval index yet.
        if self.raw_entity(track)?.is_none() {
            return Ok(Vec::new());
        }
        let (rows, more) = self.clip_rows(sequence, track, start, end, None, limit)?;
        if more {
            return Err(domain::limit(
                "Collision candidates exceed the bounded edit closure",
            ));
        }
        rows.iter().map(|(_, clip)| self.entity(&clip.id)).collect()
    }
}

/// HMAC binds project cursors to a per-ticket random secret without changing legacy cursor encoding.
pub fn cursor_mac(secret: &[u8], payload: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts arbitrary key lengths");
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
pub(super) fn exercise_atomic_projects() {
    use nw_domain::projects::edit::{AllocatedIds, ProjectEditIntent, resolve_edit};
    use std::collections::VecDeque;
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(temporary.path()).unwrap();
    let project = ProjectId::parse(Uuid::new_v4().to_string()).unwrap();
    let root = EntityId::parse(Uuid::new_v4().to_string()).unwrap();
    let header = ProjectHeader {
        envelope_version: 1,
        schema_major: 1,
        id: project.clone(),
        creator: "atomic-project".into(),
        name: "Atomic project".into(),
        revision: Revision(0),
        root_bin_id: root.clone(),
        entity_count: 0,
        sequence_count: 0,
        history_root: format!("project-history/{project}"),
        materialized_revision: Revision(0),
        change_digest: String::new(),
        compatibility: "supported".into(),
        extensions: BTreeMap::new(),
    };
    let entity = EntityEnvelope::new(
        &project,
        root.clone(),
        "bin",
        None,
        "Root".into(),
        &Bin {
            member_count: 0,
            locked: false,
        },
    )
    .unwrap();
    let scope = store.issue_scope("atomic-project", 200).unwrap();
    let mut create = Request::query("projects.create", json!({"name":"Atomic project"}));
    create.replay_scope = Some(scope.clone());
    create.idempotency_key = Some("atomic-create".into());
    let creation = || ProjectCommit {
        header: header.clone(),
        expected_header: None,
        patch: ProjectPatch {
            changes: vec![EntityChange {
                id: root.clone(),
                before: None,
                after: Some(entity.bytes().unwrap()),
            }],
        },
        operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
        history_action: HistoryAction::Normal,
        created: json!({}),
        expected_records: Vec::new(),
    };
    store
        .project_fail_after_rows
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        store
            .commit_project_with_clock("atomic-project", &create, creation(), || Ok(201))
            .unwrap_err()
            .code,
        "INJECTED_PROJECT_FAILURE"
    );
    assert!(
        store
            .get::<Value>(&format!("project/{project}"))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get::<Value>(&format!("project-entity/{project}/{root}"))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get::<Value>(&format!("project-history/{project}"))
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .replay("atomic-project", &create, 201)
            .unwrap()
            .is_none()
    );
    let accepted = store
        .commit_project_with_clock("atomic-project", &create, creation(), || Ok(201))
        .unwrap();
    let read = store.project_read(project.as_str()).unwrap();
    let input = SequenceInput {
        name: "Sequence".into(),
        settings: SequenceSettings {
            width: 64,
            height: 48,
            frame_rate: nw_domain::time::Rational::new(4, 1).unwrap(),
            sample_rate: 48000,
            audio_layout: "stereo".into(),
        },
        tracks: vec![TrackInput {
            kind: TrackKind::Video,
            name: "Video".into(),
        }],
    };
    let mut request = Request::query("sequences.create", serde_json::to_value(&input).unwrap());
    request.project = Some(nw_domain::commands::ProjectRef {
        id: project.to_string(),
        expected_revision: Some("0".into()),
    });
    request.replay_scope = Some(scope);
    request.idempotency_key = Some("atomic-sequence".into());
    let resolved = resolve_edit(
        &read,
        ProjectEditIntent::Sequence(input),
        AllocatedIds(
            (0..4)
                .map(|_| EntityId::parse(Uuid::new_v4().to_string()).unwrap())
                .collect::<VecDeque<_>>(),
        ),
    )
    .unwrap();
    let preparation = || ProjectCommit {
        header: read.header().clone(),
        expected_header: Some(read.header_raw().into()),
        patch: resolved.forward.clone(),
        operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
        history_action: HistoryAction::Normal,
        created: resolved.created.clone(),
        expected_records: Vec::new(),
    };
    store
        .project_fail_after_rows
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        store
            .commit_project_with_clock("atomic-project", &request, preparation(), || Ok(202))
            .unwrap_err()
            .code,
        "INJECTED_PROJECT_FAILURE"
    );
    assert_eq!(
        store
            .project_read(project.as_str())
            .unwrap()
            .header()
            .revision,
        Revision(0)
    );
    assert!(
        read.bin_page(&root, None, 128, PAGE_BYTES)
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        store
            .list::<Value>(&format!("project-op/{project}/"), 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .list::<Value>(&format!("project-patch/{project}/"), 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .replay("atomic-project", &request, 202)
            .unwrap()
            .is_none()
    );
    let changed = store
        .commit_project_with_clock("atomic-project", &request, preparation(), || Ok(202))
        .unwrap();
    assert_eq!(changed.result.as_ref().unwrap()["revision"], "1");
    assert!(
        read.bin_page(&root, None, 128, PAGE_BYTES)
            .unwrap()
            .items
            .is_empty(),
        "Old real MVCC read stays at revision zero"
    );
    drop(read);
    drop(store);
    let store = Store::open(temporary.path()).unwrap();
    let read = store.project_read(project.as_str()).unwrap();
    assert_eq!(read.header().revision, Revision(1));
    assert_eq!(
        read.bin_page(&root, None, 128, PAGE_BYTES)
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        store
            .replay("atomic-project", &create, 203)
            .unwrap()
            .unwrap()
            .receipt_id,
        accepted.receipt_id
    );
    assert_eq!(
        store
            .replay("atomic-project", &request, 203)
            .unwrap()
            .unwrap()
            .receipt_id,
        changed.receipt_id
    );
    drop(read);
    let mut revision = Revision(1);
    for ordinal in 0..514 {
        let snapshot = store.project_read(project.as_str()).unwrap();
        let history = snapshot.history().unwrap();
        let undo = ordinal % 2 == 0;
        let entry = if undo {
            history.undo.last()
        } else {
            history.redo.last()
        }
        .unwrap();
        let stored = snapshot.patch(&entry.id).unwrap();
        let mut compensate = Request::query(
            if undo { "history.undo" } else { "history.redo" },
            json!({}),
        );
        compensate.project = Some(nw_domain::commands::ProjectRef {
            id: project.to_string(),
            expected_revision: Some(revision.to_string()),
        });
        compensate.replay_scope = request.replay_scope.clone();
        compensate.idempotency_key = Some(format!("bounded-compensation-{ordinal}"));
        let prepared = ProjectCommit {
            header: snapshot.header().clone(),
            expected_header: Some(snapshot.header_raw().into()),
            patch: if undo { stored.inverse } else { stored.forward },
            operation_id: OperationId::parse(Uuid::new_v4().to_string()).unwrap(),
            history_action: if undo {
                HistoryAction::Undo(entry.id.clone())
            } else {
                HistoryAction::Redo(entry.id.clone())
            },
            created: json!({}),
            expected_records: Vec::new(),
        };
        drop(snapshot);
        let response =
            store
                .commit_project_with_clock("atomic-project", &compensate, prepared, || {
                    Ok(204 + ordinal)
                })
                .unwrap();
        revision = revision.next().unwrap();
        assert_eq!(response.result.unwrap()["revision"], revision.to_string());
    }
    assert_eq!(
        store
            .list::<Value>(&format!("project-op/{project}/"), 512)
            .unwrap()
            .len(),
        512
    );
    assert_eq!(
        store
            .list::<Value>(&format!("project-patch/{project}/"), 2)
            .unwrap()
            .len(),
        1
    );
    let snapshot = store.project_read(project.as_str()).unwrap();
    assert_eq!(snapshot.history().unwrap().undo.len(), 1);
    assert!(snapshot.history().unwrap().redo.is_empty());
    assert_eq!(snapshot.header().revision, Revision(515));
    assert!(snapshot.history().unwrap().journal_floor > 1);
    // Independent per-project ownership is rooted in UUID, not either data root.
    let lease = nw_platform::project_lease::ProjectWriterLease::acquire(&project).unwrap();
    assert!(nw_platform::project_lease::ProjectWriterLease::acquire(&project).is_err());
    drop(lease);
    let _reacquired = nw_platform::project_lease::ProjectWriterLease::acquire(&project).unwrap();
    regressions::run();
    render::exercise_compiler_admission();
    drop(snapshot);
    restoration::exercise_known_corruption(&store, &project);
}

#[cfg(test)]
mod regressions;
