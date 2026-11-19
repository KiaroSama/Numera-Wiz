//! Ordinary file admission precedes every probe. Extensions never grant a demuxer.
use nw_domain::{
    DomainError, Result,
    mux::StreamKind,
    records::{Asset, FileIdentity},
};
use nw_platform::files::{SourceStamp, io_error};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
};
use tokio_util::sync::CancellationToken;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAX_STILL_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MAX_STILL_PIXELS: &str = "12000000";

pub struct Input {
    pub file: File,
    pub demuxer: &'static str,
}

pub(super) struct RegisteredInput {
    pub input: Input,
    pub source: SourceStamp,
    pub expected: FileIdentity,
    pub length: u64,
}

pub(super) async fn registered_stream(
    asset: &Asset,
    source: SourceStamp,
    stream_index: u32,
    kind: StreamKind,
    cancel: CancellationToken,
) -> Result<RegisteredInput> {
    if !asset.probe.streams.iter().any(|stream| {
        stream.index == stream_index && stream.kind == kind && !stream.attached_picture()
    }) {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "Selected stream differs from its registered role",
        ));
    }
    let expected = asset.identity.clone();
    let object = asset.source_object.clone().ok_or_else(|| {
        DomainError::new("INPUT_DENIED", "Registered source lacks a native identity")
    })?;
    let demuxer = frozen_demuxer(&asset.probe.format)?;
    let length = expected.bytes.parse::<u64>().map_err(|_| {
        DomainError::new(
            "INVALID_IDENTITY",
            "Registered source has an invalid byte count",
        )
    })?;
    crate::blocking(move || {
        source.require_object(&object)?;
        if source.byte_len()? != length || source.identity_with_limit(&cancel, length)? != expected
        {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Registered source content changed",
            ));
        }
        let input = from_file(source.try_clone_file()?, &cancel)?;
        if input.demuxer != demuxer {
            return Err(DomainError::new(
                "SOURCE_CHANGED",
                "Registered source format changed",
            ));
        }
        Ok(RegisteredInput {
            input,
            source,
            expected,
            length,
        })
    })
    .await
}

pub fn open(path: &Path, cancel: &tokio_util::sync::CancellationToken) -> Result<Input> {
    check_cancel(cancel)?;
    from_file(nw_platform::files::open_regular_read(path)?, cancel)
}
/// Admit the already authorized source handle without reopening its native pathname.
pub fn from_file(mut file: File, cancel: &tokio_util::sync::CancellationToken) -> Result<Input> {
    check_cancel(cancel)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular source file is required",
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut prefix = [0u8; 16];
    let mut length = 0;
    while length < prefix.len() {
        let count = file.read(&mut prefix[length..]).map_err(io_error)?;
        if count == 0 {
            break;
        }
        length += count;
    }
    let demuxer = classify(&prefix[..length])?;
    let length = metadata.len();
    if still_codec(demuxer).is_some() && length > MAX_STILL_BYTES {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "Single-file still admission supports up to 32 MiB of source bytes",
        ));
    }
    match demuxer {
        "mov" => mov_boxes(&mut file, 0, length, 0, &mut 0, cancel)?,
        "png_pipe" => png_chunks(&mut file, length, cancel)?,
        "jpeg_pipe" => jpeg_markers(&mut file, length, cancel)?,
        _ => {}
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    Ok(Input { file, demuxer })
}

// png_pipe can decode the default picture of an APNG. Require a complete
// static PNG envelope before exposing the descriptor, then let FFmpeg decode it.
fn png_chunks(file: &mut File, length: u64, cancel: &CancellationToken) -> Result<()> {
    let invalid = || DomainError::new("INVALID_PROBE", "Invalid single-file PNG chunk layout");
    let mut start = PNG_SIGNATURE.len() as u64;
    let mut image_data = false;
    for index in 0..100_000 {
        check_cancel(cancel)?;
        if length.saturating_sub(start) < 12 {
            return Err(invalid());
        }
        let mut header = [0; 8];
        file.seek(SeekFrom::Start(start)).map_err(io_error)?;
        file.read_exact(&mut header).map_err(io_error)?;
        let bytes = u64::from(u32::from_be_bytes(
            header[..4].try_into().expect("Four-byte PNG chunk length"),
        ));
        let kind = &header[4..];
        if index == 0 && (kind != b"IHDR" || bytes != 13) {
            return Err(invalid());
        }
        start += 12 + bytes;
        if start > length {
            return Err(invalid());
        }
        match kind {
            b"acTL" | b"fcTL" | b"fdAT" => {
                return Err(DomainError::new(
                    "INPUT_ROUTE_REQUIRED",
                    "Animated PNG requires a separate input route",
                ));
            }
            b"IHDR" if index != 0 => return Err(invalid()),
            b"IDAT" => image_data = true,
            b"IEND" if bytes == 0 && start == length && image_data => return Ok(()),
            b"IEND" => return Err(invalid()),
            _ => {}
        }
    }
    Err(DomainError::new(
        "RESOURCE_LIMIT",
        "Still admission exceeded 100000 PNG chunks",
    ))
}

// The MJPEG decoder can synthesize a missing EOI or ignore trailing bytes.
// Walk the JPEG envelope to require the first top-level EOI at EOF. Segment
// payloads can hold embedded thumbnails; their bytes never form outer markers.
fn jpeg_markers(file: &mut File, length: u64, cancel: &CancellationToken) -> Result<()> {
    file.seek(SeekFrom::Start(2)).map_err(io_error)?;
    // Include one byte beyond the measured envelope to detect a growing file.
    let mut source = BufReader::new(file.take(length.saturating_sub(1)));
    let mut scanning = false;
    let mut saw_scan = false;
    loop {
        check_cancel(cancel)?;
        if scanning {
            loop {
                check_cancel(cancel)?;
                let bytes = source.fill_buf().map_err(io_error)?;
                if bytes.is_empty() {
                    return Err(invalid_jpeg());
                }
                let marker = bytes.iter().position(|byte| *byte == 0xff);
                let skipped = marker.unwrap_or(bytes.len());
                source.consume(skipped);
                if marker.is_some() {
                    break;
                }
            }
        }
        if jpeg_byte(&mut source)? != 0xff {
            return Err(invalid_jpeg());
        }
        let mut marker = jpeg_byte(&mut source)?;
        while marker == 0xff {
            check_cancel(cancel)?;
            marker = jpeg_byte(&mut source)?;
        }
        match marker {
            0x00 | 0xd0..=0xd7 if scanning => continue,
            0x01 => continue,
            0xd9 if saw_scan && source.fill_buf().map_err(io_error)?.is_empty() => return Ok(()),
            0xc0..=0xcf | 0xda..=0xfe => {
                let mut size = [0; 2];
                source.read_exact(&mut size).map_err(|_| invalid_jpeg())?;
                let payload = u16::from_be_bytes(size)
                    .checked_sub(2)
                    .ok_or_else(invalid_jpeg)?;
                let payload = u64::from(payload);
                let skipped = std::io::copy(
                    &mut source.by_ref().take(payload),
                    &mut std::io::sink(),
                )
                .map_err(io_error)?;
                if skipped != payload {
                    return Err(invalid_jpeg());
                }
                scanning = marker == 0xda;
                saw_scan |= scanning;
            }
            _ => return Err(invalid_jpeg()),
        }
    }
}

fn jpeg_byte(source: &mut impl Read) -> Result<u8> {
    let mut value = [0];
    source.read_exact(&mut value).map_err(|_| invalid_jpeg())?;
    Ok(value[0])
}

fn invalid_jpeg() -> DomainError {
    DomainError::new("INVALID_PROBE", "Invalid single-file JPEG marker layout")
}

fn check_cancel(cancel: &tokio_util::sync::CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(DomainError::new("CANCELLED", "Input admission cancelled"));
    }
    Ok(())
}

fn reference_route() -> DomainError {
    DomainError::new(
        "INPUT_ROUTE_REQUIRED",
        "External or compressed MOV references require an explicit dependency resolver",
    )
}

fn malformed() -> DomainError {
    DomainError::new("INVALID_PROBE", "Invalid or truncated MOV box layout")
}

fn read_at<const N: usize>(file: &mut File, offset: u64) -> Result<[u8; N]> {
    file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
    let mut bytes = [0u8; N];
    file.read_exact(&mut bytes).map_err(|_| malformed())?;
    Ok(bytes)
}

fn box_header(file: &mut File, start: u64, end: u64) -> Result<([u8; 4], u64, u64)> {
    if end.saturating_sub(start) < 8 {
        return Err(malformed());
    }
    let header = read_at::<8>(file, start)?;
    let size = u32::from_be_bytes(header[..4].try_into().expect("Four-byte size"));
    let (bytes, header_bytes) = match size {
        0 => (end - start, 8),
        1 => {
            if end - start < 16 {
                return Err(malformed());
            }
            (u64::from_be_bytes(read_at::<8>(file, start + 8)?), 16)
        }
        value => (u64::from(value), 8),
    };
    if bytes < header_bytes {
        return Err(malformed());
    }
    let next = start
        .checked_add(bytes)
        .filter(|next| *next <= end)
        .ok_or_else(malformed)?;
    Ok((
        header[4..8].try_into().expect("Four-byte box type"),
        start + header_bytes,
        next,
    ))
}

fn mov_boxes(
    file: &mut File,
    mut start: u64,
    end: u64,
    depth: usize,
    count: &mut u32,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<()> {
    if depth > 32 {
        return Err(DomainError::new(
            "RESOURCE_LIMIT",
            "MOV admission nesting exceeded 32",
        ));
    }
    while start < end {
        check_cancel(cancel)?;
        *count = count.checked_add(1).ok_or_else(malformed)?;
        if *count > 100000 {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "MOV admission exceeded 100000 boxes",
            ));
        }
        let (kind, mut content, next) = box_header(file, start, end)?;
        match &kind {
            b"cmov" | b"rmra" | b"rmda" | b"rdrf" => return Err(reference_route()),
            b"dref" => {
                if next - content < 8 {
                    return Err(malformed());
                }
                let header = read_at::<8>(file, content)?;
                if header[..4] != [0; 4] {
                    return Err(reference_route());
                }
                let entries =
                    u32::from_be_bytes(header[4..].try_into().expect("Four-byte reference count"));
                if entries == 0 || entries > 4096 {
                    return Err(DomainError::new(
                        "RESOURCE_LIMIT",
                        "MOV data-reference count must be 1-4096",
                    ));
                }
                content += 8;
                *count = count
                    .checked_add(entries)
                    .filter(|count| *count <= 100000)
                    .ok_or_else(|| {
                        DomainError::new(
                            "RESOURCE_LIMIT",
                            "MOV admission exceeded 100000 boxes/references",
                        )
                    })?;
                for _ in 0..entries {
                    check_cancel(cancel)?;
                    let (entry, flags, entry_end) = box_header(file, content, next)?;
                    if entry_end - flags != 4
                        || !matches!(&entry, b"url " | b"alis")
                        || read_at::<4>(file, flags)? != [0, 0, 0, 1]
                    {
                        return Err(reference_route());
                    }
                    content = entry_end;
                }
                if content != next {
                    return Err(malformed());
                }
            }
            b"moov" | b"trak" | b"mdia" | b"minf" | b"dinf" | b"edts" | b"mvex" | b"stbl"
            | b"moof" | b"traf" | b"tref" | b"udta" | b"sinf" | b"schi" => {
                mov_boxes(file, content, next, depth + 1, count, cancel)?
            }
            b"meta" => {
                if next - content < 8 {
                    return Err(malformed());
                }
                if read_at::<4>(file, content)? == [0; 4] {
                    content += 4;
                }
                mov_boxes(file, content, next, depth + 1, count, cancel)?;
            }
            _ => {}
        }
        start = next;
    }
    Ok(())
}

fn classify(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(PNG_SIGNATURE) {
        return Ok("png_pipe");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok("jpeg_pipe");
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Ok("matroska");
    }
    if bytes.starts_with(b"fLaC") {
        return Ok("flac");
    }
    if bytes.starts_with(b"OggS") {
        return Ok("ogg");
    }
    if bytes.len() >= 12 && matches!(&bytes[..4], b"RIFF" | b"RF64") {
        match &bytes[8..12] {
            b"WAVE" => return Ok("wav"),
            b"AVI " if bytes.starts_with(b"RIFF") => return Ok("avi"),
            _ => {}
        }
    }
    if bytes.len() >= 8
        && matches!(
            &bytes[4..8],
            b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip"
        )
    {
        return Ok("mov");
    }
    if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe6 == 0xe2)
    {
        return Ok("mp3");
    }
    Err(DomainError::new(
        "INPUT_ROUTE_REQUIRED",
        "No ordinary self-contained file route matches this content; reference manifests and sequences require an explicit resolver",
    ))
}

pub fn frozen_demuxer(format: &str) -> Result<&'static str> {
    match format {
        "matroska,webm" | "matroska" | "webm" => Ok("matroska"),
        "mov,mp4,m4a,3gp,3g2,mj2" | "mov" => Ok("mov"),
        "avi" => Ok("avi"),
        "wav" => Ok("wav"),
        "flac" => Ok("flac"),
        "mp3" => Ok("mp3"),
        "ogg" => Ok("ogg"),
        "png_pipe" => Ok("png_pipe"),
        "jpeg_pipe" => Ok("jpeg_pipe"),
        _ => Err(DomainError::new(
            "INPUT_ROUTE_REQUIRED",
            "The frozen probe has no admitted ordinary demuxer",
        )),
    }
}

pub(super) fn still_codec(demuxer: &str) -> Option<&'static str> {
    match demuxer {
        "png_pipe" => Some("png"),
        "jpeg_pipe" => Some("mjpeg"),
        _ => None,
    }
}

impl Input {
    pub fn options(&self) -> Vec<String> {
        options(self.demuxer)
    }
}
pub fn options(demuxer: &str) -> Vec<String> {
    let mut args = [
        "-protocol_whitelist",
        "fd",
        "-format_whitelist",
        demuxer,
        "-f",
        demuxer,
        "-fd",
        "0",
        "-blocksize",
        "65536",
    ]
    .map(str::to_owned)
    .to_vec();
    if demuxer == "mov" {
        args.extend(["-enable_drefs", "0", "-use_absolute_path", "0"].map(str::to_owned));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renamed_reference_documents_have_no_ordinary_file_route() {
        for bytes in [
            b"#EXTM3U\nhttp://127.0.0.1/private".as_slice(),
            b"ffconcat version 1.0\nfile '/private/secret.mkv'",
            b"<MPD><BaseURL>private</BaseURL></MPD>",
        ] {
            assert_eq!(classify(bytes).unwrap_err().code, "INPUT_ROUTE_REQUIRED");
        }
        assert_eq!(classify(b"\0\0\0\x18ftypisom").unwrap(), "mov");
        assert_eq!(classify(b"RIFF\0\0\0\0AVI ").unwrap(), "avi");
        assert_eq!(classify(b"\x1aE\xdf\xa3").unwrap(), "matroska");
    }
    #[test]
    fn external_mov_references_are_rejected_before_any_probe() {
        fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut bytes = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
            bytes.extend(kind);
            bytes.extend(payload);
            bytes
        }
        let directory = tempfile::tempdir().unwrap();
        // The composition root resolves native aliases before invoking canonical-only admission.
        let path = std::fs::canonicalize(directory.path())
            .unwrap()
            .join("renamed.mov");
        for target in [
            b"../ungranted/sibling.mov".as_slice(),
            b"/private/absolute.mov",
            b"http://127.0.0.1:9/private",
        ] {
            let mut entry = vec![0; 4];
            entry.extend(target);
            entry.push(0);
            let mut dref = vec![0; 4];
            dref.extend(1u32.to_be_bytes());
            dref.extend(atom(b"url ", &entry));
            let mut content = atom(b"ftyp", b"isom\0\0\0\0isom");
            content.extend(atom(b"moov", &atom(b"dinf", &atom(b"dref", &dref))));
            std::fs::write(&path, content).unwrap();
            assert_eq!(nw_platform::files::source_path(&path).unwrap(), path);
            let error = open(&path, &tokio_util::sync::CancellationToken::new())
                .err()
                .expect("External MOV references must fail before probing");
            assert_eq!(error.code, "INPUT_ROUTE_REQUIRED", "{error}");
        }
        let mut dref = vec![0; 4];
        dref.extend(1u32.to_be_bytes());
        dref.extend(atom(b"url ", &[0, 0, 0, 1]));
        let mut content = atom(b"ftyp", b"isom\0\0\0\0isom");
        content.extend(atom(b"moov", &atom(b"dinf", &atom(b"dref", &dref))));
        std::fs::write(&path, &content).unwrap();
        assert_eq!(
            open(&path, &tokio_util::sync::CancellationToken::new())
                .unwrap()
                .demuxer,
            "mov"
        );
        content.pop();
        std::fs::write(&path, content).unwrap();
        assert!(
            matches!(open(&path, &tokio_util::sync::CancellationToken::new()), Err(error) if error.code == "INVALID_PROBE")
        );
    }
}
