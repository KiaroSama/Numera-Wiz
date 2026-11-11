//! Finite native intake snapshots. Media remains registered by the common engine.
use super::*;
use nw_domain::ingest::{
    CompanionIntakePolicy, FolderEntry, FolderEntryKind, FolderEntryOutcome, FolderIntakeLimits,
    FolderReview, FolderSnapshot, MAX_FOLDER_DEPTH, MAX_FOLDER_DIRECTORIES,
    MAX_FOLDER_DIRECTORY_NAME_BYTES, MAX_FOLDER_ENTRIES, MAX_FOLDER_SECONDS,
    MAX_FOLDER_SOURCE_BYTES, MAX_FOLDER_VISITS, RegisterFolderInput,
};

const SNAPSHOT_LIMIT: usize = 128;

struct FolderScan<'a> {
    service: &'a Service,
    context: &'a Context,
    cancel: &'a CancellationToken,
    root: PathBuf,
    snapshot: FolderSnapshot,
    assets: Vec<Asset>,
    sources: Vec<(String, files::SourceStamp)>,
    directories: Vec<Arc<files::DirectoryStamp>>,
    source_bytes: u64,
    started: std::time::Instant,
}
fn bounded_error(mut error: DomainError) -> DomainError {
    let mut end = error.message.len().min(512);
    while !error.message.is_char_boundary(end) {
        end -= 1;
    }
    error.message.truncate(end);
    error
}
fn ordinary_metadata(path: &Path) -> Result<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path).map_err(files::io_error)?;
    if metadata.file_type().is_symlink() {
        return Err(DomainError::new(
            "PATH_DENIED",
            "Folder intake does not follow symbolic links",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Win32 FILE_ATTRIBUTE_REPARSE_POINT, read through the standard-library metadata API.
        const REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & REPARSE_POINT != 0 {
            return Err(DomainError::new(
                "PATH_DENIED",
                "Folder intake does not follow reparse points",
            ));
        }
    }
    Ok(metadata)
}
fn relative_text(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| DomainError::new("PATH_DENIED", "Folder entry escaped its admitted root"))?;
    let parts = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(String::from)
                .ok_or_else(|| {
                    DomainError::new(
                        "NATIVE_PATH_UNAVAILABLE",
                        "Folder snapshot requires representable native path components",
                    )
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let text = parts.join("/");
    if text.is_empty() || text.len() > 4096 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Folder relative path exceeds its representable bound",
        ));
    }
    Ok(text)
}
fn media_hint(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["mkv", "mp4", "m4v", "webm", "mov", "avi"]
                .iter()
                .any(|hint| extension.eq_ignore_ascii_case(hint))
        })
}
impl Service {
    fn folder_intake_allowed(&self, context: &Context) -> Result<()> {
        if self.policy(context)?.as_ref().is_some_and(|profile| {
            !profile.allows_command("assets.register_folder")
                || !profile.allows_command("assets.register")
                || !profile.owned_object_access
        }) {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Folder intake requires registration command and creator-scoped object authority",
            ));
        }
        Ok(())
    }
    fn folder_root(&self, context: &Context, input: &Path) -> Result<PathBuf> {
        if !input.is_absolute() || staging_path(input) || !ordinary_metadata(input)?.is_dir() {
            return Err(DomainError::new(
                "PATH_DENIED",
                "Folder intake requires an explicit ordinary native directory",
            ));
        }
        let canonical = std::fs::canonicalize(input).map_err(files::io_error)?;
        if staging_path(&canonical) || canonical.starts_with(&self.root) {
            return Err(DomainError::new(
                "PATH_DENIED",
                "Owner-managed namespaces cannot be folder intake roots",
            ));
        }
        self.root_authority(context, &canonical, false)?;
        files::DirectoryStamp::capture_resolved(&canonical)?.revalidate()?;
        Ok(canonical)
    }
    fn read_folder(&self, context: &Context, id: &str) -> Result<FolderSnapshot> {
        let raw: Value = self
            .store
            .get(&named("folder", id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        let creator = raw
            .get("creator")
            .and_then(Value::as_str)
            .ok_or_else(DomainError::object_not_found)?;
        self.object(context, id, creator)?;
        let snapshot: FolderSnapshot = commands::parse(raw)?;
        self.root_authority(context, Path::new(&snapshot.root), false)?;
        if snapshot.id != id || snapshot.data_root != self.store.root_id() {
            return Err(DomainError::new(
                "INVALID_RECORD",
                "Folder snapshot belongs to another identity or data root",
            ));
        }
        snapshot.validate_hash()?;
        Ok(snapshot)
    }
    fn folder_view(&self, context: &Context, snapshot: &FolderSnapshot) -> Result<Value> {
        let policy = self.policy(context)?;
        self.object(context, &snapshot.id, &snapshot.creator)?;
        self.root_authority(context, Path::new(&snapshot.root), false)?;
        let view = value(&FolderReview::new(snapshot, policy.is_none()))?;
        if self
            .policy(context)?
            .as_ref()
            .map(|profile| profile.revision)
            != policy.as_ref().map(|profile| profile.revision)
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Access policy changed while reading the folder snapshot",
            ));
        }
        Ok(view)
    }
    pub(super) fn folder_receipt_readable(
        &self,
        context: &Context,
        response: &Response,
    ) -> Result<bool> {
        let id = response
            .result
            .as_ref()
            .and_then(|result| result["id"].as_str())
            .ok_or_else(|| {
                DomainError::new(
                    "INVALID_RECORD",
                    "Retained folder receipt has no snapshot identity",
                )
            })?;
        match self.read_folder(context, id) {
            Ok(_) => Ok(true),
            Err(error)
                if ["OBJECT_NOT_FOUND", "PATH_DENIED", "PERMISSION_DENIED"]
                    .contains(&error.code.as_str()) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn folder_command(
        self: &Arc<Self>,
        context: &Context,
        request: &Request,
        cancellation: &CancellationToken,
    ) -> Result<Response> {
        if request.command.id == "assets.folder_inspect" {
            let input: IdInput = commands::parse(request.params.clone())?;
            let snapshot = self.read_folder(context, &input.id)?;
            return Ok(Response::success(
                &request.request_id,
                self.folder_view(context, &snapshot)?,
            ));
        }
        let input: RegisterFolderInput = commands::parse(request.params.clone())?;
        input.validate()?;
        self.folder_intake_allowed(context)?;
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
        let deadline = input
            .probe_timeout
            .unwrap_or(nw_domain::execution::CleanupLimits::default().probe);
        self.admit_probe_timeout(context, deadline)?;
        let root = self.folder_root(context, Path::new(&input.path))?;
        let root_stamp = Arc::new(files::DirectoryStamp::capture_resolved(&root)?);
        let root_object = root_stamp.object_identity()?;
        let now = self.clock.now()?;
        let mut scan = FolderScan {
            service: self,
            context,
            cancel: cancellation,
            root: root.clone(),
            snapshot: FolderSnapshot {
                schema_version: 1,
                id: Uuid::new_v4().to_string(),
                creator: context.principal.clone(),
                data_root: self.store.root_id().into(),
                hash: String::new(),
                created_at: now.to_string(),
                root: files::path_text(&root)?,
                root_object,
                input,
                limits: FolderIntakeLimits::default(),
                complete: true,
                visited_entries: 0,
                visited_directories: 0,
                scan_error: None,
                entries: Vec::new(),
            },
            assets: Vec::new(),
            sources: Vec::new(),
            directories: vec![root_stamp.clone()],
            source_bytes: 0,
            started: std::time::Instant::now(),
        };
        #[cfg(test)]
        self.preflight_started();
        // Enumeration and probes deliberately hold no mutation lock. Revocation can proceed.
        let prepared = scan.visit(root_stamp, 0, deadline);
        let guard = self.mutation.lock().map_err(|_| lock_error())?;
        if let Some(replayed) = self.refresh_acceptance(context, request, cancellation, &guard)? {
            return Ok(replayed);
        }
        prepared?;
        self.folder_intake_allowed(context)?;
        self.admit_probe_timeout(context, deadline)?;
        if self.folder_root(context, &root)? != root {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Folder root changed during intake",
            ));
        }
        for directory in &scan.directories {
            directory.revalidate()?;
        }
        for (relative, stamp) in &scan.sources {
            let entry = scan
                .snapshot
                .entries
                .iter_mut()
                .find(|entry| &entry.relative_path == relative)
                .ok_or_else(|| {
                    DomainError::new(
                        "INVALID_RECORD",
                        "Intake source lease lost its manifest row",
                    )
                })?;
            let path = root.join(relative.split('/').collect::<PathBuf>());
            self.root_authority(context, &path, false)?;
            if let Err(error) = stamp.revalidate() {
                if let Some(id) = entry.asset_id.take() {
                    scan.assets.retain(|asset| asset.id != id);
                }
                entry.outcome = FolderEntryOutcome::Rejected;
                entry.error = Some(bounded_error(error));
            }
        }
        scan.snapshot
            .entries
            .sort_by(|first, second| first.relative_path.cmp(&second.relative_path));
        for (index, entry) in scan.snapshot.entries.iter_mut().enumerate() {
            entry.index = index as u32;
        }
        scan.snapshot.hash = scan.snapshot.digest()?;
        let result = self.folder_view(context, &scan.snapshot)?;
        let mut snapshots = 0;
        self.store.visit("folder/", |_: Value| {
            snapshots += 1;
            if snapshots >= SNAPSHOT_LIMIT {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Retained folder snapshot quota reached; protected history was preserved",
                ));
            }
            Ok(())
        })?;
        let mut records = scan
            .assets
            .iter()
            .map(|asset| Ok((named("asset", &asset.id)?, value(asset)?)))
            .collect::<Result<Vec<_>>>()?;
        records.push((named("folder", &scan.snapshot.id)?, value(&scan.snapshot)?));
        let response = self.store.commit_with_clock(
            &context.principal,
            request,
            || self.clock.now(),
            result,
            &records,
        )?;
        if response
            .result
            .as_ref()
            .and_then(|result| result["id"].as_str())
            == Some(scan.snapshot.id.as_str())
        {
            let media_failure_count = scan
                .snapshot
                .entries
                .iter()
                .filter(|entry| entry.kind == FolderEntryKind::Media && entry.error.is_some())
                .count();
            tracing::info!(request_id=%request.request_id, snapshot_id=%scan.snapshot.id,
                snapshot_hash=%scan.snapshot.hash, registered_count=scan.assets.len(),
                media_failure_count, entry_count=scan.snapshot.entries.len(),
                complete=scan.snapshot.complete, "folder.snapshot.accepted");
        }
        Ok(response)
    }
}
impl FolderScan<'_> {
    fn stopped(&mut self) -> Result<bool> {
        if self.cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Folder intake cancelled before acceptance",
            ));
        }
        self.service.folder_intake_allowed(self.context)?;
        if self.snapshot.entries.len() >= MAX_FOLDER_ENTRIES
            || self.started.elapsed().as_secs() >= MAX_FOLDER_SECONDS
        {
            self.limit("Folder intake reached its entry or elapsed-time bound");
        }
        Ok(self.snapshot.scan_error.is_some())
    }
    fn limit(&mut self, message: &str) {
        self.snapshot.complete = false;
        self.snapshot.scan_error = Some(DomainError::new("RESOURCE_LIMIT", message));
    }
    fn row(
        &mut self,
        relative: String,
        kind: FolderEntryKind,
        outcome: FolderEntryOutcome,
        error: Option<DomainError>,
    ) {
        self.snapshot.entries.push(FolderEntry {
            index: 0,
            relative_path: relative,
            kind,
            outcome,
            identity: None,
            source_object: None,
            asset_id: None,
            error: error.map(bounded_error),
        });
    }
    fn visit(
        &mut self,
        stamp: Arc<files::DirectoryStamp>,
        depth: usize,
        deadline: nw_domain::execution::DeadlinePolicy,
    ) -> Result<()> {
        if self.stopped()? {
            return Ok(());
        }
        let directory = stamp.path();
        self.service
            .root_authority(self.context, directory, false)?;
        if self.snapshot.visited_directories as usize >= MAX_FOLDER_DIRECTORIES {
            self.limit("Folder intake reached its directory bound");
            return Ok(());
        }
        self.snapshot.visited_directories += 1;
        let mut entries = Vec::new();
        let mut names_bytes = 0usize;
        let enumeration = stamp.for_each_entry(|path| {
            if self.cancel.is_cancelled() {
                return Err(DomainError::new(
                    "CANCELLED",
                    "Folder enumeration cancelled",
                ));
            }
            if self.snapshot.visited_entries as usize >= MAX_FOLDER_VISITS {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Folder enumeration reached its filesystem-entry bound",
                ));
            }
            self.snapshot.visited_entries += 1;
            names_bytes = names_bytes.saturating_add(path.as_os_str().as_encoded_bytes().len());
            if names_bytes > MAX_FOLDER_DIRECTORY_NAME_BYTES {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Folder enumeration reached its bounded directory-name budget",
                ));
            }
            entries.push(path);
            Ok(())
        });
        if let Err(error) = enumeration {
            if error.code == "CANCELLED" {
                return Err(error);
            }
            // No filesystem-dependent unsorted prefix from an incomplete directory is admitted.
            self.snapshot.complete = false;
            if error.code == "RESOURCE_LIMIT" || directory == self.root {
                self.snapshot.scan_error = Some(bounded_error(error));
            } else {
                self.row(
                    relative_text(&self.root, directory)?,
                    FolderEntryKind::Directory,
                    FolderEntryOutcome::Rejected,
                    Some(error),
                );
            }
            return Ok(());
        }
        self.directories.push(stamp.clone());
        entries.sort();
        for path in entries {
            if self.stopped()? {
                break;
            }
            let relative = match relative_text(&self.root, &path) {
                Ok(relative) => relative,
                Err(error) => {
                    self.snapshot.complete = false;
                    self.snapshot.scan_error = Some(bounded_error(error));
                    break;
                }
            };
            // Exclude before metadata, opening or descending, including the real owner root.
            if path.starts_with(&self.service.root)
                || staging_path(&path)
                || self.snapshot.input.exclude_relative.iter().any(|excluded| {
                    relative == *excluded
                        || relative
                            .strip_prefix(excluded)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                })
            {
                self.row(
                    relative,
                    FolderEntryKind::Other,
                    FolderEntryOutcome::Excluded,
                    None,
                );
                continue;
            }
            let metadata = match ordinary_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.row(
                        relative,
                        FolderEntryKind::Other,
                        FolderEntryOutcome::Rejected,
                        Some(error),
                    );
                    continue;
                }
            };
            if metadata.is_dir() {
                if !self.snapshot.input.recursive {
                    self.row(
                        relative,
                        FolderEntryKind::Directory,
                        FolderEntryOutcome::Excluded,
                        None,
                    );
                } else if depth >= MAX_FOLDER_DEPTH {
                    self.snapshot.complete = false;
                    self.row(
                        relative,
                        FolderEntryKind::Directory,
                        FolderEntryOutcome::Rejected,
                        Some(DomainError::new(
                            "RESOURCE_LIMIT",
                            "Folder exceeds the admitted recursive depth",
                        )),
                    );
                } else {
                    let name = path.file_name().ok_or_else(|| {
                        DomainError::new(
                            "PATH_DENIED",
                            "Discovered directory needs an ordinary name",
                        )
                    })?;
                    match stamp.child_directory(name) {
                        Ok(child) => self.visit(Arc::new(child), depth + 1, deadline)?,
                        Err(error) => {
                            if self.cancel.is_cancelled() {
                                return Err(DomainError::new(
                                    "CANCELLED",
                                    "Folder directory admission cancelled",
                                ));
                            }
                            self.service.folder_intake_allowed(self.context)?;
                            self.service
                                .root_authority(self.context, &self.root, false)?;
                            self.snapshot.complete = false;
                            self.row(
                                relative,
                                FolderEntryKind::Directory,
                                FolderEntryOutcome::Rejected,
                                Some(error),
                            );
                        }
                    }
                }
                continue;
            }
            if !metadata.is_file() {
                self.row(
                    relative,
                    FolderEntryKind::Other,
                    FolderEntryOutcome::Rejected,
                    Some(DomainError::new(
                        "INPUT_DENIED",
                        "Folder intake admits only ordinary regular files",
                    )),
                );
                continue;
            }
            let kind = if media_hint(&path) {
                FolderEntryKind::Media
            } else {
                FolderEntryKind::Companion
            };
            if kind == FolderEntryKind::Companion
                && self.snapshot.input.companion_policy == CompanionIntakePolicy::Exclude
            {
                self.row(relative, kind, FolderEntryOutcome::CompanionExcluded, None);
                continue;
            }
            self.register_file(&stamp, path, relative, kind, deadline)?;
        }
        Ok(())
    }
    fn register_file(
        &mut self,
        parent: &files::DirectoryStamp,
        path: PathBuf,
        relative: String,
        kind: FolderEntryKind,
        deadline: nw_domain::execution::DeadlinePolicy,
    ) -> Result<()> {
        let result = (|| -> Result<_> {
            for directory in &self.directories {
                directory.revalidate()?;
            }
            let canonical = self.service.path(self.context, &path, false)?;
            if canonical != path {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Discovered source resolved outside its recorded relative path",
                ));
            }
            let name = path.file_name().ok_or_else(|| {
                DomainError::new("PATH_DENIED", "Discovered source needs an ordinary name")
            })?;
            let stamp = parent.open_child_source(name)?;
            let object = stamp.object_identity()?;
            let source_bytes = stamp.byte_len()?;
            let remaining = MAX_FOLDER_SOURCE_BYTES.saturating_sub(self.source_bytes);
            if source_bytes > remaining {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "Folder intake exceeds its 4 GiB aggregate source-size budget",
                ));
            }
            self.source_bytes += source_bytes;
            let (identity, asset, stamp) = if kind == FolderEntryKind::Media {
                let (mut asset, stamp) = tokio::runtime::Handle::current().block_on(
                    self.service.engine()?.register_held_with_policy(
                        stamp,
                        self.context.principal.clone(),
                        deadline,
                        source_bytes,
                        self.cancel.clone(),
                    ),
                )?;
                if !asset
                    .probe
                    .streams
                    .iter()
                    .any(|stream| stream.kind == nw_domain::mux::StreamKind::Video)
                {
                    return Err(DomainError::new(
                        "VIDEO_REQUIRED",
                        "Cleanup folder candidates require an admitted ordinary video stream",
                    ));
                }
                asset.display_name = relative.clone();
                (asset.identity.clone(), Some(asset), stamp)
            } else {
                (
                    stamp.identity_with_limit(self.cancel, source_bytes)?,
                    None,
                    stamp,
                )
            };
            stamp.revalidate()?;
            if identity.bytes != source_bytes.to_string() {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Source size differs from its held intake reservation",
                ));
            }
            Ok((identity, object, asset, stamp))
        })();
        if self.cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Folder intake cancelled before acceptance",
            ));
        }
        match result {
            Ok((identity, object, asset, stamp)) => {
                let asset_id = asset.as_ref().map(|asset| asset.id.clone());
                self.sources.push((relative.clone(), stamp));
                self.snapshot.entries.push(FolderEntry {
                    index: 0,
                    relative_path: relative,
                    kind,
                    outcome: if asset.is_some() {
                        FolderEntryOutcome::Registered
                    } else {
                        FolderEntryOutcome::CompanionInventoried
                    },
                    identity: Some(identity),
                    source_object: Some(object),
                    asset_id,
                    error: None,
                });
                if let Some(asset) = asset {
                    self.assets.push(asset);
                }
            }
            Err(error) => {
                let rejected_input = [
                    "VIDEO_REQUIRED",
                    "PATH_DENIED",
                    "INPUT_DENIED",
                    "SOURCE_CHANGED",
                    "TARGET_CHANGED",
                    "RESOURCE_LIMIT",
                    "NATIVE_PATH_UNAVAILABLE",
                    "NATIVE_IDENTITY_UNAVAILABLE",
                    "PERMISSION_DENIED",
                ]
                .contains(&error.code.as_str());
                self.row(
                    relative,
                    kind,
                    if kind == FolderEntryKind::Media && !rejected_input {
                        FolderEntryOutcome::ProbeFailed
                    } else {
                        FolderEntryOutcome::Rejected
                    },
                    Some(error),
                );
            }
        }
        Ok(())
    }
}
