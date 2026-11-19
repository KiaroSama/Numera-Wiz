//! Ordinary composable CLI commands lower to the existing canonical request port.
use crate::{client::NativeClient, print_response};
mod maintenance;
mod projects;
mod timeline;
use anyhow::Result;
use clap::{Args, Subcommand};
pub use maintenance::Maintenance;
use nw_domain::{
    DomainError,
    commands::{self, Request, Response},
    mux::{CleanIntent, Stream},
};
pub use projects::{Clips, History, Markers, Projects, Sequences, Tracks};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
pub use timeline::Timeline;

#[derive(Args)]
pub struct MutationIdentity {
    /// Retain this principal-bound scope when retrying the operation.
    #[arg(long)]
    replay_scope: String,
    /// Reuse this exact key and parameters after a lost response.
    #[arg(long)]
    idempotency_key: String,
    #[arg(long)]
    request_id: Option<String>,
}
impl MutationIdentity {
    fn apply(self, request: &mut Request) -> Result<()> {
        request.request_id = match self.request_id {
            Some(id) => id,
            None => {
                let bytes = serde_json::to_vec(&(
                    &request.command.id,
                    &self.replay_scope,
                    &self.idempotency_key,
                ))?;
                format!("cli-{}", hex::encode(Sha256::digest(bytes)))
            }
        };
        request.replay_scope = Some(self.replay_scope);
        request.idempotency_key = Some(self.idempotency_key);
        Ok(())
    }
}
#[derive(Args)]
pub struct Page {
    #[arg(long, default_value_t = 128)]
    limit: u32,
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long)]
    replace_snapshot: Option<String>,
}
impl Page {
    pub(crate) fn params(self) -> Value {
        json!({"limit":self.limit,"cursor":self.cursor,"replace_snapshot":self.replace_snapshot})
    }
}
#[derive(Args)]
pub struct ProbePolicy {
    #[arg(long, conflicts_with = "disable_probe_timeout")]
    probe_timeout_seconds: Option<u32>,
    #[arg(long)]
    disable_probe_timeout: bool,
}
impl ProbePolicy {
    fn apply(self, params: &mut Value) -> Result<()> {
        let policy = match (self.probe_timeout_seconds, self.disable_probe_timeout) {
            (Some(seconds), false) => {
                Some(nw_domain::execution::DeadlinePolicy::Finite { seconds })
            }
            (None, true) => Some(nw_domain::execution::DeadlinePolicy::Disabled),
            (None, false) => None,
            _ => {
                return Err(DomainError::new(
                    "VALIDATION_ERROR",
                    "Choose one explicit probe timeout policy",
                )
                .into());
            }
        };
        if let Some(policy) = policy {
            policy.duration()?;
            params["probe_timeout"] = serde_json::to_value(policy)?;
        }
        Ok(())
    }
}
#[derive(Subcommand)]
pub enum Media {
    /// Inspect an authorized media file without creating an asset or job.
    Probe {
        path: PathBuf,
        #[command(flatten)]
        probe: ProbePolicy,
    },
    /// Review an exact original-keyframe Matroska packet cut with chapters dropped.
    Cut {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        drop_chapters: bool,
    },
    /// Review H.264, HEVC or AV1 CPU export with optional AAC.
    Encode {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review a lossless FFV1 picture retime or reversal.
    Retime {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review a copy-only ordered join of compatible registered Matroska files.
    Join {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review multiple exact source ranges and their separate Matroska destinations.
    Split {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review burning registered SubRip captions with the offline font.
    BurnSubtitles {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review an ordered sample-domain edit for one original PCM audio stream.
    EditAudio {
        #[arg(long)]
        file: PathBuf,
    },
    /// Measure and review independent loudness normalization of selected PCM tracks.
    Normalize {
        #[arg(long)]
        file: PathBuf,
    },
    /// Inspect and refresh FFmpeg capabilities.
    #[command(subcommand)]
    Capabilities(Capabilities),
    /// Return one decoded preview frame from a registered video stream.
    PreviewFrame {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
    },
    /// Prepare a temporary browser-playable excerpt from a registered source.
    PreviewSegment {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        video_stream: u32,
        #[arg(long)]
        audio_stream: Option<u32>,
    },
    /// Measure peak and RMS for one registered audio stream.
    AnalyzeVolume {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
    },
    /// Measure EBU R128 integrated loudness and true peak for one registered track.
    AnalyzeLoudness {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
    },
    /// Inspect measured packet or decoded frame observations for a registered stream.
    AnalyzeStream {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
        #[arg(long, value_enum)]
        kind: ObservationKind,
        #[arg(long, default_value_t = 0)]
        first: u32,
        #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..=64))]
        limit: u32,
    },
    /// Compare matched video frame windows and report PSNR and SSIM.
    CompareVideo {
        #[arg(long)]
        reference: String,
        #[arg(long)]
        reference_stream: u32,
        #[arg(long, default_value_t = 0)]
        reference_start: u64,
        #[arg(long)]
        candidate: String,
        #[arg(long)]
        candidate_stream: u32,
        #[arg(long, default_value_t = 0)]
        candidate_start: u64,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
        frames: u32,
    },
    /// Compare decoded signal signatures for two or more original audio tracks.
    CompareAudio {
        #[arg(long)]
        asset: String,
        #[arg(long = "stream", required = true)]
        streams: Vec<u32>,
    },
}

#[derive(Subcommand)]
pub enum Previews {
    /// Observe a single registered source frame as a lossless 160 × 90 PNG.
    Observe {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=1_000_000))]
        frame: u32,
    },
    /// Observe the selected grid frame from an exact committed sequence revision.
    ObserveTimeline {
        #[arg(long)]
        project: String,
        #[arg(long)]
        sequence: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        at: String,
    },
    /// Observe a half-open interval of the selected registered PCM track.
    ObserveAudio {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
        #[arg(long)]
        start_sample: u32,
        #[arg(long)]
        end_sample: u32,
        #[arg(long)]
        sample_rate: u32,
        #[arg(long, value_parser = ["mono", "stereo"])]
        channel_layout: String,
    },
}
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ObservationKind {
    Packets,
    Frames,
}

#[derive(Subcommand)]
pub enum Capabilities {
    /// Read current component fingerprints and cached observations.
    Inspect,
    /// Inspect declarations and exercise a fixed software encode/decode path.
    Refresh {
        #[arg(long)]
        force: bool,
        /// Also run fresh probes of the default NVIDIA device with owner authority.
        #[arg(long)]
        devices: bool,
    },
    /// Clear the component cache entry matching this SHA-256 fingerprint.
    Clear {
        #[arg(long)]
        fingerprint: String,
    },
}
#[derive(Subcommand)]
pub enum Chapters {
    /// Inspect exact chapter times and tags.
    Inspect {
        #[arg(long)]
        asset: String,
    },
    /// Inspect original WAVE cue sample offsets, region lengths and labels.
    WaveCues {
        #[arg(long)]
        asset: String,
    },
    /// Return an FFmetadata document and its SHA-256 digest.
    Export {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        include_global_metadata: bool,
    },
    /// Review chapter replacement from a UTF-8 FFmetadata document.
    Import {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        ffmetadata: PathBuf,
        #[arg(long)]
        include_global_metadata: bool,
    },
    /// Review a stream-copy plan that removes chapters.
    Remove {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        out: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum Metadata {
    /// Review selected container or stream tag changes from a JSON patch.
    Patch {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        patch: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum Disposition {
    /// Review explicit changes to the flags on a retained audio or subtitle stream.
    Patch {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        stream: u32,
        #[arg(long)]
        add: Vec<String>,
        #[arg(long)]
        remove: Vec<String>,
        #[arg(long)]
        clear_all: bool,
        #[arg(long)]
        exclusive_default: bool,
    },
}
#[derive(Subcommand)]
pub enum Assets {
    /// Register a bounded folder snapshot, retaining individual probe failures.
    RegisterFolder {
        path: PathBuf,
        #[arg(long)]
        recursive: bool,
        #[arg(long = "exclude-relative")]
        exclude_relative: Vec<String>,
        #[arg(long, default_value = "inventory", value_parser = ["inventory", "exclude"])]
        companion_policy: String,
        #[command(flatten)]
        probe: ProbePolicy,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    FolderInspect {
        id: String,
    },
    Register {
        path: PathBuf,
        #[command(flatten)]
        probe: ProbePolicy,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    List {
        #[command(flatten)]
        page: Page,
    },
    Inspect {
        id: String,
    },
    /// Suggest a new output without creating it.
    Output {
        id: String,
    },
}
#[derive(Subcommand)]
pub enum Streams {
    /// Resolve exact original streams from bounded JSON documents.
    Resolve {
        #[arg(long)]
        streams_file: PathBuf,
        #[arg(long)]
        intent_file: PathBuf,
    },
    /// Prepare an immutable cleanup plan; submit it separately after review.
    Clean {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        intent_file: PathBuf,
        #[arg(long, value_parser = ["new", "preserve_best_effort", "require_reviewed"])]
        file_times: Option<String>,
        /// Explicit shared probe/operation policy document.
        #[arg(long)]
        limits_file: Option<PathBuf>,
    },
    /// Review explicit external audio and subtitle additions in strict-copy mode.
    Add {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review strict-copy removal, replacement or global metadata stripping.
    Replace {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review a single original stream's packet-copy extraction into Matroska.
    Extract {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        stream: u32,
        #[arg(long)]
        out: PathBuf,
    },
    /// Review a document of distinct Matroska outputs with per-source track selectors.
    ExtractBatch {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review one track ordinal per asset in a retained folder snapshot.
    ExtractFolder {
        #[arg(long)]
        file: PathBuf,
    },
    /// Preview candidate and retained stream counts for 1-64 assets.
    Report {
        #[arg(long = "asset", required = true)]
        assets: Vec<String>,
        #[arg(long)]
        intent_file: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum Plans {
    Inspect { id: String },
}
#[derive(Subcommand)]
pub enum Jobs {
    /// Review an admitted job from a versioned FFmWiz import report.
    Plan {
        #[arg(long)]
        asset: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Observe an accepted job; Ctrl+C stops waiting while the owner keeps the job.
    Wait {
        id: String,
        #[arg(long, default_value_t = 1800, value_parser = clap::value_parser!(u32).range(1..=86400))]
        timeout_seconds: u32,
    },
    Submit {
        #[arg(long)]
        plan: String,
        #[arg(long)]
        plan_hash: String,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    /// Accept the exact reviewed MediaV1 JSON using the common durable job service.
    SubmitMedia {
        #[arg(long)]
        plan_file: PathBuf,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    Inspect {
        id: String,
    },
    List {
        #[command(flatten)]
        page: Page,
    },
    Cancel {
        id: String,
        #[command(flatten)]
        identity: MutationIdentity,
    },
}
#[derive(Subcommand)]
pub enum Batches {
    List {
        #[command(flatten)]
        page: Page,
    },
    /// Preview an explicit bounded candidate document using the shared cleanup planner.
    Plan {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review H.264, HEVC or AV1 CPU encoding from a retained folder snapshot.
    EncodeFolder {
        #[arg(long)]
        file: PathBuf,
    },
    /// Review cleanup from a retained folder snapshot and a separate existing directory.
    PlanFolder {
        #[arg(long)]
        file: PathBuf,
    },
    PlanInspect {
        id: String,
    },
    Submit {
        #[arg(long)]
        plan: String,
        #[arg(long)]
        plan_hash: String,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    Inspect {
        id: String,
    },
    Cancel {
        id: String,
        #[command(flatten)]
        identity: MutationIdentity,
    },
    /// Observe an accepted batch; Ctrl+C stops waiting and leaves accepted jobs running.
    Wait {
        id: String,
    },
}
#[derive(Subcommand)]
pub enum Logs {
    Status,
}
#[derive(Subcommand)]
pub enum Discovery {
    List,
    Schema { id: String },
}
pub enum Command {
    Projects(Projects),
    Sequences(Sequences),
    Timeline(Timeline),
    Clips(Clips),
    Tracks(Tracks),
    Markers(Markers),
    History(History),
    Media(Media),
    Previews(Previews),
    Chapters(Chapters),
    Metadata(Metadata),
    Disposition(Disposition),
    Assets(Assets),
    Streams(Streams),
    Plans(Plans),
    Jobs(Jobs),
    Batches(Batches),
    Logs(Logs),
    Maintenance(Maintenance),
    Discovery(Option<Discovery>),
}
fn native_path(path: &Path) -> Result<String> {
    Ok(nw_platform::files::path_text(&std::path::absolute(path)?)?)
}
fn document(path: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((commands::MAX_COMMAND_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > commands::MAX_COMMAND_BYTES {
        return Err(DomainError::new("RESOURCE_LIMIT", "CLI document exceeds 1 MiB").into());
    }
    Ok(commands::decode_json(&bytes)?)
}
fn intent(path: &Path) -> Result<Value> {
    let value: CleanIntent = commands::parse(document(path)?)?;
    Ok(serde_json::to_value(value)?)
}
fn query(id: &str, params: Value) -> Request {
    let mut request = Request::query(id, params);
    request.request_id = format!("cli-{}", uuid::Uuid::new_v4());
    request
}
pub fn prepare(command: Command) -> Result<Request> {
    let request = match command {
        Command::Projects(command) => command.prepare()?,
        Command::Sequences(command) => command.prepare()?,
        Command::Timeline(command) => command.prepare()?,
        Command::Clips(command) => command.prepare()?,
        Command::Tracks(command) => command.prepare()?,
        Command::Markers(command) => command.prepare()?,
        Command::History(command) => command.prepare()?,
        Command::Chapters(Chapters::Inspect { asset }) => {
            query("chapters.inspect", json!({"id":asset}))
        }
        Command::Chapters(Chapters::WaveCues { asset }) => {
            query("chapters.wave_cues", json!({"id":asset}))
        }
        Command::Chapters(Chapters::Export {
            asset,
            include_global_metadata,
        }) => query(
            "chapters.export",
            json!({"asset_id":asset,"include_global_metadata":include_global_metadata}),
        ),
        Command::Chapters(Chapters::Remove { asset, out }) => query(
            "chapters.remove",
            json!({"asset_id":asset,"output":native_path(&out)?}),
        ),
        Command::Chapters(Chapters::Import {
            asset,
            out,
            ffmetadata,
            include_global_metadata,
        }) => {
            let limit = nw_domain::chapters::MAX_TEXT_BYTES;
            let mut bytes = Vec::new();
            std::fs::File::open(ffmetadata)?
                .take((limit + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > limit {
                return Err(DomainError::new(
                    "RESOURCE_LIMIT",
                    "FFmetadata document exceeds 64 KiB",
                )
                .into());
            }
            let ffmetadata = String::from_utf8(bytes).map_err(|_| {
                DomainError::new("INVALID_CHAPTERS", "FFmetadata document must be UTF-8")
            })?;
            query(
                "chapters.import",
                json!({"asset_id":asset,"output":native_path(&out)?,"ffmetadata":ffmetadata,"include_global_metadata":include_global_metadata}),
            )
        }
        Command::Metadata(Metadata::Patch { asset, out, patch }) => {
            let patch: nw_domain::metadata::Patch = commands::parse(document(&patch)?)?;
            query(
                "metadata.patch",
                json!({"asset_id":asset,"output":native_path(&out)?,"patch":patch}),
            )
        }
        Command::Disposition(Disposition::Patch {
            asset,
            out,
            stream,
            add,
            remove,
            clear_all,
            exclusive_default,
        }) => {
            let patch = nw_domain::dispositions::Patch {
                stream,
                add,
                remove,
                clear_all,
                exclusive_default,
            };
            query(
                "disposition.patch",
                json!({"asset_id":asset,"output":native_path(&out)?,"patch":patch}),
            )
        }
        Command::Media(Media::Probe { path, probe }) => {
            let mut params = json!({"path":native_path(&path)?});
            probe.apply(&mut params)?;
            query("media.probe", params)
        }
        Command::Media(Media::BurnSubtitles { file }) => {
            let mut input: nw_domain::subtitle_burn::BurnInput = commands::parse(document(&file)?)?;
            input.audio.validate()?;
            input.color.validate()?;
            input.output = native_path(Path::new(&input.output))?;
            query("subtitles.burn", serde_json::to_value(input)?)
        }
        Command::Media(Media::EditAudio { file }) => {
            let mut input: nw_domain::audio_edit::EditInput = commands::parse(document(&file)?)?;
            input.output = native_path(Path::new(&input.output))?;
            query("audio.edit", serde_json::to_value(input)?)
        }
        Command::Media(Media::Encode { file }) => {
            let mut input: nw_domain::avc_export::EncodeInput = commands::parse(document(&file)?)?;
            input.settings.validate()?;
            input.output = native_path(Path::new(&input.output))?;
            query("media.encode", serde_json::to_value(input)?)
        }
        Command::Media(Media::Retime { file }) => {
            let mut input: nw_domain::video_retime::RetimeInput =
                commands::parse(document(&file)?)?;
            input.output = native_path(Path::new(&input.output))?;
            query("media.retime", serde_json::to_value(input)?)
        }
        Command::Media(Media::Join { file }) => {
            let mut input: nw_domain::media_join::JoinInput = commands::parse(document(&file)?)?;
            input.validate()?;
            input.output = native_path(Path::new(&input.output))?;
            query("media.join", serde_json::to_value(input)?)
        }
        Command::Media(Media::Split { file }) => {
            let mut input: nw_domain::packet_cut::SplitInput = commands::parse(document(&file)?)?;
            input.validate()?;
            for item in &mut input.outputs {
                item.output = native_path(Path::new(&item.output))?;
            }
            query("media.split_plan", serde_json::to_value(input)?)
        }
        Command::Media(Media::Normalize { file }) => {
            let mut input: nw_domain::audio_normalization::NormalizeInput =
                commands::parse(document(&file)?)?;
            nw_domain::audio_normalization::validate_requests(&input.tracks)?;
            input.output = native_path(Path::new(&input.output))?;
            query("audio.normalize", serde_json::to_value(input)?)
        }
        Command::Media(Media::Cut {
            asset,
            start,
            end,
            out,
            drop_chapters,
        }) => {
            if !drop_chapters {
                return Err(anyhow::anyhow!(
                    "Select --drop-chapters for this packet-copy policy"
                ));
            }
            let range = nw_domain::time::TimeRange {
                start: projects::rational(&start)?,
                end: projects::rational(&end)?,
            };
            nw_domain::packet_cut::validate(range)?;
            query(
                "media.cut",
                json!(nw_domain::packet_cut::CutInput {
                    asset_id: asset,
                    range,
                    output: native_path(&out)?,
                    chapters: nw_domain::packet_cut::ChapterPolicy::Drop,
                }),
            )
        }
        Command::Media(Media::PreviewFrame {
            asset,
            stream,
            frame,
        }) => query(
            "media.preview_frame",
            json!({"asset_id":asset,"stream_index":stream,"frame":frame}),
        ),
        Command::Previews(Previews::Observe {
            asset,
            stream,
            frame,
        }) => query(
            nw_domain::preview::OBSERVE_COMMAND,
            serde_json::to_value(nw_domain::preview::ObserveInput::source(
                asset, stream, frame,
            ))?,
        ),
        Command::Previews(Previews::ObserveTimeline {
            project,
            sequence,
            revision,
            at,
        }) => {
            let input = nw_domain::preview::ObserveInput::timeline(
                nw_domain::projects::ProjectId::parse(project)?,
                nw_domain::projects::EntityId::parse(sequence)?,
                nw_domain::projects::Revision::parse(&revision)?,
                projects::rational(&at)?,
            );
            query(
                nw_domain::preview::OBSERVE_COMMAND,
                serde_json::to_value(input)?,
            )
        }
        Command::Previews(Previews::ObserveAudio {
            asset,
            stream,
            start_sample,
            end_sample,
            sample_rate,
            channel_layout,
        }) => {
            use nw_domain::{
                media::AudioLayout,
                preview::{
                    AudioEncoding, AudioObservationInput, AudioTarget, Delivery, OBSERVE_COMMAND,
                    ObserveInput, SampleRange, SourcePolicy,
                },
            };
            let channel_layout = match channel_layout.as_str() {
                "mono" => AudioLayout::Mono,
                "stereo" => AudioLayout::Stereo,
                _ => {
                    return Err(
                        DomainError::new("VALIDATION_ERROR", "Choose mono or stereo").into(),
                    );
                }
            };
            let input = AudioObservationInput {
                target: AudioTarget {
                    asset_id: asset,
                    stream_index: stream,
                },
                range: SampleRange {
                    start: start_sample,
                    end: end_sample,
                },
                sample_rate,
                channel_layout,
                source_policy: SourcePolicy::OriginalRequired,
                encoding: AudioEncoding::WavePcm16 {},
                delivery: Delivery::Inline,
            };
            input.frames()?;
            query(
                OBSERVE_COMMAND,
                serde_json::to_value(ObserveInput::Audio(input))?,
            )
        }
        Command::Media(Media::PreviewSegment {
            asset,
            video_stream,
            audio_stream,
        }) => query(
            "previews.source_segment",
            json!(nw_domain::preview::SourceSegmentInput {
                asset_id: asset,
                video_stream,
                audio_stream
            }),
        ),
        Command::Media(Media::AnalyzeVolume { asset, stream }) => query(
            "media.analyze.volume",
            json!({"asset_id":asset,"stream_index":stream}),
        ),
        Command::Media(Media::AnalyzeLoudness { asset, stream }) => query(
            "media.analyze.loudness",
            json!({"asset_id":asset,"stream_index":stream}),
        ),
        Command::Media(Media::AnalyzeStream {
            asset,
            stream,
            kind,
            first,
            limit,
        }) => {
            use nw_domain::stream_observations::{Input, Kind};
            let kind = match kind {
                ObservationKind::Packets => Kind::Packets,
                ObservationKind::Frames => Kind::Frames,
            };
            query(
                "media.analyze.stream",
                serde_json::to_value(Input {
                    asset_id: asset,
                    stream_index: stream,
                    kind,
                    first,
                    limit,
                })?,
            )
        }
        Command::Media(Media::CompareVideo {
            reference,
            reference_stream,
            reference_start,
            candidate,
            candidate_stream,
            candidate_start,
            frames,
        }) => {
            use nw_domain::reference_metrics::{CompareInput, Window};
            let input = CompareInput {
                reference: Window {
                    asset_id: reference,
                    stream_index: reference_stream,
                    start_frame: reference_start,
                },
                candidate: Window {
                    asset_id: candidate,
                    stream_index: candidate_stream,
                    start_frame: candidate_start,
                },
                frame_count: frames,
            };
            input.frame_ranges()?;
            query("media.analyze.reference", serde_json::to_value(input)?)
        }
        Command::Media(Media::CompareAudio { asset, streams }) => query(
            "media.analyze.audio_tracks",
            json!({"asset_id":asset,"stream_indices":streams}),
        ),
        Command::Media(Media::Capabilities(Capabilities::Inspect)) => {
            query("capabilities.inspect", json!({"scope":"components"}))
        }
        Command::Media(Media::Capabilities(Capabilities::Refresh { force, devices })) => query(
            "capabilities.refresh",
            json!({"force":force,"devices":devices,"components":true}),
        ),
        Command::Media(Media::Capabilities(Capabilities::Clear { fingerprint })) => query(
            "capabilities.clear",
            json!({"namespace":"components","fingerprint":fingerprint}),
        ),
        Command::Assets(Assets::Register {
            path,
            probe,
            identity,
        }) => {
            let mut params = json!({"path":native_path(&path)?});
            probe.apply(&mut params)?;
            let mut request = query("assets.register", params);
            identity.apply(&mut request)?;
            request
        }
        Command::Assets(Assets::RegisterFolder {
            path,
            recursive,
            exclude_relative,
            companion_policy,
            probe,
            identity,
        }) => {
            let mut params = json!({"path":native_path(&path)?,"recursive":recursive,"exclude_relative":exclude_relative,"companion_policy":companion_policy});
            probe.apply(&mut params)?;
            let input: nw_domain::ingest::RegisterFolderInput = commands::parse(params.clone())?;
            input.validate()?;
            let mut request = query("assets.register_folder", params);
            identity.apply(&mut request)?;
            request
        }
        Command::Assets(Assets::FolderInspect { id }) => {
            query("assets.folder_inspect", json!({"id":id}))
        }
        Command::Assets(Assets::List { page }) => query("assets.list", page.params()),
        Command::Assets(Assets::Inspect { id }) => query("assets.inspect", json!({"id":id})),
        Command::Assets(Assets::Output { id }) => query("assets.output_suggest", json!({"id":id})),
        Command::Streams(Streams::Resolve {
            streams_file,
            intent_file,
        }) => {
            let streams: Vec<Stream> = commands::parse(document(&streams_file)?)?;
            query(
                "streams.clean.resolve",
                json!({"streams":streams,"intent":intent(&intent_file)?}),
            )
        }
        Command::Streams(Streams::Clean {
            asset,
            out,
            intent_file,
            file_times,
            limits_file,
        }) => {
            let mut params = json!({"asset_id":asset,"output":native_path(&out)?,"intent":intent(&intent_file)?});
            if let Some(path) = limits_file {
                let limits: nw_domain::execution::CleanupLimits =
                    commands::parse(document(&path)?)?;
                limits.validate()?;
                params["execution_limits"] = serde_json::to_value(limits)?;
            }
            if let Some(policy) = file_times {
                let policy: nw_domain::file_times::FileTimeRequest =
                    commands::parse(json!(policy))?;
                params["file_times"] = serde_json::to_value(policy)?;
            }
            query("streams.clean", params)
        }
        Command::Streams(Streams::Add { file }) => {
            let mut input: nw_domain::track_append::AppendInput =
                commands::parse(document(&file)?)?;
            input.output = native_path(Path::new(&input.output))?;
            query("streams.add", serde_json::to_value(input)?)
        }
        Command::Streams(Streams::Replace { file }) => {
            let mut input: nw_domain::track_append::ReplaceInput =
                commands::parse(document(&file)?)?;
            input.validate()?;
            input.output = native_path(Path::new(&input.output))?;
            query("streams.replace", serde_json::to_value(input)?)
        }
        Command::Streams(Streams::Extract { asset, stream, out }) => query(
            "streams.extract",
            json!({"asset_id":asset,"stream_index":stream,"output":native_path(&out)?}),
        ),
        Command::Streams(Streams::ExtractBatch { file }) => {
            let mut input: nw_domain::batches::ExtractBatchInput =
                commands::parse(document(&file)?)?;
            input.validate()?;
            for item in &mut input.items {
                item.target.output = native_path(Path::new(&item.target.output))?;
            }
            query("streams.extract.batch_plan", serde_json::to_value(input)?)
        }
        Command::Streams(Streams::ExtractFolder { file }) => {
            let mut input: nw_domain::batches::ExtractFolderBatchInput =
                commands::parse(document(&file)?)?;
            input.validate()?;
            input.output_root = native_path(Path::new(&input.output_root))?;
            query(
                "streams.extract.folder_batch_plan",
                serde_json::to_value(input)?,
            )
        }
        Command::Streams(Streams::Report {
            assets,
            intent_file,
        }) => query(
            "streams.clean.report",
            json!({"asset_ids":assets,"intent":intent(&intent_file)?}),
        ),
        Command::Plans(Plans::Inspect { id }) => query("plans.inspect", json!({"id":id})),
        Command::Jobs(Jobs::Plan { asset, file }) => {
            let document = document(&file)?;
            let job = if let Some(job) = document.get("job") {
                if document["input_valid"] != true {
                    return Err(DomainError::new(
                        "INVALID_JOB_DOCUMENT",
                        "Resolve the import errors before review",
                    )
                    .into());
                }
                job.clone()
            } else {
                document
            };
            let job: nw_domain::config_import::ImportedJob = commands::parse(job)?;
            job.copy_paths()?;
            query("jobs.plan", json!({"asset_id":asset,"job_document":job}))
        }
        Command::Jobs(Jobs::Submit {
            plan,
            plan_hash,
            identity,
        }) => {
            let mut request = query("jobs.submit", json!({"plan_id":plan,"plan_hash":plan_hash}));
            identity.apply(&mut request)?;
            request
        }
        Command::Jobs(Jobs::SubmitMedia {
            plan_file,
            identity,
        }) => {
            let mut bytes = Vec::new();
            std::fs::File::open(plan_file)?
                .take((nw_domain::media::MAX_PLAN_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > nw_domain::media::MAX_PLAN_BYTES {
                return Err(
                    DomainError::new("RESOURCE_LIMIT", "Media review exceeds 128 KiB").into(),
                );
            }
            let raw = String::from_utf8(bytes)
                .map_err(|_| DomainError::new("VALIDATION_ERROR", "Media review must be UTF-8"))?;
            nw_domain::media_execution::decode_reviewed_media_plan(&raw)?;
            let mut request = query("jobs.submit", json!({"media_plan_json":raw}));
            identity.apply(&mut request)?;
            request
        }
        Command::Jobs(Jobs::Inspect { id } | Jobs::Wait { id, .. }) => {
            query("jobs.inspect", json!({"id":id}))
        }
        Command::Jobs(Jobs::List { page }) => query("jobs.list", page.params()),
        Command::Jobs(Jobs::Cancel { id, identity }) => {
            let mut request = query("jobs.cancel", json!({"id":id}));
            identity.apply(&mut request)?;
            request
        }
        Command::Batches(Batches::List { page }) => query("batches.list", page.params()),
        Command::Batches(Batches::Plan { file }) => {
            let mut input: nw_domain::batches::CleanBatchInput = commands::parse(document(&file)?)?;
            input.validate()?;
            for item in &mut input.items {
                item.output = native_path(Path::new(&item.output))?;
            }
            query("streams.clean.batch_plan", serde_json::to_value(input)?)
        }
        Command::Batches(Batches::EncodeFolder { file }) => {
            let mut input: nw_domain::avc_export::FolderEncodeInput =
                commands::parse(document(&file)?)?;
            input.output_root = native_path(Path::new(&input.output_root))?;
            nw_domain::batches::validate_folder_reference(
                &input.snapshot_id,
                &input.snapshot_hash,
                &input.output_root,
            )?;
            query("media.encode_folder_plan", serde_json::to_value(input)?)
        }
        Command::Batches(Batches::PlanFolder { file }) => {
            let mut input: nw_domain::batches::CleanFolderBatchInput =
                commands::parse(document(&file)?)?;
            input.validate()?;
            input.output_root = native_path(Path::new(&input.output_root))?;
            query(
                "streams.clean.folder_batch_plan",
                serde_json::to_value(input)?,
            )
        }
        Command::Batches(Batches::PlanInspect { id }) => {
            query("batches.plan_inspect", json!({"id":id}))
        }
        Command::Batches(Batches::Inspect { id } | Batches::Wait { id }) => {
            query("batches.inspect", json!({"id":id}))
        }
        Command::Batches(Batches::Submit {
            plan,
            plan_hash,
            identity,
        }) => {
            let mut request = query(
                "batches.submit",
                json!({"plan_id":plan,"plan_hash":plan_hash}),
            );
            identity.apply(&mut request)?;
            request
        }
        Command::Batches(Batches::Cancel { id, identity }) => {
            let mut request = query("batches.cancel", json!({"id":id}));
            identity.apply(&mut request)?;
            request
        }
        Command::Maintenance(command) => command.prepare()?,
        Command::Logs(Logs::Status) => query("logs.status", json!({})),
        Command::Discovery(None | Some(Discovery::List)) => query("commands.list", json!({})),
        Command::Discovery(Some(Discovery::Schema { id })) => {
            query("commands.schema", json!({"id":id}))
        }
    };
    // Validate the same strict envelope and aggregate request budget before transport.
    Ok(commands::decode_request(&serde_json::to_vec(&request)?)?)
}
pub async fn run(client: &NativeClient, command: Command) -> Result<()> {
    if let Command::Jobs(Jobs::Wait {
        id,
        timeout_seconds,
    }) = &command
    {
        return wait_job(client, id, *timeout_seconds).await;
    }
    if let Command::Batches(Batches::Wait { id }) = &command {
        return wait_batch(client, id).await;
    }
    let interchange_destination = match &command {
        Command::Timeline(Timeline::ExportEdl(args) | Timeline::ExportOtio(args)) => {
            args.out.clone()
        }
        _ => None,
    };
    match prepare(command) {
        Ok(request) => {
            if let Some(path) = interchange_destination {
                return timeline::save_interchange(client, request, &path).await;
            }
            print_response(client, request).await
        }
        Err(error) => {
            let public = error
                .downcast_ref::<DomainError>()
                .cloned()
                .unwrap_or_else(|| {
                    DomainError::new(
                        "CLI_INPUT_ERROR",
                        "CLI input document or native path is unavailable",
                    )
                });
            println!(
                "{}",
                serde_json::to_string_pretty(&Response::error("human-cli", public))?
            );
            Err(error)
        }
    }
}

async fn wait_job(client: &NativeClient, id: &str, timeout_seconds: u32) -> Result<()> {
    use nw_domain::records::JobState;
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(timeout_seconds));
    let outcome = async {
        loop {
            let mut response = client
                .request(&query("jobs.inspect", json!({"id":id})))
                .await?;
            if !response.ok {
                let error = response.error.clone().unwrap_or_else(|| {
                    DomainError::new("OBJECT_NOT_FOUND", "Job observation is unavailable")
                });
                response.error = Some(error);
                return Ok::<_, anyhow::Error>(response);
            }
            if let Some(media) = response
                .result
                .as_ref()
                .filter(|result| result["payload_kind"] == "media_job_v1")
            {
                if media["envelope_version"] != 1 || media["id"] != id {
                    return Err(DomainError::new(
                        "INVALID_RECORD",
                        "Media job observation identity differs",
                    )
                    .into());
                }
                let state: nw_domain::media_execution::MediaJobState =
                    commands::parse(media["state"].clone())?;
                if state.terminal() {
                    let error = match state {
                        nw_domain::media_execution::MediaJobState::Succeeded => None,
                        nw_domain::media_execution::MediaJobState::Cancelled => Some(
                            DomainError::new("CANCELLED", "Accepted media job was cancelled"),
                        ),
                        _ => Some(DomainError::new(
                            media["error"]["code"].as_str().unwrap_or("JOB_FAILED"),
                            "Accepted media job did not succeed",
                        )),
                    };
                    response.ok = error.is_none();
                    response.error = error;
                    return Ok(response);
                }
                tracing::debug!(job_id = id, ?state, "job.wait.progress");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
            let job: nw_domain::views::JobView =
                commands::parse(response.result.clone().ok_or_else(|| {
                    DomainError::new("INVALID_RECORD", "Job observation omitted its result")
                })?)?;
            if job.state.terminal() {
                let error = match job.state {
                    JobState::Succeeded => None,
                    JobState::Cancelled => {
                        Some(DomainError::new("CANCELLED", "Accepted job was cancelled"))
                    }
                    JobState::Skipped => Some(DomainError::new(
                        "CAPABILITY_UNAVAILABLE",
                        "Job was skipped and produced no verified output",
                    )),
                    _ => Some(match job.error {
                        Some(error) => DomainError::new(
                            &error.code,
                            error
                                .message
                                .unwrap_or_else(|| "Accepted job did not succeed".into()),
                        ),
                        None => DomainError::new("JOB_FAILED", "Accepted job did not succeed"),
                    }),
                };
                response.ok = error.is_none();
                response.error = error.clone();
                return Ok(response);
            }
            tracing::debug!(job_id = id, state = ?job.state, "job.wait.progress");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    };
    let error: anyhow::Error = tokio::select! {
        result = outcome => match result {
            Ok(response) => {
                let error = response.error.clone();
                println!("{}", serde_json::to_string_pretty(&response)?);
                return error.map_or(Ok(()), |error| Err(error.into()));
            }
            Err(error) => error,
        },
        _ = tokio::time::sleep_until(deadline) => DomainError::new("TIMEOUT", "Stopped waiting at the selected deadline; the accepted job remains owned by the worker").into(),
        signal = tokio::signal::ctrl_c() => {
            signal?;
            DomainError::new("CANCELLED", "Stopped waiting; the accepted job remains owned by the worker").into()
        }
    };
    let public = error
        .downcast_ref::<DomainError>()
        .cloned()
        .unwrap_or_else(|| {
            DomainError::new(
                "TRANSPORT_ERROR",
                "Job observation failed; accepted work remains owned by the worker",
            )
        });
    println!(
        "{}",
        serde_json::to_string_pretty(&Response::error("job-wait", public))?
    );
    Err(error)
}

async fn wait_batch(client: &NativeClient, id: &str) -> Result<()> {
    loop {
        let request = query("batches.inspect", json!({"id":id}));
        let mut response = tokio::select! {
            response = client.request(&request) => match response {
                Ok(response) => response,
                Err(error) => {
                    let public = error.downcast_ref::<DomainError>().cloned().unwrap_or_else(|| DomainError::new("TRANSPORT_ERROR", "Batch observation connection failed; accepted jobs remain owned by the worker"));
                    println!("{}", serde_json::to_string_pretty(&Response::error("batch-wait", public.clone()))?);
                    return Err(public.into());
                }
            },
            signal = tokio::signal::ctrl_c() => {
                signal?;
                let error = DomainError::new("CANCELLED", "Stopped waiting; accepted batch jobs continue");
                println!("{}", serde_json::to_string_pretty(&Response::error("batch-wait", error.clone()))?);
                return Err(error.into());
            }
        };
        if !response.ok {
            println!("{}", serde_json::to_string_pretty(&response)?);
            return Err(response
                .error
                .unwrap_or_else(|| DomainError::new("BATCH_UNAVAILABLE", "Batch inspection failed"))
                .into());
        }
        let result: nw_domain::batches::BatchResult =
            commands::parse(response.result.clone().ok_or_else(|| {
                DomainError::new("INVALID_BATCH", "Batch inspection omitted its result")
            })?)?;
        if result.complete {
            let error = if result.failed == 0 && result.interrupted == 0 && result.cancelled == 0 {
                None
            } else if result.failed == 0
                && result.interrupted == 0
                && result.succeeded == 0
                && result.cancelled > 0
            {
                Some(DomainError::new(
                    "CANCELLED",
                    "Batch cancellation completed without a published success",
                ))
            } else {
                Some(DomainError::new(
                    "PARTIAL_BATCH_FAILURE",
                    "Batch completed with failed, interrupted or cancelled candidates",
                ))
            };
            response.ok = error.is_none();
            response.error = error.clone();
            println!("{}", serde_json::to_string_pretty(&response)?);
            return error.map_or(Ok(()), |error| Err(error.into()));
        }
        tracing::debug!(
            batch_id = id,
            finished = result.finished,
            total = result.total,
            "batch.wait.progress"
        );
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
            signal = tokio::signal::ctrl_c() => {
                signal?;
                let error = DomainError::new("CANCELLED", "Stopped waiting; accepted batch jobs continue");
                println!("{}", serde_json::to_string_pretty(&Response::error("batch-wait", error.clone()))?);
                return Err(error.into());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_cli_retry_keeps_identity_and_never_allocates_a_fresh_scope() {
        let make = || {
            Command::Jobs(Jobs::Submit {
                plan: "reviewed-plan".into(),
                plan_hash: "ab".repeat(32),
                identity: MutationIdentity {
                    replay_scope: "retained-scope".into(),
                    idempotency_key: "same-operation".into(),
                    request_id: None,
                },
            })
        };
        let first = prepare(make()).unwrap();
        let second = prepare(make()).unwrap();
        assert_eq!(first.request_id, second.request_id);
        assert_eq!(first.replay_scope.as_deref(), Some("retained-scope"));
        assert_eq!(first.idempotency_key.as_deref(), Some("same-operation"));
        assert_eq!(first.params, second.params);
        let wait = prepare(Command::Jobs(Jobs::Wait {
            id: "accepted-job".into(),
            timeout_seconds: 30,
        }))
        .unwrap();
        assert_eq!(wait.command.id, "jobs.inspect");
        assert_eq!(wait.params, json!({"id":"accepted-job"}));
        assert!(wait.idempotency_key.is_none() && wait.replay_scope.is_none());
        use clap::Parser;
        #[derive(Parser)]
        struct ProjectCli {
            #[command(subcommand)]
            command: Projects,
        }
        let id = "11111111-1111-4111-8111-111111111111";
        let inspected = ProjectCli::try_parse_from(["test", "inspect", "--project", id]).unwrap();
        let inspected = prepare(Command::Projects(inspected.command)).unwrap();
        assert_eq!(inspected.command.id, "projects.inspect");
        assert_eq!(inspected.project.as_ref().unwrap().id, id);
        assert!(
            serde_json::to_value(&inspected).unwrap()["project"]
                .get("expected_revision")
                .is_none()
        );
        #[derive(Parser)]
        struct ClipCli {
            #[command(subcommand)]
            command: Clips,
        }
        let make_move = || {
            ClipCli::try_parse_from([
                "test",
                "move",
                "--project",
                id,
                "--expected-revision",
                "4",
                "--clip",
                id,
                "--delta",
                "-1/4",
                "--replay-scope",
                "retained-scope",
                "--idempotency-key",
                "same-move",
            ])
            .unwrap()
            .command
        };
        let moved = prepare(Command::Clips(make_move())).unwrap();
        let retry = prepare(Command::Clips(make_move())).unwrap();
        assert_eq!(moved.structural_digest(), retry.structural_digest());
        assert_eq!(moved.request_id, retry.request_id);
        assert_eq!(
            moved.project.as_ref().unwrap().expected_revision.as_deref(),
            Some("4")
        );
        assert_eq!(moved.params["delta"], json!({"num":"-1","den":"4"}));
        let listed = ProjectCli::try_parse_from([
            "test",
            "list",
            "--limit",
            "64",
            "--replace-snapshot",
            "old-ticket",
        ])
        .unwrap();
        let listed = prepare(Command::Projects(listed.command)).unwrap();
        assert_eq!(listed.command.id, "projects.list");
        assert_eq!(
            listed.params,
            json!({"limit":64,"cursor":null,"replace_snapshot":"old-ticket"})
        );
        assert!(listed.project.is_none());
        assert!(listed.replay_scope.is_none() && listed.idempotency_key.is_none());
        let continued = ProjectCli::try_parse_from([
            "test",
            "list",
            "--limit",
            "1",
            "--cursor",
            "retained-cursor",
        ])
        .unwrap();
        assert_eq!(
            prepare(Command::Projects(continued.command))
                .unwrap()
                .params,
            json!({"limit":1,"cursor":"retained-cursor","replace_snapshot":null})
        );
        assert!(ProjectCli::try_parse_from(["test", "list", "--project", id]).is_err());
        let partner = "22222222-2222-4222-8222-222222222222";
        let make_link = || {
            ClipCli::try_parse_from([
                "test",
                "link",
                "--project",
                id,
                "--expected-revision",
                "5",
                "--clip",
                id,
                "--clip",
                partner,
                "--replay-scope",
                "retained-scope",
                "--idempotency-key",
                "same-link",
                "--request-id",
                "supplied-link-request",
            ])
            .unwrap()
            .command
        };
        let linked = prepare(Command::Clips(make_link())).unwrap();
        let retry = prepare(Command::Clips(make_link())).unwrap();
        assert_eq!(linked.command.id, "clips.link");
        assert_eq!(linked.command.version, 1);
        assert_eq!(linked.project.as_ref().unwrap().id, id);
        assert_eq!(
            linked
                .project
                .as_ref()
                .unwrap()
                .expected_revision
                .as_deref(),
            Some("5")
        );
        assert_eq!(
            linked.params,
            json!({"clip_ids":[id,partner],"link_policy":"expand"})
        );
        assert_eq!(linked.request_id, "supplied-link-request");
        assert_eq!(linked.replay_scope.as_deref(), Some("retained-scope"));
        assert_eq!(linked.idempotency_key.as_deref(), Some("same-link"));
        assert_eq!(linked.request_id, retry.request_id);
        assert_eq!(linked.structural_digest(), retry.structural_digest());
        assert!(
            ClipCli::try_parse_from([
                "test",
                "link",
                "--project",
                id,
                "--expected-revision",
                "5",
                "--replay-scope",
                "retained-scope",
                "--idempotency-key",
                "no-clip",
            ])
            .is_err()
        );

        let make_unlink = || {
            ClipCli::try_parse_from([
                "test",
                "unlink",
                "--project",
                id,
                "--expected-revision",
                "6",
                "--clip",
                id,
                "--clip",
                partner,
                "--replay-scope",
                "retained-scope",
                "--idempotency-key",
                "same-unlink",
                "--request-id",
                "supplied-unlink-request",
            ])
            .unwrap()
            .command
        };
        let unlinked = prepare(Command::Clips(make_unlink())).unwrap();
        let retry = prepare(Command::Clips(make_unlink())).unwrap();
        assert_eq!(unlinked.command.id, "clips.unlink");
        assert_eq!(unlinked.command.version, 1);
        assert_eq!(unlinked.project.as_ref().unwrap().id, id);
        assert_eq!(
            unlinked
                .project
                .as_ref()
                .unwrap()
                .expected_revision
                .as_deref(),
            Some("6")
        );
        assert_eq!(
            unlinked.params,
            json!({"clip_ids":[id,partner],"link_policy":"expand"})
        );
        assert_eq!(unlinked.request_id, "supplied-unlink-request");
        assert_eq!(unlinked.replay_scope.as_deref(), Some("retained-scope"));
        assert_eq!(unlinked.idempotency_key.as_deref(), Some("same-unlink"));
        assert_eq!(unlinked.request_id, retry.request_id);
        assert_eq!(unlinked.structural_digest(), retry.structural_digest());
        assert_ne!(unlinked.structural_digest(), linked.structural_digest());
        assert!(
            ClipCli::try_parse_from([
                "test",
                "unlink",
                "--project",
                id,
                "--expected-revision",
                "6",
                "--replay-scope",
                "retained-scope",
                "--idempotency-key",
                "no-unlink-clip",
            ])
            .is_err()
        );
    }
}
