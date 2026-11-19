//! Owner-scoped playback state. Demand must arrive before starting the producer.
use super::{mixed::MixedInputs, *};
use nw_domain::{
    media::{MixedCapture, OccurrenceSource, capture_mixed},
    preview::native_wire::{ClockUpdate, Descriptor, Format, OpenRequest, Opened},
    projects::EntityId,
};
use nw_engine::{MixedProcessLimits, NativePreviewControl, NativePreviewOutput, NativePreviewPacket, NativePreviewPipe, NativePreviewProfile};
use std::collections::BTreeSet;
use tokio::sync::{Notify, mpsc, watch};

const MAX_ORIGINALS: usize = 4;
const STAGE_LIMIT: u64 = 128 * 1024 * 1024;
const START_LIMIT: Duration = Duration::from_secs(16);
const LIVE_LIMIT: Duration = Duration::from_secs(145);

#[derive(Clone, Copy)]
pub enum NativePreviewTrack {
    Picture,
    Sound,
}

impl NativePreviewTrack {
    fn index(self) -> usize {
        match self {
            Self::Picture => 0,
            Self::Sound => 1,
        }
    }
}

pub(in crate::runtime) struct Session {
    control: NativePreviewControl,
    opened: Opened,
    deadline: Instant,
    inputs: Arc<MixedInputs>,
    receivers: Mutex<[Option<mpsc::Receiver<NativePreviewPacket>>; 2]>,
    demand: Notify,
    result: watch::Sender<Option<Result<()>>>,
}

impl Session {
    fn check(&self) -> Result<()> {
        if Instant::now() >= self.deadline || self.control.cancellation().is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Native preview generation ended"));
        }
        Ok(())
    }
}

pub struct NativePreviewTransfer {
    service: Arc<Service>,
    context: Context,
    session: Arc<Session>,
    descriptor: Descriptor,
    receiver: mpsc::Receiver<NativePreviewPacket>,
    finished: bool,
}

impl NativePreviewTransfer {
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    pub async fn next_packet(&mut self) -> Result<Option<NativePreviewPacket>> {
        self.session.check()?;
        let cancel = self.session.control.cancellation();
        let packet = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Native preview was closed")),
            packet = self.receiver.recv() => packet,
        };
        // Check current revision and source authority off the transport thread.
        let service = self.service.clone();
        let context = self.context.clone();
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || service.check_native_preview(&context, &session))
            .await
            .map_err(|_| DomainError::new("WORKER_FAILED", "Native preview authority check failed"))??;
        if packet.is_none() {
            let mut report = self.session.result.subscribe();
            loop {
                if let Some(result) = report.borrow_and_update().clone() {
                    result?;
                    self.finished = true;
                    break;
                }
                report.changed().await.map_err(|_| DomainError::new(
                    "WORKER_FAILED", "Native preview settlement was lost",
                ))?;
            }
        }
        Ok(packet)
    }
}

impl Drop for NativePreviewTransfer {
    fn drop(&mut self) {
        if !self.finished {
            self.session.control.cancellation().cancel();
        }
    }
}

impl Service {
    fn native_preview_owner(&self, context: &Context) -> Result<()> {
        if context.profile.is_some() || context.principal != self.owner().principal {
            return Err(DomainError::new("PERMISSION_DENIED", "Native playback requires the local owner"));
        }
        Ok(())
    }

    fn check_native_preview(&self, context: &Context, session: &Session) -> Result<()> {
        session.check()?;
        self.native_preview_owner(context)?;
        let _gate = self.mutation.lock().map_err(|_| lock_error())?;
        let picture = &session.opened.picture;
        let project = self.read_project(context, picture.project_id.as_str(), ObjectAction::Read)?;
        if project.header().revision != picture.revision {
            return Err(DomainError::new("STALE_REVISION", "Project changed during native preview"));
        }
        drop(project);
        session.inputs.revalidate(self, context)
    }

    pub async fn open_native_preview(
        self: &Arc<Self>,
        context: Context,
        input: OpenRequest,
        listener: CancellationToken,
    ) -> Result<Opened> {
        self.native_preview_owner(&context)?;
        let slot = self.native_preview_slot.clone().try_acquire_owned().map_err(|_| {
            DomainError::new("OWNER_BUSY", "Finish the active native preview before opening another")
        })?;
        let engine = self.engine()?.clone();
        let width = input.width;
        let height = input.height;
        let audio = input.audio;
        let generation = self.native_preview_generation.fetch_update(
            Ordering::AcqRel, Ordering::Acquire, |current| current.checked_add(1),
        ).map_err(|_| DomainError::new("RESOURCE_LIMIT", "Native preview generation exhausted"))? + 1;
        let service = self.clone();
        let source_context = context.clone();
        let source_cancel = listener.clone();
        let (capture, inputs, workspace) = tokio::task::spawn_blocking(move || {
            service.capture_native_program(&source_context, &input, &source_cancel)
        }).await.map_err(|_| DomainError::new("WORKER_FAILED", "Native preview preparation failed"))??;
        let NativeSetup { profile, opened } = native_setup(&capture, generation, width, height, audio)?;
        let (mut pipe, sink) = NativePreviewPipe::start(profile, &listener)?;
        let [video, audio] = pipe.take_receivers();
        let session = Arc::new(Session {
            control: pipe.control(),
            opened: opened.clone(),
            deadline: Instant::now() + LIVE_LIMIT,
            inputs: Arc::new(inputs),
            receivers: Mutex::new([Some(video), profile.audio.then_some(audio)]),
            demand: Notify::new(),
            result: watch::channel(None).0,
        });
        self.check_native_preview(&context, &session)?;
        *self.native_preview.lock().map_err(|_| lock_error())? = Some(session.clone());
        let cancel = session.control.cancellation();
        self.pending.fetch_add(1, Ordering::AcqRel);
        let pending = Pending(self.pending.clone());
        tokio::spawn(async move {
            let _owners = (slot, pending);
            let mut sink = Some(sink);
            let outcome = async {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(DomainError::new("CANCELLED", "Native preview closed before demand")),
                    result = tokio::time::timeout(Duration::from_secs(5), session.demand.notified()) => {
                        result.map_err(|_| DomainError::new("PREVIEW_STALLED", "Native preview streams were not attached"))?;
                    }
                }
                let staged = stage_native_program(&engine, &session.inputs, &workspace, &cancel).await?;
                let assets = session.inputs.originals.values().map(|held| &held.asset).collect::<Vec<_>>();
                engine.stream_native_program(
                    &capture, &session.inputs.bindings, &assets, staged,
                    NativePreviewOutput {
                        profile,
                        sink: sink.take().expect("Claimed native stdout sink"),
                        limits: MixedProcessLimits {
                            workspace: &workspace,
                            max_bytes: nw_platform::process::MAX_VERIFIED_STREAM_BYTES,
                            deadline: Duration::from_secs(126),
                            cancel: cancel.clone(),
                        },
                    },
                ).await
            }.await;
            drop(sink);
            let parsed = pipe.settled().await.map(|_| ());
            session.result.send_replace(Some(outcome.and(parsed)));
        });
        Ok(opened)
    }

    fn capture_native_program(
        &self,
        context: &Context,
        input: &OpenRequest,
        cancel: &CancellationToken,
    ) -> Result<(MixedCapture, MixedInputs, files::DirectoryStamp)> {
        let project = self.read_project(context, input.project_id.as_str(), ObjectAction::Read)?;
        let capture = capture_mixed(&project, &input.sequence_id, input.revision, input.range)?;
        drop(project);
        let clips = || capture.tracks.iter().flat_map(|track| &track.clips);
        let assets = clips().filter_map(|clip| match &clip.source {
            OccurrenceSource::Registered { asset_id, .. } => Some(asset_id.as_str()),
            OccurrenceSource::SolidColor { .. } => None,
        }).collect::<BTreeSet<_>>();
        if assets.len() > MAX_ORIGINALS
            || capture.range.end.checked_sub(capture.range.start)? > nw_domain::time::Rational::new(120, 1)?
            || capture.settings.width > 3840 || capture.settings.height > 2160
        {
            return Err(DomainError::new("RESOURCE_LIMIT", "Native preview exceeds its source, raster or duration bounds"));
        }
        let path = nw_platform::security::private_directory(&self.root.join("runtime").join("native-program"))?;
        self.store.require_media_space(&path, STAGE_LIMIT * assets.len() as u64)?;
        let workspace = files::DirectoryStamp::capture_resolved(&path)?;
        let originals = self.bind_mixed_inputs(context, clips(), cancel)?;
        Ok((capture, originals, workspace))
    }

    fn native_preview_session(&self, context: &Context, id: &str) -> Result<Arc<Session>> {
        self.native_preview_owner(context)?;
        self.native_preview.lock().map_err(|_| lock_error())?.as_ref()
            .filter(|session| session.opened.picture.session_id.as_str() == id)
            .cloned()
            .ok_or_else(DomainError::object_not_found)
    }

    pub fn claim_native_preview(
        self: &Arc<Self>,
        context: Context,
        id: &str,
        track: NativePreviewTrack,
    ) -> Result<NativePreviewTransfer> {
        let session = self.native_preview_session(&context, id)?;
        self.check_native_preview(&context, &session)?;
        let descriptor = match track {
            NativePreviewTrack::Picture => session.opened.picture.clone(),
            NativePreviewTrack::Sound => session.opened.sound.clone().ok_or_else(DomainError::object_not_found)?,
        };
        let receiver = {
            let mut slots = session.receivers.lock().map_err(|_| lock_error())?;
            let receiver = slots[track.index()].take().ok_or_else(|| {
                DomainError::new("OBJECT_CONFLICT", "Native preview stream has already been claimed")
            })?;
            if slots.iter().all(Option::is_none) {
                session.demand.notify_one();
            }
            receiver
        };
        Ok(NativePreviewTransfer { service: self.clone(), context, session, descriptor, receiver, finished: false })
    }

    pub fn update_native_preview_clock(&self, context: &Context, id: &str, update: ClockUpdate) -> Result<()> {
        let session = self.native_preview_session(context, id)?;
        self.check_native_preview(context, &session)?;
        session.control.advance(update.generation, update.at)
    }

    pub fn close_native_preview(&self, context: &Context, id: &str) -> Result<()> {
        self.native_preview_owner(context)?;
        let session = {
            let mut active = self.native_preview.lock().map_err(|_| lock_error())?;
            if active.as_ref().is_none_or(|session| session.opened.picture.session_id.as_str() != id) {
                return Err(DomainError::object_not_found());
            }
            active.take().expect("The exact active session was checked")
        };
        session.control.cancellation().cancel();
        Ok(())
    }
}

struct NativeSetup {
    profile: NativePreviewProfile,
    opened: Opened,
}

fn native_setup(capture: &MixedCapture, generation: u64, width: u32, height: u32, audio: bool) -> Result<NativeSetup> {
    let profile = NativePreviewProfile {
        generation, range: capture.range, frame_rate: capture.settings.frame_rate,
        width, height, audio,
    };
    let picture = Descriptor {
        version: 1,
        session_id: EntityId::parse(Uuid::new_v4().to_string())?,
        generation: profile.generation,
        project_id: capture.project_id.clone(),
        sequence_id: capture.sequence_id.clone(),
        revision: capture.project_revision,
        range: profile.range,
        format: Format::Rgba8 { width, height, frame_rate: profile.frame_rate },
    };
    picture.validate()?;
    let sound = audio.then(|| Descriptor { format: Format::StereoF32Le {}, ..picture.clone() });
    if let Some(sound) = &sound {
        sound.validate()?;
    }
    Ok(NativeSetup { profile, opened: Opened { picture, sound } })
}

async fn stage_native_program(
    engine: &nw_engine::Engine,
    inputs: &MixedInputs,
    workspace: &files::DirectoryStamp,
    cancel: &CancellationToken,
) -> Result<Vec<nw_engine::StagedMixedInput>> {
    let start = Instant::now();
    let mut staged = Vec::with_capacity(inputs.originals.len());
    for held in inputs.originals.values() {
        let deadline = START_LIMIT.checked_sub(start.elapsed()).filter(|left| !left.is_zero()).ok_or_else(|| {
            DomainError::new("PROCESS_TIMEOUT", "Native preview staging exceeded its bound")
        })?;
        let stamp = files::SourceStamp::capture_open_resolved(
            Path::new(&held.asset.identity.path), held.stamp.try_clone_file()?,
        )?;
        let streams = inputs.bindings.iter().filter(|binding| binding.asset_id == held.asset.id).cloned().collect::<Vec<_>>();
        staged.push(engine.stage_mixed_original(
            &held.asset, stamp, &streams,
            MixedProcessLimits { workspace, max_bytes: STAGE_LIMIT, deadline, cancel: cancel.clone() },
        ).await?);
    }
    Ok(staged)
}
