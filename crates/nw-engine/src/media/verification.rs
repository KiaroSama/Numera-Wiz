//! Independent, incremental checks of the finite generated signal.
use nw_domain::{
    DomainError, Result,
    media::{CornerColor, MediaPlanV1},
    media_execution::{MAX_VERIFICATION_BYTES, exact},
    time::Rational,
};
use serde_json::Value;

fn failed(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}

pub(super) struct VideoCheck {
    segments: Vec<(u64, [u8; 3])>,
    segment: usize,
    position: u64,
    expected: u64,
}
impl VideoCheck {
    pub(super) fn new(plan: &MediaPlanV1) -> Result<Self> {
        let settings = &plan.payload.capture.settings;
        Self::from_segments(
            settings.width,
            settings.height,
            plan.payload.graph.segments.iter().map(|segment| {
                Ok((exact(&segment.frames)?, segment.color))
            }).collect::<Result<Vec<_>>>()?,
        )
    }
    fn from_segments(width: u32, height: u32, segments: Vec<(u64, CornerColor)>) -> Result<Self> {
        let stride = u64::from(width).checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n > 0)
            .ok_or_else(|| failed("Invalid decoded video dimensions"))?;
        let mut expected = 0u64;
        let mut checks = Vec::with_capacity(segments.len());
        for (frames, color) in segments {
            let bytes = frames.checked_mul(stride)
                .filter(|_| frames > 0)
                .ok_or_else(|| failed("Invalid decoded segment extent"))?;
            expected = expected.checked_add(bytes)
                .filter(|n| *n <= MAX_VERIFICATION_BYTES)
                .ok_or_else(|| failed("Decoded video exceeds its admitted byte limit"))?;
            let bgr = match color {
                CornerColor::Black => [0, 0, 0],
                CornerColor::Red => [0, 0, 255],
                CornerColor::Green => [0, 255, 0],
                CornerColor::Blue => [255, 0, 0],
                CornerColor::Yellow => [0, 255, 255],
                CornerColor::Magenta => [255, 0, 255],
                CornerColor::Cyan => [255, 255, 0],
                CornerColor::White => [255, 255, 255],
            };
            checks.push((expected, bgr));
        }
        if checks.is_empty() {
            return Err(failed("Decoded video requires a nonempty graph"));
        }
        Ok(Self { segments: checks, segment: 0, position: 0, expected })
    }
    pub(super) fn feed(&mut self, mut bytes: &[u8]) -> Result<()> {
        let end = self.position.checked_add(bytes.len() as u64)
            .filter(|n| *n <= self.expected)
            .ok_or_else(|| failed("Decoded video contains extra frames or bytes"))?;
        while !bytes.is_empty() {
            let (segment_end, color) = self.segments[self.segment];
            let count = (segment_end - self.position).min(bytes.len() as u64) as usize;
            for (index, byte) in bytes[..count].iter().enumerate() {
                let channel = ((self.position + index as u64) & 3) as usize;
                // bgr0 has three signal channels; its unused fourth byte is not alpha.
                if channel != 3 && *byte != color[channel] {
                    return Err(failed("Decoded video differs from the captured corner-color graph"));
                }
            }
            self.position += count as u64;
            bytes = &bytes[count..];
            if self.position == segment_end && self.position < self.expected {
                self.segment += 1;
            }
        }
        debug_assert_eq!(self.position, end);
        Ok(())
    }
    pub(super) fn finish(&self) -> Result<()> {
        if self.position != self.expected {
            return Err(failed("Decoded video is truncated"));
        }
        Ok(())
    }
}

pub(super) struct AudioCheck {
    expected: u64,
    position: u64,
}
impl AudioCheck {
    pub(super) fn new(samples: u64, channels: u64) -> Result<Self> {
        let expected = samples.checked_mul(channels).and_then(|n| n.checked_mul(2))
            .filter(|n| *n > 0 && *n <= MAX_VERIFICATION_BYTES)
            .ok_or_else(|| failed("Decoded audio exceeds its admitted byte limit"))?;
        Ok(Self { expected, position: 0 })
    }
    pub(super) fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        let end = self.position.checked_add(bytes.len() as u64)
            .filter(|n| *n <= self.expected)
            .ok_or_else(|| failed("Decoded PCM contains extra samples or bytes"))?;
        if bytes.iter().any(|byte| *byte != 0) {
            return Err(failed("Decoded PCM differs from explicit silence"));
        }
        self.position = end;
        Ok(())
    }
    pub(super) fn finish(&self) -> Result<()> {
        if self.position != self.expected {
            return Err(failed("Decoded PCM sample count is truncated"));
        }
        Ok(())
    }
}

fn ratio(text: &str) -> Result<Rational> {
    if text.len() > 48 {
        return Err(failed("Observed time base exceeds its bound"));
    }
    let (num, den) = text.split_once('/').ok_or_else(|| failed("Missing observed time base"))?;
    let value = Rational::new(
        num.parse().map_err(|_| failed("Invalid observed time base"))?,
        den.parse().map_err(|_| failed("Invalid observed time base"))?,
    )?;
    if value <= Rational::ZERO {
        return Err(failed("Observed time base must be positive"));
    }
    Ok(value)
}
fn decimal_time(text: &str) -> Result<Rational> {
    if text.len() > 32 {
        return Err(failed("Observed duration exceeds its bound"));
    }
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(failed("Observed duration is not a finite decimal"));
    }
    let den = 10i64.pow(fraction.len() as u32);
    let whole: i64 = whole.parse().map_err(|_| failed("Observed duration overflow"))?;
    let tail: i64 = if fraction.is_empty() { 0 } else {
        fraction.parse().map_err(|_| failed("Observed duration overflow"))?
    };
    let num = whole.checked_mul(den).and_then(|n| n.checked_add(tail))
        .ok_or_else(|| failed("Observed duration overflow"))?;
    Rational::new(num, den)
}
fn number(value: &Value) -> Result<i64> {
    value.as_i64().ok_or_else(|| failed("Missing exact decoded frame counter"))
}

pub(super) fn metadata(plan: &MediaPlanV1, probe: &Value) -> Result<()> {
    let settings = &plan.payload.capture.settings;
    let streams = probe["streams"].as_array()
        .filter(|streams| streams.len() == 2)
        .ok_or_else(|| failed("Expected exactly one video and one audio stream"))?;
    let video = &streams[0];
    let audio = &streams[1];
    let channels = match settings.audio_layout {
        nw_domain::media::AudioLayout::Mono => 1,
        nw_domain::media::AudioLayout::Stereo => 2,
    };
    if video["index"] != 0 || video["codec_type"] != "video" || video["codec_name"] != "ffv1"
        || video["width"] != settings.width || video["height"] != settings.height
        || video["pix_fmt"] != "bgr0" || video["color_range"] != "pc"
        || video["color_space"] != "gbr" || video["color_primaries"] != "bt709"
        || video["color_transfer"] != "iec61966-2-1"
        || audio["index"] != 1 || audio["codec_type"] != "audio"
        || audio["codec_name"] != "pcm_s16le" || audio["sample_fmt"] != "s16"
        || audio["sample_rate"] != settings.sample_rate.to_string() || audio["channels"] != channels
    {
        return Err(failed("Encoded stream format/color/audio contract differs"));
    }
    if probe["format"]["format_name"] != "matroska,webm" {
        return Err(failed("Encoded output is not the admitted Matroska family"));
    }
    let duration = decimal_time(probe["format"]["duration"].as_str()
        .ok_or_else(|| failed("Missing encoded duration"))?)?;
    if duration != plan.payload.capture.range.end.checked_sub(plan.payload.capture.range.start)? {
        return Err(failed("Encoded duration differs from the selected range"));
    }
    let video_time_base = ratio(video["time_base"].as_str()
        .ok_or_else(|| failed("Missing video time base"))?)?;
    let audio_time_base = ratio(audio["time_base"].as_str()
        .ok_or_else(|| failed("Missing audio time base"))?)?;
    if video_time_base != Rational::new(1, 1000)? || audio_time_base != video_time_base {
        return Err(failed("Unexpected Matroska timestamp clock"));
    }
    let mut video_count = 0u64;
    let mut samples = 0u64;
    let frames = probe["frames"].as_array().ok_or_else(|| failed("Missing decoded frame facts"))?;
    for frame in frames {
        match frame["media_type"].as_str() {
            Some("video") => {
                if frame["stream_index"] != 0 {
                    return Err(failed("Decoded video has the wrong stream identity"));
                }
                let pts = Rational::new(number(&frame["pts"])?, 1)?.checked_mul(video_time_base)?;
                let expected = Rational::new(i64::try_from(video_count)
                    .map_err(|_| failed("Video frame count overflow"))?, 1)?
                    .checked_mul(Rational::new(
                        settings.frame_rate.denominator(), settings.frame_rate.numerator(),
                    )?)?;
                if pts != expected {
                    return Err(failed("Decoded video timestamps differ from the exact output grid"));
                }
                video_count = video_count.checked_add(1).ok_or_else(|| failed("Video count overflow"))?;
            }
            Some("audio") => {
                if frame["stream_index"] != 1 {
                    return Err(failed("Decoded audio has the wrong stream identity"));
                }
                // PCM's sample clock stays exact; Matroska packet PTS uses nearest milliseconds.
                let num = u128::from(samples) * 1000;
                let den = u128::from(settings.sample_rate);
                let expected = (num + den / 2) / den;
                if i128::from(number(&frame["pts"])?) != expected as i128 {
                    return Err(failed("Decoded PCM timestamps have a gap or shifted origin"));
                }
                let count = frame["nb_samples"].as_u64().filter(|n| *n > 0)
                    .ok_or_else(|| failed("Missing positive decoded PCM sample count"))?;
                samples = samples.checked_add(count).ok_or_else(|| failed("PCM count overflow"))?;
            }
            _ => return Err(failed("Unexpected decoded stream kind")),
        }
    }
    if video_count != exact(&plan.payload.output.video_frames)?
        || samples != exact(&plan.payload.output.audio_samples)?
    {
        return Err(failed("Decoded frame or sample count differs from the accepted contract"));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn exercise() {
    let mut pixels = Vec::new();
    for color in [[0, 0, 255], [0, 0, 255], [255, 0, 0]] {
        for _ in 0..4 { pixels.extend([color[0], color[1], color[2], 165]); }
    }
    let make = || VideoCheck::from_segments(2, 2, vec![(2, CornerColor::Red), (1, CornerColor::Blue)]).unwrap();
    let mut positive = make();
    for byte in &pixels { positive.feed(&[*byte]).unwrap(); }
    positive.finish().unwrap();
    let mut wrong = pixels.clone();
    wrong[17] = 255;
    assert_eq!(make().feed(&wrong).unwrap_err().code, "VERIFICATION_FAILED");
    assert!(make().feed(&[0; 49]).is_err());
    let mut short = make();
    short.feed(&pixels[..47]).unwrap();
    assert!(short.finish().is_err());
    let mut silence = AudioCheck::new(2, 2).unwrap();
    silence.feed(&[0; 3]).unwrap();
    silence.feed(&[0; 5]).unwrap();
    silence.finish().unwrap();
    assert!(AudioCheck::new(2, 2).unwrap().feed(&[1]).is_err());
    assert!(AudioCheck::new(2, 2).unwrap().feed(&[0; 9]).is_err());
    let mut short = AudioCheck::new(2, 2).unwrap();
    short.feed(&[0; 7]).unwrap();
    assert!(short.finish().is_err());
}
