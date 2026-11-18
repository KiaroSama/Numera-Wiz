//! Exercise the immutable MediaV1 executable and the reviewed mixed reader floor.
use super::*;
use nw_domain::maintenance::media_v2::{EnablementPlanV2, MEDIA_V2_CAPABILITIES};

fn baseline() -> Result<Binary> {
    let raw = include_bytes!("../../../../../../ci/media-v1-compatibility-baseline.json");
    let manifest: Value = serde_json::from_slice(raw)?;
    let evidence = environment_path("NW_COMPAT_EVIDENCE_DIR")?;
    let prepared = read_json(&evidence.join("media-v1-preparation.json"), 65536)?;
    let label = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("windows", "x86_64") => "windows-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        _ => anyhow::bail!("Unsupported MediaV1 compatibility target"),
    };
    let target = manifest["targets"].as_array().context("MediaV1 targets")?.iter()
        .find(|target| target["target"] == label).context("MediaV1 target is missing")?;
    let binary = Binary {
        path: environment_path("NW_COMPAT_MEDIA_V1_EXE")?,
        identity: json!({"bytes":target["executable_bytes"],"sha256":target["executable_sha256"]}),
        source: text(&manifest, "source_sha")?,
        label: "media-v1-prior",
    };
    ensure!(
        prepared["status"] == "prepared" && prepared["source_sha"] == binary.source
            && prepared["run_id"] == manifest["run_id"] && prepared["target"] == label
            && prepared["manifest_sha256"] == sha(raw) && prepared["executable"] == binary.identity,
        "MediaV1 package preparation differs from its frozen source or native executable"
    );
    binary.verify()?;
    Ok(binary)
}

pub(in crate::project_compatibility) async fn run(evidence: &Evidence) -> Result<Value> {
    let configuration = configuration(evidence)?;
    let prior = baseline()?;
    let temporary = tempfile::tempdir()?;
    let root = fs::canonicalize(security::private_directory(&temporary.path().join("media-v1-to-mixed"))?)?;
    let mut epochs = BTreeSet::new();
    let mut root_id = None;
    let mut owner = Owner::start(&prior, &root, &mut epochs, &mut root_id, evidence).await?;
    let issued = owner.command(&query("access.replay_scope.create", json!({})), evidence).await?;
    let scope = text(&issued["result"], "replay_scope")?;
    owner.stop(evidence).await?;
    let source = selected_object(&root)?;
    let before = closed(&root, "media-v1-origin", evidence)?;
    ensure!(
        before.facts.required_capabilities == MEDIA_V1_CAPABILITIES,
        "Mixed compatibility must begin with the real MediaV1 reader floor"
    );
    let mut owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
    let review = owner.command(&query("maintenance.media_v2.plan", json!({"kind":"media_initialization_v2"})), evidence).await?;
    let plan: EnablementPlanV2 = serde_json::from_value(review["result"].clone())?;
    plan.validate_hash()?;
    ensure!(plan.source.guard_revision == "0" && plan.source.records_sha256 == before.facts.records_sha256,
        "Reviewed mixed initialization differs from its genuine preceding raw rows");
    let enable = durable(&scope, "mixed-origin-enable", "maintenance.media_v2.accept", json!({"plan":plan}), None);
    let enabled = owner.command(&enable, evidence).await?;
    owner.stop(evidence).await?;

    let upgraded = closed(&root, "mixed-origin-enabled", evidence)?;
    ensure!(upgraded.facts.required_capabilities == MEDIA_V2_CAPABILITIES,
        "Reviewed mixed reader floor was not activated");
    let physical = physical_state(&root)?;
    let refused = old_floor_refusal(&prior, &root, evidence).await?;
    ensure!(refused["exit_code"] == 3 && physical_state(&root)? == physical,
        "Prior reader must reject the mixed floor before changing its physical files");
    let mut owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
    same_receipt(&enabled, &owner.command(&enable, evidence).await?)?;
    let review = owner.command(&query("maintenance.media_v2.restore.plan", json!({"id":enabled["result"]["enablement_id"]})), evidence).await?;
    let plan: GenerationRestorationPlan = serde_json::from_value(review["result"].clone())?;
    plan.validate_hash()?;
    ensure!(plan.kind == nw_domain::maintenance::media_lifecycle::RestorationKind::InitializationRestorationV2,
        "Restoration did not select the matching initialization profile");
    let restore = durable(&scope, "mixed-origin-restore", "maintenance.media_v2.restore.accept", json!({"plan":plan}), None);
    let restored = owner.command(&restore, evidence).await?;
    owner.stop(evidence).await?;
    same_selected_source(&root, &source, "mixed-origin-restored", evidence)?;
    let rolled_back = closed(&root, "mixed-origin-restored", evidence)?;
    preserved(&before, &rolled_back, |key| key != "meta/clock" && !key.starts_with("scope-rate/"))?;
    let mut owner = Owner::start(&prior, &root, &mut epochs, &mut root_id, evidence).await?;
    let business = durable(&scope, "mixed-origin-old-business", "projects.create", json!({"name":"Prior package after mixed restoration"}), None);
    let written = owner.command(&business, evidence).await?;
    owner.stop(evidence).await?;
    same_selected_source(&root, &source, "mixed-origin-prior-reopened", evidence)?;
    let mut owner = Owner::start(&configuration.new, &root, &mut epochs, &mut root_id, evidence).await?;
    for (request, response) in [(&enable, &enabled), (&restore, &restored), (&business, &written)] {
        same_receipt(response, &owner.command(request, evidence).await?)?;
    }
    owner.stop(evidence).await?;
    let result = json!({"phase":"mixed-origin-package-complete","status":"passed","root_id":root_id,
        "epochs":epochs,"prior_source":prior.source,"prior_executable":prior.identity,
        "current_source":configuration.new.source,"current_executable":configuration.new.identity,
        "prior_floor_refusal":refused,"original_source_object":source.object,"original_native_source_preserved":true,
        "prior_and_current_replays_preserved":true});
    evidence.record(result.clone())?;
    Ok(result)
}
