//! Authoritative registered probe bindings and original presentation clocks.
use super::*;
use crate::{mux::StreamKind, records::Asset};
use projects::TrackKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundSource {
    pub asset_id: String,
    pub asset_row_sha256: String,
    pub content_sha256: String,
    pub probe_sha256: String,
    pub stream_index: u32,
    pub media_kind: TrackKind,
    pub codec: String,
    pub time_base: Rational,
    pub asset_origin: Rational,
    pub stream_start: Rational,
}

impl BoundSource {
    pub fn original_tick(&self, relative: Rational) -> Result<i64> {
        if self.time_base <= Rational::ZERO || relative < Rational::ZERO {
            return Err(incompatible("Original time requires a nonnegative offset and positive time base"));
        }
        let original = self.asset_origin.checked_add(relative)?;
        let tick = original.checked_mul(Rational::new(self.time_base.denominator(), self.time_base.numerator())?)?;
        if original < self.stream_start || tick.denominator() != 1 {
            return Err(incompatible("Requested offset is outside the original stream or its time grid"));
        }
        Ok(tick.numerator())
    }

    pub fn original_audio_sample(&self, relative: Rational, sample_rate: u32) -> Result<u64> {
        if self.media_kind != TrackKind::Audio || !(8000..=384000).contains(&sample_rate) || relative < Rational::ZERO {
            return Err(incompatible("Original audio requires a physical stream and admitted sample grid"));
        }
        let time = self.asset_origin.checked_add(relative)?.checked_sub(self.stream_start)?;
        projects::grid_tick(time, Rational::new(i64::from(sample_rate), 1)?)
    }
}

fn incompatible(message: &str) -> DomainError {
    DomainError::new("UNSUPPORTED_SOURCE_CLOCK", message)
}

fn clock(row: &serde_json::Value) -> Result<(Rational, Rational)> {
    let base = row["time_base"]
        .as_str()
        .and_then(|value| value.split_once('/'))
        .ok_or_else(|| incompatible("A registered stream has no exact time base"))?;
    let (num, den) = (base.0.parse::<i32>(), base.1.parse::<i32>());
    let (Ok(num), Ok(den)) = (num, den) else {
        return Err(incompatible(
            "A registered stream has a malformed time base",
        ));
    };
    if num <= 0 || den <= 0 {
        return Err(incompatible(
            "A registered stream time base must be positive",
        ));
    }
    let time_base = Rational::new(i64::from(num), i64::from(den))?;
    let start = row["start_pts"]
        .as_i64()
        .or_else(|| row["start_pts"].as_str()?.parse().ok())
        .ok_or_else(|| incompatible("A registered stream has no exact original start tick"))?;
    Ok((time_base, time_base.checked_mul(Rational::new(start, 1)?)?))
}

pub fn bind_mixed_source(
    occurrence: &CapturedOccurrence,
    asset: &Asset,
    asset_row_sha256: String,
) -> Result<BoundSource> {
    let OccurrenceSource::Registered {
        asset_id,
        content_sha256,
        probe_sha256,
        stream_index,
        media_kind,
    } = &occurrence.source
    else {
        return Err(invalid(
            "A registered occurrence is required for an asset binding",
        ));
    };
    hash_text(&asset_row_sha256)?;
    if asset.id != *asset_id
        || asset.identity.sha256 != *content_sha256
        || projects::hash(&encode(&asset.probe)?) != *probe_sha256
    {
        return Err(DomainError::new(
            "SOURCE_CHANGED",
            "Registered asset or probe differs from the captured binding",
        ));
    }
    let requested = match media_kind {
        TrackKind::Video => StreamKind::Video,
        TrackKind::Audio => StreamKind::Audio,
    };
    let selected = asset
        .probe
        .streams
        .iter()
        .find(|stream| {
            stream.index == *stream_index && stream.kind == requested && !stream.attached_picture()
        })
        .ok_or_else(|| {
            DomainError::new(
                "UNRESOLVED_SOURCE_INDEX",
                "Captured physical stream differs from the registered probe",
            )
        })?;
    let timing = |index| {
        let row =
            asset.probe.stream_details.get(&index).ok_or_else(|| {
                incompatible("A registered stream has no exact clock observation")
            })?;
        clock(row)
    };
    let (time_base, stream_start) = timing(selected.index)?;
    let mut asset_origin = stream_start;
    for stream in &asset.probe.streams {
        if stream.index != selected.index
            && matches!(stream.kind, StreamKind::Audio | StreamKind::Video)
            && !stream.attached_picture()
        {
            asset_origin = asset_origin.min(timing(stream.index)?.1);
        }
    }
    Ok(BoundSource {
        asset_id: asset_id.clone(),
        asset_row_sha256,
        content_sha256: content_sha256.clone(),
        probe_sha256: probe_sha256.clone(),
        stream_index: *stream_index,
        media_kind: *media_kind,
        codec: selected.codec.clone(),
        time_base,
        asset_origin,
        stream_start,
    })
}
