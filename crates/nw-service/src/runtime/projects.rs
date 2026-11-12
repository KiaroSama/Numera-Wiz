//! Authoritative project edits share current policy, replay-first acceptance and the receipt kernel.
use super::*;
use nw_domain::projects::{
    self as domain,
    edit::{
        AllocatedIds, ProjectEditIntent, check_compatibility, resolve_edit, validate_patch_clips,
    },
    *,
};
use nw_store::projects::{HistoryAction, ProjectCommit, ProjectReadSnapshot};
use std::collections::{BTreeMap, VecDeque};

struct HeldSource {
    asset: Asset,
    stamp: files::SourceStamp,
    record_hash: String,
}
impl Service {
    pub(super) fn read_project(
        &self,
        context: &Context,
        id: &str,
        action: ObjectAction,
    ) -> Result<ProjectReadSnapshot> {
        validate_id(id)?;
        let creator = self.store.project_creator(id)?;
        self.object_action(context, ObjectKind::Project, id, &creator, action)?;
        let read = self.store.project_read(id)?;
        self.object_action(
            context,
            ObjectKind::Project,
            id,
            &read.header().creator,
            action,
        )?;
        Ok(read)
    }
    pub(super) fn project_receipt_readable(
        &self,
        context: &Context,
        response: &Response,
    ) -> Result<bool> {
        let id = response
            .result
            .as_ref()
            .and_then(|v| v["project_id"].as_str())
            .ok_or_else(|| DomainError::new("STORAGE_ERROR", "Project receipt identity missing"))?;
        match self.read_project(context, id, ObjectAction::Read) {
            Ok(_) => Ok(true),
            Err(error) if error.code == "OBJECT_NOT_FOUND" => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn registered_project_source(
        &self,
        context: &Context,
        asset_id: &str,
        index: u32,
        cancel: &CancellationToken,
    ) -> Result<(SourceBinding, HeldSource)> {
        let key = named("asset", asset_id)?;
        let asset: Asset = self
            .store
            .get(&key)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object(context, &asset.id, &asset.creator)?;
        let path = self.path(context, Path::new(&asset.identity.path), false)?;
        if path != Path::new(&asset.identity.path) {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Registered source path no longer resolves to its frozen identity",
            ));
        }
        let stream = asset
            .probe
            .streams
            .iter()
            .find(|s| s.index == index)
            .ok_or_else(|| domain::invalid("Physical source stream index is absent"))?;
        let kind = match stream.kind {
            nw_domain::mux::StreamKind::Video if !stream.attached_picture() => TrackKind::Video,
            nw_domain::mux::StreamKind::Audio => TrackKind::Audio,
            _ => {
                return Err(domain::invalid(
                    "Project placement requires a physical video/audio stream",
                ));
            }
        };
        let record_hash = self.store.project_fact_hash(&key)?;
        let stamp = files::SourceStamp::capture_resolved(&path)?;
        if let Some(object) = &asset.source_object {
            stamp.require_object(object)?;
        }
        let expected_bytes = asset
            .identity
            .bytes
            .parse::<u64>()
            .map_err(|_| domain::invalid("Registered source size is invalid"))?;
        if expected_bytes.to_string() != asset.identity.bytes {
            return Err(domain::invalid("Registered source size is not canonical"));
        }
        #[cfg(test)]
        self.preflight_started();
        let identity = stamp.identity_with_limit(cancel, expected_bytes)?;
        if identity != asset.identity {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Held registered source bytes differ",
            ));
        }
        stamp.revalidate()?;
        let source = SourceBinding::Registered {
            asset_id: asset.id.clone(),
            content_sha256: asset.identity.sha256.clone(),
            probe_sha256: domain::hash(
                &serde_json::to_vec(&asset.probe).map_err(|e| domain::invalid(e.to_string()))?,
            ),
            stream_index: index,
            media_kind: kind,
            extent_verification: "pending".into(),
        };
        Ok((
            source,
            HeldSource {
                asset,
                stamp,
                record_hash,
            },
        ))
    }
    pub(super) fn project_command(
        self: &Arc<Self>,
        context: &Context,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if ["projects.inspect", "sequences.inspect"].contains(&request.command.id.as_str()) {
            return self.project_query(context, request, cancel);
        }
        // Dispatch released its acceptance mutex. Every attempt checks replay again before IDs/preflight.
        let initial_guard = self.mutation.lock().map_err(|_| lock_error())?;
        if let Some(replay) = self.refresh_acceptance(context, request, cancel, &initial_guard)? {
            return Ok(replay);
        }
        if request.command.id == "projects.create" {
            let input: CreateInput = commands::parse(request.params.clone())?;
            label(&input.name)?;
            let project = ProjectId::parse(Uuid::new_v4().to_string())?;
            let root = EntityId::parse(Uuid::new_v4().to_string())?;
            let lease = nw_platform::project_lease::ProjectWriterLease::acquire(&project)?;
            let header = ProjectHeader {
                envelope_version: 1,
                schema_major: 1,
                id: project.clone(),
                creator: context.principal.clone(),
                name: input.name,
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
            let root_entity = EntityEnvelope::new(
                &project,
                root.clone(),
                "bin",
                None,
                "Root".into(),
                &Bin {
                    member_count: 0,
                    locked: false,
                },
            )?;
            let prepared = ProjectCommit {
                header,
                expected_header: None,
                patch: ProjectPatch {
                    changes: vec![EntityChange {
                        id: root,
                        before: None,
                        after: Some(root_entity.bytes()?),
                    }],
                },
                operation_id: OperationId::parse(Uuid::new_v4().to_string())?,
                history_action: HistoryAction::Normal,
                created: json!({}),
                expected_records: Vec::new(),
            };
            self.project_snapshot_pressure(MAX_PATCH_BYTES)?;
            if cancel.is_cancelled() {
                return Err(DomainError::new(
                    "CANCELLED",
                    "Project creation cancelled before acceptance",
                ));
            }
            let response = self.store.commit_project_with_clock(
                &context.principal,
                request,
                prepared,
                || self.clock.now(),
            )?;
            self.project_writers
                .lock()
                .map_err(|_| lock_error())?
                .insert(project.to_string(), lease);
            tracing::info!(event="project.created", request_id=%request.request_id, project_id=%project, revision="0", receipt_id=?response.receipt_id, "Project creation committed");
            return Ok(response);
        }
        let reference = request
            .project
            .as_ref()
            .ok_or_else(|| domain::invalid("Project context required"))?;
        let read = self.read_project(context, &reference.id, ObjectAction::Edit)?;
        let expected = Revision::parse(
            reference
                .expected_revision
                .as_deref()
                .ok_or_else(|| domain::invalid("Expected revision required"))?,
        )?;
        if read.header().revision != expected {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Expected revision differs from committed project",
            ));
        }
        read.header().writable()?;
        let fresh_lease = if self
            .project_writers
            .lock()
            .map_err(|_| lock_error())?
            .contains_key(&reference.id)
        {
            None
        } else {
            Some(nw_platform::project_lease::ProjectWriterLease::acquire(
                &read.header().id,
            )?)
        };
        drop(read);
        drop(initial_guard);
        // Expensive source reads own held file handles, never a long-lived database transaction.
        // Preserve preparation errors until replay is rechecked under the acceptance mutex.
        let preparation = (|| -> Result<_> {
            let mut held = Vec::new();
            let source_permit = if request.command.id == "clips.place" {
                Some(
                    self.batch_preflights
                        .clone()
                        .try_acquire_owned()
                        .map_err(|_| {
                            domain::limit("Source preflight capacity is busy; retry this request")
                        })?,
                )
            } else {
                None
            };
            let history_command =
                ["history.undo", "history.redo"].contains(&request.command.id.as_str());
            let intent = if history_command {
                let _: Empty = commands::parse(request.params.clone())?;
                None
            } else {
                Some(match request.command.id.as_str() {
                    "sequences.create" => {
                        ProjectEditIntent::Sequence(commands::parse(request.params.clone())?)
                    }
                    "clips.place" => {
                        let input: PlaceInput = commands::parse(request.params.clone())?;
                        let source = match &input.source {
                            PlaceSource::SolidColor { rgba, duration } => {
                                SourceBinding::SolidColor {
                                    rgba: *rgba,
                                    duration: *duration,
                                }
                            }
                            PlaceSource::Registered {
                                asset_id,
                                stream_index,
                            } => {
                                let (source, source_held) = self.registered_project_source(
                                    context,
                                    asset_id,
                                    *stream_index,
                                    cancel,
                                )?;
                                held.push(source_held);
                                source
                            }
                        };
                        if let Some(held_source) = held.last() {
                            validate_declared_source_extent(
                                &held_source.asset,
                                &input.source_range,
                                &input.source,
                            )?;
                        }
                        ProjectEditIntent::Place(Box::new(input), source)
                    }
                    "clips.move" => {
                        ProjectEditIntent::Move(commands::parse(request.params.clone())?)
                    }
                    "clips.link" => {
                        ProjectEditIntent::Link(commands::parse(request.params.clone())?)
                    }
                    "clips.split" => {
                        ProjectEditIntent::Split(commands::parse(request.params.clone())?)
                    }
                    "clips.remove" => {
                        ProjectEditIntent::Remove(commands::parse(request.params.clone())?)
                    }
                    _ => {
                        return Err(DomainError::new(
                            "UNKNOWN_COMMAND",
                            "No project edit handler",
                        ));
                    }
                })
            };
            let read = self.read_project(context, &reference.id, ObjectAction::Edit)?;
            if read.header().revision != expected {
                return Err(DomainError::new(
                    "STALE_REVISION",
                    "Project changed during source preparation",
                ));
            }
            let (patch, created, history_action) = if let Some(intent) = intent {
                let id_budget = if matches!(&intent, ProjectEditIntent::Link(_)) { 1 } else { MAX_CHANGES * 2 };
                let ids = (0..id_budget)
                    .map(|_| EntityId::parse(Uuid::new_v4().to_string()))
                    .collect::<Result<VecDeque<_>>>()?;
                let resolved = resolve_edit(&read, intent, AllocatedIds(ids))?;
                (resolved.forward, resolved.created, HistoryAction::Normal)
            } else {
                let history = read.history()?;
                let undo = request.command.id == "history.undo";
                let entry = if undo {
                    history.undo.last()
                } else {
                    history.redo.last()
                }
                .ok_or_else(|| {
                    DomainError::new(
                        if undo {
                            "NOTHING_TO_UNDO"
                        } else {
                            "NOTHING_TO_REDO"
                        },
                        "No retained operation on this history stack",
                    )
                })?;
                let stored = read.patch(&entry.id)?;
                let patch = if undo { stored.inverse } else { stored.forward };
                patch.validate()?;
                check_compatibility(&read, &patch)?;
                validate_patch_clips(&read, &patch)?;
                (
                    patch,
                    json!({}),
                    if undo {
                        HistoryAction::Undo(entry.id.clone())
                    } else {
                        HistoryAction::Redo(entry.id.clone())
                    },
                )
            };
            let prepared = ProjectCommit {
                header: read.header().clone(),
                expected_header: Some(read.header_raw().to_owned()),
                patch,
                operation_id: OperationId::parse(Uuid::new_v4().to_string())?,
                history_action,
                created,
                expected_records: held
                    .iter()
                    .map(|source| {
                        (
                            format!("asset/{}", source.asset.id),
                            source.record_hash.clone(),
                        )
                    })
                    .collect(),
            };
            drop(read);
            Ok((prepared, held, source_permit))
        })();
        let guard = self.mutation.lock().map_err(|_| lock_error())?;
        if let Some(replay) = self.refresh_acceptance(context, request, cancel, &guard)? {
            return Ok(replay);
        }
        let (prepared, held, _source_permit) = preparation?;
        let current = self.read_project(context, &reference.id, ObjectAction::Edit)?;
        if current.header().revision != expected {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Project changed during preparation",
            ));
        }
        drop(current);
        for source in &held {
            self.object(context, &source.asset.id, &source.asset.creator)?;
            self.root_authority(context, Path::new(&source.asset.identity.path), false)?;
            if self.path(context, Path::new(&source.asset.identity.path), false)?
                != Path::new(&source.asset.identity.path)
            {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source path changed during preparation",
                ));
            }
            source.stamp.revalidate()?;
        }
        self.project_snapshot_pressure(
            serde_json::to_vec(&prepared.patch)
                .map_err(|e| domain::invalid(e.to_string()))?
                .len(),
        )?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Project edit cancelled before acceptance",
            ));
        }
        let response =
            self.store
                .commit_project_with_clock(&context.principal, request, prepared, || {
                    self.clock.now()
                })?;
        if let Some(lease) = fresh_lease {
            self.project_writers
                .lock()
                .map_err(|_| lock_error())?
                .insert(reference.id.clone(), lease);
        }
        if let Some(result) = &response.result {
            tracing::info!(event="project.edit.accepted", request_id=%request.request_id, project_id=%reference.id, command=%request.command.id, base_revision=%result["base_revision"].as_str().unwrap_or(""), revision=%result["revision"].as_str().unwrap_or(""), operation_id=%result["operation_id"].as_str().unwrap_or(""), receipt_id=?response.receipt_id, "Project edit committed");
        }
        Ok(response)
    }
}

#[cfg(test)]
#[path = "projects/tests.rs"]
pub(super) mod tests;

// Probe-declared bounds are additional validation, never a claim of complete decoded coverage.
fn validate_declared_source_extent(
    asset: &Asset,
    range: &SourceSpan,
    source: &PlaceSource,
) -> Result<()> {
    let PlaceSource::Registered { stream_index, .. } = source else {
        return Ok(());
    };
    let Some(details) = asset.probe.stream_details.get(stream_index) else {
        return Ok(());
    };
    let integer = |name: &str| {
        details[name]
            .as_i64()
            .or_else(|| details[name].as_str().and_then(|v| v.parse::<i64>().ok()))
    };
    let Some((numerator, denominator)) = details["time_base"]
        .as_str()
        .and_then(|s| s.split_once('/'))
    else {
        return Ok(());
    };
    let (Ok(numerator), Ok(denominator)) = (numerator.parse::<i64>(), denominator.parse::<i64>())
    else {
        return Ok(());
    };
    let (Some(start), Some(duration)) = (integer("start_pts"), integer("duration_ts")) else {
        return Ok(());
    };
    if duration <= 0 {
        return Ok(());
    }
    let grid = nw_domain::time::Rational::new(numerator, denominator)?;
    if grid <= nw_domain::time::Rational::ZERO {
        return Err(domain::invalid("Declared stream time base is not positive"));
    }
    let start = nw_domain::time::Rational::new(start, 1)?.checked_mul(grid)?;
    let end = start.checked_add(nw_domain::time::Rational::new(duration, 1)?.checked_mul(grid)?)?;
    if range.start < start || range.end > end {
        return Err(domain::invalid(
            "Requested source span exceeds established probe-declared bounds",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "projects/regression_tests.rs"]
pub(super) mod regression_tests;
