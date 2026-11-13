use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    MediaV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaPlanV1 {
    #[schemars(extend("const" = 1))]
    pub envelope_version: u32,
    pub payload_kind: PayloadKind,
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub id: String,
    #[schemars(length(min = 1, max = 256))]
    pub creator: String,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub hash: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^(0|[1-9][0-9]*)$"))]
    pub created_at: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub expires_at: String,
    pub payload: MediaPayload,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaPayload {
    #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
    pub root_id: String,
    #[schemars(extend("pattern" = "^[0-9a-f]{64}$"))]
    pub application_build: String,
    pub capture: Capture,
    pub tools: MediaTools,
    pub graph: MediaGraph,
    pub output: OutputManifest,
    pub rights: RequiredRights,
    pub resources: ResourceLimits,
    #[schemars(extend("const" = "media_execution.v1"))]
    pub required_capability: String,
    #[schemars(schema_with = "argv_schema")]
    pub argv_template: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequiredRights {
    #[schemars(length(min = 1, max = 256))]
    pub principal: String,
    #[schemars(extend("const" = "project.read"))]
    pub project: String,
    #[schemars(extend("const" = "download.create"))]
    pub publication: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    #[schemars(extend("const" = 16))]
    pub tracks: usize,
    #[schemars(extend("const" = 128))]
    pub clips: usize,
    #[schemars(extend("const" = 256))]
    pub closure_entities: usize,
    #[schemars(extend("const" = 256))]
    pub segments: usize,
    #[schemars(extend("const" = 1048576))]
    pub capture_bytes: usize,
    #[schemars(extend("const" = 131072))]
    pub plan_bytes: usize,
    #[schemars(extend("const" = 60))]
    pub duration_seconds: u32,
}
fn resources() -> ResourceLimits {
    ResourceLimits {
        tracks: MAX_TRACKS,
        clips: MAX_CLIPS,
        closure_entities: projects::MAX_CLOSURE,
        segments: MAX_SEGMENTS,
        capture_bytes: MAX_RAW_BYTES,
        plan_bytes: MAX_PLAN_BYTES,
        duration_seconds: 60,
    }
}
fn argv_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"type":"array", "minItems":1, "maxItems":80, "items":{"type":"string", "minLength":1, "maxLength":65536}})
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputManifest {
    pub intent: OutputIntent,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub video_frames: String,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub audio_samples: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColorProfile {
    OpaqueSrgbCornersBgr0V1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GapPolicy {
    OpaqueBlack,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaGraph {
    pub color_profile: ColorProfile,
    pub gap_policy: GapPolicy,
    #[schemars(length(min = 1, max = 256))]
    pub segments: Vec<VideoSegment>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoSegment {
    pub origin: SegmentOrigin,
    pub color: CornerColor,
    #[schemars(length(min = 1, max = 20), extend("pattern" = "^[1-9][0-9]*$"))]
    pub frames: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SegmentOrigin {
    Gap,
    Clip {
        #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
        clip_id: EntityId,
        #[schemars(extend("pattern" = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))]
        source_id: EntityId,
    },
}
fn facts(capture: &Capture) -> Result<()> {
    capture.settings.validate()?;
    capture.range.validate()?;
    hash_text(&capture.header_sha256)?;
    if capture.header_bytes == 0
        || capture.header_bytes > MAX_RAW_BYTES
        || capture.facts.is_empty()
        || capture.facts.len() > projects::MAX_CLOSURE
        || capture.clips.len() > MAX_CLIPS
    {
        return Err(invalid("Media capture exceeds finite bounds"));
    }
    let mut bytes = capture.header_bytes;
    let mut ids = BTreeSet::new();
    let mut previous = None;
    for fact in &capture.facts {
        hash_text(&fact.sha256)?;
        if fact.bytes == 0
            || fact.bytes > MAX_RAW_BYTES.saturating_sub(bytes)
            || previous.is_some_and(|id: &EntityId| id >= &fact.id)
        {
            return Err(invalid(
                "Media raw facts exceed bounds or canonical identity order",
            ));
        }
        bytes += fact.bytes;
        ids.insert(&fact.id);
        previous = Some(&fact.id);
    }
    if !ids.contains(&capture.sequence_id) {
        return Err(invalid("Sequence raw identity missing"));
    }
    let mut clip_ids = BTreeSet::new();
    for clip in &capture.clips {
        if !ids.contains(&clip.clip_id)
            || !ids.contains(&clip.source_id)
            || !clip_ids.insert(&clip.clip_id)
        {
            return Err(invalid("Clip/source raw identity missing or repeated"));
        }
    }
    Ok(())
}
fn derive(capture: &Capture) -> Result<(MediaGraph, OutputManifest)> {
    facts(capture)?;
    let settings = &capture.settings;
    let duration = capture.range.end.checked_sub(capture.range.start)?;
    if duration > Rational::new(60, 1)? {
        return Err(invalid("Media duration exceeds60 seconds"));
    }
    let audio_grid = Rational::new(i64::from(settings.sample_rate), 1)?;
    for point in [capture.range.start, capture.range.end] {
        projects::grid_tick(point, settings.frame_rate)?;
        projects::grid_tick(point, audio_grid)?;
    }
    let video_frames = projects::grid_tick(duration, settings.frame_rate)?;
    let audio_samples = projects::grid_tick(duration, audio_grid)?;
    if video_frames == 0 || audio_samples == 0 {
        return Err(invalid("Media output must have frames and samples"));
    }
    let mut segments = Vec::new();
    let mut cursor = capture.range.start;
    for clip in &capture.clips {
        clip.range.validate()?;
        if clip.range.start < cursor || clip.range.end > capture.range.end {
            return Err(invalid("Clips overlap or escape the captured range"));
        }
        for point in [clip.range.start, clip.range.end] {
            projects::grid_tick(point, settings.frame_rate)?;
            projects::grid_tick(point, audio_grid)?;
        }
        if clip.range.start > cursor {
            segments.push(VideoSegment {
                origin: SegmentOrigin::Gap,
                color: CornerColor::Black,
                frames: projects::grid_tick(
                    clip.range.start.checked_sub(cursor)?,
                    settings.frame_rate,
                )?
                .to_string(),
            });
        }
        segments.push(VideoSegment {
            origin: SegmentOrigin::Clip {
                clip_id: clip.clip_id.clone(),
                source_id: clip.source_id.clone(),
            },
            color: clip.color,
            frames: projects::grid_tick(
                clip.range.end.checked_sub(clip.range.start)?,
                settings.frame_rate,
            )?
            .to_string(),
        });
        cursor = clip.range.end;
        if segments.len() > MAX_SEGMENTS {
            return Err(projects::limit("Media graph exceeds256 segments"));
        }
    }
    if cursor < capture.range.end {
        segments.push(VideoSegment {
            origin: SegmentOrigin::Gap,
            color: CornerColor::Black,
            frames: projects::grid_tick(
                capture.range.end.checked_sub(cursor)?,
                settings.frame_rate,
            )?
            .to_string(),
        });
    }
    if segments.is_empty() || segments.len() > MAX_SEGMENTS {
        return Err(projects::limit(
            "Media graph requires1..256 finite segments",
        ));
    }
    Ok((
        MediaGraph {
            color_profile: ColorProfile::OpaqueSrgbCornersBgr0V1,
            gap_policy: GapPolicy::OpaqueBlack,
            segments,
        },
        OutputManifest {
            intent: capture.output.clone(),
            video_frames: video_frames.to_string(),
            audio_samples: audio_samples.to_string(),
        },
    ))
}
// No public raw filter is executable authority: validation always regenerates this entire argv.
fn lower(capture: &Capture, graph: &MediaGraph, output: &OutputManifest) -> Vec<String> {
    let s = &capture.settings;
    let fps = format!(
        "{}/{}",
        s.frame_rate.numerator(),
        s.frame_rate.denominator()
    );
    let tb = format!(
        "{}/{}",
        s.frame_rate.denominator(),
        s.frame_rate.numerator()
    );
    let mut chains = Vec::new();
    let mut labels = String::new();
    for (index, segment) in graph.segments.iter().enumerate() {
        chains.push(format!("color=c=0x{}:s={}x{}:r={fps},format=pix_fmts=bgr0,trim=end_frame={},settb=expr={tb},setpts=N[v{index}]", segment.color.rgb(), s.width, s.height, segment.frames));
        labels.push_str(&format!("[v{index}]"));
    }
    chains.push(format!(
        "{labels}concat=n={}:v=1:a=0,settb=expr={tb},setpts=N[vout]",
        graph.segments.len()
    ));
    chains.push(format!("anullsrc=r={}:cl={},atrim=end_sample={},asetpts=N/SR/TB,aformat=sample_fmts=s16:sample_rates={}:channel_layouts={}[aout]", s.sample_rate, s.audio_layout.name(), output.audio_samples, s.sample_rate, s.audio_layout.name()));
    let mut args = [
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-v",
        "error",
        "-max_alloc",
        "67108864",
        "-filter_complex_threads",
        "1",
        "-filter_complex",
    ]
    .map(String::from)
    .to_vec();
    args.push(chains.join(";"));
    args.extend(
        [
            "-map",
            "[vout]",
            "-map",
            "[aout]",
            "-c:v",
            "ffv1",
            "-level",
            "3",
            "-threads:v",
            "1",
            "-pix_fmt",
            "bgr0",
            "-color_range",
            "pc",
            "-colorspace",
            "0",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "iec61966-2-1",
            "-fps_mode",
            "cfr",
            "-r:v",
        ]
        .map(String::from),
    );
    args.push(fps);
    args.push("-enc_time_base:v".into());
    args.push(tb);
    args.extend(
        [
            "-c:a",
            "pcm_s16le",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-f",
            "matroska",
            "-y",
            "@owned_staging@",
        ]
        .map(String::from),
    );
    args
}
impl MediaPlanV1 {
    /// Semantic digest rejects inconsistent typed graphs and edited lowering, even with a new SHA.
    pub fn digest(&self) -> Result<String> {
        if self.envelope_version != 1 {
            return Err(invalid("Unsupported media envelope version"));
        }
        projects::validate_id(&self.id)?;
        projects::validate_id(&self.payload.root_id)?;
        hash_text(&self.payload.application_build)?;
        if self.creator.is_empty()
            || self.creator.len() > 256
            || self.creator.chars().any(char::is_control)
            || self.payload.rights
                != (RequiredRights {
                    principal: self.creator.clone(),
                    project: "project.read".into(),
                    publication: "download.create".into(),
                })
            || self.payload.required_capability != "media_execution.v1"
            || self.payload.resources != resources()
        {
            return Err(invalid("Media rights/resource contract differs"));
        }
        let created = decimal(&self.created_at)?;
        if created.checked_add(300) != Some(decimal(&self.expires_at)?) {
            return Err(invalid("Media review expiry must be300 seconds"));
        }
        self.payload.tools.validate()?;
        let (graph, output) = derive(&self.payload.capture)?;
        if self.payload.graph != graph
            || self.payload.output != output
            || self.payload.argv_template != lower(&self.payload.capture, &graph, &output)
        {
            return Err(invalid(
                "Media graph/output/lowering differs from captured semantics",
            ));
        }
        if encode(self)?.len() > MAX_PLAN_BYTES {
            return Err(projects::limit("Serialized media plan exceeds128KiB"));
        }
        let mut framed = b"NumeraWiz.MediaPlan.v1\0".to_vec();
        framed.extend(encode(&self.payload)?);
        Ok(projects::hash(&framed))
    }
    pub fn validate_hash(&self) -> Result<()> {
        hash_text(&self.hash)?;
        if self.digest()? != self.hash {
            return Err(invalid("Media plan semantic hash mismatch"));
        }
        Ok(())
    }
}
pub fn compile(
    capture: Capture,
    tools: MediaTools,
    root_id: &str,
    creator: &str,
    application_build: &str,
    review_id: &str,
    now: u64,
) -> Result<MediaPlanV1> {
    let (graph, output) = derive(&capture)?;
    tools.validate()?;
    let argv_template = lower(&capture, &graph, &output);
    projects::validate_id(review_id)?;
    let mut plan = MediaPlanV1 {
        envelope_version: 1,
        payload_kind: PayloadKind::MediaV1,
        id: review_id.into(),
        creator: creator.into(),
        hash: "0".repeat(64),
        created_at: now.to_string(),
        expires_at: now
            .checked_add(300)
            .ok_or_else(|| projects::limit("Media review clock range exhausted"))?
            .to_string(),
        payload: MediaPayload {
            root_id: root_id.into(),
            application_build: application_build.into(),
            capture,
            tools,
            graph,
            output,
            rights: RequiredRights {
                principal: creator.into(),
                project: "project.read".into(),
                publication: "download.create".into(),
            },
            resources: resources(),
            required_capability: "media_execution.v1".into(),
            argv_template,
        },
    };
    plan.hash = plan.digest()?;
    plan.validate_hash()?;
    Ok(plan)
}
