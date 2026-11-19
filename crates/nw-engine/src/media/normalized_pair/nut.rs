//! Pull reader for FFmpeg 9.0.2's version 3 RGBA and PCM NUT transport.
//! Packet payloads stay in mux order and use the exact clock declared by their stream.
use nw_domain::{DomainError, Result, time::Rational};
use std::io::{self, Read};

const IDENTIFIER: &[u8] = b"nut/multimedia container\0";
const MAIN: u64 = 0x4e4d7a561f5f04ad;
const STREAM: u64 = 0x4e5311405bf2f9db;
const SYNCPOINT: u64 = 0x4e4be4adeeca4569;
const INFO: u64 = 0x4e49ab68b596ba78;
const INDEX: u64 = 0x4e58dd672f23e64e;
const START: u8 = b'N';
const MAX_CONTROL_BYTES: usize = 32 * 1024;
const MAX_CONTROL_BURST: u64 = 256 * 1024;
const MAX_RESERVED_FIELDS: u64 = 16;
const MAX_TIME_BASES: u64 = 64;
const MAX_STREAMS: u64 = 2;

const KEY: u64 = 1;
const CODED_PTS: u64 = 8;
const STREAM_ID: u64 = 16;
const SIZE_MSB: u64 = 32;
const CHECKSUM: u64 = 64;
const RESERVED: u64 = 128;
const HEADER_INDEX: u64 = 1024;
const MATCH_TIME: u64 = 2048;
const CODED: u64 = 4096;
const INVALID: u64 = 8192;
const FRAME_FLAGS: u64 =
    KEY | CODED_PTS | STREAM_ID | SIZE_MSB | CHECKSUM | RESERVED | HEADER_INDEX | MATCH_TIME;

fn invalid(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}

fn read_exact(input: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    input
        .read_exact(bytes)
        .map_err(|_| invalid("Truncated or unreadable NUT record"))
}

fn next_byte(input: &mut impl Read) -> Result<Option<u8>> {
    let mut byte = [0];
    match input.read_exact(&mut byte) {
        Ok(()) => Ok(Some(byte[0])),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(_) => Err(invalid("Unreadable NUT transport")),
    }
}

fn unsigned(input: &mut impl Read) -> Result<u64> {
    let mut value = 0_u64;
    for _ in 0..10 {
        let mut byte = [0];
        read_exact(input, &mut byte)?;
        value = value
            .checked_mul(128)
            .and_then(|value| value.checked_add(u64::from(byte[0] & 127)))
            .ok_or_else(|| invalid("NUT variable integer overflow"))?;
        if byte[0] < 128 {
            return Ok(value);
        }
    }
    Err(invalid("NUT variable integer exceeded ten bytes"))
}

fn signed(input: &mut impl Read) -> Result<i64> {
    let value = unsigned(input)?;
    let magnitude = i64::try_from((value >> 1) + (value & 1))
        .map_err(|_| invalid("NUT signed integer overflow"))?;
    Ok(if value & 1 == 0 {
        -magnitude
    } else {
        magnitude
    })
}

fn bounded(input: &mut impl Read, max: u64) -> Result<u64> {
    let value = unsigned(input)?;
    if value > max {
        return Err(invalid("NUT field exceeds its profile limit"));
    }
    Ok(value)
}

// FFmpeg's AV_CRC_32_IEEE uses this polynomial and writes the bytes in
// wire order, so the residue over the protected bytes and checksum is zero.
fn crc32(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc >> 31 == 0 { 0 } else { 0x04c11db7 };
        }
    }
    crc
}

struct Wire<R> {
    input: R,
    position: u64,
    crc: Option<u32>,
}

impl<R: Read> Read for Wire<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(buffer)?;
        self.position = self
            .position
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("NUT byte position overflow"))?;
        if let Some(crc) = &mut self.crc {
            *crc = crc32(*crc, &buffer[..count]);
        }
        Ok(count)
    }
}

impl<R: Read> Wire<R> {
    fn startcode(&mut self) -> Result<u64> {
        let mut bytes = [0; 8];
        bytes[0] = START;
        read_exact(self, &mut bytes[1..])?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn expect_startcode(&mut self, expected: u64) -> Result<()> {
        let mut bytes = [0; 8];
        read_exact(self, &mut bytes)?;
        if u64::from_be_bytes(bytes) != expected {
            return Err(invalid("Unexpected NUT header sequence"));
        }
        Ok(())
    }

    fn control(&mut self, code: u64) -> Result<Vec<u8>> {
        self.crc = Some(crc32(0, &code.to_be_bytes()));
        let size = bounded(self, MAX_CONTROL_BYTES as u64)? as usize;
        if size < 4 {
            return Err(invalid("Invalid NUT control packet size"));
        }
        if size > 4096 {
            read_exact(self, &mut [0; 4])?;
            if self.crc != Some(0) {
                return Err(invalid("NUT forward pointer checksum differs"));
            }
        }
        self.crc = None;
        let mut bytes = vec![0; size];
        read_exact(self, &mut bytes)?;
        if crc32(0, &bytes) != 0 {
            return Err(invalid("NUT control packet checksum differs"));
        }
        bytes.truncate(size - 4);
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Rgba { width: u32, height: u32 },
    PcmS16 { sample_rate: u32, channels: u32 },
    StereoPcmF32,
}

impl Format {
    fn unit_bytes(self) -> Option<u64> {
        Some(match self {
            Self::Rgba { width, height } => u64::from(width)
                .checked_mul(u64::from(height))?
                .checked_mul(4)?,
            Self::PcmS16 { channels, .. } => 2 * u64::from(channels),
            Self::StereoPcmF32 => 8,
        })
    }

    fn accepts(self, size: u64) -> bool {
        self.unit_bytes().is_some_and(|unit| {
            size > 0
                && size.is_multiple_of(unit)
                && (!matches!(self, Self::Rgba { .. }) || size == unit)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stream {
    pub(super) clock: Rational,
    pub(super) format: Format,
    pts_shift: u32,
    max_pts_distance: u64,
    aspect_ratio: [u64; 2],
}

impl Stream {
    fn parse(
        mut body: &[u8],
        main: &MainHeader,
        index: usize,
        packet_limit: usize,
    ) -> Result<Self> {
        if unsigned(&mut body)? != index as u64 {
            return Err(invalid("Unexpected NUT stream index"));
        }
        let class = unsigned(&mut body)?;
        if unsigned(&mut body)? != 4 {
            return Err(invalid("Unsupported NUT codec tag size"));
        }
        let mut tag = [0; 4];
        read_exact(&mut body, &mut tag)?;
        let clock = main.time_bases[bounded(&mut body, main.time_bases.len() as u64 - 1)? as usize];
        let pts_shift = bounded(&mut body, 15)? as u32;
        let max_pts_distance = bounded(&mut body, i32::MAX as u64)?;
        // Delay, stream flags and codec extradata are zero for these raw streams.
        if unsigned(&mut body)? != 0 || unsigned(&mut body)? != 0 || unsigned(&mut body)? != 0 {
            return Err(invalid("Unsupported NUT stream flags, delay or extradata"));
        }
        let (format, aspect_ratio) = match (class, &tag) {
            (0, b"RGBA") => {
                let width = positive(&mut body)?;
                let height = positive(&mut body)?;
                let aspect = [
                    bounded(&mut body, i32::MAX as u64)?,
                    bounded(&mut body, i32::MAX as u64)?,
                ];
                if (aspect[0] == 0) != (aspect[1] == 0) || unsigned(&mut body)? != 0 {
                    return Err(invalid("Unsupported NUT video aspect ratio or color type"));
                }
                (Format::Rgba { width, height }, aspect)
            }
            (1, b"PSD\x10" | b"PFD\x20") => {
                let sample_rate = positive(&mut body)?;
                if unsigned(&mut body)? != 1 || clock != Rational::new(1, i64::from(sample_rate))? {
                    return Err(invalid("Invalid NUT PCM sample clock"));
                }
                let channels = positive(&mut body)?;
                let format = match &tag {
                    b"PSD\x10" => Format::PcmS16 {
                        sample_rate,
                        channels,
                    },
                    b"PFD\x20" if sample_rate == 48_000 && channels == 2 => Format::StereoPcmF32,
                    _ => return Err(invalid("Unsupported NUT float PCM stream shape")),
                };
                (format, [0; 2])
            }
            _ => return Err(invalid("Unsupported NUT preview codec")),
        };
        if !body.is_empty()
            || format
                .unit_bytes()
                .is_none_or(|unit| unit > packet_limit as u64)
        {
            return Err(invalid("NUT stream shape exceeds the packet profile"));
        }
        Ok(Self {
            clock,
            format,
            pts_shift,
            max_pts_distance,
            aspect_ratio,
        })
    }
}

fn positive(input: &mut impl Read) -> Result<u32> {
    let value = bounded(input, i32::MAX as u64)? as u32;
    if value == 0 {
        return Err(invalid("NUT stream dimension or rate is zero"));
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameCode {
    flags: u64,
    pts_delta: i16,
    stream: usize,
    size_mul: u64,
    size_lsb: u64,
    reserved: u64,
    header_index: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct MainHeader {
    stream_count: usize,
    max_distance: u64,
    time_bases: Vec<Rational>,
    codes: [FrameCode; 256],
    elision_headers: Vec<Vec<u8>>,
}

impl MainHeader {
    fn parse(mut body: &[u8], repeated: bool) -> Result<Self> {
        if unsigned(&mut body)? != 3 {
            return Err(invalid("Unsupported NUT version or syncpoint profile"));
        }
        let stream_count = bounded(&mut body, MAX_STREAMS)? as usize;
        let max_distance = bounded(&mut body, 65_536)?;
        let clock_count = bounded(&mut body, MAX_TIME_BASES)?;
        if stream_count == 0 || max_distance == 0 || clock_count == 0 {
            return Err(invalid(
                "NUT main header omits its stream or clock configuration",
            ));
        }
        let mut time_bases = Vec::new();
        for _ in 0..clock_count {
            let numerator = i64::from(positive(&mut body)?);
            let denominator = i64::from(positive(&mut body)?);
            let clock = Rational::new(numerator, denominator)?;
            if clock.numerator() != numerator || clock.denominator() != denominator {
                return Err(invalid("NUT time base is not reduced"));
            }
            time_bases.push(clock);
        }

        let mut code = FrameCode {
            flags: INVALID,
            pts_delta: 0,
            stream: 0,
            size_mul: 1,
            size_lsb: 0,
            reserved: 0,
            header_index: 0,
        };
        let mut codes = [code; 256];
        let mut slots = codes
            .iter_mut()
            .enumerate()
            .filter(|(index, _)| *index != usize::from(START));
        let mut remaining = 255;
        while remaining > 0 {
            code.flags = unsigned(&mut body)?;
            if code.flags & !(FRAME_FLAGS | CODED | INVALID) != 0 {
                return Err(invalid("Unsupported NUT frame-code flags"));
            }
            let fields = bounded(&mut body, 8)?;
            if fields > 0 {
                code.pts_delta = signed(&mut body)?
                    .try_into()
                    .map_err(|_| invalid("NUT frame PTS delta overflow"))?;
            }
            if fields > 1 {
                code.size_mul = bounded(&mut body, u64::from(u16::MAX))?;
            }
            if fields > 2 {
                code.stream = bounded(&mut body, stream_count as u64 - 1)? as usize;
            }
            let size = if fields > 3 {
                bounded(&mut body, u64::from(u16::MAX))?
            } else {
                0
            };
            code.reserved = if fields > 4 {
                bounded(&mut body, MAX_RESERVED_FIELDS)?
            } else {
                0
            };
            let count = if fields > 5 {
                bounded(&mut body, remaining)?
            } else {
                code.size_mul.saturating_sub(size)
            };
            if fields > 6 {
                signed(&mut body)?; // Match-time predictor for seeking.
            }
            if fields > 7 {
                code.header_index = bounded(&mut body, 127)? as usize;
            }
            if count == 0
                || count > remaining
                || size + count - 1 > u64::from(u16::MAX)
                || (code.flags & INVALID == 0 && code.size_mul == 0)
            {
                return Err(invalid("Invalid NUT frame-code run"));
            }
            for offset in 0..count {
                code.size_lsb = size + offset;
                let (_, slot) = slots
                    .next()
                    .ok_or_else(|| invalid("NUT frame-code table overflow"))?;
                *slot = code;
            }
            remaining -= count;
        }

        let mut elision_headers = vec![Vec::new()];
        let mut remaining_elision = 1024;
        let mut empty_tail = false;
        if !body.is_empty() {
            for _ in 0..bounded(&mut body, 127)? {
                let size = bounded(&mut body, 255)? as usize;
                // The pinned muxer increments its elision count after each
                // header set. Any extra entries in a repeat are empty.
                if repeated && size == 0 {
                    empty_tail = true;
                    continue;
                }
                if size == 0 || size > remaining_elision || empty_tail {
                    return Err(invalid("Invalid NUT elision header budget"));
                }
                let mut header = vec![0; size];
                read_exact(&mut body, &mut header)?;
                remaining_elision -= size;
                elision_headers.push(header);
            }
        }
        if !body.is_empty()
            || codes
                .iter()
                .any(|code| code.header_index >= elision_headers.len())
        {
            return Err(invalid("Invalid NUT main header elision table"));
        }
        Ok(Self {
            stream_count,
            max_distance,
            time_bases,
            codes,
            elision_headers,
        })
    }
}

pub(super) struct Packet {
    pub(super) stream: usize,
    pub(super) pts: i64,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct NutReader<R> {
    wire: Wire<R>,
    main: MainHeader,
    streams: Vec<Stream>,
    predictors: Vec<i64>,
    last_pts: Vec<Option<i64>>,
    syncpoint: Option<u64>,
    packet_limit: usize,
    ended: bool,
}

impl<R: Read> NutReader<R> {
    pub(super) fn new(input: R, packet_limit: usize) -> Result<Self> {
        let mut wire = Wire {
            input,
            position: 0,
            crc: None,
        };
        let mut identifier = [0; IDENTIFIER.len()];
        read_exact(&mut wire, &mut identifier)?;
        if identifier.as_slice() != IDENTIFIER {
            return Err(invalid("Invalid NUT file identifier"));
        }
        wire.expect_startcode(MAIN)?;
        let main = MainHeader::parse(&wire.control(MAIN)?, false)?;
        let mut streams = Vec::with_capacity(main.stream_count);
        for index in 0..main.stream_count {
            wire.expect_startcode(STREAM)?;
            streams.push(Stream::parse(
                &wire.control(STREAM)?,
                &main,
                index,
                packet_limit,
            )?);
        }
        let stream_count = main.stream_count;
        Ok(Self {
            wire,
            main,
            streams,
            packet_limit,
            predictors: vec![0; stream_count],
            last_pts: vec![None; stream_count],
            syncpoint: None,
            ended: false,
        })
    }

    pub(super) fn streams(&self) -> &[Stream] {
        &self.streams
    }

    pub(super) fn position(&self) -> u64 {
        self.wire.position
    }

    pub(super) fn next_packet(&mut self) -> Result<Option<Packet>> {
        if self.ended {
            return Ok(None);
        }
        // EOF or an invalid record terminates this reader.
        self.ended = true;
        let start = self.wire.position;
        loop {
            if self.wire.position - start > MAX_CONTROL_BURST {
                return Err(invalid(
                    "NUT control traffic exceeded the packet staging budget",
                ));
            }
            let Some(first) = next_byte(&mut self.wire)? else {
                break;
            };
            if first != START {
                let packet = self.frame(first)?;
                self.ended = false;
                return Ok(Some(packet));
            }
            let position = self.wire.position - 1;
            let code = self.wire.startcode()?;
            match code {
                MAIN => self.repeat_headers()?,
                INFO => self.info()?,
                SYNCPOINT => self.sync(position)?,
                INDEX => {
                    let index = self.wire.control(INDEX)?;
                    let distance = index.last_chunk::<8>().copied().map(u64::from_be_bytes);
                    if self.wire.position - start > MAX_CONTROL_BURST
                        || distance != Some(self.wire.position - position)
                        || next_byte(&mut self.wire)?.is_some()
                    {
                        return Err(invalid("Invalid terminal NUT index"));
                    }
                    break;
                }
                _ => return Err(invalid("Unexpected NUT start code")),
            }
        }
        Ok(None)
    }

    fn repeat_headers(&mut self) -> Result<()> {
        let main = MainHeader::parse(&self.wire.control(MAIN)?, true)?;
        if self.main != main {
            return Err(invalid("Repeated NUT main header changed"));
        }
        for (index, stream) in self.streams.iter().enumerate() {
            self.wire.expect_startcode(STREAM)?;
            let repeated =
                Stream::parse(&self.wire.control(STREAM)?, &main, index, self.packet_limit)?;
            if *stream != repeated {
                return Err(invalid("Repeated NUT stream header changed"));
            }
        }
        self.syncpoint = None;
        Ok(())
    }

    fn info(&mut self) -> Result<()> {
        let bytes = self.wire.control(INFO)?;
        let mut body = bytes.as_slice();
        bounded(&mut body, self.main.stream_count as u64)?;
        signed(&mut body)?; // Chapter ID.
        unsigned(&mut body)?; // Chapter start.
        unsigned(&mut body)?; // Chapter duration.
        let max_fields = (body.len() / 2) as u64;
        let count = bounded(&mut body, max_fields)?;
        for _ in 0..count {
            skip_string(&mut body)?;
            match signed(&mut body)? {
                -1 => skip_string(&mut body)?,
                -2 => {
                    skip_string(&mut body)?;
                    skip_string(&mut body)?;
                }
                -4 => {
                    unsigned(&mut body)?;
                }
                value if value <= -3 => {
                    signed(&mut body)?;
                }
                _ => {}
            }
        }
        if !body.is_empty() {
            return Err(invalid("Unexpected NUT metadata suffix"));
        }
        Ok(())
    }

    fn sync(&mut self, position: u64) -> Result<()> {
        let bytes = self.wire.control(SYNCPOINT)?;
        let mut body = bytes.as_slice();
        let coded = unsigned(&mut body)?;
        bounded(&mut body, position / 16)?; // Back-pointer in sixteen-byte units.
        if !body.is_empty() {
            return Err(invalid("Unsupported NUT syncpoint data"));
        }

        let count = self.main.time_bases.len() as u64;
        let ticks =
            i64::try_from(coded / count).map_err(|_| invalid("NUT syncpoint PTS overflow"))?;
        let clock = self.main.time_bases[(coded % count) as usize];
        for (stream, predictor) in self.streams.iter().zip(&mut self.predictors) {
            let numerator = i128::from(ticks)
                * i128::from(clock.numerator())
                * i128::from(stream.clock.denominator());
            let denominator =
                i128::from(clock.denominator()) * i128::from(stream.clock.numerator());
            *predictor = (numerator / denominator)
                .try_into()
                .map_err(|_| invalid("NUT syncpoint clock conversion overflow"))?;
        }
        self.syncpoint = Some(position);
        Ok(())
    }

    fn frame(&mut self, first: u8) -> Result<Packet> {
        if self
            .syncpoint
            .is_none_or(|position| self.wire.position - position > self.main.max_distance)
        {
            return Err(invalid("NUT frame is outside a valid syncpoint interval"));
        }
        self.wire.crc = Some(crc32(0, &[first]));
        let code = self.main.codes[usize::from(first)];
        if code.flags & INVALID != 0 {
            return Err(invalid("Invalid NUT packet frame code"));
        }
        let mut flags = code.flags;
        if flags & CODED != 0 {
            let coded = unsigned(&mut self.wire)?;
            if coded & !FRAME_FLAGS != 0 {
                return Err(invalid("Unsupported NUT coded frame flags"));
            }
            flags ^= coded;
        }
        let stream = if flags & STREAM_ID != 0 {
            bounded(&mut self.wire, self.streams.len() as u64 - 1)? as usize
        } else {
            code.stream
        };
        let declaration = self.streams[stream];
        let previous = self.predictors[stream];
        let pts = if flags & CODED_PTS != 0 {
            reconstruct_pts(unsigned(&mut self.wire)?, declaration.pts_shift, previous)?
        } else {
            previous
                .checked_add(i64::from(code.pts_delta))
                .ok_or_else(|| invalid("NUT packet PTS delta overflow"))?
        };
        if pts < 0 || self.last_pts[stream].is_some_and(|last| pts <= last) {
            return Err(invalid("NUT packet PTS is negative or did not advance"));
        }
        Rational::new(pts, 1)?
            .checked_mul(declaration.clock)
            .map_err(|_| invalid("NUT packet timestamp exceeds the exact clock range"))?;

        let mut size = code.size_lsb;
        if flags & SIZE_MSB != 0 {
            size = unsigned(&mut self.wire)?
                .checked_mul(code.size_mul)
                .and_then(|high| high.checked_add(size))
                .ok_or_else(|| invalid("NUT packet size overflow"))?;
        }
        if size > self.packet_limit as u64 || !declaration.format.accepts(size) {
            return Err(invalid(
                "NUT packet size differs from its stream shape or limit",
            ));
        }
        if flags & MATCH_TIME != 0 {
            signed(&mut self.wire)?;
        }
        let header_index = if flags & HEADER_INDEX != 0 {
            bounded(&mut self.wire, self.main.elision_headers.len() as u64 - 1)? as usize
        } else {
            code.header_index
        };
        let prefix = &self.main.elision_headers[if size > 4096 { 0 } else { header_index }];
        if prefix.len() as u64 > size {
            return Err(invalid("NUT elision header exceeds the packet length"));
        }

        let reserved = if flags & RESERVED != 0 {
            bounded(&mut self.wire, MAX_RESERVED_FIELDS)?
        } else {
            code.reserved
        };
        for _ in 0..reserved {
            unsigned(&mut self.wire)?;
        }
        if flags & CHECKSUM != 0 {
            read_exact(&mut self.wire, &mut [0; 4])?;
            if self.wire.crc != Some(0) {
                return Err(invalid("NUT frame header checksum differs"));
            }
        } else if size - prefix.len() as u64 > 2 * self.main.max_distance
            || pts.abs_diff(previous) > declaration.max_pts_distance
        {
            return Err(invalid("NUT frame header lacks a required checksum"));
        }
        self.wire.crc = None;

        let mut bytes = vec![0; size as usize];
        bytes[..prefix.len()].copy_from_slice(prefix);
        read_exact(&mut self.wire, &mut bytes[prefix.len()..])?;
        self.predictors[stream] = pts;
        self.last_pts[stream] = Some(pts);
        Ok(Packet { stream, pts, bytes })
    }
}

fn reconstruct_pts(coded: u64, shift: u32, previous: i64) -> Result<i64> {
    let modulus = 1_u64 << shift;
    if coded >= modulus {
        return i64::try_from(coded - modulus).map_err(|_| invalid("NUT full PTS overflow"));
    }
    let mask = i128::from(modulus - 1);
    let center = i128::from(previous) - mask / 2;
    (((i128::from(coded) - center) & mask) + center)
        .try_into()
        .map_err(|_| invalid("NUT reconstructed PTS overflow"))
}

fn skip_string(body: &mut &[u8]) -> Result<()> {
    let max = body.len() as u64;
    let size = bounded(body, max)? as usize;
    *body = body
        .get(size..)
        .ok_or_else(|| invalid("Truncated NUT metadata value"))?;
    Ok(())
}
