//! Read original RIFF/RF64 cue metadata without buffering the audio payload.
use nw_domain::{
    DomainError, Result,
    chapters::{WaveCue, WaveCueInventory},
    records::Asset,
};
use nw_platform::files::{SourceStamp, io_error};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use tokio_util::sync::CancellationToken;

const MAX_CUES: usize = 256;
const MAX_CHUNKS: usize = 4096;

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("INVALID_WAVE_CUES", message)
}

fn u32_at(bytes: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(bytes[index..index + 4].try_into().expect("fixed WAVE field"))
}

fn u64_at(bytes: &[u8], index: usize) -> u64 {
    u64::from_le_bytes(bytes[index..index + 8].try_into().expect("fixed WAVE field"))
}

type FourCc = [u8; 4];

struct Chunk {
    name: FourCc,
    data: u64,
    bytes: u64,
    next: u64,
}

struct Reader<'a> {
    file: File,
    cancel: &'a CancellationToken,
    chunks: usize,
    sizes: BTreeMap<FourCc, VecDeque<u64>>,
    data_size: Option<u64>,
}

impl Reader<'_> {
    fn at<const N: usize>(&mut self, offset: u64) -> Result<[u8; N]> {
        if self.cancel.is_cancelled() {
            return Err(DomainError::new("CANCELLED", "Wave cue inspection was cancelled"));
        }
        let mut bytes = [0u8; N];
        self.file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        self.file.read_exact(&mut bytes).map_err(io_error)?;
        Ok(bytes)
    }

    fn chunk(&mut self, offset: u64, end: u64) -> Result<Chunk> {
        self.chunks += 1;
        if self.chunks > MAX_CHUNKS {
            return Err(DomainError::new("RESOURCE_LIMIT", "Wave chunk inventory exceeds its metadata budget"));
        }
        let data = offset.checked_add(8).filter(|data| *data <= end)
            .ok_or_else(|| invalid("Truncated WAVE chunk header"))?;
        let head = self.at::<8>(offset)?;
        let name: FourCc = head[..4].try_into().expect("fixed WAVE chunk name");
        let raw = u32_at(&head, 4);
        let bytes = if raw == u32::MAX {
            let primary_data = if name == *b"data" { self.data_size.take() } else { None };
            primary_data.or_else(|| self.sizes.get_mut(&name)?.pop_front())
                .ok_or_else(|| invalid("RF64 chunk has no associated ds64 size"))?
        } else {
            u64::from(raw)
        };
        let next = data.checked_add(bytes).and_then(|offset| offset.checked_add(bytes & 1))
            .filter(|offset| *offset <= end).ok_or_else(|| invalid("WAVE chunk exceeds its parent extent"))?;
        Ok(Chunk { name, data, bytes, next })
    }

    fn container(&mut self, length: u64) -> Result<(u64, u64)> {
        let header = self.at::<12>(0)?;
        if &header[8..] != b"WAVE" {
            return Err(invalid("WAVE container signature is missing"));
        }
        let (first, size) = match &header[..4] {
            b"RIFF" => (12, u64::from(u32_at(&header, 4))),
            b"RF64" if u32_at(&header, 4) == u32::MAX => {
                let ds64 = self.chunk(12, length)?;
                if ds64.name != *b"ds64" || ds64.bytes < 28 {
                    return Err(invalid("RF64 requires a complete first ds64 chunk"));
                }
                let sizes = self.at::<28>(ds64.data)?;
                let rows = u64::from(u32_at(&sizes, 24));
                if rows > 1024 || 28 + rows * 12 > ds64.bytes {
                    return Err(invalid("RF64 size table exceeds its declared extent"));
                }
                self.data_size = Some(u64_at(&sizes, 8));
                for row in 0..rows {
                    let item = self.at::<12>(ds64.data + 28 + row * 12)?;
                    self.sizes.entry(item[..4].try_into().expect("fixed RF64 chunk name"))
                        .or_default().push_back(u64_at(&item, 4));
                }
                (ds64.next, u64_at(&sizes, 0))
            }
            _ => return Err(invalid("Expected an ordinary RIFF WAVE or RF64 source")),
        };
        let end = size.checked_add(8).filter(|end| (first..=length).contains(end))
            .ok_or_else(|| invalid("WAVE container extent is inconsistent with the source"))?;
        Ok((first, end))
    }

    fn points(&mut self, chunk: &Chunk, points: &mut BTreeMap<u32, WaveCue>) -> Result<()> {
        if chunk.bytes < 4 {
            return Err(invalid("Truncated cue inventory"));
        }
        let count = u32_at(&self.at::<4>(chunk.data)?, 0) as usize;
        if count > MAX_CUES - points.len() || 4 + count as u64 * 24 > chunk.bytes {
            return Err(invalid("Cue inventory exceeds its declared size or count limit"));
        }
        for ordinal in 0..count {
            let entry = self.at::<24>(chunk.data + 4 + ordinal as u64 * 24)?;
            if &entry[8..12] != b"data" || u32_at(&entry, 12) != 0 || u32_at(&entry, 16) != 0 {
                return Err(invalid("Cue does not use a sample offset from the first waveform data block"));
            }
            let id = u32_at(&entry, 0);
            let cue = WaveCue { id, sample_offset: u32_at(&entry, 20), sample_count: None, label: None };
            if points.insert(id, cue).is_some() {
                return Err(invalid("Duplicate WAVE cue identifier"));
            }
        }
        Ok(())
    }

    fn annotations(&mut self, list: &Chunk, points: &mut BTreeMap<u32, WaveCue>, text_bytes: &mut u64) -> Result<()> {
        let mut cursor = list.data + 4;
        let end = list.data + list.bytes;
        while cursor < end {
            let member = self.chunk(cursor, end)?;
            cursor = member.next;
            if !matches!(&member.name, b"labl" | b"note" | b"ltxt") {
                continue;
            }
            if member.bytes < 4 {
                return Err(invalid("Truncated WAVE annotation identifier"));
            }
            let id = u32_at(&self.at::<4>(member.data)?, 0);
            let Some(cue) = points.get_mut(&id) else { continue };
            if &member.name == b"ltxt" {
                if member.bytes < 20 {
                    return Err(invalid("Truncated WAVE cue region"));
                }
                let count = u32_at(&self.at::<8>(member.data)?, 4);
                cue.sample_count.get_or_insert(count);
            } else if cue.label.is_none() || &member.name == b"labl" {
                let size = member.bytes - 4;
                *text_bytes = text_bytes.checked_add(size).filter(|bytes| *bytes <= 65536)
                    .ok_or_else(|| invalid("WAVE cue text exceeds its metadata budget"))?;
                let mut text = vec![0; size as usize];
                self.file.seek(SeekFrom::Start(member.data + 4)).map_err(io_error)?;
                self.file.read_exact(&mut text).map_err(io_error)?;
                text.truncate(text.iter().position(|byte| *byte == 0).unwrap_or(text.len()));
                cue.label = Some(String::from_utf8_lossy(&text).into_owned());
            }
        }
        Ok(())
    }
}

fn inspect(file: File, length: u64, cancel: &CancellationToken) -> Result<(u32, Vec<WaveCue>)> {
    let mut reader = Reader { file, cancel, chunks: 0, sizes: BTreeMap::new(), data_size: None };
    let (mut cursor, end) = reader.container(length)?;
    let mut rate = None;
    let mut points = BTreeMap::new();
    let mut lists = Vec::new();
    while cursor < end {
        let chunk = reader.chunk(cursor, end)?;
        cursor = chunk.next;
        match &chunk.name {
            b"fmt " if chunk.bytes >= 16 && rate.is_none() => {
                rate = Some(u32_at(&reader.at::<8>(chunk.data)?, 4));
            }
            b"cue " => reader.points(&chunk, &mut points)?,
            b"LIST" if chunk.bytes >= 4 && reader.at::<4>(chunk.data)? == *b"adtl" => lists.push(chunk),
            _ => {}
        }
    }
    let rate = rate.filter(|rate| *rate != 0).ok_or_else(|| invalid("WAVE sample rate is absent or zero"))?;
    let mut text_bytes = 0;
    for list in &lists {
        reader.annotations(list, &mut points, &mut text_bytes)?;
    }
    let mut points: Vec<_> = points.into_values().collect();
    points.sort_by_key(|cue| (cue.sample_offset, cue.id));
    Ok((rate, points))
}

pub fn inspect_original(asset: &Asset, source: &SourceStamp, cancel: &CancellationToken) -> Result<WaveCueInventory> {
    if asset.probe.format != "wav" {
        return Err(invalid("Wave cue inspection requires a registered WAVE asset"));
    }
    let native = asset.source_object.as_ref().ok_or_else(|| invalid("Register the original WAVE file to capture its native identity"))?;
    source.require_object(native)?;
    let length = asset.identity.bytes.parse::<u64>().map_err(|_| invalid("Registered WAVE byte count is invalid"))?;
    if source.byte_len()? != length || source.identity_with_limit(cancel, length)? != asset.identity {
        return Err(DomainError::new("SOURCE_CHANGED", "Registered WAVE bytes changed before cue inspection"));
    }
    let (sample_rate_hz, cues) = inspect(source.try_clone_file()?, length, cancel)?;
    source.revalidate()?;
    Ok(WaveCueInventory { asset_id: asset.id.clone(), source_sha256: asset.identity.sha256.clone(), sample_rate_hz, cues })
}
