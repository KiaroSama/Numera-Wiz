//! Media batch composition. Child media plans and jobs remain the authoritative engine work.
use super::*;
use nw_domain::batches::{
    Batch, BatchPlan, BatchPlanReview, BatchResult, CleanBatchInput, CleanFolderBatchInput,
    ExtractBatchInput, ExtractFolderBatchInput, ExtractSelector, FailurePolicy,
    FolderBatchExclusion, FolderBatchReview, FolderOutputLayout, ItemInput, ItemResult,
    PlanReference, PlannedItem, PlannedItemView, ReviewedExtraction, UnresolvedIndex,
    UnresolvedPolicy,
};
use nw_domain::ingest::{FolderEntryOutcome, FolderSnapshot};

const FOLDER_BATCH_COMMAND: &str = "streams.clean.folder_batch_plan";

pub(super) struct MemberLease {
    index: u32,
    reference: PlanReference,
    source: files::SourceStamp,
    output_parent: files::DirectoryStamp,
}
pub(super) struct BatchPreflight {
    components: Vec<files::SourceStamp>,
    members: Vec<MemberLease>,
}
fn rejected(mut error: DomainError) -> DomainError {
    let mut end = error.message.len().min(2048);
    while !error.message.is_char_boundary(end) {
        end -= 1;
    }
    error.message.truncate(end);
    error
}

struct FolderTargets {
    source: files::DirectoryStamp,
    output: files::DirectoryStamp,
    items: Vec<ItemInput>,
    excluded: Vec<FolderBatchExclusion>,
}

struct ReviewedBatchItems {
    items: Vec<PlannedItem>,
    children: Vec<Plan>,
}

fn contains_directory(
    ancestor: &files::DirectoryStamp,
    descendant: &files::DirectoryStamp,
) -> Result<bool> {
    if descendant.path().starts_with(ancestor.path()) {
        return Ok(true);
    }
    let identity = ancestor.object_identity()?;
    if descendant.object_identity()? == identity {
        return Ok(true);
    }
    for parent in descendant.path().ancestors().skip(1) {
        if parent.is_absolute()
            && files::DirectoryStamp::capture_resolved(parent)?.object_identity()? == identity
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn folder_output_name(index: u32, source: &Path) -> Result<String> {
    let invalid = || {
        DomainError::new(
            "INVALID_RECORD",
            "Registered folder media needs a representable filename and container extension",
        )
    };
    let stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid)?;
    let extension = source
        .extension()
        .and_then(|name| name.to_str())
        .filter(|name| {
            (1..=4).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .ok_or_else(invalid)?;
    let safe_stem: String = stem
        .chars()
        .take(64)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect();
    Ok(format!(
        "media-{:02}-{safe_stem}.{}",
        index + 1,
        extension.to_ascii_lowercase()
    ))
}

fn validate_folder_destination(
    output: &files::DirectoryStamp,
    item: &ItemInput,
    plan: &Plan,
) -> Result<()> {
    let rights = nw_domain::plans::validate_requirements(plan)?;
    let parent_identity = rights.output_create.parent_object.as_ref().ok_or_else(|| {
        DomainError::new(
            "INVALID_PLAN",
            "Folder batch child has no reviewed output parent",
        )
    })?;
    let destination = Path::new(&item.output);
    if plan.output != item.output || !destination.starts_with(output.path()) {
        return Err(DomainError::new(
            "TARGET_CHANGED",
            "Folder batch child resolved outside its derived destination",
        ));
    }
    let parent = destination.parent().ok_or_else(|| {
        DomainError::new("INVALID_PLAN", "Folder batch child has no output directory")
    })?;
    if parent == output.path() {
        output.require_object(parent_identity)?;
    } else {
        files::DirectoryStamp::capture_resolved(parent)?.require_object(parent_identity)?;
        output.revalidate()?;
    }
    Ok(())
}

fn available_folder_name(
    target: PathBuf,
    original: &BTreeSet<PathBuf>,
    selected: &mut BTreeSet<PathBuf>,
) -> Result<PathBuf> {
    let stem = target.file_stem().and_then(|name| name.to_str());
    let extension = target.extension().and_then(|name| name.to_str());
    let (Some(stem), Some(extension)) = (stem, extension) else {
        return Err(DomainError::new(
            "INVALID_RECORD",
            "Folder output needs a representable filename and extension",
        ));
    };
    for number in 1..=128 {
        let candidate = if number == 1 {
            target.clone()
        } else {
            target.with_file_name(format!("{stem} ({number}).{extension}"))
        };
        if selected.contains(&candidate) || number > 1 && original.contains(&candidate) {
            continue;
        }
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                selected.insert(candidate.clone());
                return Ok(candidate);
            }
            Err(error) => return Err(files::io_error(error)),
        }
    }
    Err(DomainError::new(
        "OUTPUT_EXISTS",
        "No available name among the original and 127 numbered folder outputs",
    ))
}

impl Service {
    pub(super) fn batch_child_command(&self, context: &Context, command: &str) -> Result<()> {
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(command))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                format!("Batch composition requires current {command} command access"),
            ));
        }
        Ok(())
    }
    pub(super) fn prepare_batch(
        &self,
        context: &Context,
        input: CleanBatchInput,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<BatchPlan> {
        self.prepare_cleanup_batch(context, input, now, cancel, None)
    }

    pub(super) fn prepare_folder_batch(
        &self,
        context: &Context,
        input: CleanFolderBatchInput,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<FolderBatchReview> {
        input.validate()?;
        let revision = self
            .policy(context)?
            .as_ref()
            .map_or(0, |policy| policy.revision);
        self.batch_child_command(context, "assets.folder_inspect")?;
        self.batch_child_command(context, "streams.clean")?;
        let snapshot = self.read_folder(context, &input.snapshot_id)?;
        if snapshot.hash != input.snapshot_hash {
            return Err(DomainError::new(
                "FOLDER_SNAPSHOT_MISMATCH",
                "Folder snapshot hash differs from the retained intake",
            ));
        }
        if !snapshot.complete {
            return Err(DomainError::new(
                "FOLDER_INTAKE_INCOMPLETE",
                "Folder enumeration was incomplete; inspect and rescan before review",
            ));
        }
        let failed = snapshot
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.outcome,
                    FolderEntryOutcome::ProbeFailed | FolderEntryOutcome::Rejected
                )
            })
            .count();
        if failed != 0 && input.failure_policy == FailurePolicy::RejectAll {
            return Err(DomainError::new(
                "FOLDER_INTAKE_FAILED",
                format!(
                    "Folder intake retained {failed} failed entries; inspect and resolve them before review"
                ),
            ));
        }
        if !snapshot
            .entries
            .iter()
            .any(|entry| entry.outcome == FolderEntryOutcome::Registered)
        {
            return Err(DomainError::new(
                "FOLDER_EMPTY",
                "Folder intake retained no eligible registered video for cleanup",
            ));
        }
        let _preflight = self
            .batch_preflights
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                DomainError::new(
                    "RESOURCE_LIMIT",
                    "Folder intake and batch preflight capacity is full",
                )
            })?;
        let FolderTargets {
            source,
            output,
            items,
            excluded,
        } = self.folder_targets(
            context,
            &snapshot,
            &input.output_root,
            None,
            input.output_layout,
            cancel,
        )?;
        let plan = self.prepare_cleanup_batch(
            context,
            CleanBatchInput {
                items,
                intent: input.intent,
                file_times: input.file_times,
                execution_limits: input.execution_limits,
                failure_policy: input.failure_policy,
                unresolved_policy: input.unresolved_policy,
            },
            now,
            cancel,
            Some((&source, &output, revision)),
        )?;
        let plan = self.batch_review(context, &plan)?;
        self.confirm_batch_policy(context, revision, FOLDER_BATCH_COMMAND)?;
        Ok(FolderBatchReview {
            snapshot_id: snapshot.id,
            snapshot_hash: snapshot.hash,
            excluded,
            plan,
        })
    }

    fn folder_targets(
        &self,
        context: &Context,
        snapshot: &FolderSnapshot,
        output_root: &str,
        extension: Option<&str>,
        layout: FolderOutputLayout,
        cancel: &CancellationToken,
    ) -> Result<FolderTargets> {
        let output = Path::new(output_root);
        let output_denied = || {
            DomainError::new(
                "PATH_DENIED",
                "Folder output requires an authorized existing ordinary absolute output directory",
            )
            .field("output_root")
        };
        if !output.is_absolute() {
            return Err(output_denied());
        }
        self.check_media_namespace(output, true)?;
        let canonical = std::fs::canonicalize(output).map_err(|_| output_denied())?;
        self.root_authority(context, &canonical, true)
            .map_err(|error| {
                if error.code == "PATH_DENIED" {
                    output_denied()
                } else {
                    error
                }
            })?;
        self.check_media_namespace(&canonical, true)?;
        if !super::folders::ordinary_metadata(output).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(output_denied());
        }
        let output = files::DirectoryStamp::capture_resolved(&canonical)?;
        let source_root = Path::new(&snapshot.root);
        let source = files::DirectoryStamp::capture_resolved(source_root)?;
        source.require_object(&snapshot.root_object)?;
        if contains_directory(&source, &output)? || contains_directory(&output, &source)? {
            return Err(DomainError::new(
                "SOURCE_OUTPUT_ALIAS",
                "Folder output and intake must use disjoint directory trees",
            )
            .field("output_root"));
        }
        let mut items = Vec::new();
        let mut excluded = Vec::new();
        let original = if layout == FolderOutputLayout::PreserveTree {
            snapshot
                .entries
                .iter()
                .filter(|entry| entry.outcome == FolderEntryOutcome::Registered)
                .map(|entry| {
                    output
                        .path()
                        .join(entry.relative_path.split('/').collect::<PathBuf>())
                })
                .collect()
        } else {
            BTreeSet::new()
        };
        let mut selected = BTreeSet::new();
        for entry in &snapshot.entries {
            if cancel.is_cancelled() {
                return Err(DomainError::new(
                    "CANCELLED",
                    "Folder batch review cancelled",
                ));
            }
            if entry.outcome != FolderEntryOutcome::Registered {
                excluded.push(FolderBatchExclusion {
                    index: entry.index,
                    outcome: entry.outcome,
                });
                continue;
            }
            let id = entry.asset_id.as_deref().ok_or_else(|| {
                DomainError::new("INVALID_RECORD", "Registered folder entry has no asset")
            })?;
            let asset: Asset = self
                .store
                .get(&named("asset", id)?)?
                .ok_or_else(DomainError::object_not_found)?;
            self.object(context, id, &asset.creator)?;
            let relative = entry.relative_path.split('/').collect::<PathBuf>();
            let expected_path = source_root.join(&relative);
            if asset.id != id
                || asset.creator != snapshot.creator
                || Some(&asset.identity) != entry.identity.as_ref()
                || asset.source_object != entry.source_object
                || Path::new(&asset.identity.path) != expected_path
            {
                return Err(DomainError::new(
                    "INVALID_RECORD",
                    "Registered asset differs from its retained folder entry",
                ));
            }
            self.root_authority(context, &expected_path, false)?;
            let mut target = match layout {
                FolderOutputLayout::Indexed => output
                    .path()
                    .join(folder_output_name(entry.index, &expected_path)?),
                FolderOutputLayout::PreserveTree => {
                    let path = output.path().join(relative);
                    let parent = path.parent().filter(|parent| parent.starts_with(output.path()));
                    let Some(parent) = parent else {
                        return Err(output_denied());
                    };
                    let parent = files::DirectoryStamp::capture_resolved(parent).map_err(|_| {
                        DomainError::new(
                            "OUTPUT_DIRECTORY_UNAVAILABLE",
                            "Use an existing ordinary output subdirectory for each retained source folder",
                        )
                        .field("output_root")
                    })?;
                    if contains_directory(&source, &parent)? {
                        return Err(DomainError::new(
                            "SOURCE_OUTPUT_ALIAS",
                            "Folder output subdirectory is inside the original input tree",
                        ));
                    }
                    self.root_authority(context, parent.path(), true)?;
                    self.check_media_namespace(parent.path(), true)?;
                    available_folder_name(path, &original, &mut selected)?
                }
            };
            if let Some(extension) = extension {
                target.set_extension(extension);
            }
            items.push(ItemInput {
                asset_id: id.into(),
                output: files::path_text(&target)?,
            });
        }
        Ok(FolderTargets {
            source,
            output,
            items,
            excluded,
        })
    }

    pub(super) fn prepare_folder_encode(
        &self,
        context: &Context,
        input: nw_domain::avc_export::FolderEncodeInput,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<FolderBatchReview> {
        use nw_domain::{avc_export::Container, batches::validate_folder_reference};
        const COMMAND: &str = "media.encode_folder_plan";
        validate_folder_reference(&input.snapshot_id, &input.snapshot_hash, &input.output_root)?;
        self.batch_child_command(context, "assets.folder_inspect")?;
        self.batch_child_command(context, "media.encode")?;
        let revision = self
            .policy(context)?
            .as_ref()
            .map_or(0, |policy| policy.revision);
        let snapshot = self.read_folder(context, &input.snapshot_id)?;
        if snapshot.hash != input.snapshot_hash {
            return Err(DomainError::new(
                "FOLDER_SNAPSHOT_MISMATCH",
                "Folder snapshot hash differs from the retained intake",
            ));
        }
        if !snapshot.complete {
            return Err(DomainError::new(
                "FOLDER_INTAKE_INCOMPLETE",
                "Folder enumeration was incomplete; inspect and rescan before encode review",
            ));
        }
        let _permit = self
            .batch_preflights
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                DomainError::new("RESOURCE_LIMIT", "Folder encode review capacity is full")
            })?;
        let extension = match input.container {
            Container::Matroska => "mkv",
            Container::Mp4 => "mp4",
        };
        let FolderTargets {
            source,
            output,
            items,
            excluded,
        } = self.folder_targets(
            context,
            &snapshot,
            &input.output_root,
            Some(extension),
            FolderOutputLayout::Indexed,
            cancel,
        )?;
        if items.is_empty() {
            return Err(DomainError::new(
                "FOLDER_EMPTY",
                "Folder intake retained no eligible registered pictures",
            ));
        }
        if items.len() > nw_domain::batches::MAX_BATCH_ITEMS {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Folder encode review admits at most sixty-four registered sources",
            ));
        }
        if input.failure_policy == FailurePolicy::RejectAll
            && excluded.iter().any(|entry| {
                matches!(
                    entry.outcome,
                    FolderEntryOutcome::ProbeFailed | FolderEntryOutcome::Rejected
                )
            })
        {
            return Err(DomainError::new(
                "FOLDER_INTAKE_FAILED",
                "Review failed intake entries before a reject-all folder encode",
            ));
        }
        let items = self.review_batch_items(
            context,
            items.iter().map(|item| (item, None)),
            cancel,
            |item, _, _| {
                let plan = self.prepare_native_copy(
                    context,
                    &item.asset_id,
                    &item.output,
                    Default::default(),
                    |engine, source, path| {
                        let settings = input.settings_for(&source.probe.streams)?;
                        engine.avc_plan_at(source, path, settings, &context.principal, now)
                    },
                )?;
                validate_folder_destination(&output, item, &plan)?;
                Ok(plan)
            },
        )?;
        source.revalidate()?;
        output.revalidate()?;
        self.root_authority(context, source.path(), false)?;
        self.root_authority(context, output.path(), true)?;
        self.confirm_batch_policy(context, revision, COMMAND)?;
        let plan = self.retain_batch_plan(
            context,
            now,
            input.failure_policy,
            UnresolvedPolicy::Reject,
            Vec::new(),
            items,
        )?;
        Ok(FolderBatchReview {
            snapshot_id: snapshot.id,
            snapshot_hash: snapshot.hash,
            excluded,
            plan: self.batch_review(context, &plan)?,
        })
    }

    fn prepare_cleanup_batch(
        &self,
        context: &Context,
        input: CleanBatchInput,
        now: u64,
        cancel: &CancellationToken,
        folder: Option<(&files::DirectoryStamp, &files::DirectoryStamp, u64)>,
    ) -> Result<BatchPlan> {
        input.validate()?;
        let (command, revision) = match folder {
            Some((_, _, revision)) => (FOLDER_BATCH_COMMAND, revision),
            None => (
                "streams.clean.batch_plan",
                self.policy(context)?
                    .as_ref()
                    .map_or(0, |policy| policy.revision),
            ),
        };
        self.batch_child_command(context, "streams.clean")?;
        self.admit_execution_limits(context, &input.execution_limits)?;
        let mut unresolved = BTreeSet::new();
        for (kind, selector) in [
            (0, &input.intent.audio_selection.selector),
            (1, &input.intent.subtitle_selection.selector),
        ] {
            if let nw_domain::mux::Selector::SourceIndices { values } = selector {
                for index in values {
                    unresolved.insert((kind, *index));
                }
            }
        }
        // Authorize every source before retaining a preview; an invisible source never becomes a row.
        for item in &input.items {
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Batch preview cancelled"));
            }
            let asset: Asset = self
                .store
                .get(&named("asset", &item.asset_id)?)?
                .ok_or_else(DomainError::object_not_found)?;
            self.object(context, &asset.id, &asset.creator)?;
            for stream in &asset.probe.streams {
                let kind = match stream.kind {
                    nw_domain::mux::StreamKind::Audio => Some(0),
                    nw_domain::mux::StreamKind::Subtitle => Some(1),
                    _ => None,
                };
                if let Some(kind) = kind {
                    unresolved.remove(&(kind, stream.index));
                }
            }
        }
        let items = self.review_batch_items(
            context,
            input.items.iter().map(|item| (item, None)),
            cancel,
            |item, _, _| {
                let plan = self.prepare_cleanup(context, input.clean_input(item), now)?;
                if let Some((_, output, _)) = folder {
                    validate_folder_destination(output, item, &plan)?;
                }
                Ok(plan)
            },
        )?;
        if let Some((source, output, _)) = folder {
            source.revalidate()?;
            output.revalidate()?;
            self.root_authority(context, source.path(), false)?;
            self.root_authority(context, output.path(), true)?;
        }
        self.confirm_batch_policy(context, revision, command)?;
        self.retain_batch_plan(
            context,
            now,
            input.failure_policy,
            input.unresolved_policy,
            unresolved
                .into_iter()
                .map(|(kind, source_index)| UnresolvedIndex {
                    kind: if kind == 0 {
                        nw_domain::mux::StreamKind::Audio
                    } else {
                        nw_domain::mux::StreamKind::Subtitle
                    },
                    source_index,
                })
                .collect(),
            items,
        )
    }

    pub(super) fn prepare_extract_batch(
        &self,
        context: &Context,
        input: ExtractBatchInput,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<BatchPlan> {
        input.validate()?;
        let revision = self
            .policy(context)?
            .as_ref()
            .map_or(0, |policy| policy.revision);
        self.batch_child_command(context, "streams.extract")?;
        let items = self.review_batch_items(
            context,
            input
                .items
                .iter()
                .map(|item| (&item.target, Some(item.selector))),
            cancel,
            |item, selector, _| self.prepare_extraction(context, item, selector, now),
        )?;
        self.confirm_batch_policy(context, revision, "streams.extract.batch_plan")?;
        self.retain_batch_plan(
            context,
            now,
            input.failure_policy,
            UnresolvedPolicy::PerFileNoMatch,
            Vec::new(),
            items,
        )
    }

    fn prepare_extraction(
        &self,
        context: &Context,
        item: &ItemInput,
        selector: Option<ExtractSelector>,
        now: u64,
    ) -> Result<Plan> {
        let selector = selector.ok_or_else(|| {
            DomainError::new("INVALID_PLAN", "Batch extraction has no track selector")
        })?;
        self.prepare_native_copy(
            context,
            &item.asset_id,
            &item.output,
            Default::default(),
            |engine, asset, output| {
                let index = selector.resolve(&asset.probe.streams)?;
                engine.extract_plan_at(asset, output, index, &context.principal, now)
            },
        )
    }

    pub(super) fn prepare_folder_extraction(
        &self,
        context: &Context,
        input: ExtractFolderBatchInput,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<FolderBatchReview> {
        const COMMAND: &str = "streams.extract.folder_batch_plan";
        input.validate()?;
        let revision = self
            .policy(context)?
            .as_ref()
            .map_or(0, |policy| policy.revision);
        self.batch_child_command(context, "assets.folder_inspect")?;
        self.batch_child_command(context, "streams.extract")?;
        let snapshot = self.read_folder(context, &input.snapshot_id)?;
        if snapshot.hash != input.snapshot_hash {
            return Err(DomainError::new(
                "FOLDER_SNAPSHOT_MISMATCH",
                "Folder snapshot hash differs from the retained intake",
            ));
        }
        if !snapshot.complete {
            return Err(DomainError::new(
                "FOLDER_INTAKE_INCOMPLETE",
                "Folder enumeration was incomplete; inspect and rescan before review",
            ));
        }
        let _preflight = self
            .batch_preflights
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                DomainError::new(
                    "RESOURCE_LIMIT",
                    "Folder extraction review capacity is full",
                )
            })?;
        let FolderTargets {
            source,
            output,
            items,
            excluded,
        } = self.folder_targets(
            context,
            &snapshot,
            &input.output_root,
            Some("mkv"),
            FolderOutputLayout::Indexed,
            cancel,
        )?;
        if items.is_empty() || items.len() > nw_domain::batches::MAX_BATCH_ITEMS {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Folder extraction requires one to sixty-four registered sources",
            ));
        }
        if input.failure_policy == FailurePolicy::RejectAll
            && excluded.iter().any(|entry| {
                matches!(
                    entry.outcome,
                    FolderEntryOutcome::ProbeFailed | FolderEntryOutcome::Rejected
                )
            })
        {
            return Err(DomainError::new(
                "FOLDER_INTAKE_FAILED",
                "Review failed intake entries before a reject-all folder extraction",
            ));
        }
        let items = self.review_batch_items(
            context,
            items.iter().map(|item| (item, Some(input.selector))),
            cancel,
            |item, selector, _| {
                let plan = self.prepare_extraction(context, item, selector, now)?;
                validate_folder_destination(&output, item, &plan)?;
                Ok(plan)
            },
        )?;
        source.revalidate()?;
        output.revalidate()?;
        self.root_authority(context, source.path(), false)?;
        self.root_authority(context, output.path(), true)?;
        self.confirm_batch_policy(context, revision, COMMAND)?;
        let plan = self.retain_batch_plan(
            context,
            now,
            input.failure_policy,
            UnresolvedPolicy::PerFileNoMatch,
            Vec::new(),
            items,
        )?;
        Ok(FolderBatchReview {
            snapshot_id: snapshot.id,
            snapshot_hash: snapshot.hash,
            excluded,
            plan: self.batch_review(context, &plan)?,
        })
    }

    pub(super) fn prepare_packet_split(
        &self,
        context: &Context,
        input: nw_domain::packet_cut::SplitInput,
        cancel: &CancellationToken,
        runtime: &tokio::runtime::Handle,
    ) -> Result<BatchPlanReview> {
        input.validate()?;
        self.batch_child_command(context, "media.cut")?;
        let initial_revision = self
            .policy(context)?
            .as_ref()
            .map_or(0, |policy| policy.revision);
        let source: Asset = self
            .store
            .get(&named("asset", &input.asset_id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object(context, &source.id, &source.creator)?;
        self.path(context, Path::new(&source.identity.path), false)?;
        let _permit = self
            .batch_preflights
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                DomainError::new("RESOURCE_LIMIT", "Packet split review capacity is full")
            })?;
        let ranges: Vec<_> = input.outputs.iter().map(|item| item.range).collect();
        let (stamp, results) = runtime.block_on(self.engine()?.inspect_packet_cuts(
            &source,
            &ranges,
            cancel.clone(),
        ))?;
        let _delivery = self.mutation.lock().map_err(|_| lock_error())?;
        self.confirm_batch_policy(context, initial_revision, "media.split_plan")?;
        self.batch_child_command(context, "media.cut")?;
        stamp.revalidate()?;
        let now = self.clock.now()?;
        let targets: Vec<_> = input
            .outputs
            .into_iter()
            .map(|item| ItemInput {
                asset_id: source.id.clone(),
                output: item.output,
            })
            .collect();
        let mut reviewed = self.review_batch_items(
            context,
            targets.iter().map(|item| (item, None)),
            cancel,
            |item, _, index| {
                results[index].clone()?;
                self.prepare_native_copy(
                    context,
                    &item.asset_id,
                    &item.output,
                    Default::default(),
                    |engine, current, output| {
                        if current.identity != source.identity
                            || current.source_object != source.source_object
                        {
                            return Err(DomainError::new(
                                "SOURCE_CHANGED",
                                "Registered split source changed during inspection",
                            ));
                        }
                        engine.packet_cut_plan_at(
                            current,
                            output,
                            ranges[index],
                            &context.principal,
                            now,
                        )
                    },
                )
            },
        )?;
        for (item, range) in reviewed.items.iter_mut().zip(ranges) {
            item.packet_cut = Some(range);
        }
        stamp.revalidate()?;
        let batch = self.retain_batch_plan(
            context,
            now,
            FailurePolicy::RejectAll,
            UnresolvedPolicy::Reject,
            Vec::new(),
            reviewed,
        )?;
        self.batch_review(context, &batch)
    }

    fn review_batch_items<'a>(
        &self,
        context: &Context,
        targets: impl ExactSizeIterator<Item = (&'a ItemInput, Option<ExtractSelector>)>,
        cancel: &CancellationToken,
        mut compile: impl FnMut(&ItemInput, Option<ExtractSelector>, usize) -> Result<Plan>,
    ) -> Result<ReviewedBatchItems> {
        let mut items = Vec::with_capacity(targets.len());
        let mut children = Vec::with_capacity(targets.len());
        let mut resolved_outputs = BTreeSet::new();
        for (index, (item, selector)) in targets.enumerate() {
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Batch preview cancelled"));
            }
            let asset: Asset = self
                .store
                .get(&named("asset", &item.asset_id)?)?
                .ok_or_else(DomainError::object_not_found)?;
            self.object(context, &asset.id, &asset.creator)?;
            let mut candidate = PlannedItem {
                index: index as u32,
                asset_id: asset.id.clone(),
                source_sha256: asset.identity.sha256.clone(),
                source_bytes: asset.identity.bytes.clone(),
                output_name: item
                    .output
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                requested_output: item.output.clone(),
                plan: None,
                action: None,
                error: None,
                packet_cut: None,
                extraction: selector.map(|selector| ReviewedExtraction {
                    selector,
                    source_index: None,
                }),
            };
            match compile(item, selector, index) {
                Ok(plan) => {
                    if !resolved_outputs.insert(plan.output.clone()) {
                        candidate.error = Some(DomainError::new(
                            "OUTPUT_COLLISION",
                            "Two reviewed candidates select the same canonical output",
                        ));
                    } else {
                        candidate.output_name = plan
                            .output
                            .rsplit(['/', '\\'])
                            .next()
                            .unwrap_or("")
                            .to_owned();
                        candidate.requested_output = plan.output.clone();
                        candidate.action = Some(plan.resolution.action.clone());
                        if let Some(extraction) = &mut candidate.extraction {
                            extraction.source_index = plan
                                .resolution
                                .outcomes
                                .iter()
                                .find(|outcome| outcome.output_index == Some(0))
                                .map(|outcome| outcome.source_index);
                        }
                        candidate.plan = Some(PlanReference {
                            id: plan.id.clone(),
                            hash: plan.hash.clone(),
                        });
                        children.push(plan);
                    }
                }
                Err(error) => candidate.error = Some(rejected(error)),
            }
            items.push(candidate);
        }
        Ok(ReviewedBatchItems { items, children })
    }

    fn confirm_batch_policy(&self, context: &Context, revision: u64, command: &str) -> Result<()> {
        let current = self.policy(context)?;
        if current.as_ref().map_or(0, |policy| policy.revision) != revision
            || current
                .as_ref()
                .is_some_and(|policy| !policy.allows_command(command))
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Access policy changed while preparing the media batch",
            ));
        }
        Ok(())
    }

    fn retain_batch_plan(
        &self,
        context: &Context,
        now: u64,
        failure_policy: FailurePolicy,
        unresolved_policy: UnresolvedPolicy,
        unresolved: Vec<UnresolvedIndex>,
        reviewed: ReviewedBatchItems,
    ) -> Result<BatchPlan> {
        let mut plan = BatchPlan {
            schema_version: 1,
            id: Uuid::new_v4().to_string(),
            creator: context.principal.clone(),
            data_root: self.store.root_id().into(),
            hash: String::new(),
            created_at: now.to_string(),
            expires_at: now
                .checked_add(1800)
                .ok_or_else(|| DomainError::new("RESOURCE_LIMIT", "Batch plan clock exhausted"))?
                .to_string(),
            failure_policy,
            unresolved_policy,
            unresolved,
            items: reviewed.items,
        };
        plan.hash = plan.digest()?;
        self.store
            .retain_media_batch_review(&plan, &reviewed.children)?;
        Ok(plan)
    }
    pub(super) fn read_batch_plan(
        &self,
        context: &Context,
        id: &str,
        action: ObjectAction,
    ) -> Result<BatchPlan> {
        let plan: BatchPlan = self
            .store
            .get(&named("batch-plan", id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object_action(context, ObjectKind::Plan, id, &plan.creator, action)?;
        if plan.id != id || plan.data_root != self.store.root_id() {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Cleanup batch belongs to another identity/root",
            ));
        }
        plan.validate_hash()?;
        Ok(plan)
    }
    pub(super) fn batch_review(
        &self,
        context: &Context,
        plan: &BatchPlan,
    ) -> Result<BatchPlanReview> {
        let policy = self.policy(context)?;
        let revision = policy.as_ref().map_or(0, |value| value.revision);
        let owner = policy.is_none();
        let mut items = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let output_path = if let Some(reference) = &item.plan {
                match self.read_plan(context, &reference.id, ObjectAction::Read) {
                    Ok(child) if child.hash == reference.hash => {
                        self.plan_review(context, &child)?.output_path
                    }
                    Ok(_) => {
                        return Err(DomainError::new(
                            "INVALID_PLAN",
                            "A child plan differs from its reviewed batch",
                        ));
                    }
                    Err(error) if error.code == "OBJECT_NOT_FOUND" => None,
                    Err(error) => return Err(error),
                }
            } else {
                owner.then(|| item.requested_output.clone())
            };
            items.push(PlannedItemView {
                index: item.index,
                asset_id: item.asset_id.clone(),
                source_sha256: item.source_sha256.clone(),
                source_bytes: item.source_bytes.clone(),
                output_name: item.output_name.clone(),
                output_path,
                plan: item.plan.clone(),
                action: item.action.clone(),
                extraction: item.extraction,
                packet_cut: item.packet_cut,
                error: item
                    .error
                    .as_ref()
                    .map(|error| nw_domain::views::ErrorView {
                        code: error.code.clone(),
                        message: owner.then(|| error.message.clone()),
                        retryable: error.retryable,
                    }),
            });
        }
        if self
            .policy(context)?
            .as_ref()
            .map_or(0, |value| value.revision)
            != revision
        {
            return Err(DomainError::new(
                "POLICY_CHANGED",
                "Access changed while reading the batch proposal; retry under current policy",
            ));
        }
        Ok(BatchPlanReview {
            schema_version: 1,
            id: plan.id.clone(),
            hash: plan.hash.clone(),
            created_at: plan.created_at.clone(),
            expires_at: plan.expires_at.clone(),
            failure_policy: plan.failure_policy,
            unresolved_policy: plan.unresolved_policy,
            unresolved: plan.unresolved.clone(),
            items,
        })
    }
    pub(super) fn read_batch(
        &self,
        context: &Context,
        id: &str,
        action: ObjectAction,
    ) -> Result<(Batch, BatchPlan)> {
        let batch: Batch = self
            .store
            .get(&named("batch", id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object_action(context, ObjectKind::Job, id, &batch.creator, action)?;
        if batch.id != id {
            return Err(DomainError::new(
                "INVALID_BATCH",
                "Stored batch identity differs",
            ));
        }
        let plan: BatchPlan = self
            .store
            .get(&named("batch-plan", &batch.plan_id)?)?
            .ok_or_else(|| {
                DomainError::new("INVALID_BATCH", "Accepted batch has no retained plan")
            })?;
        batch.validate(&plan)?;
        if plan.data_root != self.store.root_id() {
            return Err(DomainError::new("INVALID_BATCH", "Batch root differs"));
        }
        Ok((batch, plan))
    }
    pub(super) fn inspect_batch(&self, context: &Context, id: &str) -> Result<BatchResult> {
        let (batch, plan) = self.read_batch(context, id, ObjectAction::Read)?;
        let policy = self.policy(context)?;
        let revision = policy.as_ref().map_or(0, |value| value.revision);
        let owner = policy.is_none();
        let mut items = Vec::with_capacity(plan.items.len());
        for (member, item) in batch.members.iter().zip(&plan.items) {
            let mut result = ItemResult {
                index: item.index,
                asset_id: item.asset_id.clone(),
                output_name: item.output_name.clone(),
                job_id: member.job_id.clone(),
                state: None,
                action: item.action.clone(),
                artifact_id: None,
                source_bytes: item.source_bytes.clone(),
                output_bytes: None,
                error: item
                    .error
                    .as_ref()
                    .map(|error| nw_domain::views::ErrorView {
                        code: error.code.clone(),
                        message: owner.then(|| error.message.clone()),
                        retryable: error.retryable,
                    }),
            };
            if let Some(id) = &member.job_id {
                let job = self.store.inspect_job(id)?;
                self.object_action(
                    context,
                    ObjectKind::Job,
                    id,
                    &job.creator,
                    ObjectAction::Read,
                )?;
                if job.creator != batch.creator
                    || item.plan.as_ref().is_none_or(|reference| {
                        job.plan.id != reference.id || job.plan.hash != reference.hash
                    })
                {
                    return Err(DomainError::new(
                        "INVALID_BATCH",
                        "Accepted batch member differs from its captured job",
                    ));
                }
                let view = self.job_view(context, &job)?;
                result.state = Some(view.state);
                result.output_bytes = view
                    .artifact
                    .as_ref()
                    .map(|artifact| artifact.bytes.clone());
                result.artifact_id = view.artifact.map(|_| format!("job-{id}"));
                result.error = view.error.map(|mut error| {
                    // Full job details remain available through its separately authorized inspection.
                    // Keep group summaries bounded even when a worker records large diagnostics.
                    error.message = error.message.filter(|message| message.len() <= 2048);
                    error
                });
            }
            items.push(result);
        }
        let mut result = BatchResult {
            schema_version: 1,
            id: batch.id,
            plan_id: batch.plan_id,
            plan_hash: batch.plan_hash,
            total: items.len(),
            finished: 0,
            succeeded: 0,
            skipped: 0,
            failed: 0,
            cancelled: 0,
            interrupted: 0,
            complete: false,
            outcome: String::new(),
            successful_video_input_bytes: String::new(),
            successful_video_output_bytes: String::new(),
            successful_video_size_delta_bytes: String::new(),
            items,
        };
        result.finish_counts()?;
        if self
            .policy(context)?
            .as_ref()
            .map_or(0, |value| value.revision)
            != revision
        {
            return Err(DomainError::new(
                "POLICY_CHANGED",
                "Access changed while inspecting batch members; retry under current policy",
            ));
        }
        if serde_json::to_vec(&result)
            .map_err(|_| DomainError::new("INVALID_BATCH", "Cannot encode batch result"))?
            .len()
            > 224 * 1024
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Batch summary exceeds its bounded response budget; inspect individual member jobs",
            ));
        }
        Ok(result)
    }
    pub(super) fn prepare_batch_submission(
        &self,
        context: &Context,
        plan: &BatchPlan,
        now: u64,
        cancel: &CancellationToken,
    ) -> Result<BatchPreflight> {
        self.batch_child_command(context, "jobs.submit")?;
        plan.admits_submission()?;
        if plan.expires_at.parse::<u64>().unwrap_or(0) <= now {
            return Err(DomainError::new(
                "PLAN_EXPIRED",
                "Review a fresh cleanup batch",
            ));
        }
        let mut components = Vec::new();
        let mut members = Vec::new();
        if plan.items.iter().any(|item| item.plan.is_some()) {
            let engine = self.engine()?;
            for component in [&engine.ffmpeg, &engine.ffprobe] {
                let stamp = files::SourceStamp::capture_resolved(Path::new(&component.path))?;
                files::revalidate(component, cancel)?;
                stamp.revalidate()?;
                components.push(stamp);
            }
        }
        for item in &plan.items {
            let Some(reference) = &item.plan else {
                continue;
            };
            if cancel.is_cancelled() {
                return Err(DomainError::new("CANCELLED", "Batch preflight cancelled"));
            }
            let child = self.read_plan(context, &reference.id, ObjectAction::Submit)?;
            if child.hash != reference.hash
                || child.packet_cut != item.packet_cut
                || child.expires_at.parse::<u64>().unwrap_or(0) <= now
            {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "A batch child changed or expired",
                ));
            }
            self.engine()?.validate_plan(&child)?;
            self.admit_execution_limits(
                context,
                &child.execution_limits.clone().unwrap_or_default(),
            )?;
            self.plan_source_path(context, &child)?;
            self.path(context, Path::new(&child.output), true)?;
            let output_parent = nw_engine::output_parent_stamp(&child)?;
            let source =
                files::SourceStamp::capture_resolved(Path::new(&child.source.identity.path))?;
            source.require_object(child.source.source_object.as_ref().ok_or_else(|| {
                DomainError::new(
                    "INVALID_PLAN",
                    "Batch source has no frozen physical identity",
                )
            })?)?;
            files::revalidate(&child.source.identity, cancel)?;
            source.revalidate()?;
            output_parent.revalidate()?;
            members.push(MemberLease {
                index: item.index,
                reference: reference.clone(),
                source,
                output_parent,
            });
        }
        for component in &components {
            component.revalidate()?;
        }
        Ok(BatchPreflight {
            components,
            members,
        })
    }
    pub(super) fn revalidate_batch_submission(
        &self,
        context: &Context,
        plan: &BatchPlan,
        prepared: &BatchPreflight,
        now: u64,
    ) -> Result<()> {
        self.batch_child_command(context, "jobs.submit")?;
        plan.admits_submission()?;
        if plan.expires_at.parse::<u64>().unwrap_or(0) <= now
            || prepared.members.len()
                != plan.items.iter().filter(|item| item.plan.is_some()).count()
        {
            return Err(DomainError::new(
                "INVALID_PLAN",
                "Batch preflight expired or lost a candidate",
            ));
        }
        for component in &prepared.components {
            component.revalidate()?;
        }
        for member in &prepared.members {
            let item = &plan.items[member.index as usize];
            if item.plan.as_ref() != Some(&member.reference) {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Batch candidate changed during preflight",
                ));
            }
            let child = self.read_plan(context, &member.reference.id, ObjectAction::Submit)?;
            if child.hash != member.reference.hash
                || child.expires_at.parse::<u64>().unwrap_or(0) <= now
            {
                return Err(DomainError::new(
                    "INVALID_PLAN",
                    "Current child differs from batch preflight",
                ));
            }
            self.engine()?.validate_plan(&child)?;
            self.admit_execution_limits(
                context,
                &child.execution_limits.clone().unwrap_or_default(),
            )?;
            self.plan_source_path(context, &child)?;
            self.path(context, Path::new(&child.output), true)?;
            member
                .source
                .require_object(child.source.source_object.as_ref().ok_or_else(|| {
                    DomainError::new("INVALID_PLAN", "Batch source identity is missing")
                })?)?;
            member.output_parent.require_object(
                nw_domain::plans::validate_requirements(&child)?
                    .output_create
                    .parent_object
                    .as_ref()
                    .ok_or_else(|| {
                        DomainError::new("INVALID_PLAN", "Batch output parent identity is missing")
                    })?,
            )?;
        }
        Ok(())
    }
    pub(super) fn batch_receipt_readable(
        &self,
        context: &Context,
        response: &Response,
    ) -> Result<bool> {
        let Some(id) = response
            .result
            .as_ref()
            .and_then(|result| result["batch_id"].as_str())
        else {
            return Err(DomainError::new(
                "STORAGE_ERROR",
                "Retained batch receipt has no batch identity",
            ));
        };
        let (batch, _) = match self.read_batch(context, id, ObjectAction::Read) {
            Ok(value) => value,
            Err(error) if error.code == "OBJECT_NOT_FOUND" => return Ok(false),
            Err(error) => return Err(error),
        };
        for member in &batch.members {
            if let Some(id) = &member.job_id {
                let job = self.store.inspect_job(id)?;
                match self.object_action(
                    context,
                    ObjectKind::Job,
                    id,
                    &job.creator,
                    ObjectAction::Read,
                ) {
                    Ok(()) => {}
                    Err(error) if error.code == "OBJECT_NOT_FOUND" => return Ok(false),
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(true)
    }
}
