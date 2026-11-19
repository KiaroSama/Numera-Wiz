//! Bounded lossless observations on the original decoded PCM sample grid.
use super::identities;
use crate::{Engine, admission, blocking, io_error};
use nw_domain::{
    DomainError, Result, mux::StreamKind, preview::{AudioObservationInput, MAX_INLINE_WAVE_BYTES},
    records::Asset,
};
use nw_platform::{files, process, security};
use std::{io::{Read, Seek}, path::{Path, PathBuf}, time::Duration};
use tokio_util::sync::CancellationToken;

pub struct ObservedWave {
    pub bytes: Vec<u8>,
    pub frames: u32,
    pub graph_sha256: String,
}

fn invalid_wave() -> DomainError {
    DomainError::new("VERIFICATION_FAILED", "The observed WAVE does not contain the exact original PCM interval")
}

fn verify_wave(bytes: &[u8], rate: u32, channels: u8, frames: u32) -> Result<()> {
    // The bit-exact FFmpeg WAVE profile emits a 16-byte PCM format chunk
    // followed by a single data chunk. Check the complete bounded result.
    let riff_u16 = |start| u16::from_le_bytes(bytes[start..start+2].try_into().expect("Bounded WAVE field"));
    let riff_u32 = |start| u32::from_le_bytes(bytes[start..start+4].try_into().expect("Bounded WAVE field"));
    let frame_bytes = u32::from(channels) * 2;
    if bytes.len() < 44 || bytes.len() > MAX_INLINE_WAVE_BYTES
        || &bytes[..4] != b"RIFF" || riff_u32(4) as usize != bytes.len() - 8
        || &bytes[8..16] != b"WAVEfmt " || riff_u32(16) != 16 || riff_u16(20) != 1
        || riff_u16(22) != u16::from(channels) || riff_u32(24) != rate
        || riff_u32(28) != rate * frame_bytes || u32::from(riff_u16(32)) != frame_bytes
        || riff_u16(34) != 16 || &bytes[36..40] != b"data"
        || riff_u32(40) != frames * frame_bytes || riff_u32(40) as usize != bytes.len() - 44
    {
        return Err(invalid_wave());
    }
    Ok(())
}

impl Engine {
    /// Render an exact, verified PCM interval without changing rate or layout.
    pub async fn observe_pcm_wave(
        &self,
        asset: &Asset,
        source: files::SourceStamp,
        request: &AudioObservationInput,
        scratch: PathBuf,
        cancel: CancellationToken,
    ) -> Result<ObservedWave> {
        let frames = request.frames()?;
        let index = request.target.stream_index;
        let (rate, channels, layout) = crate::audio_edit::format(&asset.probe, index)?;
        if rate != request.sample_rate || layout != request.channel_layout.name() {
            return Err(DomainError::new("VALIDATION_ERROR", "The requested rate and layout must match the original PCM stream"));
        }
        let admission::RegisteredInput { input, source, expected, length } =
            admission::registered_stream(asset, source, index, StreamKind::Audio, cancel.clone()).await?;
        let output = blocking(move || {
            let root = security::private_directory(&scratch)?;
            tempfile::Builder::new().prefix("observe-audio-").suffix(".wav").tempfile_in(root).map_err(io_error)
        }).await?;
        let range = request.range;
        let mut args = ["-hide_banner","-nostdin","-v","error","-xerror","-max_alloc","67108864",
            "-threads","1","-filter_threads","1","-y"].map(str::to_owned).to_vec();
        args.extend(input.options());
        args.extend([
            "-i".into(),"fd:".into(),"-map".into(),format!("0:{index}"),"-af".into(),
            format!("atrim=start_sample={}:end_sample={},asettb=1/sr,asetpts=N",range.start,range.end),
        ]);
        args.extend(["-c:a","pcm_s16le","-map_metadata","-1","-map_chapters","-1",
            "-fflags","+bitexact","-rf64","never","-f","wav","-fs"].map(str::to_owned));
        args.push(MAX_INLINE_WAVE_BYTES.to_string());
        let graph = serde_json::to_vec(&args).map_err(|_| {
            DomainError::new("SERIALIZATION_FAILED", "Audio observation graph identity is unavailable")
        })?;
        args.push(files::path_text(output.path())?);
        let ticket = self.process_budget.acquire(&cancel).await?;
        if let Err(error) = identities(self, cancel.clone()).await.and_then(|_| source.revalidate()) {
            return ticket.finish(Err(error));
        }
        let result = process::run_readonly(
            Path::new(&self.ffmpeg.path), &args, Duration::from_secs(7), 0,
            input.file, cancel.clone(),
        ).await;
        ticket.finish(result)?;
        identities(self, cancel.clone()).await?;
        let bytes = blocking(move || {
            if source.identity_with_limit(&cancel, length)? != expected {
                return Err(DomainError::new("SOURCE_CHANGED", "Registered audio changed during observation"));
            }
            let mut output = output;
            output.as_file_mut().rewind().map_err(io_error)?;
            let mut bytes = Vec::new();
            output.as_file_mut().take(MAX_INLINE_WAVE_BYTES as u64 + 1)
                .read_to_end(&mut bytes).map_err(io_error)?;
            verify_wave(&bytes, rate, channels, frames)?;
            Ok(bytes)
        }).await?;
        Ok(ObservedWave { bytes, frames, graph_sha256: nw_domain::projects::hash(&graph) })
    }
}
