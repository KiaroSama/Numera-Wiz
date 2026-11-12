//! Short-lived, authorization-bound inventory snapshots containing summaries only.
use super::*;
#[cfg(test)]
#[path = "read_model/catalog_tests.rs"]
mod catalog_tests;
#[cfg(test)]
#[path = "read_model/real_producer_tests.rs"]
mod real_producer_tests;
#[path = "projects/queries.rs"]
mod project_queries;
use nw_domain::views::{AssetSummary, JobSummary, PageInput, SnapshotPage};
use std::time::{Duration, Instant};

const LEASE: Duration = Duration::from_secs(30);
const MAX_SNAPSHOTS: usize = 32;
const MAX_PER_PRINCIPAL: usize = 4;
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
const MAX_GLOBAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 240 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InventoryKind {
    Assets,
    Jobs,
    Artifacts,
    Batches,
    Projects,
}
impl InventoryKind {
    fn command(self) -> &'static str {
        match self {
            Self::Assets => "assets.list",
            Self::Jobs => "jobs.list",
            Self::Artifacts => "artifacts.list",
            Self::Batches => "batches.list",
            Self::Projects => "projects.list",
        }
    }
    fn projection(self) -> &'static str {
        match self {
            Self::Assets => "assets.summary.v1",
            Self::Jobs => "jobs.summary.v1",
            Self::Artifacts => "artifacts.summary.v1",
            Self::Batches => "batches.summary.v1",
            Self::Projects => "projects.summary.v1",
        }
    }
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CatalogPhase {
    Entry,
    Fresh,
    Cursor,
    LegacyAssetsEntry,
    LegacyAssetsFresh,
    LegacyFresh(InventoryKind),
    LegacyCursor(InventoryKind),
}
#[cfg(test)]
pub(super) struct CatalogHook {
    phase: CatalogPhase,
    started: std::sync::mpsc::Sender<()>,
    waiting: std::sync::mpsc::Receiver<bool>,
}

struct Snapshot {
    principal: String,
    profile: Option<String>,
    policy_revision: u64,
    projection: String,
    limit: usize,
    secret: String,
    created: Instant,
    expires_at: String,
    items: Vec<Vec<u8>>,
    bytes: usize,
}
#[derive(Default)]
pub(super) struct Snapshots {
    entries: HashMap<String, Snapshot>,
    projects: HashMap<String, project_queries::ProjectTicket>,
    project_write_estimate: usize,
}
impl Snapshots {
    pub(super) fn clear_project_reads(&mut self) {
        self.projects.clear();
        self.project_write_estimate = 0;
    }
    fn expire(&mut self) {
        self.entries
            .retain(|_, entry| entry.created.elapsed() < LEASE);
        self.projects.retain(|_, entry| !entry.expired());
        if self.projects.is_empty() {
            self.project_write_estimate = 0;
        }
    }
    fn count(&self) -> usize {
        self.entries.len() + self.projects.len()
    }
    fn principal_count(&self, principal: &str) -> usize {
        self.entries
            .values()
            .filter(|s| s.principal == principal)
            .count()
            + self
                .projects
                .values()
                .filter(|s| s.principal() == principal)
                .count()
    }
    fn bytes(&self) -> usize {
        self.entries.values().map(|s| s.bytes).sum::<usize>()
            + self.projects.values().map(|s| s.bytes()).sum::<usize>()
    }
}
fn expired() -> DomainError {
    DomainError::new(
        "SNAPSHOT_EXPIRED",
        "Snapshot is unavailable, expired or no longer authorized; refresh the projection",
    )
}
impl Snapshot {
    fn cursor(&self, id: &str, offset: usize) -> String {
        let digest =
            Sha256::digest(format!("{}:{id}:{offset}:{}", self.secret, self.limit).as_bytes());
        format!("{id}:{offset}:{}", hex::encode(digest))
    }
    fn page(&self, id: &str, offset: usize) -> Result<Value> {
        let mut items = Vec::new();
        let mut bytes = 0;
        for item in self.items.iter().skip(offset).take(self.limit) {
            if bytes + item.len() + 1 > MAX_PAGE_BYTES {
                break;
            }
            items
                .push(serde_json::from_slice::<Value>(item).map_err(|error| {
                    DomainError::new("SERIALIZATION_FAILED", error.to_string())
                })?);
            bytes += item.len() + 1;
        }
        if items.is_empty() && offset < self.items.len() {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "One summary exceeds the page byte budget; request its explicit detail",
            ));
        }
        let next = offset + items.len();
        value(&SnapshotPage {
            schema_version: 1,
            project_revision: None,
            snapshot_id: id.to_owned(),
            snapshot_expires_at: self.expires_at.clone(),
            projection: self.projection.clone(),
            items,
            next_cursor: (next < self.items.len()).then(|| self.cursor(id, next)),
            event_cursor: None,
            total_count: self.items.len(),
            count_accuracy: "exact".to_owned(),
        })
    }
    fn push(&mut self, summary: &impl Serialize) -> Result<()> {
        let item = serde_json::to_vec(summary)
            .map_err(|error| DomainError::new("SERIALIZATION_FAILED", error.to_string()))?;
        if item.len() > MAX_PAGE_BYTES
            || self.bytes + item.len() > MAX_SNAPSHOT_BYTES
            || self.items.len() >= 4096
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Authorized summary snapshot exceeds its finite count or byte budget",
            ));
        }
        self.bytes += item.len();
        self.items.push(item);
        Ok(())
    }
}
impl Service {
    #[cfg(test)]
    fn catalog_test_phase(&self, phase: CatalogPhase) -> bool {
        let hook = {
            let mut slot = self.catalog_hook.lock().unwrap();
            if slot.as_ref().is_some_and(|hook| hook.phase == phase) {
                slot.take()
            } else {
                None
            }
        };
        if let Some(hook) = hook {
            hook.started.send(()).unwrap();
            hook.waiting
                .recv_timeout(Duration::from_secs(10))
                .expect("Catalog test phase was not resumed")
        } else {
            false
        }
    }
    fn catalog_policy(&self, context: &Context) -> Result<Option<AccessProfile>> {
        let policy = self.policy(context)?;
        if policy
            .as_ref()
            .is_some_and(|profile| !profile.allows_command("projects.list"))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Project catalog is outside the current profile allowlist",
            ));
        }
        Ok(policy)
    }
    fn catalog_publication(
        &self,
        context: &Context,
        revision: u64,
        created: Instant,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        if cancellation.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Snapshot query cancelled"));
        }
        let policy = self.catalog_policy(context)?;
        if created.elapsed() >= LEASE
            || policy.as_ref().map_or(0, |profile| profile.revision) != revision
        {
            return Err(expired());
        }
        Ok(())
    }
    pub(super) fn inventory_page(
        &self,
        context: &Context,
        input: PageInput,
        kind: InventoryKind,
        cancellation: &CancellationToken,
    ) -> Result<Value> {
        if !(1..=512).contains(&input.limit)
            || input
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.len() > 256)
            || input
                .replace_snapshot
                .as_ref()
                .is_some_and(|id| id.len() > 128)
            || (input.cursor.is_some() && input.replace_snapshot.is_some())
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Page limit must be 1..512 and cursor must be bounded",
            ));
        }
        let projects = kind == InventoryKind::Projects;
        let projection = kind.projection();
        #[cfg(test)]
        if projects {
            self.catalog_test_phase(CatalogPhase::Entry);
        }
        #[cfg(test)]
        if kind == InventoryKind::Assets {
            self.catalog_test_phase(CatalogPhase::LegacyAssetsEntry);
        }
        let policy = if projects {
            self.catalog_policy(context)?
        } else {
            self.policy(context)?
        };
        if !projects
            && policy
                .as_ref()
                .is_some_and(|profile| !profile.allows_command(kind.command()))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Inventory is outside the current profile allowlist",
            ));
        }
        let revision = policy.as_ref().map_or(0, |profile| profile.revision);
        if projects && cancellation.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Snapshot query cancelled"));
        }
        let mut cache = self.snapshots.try_lock().map_err(|_| {
            DomainError::new(
                "RESOURCE_LIMIT",
                "Snapshot service is busy; retry the same query",
            )
        })?;
        cache.expire();
        if let Some(cursor) = input.cursor {
            let mut parts = cursor.split(':');
            let id = parts.next().ok_or_else(expired)?;
            let offset = parts
                .next()
                .and_then(|offset| offset.parse::<usize>().ok())
                .ok_or_else(expired)?;
            let snapshot = cache.entries.get(id).ok_or_else(expired)?;
            if snapshot.principal != context.principal
                || snapshot.profile != context.profile
                || snapshot.policy_revision != revision
                || snapshot.projection != projection
                || snapshot.limit != input.limit
                || offset >= snapshot.items.len()
                || snapshot.cursor(id, offset) != cursor
            {
                return Err(expired());
            }
            let page = snapshot.page(id, offset)?;
            if projects {
                let created = snapshot.created;
                #[cfg(test)]
                let created = if self.catalog_test_phase(CatalogPhase::Cursor) {
                    Instant::now() - LEASE
                } else {
                    created
                };
                self.catalog_publication(context, revision, created, cancellation)?;
            } else {
                #[cfg(test)]
                if self.catalog_test_phase(CatalogPhase::LegacyCursor(kind)) {
                    // The owned page no longer borrows the entry. Age the very
                    // same retained ticket after cursor validation/rendering.
                    cache.entries.get_mut(id).unwrap().created = Instant::now() - LEASE;
                }
                if self
                    .policy(context)?
                    .as_ref()
                    .map_or(0, |profile| profile.revision)
                    != revision
                {
                    return Err(expired());
                }
            }
            return Ok(page);
        }
        if let Some(id) = input.replace_snapshot {
            if cache.entries.get(&id).is_some_and(|snapshot| {
                snapshot.principal == context.principal
                    && snapshot.profile == context.profile
                    && snapshot.projection == projection
            }) {
                cache.entries.remove(&id);
            }
            // Catalog navigation alone may retire any principal/profile-owned project read.
            // Drop the removed ticket immediately, including its real MVCC lease.
            if projects
                && cache
                    .projects
                    .get(&id)
                    .is_some_and(|ticket| ticket.owned_by(context))
            {
                cache.projects.remove(&id);
            }
        }
        let at_capacity = |cache: &Snapshots| {
            cache.count() >= MAX_SNAPSHOTS
                || cache.principal_count(&context.principal) >= MAX_PER_PRINCIPAL
        };
        let quota_error = || {
            DomainError::new(
                "RESOURCE_LIMIT",
                "Active snapshot lease quota reached; reuse a cursor or wait for expiry",
            )
        };
        if !projects && at_capacity(&cache) {
            return Err(quota_error());
        }
        let mut snapshot = Snapshot {
            principal: context.principal.clone(),
            profile: context.profile.clone(),
            policy_revision: revision,
            projection: projection.into(),
            limit: input.limit,
            secret: Uuid::new_v4().to_string(),
            created: Instant::now(),
            expires_at: self
                .clock
                .now()?
                .checked_add(30)
                .ok_or_else(expired)?
                .to_string(),
            items: Vec::new(),
            bytes: 0,
        };
        let check_cancel = || {
            if cancellation.is_cancelled() {
                Err(DomainError::new("CANCELLED", "Snapshot query cancelled"))
            } else {
                Ok(())
            }
        };
        if projects {
            self.store.visit_project_summaries(
                |id, creator| {
                    check_cancel()?;
                    Ok(policy.as_ref().is_none_or(|profile| {
                        profile.allows_object(
                            ObjectKind::Project,
                            id.as_str(),
                            creator,
                            ObjectAction::Read,
                        )
                    }))
                },
                |summary| snapshot.push(&summary),
            )?;
        } else if kind == InventoryKind::Batches {
            self.store
                .visit("batch/", |batch: nw_domain::batches::Batch| {
                    check_cancel()?;
                    if batch.schema_version != 1
                        || batch.members.len() > nw_domain::batches::MAX_BATCH_ITEMS
                    {
                        return Err(DomainError::new(
                            "INVALID_BATCH",
                            "Unknown batch summary record",
                        ));
                    }
                    let readable = policy.as_ref().is_none_or(|profile| {
                        profile.allows_object(
                            ObjectKind::Job,
                            &batch.id,
                            &batch.creator,
                            ObjectAction::Read,
                        ) && batch
                            .members
                            .iter()
                            .filter_map(|member| member.job_id.as_ref())
                            .all(|id| {
                                profile.allows_object(
                                    ObjectKind::Job,
                                    id,
                                    &batch.creator,
                                    ObjectAction::Read,
                                )
                            })
                    });
                    if readable {
                        snapshot.push(&nw_domain::batches::BatchSummary {
                            id: batch.id,
                            plan_id: batch.plan_id,
                            total: batch.members.len(),
                            accepted_at: batch.accepted_at,
                        })?;
                    }
                    Ok(())
                })?;
        } else if matches!(kind, InventoryKind::Jobs | InventoryKind::Artifacts) {
            self.store.visit("job/", |job: Job| {
                check_cancel()?;
                if policy.as_ref().is_none_or(|profile| {
                    profile.allows_object(
                        ObjectKind::Job,
                        &job.id,
                        &job.creator,
                        ObjectAction::Read,
                    )
                }) {
                    if kind == InventoryKind::Artifacts {
                        if job.state == JobState::Succeeded
                            && job.owned_staging.is_some()
                            && let Some(identity) = &job.artifact
                        {
                            let publication: Option<Publication> =
                                self.store.get(&named("publication", &job.id)?)?;
                            if !publication.as_ref().is_some_and(|p| {
                                p.read_purpose_version == 1
                                    && p.published
                                    && p.identity == *identity
                            }) {
                                return Ok(());
                            }
                            let id = format!("job-{}", job.id);
                            snapshot.push(&nw_domain::artifacts::ArtifactSummary {
                                artifact_id: id.clone(),
                                manifest_uri: format!("numera://artifact/{id}/manifest"),
                                job_id: job.id.clone(),
                                encoded_byte_count: identity.bytes.clone(),
                                sha256: identity.sha256.clone(),
                            })?;
                        }
                    } else {
                        snapshot.push(&JobSummary::from(&job))?;
                    }
                }
                Ok(())
            })?;
        } else {
            self.store.visit("asset/", |asset: Asset| {
                check_cancel()?;
                if policy
                    .as_ref()
                    .is_none_or(|profile| profile.allows_asset(&asset.creator))
                {
                    snapshot.push(&AssetSummary::from(&asset))?;
                }
                Ok(())
            })?;
        }
        if !projects
            && (snapshot.created.elapsed() >= LEASE
                || self
                    .policy(context)?
                    .as_ref()
                    .map_or(0, |profile| profile.revision)
                    != revision)
        {
            return Err(expired());
        }
        if cache.bytes() + snapshot.bytes + MAX_PAGE_BYTES > MAX_GLOBAL_BYTES {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Global summary snapshot budget reached",
            ));
        }
        let id = Uuid::new_v4().to_string();
        let page = snapshot.page(&id, 0)?;
        #[cfg(test)]
        if kind == InventoryKind::Assets {
            self.catalog_test_phase(CatalogPhase::LegacyAssetsFresh);
        }
        #[cfg(test)]
        if !projects && self.catalog_test_phase(CatalogPhase::LegacyFresh(kind)) {
            snapshot.created = Instant::now() - LEASE;
        }
        if projects {
            #[cfg(test)]
            if self.catalog_test_phase(CatalogPhase::Fresh) {
                snapshot.created = Instant::now() - LEASE;
            }
            self.catalog_publication(context, revision, snapshot.created, cancellation)?;
        }
        if !projects {
            check_cancel()?;
        }
        // Empty and single-page inventories need no retained lease or database snapshot.
        if page["next_cursor"].is_string() {
            // Projects alone defer count admission until a continuation really needs retention.
            // The same cache mutex covers admission and insertion; byte checks still ran above.
            if projects && at_capacity(&cache) {
                return Err(quota_error());
            }
            cache.entries.insert(id, snapshot);
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_domain::records::{FileIdentity, MediaProbe};

    pub(super) fn asset(id: &str, creator: &str) -> Asset {
        Asset {
            source_object: None,
            id: id.into(),
            creator: creator.into(),
            display_name: format!("{id}.mkv"),
            identity: FileIdentity {
                path: format!("/private/source/{id}.mkv"),
                bytes: "100".into(),
                sha256: "00".repeat(32),
            },
            probe: MediaProbe {
                probe_timeout: None,
                streams: Vec::new(),
                stream_details: Default::default(),
                format: "synthetic-summary-fixture".into(),
                global_tags: Default::default(),
                chapters: json!([]),
                verification_scope: "No media process is run by this storage fixture".into(),
            },
        }
    }
    async fn page(service: &Arc<Service>, context: &Context, params: Value) -> Response {
        service
            .dispatch(context.clone(), Request::query("assets.list", params))
            .await
    }
    #[tokio::test]
    async fn inventory_filters_before_counts_and_pins_pages_across_writes() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let reader = service.profile_context("browser").unwrap();
        let mut rows = Vec::new();
        for index in 0..300 {
            let asset = asset(&format!("foreign-{index:04}"), &service.owner().principal);
            rows.push((format!("asset/{}", asset.id), value(&asset).unwrap()));
        }
        for index in 0..3 {
            let asset = asset(&format!("own-{index}"), &reader.principal);
            rows.push((format!("asset/{}", asset.id), value(&asset).unwrap()));
        }
        for batch in rows.chunks(64) {
            service.store.internal_batch(batch).unwrap();
        }
        let first = page(&service, &reader, json!({"limit":2})).await;
        assert!(first.ok, "{first:?}");
        let first = first.result.unwrap();
        assert_eq!(first["total_count"], 3);
        assert_eq!(first["count_accuracy"], "exact");
        assert_eq!(first["items"].as_array().unwrap().len(), 2);
        assert!(!first.to_string().contains("/private/source"));
        assert!(!first.to_string().contains("foreign"));
        let later = asset("own-3", &reader.principal);
        service.store.put("asset/own-3", &later).unwrap();
        let cursor = first["next_cursor"].as_str().unwrap();
        let second = page(&service, &reader, json!({"limit":2,"cursor":cursor}))
            .await
            .result
            .unwrap();
        assert_eq!(second["snapshot_id"], first["snapshot_id"]);
        assert_eq!(second["total_count"], 3);
        assert_eq!(second["items"][0]["id"], "own-2");
        assert!(second["next_cursor"].is_null());
        let fresh = page(
            &service,
            &reader,
            json!({"limit":2,"replace_snapshot":first["snapshot_id"]}),
        )
        .await
        .result
        .unwrap();
        assert_eq!(fresh["total_count"], 4);
        assert_ne!(fresh["snapshot_id"], first["snapshot_id"]);
        assert_eq!(
            page(&service, &reader, json!({"limit":2,"cursor":cursor}))
                .await
                .error
                .unwrap()
                .code,
            "SNAPSHOT_EXPIRED"
        );

        project_catalog_lists_committed_public_headers(&service).await;
        catalog_tests::headers_and_rights().await;
        legacy_assets_entry_rechecks_current_command_allowlist().await;
    }
    async fn project_catalog_lists_committed_public_headers(service: &Arc<Service>) {
        let owner = service.owner();
        let mut bootstrap = Request::query("access.replay_scope.create", json!({}));
        bootstrap.request_id = "workbench-catalog-scope".into();
        let bootstrapped = service.dispatch(owner.clone(), bootstrap).await;
        assert!(bootstrapped.ok, "Catalog fixture scope: {bootstrapped:?}");
        assert!(bootstrapped.receipt_id.is_none());
        let scope = bootstrapped.result.unwrap()["replay_scope"]
            .as_str()
            .expect("The actual owner must issue a replay scope")
            .to_owned();
        let name = "Stage A catalog discovery";
        let mut create = Request::query("projects.create", json!({"name":name}));
        create.request_id = "workbench-catalog-create".into();
        create.replay_scope = Some(scope);
        create.idempotency_key = Some("workbench-catalog-create".into());
        let created = service.dispatch(owner.clone(), create).await;
        assert!(created.ok, "Catalog fixture project: {created:?}");
        assert!(created.receipt_id.is_some());
        let created = created.result.unwrap();
        let project_id = created["project_id"]
            .as_str()
            .expect("The accepted project must have an identity");
        assert_eq!(created["revision"], "0");

        let mut request = Request::query("projects.list", json!({"limit":64}));
        request.request_id = "workbench-catalog-list".into();
        let response = service.dispatch(owner, request).await;
        assert!(
            response.ok,
            "projects.list must discover a committed project through the shared service: {response:?}"
        );
        assert!(response.receipt_id.is_none());
        assert_eq!(response.request_id, "workbench-catalog-list");
        let catalog = response.result.unwrap();
        assert_eq!(catalog["schema_version"], 1);
        assert_eq!(catalog["project_revision"], Value::Null);
        assert_eq!(catalog["projection"], "projects.summary.v1");
        assert_eq!(catalog["total_count"], 1);
        assert_eq!(catalog["count_accuracy"], "exact");
        assert_eq!(catalog["next_cursor"], Value::Null);
        assert_eq!(catalog["event_cursor"], Value::Null);
        assert!(catalog["snapshot_expires_at"].is_string());
        assert_eq!(
            catalog["items"],
            json!([{
                "id":project_id,
                "name":name,
                "revision":"0",
                "schema_major":1,
                "envelope_version":1,
                "compatibility":"supported"
            }]),
            "Catalog rows must contain only the bounded public header summary"
        );
        let snapshot = catalog["snapshot_id"]
            .as_str()
            .expect("Catalog pages must identify their captured projection");
        assert!(!snapshot.is_empty());
        assert!(
            !service
                .snapshots
                .lock()
                .unwrap()
                .entries
                .contains_key(snapshot),
            "The limit64 catalog must not retain a single-page inventory ticket"
        );
    }
    #[tokio::test]
    async fn snapshot_cursor_cannot_change_identity_query_policy_or_lease() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::open(root.path().join("data"), None).await.unwrap();
        let reader = service.profile_context("browser").unwrap();
        for index in 0..3 {
            service
                .store
                .put(
                    &format!("asset/own-{index}"),
                    &asset(&format!("own-{index}"), &reader.principal),
                )
                .unwrap();
        }
        let first = page(&service, &reader, json!({"limit":1}))
            .await
            .result
            .unwrap();
        let cursor = first["next_cursor"].as_str().unwrap();
        for (context, params) in [
            (service.owner(), json!({"limit":1,"cursor":cursor})),
            (reader.clone(), json!({"limit":2,"cursor":cursor})),
            (
                reader.clone(),
                json!({"limit":1,"cursor":format!("{cursor}a")}),
            ),
        ] {
            assert_eq!(
                page(&service, &context, params).await.error.unwrap().code,
                "SNAPSHOT_EXPIRED"
            );
        }
        assert_eq!(
            service
                .dispatch(
                    reader.clone(),
                    Request::query("jobs.list", json!({"limit":1,"cursor":cursor}))
                )
                .await
                .error
                .unwrap()
                .code,
            "SNAPSHOT_EXPIRED"
        );
        let mut profile: AccessProfile = service.store.get("profile/browser").unwrap().unwrap();
        profile.revision += 1;
        service.store.put("profile/browser", &profile).unwrap();
        assert_eq!(
            page(&service, &reader, json!({"limit":1,"cursor":cursor}))
                .await
                .error
                .unwrap()
                .code,
            "SNAPSHOT_EXPIRED"
        );
        let fresh = page(&service, &reader, json!({"limit":1}))
            .await
            .result
            .unwrap();
        service
            .snapshots
            .lock()
            .unwrap()
            .entries
            .get_mut(fresh["snapshot_id"].as_str().unwrap())
            .unwrap()
            .created = Instant::now() - Duration::from_secs(31);
        assert_eq!(
            page(
                &service,
                &reader,
                json!({"limit":1,"cursor":fresh["next_cursor"]})
            )
            .await
            .error
            .unwrap()
            .code,
            "SNAPSHOT_EXPIRED"
        );
        assert_eq!(
            page(&service, &reader, json!({"limit":513}))
                .await
                .error
                .unwrap()
                .code,
            "VALIDATION_ERROR"
        );
        crate::runtime::projects::tests::indexed_snapshot_and_edit_cases().await;
        crate::runtime::projects::tests::held_source_current_grants().await;
        crate::runtime::projects::regression_tests::compatibility_and_zero_delta().await;
        crate::runtime::projects::regression_tests::linked_edits().await;
        project_queries::exercise_real_snapshot_lifetime().await;
        catalog_tests::retirement_and_quotas().await;
        catalog_tests::cursor_and_publication().await;
        crate::runtime::projects::regression_tests::public_link_producer().await;
    }
    #[test]
    fn page_byte_ceiling_wins_over_row_count_without_truncating_items() {
        let mut snapshot = Snapshot {
            principal: "unit".into(),
            profile: None,
            policy_revision: 0,
            projection: "unit".into(),
            limit: 512,
            secret: Uuid::new_v4().to_string(),
            created: Instant::now(),
            expires_at: "30".into(),
            items: Vec::new(),
            bytes: 0,
        };
        let name = "x".repeat(130 * 1024);
        snapshot
            .push(&json!({"id":"one","display_name":name}))
            .unwrap();
        snapshot
            .push(&json!({"id":"two","display_name":name}))
            .unwrap();
        let first = snapshot.page("snapshot", 0).unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            first["items"][0]["display_name"].as_str().unwrap().len(),
            name.len()
        );
        assert!(serde_json::to_vec(&first).unwrap().len() < 256 * 1024);
        assert!(first["next_cursor"].is_string());
        assert_eq!(
            snapshot
                .push(&json!({"display_name":"x".repeat(MAX_PAGE_BYTES)}))
                .unwrap_err()
                .code,
            "RESOURCE_LIMIT"
        );
        legacy_empty_assets_final_publication_rejects_cancellation();
        real_producer_tests::run_if_admitted();
    }
    fn legacy_empty_assets_final_publication_rejects_cancellation() {
        // The original synchronous unit has no runtime. Keep a second worker
        // available while the unchanged reaper waits for the held cache mutex.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let root = tempfile::tempdir().unwrap();
                let service = Service::open(root.path().join("data"), None).await.unwrap();
                let reader = service.profile_context("browser").unwrap();
                let mut control = Request::query("assets.list", json!({"limit":1}));
                control.request_id = "legacy-assets-final-control".into();
                let readable = service.dispatch(reader.clone(), control).await;
                assert!(
                    readable.ok,
                    "Legacy empty Assets positive control: {readable:?}"
                );
                assert!(readable.error.is_none(), "{readable:?}");
                assert!(readable.receipt_id.is_none(), "{readable:?}");
                let readable_page = readable.result.as_ref().unwrap();
                assert_eq!(readable_page["projection"], "assets.summary.v1");
                assert_eq!(readable_page["items"], json!([]));
                assert_eq!(readable_page["total_count"], 0);
                assert_eq!(readable_page["count_accuracy"], "exact");
                assert_eq!(readable_page["next_cursor"], Value::Null);
                let retained_before = service.snapshots.lock().unwrap().count();
                assert_eq!(retained_before, 0);

                let cancellation = CancellationToken::new();
                assert!(!cancellation.is_cancelled());
                let mut request = Request::query("assets.list", json!({"limit":1}));
                request.request_id = "legacy-assets-final-cancel".into();
                let (started, ready) = std::sync::mpsc::channel();
                let (resume, waiting) = std::sync::mpsc::channel();
                *service.catalog_hook.lock().unwrap() = Some(CatalogHook {
                    phase: CatalogPhase::LegacyAssetsFresh,
                    started,
                    waiting,
                });
                let task_service = service.clone();
                let task_cancellation = cancellation.clone();
                let task = tokio::spawn(async move {
                    task_service
                        .dispatch_cancel(reader, request, task_cancellation)
                        .await
                });
                tokio::task::spawn_blocking(move || {
                    ready
                        .recv_timeout(Duration::from_secs(5))
                        .expect("Legacy empty Assets final publication phase not reached")
                })
                .await
                .unwrap();

                // Page rendering has finished; do not acquire the held cache
                // mutex until the public dispatch has resumed and joined.
                assert!(!cancellation.is_cancelled());
                cancellation.cancel();
                assert!(cancellation.is_cancelled());
                resume.send(false).unwrap();
                let response = tokio::time::timeout(Duration::from_secs(5), task)
                    .await
                    .expect("Legacy empty Assets final publication did not resume")
                    .unwrap();
                let retained_after = service.snapshots.lock().unwrap().count();
                assert!(
                    !response.ok,
                    "Legacy empty Assets final publication must reject cancellation: {response:?}"
                );
                assert_eq!(
                    response.error.as_ref().map(|error| error.code.as_str()),
                    Some("CANCELLED"),
                    "{response:?}"
                );
                assert!(response.result.is_none(), "{response:?}");
                assert!(response.receipt_id.is_none(), "{response:?}");
                assert_eq!(retained_after, retained_before);
            });
    }
    async fn legacy_assets_entry_rechecks_current_command_allowlist() {
        // Keep the catalog race fixtures' isolated runtime: the unchanged reaper
        // must leave a worker available for the real public policy update.
        tokio::task::spawn_blocking(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let root = tempfile::tempdir().unwrap();
                    let service = Service::open(root.path().join("data"), None).await.unwrap();
                    let owner = service.owner();
                    let reader = service.profile_context("browser").unwrap();
                    let original: AccessProfile =
                        service.store.get("profile/browser").unwrap().unwrap();
                    assert_eq!(original.id, "browser");
                    assert_eq!(original.name, "browser");
                    assert_eq!(Some(&original.id), reader.profile.as_ref());
                    assert_eq!(original.principal, reader.principal);
                    assert!(original.enabled);
                    assert_eq!(original.mode, ProfileMode::Operate);
                    assert!(original.owned_object_access);
                    assert!(original.allows_command("assets.list"));

                    let owned = asset("legacy-entry-owned", &reader.principal);
                    assert!(original.allows_asset(&owned.creator));
                    service.store.put("asset/legacy-entry-owned", &owned).unwrap();
                    let readable = page(&service, &reader, json!({"limit":1})).await;
                    assert!(readable.ok, "Legacy Assets positive control: {readable:?}");
                    assert!(readable.error.is_none(), "{readable:?}");
                    assert!(readable.receipt_id.is_none(), "{readable:?}");
                    let readable_page = readable.result.as_ref().unwrap();
                    assert_eq!(readable_page["projection"], "assets.summary.v1");
                    assert_eq!(readable_page["total_count"], 1);
                    assert_eq!(readable_page["count_accuracy"], "exact");
                    assert_eq!(readable_page["next_cursor"], Value::Null);
                    assert_eq!(
                        readable_page["items"],
                        json!([{
                            "id":"legacy-entry-owned",
                            "display_name":"legacy-entry-owned.mkv",
                            "bytes":"100",
                            "stream_count":0
                        }])
                    );

                    let mut bootstrap = Request::query("access.replay_scope.create", json!({}));
                    bootstrap.request_id = "legacy-assets-entry-scope".into();
                    let bootstrapped = service.dispatch(owner.clone(), bootstrap).await;
                    assert!(bootstrapped.ok, "Legacy Assets owner scope: {bootstrapped:?}");
                    assert!(bootstrapped.error.is_none(), "{bootstrapped:?}");
                    assert!(bootstrapped.receipt_id.is_none(), "{bootstrapped:?}");
                    let scope = bootstrapped.result.as_ref().unwrap()["replay_scope"]
                        .as_str()
                        .unwrap()
                        .to_owned();
                    let mut allowed = original.allowed_command_ids.clone();
                    assert!(allowed.remove("assets.list"));
                    let mut update = Request::query(
                        "access.profiles.update",
                        json!({
                            "name":original.id,
                            "expected_revision":original.revision,
                            "enabled":original.enabled,
                            "mode":original.mode,
                            "owned_object_access":original.owned_object_access,
                            "client_sessions":original.client_sessions,
                            "allowed_command_ids":allowed,
                            "object_grants":original.object_grants,
                            "read_roots":original.read_roots,
                            "write_roots":original.write_roots
                        }),
                    );
                    update.request_id = "legacy-assets-entry-revoke".into();
                    update.replay_scope = Some(scope);
                    update.idempotency_key = Some("legacy-assets-entry-revoke-once".into());
                    let mut expected = value(&original).unwrap();
                    expected["revision"] = json!(original.revision + 1);
                    expected["allowed_command_ids"] = json!(allowed);
                    let retained_before = service.snapshots.lock().unwrap().count();
                    assert_eq!(retained_before, 0);

                    let (started, ready) = std::sync::mpsc::channel();
                    let (resume, waiting) = std::sync::mpsc::channel();
                    *service.catalog_hook.lock().unwrap() = Some(CatalogHook {
                        phase: CatalogPhase::LegacyAssetsEntry,
                        started,
                        waiting,
                    });
                    let task_service = service.clone();
                    let task_reader = reader.clone();
                    let task = tokio::spawn(async move {
                        page(&task_service, &task_reader, json!({"limit":1})).await
                    });
                    tokio::task::spawn_blocking(move || {
                        ready
                            .recv_timeout(Duration::from_secs(5))
                            .expect("Legacy Assets entry phase not reached")
                    })
                    .await
                    .unwrap();

                    // Entry is before policy capture and the cache/scan locks.
                    // No fixture lock is held while the owner commits the update.
                    let changed = tokio::time::timeout(
                        Duration::from_secs(5),
                        service.dispatch(owner, update),
                    )
                    .await
                    .expect("Legacy Assets public policy update did not finish");
                    assert!(changed.ok, "Legacy Assets allowlist update: {changed:?}");
                    assert!(changed.error.is_none(), "{changed:?}");
                    assert!(changed.receipt_id.is_some(), "{changed:?}");
                    assert_eq!(changed.result.as_ref(), Some(&expected), "{changed:?}");
                    let current: AccessProfile =
                        service.store.get("profile/browser").unwrap().unwrap();
                    assert_eq!(value(&current).unwrap(), expected);
                    assert_eq!(current.revision, original.revision + 1);
                    assert!(!current.allows_command("assets.list"));
                    assert!(current.allows_asset(&owned.creator));
                    resume.send(false).unwrap();
                    let response = tokio::time::timeout(Duration::from_secs(5), task)
                        .await
                        .expect("Legacy Assets dispatch did not resume")
                        .unwrap();
                    let retained_after = service.snapshots.lock().unwrap().count();
                    assert!(
                        !response.ok,
                        "Legacy assets entry must recheck the current command allowlist: {response:?}"
                    );
                    assert_eq!(
                        response.error.as_ref().map(|error| error.code.as_str()),
                        Some("PERMISSION_DENIED"),
                        "{response:?}"
                    );
                    assert!(response.result.is_none(), "{response:?}");
                    assert!(response.receipt_id.is_none(), "{response:?}");
                    assert_eq!(retained_after, retained_before);
                });
        })
        .await
        .unwrap();
    }
}
