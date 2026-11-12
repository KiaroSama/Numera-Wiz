//! Admitted Linux CPU producers, called by the unchanged byte-ceiling test identity.
use super::*;
#[path = "../../../../../test-support/media.rs"]
mod fixture;

const KINDS: [InventoryKind; 4] = [
    InventoryKind::Assets,
    InventoryKind::Jobs,
    InventoryKind::Artifacts,
    InventoryKind::Batches,
];

pub(super) fn run_if_admitted() {
    let Some(bin) = std::env::var_os("NW_FIXTURE_COMPONENTS") else {
        eprintln!("real-producers: not-admitted (NW_FIXTURE_COMPONENTS absent)");
        return;
    };
    assert!(!bin.is_empty(), "Explicit fixture component directory is empty");
    let bin = std::fs::canonicalize(PathBuf::from(bin)).expect("Explicit fixture component directory");
    assert!(bin.is_dir());
    for name in ["ffmpeg", "ffprobe"] {
        let path = bin.join(name);
        let metadata = std::fs::metadata(&path).expect("Explicit fixture component must exist");
        assert!(metadata.is_file() && metadata.len() > 0, "{path:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(metadata.permissions().mode() & 0o111, 0, "{path:?}");
        }
    }
    eprintln!("real-producers: started source=existing-media-FFV1-two-PCM-1s-64x48-at4 components={bin:?}");
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(exercise(bin));
}

fn request(command: &str, params: Value) -> Request {
    let mut request = Request::query(command, params);
    request.request_id = Uuid::new_v4().to_string();
    request
}
fn view(response: Response, durable: bool) -> Value {
    assert!(response.ok, "{response:?}");
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(response.receipt_id.is_some(), durable, "{response:?}");
    response.result.unwrap()
}
async fn query(service: &Arc<Service>, context: &Context, command: &str, params: Value) -> Value {
    view(service.dispatch(context.clone(), request(command, params)).await, false)
}
async fn scope(service: &Arc<Service>, context: &Context) -> String {
    query(service, context, "access.replay_scope.create", json!({})).await["replay_scope"]
        .as_str().unwrap().to_owned()
}
async fn write(
    service: &Arc<Service>, context: &Context, scope: &str, command: &str, params: Value,
) -> Value {
    let mut request = request(command, params);
    request.replay_scope = Some(scope.to_owned());
    request.idempotency_key = Some(Uuid::new_v4().to_string());
    view(service.dispatch(context.clone(), request).await, true)
}
async fn put_profile(service: &Arc<Service>, owner_scope: &str, params: Value) -> AccessProfile {
    serde_json::from_value(write(service, &service.owner(), owner_scope, "access.profiles.put", params).await).unwrap()
}
async fn update_profile(
    service: &Arc<Service>, owner_scope: &str, current: &AccessProfile, desired: &AccessProfile,
) -> AccessProfile {
    let response = tokio::time::timeout(Duration::from_secs(5), write(
        service, &service.owner(), owner_scope, "access.profiles.update", json!({
            "name":current.id,"expected_revision":current.revision,
            "enabled":desired.enabled,"mode":desired.mode,"owned_object_access":desired.owned_object_access,
            "client_sessions":desired.client_sessions,"allowed_command_ids":desired.allowed_command_ids,
            "object_grants":desired.object_grants,"read_roots":desired.read_roots,"write_roots":desired.write_roots
        }),
    )).await.expect("Public profile update exceeded the existing five-second barrier budget");
    let updated: AccessProfile = serde_json::from_value(response).unwrap();
    let mut expected = desired.clone();
    expected.revision = current.revision + 1;
    assert_eq!(value(&updated).unwrap(), value(&expected).unwrap());
    assert_eq!(updated.principal, current.principal);
    assert_eq!(updated.id, current.id);
    updated
}
fn identity(path: &Path) -> nw_domain::records::FileIdentity {
    files::identity(path, &CancellationToken::new()).unwrap()
}
fn ids(page: &Value, kind: InventoryKind) -> BTreeSet<String> {
    page["items"].as_array().unwrap().iter().map(|row| {
        row[if kind == InventoryKind::Artifacts { "artifact_id" } else { "id" }]
            .as_str().unwrap().to_owned()
    }).collect()
}
fn page_control(page: &Value, kind: InventoryKind, expected: &BTreeSet<String>, limit: usize) {
    assert_eq!(page["projection"], kind.projection());
    assert_eq!(page["total_count"], expected.len());
    assert_eq!(page["count_accuracy"], "exact");
    assert_eq!(page["items"].as_array().unwrap().len(), expected.len().min(limit));
    assert!(ids(page, kind).is_subset(expected));
    assert_eq!(page["next_cursor"].is_string(), expected.len() > limit);
    if expected.len() <= limit {
        assert_eq!(&ids(page, kind), expected);
    }
}
fn private_paths_absent(page: &Value, roots: &[&Path]) {
    let encoded = page.to_string();
    for root in roots {
        assert!(!encoded.contains(root.to_str().unwrap()), "Summary exposed private path");
    }
    for field in ["source_path", "output_path", "read_roots", "write_roots", "principal", "creator", "prepared_argv"] {
        assert!(!encoded.contains(&format!("\"{field}\"")), "Summary exposed {field}");
    }
}
async fn completed_job(service: &Arc<Service>, context: &Context, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = query(service, context, "jobs.inspect", json!({"id":id})).await;
            let state: JobState = serde_json::from_value(job["state"].clone()).unwrap();
            if state.terminal() {
                assert_eq!(state, JobState::Succeeded, "{job}");
                return job;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("Real public job exceeded the existing thirty-second completion bound")
}
async fn completed_batch(service: &Arc<Service>, context: &Context, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let batch = query(service, context, "batches.inspect", json!({"id":id})).await;
            if batch["complete"] == true {
                assert_eq!(batch["total"], 2);
                assert_eq!(batch["finished"], 2);
                assert_eq!(batch["succeeded"], 2);
                for field in ["skipped", "failed", "cancelled", "interrupted"] {
                    assert_eq!(batch[field], 0, "{batch}");
                }
                assert_eq!(batch["items"].as_array().unwrap().len(), 2);
                for item in batch["items"].as_array().unwrap() {
                    assert!(item["job_id"].is_string());
                    assert_eq!(item["state"], "succeeded");
                    assert_eq!(item["action"], "copy_unchanged");
                    assert!(item["error"].is_null());
                }
                return batch;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("Real public batch exceeded the existing thirty-second completion bound")
}
async fn drained(service: &Arc<Service>) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while service.busy() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("Actual accepted workers did not drain within thirty seconds");
    assert!(!service.busy());
}

async fn exercise(bin: PathBuf) {
    let started = Instant::now();
    let media = fixture::generate().await;
    assert_eq!(std::fs::canonicalize(&media.bin).unwrap(), bin);
    let probe = fixture::probe(&media.bin, &media.source).await;
    let streams = probe["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[0]["codec_name"], "ffv1");
    assert_eq!(streams[0]["width"], 64);
    assert_eq!(streams[0]["height"], 48);
    assert_eq!(streams[0]["r_frame_rate"], "4/1");
    for stream in &streams[1..] {
        assert_eq!(stream["codec_name"], "pcm_s16le");
        assert_eq!(stream["codec_type"], "audio");
    }
    let original_bytes = std::fs::read(&media.source).unwrap();
    let source_identity = identity(&media.source);
    assert_eq!(source_identity.sha256, hex::encode(Sha256::digest(&original_bytes)));
    let second_source = media.directory.path().join("source-second.mkv");
    std::fs::copy(&media.source, &second_source).unwrap();
    assert_eq!(std::fs::read(&second_source).unwrap(), original_bytes);
    let root = tempfile::tempdir().unwrap();
    let outputs = root.path().join("outputs");
    std::fs::create_dir(&outputs).unwrap();
    assert!(!outputs.starts_with(&bin));
    assert!(!media.directory.path().starts_with(&bin));
    let service = Service::open(root.path().join("data"), Some((bin.join("ffmpeg"), bin.join("ffprobe"))))
        .await.unwrap();
    let engine = service.engine().unwrap();
    for (name, component) in [("ffmpeg", &engine.ffmpeg), ("ffprobe", &engine.ffprobe)] {
        let observed = identity(&bin.join(name));
        assert_eq!(observed.sha256, component.sha256);
        assert_eq!(observed.bytes, component.bytes);
        eprintln!("real-producers: component={name} bytes={} sha256={}", observed.bytes, observed.sha256);
    }
    eprintln!("real-producers: source bytes={} sha256={} streams=3", source_identity.bytes, source_identity.sha256);
    let owner_scope = scope(&service, &service.owner()).await;
    let producer_commands = [
        "access.replay_scope.create", "assets.register", "assets.list", "streams.clean",
        "jobs.submit", "jobs.inspect", "jobs.list", "artifacts.list", "artifacts.manifest",
        "streams.clean.batch_plan", "batches.submit", "batches.inspect", "batches.list",
    ];
    let mut producer_profile = put_profile(&service, &owner_scope, json!({
        "name":"inventory-producer","expected_revision":null,"enabled":true,"mode":"operate",
        "owned_object_access":true,"client_sessions":false,"allowed_command_ids":producer_commands,
        "object_grants":[],"read_roots":[media.directory.path()],"write_roots":[outputs]
    })).await;
    let producer = service.profile_context(&producer_profile.id).unwrap();
    assert_eq!(producer.principal, producer_profile.principal);
    assert!(producer_profile.enabled && producer_profile.owned_object_access);
    assert_eq!(producer_profile.mode, ProfileMode::Operate);
    let producer_scope = scope(&service, &producer).await;
    let mut assets = BTreeSet::new();
    let mut asset_order = Vec::new();
    for path in [&media.source, &second_source] {
        let asset = write(&service, &producer, &producer_scope, "assets.register", json!({"path":path})).await;
        let id = asset["id"].as_str().unwrap().to_owned();
        assert_eq!(asset["creator"], producer.principal);
        assert_eq!(asset["identity"]["sha256"], source_identity.sha256);
        assert_eq!(asset["identity"]["bytes"], source_identity.bytes);
        assert_eq!(asset["probe"]["streams"].as_array().unwrap().len(), 3);
        assert!(assets.insert(id.clone()), "Equal bytes must still have distinct actual asset IDs");
        asset_order.push(id);
    }
    let mut jobs = BTreeSet::new();
    let mut batches = BTreeSet::new();
    let mut job_outputs = HashMap::new();
    let mut batch_members = HashMap::new();
    let mut batch_order = Vec::new();
    let standalone_output = outputs.join("standalone.mkv");
    let plan = query(&service, &producer, "streams.clean", json!({
        "asset_id":asset_order[0],"output":standalone_output,"intent":fixture::rules(),"file_times":"new"
    })).await;
    assert_eq!(plan["resolution"]["action"], "copy_unchanged");
    assert_eq!(plan["source_sha256"], source_identity.sha256);
    let standalone = write(&service, &producer, &producer_scope, "jobs.submit", json!({
        "plan_id":plan["id"],"plan_hash":plan["hash"]
    })).await["job_id"].as_str().unwrap().to_owned();
    assert!(jobs.insert(standalone.clone()));
    job_outputs.insert(standalone.clone(), standalone_output);
    completed_job(&service, &producer, &standalone).await;
    for batch_index in 0..2 {
        let paths: Vec<_> = (0..2).map(|index| outputs.join(format!("batch-{batch_index}-{index}.mkv"))).collect();
        let items: Vec<_> = asset_order.iter().zip(&paths).map(|(asset, output)| json!({"asset_id":asset,"output":output})).collect();
        let plan = query(&service, &producer, "streams.clean.batch_plan", json!({
            "items":items,"intent":fixture::rules(),"file_times":"new",
            "execution_limits":nw_domain::execution::CleanupLimits::default(),
            "failure_policy":"reject_all","unresolved_policy":"reject"
        })).await;
        assert_eq!(plan["items"].as_array().unwrap().len(), 2);
        for item in plan["items"].as_array().unwrap() {
            assert_eq!(item["action"], "copy_unchanged");
            assert!(item["error"].is_null());
            assert_eq!(item["source_sha256"], source_identity.sha256);
        }
        let accepted = write(&service, &producer, &producer_scope, "batches.submit", json!({
            "plan_id":plan["id"],"plan_hash":plan["hash"]
        })).await;
        assert_eq!(accepted["accepted_jobs"], 2);
        assert_eq!(accepted["rejected_candidates"], 0);
        let batch = accepted["batch_id"].as_str().unwrap().to_owned();
        assert!(batches.insert(batch.clone()));
        let members = accepted["members"].as_array().unwrap();
        assert_eq!(members.len(), 2);
        let mut member_ids = Vec::new();
        for (index, member) in members.iter().enumerate() {
            assert_eq!(member["index"], index);
            let id = member["job_id"].as_str().unwrap().to_owned();
            assert!(jobs.insert(id.clone()));
            job_outputs.insert(id.clone(), paths[index].clone());
            member_ids.push(id);
        }
        let finished = completed_batch(&service, &producer, &batch).await;
        for (index, item) in finished["items"].as_array().unwrap().iter().enumerate() {
            assert_eq!(item["job_id"], member_ids[index]);
            assert_eq!(item["asset_id"], asset_order[index]);
            assert_eq!(item["artifact_id"], format!("job-{}", member_ids[index]));
        }
        batch_order.push(batch.clone());
        batch_members.insert(batch, member_ids);
    }
    assert_eq!(jobs.len(), 5);
    assert_eq!(batches.len(), 2);
    let mut artifacts = BTreeSet::new();
    for id in &jobs {
        let inspected = completed_job(&service, &producer, id).await;
        assert_eq!(inspected["id"], *id);
        assert_eq!(inspected["plan"]["resolution"]["action"], "copy_unchanged");
        let output = &job_outputs[id];
        assert_eq!(std::fs::read(output).unwrap(), original_bytes);
        let output_identity = identity(output);
        assert_eq!(output_identity.sha256, source_identity.sha256);
        assert_eq!(output_identity.bytes, source_identity.bytes);
        let artifact = format!("job-{id}");
        let manifest = query(&service, &producer, "artifacts.manifest", json!({"artifact_id":artifact})).await;
        assert_eq!(manifest["artifact_id"], artifact);
        assert_eq!(manifest["job_id"], *id);
        assert_eq!(manifest["plan_id"], inspected["plan"]["id"]);
        assert_eq!(manifest["sha256"], source_identity.sha256);
        assert_eq!(manifest["encoded_byte_count"], source_identity.bytes);
        assert_eq!(manifest["availability"], "retained_published_object");
        assert_eq!(manifest["manifest_uri"], format!("numera://artifact/{artifact}/manifest"));
        let actual = service.store.inspect_job(id).unwrap();
        assert_eq!(actual.creator, producer.principal);
        assert_eq!(actual.state, JobState::Succeeded);
        assert!(assets.contains(&actual.plan.source.id));
        assert_eq!(actual.plan.output, files::path_text(output).unwrap());
        assert_eq!(inspected["artifact"]["sha256"], source_identity.sha256);
        assert_eq!(inspected["artifact"]["bytes"], source_identity.bytes);
        assert!(actual.owned_staging.is_some());
        let publication: Publication = service.store.get(&named("publication", id).unwrap()).unwrap().unwrap();
        assert_eq!(publication.read_purpose_version, 1);
        assert!(publication.published);
        assert_eq!(value(&publication.identity).unwrap(), value(actual.artifact.as_ref().unwrap()).unwrap());
        assert!(artifacts.insert(artifact));
    }
    drained(&service).await;
    let expected = [assets, jobs, artifacts, batches];
    for (kind, ids) in KINDS.into_iter().zip(&expected) {
        let page = query(&service, &producer, kind.command(), json!({"limit":64})).await;
        page_control(&page, kind, ids, 64);
        private_paths_absent(&page, &[media.directory.path(), root.path(), &bin]);
        if kind == InventoryKind::Artifacts {
            for item in page["items"].as_array().unwrap() {
                assert!(expected[1].contains(item["job_id"].as_str().unwrap()));
                assert_eq!(item["artifact_id"], format!("job-{}", item["job_id"].as_str().unwrap()));
                assert_eq!(item["sha256"], source_identity.sha256);
                assert_eq!(item["encoded_byte_count"], source_identity.bytes);
            }
        }
    }
    let grants: Vec<_> = expected[1].iter().chain(&expected[3]).map(|id| json!({"kind":"job","id":id,"actions":["read"]})).collect();
    let mut delegated_profile = put_profile(&service, &owner_scope, json!({
        "name":"inventory-delegated","expected_revision":null,"enabled":true,"mode":"operate",
        "owned_object_access":false,"client_sessions":false,"allowed_command_ids":[
            "access.replay_scope.create", "assets.list", "jobs.list", "jobs.inspect", "artifacts.list",
            "artifacts.manifest", "batches.list", "batches.inspect"
        ],"object_grants":grants,"read_roots":[],"write_roots":[]
    })).await;
    let delegated = service.profile_context(&delegated_profile.id).unwrap();
    assert_ne!(delegated.principal, producer.principal);
    assert!(!delegated_profile.owned_object_access);
    assert!(delegated_profile.enabled);
    assert_eq!(delegated_profile.mode, ProfileMode::Operate);
    assert_eq!(delegated_profile.object_grants.len(), 7);
    for id in expected[1].iter().chain(&expected[3]) {
        assert!(delegated_profile.allows_object(ObjectKind::Job, id, &producer.principal, ObjectAction::Read));
    }
    assert!(delegated_profile.read_roots.is_empty() && delegated_profile.write_roots.is_empty());
    for (index, kind) in KINDS.into_iter().enumerate() {
        let wanted = if index == 0 { BTreeSet::new() } else { expected[index].clone() };
        let page = query(&service, &delegated, kind.command(), json!({"limit":64})).await;
        page_control(&page, kind, &wanted, 64);
        private_paths_absent(&page, &[media.directory.path(), root.path(), &bin]);
    }
    for id in &expected[1] {
        assert_eq!(query(&service, &delegated, "jobs.inspect", json!({"id":id})).await["id"], *id);
        let manifest = query(&service, &delegated, "artifacts.manifest", json!({"artifact_id":format!("job-{id}")})).await;
        assert_eq!(manifest["job_id"], *id);
        assert_eq!(manifest["sha256"], source_identity.sha256);
        assert_eq!(manifest["encoded_byte_count"], source_identity.bytes);
        private_paths_absent(&manifest, &[media.directory.path(), root.path(), &bin]);
    }
    for (batch, members) in &batch_members {
        let inspected = completed_batch(&service, &delegated, batch).await;
        for (index, item) in inspected["items"].as_array().unwrap().iter().enumerate() {
            assert_eq!(item["job_id"], members[index]);
        }
    }
    // Assets has no grant kind. The owning profile also loses all rows when
    // owned-object access is disabled through the public revisioned update.
    let original = producer_profile.clone();
    let mut foreign = original.clone();
    foreign.owned_object_access = false;
    producer_profile = update_profile(&service, &owner_scope, &producer_profile, &foreign).await;
    let zero = query(&service, &producer, "assets.list", json!({"limit":64})).await;
    page_control(&zero, InventoryKind::Assets, &BTreeSet::new(), 64);
    producer_profile = update_profile(&service, &owner_scope, &producer_profile, &original).await;
    assert_eq!(service.snapshots.lock().unwrap().count(), 0);
    eprintln!("real-producers: baseline assets=2 jobs=5 artifacts=5 batches=2 drained=true setup_elapsed_ms={}", started.elapsed().as_millis());

    let matrix_started = Instant::now();
    let mut completed = 0;
    for (index, kind) in KINDS.into_iter().enumerate() {
        for path in [PagePath::Fresh, PagePath::Cursor] {
            for action in [Action::Cancel, Action::Ttl, Action::Command] {
                Matrix {
                    service: &service, owner_scope: &owner_scope, context: &producer,
                    profile: &mut producer_profile, expected: &expected[index], withdrawal: None,
                }.case(kind, path, action).await;
                completed += 1;
            }
        }
    }
    let withdrawn_batch = batch_order[1].clone();
    let withdrawn_job = batch_members[&withdrawn_batch][1].clone();
    for (index, kind) in KINDS.into_iter().enumerate().skip(1) {
        for path in [PagePath::Fresh, PagePath::Cursor] {
            Matrix {
                service: &service, owner_scope: &owner_scope, context: &delegated,
                profile: &mut delegated_profile, expected: &expected[index],
                withdrawal: Some(Withdrawal { job: &withdrawn_job, batch: &withdrawn_batch, all: &expected }),
            }.case(kind, path, Action::Grant).await;
            completed += 1;
        }
    }
    assert_eq!(completed, 30);
    drained(&service).await;
    assert_eq!(service.snapshots.lock().unwrap().count(), 0);
    eprintln!("real-producers: completed cases=30 matrix_elapsed_ms={} total_elapsed_ms={}", matrix_started.elapsed().as_millis(), started.elapsed().as_millis());
}

#[derive(Clone, Copy)]
enum PagePath { Fresh, Cursor }
impl PagePath {
    fn label(self) -> &'static str { match self { Self::Fresh => "fresh", Self::Cursor => "cursor" } }
}
#[derive(Clone, Copy)]
enum Action { Cancel, Ttl, Command, Grant }
impl Action {
    fn label(self) -> &'static str {
        match self { Self::Cancel => "cancel", Self::Ttl => "ttl", Self::Command => "command", Self::Grant => "grant" }
    }
}
fn retained(service: &Service, context: &Context) -> BTreeSet<String> {
    service.snapshots.lock().unwrap().entries.iter()
        .filter(|(_, snapshot)| snapshot.principal == context.principal)
        .map(|(id, _)| id.clone()).collect()
}
async fn retire(service: &Arc<Service>, context: &Context, kind: InventoryKind, id: &str) {
    let page = query(service, context, kind.command(), json!({"limit":64,"replace_snapshot":id})).await;
    assert!(page["next_cursor"].is_null());
    assert!(retained(service, context).is_empty());
}
#[derive(Clone, Copy)]
struct Withdrawal<'a> {
    job: &'a str,
    batch: &'a str,
    all: &'a [BTreeSet<String>; 4],
}
struct Matrix<'a> {
    service: &'a Arc<Service>,
    owner_scope: &'a str,
    context: &'a Context,
    profile: &'a mut AccessProfile,
    expected: &'a BTreeSet<String>,
    withdrawal: Option<Withdrawal<'a>>,
}
impl Matrix<'_> {
    async fn case(&mut self, kind: InventoryKind, path: PagePath, action: Action) {
        let service = self.service;
        let owner_scope = self.owner_scope;
        let context = self.context;
        let profile = &mut *self.profile;
        let expected = self.expected;
        let withdrawal = self.withdrawal;
        let label = format!("{}/{}/{}", kind.command(), path.label(), action.label());
        eprintln!("real-producers: case started {label}");
        assert!(retained(service, context).is_empty(), "{label}");
        assert!(!service.busy());
        let baseline = query(service, context, kind.command(), json!({"limit":64})).await;
        page_control(&baseline, kind, expected, 64);
        let first = query(service, context, kind.command(), json!({"limit":1})).await;
        page_control(&first, kind, expected, 1);
        let ticket = first["snapshot_id"].as_str().unwrap().to_owned();
        let cursor = first["next_cursor"].as_str().unwrap().to_owned();
        let continuation = query(service, context, kind.command(), json!({"limit":1,"cursor":cursor})).await;
        assert_eq!(continuation["snapshot_id"], ticket);
        assert_eq!(continuation["projection"], kind.projection());
        assert_eq!(continuation["total_count"], expected.len());
        assert_eq!(continuation["count_accuracy"], "exact");
        assert_eq!(continuation["items"].as_array().unwrap().len(), 1);
        assert!(ids(&continuation, kind).is_subset(expected));
        assert!(ids(&continuation, kind).is_disjoint(&ids(&first, kind)));
        let same_cursor = query(service, context, kind.command(), json!({"limit":1,"cursor":cursor})).await;
        assert_eq!(same_cursor, continuation, "{label}: stable cursor identity/content");
        assert_eq!(retained(service, context), BTreeSet::from([ticket.clone()]));
        let params = match path {
            PagePath::Fresh => {
                retire(service, context, kind, &ticket).await;
                json!({"limit":1})
            }
            PagePath::Cursor => json!({"limit":1,"cursor":cursor}),
        };
        let before = retained(service, context);
        assert!(before.len() <= 1);
        let original = profile.clone();
        let mut changed = original.clone();
        match action {
            Action::Command => { assert!(changed.allowed_command_ids.remove(kind.command())); }
            Action::Grant => {
                let job = withdrawal.unwrap().job;
                let count = changed.object_grants.len();
                changed.object_grants.retain(|grant| grant.id != job);
                assert_eq!(changed.object_grants.len() + 1, count);
            }
            Action::Cancel | Action::Ttl => {}
        }
        let cancellation = CancellationToken::new();
        let (started, ready) = std::sync::mpsc::channel();
        let (resume, waiting) = std::sync::mpsc::channel();
        let phase = match path {
            PagePath::Fresh => CatalogPhase::LegacyFresh(kind),
            PagePath::Cursor => CatalogPhase::LegacyCursor(kind),
        };
        assert!(service.catalog_hook.lock().unwrap().is_none());
        *service.catalog_hook.lock().unwrap() = Some(CatalogHook { phase, started, waiting });
        let task_service = service.clone();
        let task_context = context.clone();
        let task_cancellation = cancellation.clone();
        let dispatch = request(kind.command(), params);
        let task = tokio::spawn(async move {
            task_service.dispatch_cancel(task_context, dispatch, task_cancellation).await
        });
        tokio::task::spawn_blocking(move || {
            ready.recv_timeout(Duration::from_secs(5)).expect("Real producer final page barrier not reached")
        }).await.unwrap();
        // Do not inspect/replace/count the cache while the request owns its mutex.
        assert!(!cancellation.is_cancelled(), "{label}");
        let expire = match action {
            Action::Cancel => { cancellation.cancel(); false }
            Action::Ttl => true,
            Action::Command | Action::Grant => {
                *profile = update_profile(service, owner_scope, profile, &changed).await;
                false
            }
        };
        resume.send(expire).unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), task).await
            .expect("Real producer final page dispatch did not resume").unwrap();
        let after = retained(service, context);
        assert!(!response.ok, "{label}: post-render publication must fail: {response:?}");
        let code = if matches!(action, Action::Cancel) { "CANCELLED" } else { "SNAPSHOT_EXPIRED" };
        assert_eq!(response.error.as_ref().map(|error| error.code.as_str()), Some(code), "{label}: {response:?}");
        assert!(response.result.is_none(), "{label}: {response:?}");
        assert!(response.receipt_id.is_none(), "{label}: {response:?}");
        if matches!(action, Action::Ttl) {
            // The reaper may have removed the actual expired cursor after unlock.
            assert!(after.is_subset(&before), "{label}: no newly retained ticket");
        } else {
            assert_eq!(after, before, "{label}: no newly retained ticket");
        }
        if matches!(action, Action::Grant) {
            let Withdrawal { job, batch, all } = withdrawal.unwrap();
            for (index, inventory) in KINDS.into_iter().enumerate().skip(1) {
                let mut visible = all[index].clone();
                let removed = match inventory {
                    InventoryKind::Jobs => visible.remove(job),
                    InventoryKind::Artifacts => visible.remove(&format!("job-{job}")),
                    InventoryKind::Batches => visible.remove(batch),
                    InventoryKind::Assets | InventoryKind::Projects => unreachable!(),
                };
                assert!(removed);
                let fresh = query(service, context, inventory.command(), json!({"limit":64})).await;
                page_control(&fresh, inventory, &visible, 64);
                assert_eq!(visible.len(), if index == 3 { 1 } else { 4 });
            }
        }
        if matches!(action, Action::Command | Action::Grant) {
            *profile = update_profile(service, owner_scope, profile, &original).await;
        }
        if let Some(id) = before.iter().next() {
            retire(service, context, kind, id).await;
        }
        assert!(retained(service, context).is_empty(), "{label}");
        let restored = query(service, context, kind.command(), json!({"limit":64})).await;
        page_control(&restored, kind, expected, 64);
        eprintln!("real-producers: case completed {label}");
    }
}
