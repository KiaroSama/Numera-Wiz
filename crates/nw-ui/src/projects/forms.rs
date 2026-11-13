//! Human input strings become exact canonical parameters once, at the click.
use super::*;
use nw_domain::projects::{SequenceInput, SequenceSettings, TrackInput, TrackKind};

#[derive(Clone)]
pub(super) struct ExactForm {
    pub num: String,
    pub den: String,
}
impl ExactForm {
    pub fn new(num: &str, den: &str) -> Self {
        Self {
            num: num.into(),
            den: den.into(),
        }
    }
    pub fn value(&self) -> Result<Rational, String> {
        if self.num.len() > 20 || self.den.len() > 20 {
            return Err("Exact time exceeds its integer bound".into());
        }
        Rational::new(
            self.num
                .parse()
                .map_err(|_| "Expected an integer numerator")?,
            self.den
                .parse()
                .map_err(|_| "Expected a positive integer denominator")?,
        )
        .map_err(|error| error.message)
    }
}
pub(super) struct Forms {
    pub project_name: String,
    pub sequence_name: String,
    pub width: String,
    pub height: String,
    pub frame_rate: ExactForm,
    pub sample_rate: String,
    pub audio_layout: String,
    pub video_tracks: String,
    pub audio_tracks: String,
    pub registered: bool,
    pub rgba: String,
    pub duration: ExactForm,
    pub source_start: ExactForm,
    pub source_end: ExactForm,
    pub sequence_start: ExactForm,
    pub delta: ExactForm,
    pub split: ExactForm,
    pub viewport_start: ExactForm,
    pub viewport_end: ExactForm,
}
impl Default for Forms {
    fn default() -> Self {
        Self {
            project_name: "New project".into(),
            sequence_name: "Sequence".into(),
            width: "1920".into(),
            height: "1080".into(),
            frame_rate: ExactForm::new("25", "1"),
            sample_rate: "48000".into(),
            audio_layout: "stereo".into(),
            video_tracks: "Video\nSource".into(),
            audio_tracks: "Audio".into(),
            registered: false,
            rgba: "0.2, 0.4, 0.8, 1".into(),
            duration: ExactForm::new("4", "1"),
            source_start: ExactForm::new("0", "1"),
            source_end: ExactForm::new("4", "1"),
            sequence_start: ExactForm::new("0", "1"),
            delta: ExactForm::new("2", "1"),
            split: ExactForm::new("4", "1"),
            viewport_start: ExactForm::new("0", "1"),
            viewport_end: ExactForm::new("8", "1"),
        }
    }
}
impl Forms {
    pub fn sequence(&self) -> Result<Value, String> {
        nw_domain::projects::label(&self.sequence_name).map_err(|error| error.message)?;
        let settings = SequenceSettings {
            width: self.width.parse().map_err(|_| "Width must be an integer")?,
            height: self
                .height
                .parse()
                .map_err(|_| "Height must be an integer")?,
            frame_rate: self.frame_rate.value()?,
            sample_rate: self
                .sample_rate
                .parse()
                .map_err(|_| "Sample rate must be an integer")?,
            audio_layout: self.audio_layout.clone(),
        };
        settings.validate().map_err(|error| error.message)?;
        let mut tracks = Vec::new();
        for (names, kind) in [
            (&self.video_tracks, TrackKind::Video),
            (&self.audio_tracks, TrackKind::Audio),
        ] {
            for name in names.lines().filter(|name| !name.is_empty()) {
                nw_domain::projects::label(name).map_err(|error| error.message)?;
                if tracks.len() >= 64 {
                    return Err("A sequence can contain at most 64 explicit tracks".into());
                }
                tracks.push(TrackInput {
                    kind,
                    name: name.into(),
                });
            }
        }
        if tracks.is_empty() {
            return Err("Name at least one video or audio track".into());
        }
        Ok(json!(SequenceInput {
            name: self.sequence_name.clone(),
            settings,
            tracks
        }))
    }
    pub fn range(&self) -> Result<TimeRange, String> {
        let range = TimeRange {
            start: self.viewport_start.value()?,
            end: self.viewport_end.value()?,
        };
        range.validate().map_err(|error| error.message)?;
        Ok(range)
    }
    pub fn place(
        &self,
        group: &Group,
        asset: Option<&Asset>,
        stream: Option<u32>,
    ) -> Result<Value, String> {
        let sequence = group.view.sequence.as_deref().ok_or("Select a sequence")?;
        let track = group.view.track.as_deref().ok_or("Select a track")?;
        let source = if self.registered {
            let asset = asset.ok_or("Inspect a registered source first")?;
            let stream = stream.ok_or("Select the physical source stream")?;
            if !asset.probe.streams.iter().any(|item| item.index == stream) {
                return Err("The selected stream is absent from this inspected source".into());
            }
            json!({"kind":"registered","asset_id":asset.id,"stream_index":stream})
        } else {
            let channels = self
                .rgba
                .split(',')
                .map(|value| value.trim().parse::<f64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "RGBA needs four finite numbers")?;
            let rgba: [f64; 4] = channels
                .try_into()
                .map_err(|_| "RGBA needs four channels")?;
            if rgba
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return Err("RGBA channels must be finite and between zero and one".into());
            }
            json!({"kind":"solid_color","rgba":rgba,"duration":self.duration.value()?})
        };
        Ok(
            json!({"sequence_id":sequence,"track_id":track,"source":source,
            "source_range":{"start":self.source_start.value()?,"end":self.source_end.value()?},
            "sequence_start":self.sequence_start.value()?,"collision":"reject"}),
        )
    }
}
