//! Authoritative project edits share current policy, replay-first acceptance and the receipt kernel.
use super::*;
use nw_domain::projects::{
    self as domain,
    edit::{
        AllocatedIds, ProjectEditIntent, ProjectRead, check_compatibility, resolve_edit,
        validate_patch_clips,
    },
    *,
};
use nw_store::projects::{HistoryAction, ProjectCommit, ProjectReadSnapshot};
use std::collections::{BTreeMap, VecDeque};

pub(super) struct HeldSource {
    pub(super) asset: Asset,
    pub(super) stamp: files::SourceStamp,
    pub(super) record_hash: String,
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
    pub(super) fn registered_project_source(
        &self,
        context: &Context,
        asset_id: &str,
        index: u32,
        cancel: &CancellationToken,
    ) -> Result<(SourceBinding, HeldSource)> {
        let key = named("asset", asset_id)?;
        let (asset, record_hash) = self
            .store
            .registered_asset_record(&key)?
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

    pub(super) fn export_binary_track(
        &self,
        context: &Context,
        command: &str,
        format: binary_interchange::Format,
        capture: &nw_domain::media::MixedCapture,
        track_id: &EntityId,
        cancel: &CancellationToken,
    ) -> Result<nw_domain::interchange::BinaryTrackExport> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use nw_domain::{
            interchange::{BinaryTrackExport, MAX_BINARY_BYTES},
            media::OccurrenceSource,
        };

        let track = capture
            .tracks
            .iter()
            .find(|track| track.id == *track_id)
            .ok_or_else(|| domain::invalid("Select one enabled track for binary interchange"))?;
        let _permit = self
            .batch_preflights
            .try_acquire()
            .map_err(|_| domain::limit("Source preflight capacity is busy; retry this request"))?;
        let stale_source =
            || DomainError::new("SOURCE_CHANGED", "Binary interchange source changed");
        let mut assets = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for clip in &track.clips {
            let OccurrenceSource::Registered {
                asset_id,
                stream_index,
                content_sha256,
                probe_sha256,
                media_kind,
                ..
            } = &clip.source
            else {
                return Err(domain::invalid(
                    "Binary interchange requires registered media",
                ));
            };
            let (binding, held) = match sources.entry((asset_id, *stream_index)) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let source =
                        self.registered_project_source(context, asset_id, *stream_index, cancel)?;
                    entry.insert(source)
                }
            };
            if !matches!(&*binding, SourceBinding::Registered {
                content_sha256: actual_content,
                probe_sha256: actual_probe,
                media_kind: actual_kind,
                ..
            } if actual_content == content_sha256 && actual_probe == probe_sha256 && actual_kind == media_kind)
            {
                return Err(stale_source());
            }
            assets
                .entry(asset_id.clone())
                .or_insert_with(|| held.asset.clone());
        }
        let output = binary_interchange::export(format, capture, track_id, &assets)?;
        if output.document.len() > MAX_BINARY_BYTES {
            return Err(domain::limit("Binary interchange document exceeds 160 KiB"));
        }

        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Binary interchange export cancelled",
            ));
        }
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(command))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Binary interchange grant changed",
            ));
        }
        let current =
            self.read_project(context, capture.project_id.as_str(), ObjectAction::Read)?;
        if current.header().revision != capture.project_revision {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Binary interchange project revision changed",
            ));
        }
        for (_, held) in sources.into_values() {
            self.object(context, &held.asset.id, &held.asset.creator)?;
            let path = Path::new(&held.asset.identity.path);
            if self.path(context, path, false)? != path {
                return Err(stale_source());
            }
            held.stamp.revalidate()?;
        }
        Ok(BinaryTrackExport {
            project_id: capture.project_id.clone(),
            sequence_id: capture.sequence_id.clone(),
            track_id: track_id.clone(),
            revision: capture.project_revision,
            range: capture.range,
            document_base64: STANDARD.encode(output.document),
            document_sha256: output.document_sha256,
            sources: output.sources,
        })
    }

    pub(super) fn wave_marker_projection(
        &self,
        context: &Context,
        request: &Request,
        input: wave_projection::Input,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        let project = &request
            .project
            .as_ref()
            .ok_or_else(|| domain::invalid("Wave cue projection requires a project context"))?
            .id;
        let read = self.read_project(context, project, ObjectAction::Read)?;
        let stale = || DomainError::new("STALE_REVISION", "Wave cue projection revision changed");
        if read.header().revision != input.revision {
            return Err(stale());
        }
        let project_id = read.header().id.clone();
        let clip: Clip = read.entity(&input.clip_id)?.payload("clip")?;
        let SourceBinding::Registered {
            asset_id,
            content_sha256,
            probe_sha256,
            stream_index,
            media_kind: TrackKind::Audio,
            ..
        } = read.entity(&clip.source_id)?.payload("source_binding")?
        else {
            return Err(domain::invalid("Choose a registered WAVE audio occurrence"));
        };
        drop(read);
        let _permit = self
            .batch_preflights
            .try_acquire()
            .map_err(|_| domain::limit("Source preflight capacity is busy; retry this request"))?;
        let (binding, held) =
            self.registered_project_source(context, &asset_id, stream_index, cancel)?;
        if !matches!(
            binding,
            SourceBinding::Registered {
                content_sha256: actual_content,
                probe_sha256: actual_probe,
                media_kind: TrackKind::Audio,
                ..
            } if actual_content == content_sha256 && actual_probe == probe_sha256
        ) {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "WAVE occurrence source changed",
            ));
        }
        let wave = nw_engine::inspect_wave_cues(&held.asset, &held.stamp, cancel)?;
        let cues = wave_projection::map_cues(&clip, &wave)?;

        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Wave cue projection cancelled",
            ));
        }
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Wave cue projection grant changed",
            ));
        }
        if self
            .read_project(context, project, ObjectAction::Read)?
            .header()
            .revision
            != input.revision
        {
            return Err(stale());
        }
        self.object(context, &held.asset.id, &held.asset.creator)?;
        let path = Path::new(&held.asset.identity.path);
        if self.path(context, path, false)? != path
            || self
                .store
                .project_fact_hash(&named("asset", &held.asset.id)?)?
                != held.record_hash
        {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Registered WAVE authority changed",
            ));
        }
        held.stamp.revalidate()?;
        let result = wave_projection::Projection {
            project_id,
            sequence_id: clip.sequence_id,
            clip_id: input.clip_id,
            occurrence_id: clip.occurrence_id,
            revision: input.revision,
            asset_id: wave.asset_id,
            source_sha256: wave.source_sha256,
            sample_rate_hz: wave.sample_rate_hz,
            cues,
        };
        Ok(Response::success(&request.request_id, value(&result)?).bounded())
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
            let source_permit = if matches!(
                request.command.id.as_str(),
                "clips.place"
                    | "clips.slip"
                    | "clips.roll"
                    | "clips.slide"
                    | "interchange.edl.import"
                    | "interchange.otio.import"
                    | "interchange.fcp7.import"
                    | "interchange.fcpxml.import"
                    | "interchange.aaf.import"
                    | "interchange.omf.import"
            ) {
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
                    "interchange.edl.import"
                    | "interchange.otio.import"
                    | "interchange.fcp7.import"
                    | "interchange.fcpxml.import"
                    | "interchange.aaf.import"
                    | "interchange.omf.import" => {
                        use nw_domain::interchange::{self, BinaryTrackImport, CutTrack};
                        let params = request.params.clone();
                        let command = request.command.id.as_str();
                        let (target, document_sha256, cuts) = if command == "interchange.edl.import"
                        {
                            let import = interchange::parse_cmx3600(commands::parse(params)?)?;
                            (
                                CutTrack::Edl(import.frames_per_second),
                                import.document_sha256,
                                import.cuts,
                            )
                        } else {
                            let (import, mismatch) = match command {
                                "interchange.fcp7.import" => (
                                    interchange::fcp7::import(commands::parse(params)?)?,
                                    "FCP7_TRACK_MISMATCH",
                                ),
                                "interchange.fcpxml.import" => (
                                    interchange::fcpxml::import(commands::parse(params)?)?,
                                    "FCPXML_TRACK_MISMATCH",
                                ),
                                "interchange.aaf.import" | "interchange.omf.import" => {
                                    let (format, mismatch) = if command == "interchange.aaf.import"
                                    {
                                        (binary_interchange::Format::Aaf, "AAF_TRACK_MISMATCH")
                                    } else {
                                        (binary_interchange::Format::Omf, "OMF_TRACK_MISMATCH")
                                    };
                                    let input: BinaryTrackImport = commands::parse(params)?;
                                    let bytes = interchange::decode_binary_document(
                                        &input.document_base64,
                                        interchange::MAX_BINARY_IMPORT_BYTES,
                                    )?;
                                    let import = binary_interchange::import(
                                        format,
                                        &bytes,
                                        &input.sequence_id,
                                        &input.track_id,
                                        input.start,
                                        &input.sources,
                                    )?;
                                    (import, mismatch)
                                }
                                _ => (
                                    interchange::parse_otio(commands::parse(params)?)?,
                                    "OTIO_TRACK_MISMATCH",
                                ),
                            };
                            (
                                CutTrack::Exact {
                                    kind: import.kind,
                                    range: import.range,
                                    mismatch,
                                },
                                import.document_sha256,
                                import.cuts,
                            )
                        };
                        let mut bindings = BTreeMap::new();
                        let mut placements = Vec::with_capacity(cuts.len());
                        for (input, expected_sha256) in cuts {
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
                                    let key = (asset_id.as_str(), *stream_index);
                                    let (binding, held_index) =
                                        match bindings.entry((key.0.to_owned(), key.1)) {
                                            std::collections::btree_map::Entry::Occupied(entry) => {
                                                entry.into_mut()
                                            }
                                            std::collections::btree_map::Entry::Vacant(entry) => {
                                                let (binding, source) = self
                                                    .registered_project_source(
                                                        context, key.0, key.1, cancel,
                                                    )?;
                                                let index = held.len();
                                                held.push(source);
                                                entry.insert((binding, index))
                                            }
                                        };
                                    let asset = &held[*held_index].asset;
                                    if expected_sha256.as_deref()
                                        != Some(asset.identity.sha256.as_str())
                                    {
                                        return Err(DomainError::new(
                                            "SOURCE_CHANGED",
                                            "Interchange source digest differs from the registered media",
                                        ));
                                    }
                                    validate_declared_source_extent(
                                        asset,
                                        &input.source_range,
                                        &input.source,
                                    )?;
                                    binding.clone()
                                }
                            };
                            placements.push((input, source));
                        }
                        ProjectEditIntent::ImportCuts {
                            target,
                            document_sha256,
                            placements,
                        }
                    }
                    "clips.move" => {
                        ProjectEditIntent::Move(commands::parse(request.params.clone())?)
                    }
                    "markers.place" => {
                        ProjectEditIntent::Marker(commands::parse(request.params.clone())?)
                    }
                    "markers.remove" => {
                        ProjectEditIntent::RemoveMarker(commands::parse(request.params.clone())?)
                    }
                    "clips.link" => {
                        ProjectEditIntent::Link(commands::parse(request.params.clone())?)
                    }
                    "clips.unlink" => {
                        ProjectEditIntent::Unlink(commands::parse(request.params.clone())?)
                    }
                    "clips.split" => {
                        ProjectEditIntent::Split(commands::parse(request.params.clone())?)
                    }
                    "clips.join_through_edit" => {
                        ProjectEditIntent::JoinThroughEdit(commands::parse(request.params.clone())?)
                    }
                    "clips.trim" => {
                        ProjectEditIntent::Trim(commands::parse(request.params.clone())?)
                    }
                    "clips.slip" => {
                        ProjectEditIntent::Slip(commands::parse(request.params.clone())?)
                    }
                    "clips.roll" => {
                        ProjectEditIntent::Roll(commands::parse(request.params.clone())?)
                    }
                    "clips.slide" => {
                        ProjectEditIntent::Slide(commands::parse(request.params.clone())?)
                    }
                    "timeline.remove_gap" => {
                        ProjectEditIntent::RemoveGap(commands::parse(request.params.clone())?)
                    }
                    "mixer.set_track_gain" => {
                        ProjectEditIntent::SetTrackGain(commands::parse(request.params.clone())?)
                    }
                    "tracks.set_locked" => {
                        ProjectEditIntent::SetTrackLocked(commands::parse(request.params.clone())?)
                    }
                    "tracks.set_enabled" => {
                        ProjectEditIntent::SetTrackEnabled(commands::parse(request.params.clone())?)
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
                let id_budget = match &intent {
                    ProjectEditIntent::Unlink(_)
                    | ProjectEditIntent::Trim(_)
                    | ProjectEditIntent::JoinThroughEdit(_)
                    | ProjectEditIntent::Slip(_)
                    | ProjectEditIntent::Roll(_)
                    | ProjectEditIntent::Slide(_)
                    | ProjectEditIntent::RemoveGap(_)
                    | ProjectEditIntent::SetTrackGain(_)
                    | ProjectEditIntent::SetTrackEnabled(_)
                    | ProjectEditIntent::SetTrackLocked(_) => 0,
                    ProjectEditIntent::Link(_) => 1,
                    ProjectEditIntent::ImportCuts { placements, .. } => placements.len() * 3,
                    _ => MAX_CHANGES * 2,
                };
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
            let header = read.header().clone();
            let expected_header = Some(read.header_raw().to_owned());
            let source_checks = if matches!(
                request.command.id.as_str(),
                "clips.slip" | "clips.roll" | "clips.slide"
            ) {
                source_window_checks(&read, &patch)?
            } else {
                Vec::new()
            };
            drop(read);
            if !source_checks.is_empty() {
                held = self.prepare_window_sources(context, source_checks, cancel)?;
            }
            let prepared = ProjectCommit {
                header,
                expected_header,
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

    fn prepare_window_sources(
        &self,
        context: &Context,
        checks: Vec<(SourceBinding, SourceSpan)>,
        cancel: &CancellationToken,
    ) -> Result<Vec<HeldSource>> {
        let mut held = BTreeMap::new();
        for (binding, range) in checks {
            let SourceBinding::Registered {
                asset_id,
                content_sha256,
                probe_sha256,
                stream_index,
                ..
            } = binding
            else {
                continue;
            };
            let (verified, source) = match held.entry((asset_id.clone(), stream_index)) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(
                    self.registered_project_source(context, &asset_id, stream_index, cancel)?,
                ),
            };
            if !matches!(
                verified,
                SourceBinding::Registered { content_sha256: content, probe_sha256: probe, .. }
                    if *content == content_sha256 && *probe == probe_sha256
            ) {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source differs from the committed clip binding",
                ));
            }
            let bounds =
                nw_domain::media::declared_relative_stream_window(&source.asset, stream_index)?
                    .ok_or_else(|| {
                        DomainError::new(
                            "SOURCE_EXTENT_UNAVAILABLE",
                            "Extending the source window requires a declared stream interval",
                        )
                    })?;
            if range.start < bounds.start || range.end > bounds.end {
                return Err(domain::invalid(
                    "The new source window exceeds the declared stream interval",
                ));
            }
        }
        Ok(held.into_values().map(|(_, source)| source).collect())
    }
}

fn source_window_checks(
    read: &ProjectReadSnapshot,
    patch: &ProjectPatch,
) -> Result<Vec<(SourceBinding, SourceSpan)>> {
    let mut checks = Vec::new();
    let mut streams = std::collections::BTreeSet::new();
    for change in &patch.changes {
        let parse_clip = |raw: &Option<String>| -> Result<Clip> {
            let raw = raw.as_deref().ok_or_else(|| {
                domain::invalid("A source-window edit must update an existing clip")
            })?;
            let entity: EntityEnvelope =
                serde_json::from_str(raw).map_err(|error| domain::invalid(error.to_string()))?;
            entity.payload("clip")
        };
        let before = parse_clip(&change.before)?;
        let clip = parse_clip(&change.after)?;
        if clip.source_range.start >= before.source_range.start
            && clip.source_range.end <= before.source_range.end
        {
            continue;
        }
        let bound: SourceBinding = read.entity(&clip.source_id)?.payload("source_binding")?;
        if let SourceBinding::Registered {
            asset_id,
            stream_index,
            ..
        } = &bound
        {
            streams.insert((asset_id.clone(), *stream_index));
            if streams.len() > 8 {
                return Err(domain::limit(
                    "A source-window edit can extend at most eight registered streams",
                ));
            }
            checks.push((bound, clip.source_range));
        }
    }
    Ok(checks)
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
