//! Paces the emitted NUT profile against the current presentation clock.
use super::nut::{Format, NutReader};
use nw_domain::{
    DomainError, Result,
    preview::native_wire::{Format as WireFormat, Header},
    projects::grid_tick,
    time::{Rational, TimeRange},
};
use nw_platform::process::{AsyncChunkSink, SinkFuture};
use std::{
    io::{self, Cursor, Read},
    time::Duration,
};
use tokio::{
    runtime::Handle,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const STDOUT_CHUNKS: usize = 2;
const VIDEO_SLOTS: usize = 4;
const AUDIO_SLOTS: usize = 16;
const STALL_LIMIT: Duration = Duration::from_millis(750);
const AUDIO_RATE: i64 = 48_000;
const AUDIO_FRAME_BYTES: usize = 2 * 4;
const AUDIO_PACKET_SAMPLES: usize = 480;

#[derive(Clone, Copy)]
pub struct NativePreviewProfile {
    pub generation: u64,
    pub range: TimeRange,
    pub frame_rate: Rational,
    pub width: u32,
    pub height: u32,
    pub audio: bool,
}

impl NativePreviewProfile {
    pub(in crate::media) fn validate(self) -> Result<usize> {
        self.range.validate()?;
        let duration = self.range.end.checked_sub(self.range.start)?;
        if self.generation == 0
            || !(1..=1280).contains(&self.width)
            || !(1..=720).contains(&self.height)
            || self.frame_rate <= Rational::ZERO
            || self.frame_rate > Rational::new(60, 1)?
            || duration > Rational::new(120, 1)?
        {
            return Err(reject("Native preview profile exceeds its bounds"));
        }
        for point in [self.range.start, self.range.end] {
            grid_tick(point, self.frame_rate)?;
            if self.audio {
                grid_tick(point, Rational::new(AUDIO_RATE, 1)?)?;
            }
        }
        Ok((self.width * self.height * 4) as usize)
    }
}

pub struct NativePreviewPacket {
    pub generation: u64,
    pub sequence: u64,
    pub pts: Rational,
    pub duration: Rational,
    pub bytes: Vec<u8>,
}

impl NativePreviewPacket {
    pub fn wire_header(&self, format: WireFormat) -> Result<Header> {
        Ok(Header {
            generation: self.generation,
            sequence: self.sequence,
            pts: self.pts,
            duration: self.duration,
            format,
            payload_bytes: u32::try_from(self.bytes.len())
                .map_err(|_| reject("Native payload length exceeds its wire range"))?,
        })
    }
}

#[derive(Debug, Default)]
pub struct NativePreviewStats {
    pub video_frames: u64,
    pub audio_packets: u64,
    pub audio_sample_frames: u64,
    pub late_video_frames: u64,
    pub wire_bytes: u64,
}

struct ChunkSink {
    sender: Option<mpsc::Sender<Vec<u8>>>,
    cancel: CancellationToken,
}

impl AsyncChunkSink for ChunkSink {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> SinkFuture<'a> {
        Box::pin(async move {
            if bytes.len() > 8192 {
                return Err(reject(
                    "Native preview ingress exceeded the stdout chunk bound",
                ));
            }
            self.sender
                .as_ref()
                .ok_or_else(cancelled)?
                .send(bytes.to_vec())
                .await
                .map_err(|_| {
                    if self.cancel.is_cancelled() {
                        cancelled()
                    } else {
                        reject("Native preview demultiplexer exited")
                    }
                })
        })
    }

    fn finish(&mut self) -> SinkFuture<'_> {
        self.sender.take();
        Box::pin(async { Ok(()) })
    }
}

struct ChunkReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    current: Cursor<Vec<u8>>,
    runtime: Handle,
    cancel: CancellationToken,
}

impl Read for ChunkReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let already = self.current.read(buffer)?;
            if already != 0 {
                return Ok(already);
            }
            let chunk = self.runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = self.cancel.cancelled() => Err(io::Error::other("Preview generation cancelled")),
                    chunk = self.receiver.recv() => Ok(chunk),
                }
            })?;
            let Some(chunk) = chunk else { return Ok(0) };
            self.current = Cursor::new(chunk);
        }
    }
}

pub struct NativePreviewPipe {
    pub video: mpsc::Receiver<NativePreviewPacket>,
    pub audio: mpsc::Receiver<NativePreviewPacket>,
    control: NativePreviewControl,
    worker: JoinHandle<Result<NativePreviewStats>>,
    complete: bool,
}

#[derive(Clone)]
pub struct NativePreviewControl {
    position: watch::Sender<Rational>,
    profile: NativePreviewProfile,
    cancel: CancellationToken,
}

impl NativePreviewPipe {
    pub fn start(
        profile: NativePreviewProfile,
        parent: &CancellationToken,
    ) -> Result<(Self, Box<dyn AsyncChunkSink>)> {
        let picture_bytes = profile.validate()?;
        let runtime =
            Handle::try_current().map_err(|_| reject("Native preview needs the media runtime"))?;
        let cancel = parent.child_token();
        let (sender, receiver) = mpsc::channel(STDOUT_CHUNKS);
        let (video_tx, video) = mpsc::channel(VIDEO_SLOTS);
        let (audio_tx, audio) = mpsc::channel(AUDIO_SLOTS);
        let (position, clock) = watch::channel(profile.range.start);
        let reader = ChunkReader {
            receiver,
            current: Cursor::new(Vec::new()),
            runtime: runtime.clone(),
            cancel: cancel.clone(),
        };
        let worker_cancel = cancel.clone();
        let worker = runtime.spawn_blocking(move || {
            let outcome = demux(reader, profile, picture_bytes, clock, [video_tx, audio_tx]);
            if worker_cancel.is_cancelled() {
                Err(cancelled())
            } else {
                outcome
            }
        });
        Ok((
            Self {
                video,
                audio,
                control: NativePreviewControl {
                    position,
                    profile,
                    cancel: cancel.clone(),
                },
                worker,
                complete: false,
            },
            Box::new(ChunkSink {
                sender: Some(sender),
                cancel,
            }),
        ))
    }

    pub fn control(&self) -> NativePreviewControl {
        self.control.clone()
    }

    pub fn take_receivers(&mut self) -> [mpsc::Receiver<NativePreviewPacket>; 2] {
        [
            std::mem::replace(&mut self.video, mpsc::channel(1).1),
            std::mem::replace(&mut self.audio, mpsc::channel(1).1),
        ]
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.control.cancellation()
    }

    pub fn advance(&self, generation: u64, time: Rational) -> Result<()> {
        self.control.advance(generation, time)
    }

    pub async fn settled(mut self) -> Result<NativePreviewStats> {
        let result = (&mut self.worker)
            .await
            .map_err(|_| reject("Native preview demultiplexer could not settle"))?;
        self.complete = result.is_ok();
        result
    }
}

impl NativePreviewControl {
    pub fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn advance(&self, generation: u64, time: Rational) -> Result<()> {
        if generation != self.profile.generation || time > self.profile.range.end {
            return Err(reject(
                "Preview clock does not belong to the active generation or range",
            ));
        }
        if !self.position.send_if_modified(|position| {
            if time <= *position {
                return false;
            }
            *position = time;
            true
        }) && time != *self.position.borrow()
        {
            return Err(reject("Preview clock moved backwards"));
        }
        Ok(())
    }
}
impl Drop for NativePreviewPipe {
    fn drop(&mut self) {
        if !self.complete {
            self.control.cancel.cancel();
        }
    }
}

fn reject(message: &'static str) -> DomainError {
    DomainError::new("INVALID_NATIVE_PREVIEW", message)
}

fn cancelled() -> DomainError {
    DomainError::new("CANCELLED", "Native preview generation was cancelled")
}

fn demux(
    reader: ChunkReader,
    profile: NativePreviewProfile,
    picture_bytes: usize,
    mut clock: watch::Receiver<Rational>,
    senders: [mpsc::Sender<NativePreviewPacket>; 2],
) -> Result<NativePreviewStats> {
    let runtime = reader.runtime.clone();
    let cancel = reader.cancel.clone();
    let mut nut = NutReader::new(
        reader,
        picture_bytes.max(AUDIO_FRAME_BYTES * AUDIO_PACKET_SAMPLES),
    )?;
    let expected = [
        Format::Rgba {
            width: profile.width,
            height: profile.height,
        },
        Format::StereoPcmF32,
    ];
    if !nut
        .streams()
        .iter()
        .map(|stream| stream.format)
        .eq(expected.into_iter().take(1 + usize::from(profile.audio)))
    {
        return Err(reject(
            "NUT declarations differ from the selected native preview profile",
        ));
    }

    let horizon = Rational::new(1, 4)?;
    let lateness = Rational::new(3, 25)?;
    let frame = Rational::new(
        profile.frame_rate.denominator(),
        profile.frame_rate.numerator(),
    )?;
    let mut ends = [profile.range.start; 2];
    let mut counts = [0u64; 2];
    let mut late_video_frames = 0;
    let mut audio_sample_frames = 0;
    while let Some(packet) = nut.next_packet()? {
        let stream = packet.stream;
        let clock_unit = nut.streams()[stream].clock;
        let pts = profile
            .range
            .start
            .checked_add(clock_unit.checked_mul(Rational::new(packet.pts, 1)?)?)?;
        let duration = if stream == 0 {
            frame
        } else {
            let samples = packet.bytes.len() / AUDIO_FRAME_BYTES;
            if samples == 0 || samples > AUDIO_PACKET_SAMPLES {
                return Err(reject(
                    "Native audio packet exceeds 480 stereo sample frames",
                ));
            }
            audio_sample_frames += samples as u64;
            Rational::new(samples as i64, AUDIO_RATE)?
        };
        let end = pts.checked_add(duration)?;
        if pts != ends[stream] || end > profile.range.end {
            return Err(reject(
                "Native packet timing differs from the exact program interval",
            ));
        }
        ends[stream] = end;
        let sequence = counts[stream];
        counts[stream] += 1;
        let packet = NativePreviewPacket {
            generation: profile.generation,
            sequence,
            pts,
            duration,
            bytes: packet.bytes,
        };
        let late = runtime.block_on(async {
            let deliver = async {
                loop {
                    let time = *clock.borrow_and_update();
                    if pts <= time.checked_add(horizon)? {
                        if stream == 0 && end <= time.checked_sub(lateness)? {
                            return Ok::<_, DomainError>(true);
                        }
                        senders[stream].send(packet).await.map_err(|_| cancelled())?;
                        return Ok(false);
                    }
                    clock.changed().await.map_err(|_| cancelled())?;
                }
            };
            tokio::select! {
                _ = cancel.cancelled() => Err(cancelled()),
                result = tokio::time::timeout(STALL_LIMIT, deliver) => result
                    .map_err(|_| DomainError::new("PREVIEW_STALLED", "Native preview consumers did not advance the bounded clock or queues"))?,
            }
        })?;
        late_video_frames += u64::from(late);
    }

    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    if ends[0] != profile.range.end || (profile.audio && ends[1] != profile.range.end) {
        return Err(reject(
            "Native preview ended before all selected streams completed",
        ));
    }
    Ok(NativePreviewStats {
        video_frames: counts[0],
        audio_packets: counts[1],
        audio_sample_frames,
        late_video_frames,
        wire_bytes: nut.position(),
    })
}
