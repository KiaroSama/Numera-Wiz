//! A two-pass linear loudness plan for individually selected physical audio tracks.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizeInput {
    pub asset_id: String,
    #[schemars(length(min = 1, max = 2))]
    pub tracks: Vec<TrackRequest>,
    pub output: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackRequest {
    pub stream_index: u32,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub integrated_centi_lufs: i32,
    pub true_peak_centi_dbtp: i32,
    pub range_centi_lu: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { integrated_centi_lufs: -1600, true_peak_centi_dbtp: -150, range_centi_lu: 1100 }
    }
}

impl Settings {
    pub fn validate(self) -> Result<()> {
        if !(-3000..=-1000).contains(&self.integrated_centi_lufs)
            || !(-900..=-100).contains(&self.true_peak_centi_dbtp)
            || !(100..=2000).contains(&self.range_centi_lu)
        {
            return Err(invalid("Choose integrated loudness -30 to -10 LUFS, peak -9 to -1 dBTP and range 1–20 LU"));
        }
        Ok(())
    }

    pub fn loudnorm(self) -> String {
        format!("loudnorm=I={}:TP={}:LRA={}", decimal(self.integrated_centi_lufs),
            decimal(self.true_peak_centi_dbtp), decimal(self.range_centi_lu))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FirstPass {
    pub integrated_centi_lufs: i32,
    pub true_peak_centi_dbtp: i32,
    pub range_centi_lu: i32,
    pub gate_centi_lufs: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub request: TrackRequest,
    pub measurement: FirstPass,
}

impl Track {
    pub fn validate(self) -> Result<()> {
        self.request.settings.validate()?;
        let m = self.measurement;
        let s = self.request.settings;
        if !(-9900..=0).contains(&m.integrated_centi_lufs)
            || !(-9900..=200).contains(&m.true_peak_centi_dbtp)
            || !(0..=s.range_centi_lu).contains(&m.range_centi_lu)
            || !(-9900..=0).contains(&m.gate_centi_lufs)
        {
            return Err(invalid("First-pass loudness facts are outside the linear normalization profile"));
        }
        let gain = s.integrated_centi_lufs - m.integrated_centi_lufs;
        if m.true_peak_centi_dbtp + gain > s.true_peak_centi_dbtp {
            return Err(invalid("The requested loudness and true peak cannot both be reached with linear gain"));
        }
        Ok(())
    }

    pub fn second_pass(self, sample_rate: u32) -> Result<String> {
        self.validate()?;
        let gain = self.request.settings.integrated_centi_lufs - self.measurement.integrated_centi_lufs;
        Ok(format!("volume={}dB:precision=double,aresample={sample_rate}", decimal(gain)))
    }
}

pub fn validate_requests(tracks: &[TrackRequest]) -> Result<()> {
    let mut indices = BTreeSet::new();
    if !(1..=2).contains(&tracks.len()) || tracks.iter().any(|track| !indices.insert(track.stream_index)) {
        return Err(invalid("Select one or two distinct physical audio streams"));
    }
    for track in tracks {
        track.settings.validate()?;
    }
    Ok(())
}

fn decimal(value: i32) -> String {
    let magnitude = value.unsigned_abs();
    let sign = if value < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", magnitude / 100, magnitude % 100)
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("NORMALIZATION_UNQUALIFIED", message)
}
