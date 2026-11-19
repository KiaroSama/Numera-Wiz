use super::{MAX_FRAGMENT_HEADERS, MAX_INIT_BYTES, invalid, take_box, verify_fragment, verify_initialization};
use nw_domain::{DomainError, Result, preview::MAX_SOURCE_SEGMENT_BYTES};
use nw_platform::process::{AsyncChunkSink, SinkFuture};
use tokio::sync::mpsc;

pub enum BrowserMp4Part {
    Initialization { mime_type: String, bytes: Vec<u8> },
    Fragment(Vec<u8>),
}

struct BrowserMp4Sink {
    buffered: Vec<u8>,
    leading: Option<Vec<u8>>,
    ordinal: u32,
    initialized: bool,
    sender: mpsc::Sender<BrowserMp4Part>,
}

pub fn browser_mp4_stream() -> (Box<dyn AsyncChunkSink>, mpsc::Receiver<BrowserMp4Part>) {
    let (sender, receiver) = mpsc::channel(2);
    let producer = BrowserMp4Sink { buffered: Vec::new(), leading: None, ordinal: 0, initialized: false, sender };
    (Box::new(producer), receiver)
}

/// Bound a box as soon as its complete header is present, before collecting its payload.
fn complete_box(buffer: &[u8]) -> Result<Option<usize>> {
    let Some(header) = buffer.first_chunk::<8>() else { return Ok(None); };
    let size = u32::from_be_bytes(header[..4].try_into().expect("MP4 length field"));
    let (length, header_bytes) = match size {
        0 => return Err(invalid("Open-ended boxes are forbidden in program playback")),
        1 => {
            let Some(header) = buffer.first_chunk::<16>() else { return Ok(None); };
            (u64::from_be_bytes(header[8..16].try_into().expect("Extended length field")), 16)
        }
        size => (u64::from(size), 8),
    };
    let limit = match &header[4..8] {
        b"ftyp" | b"moov" => MAX_INIT_BYTES,
        b"moof" => MAX_FRAGMENT_HEADERS,
        b"mdat" => MAX_SOURCE_SEGMENT_BYTES,
        _ => return Err(invalid("Unexpected program MP4 box")),
    };
    if length < header_bytes || length > limit as u64 {
        return Err(invalid("Program MP4 box exceeds its size bound"));
    }
    Ok((buffer.len() >= length as usize).then_some(length as usize))
}

impl BrowserMp4Sink {
    async fn accept(&mut self, bytes: Vec<u8>) -> Result<()> {
        let mut trailing = bytes.as_slice();
        let right = take_box(&mut trailing)?.ok_or_else(|| invalid("Missing program MP4 box"))?;
        let Some(mut prefix) = self.leading.take() else {
            let expected = if self.initialized { *b"moof" } else { *b"ftyp" };
            if right.kind != expected {
                return Err(invalid("Program MP4 box order differs"));
            }
            self.leading = Some(bytes);
            return Ok(());
        };
        let mut previous = prefix.as_slice();
        let left = take_box(&mut previous)?.ok_or_else(|| invalid("Missing program MP4 box prefix"))?;
        let packet = if self.initialized {
            self.ordinal = self.ordinal.checked_add(1).ok_or_else(|| invalid("Fragment index overflow"))?;
            verify_fragment(&left, &right, self.ordinal)?;
            prefix.extend_from_slice(&bytes);
            BrowserMp4Part::Fragment(prefix)
        } else {
            if prefix.len() + bytes.len() > MAX_INIT_BYTES {
                return Err(invalid("Program MP4 initialization exceeds its size bound"));
            }
            let mime_type = verify_initialization(&left, &right, true)?;
            self.initialized = true;
            prefix.extend_from_slice(&bytes);
            BrowserMp4Part::Initialization { mime_type, bytes: prefix }
        };
        self.sender.send(packet).await.map_err(|_| DomainError::new("CANCELLED", "Program playback receiver closed"))
    }
}

impl AsyncChunkSink for BrowserMp4Sink {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> SinkFuture<'a> {
        Box::pin(async move {
            if bytes.len() > 8192 || self.buffered.len() > MAX_SOURCE_SEGMENT_BYTES {
                return Err(invalid("Program MP4 ingress exceeds its byte bound"));
            }
            self.buffered.extend_from_slice(bytes);
            while let Some(length) = complete_box(&self.buffered)? {
                let rest = self.buffered.split_off(length);
                let complete = std::mem::replace(&mut self.buffered, rest);
                self.accept(complete).await?;
            }
            Ok(())
        })
    }

    fn finish(&mut self) -> SinkFuture<'_> {
        Box::pin(async {
            if !self.initialized || self.ordinal == 0 || !self.buffered.is_empty() || self.leading.is_some() {
                return Err(invalid("Program MP4 ended before its complete fragments"));
            }
            Ok(())
        })
    }
}
