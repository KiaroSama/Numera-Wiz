//! Play decoded stereo samples and track the audio device's predicted playout.
use anyhow::{Result, ensure};
use cpal::{
    FromSample, OutputCallbackInfo, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as clock_channel,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const RATE: u32 = 48_000;
const QUEUED_PACKETS: usize = 12;
const QUEUED_MARKS: usize = 32;

struct Mark {
    starts_at: Instant,
    first_frame: u64,
    frames: u64,
}

pub(super) struct AudioPlayout {
    pub feed: AudioFeed,
    pub clock: AudioClock,
    worker: Option<JoinHandle<Result<()>>>,
    stop: CancellationToken,
}

pub(super) struct AudioFeed {
    samples: mpsc::Sender<Box<[f32]>>,
    start: Option<oneshot::Sender<()>>,
    accepted: u64,
    expected: u64,
}

pub(super) struct AudioClock {
    marks: clock_channel::Receiver<Mark>,
    window: VecDeque<Mark>,
    failed: Arc<AtomicBool>,
    last_position: u64,
}

impl AudioPlayout {
    pub async fn open(expected: u64) -> Result<Self> {
        ensure!(
            (1..=120 * u64::from(RATE)).contains(&expected),
            "Native audio range exceeds its budget"
        );
        let (samples, receiver) = mpsc::channel(QUEUED_PACKETS);
        let (marks_tx, marks) = clock_channel::sync_channel(QUEUED_MARKS);
        let (started, play) = oneshot::channel();
        let (ready, initialized) = oneshot::channel();
        let stop = CancellationToken::new();
        let failed = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let driver_stop = stop.clone();
        let driver_failure = failed.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let cursor = Cursor {
                receiver,
                current: None,
                delivered: 0,
                marks: marks_tx,
                failed: driver_failure.clone(),
            };
            let stream = match device(cursor) {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return Err(error);
                }
            };
            if ready.send(Ok(())).is_err() {
                return Ok(());
            }
            runtime.block_on(async {
                tokio::select! {
                    _ = driver_stop.cancelled() => return Ok(()),
                    request = play => { if request.is_err() { return Ok(()); } }
                }
                stream
                    .play()
                    .inspect_err(|_| driver_failure.store(true, Ordering::Release))?;
                driver_stop.cancelled().await;
                Ok(())
            })
        });
        initialized.await?.map_err(anyhow::Error::msg)?;
        Ok(Self {
            feed: AudioFeed {
                samples,
                start: Some(started),
                accepted: 0,
                expected,
            },
            clock: AudioClock {
                marks,
                window: VecDeque::new(),
                failed,
                last_position: 0,
            },
            worker: Some(worker),
            stop,
        })
    }

    pub async fn settle(mut self) -> Result<()> {
        self.stop.cancel();
        self.worker.take().expect("Owned audio device").await?
    }
}

impl AudioFeed {
    pub async fn queue_packet(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            (8..=480 * 8).contains(&bytes.len()) && bytes.len().is_multiple_of(8),
            "Invalid native stereo sample extent"
        );
        let frames = (bytes.len() / 8) as u64;
        ensure!(
            self.accepted + frames <= self.expected,
            "Decoded PCM extends beyond the requested interval"
        );
        let mut data = Vec::with_capacity(bytes.len() / 4);
        for raw in bytes.as_chunks::<4>().0 {
            let value = f32::from_le_bytes(*raw);
            ensure!(
                value.is_finite(),
                "Native audition contains an invalid sample"
            );
            data.push(value.clamp(-1.0, 1.0));
        }
        self.samples
            .send(data.into_boxed_slice())
            .await
            .map_err(|_| anyhow::anyhow!("Native audio device stopped receiving samples"))?;
        self.accepted += frames;
        if self.accepted >= 4 * 480 || self.accepted == self.expected {
            self.start()?;
        }
        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        if let Some(start) = self.start.take() {
            start
                .send(())
                .map_err(|_| anyhow::anyhow!("Native audio device stopped before playback"))?;
        }
        Ok(())
    }

    pub fn complete_input(&self) -> Result<()> {
        ensure!(
            self.accepted == self.expected,
            "Native PCM response ended before the requested samples"
        );
        Ok(())
    }
}

impl AudioClock {
    pub fn position(&mut self) -> Result<u64> {
        self.position_at(Instant::now())
    }

    fn position_at(&mut self, now: Instant) -> Result<u64> {
        ensure!(
            !self.failed.load(Ordering::Acquire),
            "Native audio device stopped or its clock became unavailable"
        );
        while let Ok(mark) = self.marks.try_recv() {
            self.window.push_back(mark);
        }
        while self.window.get(1).is_some_and(|mark| mark.starts_at <= now) {
            self.window.pop_front();
        }
        ensure!(
            self.window.len() <= QUEUED_MARKS,
            "The device's future audio buffer exceeded its time budget"
        );
        if let Some(mark) = self.window.front()
            && let Some(elapsed) = now.checked_duration_since(mark.starts_at)
        {
            let frames = (elapsed.as_nanos() * u128::from(RATE) / 1_000_000_000)
                .min(u128::from(mark.frames)) as u64;
            self.last_position = self.last_position.max(mark.first_frame + frames);
        }
        Ok(self.last_position)
    }
}

impl Drop for AudioPlayout {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn device(cursor: Cursor) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let output = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("No native audio output device was found"))?;
    let selected = output
        .supported_output_configs()?
        .filter(|config| {
            config.channels() == 2
                && matches!(
                    config.sample_format(),
                    SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
                )
        })
        .filter_map(|config| config.try_with_sample_rate(RATE))
        .min_by_key(|config| match config.sample_format() {
            SampleFormat::F32 => 0,
            SampleFormat::I16 => 1,
            _ => 2,
        })
        .ok_or_else(|| {
            anyhow::anyhow!("The audio device does not offer a supported stereo 48 kHz stream")
        })?;
    let config = selected.config();
    match selected.sample_format() {
        SampleFormat::F32 => stream::<f32>(&output, config, cursor),
        SampleFormat::I16 => stream::<i16>(&output, config, cursor),
        SampleFormat::U16 => stream::<u16>(&output, config, cursor),
        _ => unreachable!("Filtered native sample type"),
    }
}

fn stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut cursor: Cursor,
) -> Result<cpal::Stream> {
    let failed = cursor.failed.clone();
    Ok(device.build_output_stream(
        config,
        move |samples: &mut [T], info| cursor.write(samples, info),
        move |_| failed.store(true, Ordering::Release),
        Some(Duration::from_secs(2)),
    )?)
}

struct Cursor {
    receiver: mpsc::Receiver<Box<[f32]>>,
    current: Option<(Box<[f32]>, usize)>,
    delivered: u64,
    marks: clock_channel::SyncSender<Mark>,
    failed: Arc<AtomicBool>,
}

impl Cursor {
    fn write<T: SizedSample + FromSample<f32>>(
        &mut self,
        samples: &mut [T],
        info: &OutputCallbackInfo,
    ) {
        let first_frame = self.delivered;
        for frame in samples.chunks_exact_mut(2) {
            if self.current.is_none() {
                self.current = self.receiver.try_recv().ok().map(|data| (data, 0));
            }
            let Some((data, position)) = self.current.as_mut() else {
                break; // CPAL fills unused output with silence.
            };
            frame[0] = <T as cpal::Sample>::from_sample(data[*position]);
            frame[1] = <T as cpal::Sample>::from_sample(data[*position + 1]);
            *position += 2;
            self.delivered += 1;
            if *position == data.len() {
                self.current = None;
            }
        }
        if self.delivered > first_frame {
            let timestamp = info.timestamp();
            let offset = timestamp
                .playback
                .saturating_duration_since(timestamp.callback);
            let mark = Mark {
                starts_at: Instant::now() + offset,
                first_frame,
                frames: self.delivered - first_frame,
            };
            if self.marks.try_send(mark).is_err() {
                self.failed.store(true, Ordering::Release);
            }
        }
    }
}

#[cfg(test)]
pub(super) fn exercise_device_clock() {
    let (callback, marks) = clock_channel::sync_channel(QUEUED_MARKS);
    let failed = Arc::new(AtomicBool::new(false));
    let mut clock = AudioClock {
        marks,
        window: VecDeque::new(),
        failed: failed.clone(),
        last_position: 0,
    };
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    callback
        .try_send(Mark {
            starts_at: at(15),
            first_frame: 0,
            frames: 480,
        })
        .unwrap();
    callback
        .try_send(Mark {
            starts_at: at(30),
            first_frame: 480,
            frames: 480,
        })
        .unwrap();
    assert_eq!(clock.position_at(at(10)).unwrap(), 0);
    assert_eq!(clock.position_at(at(20)).unwrap(), 240);
    assert_eq!(clock.position_at(at(28)).unwrap(), 480);
    assert_eq!(clock.position_at(at(35)).unwrap(), 720);
    assert_eq!(clock.position_at(at(50)).unwrap(), 960);
    failed.store(true, Ordering::Release);
    assert!(clock.position_at(at(51)).is_err());
}
