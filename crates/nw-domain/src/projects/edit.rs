//! Pure bounded resolvers over indexed project facts; no service or filesystem authority here.
use super::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub trait ProjectRead {
    fn header(&self) -> &ProjectHeader;
    fn raw_entity(&self, id: &EntityId) -> Result<Option<String>>;
    fn dependents(&self, id: &EntityId, limit: usize) -> Result<Vec<EntityId>>;
    fn intersecting(&self, sequence: &EntityId, track: &EntityId, start: Rational, end: Rational, limit: usize) -> Result<Vec<EntityEnvelope>>;
    fn entity(&self, id: &EntityId) -> Result<EntityEnvelope> {
        let raw = self.raw_entity(id)?.ok_or_else(DomainError::object_not_found)?;
        let entity: EntityEnvelope = serde_json::from_str(&raw).map_err(|e| invalid(e.to_string()))?;
        if entity.id != *id || entity.project_id != self.header().id { return Err(DomainError::new("PROJECT_CORRUPT", "Entity key identity differs")); }
        entity.validate()?; Ok(entity)
    }
}
pub enum ProjectEditIntent { Sequence(SequenceInput), Place(Box<PlaceInput>, SourceBinding), Move(MoveInput), Split(SplitInput), Remove(RemoveInput) }
pub struct AllocatedIds(pub VecDeque<EntityId>);
impl AllocatedIds { fn next(&mut self) -> Result<EntityId> { self.0.pop_front().ok_or_else(|| limit("Allocated edit ID budget exceeded")) } }
struct Edit<'a, R: ProjectRead> { read: &'a R, changes: BTreeMap<EntityId, EntityChange>, sources: Vec<SourceBinding> }
impl<R: ProjectRead> Edit<'_, R> {
    fn get(&self, id: &EntityId) -> Result<EntityEnvelope> {
        if let Some(change) = self.changes.get(id) {
            return change.after.as_ref().ok_or_else(DomainError::object_not_found).and_then(|raw| serde_json::from_str(raw).map_err(|e| invalid(e.to_string())));
        }
        self.read.entity(id)
    }
    fn insert(&mut self, entity: EntityEnvelope) -> Result<()> {
        if self.changes.contains_key(&entity.id) || self.read.raw_entity(&entity.id)?.is_some() { return Err(DomainError::new("OBJECT_CONFLICT", "Allocated entity ID is already present")); }
        self.set(entity)
    }
    fn set(&mut self, entity: EntityEnvelope) -> Result<()> {
        let before = match self.changes.get(&entity.id) { Some(c) => c.before.clone(), None => self.read.raw_entity(&entity.id)? };
        let after = Some(entity.bytes()?);
        if before != after { self.changes.insert(entity.id.clone(), EntityChange { id: entity.id.clone(), before, after }); }
        if self.changes.len() > MAX_CHANGES { return Err(limit("Edit closure exceeds 128 changes")); } Ok(())
    }
    fn delete(&mut self, id: &EntityId) -> Result<()> {
        let before = self.read.raw_entity(id)?.ok_or_else(DomainError::object_not_found)?;
        self.changes.insert(id.clone(), EntityChange { id: id.clone(), before: Some(before), after: None }); Ok(())
    }
    fn count(&mut self, id: &EntityId, delta: i32) -> Result<()> {
        let mut entity = self.get(id)?;
        if entity.kind == "bin" { let mut b: Bin = entity.payload("bin")?; b.member_count = change_count(b.member_count, delta)?; entity.update(&b)?; }
        else { let mut t: Track = entity.payload("track")?; t.clip_count = change_count(t.clip_count, delta)?; entity.update(&t)?; }
        self.set(entity)
    }
    fn clip_context(&self, clip: &Clip) -> Result<Track> {
        let sequence: Sequence = self.get(&clip.sequence_id)?.payload("sequence")?;
        let track: Track = self.get(&clip.track_id)?.payload("track")?;
        if sequence.locked || track.locked || clip.locked { return Err(DomainError::new("ENTITY_LOCKED", "Edit touches a locked clip, track or sequence")); }
        if track.sequence_id != clip.sequence_id || clip.rate != Rational::new(1, 1)? || clip.duration != clip.source_range.duration()? {
            return Err(invalid("Clip must use unit forward source mapping in its containing sequence"));
        }
        grid_tick(clip.sequence_start, track.grid)?; grid_tick(clip.end()?, track.grid)?;
        let source: SourceBinding = self.get(&clip.source_id)?.payload("source_binding")?;
        source_valid(&source, &clip.source_range, track.kind)?; Ok(track)
    }
    fn finish(self, created: serde_json::Value) -> Result<ResolvedEdit> {
        let forward = ProjectPatch { changes: self.changes.into_values().collect() }; forward.validate()?;
        check_compatibility(self.read, &forward)?;
        validate_patch_clips(self.read, &forward)?;
        let changed_ids = forward.changes.iter().map(|c| c.id.clone()).collect();
        Ok(ResolvedEdit { base_revision: self.read.header().revision, inverse: forward.inverse(), forward, changed_ids, created, source_requirements: self.sources, compatibility_report: "supported_affected_closure".into() })
    }
}
fn change_count(value: usize, delta: i32) -> Result<usize> {
    value.checked_add_signed(delta as isize).ok_or_else(|| DomainError::new("PROJECT_CORRUPT", "Container count overflow/underflow"))
}
fn source_valid(source: &SourceBinding, span: &SourceSpan, kind: TrackKind) -> Result<()> {
    span.duration()?;
    match source {
        SourceBinding::SolidColor { rgba, duration } => {
            if kind != TrackKind::Video || rgba.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || *duration <= Rational::ZERO || span.start < Rational::ZERO || span.end > *duration {
                return Err(invalid("Solid color requires finite normalized RGBA, video track and a contained positive source range"));
            }
        }
        SourceBinding::Registered { media_kind, .. } if *media_kind != kind => return Err(invalid("Source stream media kind differs from the track")),
        _ => {}
    }
    Ok(())
}
fn expand_links<R: ProjectRead>(read: &R, ids: &[EntityId]) -> Result<Vec<EntityEnvelope>> {
    if ids.is_empty() || ids.len() > MAX_CHANGES { return Err(limit("Select 1-128 clips")); }
    let mut pending: VecDeque<_> = ids.iter().cloned().collect();
    let mut seen = BTreeSet::new(); let mut clips = Vec::new();
    while let Some(id) = pending.pop_front() {
        if !seen.insert(id.clone()) { continue; }
        if seen.len() > MAX_CHANGES { return Err(limit("Linked edit exceeds 128 clips")); }
        let entity = read.entity(&id)?; let clip: Clip = entity.payload("clip")?;
        if let Some(group) = &clip.link_group_id { pending.extend(read.dependents(group, MAX_CLOSURE)?); }
        pending.extend(entity.references.iter().filter(|r| r.role == "link").map(|r| r.target.clone()));
        for dependent in read.dependents(&id, MAX_CLOSURE)? {
            let linked = read.entity(&dependent)?;
            if linked.kind == "clip" && linked.references.iter().any(|r| r.role == "link" && r.target == id) { pending.push_back(dependent); }
        }
        clips.push(entity);
    }
    Ok(clips)
}
pub fn resolve_edit<R: ProjectRead>(read: &R, intent: ProjectEditIntent, mut ids: AllocatedIds) -> Result<ResolvedEdit> {
    read.header().writable()?;
    let mut edit = Edit { read, changes: BTreeMap::new(), sources: Vec::new() };
    let root = read.header().root_bin_id.clone();
    let created = match intent {
        ProjectEditIntent::Sequence(input) => {
            label(&input.name)?; input.settings.validate()?;
            if input.tracks.is_empty() || input.tracks.len() > 64 { return Err(limit("Sequence requires 1-64 explicit tracks")); }
            let bin: Bin = read.entity(&root)?.payload("bin")?;
            if bin.locked { return Err(DomainError::new("ENTITY_LOCKED", "Root bin is locked")); }
            let sequence_id = ids.next()?; let mut tracks = Vec::new();
            for (order, track) in input.tracks.iter().enumerate() {
                label(&track.name)?;
                let id = ids.next()?;
                let payload = Track { sequence_id: sequence_id.clone(), kind: track.kind, order: order as u32, grid: match track.kind { TrackKind::Video => input.settings.frame_rate, TrackKind::Audio => Rational::new(i64::from(input.settings.sample_rate), 1)? }, clip_count: 0, locked: false };
                edit.insert(EntityEnvelope::new(&read.header().id, id.clone(), "track", Some(sequence_id.clone()), track.name.clone(), &payload)?)?;
                tracks.push(json!({"id":id,"kind":track.kind}));
            }
            let payload = Sequence { settings: input.settings, track_count: tracks.len(), locked: false };
            edit.insert(EntityEnvelope::new(&read.header().id, sequence_id.clone(), "sequence", Some(root.clone()), input.name, &payload)?)?;
            edit.count(&root, 1)?;
            json!({"sequence_id":sequence_id,"tracks":tracks})
        }
        ProjectEditIntent::Place(input, source) => {
            let input = *input;
            if input.collision != "reject" { return Err(invalid("Only collision:reject is supported")); }
            let duration = input.source_range.duration()?;
            let source_id = ids.next()?; let clip_id = ids.next()?;
            let clip = Clip { sequence_id: input.sequence_id, track_id: input.track_id, source_id: source_id.clone(), occurrence_id: ids.next()?, source_range: input.source_range, sequence_start: input.sequence_start, duration, rate: Rational::new(1, 1)?, link_group_id: None, locked: false };
            let source_label = match &source { SourceBinding::SolidColor { .. } => "Solid color".to_owned(), SourceBinding::Registered { .. } => "Registered source".to_owned() };
            edit.insert(EntityEnvelope::new(&read.header().id, source_id.clone(), "source_binding", Some(root.clone()), source_label, &source)?)?;
            edit.clip_context(&clip)?;
            let mut entity = EntityEnvelope::new(&read.header().id, clip_id.clone(), "clip", Some(clip.track_id.clone()), "Clip".into(), &clip)?;
            entity.references.push(EntityReference { role: "source".into(), target: source_id.clone() });
            edit.count(&clip.track_id, 1)?; edit.count(&root, 1)?;
            edit.insert(entity)?; edit.sources.push(source);
            json!({"source_id":source_id,"clip_ids":[clip_id]})
        }
        ProjectEditIntent::Move(input) => {
            if input.collision != "reject" || input.link_policy != "expand" { return Err(invalid("Move requires collision:reject and link_policy:expand")); }
            for mut entity in expand_links(read, &input.clip_ids)? {
                let mut clip: Clip = entity.payload("clip")?; edit.clip_context(&clip)?;
                clip.sequence_start = clip.sequence_start.checked_add(input.delta)?;
                edit.clip_context(&clip)?; entity.update(&clip)?; edit.set(entity)?;
            }
            json!({})
        }
        ProjectEditIntent::Split(input) => {
            if input.link_policy != "expand" { return Err(invalid("Split requires link_policy:expand")); }
            let primary: Clip = read.entity(&input.clip_id)?.payload("clip")?;
            if input.at_sequence == primary.sequence_start || input.at_sequence == primary.end()? {
                return edit.finish(json!({"clip_ids":[],"no_change":true}));
            }
            let mut right_ids = Vec::new();
            let mut right_entities = Vec::new();
            let mut right_mapping = BTreeMap::new();
            let mut right_groups: BTreeMap<EntityId, EntityId> = BTreeMap::new();
            for mut entity in expand_links(read, &[input.clip_id])? {
                let mut left: Clip = entity.payload("clip")?; let track = edit.clip_context(&left)?;
                grid_tick(input.at_sequence, track.grid)?;
                if input.at_sequence <= left.sequence_start || input.at_sequence >= left.end()? { return Err(invalid("Split must be interior to every linked clip")); }
                let cut = input.at_sequence.checked_sub(left.sequence_start)?;
                let mut right = left.clone(); right.sequence_start = input.at_sequence;
                right.source_range.start = left.source_range.start.checked_add(cut)?;
                right.duration = left.duration.checked_sub(cut)?;
                right.occurrence_id = ids.next()?;
                if let Some(group) = &left.link_group_id {
                    let right_group = match right_groups.get(group) { Some(id) => id.clone(), None => { let id = ids.next()?; right_groups.insert(group.clone(), id.clone()); id } };
                    right.link_group_id = Some(right_group);
                }
                left.source_range.end = right.source_range.start; left.duration = cut;
                entity.update(&left)?; edit.set(entity.clone())?;
                let left_id = entity.id.clone();
                entity.id = ids.next()?; entity.update(&right)?;
                right_mapping.insert(left_id, entity.id.clone());
                right_ids.push(entity.id.clone()); right_entities.push(entity); edit.count(&left.track_id, 1)?;
            }
            for mut entity in right_entities {
                for reference in &mut entity.references {
                    if reference.role == "link" { reference.target = right_mapping.get(&reference.target).cloned().ok_or_else(|| invalid("Split cannot retain a link outside its complete right closure"))?; }
                }
                edit.insert(entity)?;
            }
            json!({"clip_ids":right_ids,"no_change":right_ids.is_empty()})
        }
        ProjectEditIntent::Remove(input) => {
            if input.mode != "lift" || input.link_policy != "expand" { return Err(invalid("Remove requires mode:lift and link_policy:expand")); }
            for entity in expand_links(read, &input.clip_ids)? {
                let clip: Clip = entity.payload("clip")?; edit.clip_context(&clip)?;
                edit.count(&clip.track_id, -1)?; edit.delete(&entity.id)?;
            }
            json!({})
        }
    };
    edit.finish(created)
}
/// Check only the affected semantic closure. Ownership visits ancestors, never sibling clips.
pub fn check_compatibility<R: ProjectRead>(read: &R, patch: &ProjectPatch) -> Result<()> {
    read.header().writable()?;
    let mut queue: VecDeque<_> = patch.changes.iter().map(|c| c.id.clone()).collect();
    queue.push_back(read.header().root_bin_id.clone());
    let mut seen = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) { continue; }
        if seen.len() > MAX_CLOSURE { return Err(limit("Affected semantic closure exceeds 256 entities")); }
        let raw = patch.changes.iter().find(|c| c.id == id).and_then(|c| c.before.as_ref().or(c.after.as_ref())).cloned().or(read.raw_entity(&id)?);
        if let Some(raw) = raw {
            let entity: EntityEnvelope = serde_json::from_str(&raw).map_err(|e| invalid(e.to_string()))?;
            entity.validate()?; entity.semantics()?;
            let locked = if entity.known() { match entity.kind.as_str() {
                "bin" => entity.payload::<Bin>("bin")?.locked,
                "sequence" => entity.payload::<Sequence>("sequence")?.locked,
                "track" => entity.payload::<Track>("track")?.locked,
                "clip" => entity.payload::<Clip>("clip")?.locked,
                _ => false,
            } } else { false };
            if locked { return Err(DomainError::new("ENTITY_LOCKED", "Affected ownership/semantic closure contains a locked entity")); }
            if let Some(owner) = entity.owner_id { queue.push_back(owner); }
            queue.extend(entity.references.into_iter().filter(|r| r.role == "semantic_input" || r.role == "link").map(|r| r.target));
        }
        queue.extend(read.dependents(&id, MAX_CLOSURE)?);
    }
    Ok(())
}
pub fn validate_patch_clips<R: ProjectRead>(read: &R, patch: &ProjectPatch) -> Result<()> {
    let edit = Edit { read, changes: patch.changes.iter().map(|c| (c.id.clone(), c.clone())).collect(), sources: Vec::new() };
    let ids: BTreeSet<_> = patch.changes.iter().map(|c| c.id.clone()).collect();
    let mut after_clips = Vec::new();
    for change in &patch.changes {
        if let Some(raw) = &change.after {
            let entity: EntityEnvelope = serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
            if entity.kind == "clip" { let clip: Clip = entity.payload("clip")?; edit.clip_context(&clip)?; after_clips.push((entity.id, clip)); }
        }
        if let Some(raw) = &change.before {
            let entity: EntityEnvelope = serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
            // Undo cannot bypass a lock in the currently captured state.
            match entity.kind.as_str() {
                "clip" => { let c: Clip = entity.payload("clip")?; if c.locked { return Err(DomainError::new("ENTITY_LOCKED", "Clip is locked")); } }
                "track" => { let t: Track = entity.payload("track")?; if t.locked { return Err(DomainError::new("ENTITY_LOCKED", "Track is locked")); } }
                "sequence" => { let s: Sequence = entity.payload("sequence")?; if s.locked { return Err(DomainError::new("ENTITY_LOCKED", "Sequence is locked")); } }
                "bin" => { let b: Bin = entity.payload("bin")?; if b.locked { return Err(DomainError::new("ENTITY_LOCKED", "Bin is locked")); } }
                _ => {}
            }
        }
    }
    for (id, clip) in &after_clips {
        for other in read.intersecting(&clip.sequence_id, &clip.track_id, clip.sequence_start, clip.end()?, MAX_CHANGES + 1)? {
            if !ids.contains(&other.id) { return Err(DomainError::new("CLIP_COLLISION", "Placement overlaps an existing clip on this track")); }
        }
        for (other_id, other) in &after_clips {
            if id != other_id && clip.track_id == other.track_id && clip.sequence_start < other.end()? && clip.end()? > other.sequence_start { return Err(DomainError::new("CLIP_COLLISION", "Edited clips overlap on the same track")); }
        }
    }
    Ok(())
}
