//! Framed native presentation metadata shared by the owner and its client.
use super::{EntityId, ProjectId, Rational, Revision, TimeRange};
use crate::{DomainError, Result, projects::grid_tick};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const MAX_HEADER_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Format {
    Rgba8 {
        width: u32,
        height: u32,
        frame_rate: Rational,
    },
    StereoF32Le {},
}

impl Format {
    pub fn units(self, payload_bytes: usize) -> Result<Rational> {
        match self {
            Self::Rgba8 { width, height, frame_rate } => {
                if !(1..=1280).contains(&width)
                    || !(1..=720).contains(&height)
                    || frame_rate <= Rational::ZERO
                    || frame_rate > Rational::new(60, 1)?
                    || payload_bytes != (width * height * 4) as usize
                {
                    return Err(wire_error("Invalid native RGBA geometry, cadence or payload length"));
                }
                Rational::new(frame_rate.denominator(), frame_rate.numerator())
            }
            Self::StereoF32Le {} => {
                if !(8..=480 * 8).contains(&payload_bytes) || !payload_bytes.is_multiple_of(8) {
                    return Err(wire_error("Invalid native PCM packet length"));
                }
                Rational::new((payload_bytes / 8) as i64, 48_000)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u16,
    pub session_id: EntityId,
    pub generation: u64,
    pub project_id: ProjectId,
    pub sequence_id: EntityId,
    pub revision: Revision,
    pub range: TimeRange,
    pub format: Format,
}

impl Descriptor {
    pub fn validate(&self) -> Result<()> {
        self.range.validate()?;
        if self.version != 1 || self.generation == 0
            || self.range.end.checked_sub(self.range.start)? > Rational::new(120, 1)?
        {
            return Err(wire_error("Invalid native preview descriptor version, generation or range"));
        }
        let clock = match self.format {
            Format::Rgba8 { width, height, frame_rate } => {
                let payload = usize::try_from(u64::from(width) * u64::from(height))
                    .ok()
                    .and_then(|pixels| pixels.checked_mul(4))
                    .ok_or_else(|| wire_error("Native preview dimensions exceed the limit"))?;
                self.format.units(payload)?;
                frame_rate
            }
            Format::StereoF32Le {} => Rational::new(48_000, 1)?,
        };
        grid_tick(self.range.start, clock)?;
        grid_tick(self.range.end, clock)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub generation: u64,
    pub sequence: u64,
    pub pts: Rational,
    pub duration: Rational,
    pub format: Format,
    pub payload_bytes: u32,
}

impl Header {
    pub fn validate(&self, expected: &Descriptor) -> Result<Rational> {
        if self.generation != expected.generation || self.format != expected.format {
            return Err(wire_error("Packet belongs to a different preview generation or format"));
        }
        if self.duration != self.format.units(self.payload_bytes as usize)? {
            return Err(wire_error("Native preview packet duration differs from the payload"));
        }
        let end = self.pts.checked_add(self.duration)?;
        if !expected.range.contains(self.pts) || end > expected.range.end {
            return Err(wire_error("Native preview packet is outside the captured interval"));
        }
        let clock = match self.format {
            Format::Rgba8 { frame_rate, .. } => frame_rate,
            Format::StereoF32Le {} => Rational::new(48_000, 1)?,
        };
        grid_tick(self.pts, clock)?;
        Ok(end)
    }
}

/// Detect stale or malformed headers before requesting their declared payloads.
pub struct PacketOrder {
    descriptor: Descriptor,
    next_packet: u64,
    next_time: Rational,
}

impl PacketOrder {
    pub fn new(descriptor: Descriptor) -> Result<Self> {
        descriptor.validate()?;
        let next_time = descriptor.range.start;
        Ok(Self { descriptor, next_packet: 0, next_time })
    }

    pub fn admit(&mut self, packet: &Header) -> Result<usize> {
        let end = packet.validate(&self.descriptor)?;
        match packet.format {
            Format::StereoF32Le {} => {
                if packet.sequence != self.next_packet || packet.pts != self.next_time {
                    return Err(wire_error("Native audio packets have missing or repeated time"));
                }
            }
            Format::Rgba8 { frame_rate, .. } => {
                let offset = packet.pts.checked_sub(self.descriptor.range.start)?;
                if packet.sequence < self.next_packet
                    || packet.pts < self.next_time
                    || packet.sequence != grid_tick(offset, frame_rate)?
                {
                    return Err(wire_error("Native picture packet order differs from its frame index"));
                }
            }
        }
        self.next_packet = packet.sequence.checked_add(1)
            .ok_or_else(|| wire_error("Native packet sequence exceeds its bound"))?;
        self.next_time = end;
        Ok(packet.payload_bytes as usize)
    }
}

pub fn header_length(prefix: [u8; 4]) -> Result<usize> {
    let bytes = u32::from_be_bytes(prefix) as usize;
    if !(1..=MAX_HEADER_BYTES).contains(&bytes) {
        return Err(wire_error("Native preview header exceeds its length limit"));
    }
    Ok(bytes)
}

pub fn encode_header(header: &impl Serialize) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(header).map_err(|_| wire_error("Native header cannot be encoded"))?;
    if json.len() > MAX_HEADER_BYTES {
        return Err(wire_error("Native preview header exceeds its length limit"));
    }
    let mut prefix = (json.len() as u32).to_be_bytes().to_vec();
    prefix.extend(json);
    Ok(prefix)
}

pub fn decode_header<T: DeserializeOwned>(prefix: [u8; 4], bytes: &[u8]) -> Result<T> {
    if header_length(prefix)? != bytes.len() {
        return Err(wire_error("Native preview header is truncated or has trailing bytes"));
    }
    serde_json::from_slice(bytes).map_err(|_| wire_error("Native preview header is invalid JSON"))
}

fn wire_error(message: &'static str) -> DomainError {
    DomainError::new("INVALID_NATIVE_PREVIEW", message)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    pub project_id: ProjectId,
    pub sequence_id: EntityId,
    pub revision: Revision,
    pub range: TimeRange,
    pub width: u32,
    pub height: u32,
    pub audio: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Opened {
    pub picture: Descriptor,
    pub sound: Option<Descriptor>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockUpdate {
    pub generation: u64,
    pub at: Rational,
}
