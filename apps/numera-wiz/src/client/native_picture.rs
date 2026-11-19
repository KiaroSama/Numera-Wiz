//! Native picture scheduling against the monotonic display clock.
use super::{native_audio::{AudioClock, AudioFeed, AudioPlayout}, *};
use anyhow::ensure;
use bytes::Bytes;
use nw_domain::{
    preview::native_wire::{self, ClockUpdate, Descriptor, Format, Header, OpenRequest, Opened, PacketOrder},
    time::Rational,
};
use nw_ui::{NativePictureFrame, NativePictureMailbox};
use serde::de::DeserializeOwned;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

enum PlaybackClock<'a> {
    Monotonic(tokio::time::Instant),
    Device(&'a mut AudioClock),
}

struct NativeBody {
    response: reqwest::Response,
    pending: Bytes,
}

impl NativeBody {
    async fn take(&mut self, length: usize, can_end: bool) -> Result<Option<Vec<u8>>> {
        let mut bytes = Vec::with_capacity(length);
        while bytes.len() < length {
            if self.pending.is_empty() {
                let Some(chunk) = self.response.chunk().await? else {
                    ensure!(can_end && bytes.is_empty(), "Truncated native preview stream");
                    return Ok(None);
                };
                ensure!(chunk.len() <= 2 * 1280 * 720 * 4 + 8192, "Excessive native preview chunk");
                self.pending = chunk;
            }
            let copy = (length - bytes.len()).min(self.pending.len());
            bytes.extend_from_slice(&self.pending.split_to(copy));
        }
        Ok(Some(bytes))
    }

    async fn header<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let Some(prefix) = self.take(4, true).await? else {
            return Ok(None);
        };
        let prefix: [u8; 4] = prefix.try_into().expect("Exact native prefix");
        let length = native_wire::header_length(prefix)?;
        let bytes = self.take(length, false).await?.expect("Required header payload");
        Ok(Some(native_wire::decode_header(prefix, &bytes)?))
    }

    async fn packet(&mut self, order: &mut PacketOrder) -> Result<Option<(Header, Vec<u8>)>> {
        let Some(header) = self.header::<Header>().await? else {
            return Ok(None);
        };
        let size = order.admit(&header)?;
        let payload = self.take(size, false).await?.expect("Required picture payload");
        Ok(Some((header, payload)))
    }
}

impl NativeClient {
    pub(crate) async fn play_native_picture(
        &self,
        input: OpenRequest,
        mailbox: &NativePictureMailbox,
        ctx: &eframe::egui::Context,
    ) -> Result<()> {
        ensure!(self.profile.is_none(), "Native program playback uses the local owner");
        let record = self.attach().await?;
        let bearer = self.scoped_token(&record).await?;
        let base = format!("http://127.0.0.1:{}/internal/native-previews", record.port);
        let opening = async {
            Ok::<Opened, anyhow::Error>(
                self.http.post(&base).bearer_auth(&bearer).json(&input).send().await?
                    .error_for_status()?.json().await?,
            )
        };
        let opened = tokio::select! {
            _ = cancelled(mailbox) => return Ok(()),
            result = opening => result?,
        };
        let session = format!("{base}/{}", opened.picture.session_id);
        let pictured = self.present_native_picture(&session, &bearer, &input, opened, mailbox, ctx);
        let result = tokio::select! {
            _ = cancelled(mailbox) => Ok(()),
            result = pictured => result,
        };
        let closed = self.http.delete(&session).bearer_auth(&bearer)
            .timeout(Duration::from_secs(2)).send().await;
        if let Err(error) = closed.and_then(reqwest::Response::error_for_status) {
            tracing::debug!(status=?error.status(), "Native preview close was not acknowledged");
        }
        result
    }

    async fn present_native_picture(
        &self,
        session: &str,
        bearer: &str,
        input: &OpenRequest,
        opened: Opened,
        mailbox: &NativePictureMailbox,
        ctx: &eframe::egui::Context,
    ) -> Result<()> {
        let expected = opened.picture;
        expected.validate()?;
        let Format::Rgba8 { width, height, .. } = expected.format else {
            anyhow::bail!("Native owner did not select a picture format");
        };
        ensure!(
            expected.project_id == input.project_id
                && expected.sequence_id == input.sequence_id
                && expected.revision == input.revision
                && expected.range == input.range
                && (width, height) == (input.width, input.height)
                && opened.sound.is_some() == input.audio,
            "Native owner returned a different preview capture"
        );
        let mut audio = if let Some(sound) = &opened.sound {
            sound.validate()?;
            let expected_sound = Descriptor { format: Format::StereoF32Le {}, ..expected.clone() };
            ensure!(*sound == expected_sound, "Native audio belongs to a different preview");
            let duration = sound.range.end.checked_sub(sound.range.start)?;
            let samples = nw_domain::projects::grid_tick(duration, Rational::new(48_000, 1)?)?;
            Some(AudioPlayout::open(samples).await?)
        } else {
            None
        };
        let sound_stream = async {
            match opened.sound {
                Some(sound) => {
                    let reader = self.native_body(session, bearer, "sound", &sound).await?;
                    Ok::<_, anyhow::Error>(Some((sound, reader)))
                }
                None => Ok(None),
            }
        };
        let (mut body, sound) = tokio::try_join!(
            self.native_body(session, bearer, "picture", &expected),
            sound_stream,
        )?;
        let mut order = PacketOrder::new(expected.clone())?;
        let first = body.packet(&mut order).await?.ok_or_else(|| anyhow::anyhow!("Native picture stream contains no frames"))?;
        let origin = tokio::time::Instant::now();
        let done = CancellationToken::new();
        let (position, mut picture_clock) = watch::channel(expected.range.start);
        let mut audio_progress = position.subscribe();
        let (feed, sound_clock) = match audio.as_mut() {
            Some(audio) => (Some(&mut audio.feed), Some(&mut audio.clock)),
            None => (None, None),
        };

        let render = async {
            let result = async {
                let mut next = Some(first);
                while let Some((header, rgba)) = next {
                    if input.audio {
                        wait_for_time(&mut picture_clock, header.pts, &done).await?;
                    } else {
                        let due = origin + delay(header.pts.checked_sub(expected.range.start)?)?;
                        tokio::select! {
                            _ = done.cancelled() => return Ok(()),
                            _ = tokio::time::sleep_until(due) => {},
                        }
                    }
                    if done.is_cancelled() {
                        return Ok(());
                    }
                    mailbox.publish(NativePictureFrame {
                        width: width as usize, height: height as usize, at: header.pts, rgba,
                    });
                    ctx.request_repaint();
                    next = tokio::select! {
                        _ = done.cancelled() => return Ok(()),
                        packet = body.packet(&mut order) => packet?,
                    };
                }
                ensure!(order.next_time() == expected.range.end, "Native picture stream ended before its last frame");
                Ok::<_, anyhow::Error>(())
            }.await;
            if result.is_err() {
                done.cancel();
            }
            result
        };
        let playback = async {
            let result = match (sound, feed) {
                (Some((descriptor, body)), Some(feed)) => deliver_native_sound(descriptor, body, feed, &mut audio_progress, &done).await,
                (None, None) => Ok(()),
                _ => Err(anyhow::anyhow!("Native audio capture and device were not paired")),
            };
            if result.is_err() {
                done.cancel();
            }
            result
        };
        let media = async {
            let (rendered, played) = tokio::join!(render, playback);
            done.cancel();
            rendered.and(played)
        };
        let clock = match sound_clock {
            Some(clock) => PlaybackClock::Device(clock),
            None => PlaybackClock::Monotonic(origin),
        };
        let advance = async {
            let result = self.advance_native_picture(session, bearer, &expected, clock, &position, &done).await;
            if result.is_err() {
                done.cancel();
            }
            result
        };
        let (delivered, advanced) = tokio::join!(media, advance);
        let released = match audio {
            Some(device) => device.settle().await,
            None => Ok(()),
        };
        delivered.and(advanced).and(released)
    }

    async fn native_body(
        &self,
        session: &str,
        bearer: &str,
        track: &'static str,
        expected: &Descriptor,
    ) -> Result<NativeBody> {
        let response = self.http.get(format!("{session}/{track}")).bearer_auth(bearer).send().await?
            .error_for_status()?;
        ensure!(response.headers().get(reqwest::header::CONTENT_TYPE).is_some_and(|value| value == "application/octet-stream"),
            "Invalid native preview content type");
        let mut body = NativeBody { response, pending: Bytes::new() };
        let descriptor = body.header::<Descriptor>().await?.ok_or_else(|| anyhow::anyhow!("Missing native preview descriptor"))?;
        ensure!(descriptor == *expected, "Native stream belongs to a different capture");
        Ok(body)
    }

    async fn advance_native_picture(
        &self,
        session: &str,
        bearer: &str,
        descriptor: &Descriptor,
        mut clock: PlaybackClock<'_>,
        position: &watch::Sender<Rational>,
        done: &CancellationToken,
    ) -> Result<()> {
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let path = format!("{session}/clock");
        let mut last = None;
        let mut progressed = tokio::time::Instant::now();
        loop {
            tokio::select! {
                _ = done.cancelled() => return Ok(()),
                _ = tick.tick() => {},
            }
            let elapsed = match &mut clock {
                PlaybackClock::Device(clock) => Rational::new(i64::try_from(clock.position()?)?, 48_000)?,
                PlaybackClock::Monotonic(origin) => Rational::new(origin.elapsed().as_nanos().min(i64::MAX as u128) as i64, 1_000_000_000)?,
            };
            let at = descriptor.range.start.checked_add(elapsed)?.min(descriptor.range.end);
            if last == Some(at) {
                ensure!(progressed.elapsed() < Duration::from_secs(2), "Native playback clock stopped advancing");
                continue;
            }
            progressed = tokio::time::Instant::now();
            position.send_replace(at);
            let request = self.http.put(&path).bearer_auth(bearer)
                .json(&ClockUpdate { generation: descriptor.generation, at })
                .timeout(Duration::from_secs(2)).send();
            tokio::select! {
                biased;
                _ = done.cancelled() => return Ok(()),
                response = request => { response?.error_for_status()?; }
            }
            last = Some(at);
            if at == descriptor.range.end {
                done.cancelled().await;
                return Ok(());
            }
        }
    }

}

async fn deliver_native_sound(
    descriptor: Descriptor,
    mut body: NativeBody,
    feed: &mut AudioFeed,
    position: &mut watch::Receiver<Rational>,
    done: &CancellationToken,
) -> Result<()> {
    let end = descriptor.range.end;
    let mut order = PacketOrder::new(descriptor)?;
    loop {
        let packet = tokio::select! {
            _ = done.cancelled() => return Ok(()),
            packet = body.packet(&mut order) => packet?,
        };
        let Some((_, bytes)) = packet else {
            break;
        };
        tokio::select! {
            _ = done.cancelled() => return Ok(()),
            result = feed.queue_packet(&bytes) => result?,
        }
    }
    feed.complete_input()?;
    wait_for_time(position, end, done).await
}

async fn wait_for_time(position: &mut watch::Receiver<Rational>, at: Rational, done: &CancellationToken) -> Result<()> {
    while *position.borrow_and_update() < at {
        tokio::select! {
            _ = done.cancelled() => return Ok(()),
            changed = position.changed() => changed?,
        }
    }
    Ok(())
}

fn delay(time: Rational) -> Result<Duration> {
    let nanos = i128::from(time.numerator()) * 1_000_000_000 / i128::from(time.denominator());
    Ok(Duration::from_nanos(u64::try_from(nanos)?))
}

async fn cancelled(mailbox: &NativePictureMailbox) {
    while !mailbox.cancelled() {
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}
