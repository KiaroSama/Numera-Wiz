//! Closed F/S/H observation derived solely from the actual selected initialization.
use super::*;
use crate::{RECORDS, decode, media_generations};
use nw_domain::maintenance::{
    media_lifecycle::{EnablementPhase, MediaInitializationRecord},
    promotion::MEDIA_V1_CAPABILITIES,
};
use redb::ReadableDatabase;

/// All paths, selections and the record are observed under one lease registry
/// guard. This value is never accepted as input or used to authorize a writer.
pub struct InitializationRead<'a> {
    pub target_path: &'a Path,
    pub target_selection: &'a Selection,
    pub source_path: &'a Path,
    pub source_selection: &'a Selection,
    pub source_activation: &'a Activation,
    pub baseline_path: &'a Path,
    pub record: &'a MediaInitializationRecord,
}

struct ClosedGeneration {
    path: PathBuf,
    directory: DirectoryStamp,
    source: files::SourceStamp,
}
impl ClosedGeneration {
    fn capture(lease: &RootLease, expected: PathBuf) -> Result<Self> {
        let path = std::fs::canonicalize(&expected).map_err(storage)?;
        if path != expected || !path.starts_with(lease.state()) {
            return Err(invalid(
                "Initialization observation requires exact existing descendant paths",
            ));
        }
        Ok(Self {
            directory: DirectoryStamp::capture_resolved(&path)?,
            source: files::SourceStamp::capture_resolved(&path.join("store.redb"))?,
            path,
        })
    }
    fn revalidate(&self) -> Result<()> {
        self.source.revalidate()?;
        self.directory.revalidate()
    }
    fn facts(&self, root_id: &str, capabilities: &[String]) -> Result<GenerationFacts> {
        self.revalidate()?;
        let facts = versions::inspect_closed(&self.path)?;
        if facts.root_id != root_id
            || facts.store_version != 3
            || facts.required_capabilities.as_slice() != capabilities
        {
            return Err(invalid("Initialization observed root/schema/floor differs"));
        }
        Ok(facts)
    }
}

fn selected_record(
    target: &Path,
    selected: &Selection,
    id: &str,
) -> Result<MediaInitializationRecord> {
    let database = redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(target.join("store.redb"))
        .map_err(versions::database_error)?;
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    let row = table
        .get(format!("generation-initialization/{id}").as_str())
        .map_err(storage)?
        .ok_or_else(|| invalid("Selected target is not a genuine initialization"))?;
    let record: MediaInitializationRecord = decode(row.value())?;
    record.plan.validate_hash()?;
    if record.schema_version != 1
        || record.id != id
        || record.plan.id != id
        || selected.root_id.as_deref() != Some(record.plan.root_id.as_str())
        || selected.activation_id.as_deref() != Some(record.activation_id.as_str())
        || selected.generation != record.plan.target_generation
        || selected.store_version != Some(3)
        || record.phase != EnablementPhase::Succeeded
        || record.completed_at.is_none()
    {
        return Err(invalid(
            "Selected initialization record/target/completion differs",
        ));
    }
    for key in [
        "meta/pending-initialization",
        "meta/pending-promotion",
        "meta/pending-restoration",
    ] {
        if table.get(key).map_err(storage)?.is_some() {
            return Err(invalid(
                "Selected initialization must be completed before observation",
            ));
        }
    }
    let guard = media_generations::guard(&table)?;
    if guard.migration_id != record.id
        || guard.activation_id != record.activation_id
        || guard.validate()? != 0
    {
        return Err(invalid("Selected initialization target guard differs"));
    }
    crate::restoration_control::receipt(
        &table,
        &record.receipt_key,
        &record.request_digest,
        &record.id,
        "initialization_id",
    )?;
    Ok(record)
}

fn original_ancestry(
    baseline: &Path,
    source: &Path,
    selected: &Selection,
    record: &MediaInitializationRecord,
) -> Result<()> {
    let plan = &record.plan;
    let database = redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(baseline.join("store.redb"))
        .map_err(versions::database_error)?;
    let read = database.begin_read().map_err(storage)?;
    let table = read.open_table(RECORDS).map_err(storage)?;
    let prefix = CompatiblePrefix {
        root_id: plan.root_id.clone(),
        activation_id: plan.source_activation_id.clone(),
        operation_id: plan.source_operation_id.clone(),
        sequence: plan.source_activation_sequence,
        sha256: plan.source_prefix_sha256.clone(),
    };
    let (guard, origin) =
        crate::initialization::source_ancestry(&table, &plan.root_id, selected, &prefix)?;
    if origin != plan.origin || guard.validate()? != 0 {
        return Err(invalid(
            "Initialization baseline lacks the genuine reviewed zero ancestry",
        ));
    }
    let original_guard = table
        .get("meta/rollback-guard")
        .map_err(storage)?
        .ok_or_else(|| invalid("Initialization baseline guard is missing"))?;
    let source_database = redb::Database::builder()
        .set_cache_size(8 * 1024 * 1024)
        .open_read_only(source.join("store.redb"))
        .map_err(versions::database_error)?;
    let source_read = source_database.begin_read().map_err(storage)?;
    let source_table = source_read.open_table(RECORDS).map_err(storage)?;
    let source_guard = source_table
        .get("meta/rollback-guard")
        .map_err(storage)?
        .ok_or_else(|| invalid("Initialization original source guard is missing"))?;
    if source_guard.value() != original_guard.value() {
        return Err(invalid("Initialization original source zero guard changed"));
    }
    Ok(())
}

impl RootLease {
    /// Observe the actual completed selected initialization target F, original
    /// compatible source S and retained rollback baseline H. No paths/record or
    /// ownership assertions are accepted from the caller. The callback must not
    /// reenter attachment/registry operations while all three guards are held.
    pub fn with_selected_initialization_read<T>(
        &self,
        observe: impl FnOnce(InitializationRead<'_>) -> Result<T>,
    ) -> Result<T> {
        let selected = self.selection()?;
        let GenerationLocation::Workspace {
            operation_id,
            role: GenerationRole::Target,
        } = &selected.location
        else {
            return Err(invalid(
                "Selected generation is not an initialization workspace target",
            ));
        };
        let target = ClosedGeneration::capture(self, self.path(&selected)?)?;
        let mut guards = self.historical_guards.lock().map_err(storage)?;
        retain_read_guard(&mut guards, &target.path)?;
        target.revalidate()?;
        let record = selected_record(&target.path, &selected, operation_id)?;
        let plan = &record.plan;
        let history = activations(self.root())?;
        let prefix_count = plan.source_activation_sequence as usize;
        if history.len() != prefix_count + 1
            || media_generations::prefix_digest(&history[..prefix_count])
                != plan.source_prefix_sha256
        {
            return Err(invalid(
                "Initialization compatible prefix or target successor differs",
            ));
        }
        let source_activation = &history[prefix_count - 1].0;
        let target_activation = &history[prefix_count].0;
        if source_activation.id != plan.source_activation_id
            || source_activation.operation_id != plan.source_operation_id
            || source_activation.generation != plan.source_generation
            || source_activation.root_id != plan.root_id
            || source_activation.store_version != 3
            || source_activation.required_capabilities != plan.source_capabilities
            || history[..prefix_count].iter().any(|(activation, _)| {
                activation
                    .required_capabilities
                    .iter()
                    .any(|capability| !CURRENT_CAPABILITIES.contains(&capability.as_str()))
            })
            || target_activation.id != record.activation_id
            || target_activation.operation_id != record.id
            || target_activation.generation != selected.generation
            || target_activation.location != selected.location
            || target_activation.root_id != plan.root_id
            || target_activation.store_version != 3
            || target_activation.required_capabilities != plan.required_capabilities
        {
            return Err(invalid(
                "Initialization actual source/target activation ancestry differs",
            ));
        }
        let source_selection = Selection {
            generation: source_activation.generation,
            store_version: Some(source_activation.store_version),
            root_id: Some(source_activation.root_id.clone()),
            location: source_activation.location.clone(),
            activation_id: Some(source_activation.id.clone()),
        };
        let source = ClosedGeneration::capture(self, self.path(&source_selection)?)?;
        let baseline = ClosedGeneration::capture(
            self,
            location_path(
                self.root(),
                &GenerationLocation::Workspace {
                    operation_id: record.id.clone(),
                    role: GenerationRole::Rollback,
                },
            )?,
        )?;
        let target_object = target.source.object_identity()?;
        let source_object = source.source.object_identity()?;
        let baseline_object = baseline.source.object_identity()?;
        if target_object == source_object
            || target_object == baseline_object
            || source_object == baseline_object
        {
            return Err(invalid(
                "Initialization target/source/baseline stores must be independent native objects",
            ));
        }
        retain_read_guard(&mut guards, &source.path)?;
        retain_read_guard(&mut guards, &baseline.path)?;
        let media_capabilities: Vec<String> = MEDIA_V1_CAPABILITIES
            .iter()
            .map(|capability| (*capability).into())
            .collect();
        target.facts(&plan.root_id, &media_capabilities)?;
        // S is intentionally fenced; its historical compatible activation is
        // validated above, while only H retains that preacceptance floor.
        source.facts(&plan.root_id, &media_capabilities)?;
        let baseline_facts = baseline.facts(&plan.root_id, &plan.source_capabilities)?;
        if baseline_facts.record_count != plan.source_records
            || baseline_facts.records_sha256 != plan.source_records_sha256
        {
            return Err(invalid(
                "Initialization retained baseline differs from the actual reviewed source",
            ));
        }
        same_replay_identity(&target.path, &source.path, &plan.root_id)?;
        same_replay_identity(&target.path, &baseline.path, &plan.root_id)?;
        original_ancestry(&baseline.path, &source.path, &source_selection, &record)?;
        target.revalidate()?;
        source.revalidate()?;
        baseline.revalidate()?;
        let result = observe(InitializationRead {
            target_path: &target.path,
            target_selection: &selected,
            source_path: &source.path,
            source_selection: &source_selection,
            source_activation,
            baseline_path: &baseline.path,
            record: &record,
        });
        target.revalidate()?;
        source.revalidate()?;
        baseline.revalidate()?;
        if self.selection()? != selected
            || activations(self.root())?
                .iter()
                .map(|(_, hash)| hash)
                .ne(history.iter().map(|(_, hash)| hash))
        {
            return Err(invalid(
                "Initialization selection/history changed during observation",
            ));
        }
        result
    }
}
