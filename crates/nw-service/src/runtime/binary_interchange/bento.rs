//! Translate standard OMF Bento 2.0 bookkeeping to and from the pinned codec's internal dialect.
use super::{Format, preflight::exact_rate};
use nw_domain::{DomainError, Result};
use std::collections::{BTreeMap, BTreeSet};

const LABEL: [u8; 8] = [0xa4, b'C', b'M', 0xa5, b'H', b'd', b'r', 0xd7];
const LABEL_SIZE: usize = 24;
const TOC_OBJECT: u32 = 1;
const TOC_TYPE: u32 = 19;
const TYPE_NAME: u32 = 23;
const PROPERTY_NAME: u32 = 24;
const REFERENCE_LIST: u32 = 31;
const REFERENCE_DATA: u32 = 32;
const CODEC_FIRST_ID: u32 = 100;
const STANDARD_FIRST_ID: u32 = 65536;
const BLOCK_SIZE: usize = 1024;
const MAX_VALUES: usize = 16_384;

fn invalid() -> DomainError {
    Format::Omf.invalid("Invalid OMF Bento label, value or reference table")
}

fn word(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| invalid())?))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Standard,
    Codec,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    object: u32,
    property: u32,
    kind: u32,
}

#[derive(Clone, Copy)]
struct Value<'a> {
    key: Key,
    reference_list: u32,
    data: &'a [u8],
}

struct Reader<'a> {
    input: &'a [u8],
    toc: &'a [u8],
    block_size: usize,
    dialect: Dialect,
}

impl<'a> Reader<'a> {
    fn open(input: &'a [u8], dialect: Dialect) -> Result<Self> {
        let index = input.len().checked_sub(LABEL_SIZE).ok_or_else(invalid)?;
        let label = &input[index..];
        let major = if dialect == Dialect::Standard { 2 } else { 1 };
        if label[..8] != LABEL || label[12..16] != [0, major, 0, 0] {
            return Err(invalid());
        }
        let block_size = usize::from(u16::from_be_bytes([label[10], label[11]]).max(1)) * 1024;
        let start = word(&label[16..20])? as usize;
        let size = word(&label[20..24])? as usize;
        let toc = input[..index].get(start..start.checked_add(size).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        Ok(Self { input, toc, block_size, dialect })
    }

    fn extent(&self, operand: &[u8]) -> Result<&'a [u8]> {
        let (offset, length) = operand.split_at(operand.len() - 4);
        let wide = match offset.try_into() {
            Ok(bytes) => u64::from_be_bytes(bytes),
            Err(_) => u64::from(word(offset)?),
        };
        let start = usize::try_from(wide).map_err(|_| invalid())?;
        let length = word(length)? as usize;
        self.input.get(start..start.checked_add(length).ok_or_else(invalid)?)
            .ok_or_else(invalid)
    }

    fn read(&self) -> Result<Vec<Value<'a>>> {
        let mut key = Key::default();
        let mut reference_list = 0;
        let mut has_value = false;
        let mut position = 0;
        let mut entries = Vec::new();
        while let Some(&op) = self.toc.get(position) {
            position += 1;
            if op == 24 {
                position = (position - 1) / self.block_size * self.block_size + self.block_size;
                key = Key::default();
                reference_list = 0;
                has_value = false;
                continue;
            }
            let size = match (self.dialect, op) {
                (_, 0 | 255) | (Dialect::Standard, 9) => 0,
                (Dialect::Standard, 1) => 12,
                (Dialect::Standard, 2) | (_, 5) => 8,
                (_, 7) => 12,
                (_, 1..=4 | 9..=13 | 15) => 4,
                (_, 6 | 8 | 14) => return Err(Format::Omf.unsupported("Fragmented Bento values are outside the OMF cut subset")),
                _ => return Err(invalid()),
            };
            let operand = self.toc.get(position..position + size).ok_or_else(invalid)?;
            position += size;
            match (self.dialect, op) {
                (Dialect::Standard, 1) => key = Key { object: word(&operand[..4])?, property: word(&operand[4..8])?, kind: word(&operand[8..])? },
                (Dialect::Standard, 2) => {
                    key.property = word(&operand[..4])?;
                    key.kind = word(&operand[4..])?;
                }
                (Dialect::Codec, 1) => key = Key { object: word(operand)?, ..Key::default() },
                (Dialect::Codec, 2) => {
                    key.property = word(operand)?;
                    key.kind = 0;
                }
                (_, 3) => key.kind = word(operand)?,
                (_, 15) if !has_value => reference_list = word(operand)?,
                (_, 15) => return Err(invalid()),
                (_, 5 | 7 | 9..=13) => {
                    if has_value || key.object == 0 || key.property == 0 || key.kind == 0 || entries.len() == MAX_VALUES {
                        return Err(invalid());
                    }
                    let data = match op {
                        5 | 7 => self.extent(operand)?,
                        _ => &operand[..usize::from(op - 9)],
                    };
                    entries.push(Value { key, reference_list, data });
                    has_value = true;
                }
                _ => {}
            }
            if (1..=3).contains(&op) {
                reference_list = 0;
                has_value = false;
            }
        }
        Ok(entries)
    }
}

fn names<'a>(entries: &[Value<'a>]) -> Result<BTreeMap<u32, &'a str>> {
    let mut namespaces = BTreeSet::new();
    let mut names = BTreeMap::new();
    for entry in entries.iter().filter(|entry| matches!(entry.key.property, TYPE_NAME | PROPERTY_NAME)) {
        let name = std::str::from_utf8(entry.data).map_err(|_| invalid())?.trim_end_matches('\0');
        if name.len() > 255 || !namespaces.insert(name) || names.insert(entry.key.object, name).is_some() {
            return Err(invalid());
        }
    }
    Ok(names)
}

fn data_order(entries: &[Value<'_>], names: &BTreeMap<u32, &str>, head: u32) -> Result<bool> {
    let mut values = entries.iter().filter(|entry| entry.key.object == head
        && names.get(&entry.key.property) == Some(&"OMFI:HEAD:ByteOrder"));
    let order = match values.next().map(|entry| entry.data) {
        Some(b"MM") => true,
        Some(b"II") => false,
        _ => return Err(invalid()),
    };
    if values.next().is_some() {
        return Err(invalid());
    }
    Ok(order)
}

fn media_guard(entries: &[Value<'_>], names: &BTreeMap<u32, &str>, big_endian: bool) -> Result<()> {
    let format = Format::Omf;
    let decode: fn([u8; 4]) -> i32 = if big_endian { i32::from_be_bytes } else { i32::from_le_bytes };
    for entry in entries {
        match names.get(&entry.key.property).copied() {
            Some("OMFI:HEAD:Version") if entry.data != [2, 0] => return Err(format.unsupported("Linked OMF requires Interchange version 2.0")),
            Some("OMFI:HEAD:MediaData") if !entry.data.is_empty() && entry.data != [0; 2] => return Err(format.unsupported("Embedded OMF essence requires a separate media extraction workflow")),
            Some("OMFI:WAVE:Data" | "OMFI:AIFC:Data" | "OMFI:WAVE:AudioData" | "OMFI:AIFC:AudioData") => {
                return Err(format.unsupported("Embedded OMF essence requires a separate media extraction workflow"));
            }
            Some("OMFI:WAVD:Summary" | "OMFI:AIFD:Summary") if !pcm_header(entry.data) => {
                return Err(format.unsupported("OMF sound descriptors must hold PCM header metadata only"));
            }
            Some("OMFI:MSLT:EditRate" | "OMFI:MDFL:SampleRate") if !exact_rate(entry.data, decode) => {
                return Err(format.unsupported("OMF edit rate cannot be represented exactly"));
            }
            Some("OMFI:TRKD:Origin") if entry.data != [0; 4] => {
                return Err(format.unsupported("OMF timeline slot origins must be zero"));
            }
            Some("OMFI:SCLP:FadeInLength" | "OMFI:SCLP:FadeOutLength" | "OMFI:CMOB:DefFadeLength")
                if entry.data.iter().any(|value| *value != 0) => {
                return Err(format.unsupported("OMF clip fades are outside the cut subset"));
            }
            _ => {}
        }
    }
    Ok(())
}

/// A WAVE or AIFF descriptor can advertise a data chunk with no sample bytes.
fn pcm_header(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    if !(12..=4096).contains(&bytes.len()) {
        return false;
    }
    let wave = match (&bytes[..4], &bytes[8..12]) {
        (b"RIFF", b"WAVE") => true,
        (b"FORM", b"AIFF" | b"AIFC") => false,
        _ => return false,
    };
    let mut chunks = &bytes[12..];
    let mut format_chunk = false;
    while let Some((header, rest)) = chunks.split_first_chunk::<8>() {
        let id = &header[..4];
        if (wave && id == b"data") || (!wave && id == b"SSND") {
            return format_chunk && rest.len() == if wave { 0 } else { 8 };
        }
        let size = [header[4], header[5], header[6], header[7]];
        let size = if wave { u32::from_le_bytes(size) } else { u32::from_be_bytes(size) };
        let Ok(padded) = usize::try_from(u64::from(size) + u64::from(size % 2)) else { return false; };
        let Some(next) = rest.get(padded..) else { return false; };
        format_chunk |= (wave && id == b"fmt ") || (!wave && id == b"COMM");
        chunks = next;
    }
    format_chunk && chunks.is_empty()
}

fn reference_keys<'a>(kind: Option<&&str>, value: &'a [u8]) -> Result<Option<&'a [u8]>> {
    match kind.copied() {
        Some("omfi:ObjRef") if value.len() == 4 => Ok(Some(value)),
        Some("omfi:ObjRefArray") => {
            if value.len().is_multiple_of(4) {
                return Ok(Some(value));
            }
            let ([a, b], keys) = value.split_first_chunk::<2>().ok_or_else(invalid)?;
            let counts = [u16::from_be_bytes([*a, *b]), u16::from_le_bytes([*a, *b])];
            if !counts.into_iter().any(|count| keys.len() == usize::from(count) * 4) { return Err(invalid()); }
            Ok(Some(keys))
        }
        Some("omfi:ObjRef") => Err(invalid()),
        _ => Ok(None),
    }
}

fn mapped_targets(entries: &[Value<'_>]) -> Result<BTreeMap<u32, BTreeMap<u32, u32>>> {
    let mut lists = BTreeMap::new();
    for entry in entries.iter().filter(|entry| entry.key.property == REFERENCE_LIST) {
        let (pairs, rest) = entry.data.as_chunks::<8>();
        if entry.key.kind != REFERENCE_DATA || !rest.is_empty() || lists.contains_key(&entry.key.object) {
            return Err(invalid());
        }
        let mut targets = BTreeMap::new();
        for pair in pairs {
            if targets.insert(word(&pair[..4])?, word(&pair[4..])?).is_some() { return Err(invalid()); }
        }
        lists.insert(entry.key.object, targets);
    }
    Ok(lists)
}

pub(super) fn linked_codec_input(input: &[u8]) -> Result<Vec<u8>> {
    let values = Reader::open(input, Dialect::Standard)?.read()?;
    let names = names(&values)?;
    let big_endian = data_order(&values, &names, TOC_OBJECT)?;
    media_guard(&values, &names, big_endian)?;
    let lists = mapped_targets(&values)?;
    let objects: BTreeSet<_> = values.iter().map(|entry| entry.key.object).collect();
    let mut writer = Writer::new(Dialect::Codec);
    for entry in values {
        if entry.key.property < STANDARD_FIRST_ID && !matches!(entry.key.property, TYPE_NAME | PROPERTY_NAME) {
            continue;
        }
        let kind = names.get(&entry.key.kind);
        if let Some(keys) = reference_keys(kind, entry.data)? {
            let list = lists.get(&entry.reference_list);
            let (reference_keys, _) = keys.as_chunks::<4>();
            let mut data = Vec::new();
            if kind == Some(&"omfi:ObjRefArray") {
                let count = u16::try_from(reference_keys.len()).map_err(|_| invalid())?;
                data.extend_from_slice(&count.to_be_bytes());
            }
            for raw_key in reference_keys {
                let variants = [u32::from_be_bytes(*raw_key), u32::from_le_bytes(*raw_key)];
                let target = if let Some(list) = list {
                    variants.into_iter().find_map(|key| list.get(&key).copied())
                } else if entry.reference_list == 0 {
                    variants.into_iter().find(|key| objects.contains(key))
                } else {
                    None
                }.ok_or_else(invalid)?;
                data.extend_from_slice(&target.to_be_bytes());
            }
            writer.value(entry.key, 0, &data)?;
        } else {
            writer.value(entry.key, 0, entry.data)?;
        }
    }
    writer.finish()
}

pub(super) fn standard_output(input: &[u8]) -> Result<Vec<u8>> {
    let values = Reader::open(input, Dialect::Codec)?.read()?;
    let names = names(&values)?;
    let head = values.iter().find(|entry| names.get(&entry.key.property) == Some(&"OMFI:OOBJ:ObjClass") && entry.data == b"HEAD")
        .map(|entry| entry.key.object).ok_or_else(invalid)?;
    let map_id = |id: u32| -> Result<u32> {
        match id {
            id if id == head => Ok(TOC_OBJECT),
            id if id < CODEC_FIRST_ID => Ok(id),
            id => id.checked_add(STANDARD_FIRST_ID - CODEC_FIRST_ID).ok_or_else(invalid),
        }
    };
    let max_id = values.iter().map(|entry| entry.key.object).max().ok_or_else(invalid)?;
    let mut next_id = map_id(max_id)?.max(STANDARD_FIRST_ID).checked_add(1).ok_or_else(invalid)?;
    let mut writer = Writer::new(Dialect::Standard);
    for entry in values {
        if entry.key.object == TOC_OBJECT || entry.key.property == 22 {
            continue;
        }
        let key = Key { object: map_id(entry.key.object)?, property: map_id(entry.key.property)?, kind: map_id(entry.key.kind)? };
        let mut list = 0;
        if let Some(keys) = reference_keys(names.get(&entry.key.kind), entry.data)?
            && !keys.is_empty()
        {
            list = next_id;
            next_id = next_id.checked_add(1).ok_or_else(invalid)?;
            let mut pairs = Vec::new();
            for bytes in keys.as_chunks::<4>().0.iter().copied().collect::<BTreeSet<_>>() {
                pairs.extend_from_slice(&bytes);
                pairs.extend_from_slice(&map_id(u32::from_be_bytes(bytes))?.to_be_bytes());
            }
            writer.value(Key { object: list, property: REFERENCE_LIST, kind: REFERENCE_DATA }, 0, &pairs)?;
        }
        writer.value(key, list, entry.data)?;
    }
    writer.value(Key { object: TOC_OBJECT, property: 2, kind: TOC_TYPE }, 0, &next_id.to_be_bytes())?;
    writer.value(Key { object: TOC_OBJECT, property: 3, kind: TOC_TYPE }, 0, &STANDARD_FIRST_ID.to_be_bytes())?;
    writer.value(Key { object: TOC_OBJECT, property: 6, kind: TOC_TYPE }, 0, &[0; 4])?;
    writer.finish()
}

struct Writer {
    dialect: Dialect,
    heap: Vec<u8>,
    toc: BTreeMap<Key, (u32, Vec<u8>)>,
}

impl Writer {
    fn new(dialect: Dialect) -> Self {
        Self { dialect, heap: Vec::new(), toc: BTreeMap::new() }
    }

    fn value(&mut self, key: Key, reference_list: u32, data: &[u8]) -> Result<()> {
        let mut field = Vec::new();
        if data.len() <= 4 {
            field.push(9 + data.len() as u8);
            if !data.is_empty() || self.dialect == Dialect::Codec {
                field.extend_from_slice(data);
                field.resize(5, 0);
            }
        } else {
            field = location(self.heap.len(), data.len())?;
            if self.heap.len() + data.len() > super::MAX_BINARY_IMPORT_BYTES * 4 {
                return Err(nw_domain::projects::limit("OMF metadata exceeds the bounded Bento extent"));
            }
            self.heap.extend_from_slice(data);
        }
        if self.toc.insert(key, (reference_list, field)).is_some() {
            return Err(invalid());
        }
        Ok(())
    }

    fn make_toc(&self) -> Vec<u8> {
        let mut toc = Vec::new();
        for (key, (reference_list, field)) in &self.toc {
            let mut entry = vec![1];
            entry.extend_from_slice(&key.object.to_be_bytes());
            if self.dialect == Dialect::Codec { entry.push(2); }
            entry.extend_from_slice(&key.property.to_be_bytes());
            if self.dialect == Dialect::Codec { entry.push(3); }
            entry.extend_from_slice(&key.kind.to_be_bytes());
            if *reference_list != 0 {
                entry.push(15);
                entry.extend_from_slice(&reference_list.to_be_bytes());
            }
            entry.extend_from_slice(field);
            if self.dialect == Dialect::Standard && toc.len() % BLOCK_SIZE + entry.len() >= BLOCK_SIZE {
                toc.resize(toc.len().next_multiple_of(BLOCK_SIZE), 24);
            }
            toc.extend(entry);
        }
        if self.dialect == Dialect::Standard { toc.push(24); }
        toc
    }

    fn finish(mut self) -> Result<Vec<u8>> {
        let start = self.heap.len();
        let toc_key = Key { object: TOC_OBJECT, property: 4, kind: TOC_TYPE };
        let container_key = Key { object: TOC_OBJECT, property: 5, kind: TOC_TYPE };
        let (toc, padding, major) = if self.dialect == Dialect::Standard {
            self.toc.insert(toc_key, (0, location(start, 0)?));
            self.toc.insert(container_key, (0, location(0, 0)?));
            let bytes = self.make_toc().len();
            let aligned = (start + bytes).next_multiple_of(4);
            self.toc.insert(toc_key, (0, location(start, bytes)?));
            self.toc.insert(container_key, (0, location(0, aligned + LABEL_SIZE)?));
            (self.make_toc(), aligned - start - bytes, 2)
        } else {
            (self.make_toc(), 0, 1)
        };
        let pointer = location(start, toc.len())?;
        self.heap.extend(toc);
        self.heap.resize(self.heap.len() + padding, 0);
        self.heap.extend_from_slice(&LABEL);
        self.heap.extend_from_slice(&[0, 0, 0, 1, 0, major, 0, 0]);
        self.heap.extend_from_slice(&pointer[1..]);
        Ok(self.heap)
    }
}

fn location(start: usize, length: usize) -> Result<Vec<u8>> {
    let mut field = vec![5];
    field.extend_from_slice(&u32::try_from(start).map_err(|_| invalid())?.to_be_bytes());
    field.extend_from_slice(&u32::try_from(length).map_err(|_| invalid())?.to_be_bytes());
    Ok(field)
}
