//! Avid Log Exchange clip metadata for explicitly selected registered assets.
use crate::{
    DomainError, Result, mux::StreamKind, projects::hash, records::Asset, time::Rational,
    views::AssetSummary,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write};

const MAX_ASSETS: usize = 64;
const MAX_BYTES: usize = 96 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Rate {
    #[serde(rename = "23.976")]
    Fps23976,
    #[default]
    #[serde(rename = "24")]
    Fps24,
    #[serde(rename = "25")]
    Fps25,
    #[serde(rename = "29.97")]
    Fps2997,
    #[serde(rename = "30")]
    Fps30,
    #[serde(rename = "50")]
    Fps50,
    #[serde(rename = "59.94")]
    Fps5994,
    #[serde(rename = "60")]
    Fps60,
}

impl Rate {
    pub const ALL: [Self; 8] = [
        Self::Fps23976,
        Self::Fps24,
        Self::Fps25,
        Self::Fps2997,
        Self::Fps30,
        Self::Fps50,
        Self::Fps5994,
        Self::Fps60,
    ];

    pub fn label(self) -> &'static str {
        self.format().0
    }

    fn format(self) -> (&'static str, i64, bool) {
        match self {
            Self::Fps23976 => ("23.976", 24, true),
            Self::Fps24 => ("24", 24, false),
            Self::Fps25 => ("25", 25, false),
            Self::Fps2997 => ("29.97", 30, true),
            Self::Fps30 => ("30", 30, false),
            Self::Fps50 => ("50", 50, false),
            Self::Fps5994 => ("59.94", 60, true),
            Self::Fps60 => ("60", 60, false),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[schemars(length(min = 1, max = MAX_ASSETS))]
    pub asset_ids: Vec<String>,
    #[serde(default)]
    pub frame_rate: Rate,
}

impl Input {
    pub fn validate(&self) -> Result<()> {
        if self.asset_ids.is_empty() || self.asset_ids.len() > MAX_ASSETS {
            return Err(crate::projects::limit(
                "Select one to 64 assets for ALE export",
            ));
        }
        if self.asset_ids.iter().collect::<BTreeSet<_>>().len() != self.asset_ids.len() {
            return Err(invalid("ALE asset identifiers must be unique"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub asset_ids: Vec<String>,
    pub frame_rate: Rate,
    pub document: String,
    pub document_sha256: String,
}

fn invalid(message: &'static str) -> DomainError {
    DomainError::new("ALE_UNREPRESENTABLE", message)
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn probe_number(details: &Value, key: &str) -> Option<u64> {
    details[key]
        .as_u64()
        .or_else(|| details[key].as_str()?.parse().ok())
}

fn parse_timecode(code: &str, nominal_rate: i64) -> Result<i64> {
    let invalid_code =
        || invalid("ALE requires a valid non-drop source timecode at the chosen FPS");
    let mut parts = code.split(':').map(|s| {
        if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid_code());
        }
        s.parse::<i64>().map_err(|_| invalid_code())
    });
    let field = |part: Option<Result<i64>>| part.ok_or_else(invalid_code).and_then(|n| n);
    let (hour, minute, second, frame) = (
        field(parts.next())?,
        field(parts.next())?,
        field(parts.next())?,
        field(parts.next())?,
    );
    if parts.next().is_some() || hour >= 24 || minute >= 60 || second >= 60 || frame >= nominal_rate
    {
        return Err(invalid_code());
    }
    Ok(((hour * 60 + minute) * 60 + second) * nominal_rate + frame)
}

fn timecode(frames: i64, nominal_rate: i64) -> Result<String> {
    let seconds = frames / nominal_rate;
    if !(0..86400).contains(&seconds) {
        return Err(invalid("ALE timecodes must fit within 24 hours"));
    }
    Ok(format!(
        "{:02}:{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        frames % nominal_rate
    ))
}

pub fn export(input: &Input, assets: &[Asset]) -> Result<Export> {
    let (fps_text, nominal_rate, ntsc) = input.frame_rate.format();
    let frame_rate = if ntsc {
        Rational::new(nominal_rate * 1000, 1001)?
    } else {
        Rational::new(nominal_rate, 1)?
    };
    let mut height = None;
    let mut sample_rate = None;
    let mut body = String::new();
    for asset in assets {
        let video = asset
            .probe
            .streams
            .iter()
            .find(|stream| stream.kind == StreamKind::Video && !stream.attached_picture());
        let details = &asset.probe.stream_details;
        let mut channels = 0;
        for stream in asset
            .probe
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Audio)
        {
            let audio = details
                .get(&stream.index)
                .ok_or_else(|| invalid("ALE audio metadata is missing"))?;
            let count = probe_number(audio, "channels")
                .filter(|n| (1..=16).contains(n))
                .ok_or_else(|| invalid("ALE requires known channel counts from 1 through 16"))?;
            channels += count;
            if channels > 16 {
                return Err(invalid(
                    "ALE cannot describe more than 16 audio channels per asset",
                ));
            }
            let current_rate = probe_number(audio, "sample_rate");
            if sample_rate.is_some() && current_rate != sample_rate {
                return Err(invalid(
                    "Export assets at each audio sample rate in a separate ALE log",
                ));
            }
            sample_rate =
                Some(current_rate.ok_or_else(|| invalid("ALE requires a known sample rate"))?);
        }
        if video.is_none() && channels == 0 {
            return Err(invalid("ALE requires an audio or picture stream"));
        }
        if let Some(video) = video {
            height = height.or_else(|| probe_number(details.get(&video.index)?, "height"));
        }
        let tags = &asset.probe.global_tags;
        let tag = |key: &str| {
            tags.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.as_str())
        };
        let source_code = tag("timecode").or_else(|| {
            video?
                .tags
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("timecode"))
                .map(|(_, value)| value.as_str())
        });
        let start = source_code
            .map(|code| parse_timecode(code, nominal_rate))
            .transpose()?
            .unwrap_or(0);
        let duration = asset
            .probe
            .container_duration
            .filter(|t| *t > Rational::ZERO)
            .ok_or_else(|| invalid("ALE requires a known positive source duration"))?;
        let frames = duration.checked_mul(frame_rate)?;
        // Round the exclusive end outward when a source ends between log frames.
        let frames = frames.numerator() / frames.denominator()
            + i64::from(frames.numerator() % frames.denominator() != 0);
        let end = start
            .checked_add(frames)
            .ok_or_else(|| invalid("ALE source duration is too long"))?;
        let mut tracks = String::new();
        if video.is_some() {
            tracks.push('V');
        }
        for index in 1..=channels {
            write!(tracks, "A{index}").expect("write string");
        }
        let basename = asset
            .identity
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default();
        let name = AssetSummary::from(asset).display_name;
        let tape = tag("tape name")
            .or_else(|| tag("tape"))
            .or_else(|| tag("reel_name"));
        writeln!(
            body,
            "{}\t{tracks}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\r",
            clean(&name),
            timecode(start, nominal_rate)?,
            timecode(end, nominal_rate)?,
            clean(tape.unwrap_or_default()),
            clean(tag("scene").unwrap_or_default()),
            clean(tag("take").unwrap_or_default()),
            clean(tag("comment").unwrap_or_default()),
            clean(basename)
        )
        .expect("write string");
        if body.len() > MAX_BYTES {
            return Err(crate::projects::limit("ALE document exceeds 96 KiB"));
        }
    }
    let video_format = match height {
        None | Some(1080) => "1080",
        Some(720) => "720",
        Some(480 | 486) => "NTSC",
        Some(576) => "PAL",
        Some(_) => "CUSTOM",
    };
    let audio_format = match sample_rate {
        None | Some(48000) => "48kHz",
        Some(44100) => "44kHz",
        Some(24000) => "24kHz",
        Some(22050) => "22kHz",
        Some(_) => return Err(invalid("ALE supports 22.05, 24, 44.1 and 48 kHz audio")),
    };
    let document = format!(
        "Heading\r\nFIELD_DELIM\tTABS\r\nVIDEO_FORMAT\t{video_format}\r\n\
        AUDIO_FORMAT\t{audio_format}\r\nFPS\t{fps_text}\r\n\r\nColumn\r\n\
        Name\tTracks\tStart\tEnd\tTape\tScene\tTake\tComments\tSource File\r\n\r\nData\r\n{body}"
    );
    if document.len() > MAX_BYTES {
        return Err(crate::projects::limit("ALE document exceeds 96 KiB"));
    }
    Ok(Export {
        asset_ids: input.asset_ids.clone(),
        frame_rate: input.frame_rate,
        document_sha256: hash(document.as_bytes()),
        document,
    })
}
