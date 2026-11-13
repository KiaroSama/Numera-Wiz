//! Actual pinned-old/current package floor, restoration and forward preservation.
//! This is appended after the original seven-epoch assertions, under their deadline.
use super::*;
use nw_domain::maintenance::{
    media_lifecycle::GenerationRestorationPlan,
    promotion::{GenerationPromotionPlan, MEDIA_V1_CAPABILITIES},
};
use nw_platform::files::{DirectoryStamp, SourceStamp};
use tokio_util::sync::CancellationToken;

pub(super) struct Journey<'a> {
    pub configuration: &'a Configuration,
    pub root: &'a Path,
    pub epochs: &'a mut BTreeSet<String>,
    pub root_id: &'a mut Option<String>,
    pub evidence: &'a Evidence,
    pub scope: &'a str,
    pub old_profile: (&'a Value, &'a Value),
    pub migration: (&'a Value, &'a Value),
    pub before: &'a Snapshot,
}

/// Read physical state directly, without a RootLease, database open or recovery.
/// Compare this BEFORE any current-reader startup after the refused old process.
fn physical_state(root: &Path) -> Result<Value> {
    let state = fs::canonicalize(root.join("state"))?;
    let _owner = RootLock::acquire(&state)?;
    let mut pending = vec![(state.clone(), 0usize)];
    let mut entries = 0usize;
    let mut bytes = 0u64;
    let mut objects = BTreeMap::new();
    let cancel = CancellationToken::new();
    while let Some((path, depth)) = pending.pop() {
        ensure!(depth <= 8, "Physical fixture depth exceeds bound");
        let directory = DirectoryStamp::capture_resolved(&path)?;
        let relative = path.strip_prefix(&state)?;
        objects.insert(
            relative.to_str().context("Physical relative directory encoding")?.replace('\\', "/"),
            json!({"kind":"directory","native":directory.object_identity()?}),
        );
        directory.for_each_entry(|path| {
            let observe = || -> Result<()> {
                entries += 1;
                ensure!(entries <= 256, "Physical fixture entry count exceeds bound");
                let metadata = fs::symlink_metadata(&path)?;
                ensure!(!metadata.file_type().is_symlink(), "Physical fixture contains a symlink");
                if metadata.is_dir() {
                    pending.push((path, depth + 1));
                } else {
                    ensure!(metadata.is_file(), "Physical fixture contains an unsupported object");
                    let remaining = (32u64 * 1024 * 1024).checked_sub(bytes)
                        .context("Physical fixture byte budget")?;
                    let source = SourceStamp::capture_resolved(&path)?;
                    let native = source.object_identity()?;
                    let identity = source.identity_with_limit(&cancel, remaining)?;
                    let length = identity.bytes.parse::<u64>()?;
                    bytes = bytes.checked_add(length).context("Physical fixture bytes")?;
                    objects.insert(
                        path.strip_prefix(&state)?.to_str().context("Physical relative file encoding")?.replace('\\', "/"),
                        json!({"kind":"file","native":native,"bytes":identity.bytes,"sha256":identity.sha256}),
                    );
                }
                Ok(())
            };
            observe().map_err(nw_platform::files::io_error)
        })?;
        directory.revalidate()?;
    }
    ensure!(!objects.is_empty() && bytes > 0, "Physical fixture must contain genuine data");
    Ok(json!({"entries":entries,"bytes":bytes,"objects":objects}))
}

async fn old_floor_refusal(binary: &Binary, root: &Path, evidence: &Evidence) -> Result<Value> {
    binary.verify()?;
    let mut child = Command::new(&binary.path)
        .arg("--data-dir").arg(root).arg("__owner")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn()?;
    let stdout = tokio::spawn(drain(child.stdout.take().context("Refused prior stdout")?));
    let stderr = tokio::spawn(drain(child.stderr.take().context("Refused prior stderr")?));
    let status = match tokio::time::timeout(Duration::from_secs(45), child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            evidence.record(json!({"phase":"media-old-floor-failed","reason":"Prior reader failed to refuse within45 seconds; only owned child terminated for cleanup","counted_as_refusal":false}))?;
            anyhow::bail!("Actual prior reader floor refusal exceeded its deadline");
        }
    };
    let output = tokio::time::timeout(Duration::from_secs(5), stdout).await???;
    let errors = tokio::time::timeout(Duration::from_secs(5), stderr).await???;
    ensure!(output.total <= MAX_BODY as u64 && errors.total <= MAX_BODY as u64,
        "Prior refusal diagnostics exceed the retained bound");
    let diagnostics = String::from_utf8_lossy(&errors.bytes);
    let floor = diagnostics.contains("STORE_VERSION_UNSUPPORTED");
    let busy = diagnostics.contains("OWNER_BUSY");
    evidence.record(json!({"phase":"media-old-floor-exit","binary":binary.label,"source":binary.source,
        "build":binary.identity["sha256"],"exit_code":status.code(),"cleanly_exited":true,
        "killed":false,"recognized_floor_error":floor,"owner_busy":busy,
        "stdout_bytes":output.total,"stdout_sha256":sha(&output.bytes),
        "stderr_bytes":errors.total,"stderr_sha256":sha(&errors.bytes)}))?;
    ensure!(status.code().is_some_and(|code| code != 0) && floor && !busy,
        "Prior reader must actually exit with STORE_VERSION_UNSUPPORTED, not owner contention");
    Ok(json!({"exit_code":status.code(),"error_code":"STORE_VERSION_UNSUPPORTED","killed":false,
        "stderr_bytes":errors.total,"stderr_sha256":sha(&errors.bytes)}))
}

async fn old_replays(owner: &mut Owner, journey: &Journey<'_>) -> Result<()> {
    same_receipt(journey.old_profile.1,
        &owner.command(journey.old_profile.0, journey.evidence).await?)?;
    same_receipt(journey.migration.1,
        &owner.command(journey.migration.0, journey.evidence).await?)
}

pub(super) async fn run(journey: Journey<'_>) -> Result<()> {
    let started = Instant::now();
    let original_epochs = journey.epochs.len();
    ensure!(original_epochs == 7, "Media package journey must follow all original seven epochs");
    let migration_id = text(&journey.migration.1["result"], "migration_id")?;
    let source_guard = guard(journey.before, &migration_id, "1")?;
    let mut owner = Owner::start(&journey.configuration.new, journey.root,
        journey.epochs, journey.root_id, journey.evidence).await?;
    let review = owner.command(&query("maintenance.promotion.plan", json!({})), journey.evidence).await?;
    ensure!(review["receipt_id"].is_null(), "Promotion review must not invent a receipt");
    let plan: GenerationPromotionPlan = serde_json::from_value(review["result"].clone())?;
    plan.validate_hash()?;
    ensure!(plan.source_guard.migration_id == migration_id && plan.source_guard.revision == "1"
        && Some(plan.source_activation_id.as_str()) == journey.before.activation.as_deref()
        && plan.source_records_sha256 == journey.before.facts.records_sha256,
        "Promotion review must capture the actual adopted package source");
    let promotion = durable(journey.scope, "compat-media-promotion", "maintenance.promotion.accept",
        json!({"plan":review["result"]}), None);
    let promoted_receipt = owner.command(&promotion, journey.evidence).await?;
    same_receipt(&promoted_receipt, &owner.command(&promotion, journey.evidence).await?)?;
    let inspected = owner.command(&query("maintenance.promotion.inspect", json!({"id":plan.id})), journey.evidence).await?;
    ensure!(inspected["result"]["phase"] == "succeeded", "Package promotion did not complete");
    owner.stop(journey.evidence).await?;
    let promoted = closed(journey.root, "media-package-promoted", journey.evidence)?;
    ensure!(promoted.facts.required_capabilities.iter().map(String::as_str).collect::<BTreeSet<_>>()
        == MEDIA_V1_CAPABILITIES.iter().copied().collect::<BTreeSet<_>>(), "Media reader floor is not actual");
    projects_identical(journey.before, &promoted)?;

    let physical_before = physical_state(journey.root)?;
    let refused = old_floor_refusal(&journey.configuration.old, journey.root, journey.evidence).await?;
    // Do not acquire RootLease or run current recovery between old exit and this
    // comparison: recovery must never hide a mutation made by the refused reader.
    let physical_after = physical_state(journey.root)?;
    ensure!(physical_before == physical_after, "Refused old reader mutated physical state or identities");
    journey.evidence.record(json!({"phase":"media-old-floor-no-mutation","physical":physical_after,
        "before_current_recovery":true,"old_refusal":refused}))?;

    let mut owner = Owner::start(&journey.configuration.new, journey.root,
        journey.epochs, journey.root_id, journey.evidence).await?;
    same_receipt(&promoted_receipt, &owner.command(&promotion, journey.evidence).await?)?;
    old_replays(&mut owner, &journey).await?;
    let review = owner.command(&query("maintenance.promotion.restore.plan", json!({"id":plan.id})), journey.evidence).await?;
    let restore: GenerationRestorationPlan = serde_json::from_value(review["result"].clone())?;
    restore.validate_hash()?;
    ensure!(restore.enablement_id == plan.id && restore.source_operation_id == migration_id
        && restore.source_activation_id == plan.source_activation_id && restore.source_guard_revision == "1",
        "Restoration review lost actual promotion/source ancestry");
    let restoration = durable(journey.scope, "compat-media-restoration", "maintenance.promotion.restore.accept",
        json!({"plan":review["result"]}), None);
    let restored_receipt = owner.command(&restoration, journey.evidence).await?;
    same_receipt(&restored_receipt, &owner.command(&restoration, journey.evidence).await?)?;
    let inspected = owner.command(&query("maintenance.promotion.inspect", json!({"id":plan.id})), journey.evidence).await?;
    ensure!(inspected["result"]["phase"] == "restored", "Package restoration did not expose compatible source");
    owner.stop(journey.evidence).await?;
    let restored = closed(journey.root, "media-package-restored-source", journey.evidence)?;
    ensure!(guard(&restored, &migration_id, "1")? == source_guard,
        "Restoration changed the source adoption provenance");
    projects_identical(journey.before, &restored)?;
    ensure!(restored.facts.required_capabilities.iter().map(String::as_str).collect::<BTreeSet<_>>()
        == nw_store::generations::CURRENT_CAPABILITIES.iter().copied().collect::<BTreeSet<_>>(),
        "Restored source is not the actual legacy-compatible reader floor");
    ensure!(restored.rows.contains_key(&format!("generation-promotion/{}", plan.id))
        && restored.rows.contains_key(&format!("generation-restoration/{}", restore.id)),
        "Actual newer control rows were not retained for the old reader");

    let mut owner = Owner::start(&journey.configuration.old, journey.root,
        journey.epochs, journey.root_id, journey.evidence).await?;
    old_replays(&mut owner, &journey).await?;
    let old_write = durable(journey.scope, "compat-old-business-after-media-restore", "access.profiles.create",
        json!({"name":"compat-after-media-restoration","read_roots":[],"write_roots":[]}), None);
    let written = owner.command(&old_write, journey.evidence).await?;
    same_receipt(&written, &owner.command(&old_write, journey.evidence).await?)?;
    owner.stop(journey.evidence).await?;
    let after_old = closed(journey.root, "media-package-old-business-preserved", journey.evidence)?;
    guard(&after_old, &migration_id, "2")?;
    // Every existing row, including newer unknown control/receipt rows, must stay
    // exact except the two metadata cells changed by this genuine old mutation.
    preserved(&restored, &after_old, |key| !["meta/clock", "meta/rollback-guard"].contains(&key))?;
    projects_identical(&restored, &after_old)?;

    let mut owner = Owner::start(&journey.configuration.new, journey.root,
        journey.epochs, journey.root_id, journey.evidence).await?;
    old_replays(&mut owner, &journey).await?;
    same_receipt(&written, &owner.command(&old_write, journey.evidence).await?)?;
    same_receipt(&promoted_receipt, &owner.command(&promotion, journey.evidence).await?)?;
    same_receipt(&restored_receipt, &owner.command(&restoration, journey.evidence).await?)?;
    let next = owner.command(&query("maintenance.promotion.plan", json!({})), journey.evidence).await?;
    let next: GenerationPromotionPlan = serde_json::from_value(next["result"].clone())?;
    next.validate_hash()?;
    ensure!(next.source_guard.migration_id == migration_id && next.source_guard.revision == "2"
        && next.source_activation_id == plan.source_activation_id,
        "Current restart did not retain genuine old post-restoration work");
    owner.stop(journey.evidence).await?;
    let final_raw = closed(journey.root, "media-package-final-replay-preserved", journey.evidence)?;
    guard(&final_raw, &migration_id, "2")?;
    preserved(&after_old, &final_raw, |key| key != "meta/clock")?;
    ensure!(journey.epochs.len() == original_epochs + 4, "Added package owners must establish four genuine epochs");
    journey.evidence.record(json!({"phase":"media-package-complete","status":"passed",
        "added_elapsed_ms":u64::try_from(started.elapsed().as_millis())?,"original_epochs":original_epochs,
        "additional_epochs":4,"source_guard_before":"1","source_guard_after_old_business":"2",
        "prior_source":journey.configuration.old.source,"current_source":journey.configuration.new.source,
        "promotion_id":plan.id,"restoration_id":restore.id,"promotion_receipt":promoted_receipt["receipt_id"],
        "restoration_receipt":restored_receipt["receipt_id"],"old_business_receipt":written["receipt_id"],
        "old_floor_refusal_before_current_recovery":true,"old_newer_rows_preserved":true,
        "scope":"Actual adopted-r1 package promotion/refusal/restoration/old-write/current-replay; fresh/r0/cut-point/RSS/general-media/full-v1 remain unqualified"}))?;
    Ok(())
}
