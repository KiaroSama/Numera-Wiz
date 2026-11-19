//! Sample-domain cuts, ordered edits and pitch-preserving tempo changes.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REVERSE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub asset_id: String,
    pub output: String,
    pub stream_index: u32,
    #[schemars(length(min = 1, max = 16))]
    pub pieces: Vec<Piece>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    pub start_sample: u32,
    pub end_sample: u32,
    pub reverse: bool,
    #[schemars(range(min = 10, max = 800))]
    pub tempo_hundredths: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub stream_index: u32,
    pub source_samples: u32,
    pub sample_rate: u32,
    pub channel_layout: String,
    pub channels: u8,
    pub pieces: Vec<Piece>,
}

impl Edit {
    pub fn output_samples(&self) -> Result<(u64, u64)> {
        if !(1..=16).contains(&self.pieces.len())
            || !(8000..=192000).contains(&self.sample_rate)
            || !matches!((self.channels, self.channel_layout.as_str()),
                (1, "mono") | (2, "stereo") | (6, "5.1" | "5.1(side)"))
        {
            return Err(invalid("Select one to sixteen pieces and an admitted PCM channel layout and sample rate"));
        }
        let mut nominal = 0u64;
        let mut tolerance = 0u64;
        let mut reverse_bytes = 0u64;
        let frame_bytes = u64::from(self.channels) * 2;
        for piece in &self.pieces {
            if piece.start_sample >= piece.end_sample || piece.end_sample > self.source_samples
                || !(10..=800).contains(&piece.tempo_hundredths)
            {
                return Err(invalid("Each positive sample interval must fit the verified source and use a tempo from 0.10× to 8×"));
            }
            let length = u64::from(piece.end_sample - piece.start_sample);
            let tempo = u64::from(piece.tempo_hundredths);
            nominal += (length * 100 + tempo / 2) / tempo;
            if piece.reverse {
                reverse_bytes += length * frame_bytes;
            }
            if tempo != 100 {
                if length < u64::from(self.sample_rate) {
                    return Err(invalid("Tempo changes require at least one second of original audio per piece"));
                }
                // FFmpeg's overlap-add filter window can change the exact sample
                // count. Review and verify a bounded deviation rather than pad silently.
                tolerance += u64::from(self.sample_rate) / 20
                    + length * 2 / tempo;
            }
        }
        if reverse_bytes > MAX_REVERSE_BYTES || nominal.saturating_add(tolerance) * frame_bytes > MAX_OUTPUT_BYTES {
            return Err(DomainError::new("RESOURCE_LIMIT", "The selected audio exceeds the reverse memory or output size budget"));
        }
        Ok((nominal, tolerance))
    }
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("VALIDATION_ERROR", message)
}
