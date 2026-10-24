//! Ordinary file admission precedes every probe. Extensions never grant a demuxer.
use nw_domain::{DomainError, Result};
use nw_platform::files::io_error;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub struct Input {
    pub file: File,
    pub demuxer: &'static str,
}

pub fn open(path: &Path, cancel: &tokio_util::sync::CancellationToken) -> Result<Input> {
    check_cancel(cancel)?;
    let mut file = File::open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(DomainError::new(
            "INPUT_DENIED",
            "A regular source file is required",
        ));
    }
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
    if demuxer == "mov" {
        let length = file.metadata().map_err(io_error)?.len();
        mov_boxes(&mut file, 0, length, 0, &mut 0, cancel)?;
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    Ok(Input { file, demuxer })
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
        _ => Err(DomainError::new(
            "INPUT_ROUTE_REQUIRED",
            "The frozen probe has no admitted ordinary demuxer",
        )),
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
        let path = directory.path().join("renamed.mov");
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
            assert!(
                matches!(open(&path, &tokio_util::sync::CancellationToken::new()), Err(error) if error.code == "INPUT_ROUTE_REQUIRED")
            );
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
