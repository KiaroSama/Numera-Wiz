//! Short-lived, authorization-bound inventory snapshots containing summaries only.
use super::*;
use nw_domain::views::{AssetSummary, JobSummary, PageInput, SnapshotPage};
use std::time::{Duration, Instant};

const LEASE: Duration = Duration::from_secs(30);
const MAX_SNAPSHOTS: usize = 32;
const MAX_PER_PRINCIPAL: usize = 4;
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
const MAX_GLOBAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 240 * 1024;

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
    pub(super) fn inventory_page(
        &self,
        context: &Context,
        input: PageInput,
        jobs: bool,
        cancellation: &CancellationToken,
    ) -> Result<Value> {
        self.inventory_projection(context, input, jobs, false, cancellation)
    }
    pub(super) fn artifact_inventory(
        &self,
        context: &Context,
        input: PageInput,
        cancellation: &CancellationToken,
    ) -> Result<Value> {
        self.inventory_projection(context, input, true, true, cancellation)
    }
    fn inventory_projection(
        &self,
        context: &Context,
        input: PageInput,
        jobs: bool,
        artifacts: bool,
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
        let projection = if artifacts {
            "artifacts.summary.v1"
        } else if jobs {
            "jobs.summary.v1"
        } else {
            "assets.summary.v1"
        };
        let policy = self.policy(context)?;
        let revision = policy.as_ref().map_or(0, |profile| profile.revision);
        let mut cache = self.snapshots.try_lock().map_err(|_| {
            DomainError::new(
                "RESOURCE_LIMIT",
                "Snapshot service is busy; retry the same query",
            )
        })?;
        cache
            .entries
            .retain(|_, snapshot| snapshot.created.elapsed() < LEASE);
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
            if self
                .policy(context)?
                .as_ref()
                .map_or(0, |profile| profile.revision)
                != revision
            {
                return Err(expired());
            }
            return Ok(page);
        }
        if let Some(id) = input.replace_snapshot
            && cache.entries.get(&id).is_some_and(|snapshot| {
                snapshot.principal == context.principal
                    && snapshot.profile == context.profile
                    && snapshot.projection == projection
            })
        {
            cache.entries.remove(&id);
        }
        if cache.entries.len() >= MAX_SNAPSHOTS
            || cache
                .entries
                .values()
                .filter(|snapshot| snapshot.principal == context.principal)
                .count()
                >= MAX_PER_PRINCIPAL
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Active snapshot lease quota reached; reuse a cursor or wait for expiry",
            ));
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
        if jobs {
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
                    if artifacts {
                        if job.state == JobState::Succeeded
                            && job.owned_staging.is_some()
                            && let Some(identity) = &job.artifact
                        {
                            let publication: Option<Publication> = self.store.get(&named("publication", &job.id)?)?;
                            if !publication.as_ref().is_some_and(|p| p.read_purpose_version == 1 && p.published && p.identity == *identity) { return Ok(()); }
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
        if snapshot.created.elapsed() >= LEASE
            || self
                .policy(context)?
                .as_ref()
                .map_or(0, |profile| profile.revision)
                != revision
        {
            return Err(expired());
        }
        if cache
            .entries
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            + snapshot.bytes
            > MAX_GLOBAL_BYTES
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Global summary snapshot budget reached",
            ));
        }
        let id = Uuid::new_v4().to_string();
        let page = snapshot.page(&id, 0)?;
        // Empty and single-page inventories need no retained lease or database snapshot.
        if page["next_cursor"].is_string() {
            cache.entries.insert(id, snapshot);
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_domain::records::{FileIdentity, MediaProbe};

    fn asset(id: &str, creator: &str) -> Asset {
        Asset {
            id: id.into(),
            creator: creator.into(),
            display_name: format!("{id}.mkv"),
            identity: FileIdentity {
                path: format!("/private/source/{id}.mkv"),
                bytes: "100".into(),
                sha256: "00".repeat(32),
            },
            probe: MediaProbe {
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
    }
}
