//! Authenticated finite compiler reviews do not persist new media records on old reader floors.
use super::*;
use nw_domain::media::{self as domain, RenderInput};
use std::time::{Duration, Instant};

impl Service {
    pub(super) fn media_review(
        self: &Arc<Self>,
        context: &Context,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        let started = Instant::now();
        let input: RenderInput = commands::parse(request.params.clone())?;
        let reference = request.project.as_ref().ok_or_else(|| {
            DomainError::new("VALIDATION_ERROR", "Media review requires a project context")
        })?;
        let policy = self.policy(context)?;
        if policy.as_ref().is_some_and(|p| !p.allows_command(&request.command.id)) {
            return Err(DomainError::new("PERMISSION_DENIED", "Media review command is not granted"));
        }
        let read = self.read_project(context, &reference.id, ObjectAction::Read)?;
        let header_sha256 = nw_domain::projects::hash(read.header_raw().as_bytes());
        let revision = read.header().revision;
        let capture = domain::capture(&read, &input)?;
        drop(read);
        if cancel.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Media review cancelled before component observation"));
        }
        let engine = self.engine()?;
        let tools = tokio::runtime::Handle::current()
            .block_on(engine.qualify_media_compiler(cancel.clone()))?;
        let build = files::identity(
            &std::env::current_exe().map_err(files::io_error)?,
            cancel,
        )?.sha256;
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
            .map_err(|_| DomainError::new("SERIALIZATION_FAILED", "Media review serialization failed"))?
            .len() > commands::MAX_RESULT_BYTES
        {
            return Err(DomainError::new("RESOURCE_LIMIT", "Media review exceeds the response budget"));
        }
        let current = self.read_project(context, &reference.id, ObjectAction::Read)?;
        if current.header().revision != revision
            || nw_domain::projects::hash(current.header_raw().as_bytes()) != header_sha256
        {
            return Err(DomainError::new("STALE_REVISION", "Project changed during media review"));
        }
        let current_policy = self.policy(context)?;
        if current_policy.as_ref().map_or(0, |p| p.revision)
            != policy.as_ref().map_or(0, |p| p.revision)
            || current_policy.as_ref().is_some_and(|p| !p.allows_command(&request.command.id))
        {
            return Err(DomainError::new("STALE_REVISION", "Policy changed during media review"));
        }
        if cancel.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Media review cancelled before publication"));
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err(DomainError::new("RESOURCE_LIMIT", "Media review exceeds its 30-second admission"));
        }
        Ok(response)
    }
}
