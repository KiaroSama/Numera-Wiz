//! Native picture scheduling against the monotonic display clock.
use super::*;
use anyhow::ensure;
use bytes::Bytes;
use nw_domain::{
    preview::native_wire::{self, ClockUpdate, Descriptor, Format, Header, OpenRequest, Opened, PacketOrder},
    time::Rational,
};
use nw_ui::{NativePictureFrame, NativePictureMailbox};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

struct PictureBody {
    response: reqwest::Response,
    pending: Bytes,
}

impl PictureBody {
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
        ensure!(self.profile.is_none() && !input.audio, "This picture transport uses the local owner");
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
                && opened.sound.is_none(),
            "Native owner returned a different preview capture"
        );
        let response = self.http.get(format!("{session}/picture")).bearer_auth(bearer).send().await?
            .error_for_status()?;
        ensure!(response.headers().get(reqwest::header::CONTENT_TYPE).is_some_and(|value| value == "application/octet-stream"),
            "Invalid native picture content type");
        let mut body = PictureBody { response, pending: Bytes::new() };
        let descriptor = body.header::<Descriptor>().await?.ok_or_else(|| anyhow::anyhow!("Missing native picture descriptor"))?;
        ensure!(descriptor == expected, "Native picture stream belongs to a different capture");
        let mut order = PacketOrder::new(descriptor.clone())?;
        let first = body.packet(&mut order).await?.ok_or_else(|| anyhow::anyhow!("Native picture stream contains no frames"))?;
        let origin = tokio::time::Instant::now();
        let done = CancellationToken::new();
        let render = async {
            let outcome = async {
                let mut next = Some(first);
                while let Some((header, rgba)) = next {
                    let offset = header.pts.checked_sub(descriptor.range.start)?;
                    let due = origin + delay(offset)?;
                    tokio::select! {
                        _ = done.cancelled() => return Ok(()),
                        _ = tokio::time::sleep_until(due) => {},
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
                ensure!(order.next_time() == descriptor.range.end, "Native picture stream ended before its last frame");
                Ok::<_, anyhow::Error>(())
            }.await;
            done.cancel();
            outcome
        };
        let advance = async {
            let result = self.advance_native_picture(session, bearer, &descriptor, origin, &done).await;
            if result.is_err() {
                done.cancel();
            }
            result
        };
        let (rendered, advanced) = tokio::join!(render, advance);
        rendered.and(advanced)
    }

    async fn advance_native_picture(
        &self,
        session: &str,
        bearer: &str,
        descriptor: &Descriptor,
        origin: tokio::time::Instant,
        done: &CancellationToken,
    ) -> Result<()> {
        let mut tick = tokio::time::interval(Duration::from_millis(40));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let path = format!("{session}/clock");
        loop {
            tokio::select! {
                _ = done.cancelled() => return Ok(()),
                _ = tick.tick() => {},
            }
            let nanos = origin.elapsed().as_nanos().min(i64::MAX as u128) as i64;
            let at = descriptor.range.start.checked_add(Rational::new(nanos, 1_000_000_000)?)?
                .min(descriptor.range.end);
            let request = self.http.put(&path).bearer_auth(bearer)
                .json(&ClockUpdate { generation: descriptor.generation, at })
                .timeout(Duration::from_secs(2)).send();
            tokio::select! {
                biased;
                _ = done.cancelled() => return Ok(()),
                response = request => { response?.error_for_status()?; }
            }
            if at == descriptor.range.end {
                done.cancelled().await;
                return Ok(());
            }
        }
    }
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
