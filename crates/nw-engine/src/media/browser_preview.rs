//! Bound a generated fragmented MP4 and expose its appendable browser units.
mod stream;
use super::identities;
use crate::{Engine, admission, blocking, preservation};
use nw_domain::{
    DomainError, Result,
    mux::StreamKind,
    preview::{MAX_SOURCE_SEGMENT_BYTES, MAX_SOURCE_SEGMENT_FRAGMENTS},
    records::Asset,
};
use nw_platform::{files::SourceStamp, process};
use std::{ops::Range, path::Path, time::Duration};
pub use stream::{BrowserMp4Part, browser_mp4_stream};
use tokio_util::sync::CancellationToken;

const MAX_INIT_BYTES: usize = 64 * 1024;
const MAX_FRAGMENT_HEADERS: usize = 64 * 1024;
const VIDEO_FILTER: &str = concat!(
    "trim=start=0:end=2,scale=320:180:force_original_aspect_ratio=decrease:",
    "force_divisible_by=2:flags=lanczos:in_color_matrix=bt709:out_color_matrix=bt709:",
    "in_range=limited:out_range=limited,",
    "pad=320:180:(ow-iw)/2:(oh-ih)/2:black,setsar=1,",
    "setparams=range=limited:color_primaries=bt709:color_trc=bt709:colorspace=bt709",
);

pub struct BrowserSegment {
    pub bytes: Vec<u8>,
    pub initialization: Range<usize>,
    pub fragments: Vec<Range<usize>>,
    pub mime_type: String,
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_BROWSER_PREVIEW", message)
}

struct BoxRef<'a> {
    kind: [u8; 4],
    payload: &'a [u8],
}

fn take_box<'a>(remaining: &mut &'a [u8]) -> Result<Option<BoxRef<'a>>> {
    let source = *remaining;
    if source.is_empty() {
        return Ok(None);
    }
    let Some(header) = source.first_chunk::<8>() else {
        return Err(invalid("Truncated MP4 box header"));
    };
    let size = u32::from_be_bytes(header[..4].try_into().expect("MP4 box size"));
    let (header_bytes, size) = match size {
        0 => {
            return Err(invalid(
                "Open-ended MP4 boxes are outside the preview profile",
            ));
        }
        1 => {
            let Some(header) = source.first_chunk::<16>() else {
                return Err(invalid("Truncated extended MP4 box header"));
            };
            let size = u64::from_be_bytes(header[8..].try_into().expect("Extended MP4 box size"));
            (
                16,
                usize::try_from(size).map_err(|_| invalid("Unrepresentable MP4 box size"))?,
            )
        }
        size => (8, size as usize),
    };
    if size < header_bytes || size > source.len() {
        return Err(invalid("MP4 box exceeds its enclosing bytes"));
    }
    let kind = header[4..8].try_into().expect("MP4 box type");
    let (data, rest) = source.split_at(size);
    *remaining = rest;
    Ok(Some(BoxRef {
        kind,
        payload: &data[header_bytes..],
    }))
}

fn descriptor(mut source: &[u8], expected: u8) -> Result<&[u8]> {
    let Some((&id, tail)) = source.split_first() else {
        return Err(invalid("Missing MP4 audio descriptor"));
    };
    if id != expected {
        return Err(invalid("Unexpected MP4 audio descriptor"));
    }
    source = tail;
    let mut bytes = 0usize;
    for _ in 0..4 {
        let Some((&size, tail)) = source.split_first() else {
            return Err(invalid("Truncated MP4 audio descriptor size"));
        };
        source = tail;
        bytes = bytes * 128 + usize::from(size & 0x7f);
        if size < 128 {
            return source
                .get(..bytes)
                .ok_or_else(|| invalid("Truncated MP4 audio descriptor"));
        }
    }
    Err(invalid("MP4 audio descriptor size exceeds four bytes"))
}

fn verify_aac(track: &[u8], entry: &[u8]) -> Result<()> {
    let extensions = entry
        .get(28..)
        .ok_or_else(|| invalid("Incomplete AAC sample entry"))?;
    let esds = child(extensions, *b"esds")?;
    let es = descriptor(
        esds.strip_prefix(&[0, 0, 0, 0])
            .ok_or_else(|| invalid("Unsupported AAC descriptor version"))?,
        0x03,
    )?;
    if es.get(2) != Some(&0) {
        return Err(invalid("Unexpected AAC descriptor flags"));
    }
    let decoder = descriptor(
        es.get(3..)
            .ok_or_else(|| invalid("Incomplete AAC elementary stream"))?,
        0x04,
    )?;
    if !decoder.starts_with(&[0x40, 0x15]) {
        return Err(invalid("Preview sound requires AAC"));
    }
    let config = descriptor(
        decoder
            .get(13..)
            .ok_or_else(|| invalid("Incomplete AAC decoder configuration"))?,
        0x05,
    )?;
    if !config.starts_with(&[0x11, 0x90]) {
        return Err(invalid("Preview sound requires 48 kHz stereo AAC-LC"));
    }
    let edit = child(child(track, *b"edts")?, *b"elst")?;
    if edit != [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 4, 0, 0, 1, 0, 0] {
        return Err(invalid("Preview audio lacks its exact AAC priming edit"));
    }
    Ok(())
}

fn child(mut siblings: &[u8], kind: [u8; 4]) -> Result<&[u8]> {
    let mut selected = None;
    while let Some(node) = take_box(&mut siblings)? {
        if node.kind == kind && selected.replace(node.payload).is_some() {
            return Err(invalid("Duplicate preview MP4 box"));
        }
    }
    selected.ok_or_else(|| invalid("Missing preview MP4 box"))
}

fn codec_list(mut movie: &[u8], sound_expected: bool) -> Result<String> {
    let mut video = None;
    let mut audio = false;
    while let Some(node) = take_box(&mut movie)? {
        if node.kind != *b"trak" {
            continue;
        }
        let media = child(node.payload, *b"mdia")?;
        let info = child(media, *b"minf")?;
        let table = child(info, *b"stbl")?;
        let description = child(table, *b"stsd")?;
        let Some((header, mut entries)) = description.split_first_chunk::<8>() else {
            return Err(invalid("Missing MP4 sample description"));
        };
        if header != &[0, 0, 0, 0, 0, 0, 0, 1] {
            return Err(invalid("Preview MP4 must have one sample entry per track"));
        }
        let sample = take_box(&mut entries)?.ok_or_else(|| invalid("Missing MP4 sample entry"))?;
        if !entries.is_empty() {
            return Err(invalid("Unexpected bytes after MP4 sample entry"));
        }
        match &sample.kind {
            b"avc1" if video.is_none() => {
                let extensions = sample
                    .payload
                    .get(78..)
                    .ok_or_else(|| invalid("Incomplete AVC sample entry"))?;
                let avc = child(extensions, *b"avcC")?;
                let config = avc
                    .first_chunk::<7>()
                    .ok_or_else(|| invalid("Incomplete AVC configuration"))?;
                if config[0] != 1 || config[1] != 66 || config[3] > 31 {
                    return Err(invalid("Preview MP4 requires bounded AVC Baseline video"));
                }
                video = Some(format!(
                    "avc1.{:02X}{:02X}{:02X}",
                    config[1], config[2], config[3]
                ));
            }
            b"mp4a" if sound_expected && !audio => {
                verify_aac(node.payload, sample.payload)?;
                audio = true;
            }
            _ => return Err(invalid("Unexpected track in preview MP4")),
        }
    }
    let video = video.ok_or_else(|| invalid("Preview MP4 has no admitted video track"))?;
    if audio != sound_expected {
        return Err(invalid("Preview MP4 audio does not match the capture"));
    }
    let suffix = if audio { ", mp4a.40.2" } else { "" };
    Ok(format!("video/mp4; codecs=\"{video}{suffix}\""))
}

fn verify_initialization(
    file: &BoxRef<'_>,
    movie: &BoxRef<'_>,
    sound_expected: bool,
) -> Result<String> {
    if file.kind != *b"ftyp" || file.payload.len() < 8 || movie.kind != *b"moov" {
        return Err(invalid("Preview MP4 initialization requires ftyp and moov"));
    }
    child(movie.payload, *b"mvex")?;
    codec_list(movie.payload, sound_expected)
}

fn verify_fragment(fragment: &BoxRef<'_>, media: &BoxRef<'_>, ordinal: u32) -> Result<()> {
    if fragment.kind != *b"moof"
        || fragment.payload.len() > MAX_FRAGMENT_HEADERS
        || media.kind != *b"mdat"
        || media.payload.is_empty()
    {
        return Err(invalid("Unexpected preview MP4 fragment layout"));
    }
    let sequence = child(fragment.payload, *b"mfhd")?;
    if sequence.len() != 8
        || sequence[..4] != [0, 0, 0, 0]
        || u32::from_be_bytes(sequence[4..].try_into().expect("MP4 fragment sequence")) != ordinal
    {
        return Err(invalid("Preview MP4 fragment sequence differs"));
    }
    Ok(())
}

impl BrowserSegment {
    pub fn verify(bytes: Vec<u8>, sound_expected: bool) -> Result<Self> {
        if bytes.len() > MAX_SOURCE_SEGMENT_BYTES {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Browser preview segment exceeded 2 MiB",
            ));
        }
        let mut remaining = bytes.as_slice();
        let file = take_box(&mut remaining)?.ok_or_else(|| invalid("Missing MP4 file type"))?;
        let movie = take_box(&mut remaining)?.ok_or_else(|| invalid("Missing MP4 movie header"))?;
        let initialization = 0..bytes.len() - remaining.len();
        if initialization.len() > MAX_INIT_BYTES {
            return Err(invalid("Preview MP4 initialization exceeds 64 KiB"));
        }
        let mime_type = verify_initialization(&file, &movie, sound_expected)?;
        let mut fragments = Vec::new();
        while !remaining.is_empty() {
            let start = bytes.len() - remaining.len();
            let fragment =
                take_box(&mut remaining)?.ok_or_else(|| invalid("Missing MP4 fragment header"))?;
            let media =
                take_box(&mut remaining)?.ok_or_else(|| invalid("Missing MP4 fragment payload"))?;
            if fragments.len() >= MAX_SOURCE_SEGMENT_FRAGMENTS {
                return Err(invalid("Unexpected preview MP4 fragment layout"));
            }
            verify_fragment(&fragment, &media, fragments.len() as u32 + 1)?;
            fragments.push(start..bytes.len() - remaining.len());
        }
        if fragments.is_empty() {
            return Err(invalid("Preview MP4 has no media fragments"));
        }
        Ok(Self {
            bytes,
            initialization,
            fragments,
            mime_type,
        })
    }
}

pub(super) fn append_browser_audio(args: &mut Vec<String>) {
    args.extend(
        [
            "-c:a",
            "aac",
            "-profile:a",
            "aac_low",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-b:a",
            "128k",
        ]
        .map(str::to_owned),
    );
}

pub(super) fn append_browser_output(args: &mut Vec<String>) {
    args.extend(
        [
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-tune",
            "zerolatency",
            "-profile:v",
            "baseline",
            "-level:v",
            "3.1",
            "-pix_fmt",
            "yuv420p",
            "-threads:v",
            "1",
            "-g",
            "60",
            "-bf",
            "0",
            "-sc_threshold",
            "0",
            "-force_key_frames",
            "expr:gte(t,n_forced*0.5)",
            "-fps_mode:v",
            "passthrough",
            "-enc_time_base:v",
            "filter",
            "-avoid_negative_ts",
            "disabled",
            "-use_editlist",
            "1",
            "-movflags",
            "+frag_keyframe+delay_moov+default_base_moof+skip_trailer",
            "-f",
            "mp4",
            "pipe:1",
        ]
        .map(str::to_owned),
    );
}
impl Engine {
    /// Encode the first two seconds of an explicitly selected SDR source.
    pub async fn browser_source_segment(
        &self,
        asset: &Asset,
        source: SourceStamp,
        video_stream: u32,
        audio_stream: Option<u32>,
        cancel: CancellationToken,
    ) -> Result<(BrowserSegment, SourceStamp)> {
        let unsupported = || {
            DomainError::new(
                "BROWSER_PREVIEW_UNAVAILABLE",
                "This preview profile needs zero-origin limited BT.709 video and optional zero-origin stereo 48 kHz audio",
            )
        };
        let video = asset
            .probe
            .stream_details
            .get(&video_stream)
            .ok_or_else(unsupported)?;
        if ["color_space", "color_transfer", "color_primaries"]
            .iter()
            .any(|field| video[*field] != "bt709")
            || video["color_range"] != "tv"
            || preservation::integer(&video["start_pts"])? != Some(0)
        {
            return Err(unsupported());
        }
        if let Some(index) = audio_stream {
            let detail = asset
                .probe
                .stream_details
                .get(&index)
                .ok_or_else(unsupported)?;
            if !asset
                .probe
                .streams
                .iter()
                .any(|stream| stream.index == index && stream.kind == StreamKind::Audio)
                || preservation::integer(&detail["sample_rate"])? != Some(48000)
                || detail["channels"] != 2
                || detail["channel_layout"] != "stereo"
                || preservation::integer(&detail["start_pts"])? != Some(0)
            {
                return Err(unsupported());
            }
        }
        let admission::RegisteredInput {
            input,
            source,
            expected,
            length,
        } = admission::registered_stream(
            asset,
            source,
            video_stream,
            StreamKind::Video,
            cancel.clone(),
        )
        .await?;
        let mut args = [
            "-hide_banner",
            "-nostdin",
            "-v",
            "warning",
            "-xerror",
            "-copyts",
            "-max_alloc",
            "67108864",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-drop_changed",
            "1",
        ]
        .map(str::to_owned)
        .to_vec();
        args.extend(input.options());
        args.extend([
            "-i".into(),
            "fd:".into(),
            "-map".into(),
            format!("0:{video_stream}"),
        ]);
        if let Some(index) = audio_stream {
            args.extend(["-map".into(), format!("0:{index}")]);
            args.extend(["-af", "atrim=start=0:end=2"].map(str::to_owned));
            append_browser_audio(&mut args);
        }
        args.extend(["-t", "2", "-vf", VIDEO_FILTER].map(str::to_owned));
        append_browser_output(&mut args);
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone())
            .await
            .and_then(|_| source.revalidate())
        {
            return ticket.finish(Err(error));
        }
        let process = process::run_readonly(
            Path::new(&self.ffmpeg.path),
            &args,
            Duration::from_secs(12),
            MAX_SOURCE_SEGMENT_BYTES,
            input.file,
            cancel.clone(),
        )
        .await;
        let output = ticket.finish(process)?;
        if output.diagnostic_bytes > 4096
            || output
                .stderr_tail
                .contains("Avoiding reinit; dropping frame")
        {
            return Err(invalid("The preview source format changed while decoding"));
        }
        let verified = BrowserSegment::verify(output.stdout, audio_stream.is_some())?;
        identities(self, cancel.clone()).await?;
        blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new(
                    "SOURCE_CHANGED",
                    "Registered source changed during preview capture",
                ));
            }
            Ok((verified, source))
        })
        .await
    }
}
