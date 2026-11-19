//! Decode and deliver an authorized frame from a committed sequence.
use super::*;
use nw_domain::{
    media::{OccurrenceSource, bind_mixed_source, capture_mixed_frame, visible_occurrence},
    preview::{self, FrameOrigin, FrameTiming, TimelineFrame, TimelineFrameInput, TimelineObservation},
    projects::{EntityId, ProjectId},
    time::Rational,
};

struct TimelineFrameData {
    project_id: ProjectId,
    sequence_id: EntityId,
    origin: FrameOrigin,
    bytes: Vec<u8>,
    encoded: Option<EncodedTimeline>,
}

struct EncodedTimeline {
    component_sha256: String,
    graph_sha256: String,
    observation: TimelineObservation,
}

impl Service {
    pub(in crate::runtime) fn timeline_frame(
        &self,
        context: &Context,
        request: &Request,
        input: TimelineFrameInput,
        cancel: CancellationToken,
    ) -> Result<TimelineFrame> {
        let project = &request
            .project
            .as_ref()
            .ok_or_else(|| {
                DomainError::new(
                    "VALIDATION_ERROR",
                    "Timeline preview requires a project context",
                )
            })?
            .id;
        let (revision, at) = (input.revision, input.at);
        let data = self.timeline_frame_data(context, project, input, false, cancel)?;
        Ok(TimelineFrame {
            project_id: data.project_id,
            sequence_id: data.sequence_id,
            revision,
            at,
            origin: data.origin,
            width: preview::WIDTH,
            height: preview::HEIGHT,
            rgba_base64: STANDARD.encode(data.bytes),
        })
    }

    pub(super) fn observe_timeline_frame(
        &self,
        context: &Context,
        project: &ProjectId,
        input: TimelineFrameInput,
        cancel: CancellationToken,
    ) -> Result<StillFile> {
        let data = self.timeline_frame_data(context, project.as_str(), input, true, cancel)?;
        let encoded = data.encoded.expect("Encoded timeline frame");
        let range = encoded.observation.interval;
        let duration = range.end.checked_sub(range.start)?;
        let source_sha256 = match &encoded.observation.origin {
            FrameOrigin::Registered { source_sha256, .. } => Some(source_sha256.clone()),
            _ => None,
        };
        Ok(StillFile {
            bytes: data.bytes,
            actual_frame_index: None,
            timing: Some(FrameTiming {
                time_base: duration,
                pts: Some(range.start),
                duration: Some(duration),
            }),
            source_sha256,
            component_sha256: encoded.component_sha256,
            graph_sha256: encoded.graph_sha256,
            timeline: Some(encoded.observation),
        })
    }

    fn timeline_frame_data(
        &self,
        context: &Context,
        project: &str,
        input: TimelineFrameInput,
        encode_png: bool,
        cancel: CancellationToken,
    ) -> Result<TimelineFrameData> {
        let read = self.read_project(context, project, ObjectAction::Read)?;
        let capture = capture_mixed_frame(&read, &input.sequence_id, input.revision, input.at)?;
        drop(read);
        let mut held_source = None;
        let mut original_source_pts: Option<Rational> = None;
        let (origin, pixels) = match visible_occurrence(&capture, input.at) {
            Some(clip) => match &clip.source {
                OccurrenceSource::Registered {
                    asset_id,
                    stream_index,
                    content_sha256,
                    ..
                } => {
                    let (_, source) =
                        self.registered_project_source(context, asset_id, *stream_index, &cancel)?;
                    if source.asset.source_object.is_none() {
                        return Err(DomainError::new(
                            "INVALID_PLAN",
                            "Register the original again to capture its native file identity",
                        ));
                    }
                    let binding =
                        bind_mixed_source(clip, &source.asset, source.record_hash.clone())?;
                    let held = files::SourceStamp::capture_open_resolved(
                        Path::new(&source.asset.identity.path),
                        source.stamp.try_clone_file()?,
                    )?;
                    let pixels = tokio::runtime::Handle::current().block_on(
                        self.engine()?.preview_bound_frame(
                            &source.asset,
                            held,
                            &binding,
                            clip.source_range.start,
                            cancel.clone(),
                        ),
                    )?;
                    if encode_png {
                        original_source_pts = Some(binding.asset_origin.checked_add(clip.source_range.start)?);
                    }
                    held_source = Some(source);
                    (
                        FrameOrigin::Registered {
                            clip_id: clip.id.clone(),
                            asset_id: asset_id.clone(),
                            source_sha256: content_sha256.clone(),
                            stream_index: *stream_index,
                        },
                        pixels,
                    )
                }
                OccurrenceSource::SolidColor {
                    rgb: [red, green, blue],
                } => (
                    FrameOrigin::SolidColor {
                        clip_id: clip.id.clone(),
                    },
                    [*red, *green, *blue, 255].repeat(preview::WIDTH * preview::HEIGHT),
                ),
            },
            None => (
                FrameOrigin::Gap {},
                [0, 0, 0, 255].repeat(preview::WIDTH * preview::HEIGHT),
            ),
        };
        let (bytes, encoded) = if encode_png {
            let engine = self.engine()?;
            let png = tokio::runtime::Handle::current().block_on(engine.encode_monitor_png(
                pixels,
                self.root.join("runtime").join("timeline-monitor"),
                cancel.clone(),
            ))?;
            let graph = serde_json::to_vec(&(&capture, png.graph_sha256)).map_err(|_| {
                DomainError::new("SERIALIZATION_FAILED", "Timeline observation graph identity is unavailable")
            })?;
            let observation = TimelineObservation {
                interval: capture.range,
                origin: origin.clone(),
                original_source_pts,
            };
            (png.bytes, Some(EncodedTimeline {
                component_sha256: engine.ffmpeg.sha256.clone(),
                graph_sha256: nw_domain::projects::hash(&graph),
                observation,
            }))
        } else {
            (pixels, None)
        };
        let _guard = self.mutation.lock().map_err(|_| lock_error())?;
        if cancel.is_cancelled() {
            return Err(DomainError::new(
                "CANCELLED",
                "Timeline preview cancelled before delivery",
            ));
        }
        let command = if encode_png { preview::OBSERVE_COMMAND } else { "timeline.preview_frame" };
        if self
            .policy(context)?
            .as_ref()
            .is_some_and(|policy| !policy.allows_command(command))
        {
            return Err(DomainError::new(
                "PERMISSION_DENIED",
                "Timeline preview grant changed during decoding",
            ));
        }
        let read = self.read_project(context, project, ObjectAction::Read)?;
        let current = capture_mixed_frame(&read, &input.sequence_id, input.revision, input.at)?;
        if value(&current)? != value(&capture)? {
            return Err(DomainError::new(
                "STALE_REVISION",
                "Project capture changed during timeline preview",
            ));
        }
        drop(read);
        if let Some(source) = held_source {
            self.object(context, &source.asset.id, &source.asset.creator)?;
            let path = self.path(context, Path::new(&source.asset.identity.path), false)?;
            if path != Path::new(&source.asset.identity.path)
                || self
                    .store
                    .project_fact_hash(&named("asset", &source.asset.id)?)?
                    != source.record_hash
            {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source authority differs from the captured asset",
                ));
            }
            source.stamp.revalidate()?;
        }
        Ok(TimelineFrameData {
            project_id: capture.project_id,
            sequence_id: capture.sequence_id,
            origin,
            bytes,
            encoded,
        })
    }
}
