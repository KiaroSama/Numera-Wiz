//! Decode and deliver an authorized frame from a committed sequence.
use super::*;
use nw_domain::{
    media::{OccurrenceSource, bind_mixed_source, capture_mixed_frame, visible_occurrence},
    preview::{self, FrameOrigin, TimelineFrame, TimelineFrameInput},
};

impl Service {
    pub(in crate::runtime) fn timeline_frame(
        &self,
        context: &Context,
        request: &Request,
        input: TimelineFrameInput,
        cancel: CancellationToken,
    ) -> Result<TimelineFrame> {
        let project = &request.project.as_ref().ok_or_else(|| {
            DomainError::new("VALIDATION_ERROR", "Timeline preview requires a project context")
        })?.id;
        let read = self.read_project(context, project, ObjectAction::Read)?;
        let capture = capture_mixed_frame(&read, &input.sequence_id, input.revision, input.at)?;
        drop(read);
        let mut held_source = None;
        let (origin, pixels) = match visible_occurrence(&capture, input.at) {
            Some(clip) => match &clip.source {
                OccurrenceSource::Registered { asset_id, stream_index, content_sha256, .. } => {
                    let (_, source) = self.registered_project_source(context, asset_id, *stream_index, &cancel)?;
                    if source.asset.source_object.is_none() {
                        return Err(DomainError::new("INVALID_PLAN", "Register the original again to capture its native file identity"));
                    }
                    let binding = bind_mixed_source(clip, &source.asset, source.record_hash.clone())?;
                    let held = files::SourceStamp::capture_open_resolved(
                        Path::new(&source.asset.identity.path), source.stamp.try_clone_file()?,
                    )?;
                    let pixels = tokio::runtime::Handle::current().block_on(self.engine()?.preview_bound_frame(
                        &source.asset, held, &binding, clip.source_range.start, cancel.clone(),
                    ))?;
                    held_source = Some(source);
                    (FrameOrigin::Registered {
                        clip_id: clip.id.clone(), asset_id: asset_id.clone(), source_sha256: content_sha256.clone(), stream_index: *stream_index,
                    }, pixels)
                }
                OccurrenceSource::SolidColor { rgb: [red, green, blue] } => (
                    FrameOrigin::SolidColor { clip_id: clip.id.clone() },
                    [*red, *green, *blue, 255].repeat(preview::WIDTH * preview::HEIGHT),
                ),
            },
            None => (FrameOrigin::Gap {}, [0, 0, 0, 255].repeat(preview::WIDTH * preview::HEIGHT)),
        };
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Timeline preview cancelled before delivery"));
        }
        if self.policy(context)?.as_ref().is_some_and(|policy| !policy.allows_command(&request.command.id)) {
            return Err(DomainError::new("PERMISSION_DENIED", "Timeline preview grant changed during decoding"));
        }
        let read = self.read_project(context, project, ObjectAction::Read)?;
        let current = capture_mixed_frame(&read, &input.sequence_id, input.revision, input.at)?;
        if value(&current)? != value(&capture)? {
            return Err(DomainError::new("STALE_REVISION", "Project capture changed during timeline preview"));
        }
        drop(read);
        if let Some(source) = held_source {
            self.object(context, &source.asset.id, &source.asset.creator)?;
            let path = self.path(context, Path::new(&source.asset.identity.path), false)?;
            if path != Path::new(&source.asset.identity.path)
                || self.store.project_fact_hash(&named("asset", &source.asset.id)?)? != source.record_hash
            {
                return Err(DomainError::new("SOURCE_CHANGED", "Registered source authority differs from the captured asset"));
            }
            source.stamp.revalidate()?;
        }
        Ok(TimelineFrame {
            project_id: capture.project_id,
            sequence_id: capture.sequence_id,
            revision: input.revision,
            at: input.at,
            origin,
            width: preview::WIDTH,
            height: preview::HEIGHT,
            rgba_base64: STANDARD.encode(pixels),
        })
    }
}
