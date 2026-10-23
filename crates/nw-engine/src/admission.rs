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

pub fn open(path: &Path) -> Result<Input> {
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
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    Ok(Input { file, demuxer })
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
        let mut args = [
            "-protocol_whitelist",
            "fd",
            "-format_whitelist",
            self.demuxer,
            "-f",
            self.demuxer,
            "-fd",
            "0",
            "-blocksize",
            "65536",
        ]
        .map(str::to_owned)
        .to_vec();
        if self.demuxer == "mov" {
            args.extend(["-enable_drefs", "0", "-use_absolute_path", "0"].map(str::to_owned));
        }
        args
    }
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
}
