//! Ordered, explicit media concatenation with reviewed packet-clock boundaries.
use crate::{DomainError, Result, packet_cut, records::Asset, time::Rational};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_INPUTS: usize = 8;
const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JoinInput {
    #[schemars(length(min = 2, max = 8))]
    pub inputs_in_order: Vec<String>,
    pub output: String,
    pub audio: Audio,
    pub strategy: Strategy,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Audio {
    CopyAll,
    Drop,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Copy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metadata {
    FirstInput,
    Strip,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub input: Asset,
    pub duration: Rational,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub primary_duration: Rational,
    pub following: Vec<Segment>,
    pub metadata: Metadata,
    pub audio: Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SegmentReview {
    pub position: usize,
    pub asset_id: String,
    pub duration: Rational,
    pub offset: Rational,
}

fn invalid(message: &str) -> DomainError {
    DomainError::new("JOIN_INCOMPATIBLE", message)
}

impl JoinInput {
    pub fn validate(&self) -> Result<()> {
        let count = self.inputs_in_order.len();
        if !(2..=MAX_INPUTS).contains(&count)
            || self.inputs_in_order.iter().collect::<BTreeSet<_>>().len() != count
        {
            return Err(invalid(
                "Choose two to eight different registered assets in playback order",
            ));
        }
        Ok(())
    }
}

impl Join {
    pub fn all<'a>(&'a self, primary: &'a Asset) -> impl Iterator<Item = (&'a Asset, Rational)> {
        std::iter::once((primary, self.primary_duration)).chain(
            self.following
                .iter()
                .map(|segment| (&segment.input, segment.duration)),
        )
    }

    pub fn validate(&self, primary: &Asset, output: &str) -> Result<()> {
        if !(1..MAX_INPUTS).contains(&self.following.len()) {
            return Err(invalid("A join needs two to eight registered sources"));
        }
        let mut native_ids = Vec::new();
        let mut paths = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut bytes = 0u64;
        let mut duration_sum = Rational::ZERO;
        for (asset, duration) in self.all(primary) {
            let object = asset
                .source_object
                .as_ref()
                .ok_or_else(|| invalid("Join input has no registered file identity"))?;
            if !ids.insert(&asset.id)
                || !paths.insert(&asset.identity.path)
                || native_ids.contains(&object)
                || asset.identity.path == output
                || duration <= Rational::ZERO
            {
                return Err(invalid(
                    "Join inputs need distinct file identities, positive durations and a new destination",
                ));
            }
            native_ids.push(object);
            let size = asset
                .identity
                .bytes
                .parse::<u64>()
                .map_err(|_| invalid("Invalid joined source size"))?;
            bytes = bytes
                .checked_add(size)
                .ok_or_else(|| invalid("Joined size overflow"))?;
            packet_cut::ffmpeg_time(duration)?;
            duration_sum = duration_sum.checked_add(duration)?;
        }
        packet_cut::ffmpeg_time(duration_sum)?;
        if bytes > MAX_INPUT_BYTES {
            return Err(DomainError::new(
                "RESOURCE_LIMIT",
                "Join staging is limited to 8 GiB across all inputs",
            ));
        }
        Ok(())
    }
}
