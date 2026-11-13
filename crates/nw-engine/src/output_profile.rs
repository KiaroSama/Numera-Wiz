//! Explicit development mux routes. Actual verification remains mandatory.
use crate::admission;
use nw_domain::{
    DomainError, Result,
    mux::{CleanResolution, StreamKind},
    records::Asset,
};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputProfile {
    Matroska,
    QuickTimeMpeg4,
    IsoMp4Mpeg4,
}
impl OutputProfile {
    pub fn from_output(path: &Path) -> Result<Self> {
        match path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mkv") => Ok(Self::Matroska),
            Some("mov") => Ok(Self::QuickTimeMpeg4),
            Some("mp4" | "m4v") => Ok(Self::IsoMp4Mpeg4),
            _ => Err(DomainError::new(
                "CONTAINER_UNQUALIFIED",
                "No admitted development preservation profile for this output container",
            )),
        }
    }
    pub fn muxer(self) -> &'static str {
        match self {
            Self::Matroska => "matroska",
            Self::QuickTimeMpeg4 => "mov",
            Self::IsoMp4Mpeg4 => "mp4",
        }
    }
    pub fn scope(self) -> &'static str {
        match self {
            Self::Matroska => crate::preservation::SCOPE,
            Self::QuickTimeMpeg4 | Self::IsoMp4Mpeg4 => ISO_SCOPE,
        }
    }
    pub fn admit(self, source: &Asset, outcome: &CleanResolution) -> Result<()> {
        if self == Self::Matroska {
            return Ok(());
        }
        if admission::frozen_demuxer(&source.probe.format)? != "mov" {
            return Err(incompatible(
                "This MOV/MP4 route requires an original self-contained MOV-family source",
            ));
        }
        let extension = Path::new(&source.identity.path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        let same_family = match self {
            Self::QuickTimeMpeg4 => extension.as_deref() == Some("mov"),
            Self::IsoMp4Mpeg4 => matches!(extension.as_deref(), Some("mp4" | "m4v")),
            Self::Matroska => unreachable!(),
        };
        if !same_family {
            return Err(incompatible(
                "Review an output in the original MOV/MP4 container family; cross-family conversion is not admitted by this profile",
            ));
        }
        let (brand, compatible, vendor) = match self {
            Self::QuickTimeMpeg4 => ("qt  ", "qt  ", Some("FFMP")),
            Self::IsoMp4Mpeg4 => ("isom", "isomiso2mp41", None),
            Self::Matroska => unreachable!(),
        };
        let tags = &source.probe.global_tags;
        for (key, expected) in [
            ("major_brand", brand),
            ("minor_version", "512"),
            ("compatible_brands", compatible),
        ] {
            if tags.get(key).map(String::as_str) != Some(expected) {
                return Err(incompatible(
                    "Original brand/version metadata is outside this exact MOV/MP4 profile",
                ));
            }
        }
        if !outcome.preservation.global_tags || !outcome.preservation.stream_tags {
            return Err(incompatible(
                "This MOV/MP4 profile retains required original tags; metadata stripping requires a separately admitted technical-metadata policy",
            ));
        }
        if source
            .probe
            .chapters
            .as_array()
            .is_none_or(|chapters| !chapters.is_empty())
        {
            return Err(incompatible(
                "Nonempty MOV/MP4 chapter preservation requires its own qualified route",
            ));
        }
        if tags.keys().any(|key| {
            ![
                "major_brand",
                "minor_version",
                "compatible_brands",
                "encoder",
                "creation_time",
                "title",
                "artist",
                "album",
                "comment",
                "copyright",
                "date",
                "description",
                "genre",
                "composer",
            ]
            .contains(&key.as_str())
        }) {
            return Err(incompatible(
                "An authored global tag has no admitted MOV/MP4 preservation mapping",
            ));
        }
        if outcome.streams.is_empty() {
            return Err(incompatible("A retained ordinary video stream is required"));
        }
        for stream in &outcome.streams {
            if stream.kind != StreamKind::Video
                || stream.attached_picture()
                || stream.codec != "mpeg4"
            {
                return Err(incompatible(
                    "This initial MOV/MP4 packet-copy profile admits ordinary MPEG-4 Part 2 video; retained audio, covers, subtitles, attachments, data and other codecs require their own preservation routes",
                ));
            }
            if stream.dispositions.iter().any(|value| value != "default") {
                return Err(incompatible(
                    "A retained disposition has no admitted MOV/MP4 preservation mapping",
                ));
            }
            if stream.tags.keys().any(|key| {
                !["language", "title", "handler_name", "vendor_id", "encoder"]
                    .contains(&key.as_str())
            }) {
                return Err(incompatible(
                    "An authored stream tag has no admitted MOV/MP4 preservation mapping",
                ));
            }
            let language = stream.tags.get("language").ok_or_else(|| {
                incompatible("The retained MOV/MP4 stream requires an explicit original language")
            })?;
            if language.len() != 3 || !language.bytes().all(|byte| byte.is_ascii_lowercase()) {
                return Err(incompatible(
                    "MOV/MP4 language requires an admitted lowercase three-letter code",
                ));
            }
            if stream.tags.get("title").is_some_and(String::is_empty) {
                return Err(incompatible(
                    "An empty MOV/MP4 title cannot be retained as an authored value; choose an explicit clear",
                ));
            }
            if stream.tags.get("vendor_id").map(String::as_str) != vendor
                || stream
                    .tags
                    .get("encoder")
                    .is_some_and(|value| value.len() > 31)
                || stream
                    .tags
                    .get("handler_name")
                    .is_some_and(|value| value.len() > 255)
            {
                return Err(incompatible(
                    "A required MOV/MP4 vendor or length-limited label cannot be reproduced exactly",
                ));
            }
        }
        if !outcome.streams[0].dispositions.contains("default") {
            return Err(incompatible(
                "This MOV/MP4 route requires the original retained primary video default; it never silently promotes another track",
            ));
        }
        Ok(())
    }
}
const ISO_SCOPE: &str = "Original-family MOV/MP4 MPEG-4 Part 2 ordinary video packet-copy profile. Full per-stream packet payload, exact original-clock timestamps, flags, codec/extradata, exposed side data and all authored stream/global tags must match before publication. Reviewed source brands/vendor/default dispositions remain exact. Unambiguous MOV/MP4 track name metadata is the canonical title; raw tags remain in the source probe details and dual name/title records are incompatible. No undefined-language omission or duration-tag rewrite is admitted for this profile. Generated container encoder metadata is permitted only where no authored value is retained. Nonempty chapters, metadata stripping, other codecs/stream categories, cross-family conversion and decoded reframing require separate preservation routes.";
fn incompatible(message: &str) -> DomainError {
    DomainError::new("INCOMPATIBLE_PRESERVATION", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_domain::{mux::*, records::*};
    use std::collections::{BTreeMap, BTreeSet};
    fn fixture() -> (Asset, CleanResolution) {
        let streams = vec![Stream {
            index: 0,
            kind: StreamKind::Video,
            codec: "mpeg4".into(),
            tags: BTreeMap::from([
                ("language".into(), "und".into()),
                ("encoder".into(), "Preserved encoder".into()),
            ]),
            dispositions: BTreeSet::from(["default".into()]),
        }];
        let source = Asset {
            source_object: None,
            display_name: "source.mp4".into(),
            id: "a".into(),
            creator: "owner".into(),
            identity: FileIdentity {
                path: "/source.mp4".into(),
                bytes: "1".into(),
                sha256: "0".repeat(64),
            },
            probe: MediaProbe {
                probe_timeout: None,
                streams: streams.clone(),
                stream_details: BTreeMap::new(),
                format: "mov,mp4,m4a,3gp,3g2,mj2".into(),
                global_tags: BTreeMap::from([
                    ("major_brand".into(), "isom".into()),
                    ("minor_version".into(), "512".into()),
                    ("compatible_brands".into(), "isomiso2mp41".into()),
                ]),
                chapters: serde_json::json!([]),
                verification_scope: "Synthetic profile admission only".into(),
            },
        };
        let resolution = CleanResolution {
            action: CleanAction::Remux,
            streams,
            outcomes: vec![],
            reasons: vec![],
            warnings: vec![],
            preservation: Preservation::default(),
        };
        (source, resolution)
    }
    #[test]
    fn iso_profile_refuses_unrepresentable_tags_streams_defaults_and_brand_changes() {
        let profile = OutputProfile::from_output(Path::new("/output.mp4")).unwrap();
        let (source, resolution) = fixture();
        profile.admit(&source, &resolution).unwrap();
        let mut changed = resolution.clone();
        changed.streams[0]
            .tags
            .insert("review_note".into(), "Must not disappear".into());
        assert_eq!(
            profile.admit(&source, &changed).unwrap_err().code,
            "INCOMPATIBLE_PRESERVATION"
        );
        changed = resolution.clone();
        changed.streams[0]
            .tags
            .insert("name".into(), "Original name".into());
        changed.streams[0]
            .tags
            .insert("title".into(), "Separate title".into());
        assert!(profile.admit(&source, &changed).is_err());
        changed = resolution.clone();
        changed.streams[0]
            .tags
            .insert("vendor_id".into(), "[0][0][0][0]".into());
        assert!(profile.admit(&source, &changed).is_err());
        changed = resolution.clone();
        changed.streams[0].codec = "h264".into();
        assert!(profile.admit(&source, &changed).is_err());
        changed = resolution.clone();
        changed.streams[0].dispositions.clear();
        assert!(profile.admit(&source, &changed).is_err());
        let mut changed_source = source.clone();
        changed_source
            .probe
            .global_tags
            .insert("major_brand".into(), "avc1".into());
        assert!(profile.admit(&changed_source, &resolution).is_err());
        changed_source = source.clone();
        changed_source.identity.path = "/source.mov".into();
        assert!(profile.admit(&changed_source, &resolution).is_err());
        changed = resolution;
        changed.preservation.stream_tags = false;
        assert!(profile.admit(&source, &changed).is_err());
    }
}
