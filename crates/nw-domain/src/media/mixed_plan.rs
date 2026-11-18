//! Separately framed review contract for registered mixed media.
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MixedKind {
    MediaV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderContractV2 {
    pub contract_kind: MixedKind,
    pub sequence_id: EntityId,
    pub revision: Revision,
    #[serde(deserialize_with = "read_range")]
    #[schemars(schema_with = "range_schema")]
    pub range: TimeRange,
    pub output: MixedOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedOutput {
    pub container: Container,
    pub video_codec: VideoCodec,
    pub audio_codec: AudioCodec,
    pub destination: Destination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedTools {
    pub ffmpeg: ComponentIdentity,
    pub ffprobe: ComponentIdentity,
    pub profile_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaPlanV2 {
    #[schemars(extend("const" = 2))]
    pub envelope_version: u32,
    pub payload_kind: MixedKind,
    pub id: String,
    pub hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub payload: MixedPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedPayload {
    pub root_id: String,
    pub application_build: String,
    pub principal: String,
    #[schemars(extend("const" = "media_execution.v2"))]
    pub required_capability: String,
    pub capture: MixedCapture,
    pub sources: Vec<BoundSource>,
    pub graph: MixedGraph,
    pub tools: MixedTools,
    pub output: MixedOutput,
    pub resources: MixedResources,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixedResources {
    pub decoded_output_bytes: String,
    pub video_spans: usize,
    pub audio_spans: usize,
    pub source_bindings: usize,
}

pub struct MixedReview<'a> {
    pub root_id: &'a str,
    pub application_build: &'a str,
    pub principal: &'a str,
    pub id: &'a str,
    pub now: u64,
}

fn source_set(capture: &MixedCapture, sources: &[BoundSource]) -> Result<()> {
    let expected = capture.tracks.iter().flat_map(|t| &t.clips).filter_map(|clip| {
        let OccurrenceSource::Registered { asset_id, content_sha256, probe_sha256, stream_index, media_kind } = &clip.source else {
            return None;
        };
        Some((asset_id.as_str(), *stream_index, content_sha256.as_str(), probe_sha256.as_str(), *media_kind == projects::TrackKind::Audio))
    }).collect::<BTreeSet<_>>();
    let mut keys = BTreeSet::new();
    let mut last = None;
    for source in sources {
        projects::validate_id(&source.asset_id)?;
        hash_text(&source.asset_row_sha256)?;
        hash_text(&source.content_sha256)?;
        hash_text(&source.probe_sha256)?;
        let key = (source.asset_id.as_str(), source.stream_index);
        if last.is_some_and(|last| last >= key) || source.codec.is_empty() || source.codec.len() > 64
            || source.time_base <= Rational::ZERO || source.asset_origin > source.stream_start {
            return Err(invalid("Registered source bindings are not in canonical order or lack an exact clock"));
        }
        last = Some(key);
        keys.insert((key.0, key.1, source.content_sha256.as_str(), source.probe_sha256.as_str(), source.media_kind == projects::TrackKind::Audio));
    }
    if expected != keys {
        return Err(invalid("Registered bindings differ from the captured source set"));
    }
    Ok(())
}

fn resources(capture: &MixedCapture, graph: &MixedGraph, sources: usize) -> Result<MixedResources> {
    let limit = || projects::limit("Mixed media output counters exceed their bounded range");
    let video_bytes = decimal(&graph.output_frames)?
        .checked_mul(u64::from(capture.settings.width) * u64::from(capture.settings.height) * 4)
        .ok_or_else(limit)?;
    let channels = match capture.settings.audio_layout {
        AudioLayout::Mono => 1,
        AudioLayout::Stereo => 2,
    };
    let audio_bytes = decimal(&graph.output_samples)?.checked_mul(channels * 2).ok_or_else(limit)?;
    let decoded_bytes = video_bytes.checked_add(audio_bytes)
        .filter(|bytes| *bytes <= crate::media_execution::MAX_OUTPUT_BYTES)
        .ok_or_else(limit)?;
    Ok(MixedResources {
        decoded_output_bytes: decoded_bytes.to_string(),
        video_spans: graph.video.len(),
        audio_spans: graph.audio.len(),
        source_bindings: sources,
    })
}

impl MediaPlanV2 {
    pub fn compile(
        request: &RenderContractV2,
        capture: MixedCapture,
        sources: Vec<BoundSource>,
        tools: MixedTools,
        review: MixedReview<'_>,
    ) -> Result<Self> {
        if capture.sequence_id != request.sequence_id || capture.project_revision != request.revision
            || capture.range != request.range {
            return Err(invalid("Mixed review range or sequence differs from its authoritative capture"));
        }
        let graph = compile_mixed_graph(&capture)?;
        let resources = resources(&capture, &graph, sources.len())?;
        let mut plan = Self {
            envelope_version: 2,
            payload_kind: MixedKind::MediaV2,
            id: review.id.into(),
            hash: "0".repeat(64),
            created_at: review.now.to_string(),
            expires_at: review.now.checked_add(300).ok_or_else(|| projects::limit("Mixed review clock range exhausted"))?.to_string(),
            payload: MixedPayload {
                root_id: review.root_id.into(), application_build: review.application_build.into(), principal: review.principal.into(),
                required_capability: "media_execution.v2".into(), capture, sources, graph, tools,
                output: request.output.clone(), resources,
            },
        };
        plan.hash = plan.digest()?;
        Ok(plan)
    }

    pub fn digest(&self) -> Result<String> {
        let payload = &self.payload;
        projects::validate_id(&self.id)?;
        projects::validate_id(&payload.root_id)?;
        for value in [&payload.application_build, &payload.tools.profile_sha256, &payload.tools.ffmpeg.sha256, &payload.tools.ffprobe.sha256] {
            hash_text(value)?;
        }
        if self.envelope_version != 2 || payload.required_capability != "media_execution.v2"
            || payload.principal.is_empty() || payload.principal.len() > 256 || payload.principal.chars().any(char::is_control)
            || decimal(&payload.tools.ffmpeg.bytes)? == 0 || decimal(&payload.tools.ffprobe.bytes)? == 0
            || decimal(&self.created_at)?.checked_add(300) != Some(decimal(&self.expires_at)?) {
            return Err(invalid("Invalid mixed review identity, component or expiry"));
        }
        let graph = compile_mixed_graph(&payload.capture)?;
        source_set(&payload.capture, &payload.sources)?;
        if payload.graph != graph || payload.resources != resources(&payload.capture, &graph, payload.sources.len())? {
            return Err(invalid("Mixed review graph or resources differ from the authoritative capture"));
        }
        if encode(self)?.len() > MAX_PLAN_BYTES {
            return Err(projects::limit("Serialized mixed media plan exceeds128KiB"));
        }
        let mut framed = b"NumeraWiz.MediaPlan.v2\0".to_vec();
        framed.extend(encode(payload)?);
        Ok(projects::hash(&framed))
    }

    pub fn validate_hash(&self) -> Result<()> {
        hash_text(&self.hash)?;
        if self.digest()? != self.hash {
            return Err(invalid("Mixed plan semantic hash mismatch"));
        }
        Ok(())
    }
}
