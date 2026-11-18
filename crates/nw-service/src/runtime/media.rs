//! Authenticated finite compiler reviews do not persist new media records on old reader floors.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nw_domain::{
    capabilities::{CapabilityScope, ClearInput, InspectInput, RefreshInput},
    media::{self as domain, RenderInput},
};
use std::time::{Duration, Instant};

impl Service {
    pub(super) fn source_preview(
        &self,
        context: &Context,
        input: nw_domain::preview::FrameInput,
        cancel: CancellationToken,
    ) -> Result<nw_domain::preview::Frame> {
        use nw_domain::preview;
        let asset: Asset = self
            .store
            .get(&named("asset", &input.asset_id)?)?
            .ok_or_else(DomainError::object_not_found)?;
        self.object(context, &asset.id, &asset.creator)?;
        let path = self.path(context, Path::new(&asset.identity.path), false)?;
        let source = files::SourceStamp::capture_resolved(&path)?;
        let pixels =
            tokio::runtime::Handle::current().block_on(self.engine()?.preview_registered_frame(
                &asset,
                source,
                input.stream_index,
                input.frame,
                cancel.clone(),
            ))?;
        let _delivery = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Preview cancelled before delivery",
            ));
        }
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command("media.preview_frame"))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Source preview grant changed during decoding",
            ));
        }
        self.object(context, &asset.id, &asset.creator)?;
        self.path(context, Path::new(&asset.identity.path), false)?;
        Ok(preview::Frame {
            asset_id: asset.id,
            source_sha256: asset.identity.sha256,
            stream_index: input.stream_index,
            frame: input.frame,
            width: preview::WIDTH,
            height: preview::HEIGHT,
            rgba_base64: STANDARD.encode(pixels),
        })
    }

    pub(super) fn component_capabilities(
        &self,
        command: &str,
        params: Value,
        owner: bool,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let runtime = tokio::runtime::Handle::current();
        match command {
            "capabilities.inspect" => {
                let InspectInput {
                    scope: CapabilityScope::Components,
                } = commands::parse(params)?;
                value(&runtime.block_on(self.engine()?.inspect_capabilities(cancel))?)
            }
            "capabilities.refresh" => {
                let input: RefreshInput = commands::parse(params)?;
                if input.devices && !owner {
                    return Err(DomainError::new(
                        "PERMISSION_DENIED",
                        "An explicit NVENC device probe requires owner authority",
                    ));
                }
                if !input.components && !input.devices {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Select at least one capability source to refresh",
                    ));
                }
                value(&runtime.block_on(self.engine()?.refresh_capabilities(input, cancel))?)
            }
            "capabilities.clear" => {
                let ClearInput {
                    namespace: CapabilityScope::Components,
                    fingerprint,
                } = commands::parse(params)?;
                if fingerprint.len() != 64
                    || !fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(DomainError::new(
                        "VALIDATION_ERROR",
                        "Capability fingerprint must be a lowercase SHA-256 digest",
                    ));
                }
                value(&runtime.block_on(self.engine()?.clear_capabilities(&fingerprint, cancel))?)
            }
            _ => Err(DomainError::new(
                "UNKNOWN_COMMAND",
                "Unregistered capability command",
            )),
        }
    }

    pub(super) fn media_review(
        self: &Arc<Self>,
        context: &Context,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        let started = Instant::now();
        let input: RenderInput = commands::parse(request.params.clone())?;
        let reference = request.project.as_ref().ok_or_else(|| {
            DomainError::new(
                "VALIDATION_ERROR",
                "Media review requires a project context",
            )
        })?;
        let policy = self.policy(context)?;
        if policy
            .as_ref()
            .is_some_and(|p| !p.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Media review command is not granted",
            ));
        }
        let read = self.read_project(context, &reference.id, ObjectAction::Read)?;
        let header_sha256 = nw_domain::projects::hash(read.header_raw().as_bytes());
        let revision = read.header().revision;
        let capture = domain::capture(&read, &input)?;
        drop(read);
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Media review cancelled before component observation",
            ));
        }
        let engine = self.engine()?;
        let tools = tokio::runtime::Handle::current()
            .block_on(engine.qualify_media_compiler(cancel.clone()))?;
        let build =
            files::identity(&std::env::current_exe().map_err(files::io_error)?, cancel)?.sha256;
        let review_id = Uuid::new_v4().to_string();
        let plan = domain::compile(
            capture,
            tools,
            self.store.root_id(),
            &context.principal,
            &build,
            &review_id,
            self.clock.now()?,
        )?;
        let response = Response::success(&request.request_id, plan);
        if serde_json::to_vec(&response)
            .map_err(|_| {
                DomainError::new("SERIALIZATION_FAILED", "Media review serialization failed")
            })?
            .len()
            > commands::MAX_RESULT_BYTES
        {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Media review exceeds the response budget",
            ));
        }
        let current = self.read_project(context, &reference.id, ObjectAction::Read)?;
        if current.header().revision != revision
            || nw_domain::projects::hash(current.header_raw().as_bytes()) != header_sha256
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Project changed during media review",
            ));
        }
        let current_policy = self.policy(context)?;
        if current_policy.as_ref().map_or(0, |p| p.revision)
            != policy.as_ref().map_or(0, |p| p.revision)
            || current_policy
                .as_ref()
                .is_some_and(|p| !p.allows_command(&request.command.id))
        {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Policy changed during media review",
            ));
        }
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Media review cancelled before publication",
            ));
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Media review exceeds its 30-second admission",
            ));
        }
        Ok(response)
    }
}
