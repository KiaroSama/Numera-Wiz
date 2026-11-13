//! Independently rooted genuine fresh and unadopted migration package journeys.
//! No startup Scope is reconstructed: bootstrap acceptance is inspected and its
//! actual raw receipt is preserved; only caller-issued requests are replayed.
use super::*;
use nw_domain::maintenance::media_lifecycle::{
    EnablementPhase, InitializationOrigin, MediaInitializationPlan, MediaInitializationRecord,
    RestorationKind,
};
use nw_store::generations::{Activation, GenerationRole, Selection};

fn compatible(snapshot: &Snapshot) -> Result<()> {
    ensure!(
        snapshot.facts.store_version == 3
            && snapshot.facts.required_capabilities.len()
                == nw_store::generations::CURRENT_CAPABILITIES.len()
            && snapshot.facts.required_capabilities.iter().map(String::as_str)
                .collect::<BTreeSet<_>>()
                == nw_store::generations::CURRENT_CAPABILITIES.iter().copied().collect(),
        "Origin source must have the actual legacy-compatible schema3 floor"
    );
    Ok(())
}

fn unchanged_baseline(before: &Snapshot, after: &Snapshot) -> Result<()> {
    // The only baseline cells allowed to advance are genuine canonical control
    // issuance/acceptance metadata. In particular all unknown/business/receipt
    // bytes, profiles, original guard and ancestry remain exact.
    preserved(before, after, |key| {
        !["meta/clock", "meta/required-capabilities"].contains(&key)
            && !key.starts_with("scope-rate/")
    })
}

fn one_initialization(snapshot: &Snapshot, id: &str) -> Result<()> {
    ensure!(
        snapshot.rows.keys().filter(|key| key.starts_with("generation-initialization/")).count() == 1
            && snapshot.rows.contains_key(&format!("generation-initialization/{id}"))
            && !snapshot.rows.contains_key("meta/pending-initialization")
            && !snapshot.rows.contains_key("meta/pending-restoration"),
        "Startup repeated initialization or retained pending maintenance"
    );
    Ok(())
}

fn raw_receipt(snapshot: &Snapshot, record: &MediaInitializationRecord) -> Result<Value> {
    let raw = snapshot.rows.get(&record.receipt_key).context("Actual initialization receipt")?;
    let receipt: Value = serde_json::from_slice(raw)?;
    let response = &receipt["response"];
    ensure!(
        record.receipt_key.starts_with("receipt/")
            && receipt["digest"] == record.request_digest
            && response["ok"] == true
            && response["receipt_id"].is_string()
            && response["result"]["initialization_id"] == record.id
            && response["result"]["state"] == "accepted",
        "Raw canonical receipt does not prove the observed initialization"
    );
    Ok(response.clone())
}

/// Observe genuine H and S while the real selected F remains guarded by the
/// production selected-read seam. The source selection is only a projection of
/// an actual immutable activation already validated in the full history by that
/// seam; this helper never publishes activation, opens a writer or recovers.
fn retained_origin(
    root: &Path,
    record: &MediaInitializationRecord,
    evidence: &Evidence,
) -> Result<(SelectedObject, Snapshot, Snapshot)> {
    let plan = &record.plan;
    let lease = RootLease::acquire(root)?;
    lease.with_selected_generation_read(|_, selected| {
        let observe = || -> Result<(SelectedObject, Snapshot, Snapshot)> {
            ensure!(
                selected.activation_id.as_deref() == Some(record.activation_id.as_str())
                    && selected.generation == plan.target_generation
                    && matches!(&selected.location, GenerationLocation::Workspace {
                        operation_id, role: GenerationRole::Target
                    } if operation_id == &plan.id),
                "Initialization did not select its actual target activation"
            );
            let activation_path = lease.state().join("activations")
                .join(format!("{:010}.json", plan.source_activation_sequence));
            let activation: Activation = serde_json::from_slice(
                &security::read_private(&activation_path, 65536)?
            )?;
            ensure!(
                activation.id == plan.source_activation_id
                    && activation.sequence == plan.source_activation_sequence
                    && activation.operation_id == plan.source_operation_id
                    && activation.root_id == plan.root_id
                    && activation.generation == plan.source_generation
                    && activation.store_version == 3
                    && activation.required_capabilities == plan.source_capabilities,
                "Actual compatible activation differs from initialization ancestry"
            );
            let selection = Selection {
                generation: activation.generation,
                store_version: Some(activation.store_version),
                root_id: Some(activation.root_id),
                location: activation.location,
                activation_id: Some(activation.id),
            };
            let source_expected = lease.path(&selection)?;
            let source_path = fs::canonicalize(&source_expected)?;
            ensure!(source_path == source_expected && source_path.starts_with(lease.state()), "Source escaped its actual activation path");
            let source_directory = DirectoryStamp::capture_resolved(&source_path)?;
            let _source_lock = RootLock::acquire(&source_path)?;
            let source_stamp = SourceStamp::capture_resolved(&source_path.join("store.redb"))?;
            let source = raw_locked(&source_path, selection.activation_id.clone())?;
            guard(&source, &plan.source_operation_id, "0")?;
            let original = SelectedObject {
                path: source_path.clone(),
                selection,
                object: source_stamp.object_identity()?,
            };
            let baseline_expected = lease.state().join("generation-workspaces")
                .join(&plan.id).join("rollback");
            let baseline_path = fs::canonicalize(&baseline_expected)?;
            ensure!(baseline_path == baseline_expected && baseline_path.starts_with(lease.state()), "Baseline escaped its retained operation path");
            let baseline_directory = DirectoryStamp::capture_resolved(&baseline_path)?;
            let _baseline_lock = RootLock::acquire(&baseline_path)?;
            let baseline_stamp = SourceStamp::capture_resolved(&baseline_path.join("store.redb"))?;
            let baseline = raw_locked(&baseline_path, Some(plan.source_activation_id.clone()))?;
            compatible(&baseline)?;
            ensure!(
                baseline.facts.root_id == plan.root_id
                    && baseline.facts.records_sha256 == plan.source_records_sha256
                    && baseline.facts.record_count == plan.source_records
                    && guard(&baseline, &plan.source_operation_id, "0")?
                        == guard(&source, &plan.source_operation_id, "0")?,
                "Retained actual H baseline or original S guard differs from review"
            );
            unchanged_baseline(&baseline, &source)?;
            record_snapshot("origin-retained-compatible-baseline-H", &baseline, &baseline_path, evidence)?;
            record_snapshot("origin-fenced-original-source-S", &source, &source_path, evidence)?;
            evidence.record(json!({"phase":"origin-original-source","selected_path":original.path,
                "generation":original.selection.generation,"store_version":original.selection.store_version,
                "activation_id":original.selection.activation_id,"location":original.selection.location,
                "root_id":original.selection.root_id,"native_store":original.object}))?;
            baseline_stamp.revalidate()?;
            baseline_directory.revalidate()?;
            source_stamp.revalidate()?;
            source_directory.revalidate()?;
            Ok((original, baseline, source))
        };
        observe().map_err(nw_platform::files::io_error)
    }).map_err(anyhow::Error::from)
}

async fn inspect_initialization(
    owner: &mut Owner,
    record: &MediaInitializationRecord,
    phase: &str,
    evidence: &Evidence,
) -> Result<()> {
    let inspected = owner.command(&query("maintenance.initialization.inspect", json!({"id":record.id})), evidence).await?;
    let observed: MediaInitializationRecord = serde_json::from_value(inspected["result"].clone())?;
    observed.plan.validate_hash()?;
    ensure!(
        inspected["receipt_id"].is_null()
            && inspected["result"]["phase"] == phase
            && observed.id == record.id
            && observed.receipt_key == record.receipt_key
            && observed.request_digest == record.request_digest
            && observed.activation_id == record.activation_id
            && serde_json::to_value(&observed.plan)? == serde_json::to_value(&record.plan)?,
        "Initialization inspection changed accepted identity/plan/receipt"
    );
    Ok(())
}

async fn issue_scope(owner: &mut Owner, evidence: &Evidence) -> Result<String> {
    let issued = owner.command(&query("access.replay_scope.create", json!({})), evidence).await?;
    text(&issued["result"], "replay_scope")
}

async fn replay_pair(owner: &mut Owner, pair: &Option<(Value, Value)>, evidence: &Evidence) -> Result<()> {
    if let Some((request, response)) = pair {
        same_receipt(response, &owner.command(request, evidence).await?)?;
    }
    Ok(())
}

async fn scenario(fresh: bool, evidence: &Evidence) -> Result<Value> {
    let started = Instant::now();
    let configuration = configuration(evidence)?;
    let temporary = tempfile::tempdir()?;
    let root = fs::canonicalize(security::private_directory(&temporary.path().join("origin-root"))?)?;
    ensure!(
        fs::read_dir(&root)?.next().is_none()
            && !root.starts_with(configuration.old.path.parent().context("Old executable parent")?)
            && !root.starts_with(configuration.new.path.parent().context("Current executable parent")?),
        "Origin journey must begin with genuinely absent application state outside packages"
    );
    let origin_name = if fresh { "fresh" } else { "migrated-r0" };
    evidence.record(json!({"phase":"origin-started","origin":origin_name,"canonical_root":root,
        "absent_application_state":true,"same_600_second_aggregate":true,
        "prior_source":configuration.old.source,"current_source":configuration.new.source,
        "prior_executable":configuration.old.identity,"current_executable":configuration.new.identity}))?;
    let mut epochs = BTreeSet::new();
    let mut root_id = None;
    let mut migration = None;
    let mut migrated_source = None;
    let mut migrated_baseline = None;
    let mut initialization = None;

    if !fresh {
        let mut owner = Owner::start(&configuration.old, &root, &mut epochs, &mut root_id, evidence).await?;
        let scope = issue_scope(&mut owner, evidence).await?;
        // The unchanged old canonical producer is the only source of ancestry.
        // Issuing this scope before review avoids changing the reviewed raw hash.
        let review = owner.command(&query("maintenance.plan", json!({})), evidence).await?;
        ensure!(
            review["receipt_id"].is_null()
                && review["result"]["source_store_version"] == 2
                && review["result"]["target_store_version"] == 3
                && review["result"]["application_build"] == configuration.old.hash()?
                && review["result"]["root_id"].as_str() == root_id.as_deref(),
            "Old package must genuinely review its own schema2-to3 migration"
        );
        let request = durable(&scope, "origin-old-migration", "maintenance.migrate", json!({"plan":review["result"]}), None);
        let accepted = owner.command(&request, evidence).await?;
        same_receipt(&accepted, &owner.command(&request, evidence).await?)?;
        let id = text(&accepted["result"], "migration_id")?;
        ensure!(review["result"]["id"] == id && accepted["result"]["state"] == "accepted", "Old migration acceptance identity differs");
        let inspected = owner.command(&query("maintenance.inspect", json!({"id":id})), evidence).await?;
        ensure!(inspected["result"]["id"] == id && inspected["result"]["phase"] == "succeeded"
            && inspected["receipt_id"].is_null(), "Actual old migration did not succeed");
        owner.stop(evidence).await?;
        let snapshot = closed(&root, "origin-old-migrated-r0", evidence)?;
        compatible(&snapshot)?;
        guard(&snapshot, &id, "0")?;
        ensure!(
            !snapshot.rows.contains_key("meta/genesis-origin")
                && !snapshot.rows.keys().any(|key| key.starts_with("generation-initialization/")),
            "Migrated origin must not contain manufactured fresh initialization"
        );
        migrated_source = Some(selected_object(&root)?);
        migrated_baseline = Some(snapshot);
        migration = Some((request, accepted));
    }

    let requests = if fresh {
        // Nothing, including a requests directory, is seeded before startup.
        let owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
        owner.stop(evidence).await?;
        security::private_directory(&root.join("requests"))?
    } else {
        let requests = security::private_directory(&root.join("requests"))?;
        let mut owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
        replay_pair(&mut owner, &migration, evidence).await?;
        let scope = issue_scope(&mut owner, evidence).await?;
        let review = owner.command(&query("maintenance.initialization.plan", json!({})), evidence).await?;
        let plan: MediaInitializationPlan = serde_json::from_value(review["result"].clone())?;
        plan.validate_hash()?;
        let source = migrated_source.as_ref().context("Migrated native source")?;
        ensure!(
            review["receipt_id"].is_null()
                && plan.origin == InitializationOrigin::MigratedZeroRevision
                && plan.source_guard_revision == "0"
                && Some(plan.source_activation_id.as_str()) == source.selection.activation_id.as_deref()
                && plan.source_generation == source.selection.generation
                && plan.source_operation_id == text(&migration.as_ref().context("Old migration")?.1["result"], "migration_id")?,
            "Current startup auto-initialized or changed genuine unadopted migration ancestry"
        );
        let request = durable(&scope, "origin-explicit-initialization", "maintenance.initialization.accept", json!({"plan":review["result"]}), None);
        retain_request(&requests, &request)?;
        let accepted = owner.command(&request, evidence).await?;
        retain_response(&requests, &accepted)?;
        let inspected = owner.command(&query("maintenance.initialization.inspect", json!({"id":plan.id})), evidence).await?;
        ensure!(inspected["result"]["id"] == plan.id
            && inspected["result"]["phase"] == "succeeded"
            && inspected["receipt_id"].is_null(), "Explicit initialization did not complete before old-floor refusal");
        same_receipt(&accepted, &owner.command(&request, evidence).await?)?;
        initialization = Some((request, accepted));
        owner.stop(evidence).await?;
        requests
    };
    if let Some((request, response)) = &migration {
        retain_request(&requests, request)?;
        retain_response(&requests, response)?;
    }
    let initialized = closed(&root, "origin-initialized-target-F", evidence)?;
    let selected = selected_object(&root)?;
    let GenerationLocation::Workspace { operation_id: initialization_id, role: GenerationRole::Target } = &selected.selection.location else {
        anyhow::bail!("Genuine initialization must select its actual workspace target");
    };
    let record: MediaInitializationRecord = serde_json::from_slice(initialized.rows
        .get(&format!("generation-initialization/{initialization_id}")).context("Actual initialization row")?)?;
    record.plan.validate_hash()?;
    let expected_origin = if fresh {
        InitializationOrigin::CompatibleGenesisV1
    } else {
        InitializationOrigin::MigratedZeroRevision
    };
    ensure!(
        record.id == *initialization_id && record.plan.id == record.id
            && record.plan.root_id == initialized.facts.root_id
            && record.phase == EnablementPhase::Succeeded
            && record.completed_at.is_some()
            && record.plan.application_build == configuration.new.hash()?
            && record.plan.source_guard_revision == "0"
            && record.plan.origin == expected_origin
            && initialized.facts.required_capabilities.iter().map(String::as_str).collect::<BTreeSet<_>>()
                == MEDIA_V1_CAPABILITIES.iter().copied().collect(),
        "Actual automatic/explicit initialization did not establish its genuine record and floor"
    );
    guard(&initialized, &record.id, "0")?;
    one_initialization(&initialized, &record.id)?;
    let accepted_initialization = raw_receipt(&initialized, &record)?;
    if let Some((_, accepted)) = &initialization {
        same_receipt(accepted, &accepted_initialization)?;
    } else {
        // The actual accepted Response is available from its raw receipt. No
        // Request or opaque internal bootstrap Scope can be recovered from it.
        retain_response(&requests, &accepted_initialization)?;
    }
    let receipt_bytes = initialized.rows.get(&record.receipt_key).context("Initialization raw receipt")?;
    security::write_private(&requests.join("initialization-raw-receipt.json"), receipt_bytes, false)?;
    let (original_source, baseline, fenced_source) = retained_origin(&root, &record, evidence)?;
    if let Some(source) = &migrated_source {
        ensure!(*source == original_source, "Initialization replaced migrated original S identity");
        unchanged_baseline(migrated_baseline.as_ref().context("Migrated raw baseline")?, &baseline)?;
    } else {
        ensure!(
            record.plan.source_generation == 1 && record.plan.source_activation_sequence == 1
                && matches!(&original_source.selection.location, GenerationLocation::LegacyOne {})
                && baseline.rows.get("meta/genesis-origin").is_some_and(|id| id == record.plan.source_operation_id.as_bytes())
                && baseline.rows.contains_key(&format!("generation-genesis/{}", record.plan.source_operation_id)),
            "Fresh root lacks its actual compatible genesis S identity"
        );
    }
    evidence.record(json!({"phase":"origin-initialization-proved","origin":origin_name,
        "initialization_id":record.id,"initialization_receipt":accepted_initialization["receipt_id"],
        "raw_receipt_sha256":sha(receipt_bytes),"source_operation_id":record.plan.source_operation_id,
        "source_activation_id":record.plan.source_activation_id,"fenced_activation_id":record.activation_id,
        "bootstrap_request_replay":if fresh { "unavailable: internal opaque Scope is not exported; inspect identity and raw receipt preservation only" } else { "genuine caller-issued Scope and retained Request/Response replayed" }}))?;

    let physical_before = physical_state(&root)?;
    let refused = old_floor_refusal(&configuration.old, &root, evidence).await?;
    ensure!(refused["exit_code"] == 3, "Old floor refusal must be actual exit3");
    // First operation after the refused child exits: direct physical/native
    // observation, with no RootLease, writable database or recovery in between.
    let physical_after = physical_state(&root)?;
    ensure!(physical_before == physical_after, "Old refusal mutated physical origin state");
    evidence.record(json!({"phase":"origin-old-floor-no-mutation","origin":origin_name,
        "before_current_recovery":true,"old_refusal":refused,"physical":physical_after}))?;

    let mut owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
    inspect_initialization(&mut owner, &record, "succeeded", evidence).await?;
    replay_pair(&mut owner, &initialization, evidence).await?;
    replay_pair(&mut owner, &migration, evidence).await?;
    // A genuinely caller-issued Scope must precede the stopped restoration
    // review. It never substitutes for startup's accepted bootstrap Scope.
    let restoration_scope = issue_scope(&mut owner, evidence).await?;
    let review = owner.command(&query("maintenance.initialization.restore.plan", json!({"id":record.id})), evidence).await?;
    let restore: GenerationRestorationPlan = serde_json::from_value(review["result"].clone())?;
    restore.validate_hash()?;
    ensure!(
        review["receipt_id"].is_null() && restore.kind == RestorationKind::InitializationRestorationV1
            && restore.enablement_id == record.id
            && restore.source_operation_id == record.plan.source_operation_id
            && restore.source_activation_id == record.plan.source_activation_id
            && restore.source_generation == record.plan.source_generation
            && restore.source_guard_revision == "0"
            && restore.fenced_activation_id == record.activation_id,
        "Stopped r0 restoration lost the genuine origin ancestry"
    );
    let restoration = durable(&restoration_scope, "origin-stopped-restoration", "maintenance.initialization.restore.accept", json!({"plan":review["result"]}), None);
    retain_request(&requests, &restoration)?;
    let restored_receipt = owner.command(&restoration, evidence).await?;
    retain_response(&requests, &restored_receipt)?;
    ensure!(restored_receipt["result"]["restoration_id"] == restore.id, "Restoration acceptance identity differs");
    same_receipt(&restored_receipt, &owner.command(&restoration, evidence).await?)?;
    inspect_initialization(&mut owner, &record, "restored", evidence).await?;
    let inspected = owner.command(&query("maintenance.initialization.restore.inspect", json!({"id":restore.id})), evidence).await?;
    ensure!(inspected["receipt_id"].is_null() && inspected["result"]["id"] == restore.id
        && inspected["result"]["status"] == "armed", "Actual restoration barrier was not inspected");
    owner.stop(evidence).await?;
    let restored = closed(&root, "origin-restored-original-S", evidence)?;
    same_selected_source(&root, &original_source, "origin-after-restoration", evidence)?;
    compatible(&restored)?;
    one_initialization(&restored, &record.id)?;
    ensure!(guard(&restored, &record.plan.source_operation_id, "0")?
        == guard(&baseline, &record.plan.source_operation_id, "0")?, "Restoration reset original guard bytes");
    unchanged_baseline(&baseline, &restored)?;
    preserved(&initialized, &restored, |key| key.starts_with("receipt/"))?;
    // S retains its genuine accepted journal bytes. F separately records
    // completion; inspect derives "restored" from the real exposure barrier.
    // Restoration does not rewrite S's original acceptance into F's record.
    preserved(&fenced_source, &restored, |key| key.starts_with("generation-initialization/"))?;
    ensure!(restored.rows.contains_key(&format!("generation-restoration/{}", restore.id)), "Newer restoration row was lost");
    same_receipt(&accepted_initialization, &raw_receipt(&restored, &record)?)?;

    let mut owner = Owner::start(&configuration.old, &root, &mut epochs, &mut root_id, evidence).await?;
    replay_pair(&mut owner, &migration, evidence).await?;
    let old_scope = issue_scope(&mut owner, evidence).await?;
    let old_write = durable(&old_scope, "origin-old-business", "access.profiles.create",
        json!({"name":"origin-after-restoration","read_roots":[],"write_roots":[]}), None);
    retain_request(&requests, &old_write)?;
    let written = owner.command(&old_write, evidence).await?;
    retain_response(&requests, &written)?;
    ensure!(
        accepted_initialization["receipt_id"] != restored_receipt["receipt_id"]
            && written["receipt_id"] != accepted_initialization["receipt_id"]
            && written["receipt_id"] != restored_receipt["receipt_id"],
        "Distinct genuine acceptances must retain distinct receipt identities"
    );
    same_receipt(&written, &owner.command(&old_write, evidence).await?)?;
    owner.stop(evidence).await?;
    let after_old = closed(&root, "origin-old-business-r1", evidence)?;
    same_selected_source(&root, &original_source, "origin-after-old-business", evidence)?;
    guard(&after_old, &record.plan.source_operation_id, "1")?;
    compatible(&after_old)?;
    ensure!(after_old.rows.contains_key("profile/origin-after-restoration"), "Old business profile was not retained");
    preserved(&restored, &after_old, |key| !["meta/clock", "meta/rollback-guard"].contains(&key) && !key.starts_with("scope-rate/"))?;
    one_initialization(&after_old, &record.id)?;

    let mut owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
    inspect_initialization(&mut owner, &record, "restored", evidence).await?;
    replay_pair(&mut owner, &migration, evidence).await?;
    replay_pair(&mut owner, &initialization, evidence).await?;
    same_receipt(&written, &owner.command(&old_write, evidence).await?)?;
    same_receipt(&restored_receipt, &owner.command(&restoration, evidence).await?)?;
    let next = owner.command(&query("maintenance.promotion.plan", json!({})), evidence).await?;
    let next: GenerationPromotionPlan = serde_json::from_value(next["result"].clone())?;
    next.validate_hash()?;
    ensure!(
        next.source_guard.migration_id == record.plan.source_operation_id
            && next.source_guard.activation_id == record.plan.source_activation_id
            && next.source_guard.revision == "1"
            && next.source_generation == record.plan.source_generation
            && next.source_activation_id == record.plan.source_activation_id
            && next.source_records_sha256 == after_old.facts.records_sha256,
        "Current promotion review did not accept the genuine now-adopted origin and exact guard1"
    );
    owner.stop(evidence).await?;
    let final_raw = closed(&root, "origin-final-current-replays", evidence)?;
    same_selected_source(&root, &original_source, "origin-after-final-current-reopen", evidence)?;
    compatible(&final_raw)?;
    one_initialization(&final_raw, &record.id)?;
    ensure!(guard(&final_raw, &record.plan.source_operation_id, "1")?
        == guard(&after_old, &record.plan.source_operation_id, "1")?, "Final replay changed original guard1");
    preserved(&after_old, &final_raw, |key| key != "meta/clock")?;
    ensure!(final_raw.rows.get(&record.receipt_key) == Some(receipt_bytes), "Actual initialization raw receipt changed across all owners");
    let expected_epochs = if fresh { 4 } else { 5 };
    ensure!(epochs.len() == expected_epochs, "Origin owners must each establish an independent genuine epoch");
    let completion = json!({"phase":"origin-package-complete","status":"passed","origin":origin_name,
        "root_id":root_id,"owner_epochs":epochs.len(),"epochs":epochs,
        "elapsed_journey_ms":u64::try_from(started.elapsed().as_millis())?,
        "prior_source":configuration.old.source,"current_source":configuration.new.source,
        "prior_executable":configuration.old.identity,"current_executable":configuration.new.identity,
        "initialization_id":record.id,"restoration_id":restore.id,
        "initialization_receipt":accepted_initialization["receipt_id"],
        "initialization_raw_receipt_sha256":sha(receipt_bytes),
        "restoration_receipt":restored_receipt["receipt_id"],"old_business_receipt":written["receipt_id"],
        "source_operation_id":record.plan.source_operation_id,"source_activation_id":record.plan.source_activation_id,
        "source_generation":record.plan.source_generation,"source_native":original_source.object,
        "source_guard_before":"0","source_guard_after_old_business":"1",
        "old_floor_outcome":refused,"direct_physical_equality_before_recovery":true,
        "original_S_path_selection_native_preserved":true,"newer_raw_rows_receipts_preserved":true,
        "no_repeated_initialization_or_overlay":true,"caller_restoration_replayed":true,
        "caller_initialization_replayed":!fresh,"old_business_replayed_on_current":true,
        "bootstrap_scope_boundary":if fresh { "internal accepted Request/Scope unavailable; genuine inspect identity plus raw receipt preservation" } else { "caller-issued Scope retained privately and genuine accepted Request/Response replayed" },
        "scope":"Actual origin package lifecycle only; cuts, RSS, combined-resource qualification, general media, named clients, owner hardware, full D12 and full v1 remain unqualified"});
    evidence.record(completion.clone())?;
    Ok(completion)
}

pub(super) async fn run(fresh: bool, evidence: &Evidence) -> Result<Value> {
    let result = scenario(fresh, evidence).await;
    if result.is_err() {
        evidence.record(json!({"phase":"origin-failed","status":"failed",
            "reason":"Actual origin assertion failed; last sanitized event identifies boundary; owned children drop for cleanup and root is not reopened"}))?;
    }
    result
}
