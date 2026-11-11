//! One absolute-lifetime MVCC ticket serves bounded, authenticated project projections.
use super::*;
use nw_domain::projects::{
    self as domain,
    edit::ProjectRead,
    views::{InspectInput, SequenceInspectInput},
    *,
};
use nw_store::projects::ProjectReadSnapshot;
use std::collections::BTreeMap;

pub(super) struct ProjectTicket {
    principal: String,
    profile: Option<String>,
    policy_revision: u64,
    project_id: String,
    project_revision: Revision,
    read: ProjectReadSnapshot,
    created: Instant,
    expires_at: String,
    secret: String,
    query_bindings: BTreeMap<String, String>,
    generation_bytes: u64,
}
impl ProjectTicket {
    pub(super) fn expired(&self) -> bool {
        self.created.elapsed() >= LEASE
    }
    pub(super) fn principal(&self) -> &str {
        &self.principal
    }
    pub(super) fn bytes(&self) -> usize {
        8192 + self.read.header_raw().len() * 2
            + self.query_bindings.values().map(String::len).sum::<usize>()
    }
    fn mac(&self, payload: &str) -> String {
        nw_store::projects::cursor_mac(self.secret.as_bytes(), payload.as_bytes())
    }
    fn cursor(&self, id: &str, query: &str, key: &str, count: usize) -> String {
        let payload = format!("{id}:{query}:{}:{count}", hex::encode(key.as_bytes()));
        format!("{payload}:{}", self.mac(&payload))
    }
}
struct Query {
    projection: String,
    revision: Option<Revision>,
    replacement: Option<String>,
    cursor: Option<String>,
    limit: usize,
    entity: Option<EntityId>,
    bin: Option<EntityId>,
    sequence: Option<EntityId>,
    tracks: Vec<EntityId>,
    range: Option<nw_domain::time::TimeRange>,
    binding: String,
}
impl Query {
    fn parse(request: &Request) -> Result<Self> {
        let mut query = if request.command.id == "projects.inspect" {
            let input: InspectInput = commands::parse(request.params.clone())?;
            if input.sort != "label" {
                return Err(domain::invalid("Project bin sort is label then ID"));
            }
            match input.projection.as_str() {
                "summary"
                    if input.entity_id.is_none()
                        && input.bin_id.is_none()
                        && input.cursor.is_none() => {}
                "entity"
                    if input.entity_id.is_some()
                        && input.bin_id.is_none()
                        && input.cursor.is_none() => {}
                "bin_page" if input.bin_id.is_some() && input.entity_id.is_none() => {}
                _ => {
                    return Err(domain::invalid(
                        "Invalid project projection or projection-specific fields",
                    ));
                }
            }
            Self {
                projection: input.projection,
                revision: input.revision,
                replacement: input.replace_snapshot,
                cursor: input.cursor,
                limit: input.limit,
                entity: input.entity_id,
                bin: input.bin_id,
                sequence: None,
                tracks: Vec::new(),
                range: None,
                binding: String::new(),
            }
        } else {
            let input: SequenceInspectInput = commands::parse(request.params.clone())?;
            match input.projection.as_str() {
                "entity"
                    if input.track_ids.is_empty()
                        && input.range.is_none()
                        && input.cursor.is_none() => {}
                "track_page" if input.track_ids.is_empty() && input.range.is_none() => {}
                "viewport"
                    if !input.track_ids.is_empty()
                        && input.track_ids.len() <= 64
                        && input.range.is_some() => {}
                _ => {
                    return Err(domain::invalid(
                        "Invalid sequence projection or projection-specific fields",
                    ));
                }
            }
            if let Some(range) = &input.range {
                range.validate()?;
            }
            let mut tracks = input.track_ids;
            tracks.sort();
            if tracks.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(domain::invalid("Duplicate viewport tracks"));
            }
            Self {
                projection: input.projection,
                revision: input.revision,
                replacement: input.replace_snapshot,
                cursor: input.cursor,
                limit: input.limit,
                entity: None,
                bin: None,
                sequence: Some(input.sequence_id),
                tracks,
                range: input.range,
                binding: String::new(),
            }
        };
        if !(1..=512).contains(&query.limit)
            || query.cursor.as_ref().is_some_and(|v| v.len() > 2048)
            || query
                .replacement
                .as_ref()
                .is_some_and(|v| validate_id(v).is_err())
            || (query.cursor.is_some() && query.replacement.is_some())
        {
            return Err(domain::invalid(
                "Invalid project page size, cursor or replacement",
            ));
        }
        query.binding = serde_json::to_string(&json!({"command":request.command.id,"projection":query.projection,"limit":query.limit,"entity":query.entity,"bin":query.bin,"sequence":query.sequence,"tracks":query.tracks,"range":query.range,"sort":"label_or_track_order_exact_grid"})).map_err(|e| domain::invalid(e.to_string()))?;
        if query.binding.len() > 8192 {
            return Err(domain::limit("Project query binding exceeds 8 KiB"));
        }
        Ok(query)
    }
    fn render(
        &self,
        ticket: &ProjectTicket,
        id: &str,
        binding: &str,
        after: Option<&str>,
        prior_count: usize,
    ) -> Result<Value> {
        let read = &ticket.read;
        let header = read.header();
        let mut result = json!({"schema_version":1,"project_revision":ticket.project_revision,"snapshot_id":id,"snapshot_expires_at":ticket.expires_at,"projection":self.projection,"event_cursor":null});
        match self.projection.as_str() {
            "summary" => {
                result["project_id"] = json!(header.id);
                result["root_bin_id"] = json!(header.root_bin_id);
                result["name"] = json!(header.name);
                result["entity_count"] = json!(header.entity_count);
                result["sequence_count"] = json!(header.sequence_count);
                result["schema_major"] = json!(header.schema_major);
                result["envelope_version"] = json!(header.envelope_version);
                result["compatibility"] = json!(if header.writable().is_ok() {
                    "supported"
                } else {
                    "read_only_unsupported"
                });
                result["history"] = read.history_summary()?;
            }
            "entity" => {
                let entity_id = self
                    .entity
                    .as_ref()
                    .or(self.sequence.as_ref())
                    .ok_or_else(|| domain::invalid("Entity identity required"))?;
                let entity = read.entity(entity_id)?;
                if self.sequence.is_some() && entity.kind != "sequence" {
                    return Err(DomainError::object_not_found());
                }
                result["entity"] = entity_projection(&entity)?;
            }
            _ => {
                let page = match self.projection.as_str() {
                    "bin_page" => read.bin_page(
                        self.bin
                            .as_ref()
                            .ok_or_else(|| domain::invalid("Bin identity required"))?,
                        after,
                        self.limit,
                        MAX_PAGE_BYTES - 8192,
                    )?,
                    "track_page" => read.track_page(
                        self.sequence
                            .as_ref()
                            .ok_or_else(|| domain::invalid("Sequence identity required"))?,
                        after,
                        self.limit,
                        MAX_PAGE_BYTES - 8192,
                    )?,
                    "viewport" => read.clip_range(
                        self.sequence
                            .as_ref()
                            .ok_or_else(|| domain::invalid("Sequence identity required"))?,
                        &self.tracks,
                        self.range
                            .ok_or_else(|| domain::invalid("Viewport range required"))?,
                        after,
                        self.limit,
                        MAX_PAGE_BYTES - 8192,
                    )?,
                    _ => return Err(domain::invalid("Unknown project projection")),
                };
                let count = prior_count
                    .checked_add(page.items.len())
                    .ok_or_else(|| domain::limit("Cursor item count overflow"))?;
                result["items"] = json!(page.items);
                result["next_cursor"] = json!(
                    page.next_key
                        .as_ref()
                        .map(|key| ticket.cursor(id, binding, key, count))
                );
                result["total_count"] = json!(if self.projection == "viewport" {
                    count
                } else {
                    page.total_count
                });
                result["count_accuracy"] = json!(page.count_accuracy);
            }
        }
        if serde_json::to_vec(&result)
            .map_err(|e| domain::invalid(e.to_string()))?
            .len()
            > MAX_PAGE_BYTES
        {
            return Err(domain::limit(
                "Detail section exceeds the bounded project response budget",
            ));
        }
        Ok(result)
    }
}
fn entity_projection(entity: &EntityEnvelope) -> Result<Value> {
    let mut result = json!({"id":entity.id,"kind":entity.kind,"kind_version":entity.kind_version,"envelope_version":entity.envelope_version,"owner_id":entity.owner_id,"enabled":entity.enabled,"references":entity.references,"semantic_scope":entity.semantic_scope,"name":entity.label,"payload_sha256":entity.payload_sha256,"compatibility":if entity.semantics().is_ok() { "supported_or_optional_annotation" } else { "read_only_affected_scope" }});
    let payload = if entity.envelope_version == 1 && entity.kind_version == 1 {
        match entity.kind.as_str() {
            "clip" => value(&entity.payload::<Clip>("clip")?)?,
            "sequence" => value(&entity.payload::<Sequence>("sequence")?)?,
            "track" => value(&entity.payload::<Track>("track")?)?,
            "bin" => value(&entity.payload::<Bin>("bin")?)?,
            "source_binding" => {
                let mut value = value(&entity.payload::<SourceBinding>("source_binding")?)?;
                if let Some(kind) = value.as_object_mut().and_then(|v| v.remove("kind")) {
                    value["source_kind"] = kind;
                }
                value
            }
            _ => json!({"payload_json":entity.payload_json}),
        }
    } else {
        json!({"payload_json":entity.payload_json})
    };
    if let Some(fields) = payload.as_object() {
        result
            .as_object_mut()
            .expect("Object projection")
            .extend(fields.clone());
    }
    Ok(result)
}
impl Service {
    pub(in crate::runtime) fn project_query(
        &self,
        context: &Context,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        let reference = request
            .project
            .as_ref()
            .ok_or_else(|| domain::invalid("Project query context required"))?;
        let query = Query::parse(request)?;
        let current = self.read_project(context, &reference.id, ObjectAction::Read)?;
        let policy_revision = self.policy(context)?.as_ref().map_or(0, |p| p.revision);
        let mut cache = self
            .snapshots
            .try_lock()
            .map_err(|_| domain::limit("Snapshot service is busy; retry the query"))?;
        cache.expire();
        let binding = domain::hash(query.binding.as_bytes());
        if let Some(cursor) = &query.cursor {
            let parts: Vec<_> = cursor.split(':').collect();
            if parts.len() != 5 {
                return Err(expired());
            }
            let id = parts[0];
            let ticket = cache.projects.get(id).ok_or_else(expired)?;
            let payload = parts[..4].join(":");
            if ticket.principal != context.principal
                || ticket.profile != context.profile
                || ticket.policy_revision != policy_revision
                || ticket.project_id != reference.id
                || parts[1] != binding
                || ticket.query_bindings.get(&binding) != Some(&query.binding)
                || ticket.mac(&payload) != parts[4]
                || query.revision.is_some_and(|r| r != ticket.project_revision)
            {
                return Err(expired());
            }
            let key = String::from_utf8(hex::decode(parts[2]).map_err(|_| expired())?)
                .map_err(|_| expired())?;
            let count = parts[3].parse::<usize>().map_err(|_| expired())?;
            if count > MAX_ENTITIES {
                return Err(expired());
            }
            let mut result = query.render(ticket, id, &binding, Some(&key), count)?;
            self.project_writer_projection(&reference.id, &query.projection, &mut result);
            if ticket.expired() {
                return Err(expired());
            }
            self.recheck_project_query(
                context,
                request,
                current.header(),
                policy_revision,
                cancel,
            )?;
            return Ok(Response::success(&request.request_id, result));
        }
        if let Some(id) = &query.replacement
            && cache.projects.get(id).is_some_and(|ticket| {
                ticket.principal == context.principal
                    && ticket.profile == context.profile
                    && ticket.project_id == reference.id
            })
        {
            cache.projects.remove(id);
        }
        let desired = query.revision.unwrap_or(current.header().revision);
        let existing = cache
            .projects
            .iter()
            .find(|(_, ticket)| {
                ticket.principal == context.principal
                    && ticket.profile == context.profile
                    && ticket.policy_revision == policy_revision
                    && ticket.project_id == reference.id
                    && ticket.project_revision == desired
            })
            .map(|(id, _)| id.clone());
        if let Some(id) = existing {
            if cache.bytes() + query.binding.len() + MAX_PAGE_BYTES > MAX_GLOBAL_BYTES {
                return Err(domain::limit("Global snapshot metadata budget reached"));
            }
            let ticket = cache.projects.get_mut(&id).ok_or_else(expired)?;
            if ticket.query_bindings.len() >= 8 && !ticket.query_bindings.contains_key(&binding) {
                return Err(domain::limit(
                    "One project snapshot supports at most eight query bindings",
                ));
            }
            let mut result = query.render(ticket, &id, &binding, None, 0)?;
            self.project_writer_projection(&reference.id, &query.projection, &mut result);
            if ticket.expired() {
                return Err(expired());
            }
            self.recheck_project_query(
                context,
                request,
                current.header(),
                policy_revision,
                cancel,
            )?;
            ticket.query_bindings.insert(binding, query.binding);
            return Ok(Response::success(&request.request_id, result));
        }
        if desired != current.header().revision {
            return Err(DomainError::new(
                "REVISION_NOT_RETAINED",
                "Requested project revision has no live captured ticket",
            ));
        }
        if cache.count() >= MAX_SNAPSHOTS
            || cache.principal_count(&context.principal) >= MAX_PER_PRINCIPAL
        {
            return Err(domain::limit(
                "Active snapshot lease quota reached; replace or reuse a captured project snapshot",
            ));
        }
        let id = Uuid::new_v4().to_string();
        let ticket = ProjectTicket {
            principal: context.principal.clone(),
            profile: context.profile.clone(),
            policy_revision,
            project_id: reference.id.clone(),
            project_revision: desired,
            read: current,
            created: Instant::now(),
            expires_at: self
                .clock
                .now()?
                .checked_add(30)
                .ok_or_else(expired)?
                .to_string(),
            secret: Uuid::new_v4().to_string(),
            query_bindings: BTreeMap::from([(binding.clone(), query.binding.clone())]),
            generation_bytes: self.store.project_generation_bytes()?,
        };
        if cache.bytes() + ticket.bytes() + MAX_PAGE_BYTES > MAX_GLOBAL_BYTES {
            return Err(domain::limit(
                "Global snapshot serialization/metadata budget reached",
            ));
        }
        let mut result = query.render(&ticket, &id, &binding, None, 0)?;
        self.project_writer_projection(&reference.id, &query.projection, &mut result);
        if ticket.expired() {
            return Err(expired());
        }
        self.recheck_project_query(
            context,
            request,
            ticket.read.header(),
            policy_revision,
            cancel,
        )?;
        cache.projects.insert(id, ticket);
        Ok(Response::success(&request.request_id, result))
    }
    fn project_writer_projection(&self, project: &str, projection: &str, result: &mut Value) {
        if projection != "summary" {
            return;
        }
        let owned = self
            .project_writers
            .lock()
            .is_ok_and(|writers| writers.contains_key(project));
        let status = if owned {
            "this_owner"
        } else {
            match ProjectId::parse(project)
                .and_then(|id| nw_platform::project_lease::ProjectWriterLease::acquire(&id))
            {
                Ok(lease) => {
                    drop(lease);
                    "available"
                }
                Err(error) if error.code == "PROJECT_WRITER_BUSY" => "other_owner_read_only",
                Err(_) => "writer_lease_unavailable_read_only",
            }
        };
        result["writer_status"] = json!(status);
        result["read_only"] =
            json!(status.ends_with("read_only") || result["compatibility"] != "supported");
    }
    fn recheck_project_query(
        &self,
        context: &Context,
        request: &Request,
        header: &ProjectHeader,
        policy_revision: u64,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Project query cancelled"));
        }
        let policy = self.policy(context)?;
        if policy.as_ref().map_or(0, |p| p.revision) != policy_revision
            || policy
                .as_ref()
                .is_some_and(|p| !p.allows_command(&request.command.id))
        {
            return Err(expired());
        }
        self.object_action(
            context,
            ObjectKind::Project,
            header.id.as_str(),
            &header.creator,
            ObjectAction::Read,
        )
    }
    pub(in crate::runtime) fn project_snapshot_pressure(
        &self,
        projected_write_bytes: usize,
    ) -> Result<()> {
        let mut cache = self.snapshots.lock().map_err(|_| lock_error())?;
        cache.expire();
        if cache.projects.is_empty() {
            cache.project_write_estimate = 0;
            return Ok(());
        }
        // This conservative software estimate and observed growth are not a hard instantaneous filesystem cap.
        if projected_write_bytes > 0 {
            cache.project_write_estimate = cache
                .project_write_estimate
                .saturating_add(projected_write_bytes.saturating_mul(8) + 64 * 1024);
        }
        let growth = self.store.project_generation_bytes()?.saturating_sub(
            cache
                .projects
                .values()
                .map(|p| p.generation_bytes)
                .min()
                .unwrap_or(0),
        );
        if cache.project_write_estimate > 32 * 1024 * 1024 || growth > 32 * 1024 * 1024 {
            cache.clear_project_reads();
        }
        Ok(())
    }
    pub(in crate::runtime) fn start_snapshot_reaper(service: &Arc<Self>) {
        let weak = Arc::downgrade(service);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(service) = weak.upgrade() else {
                    break;
                };
                if let Ok(mut snapshots) = service.snapshots.try_lock() {
                    snapshots.expire();
                }
                if let Err(error) = service.project_snapshot_pressure(0) {
                    tracing::debug!(code=%error.code, "Project snapshot pressure observation unavailable");
                }
                // The upgraded owner is dropped before sleeping; an idle query lease cannot keep it alive.
            }
        });
    }
}

#[cfg(test)]
pub(super) async fn exercise_real_snapshot_lifetime() {
    use nw_domain::commands::ProjectRef;
    let temporary = tempfile::tempdir().unwrap();
    let service = Service::open(temporary.path().join("data"), None).await.unwrap();
    let context = service.owner();
    let scope = service.store.issue_scope_with_clock(&context.principal, || service.clock.now()).unwrap();
    let mut create = Request::query("projects.create", json!({"name":"Actual MVCC lifetime"}));
    create.replay_scope = Some(scope.clone());
    create.idempotency_key = Some("cow-create".into());
    let created = service.dispatch(context.clone(),create).await;
    assert!(created.ok,"{:?}",created.error);
    let created = created.result.unwrap();
    let project = created["project_id"].as_str().unwrap();
    let bin = created["root_bin_id"].as_str().unwrap();
    let sequence = |revision: u64| {
        let mut request = Request::query("sequences.create",json!({"name":format!("Sequence {revision}"),"settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"}]}));
        request.project = Some(ProjectRef { id:project.into(), expected_revision:Some(revision.to_string()) });
        request.replay_scope = Some(scope.clone());
        request.idempotency_key = Some(format!("cow-sequence-{revision}"));
        request
    };
    for revision in 0..2 {
        let response = service.dispatch(context.clone(),sequence(revision)).await;
        assert!(response.ok,"{:?}",response.error);
    }
    let blob = "x".repeat(512 * 1024);
    service.store.put("unit/project-cow-churn",&json!({"iteration":0,"bytes":blob})).unwrap();
    let page_request = |cursor: Option<Value>| {
        let mut request = Request::query("projects.inspect",json!({"projection":"bin_page","bin_id":bin,"limit":1,"cursor":cursor}));
        request.project = Some(ProjectRef { id:project.into(), expected_revision:None });
        request
    };
    let first = service.dispatch(context.clone(),page_request(None)).await;
    assert!(first.ok,"{:?}",first.error);
    let first = first.result.unwrap();
    let ticket = first["snapshot_id"].as_str().unwrap().to_owned();
    assert_eq!(service.store.close_generation(&service.root_lease).unwrap_err().code,"OWNER_BUSY");
    let moved = service.dispatch(context.clone(),sequence(2)).await;
    assert!(moved.ok,"{:?}",moved.error);
    let old_page = service.dispatch(context.clone(),page_request(Some(first["next_cursor"].clone()))).await;
    assert!(old_page.ok,"{:?}",old_page.error);
    let old_page = old_page.result.unwrap();
    assert_eq!(old_page["project_revision"],"2");
    assert_eq!(old_page["total_count"],2);
    assert_eq!(service.store.project_read(project).unwrap().header().revision,Revision(3));
    let baseline = service.snapshots.lock().unwrap().projects[&ticket].generation_bytes;
    let growth_created = service.snapshots.lock().unwrap().projects[&ticket].created;
    let mut observed_growth = false;
    // Real ordinary writes retain old redb pages. No synthetic pressure argument is used.
    for iteration in 1..=128 {
        service.store.put("unit/project-cow-churn",&json!({"iteration":iteration,"bytes":blob})).unwrap();
        observed_growth |= service.store.project_generation_bytes().unwrap().saturating_sub(baseline) > 32 * 1024 * 1024;
        if observed_growth { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(observed_growth,"Write fixture must actually grow the retained generation beyond the observation threshold");
    tokio::time::timeout(Duration::from_secs(4),async {
        loop {
            if !service.snapshots.lock().unwrap().projects.contains_key(&ticket) { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await.expect("Background growth observation must release the actual read");
    assert!(growth_created.elapsed() < LEASE,"Growth expiry must precede the independent TTL limit");
    let stale = service.dispatch(context.clone(),page_request(Some(first["next_cursor"].clone()))).await;
    assert_eq!(stale.error.unwrap().code,"SNAPSHOT_EXPIRED");
    // Successful close proves the cache released the real database read lease.
    service.store.close_generation(&service.root_lease).unwrap();
    service.store.reopen_current_generation(&service.root_lease).unwrap();

    let fresh = service.dispatch(context.clone(),page_request(None)).await;
    assert!(fresh.ok,"{:?}",fresh.error);
    let fresh = fresh.result.unwrap();
    let ticket = fresh["snapshot_id"].as_str().unwrap().to_owned();
    let started = service.snapshots.lock().unwrap().projects[&ticket].created;
    assert_eq!(service.store.close_generation(&service.root_lease).unwrap_err().code,"OWNER_BUSY");
    let mut writes = 0u64;
    // Do not query or invoke pressure cleanup during this real absolute TTL interval.
    tokio::time::timeout(LEASE + Duration::from_secs(5),async {
        loop {
            service.store.put("unit/project-ttl-unrelated",&writes).unwrap();
            writes += 1;
            if !service.snapshots.lock().unwrap().projects.contains_key(&ticket) { break; }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }).await.expect("Background TTL reaper must run during unrelated writes");
    assert!(started.elapsed() >= LEASE);
    assert!(writes > 1);
    service.store.close_generation(&service.root_lease).unwrap();
    service.store.reopen_current_generation(&service.root_lease).unwrap();
    let stale = service.dispatch(context,page_request(Some(fresh["next_cursor"].clone()))).await;
    assert_eq!(stale.error.unwrap().code,"SNAPSHOT_EXPIRED");
}
