//! Typed human project verbs lower to the same envelope used by HTTP and MCP.
use super::*;
use nw_domain::{projects as model, time::Rational};

#[derive(Args)]
pub struct ReadContext {
    #[arg(long)] pub project: String,
    #[arg(long)] pub revision: Option<String>,
    #[arg(long)] pub replace_snapshot: Option<String>,
}
#[derive(Args)]
pub struct WriteContext {
    #[arg(long)] pub project: String,
    #[arg(long)] pub expected_revision: String,
    #[command(flatten)] pub identity: MutationIdentity,
}
impl WriteContext {
    fn request(self, id: &str, params: Value) -> Result<Request> {
        let mut request = query(id, params);
        model::validate_id(&self.project)?; model::Revision::parse(&self.expected_revision)?;
        request.project = Some(commands::ProjectRef { id: self.project, expected_revision: Some(self.expected_revision) });
        self.identity.apply(&mut request)?; Ok(request)
    }
}
fn rational(value: &str) -> Result<Rational> {
    let (num, den) = value.split_once('/').unwrap_or((value, "1"));
    Ok(Rational::new(num.parse::<i64>()?, den.parse::<i64>()?)?)
}
fn read_request(context: ReadContext, id: &str, mut params: Value) -> Result<Request> {
    model::validate_id(&context.project)?;
    if let Some(revision) = context.revision { model::Revision::parse(&revision)?; params["revision"] = json!(revision); }
    if let Some(replacement) = context.replace_snapshot { params["replace_snapshot"] = json!(replacement); }
    let mut request = query(id, params); request.project = Some(commands::ProjectRef { id: context.project, expected_revision: None }); Ok(request)
}
#[derive(Subcommand)]
pub enum Projects {
    Create { #[arg(long)] name: String, #[command(flatten)] identity: MutationIdentity },
    Inspect {
        #[command(flatten)] context: ReadContext,
        #[arg(long, default_value = "summary", value_parser = ["summary", "entity", "bin_page"])] projection: String,
        #[arg(long)] entity: Option<String>, #[arg(long)] bin: Option<String>,
        #[arg(long, default_value_t = 128)] limit: usize, #[arg(long)] cursor: Option<String>,
    },
}
impl Projects {
    pub(super) fn prepare(self) -> Result<Request> {
        match self {
            Self::Create { name, identity } => { model::label(&name)?; let mut request = query("projects.create", json!({"name":name})); identity.apply(&mut request)?; Ok(request) }
            Self::Inspect { context, projection, entity, bin, limit, cursor } => read_request(context, "projects.inspect", json!({"projection":projection,"entity_id":entity,"bin_id":bin,"limit":limit,"cursor":cursor})),
        }
    }
}
#[derive(Subcommand)]
pub enum Sequences {
    Create {
        #[command(flatten)] context: WriteContext,
        #[arg(long)] name: String, #[arg(long)] width: u32, #[arg(long)] height: u32,
        /// Exact numerator/denominator, for example 30000/1001.
        #[arg(long)] frame_rate: String,
        #[arg(long, default_value_t = 48000)] sample_rate: u32,
        #[arg(long, default_value = "stereo")] audio_layout: String,
        #[arg(long = "video-track")] video_tracks: Vec<String>, #[arg(long = "audio-track")] audio_tracks: Vec<String>,
    },
    Inspect {
        #[command(flatten)] context: ReadContext,
        #[arg(long)] sequence: String,
        #[arg(long, default_value = "entity", value_parser = ["entity", "track_page", "viewport"])] projection: String,
        #[arg(long = "track")] tracks: Vec<String>,
        #[arg(long, requires = "end")] start: Option<String>, #[arg(long, requires = "start")] end: Option<String>,
        #[arg(long, default_value_t = 128)] limit: usize, #[arg(long)] cursor: Option<String>,
    },
}
impl Sequences {
    pub(super) fn prepare(self) -> Result<Request> {
        match self {
            Self::Create { context, name, width, height, frame_rate, sample_rate, audio_layout, video_tracks, audio_tracks } => {
                let settings = model::SequenceSettings { width, height, frame_rate: rational(&frame_rate)?, sample_rate, audio_layout }; settings.validate()?;
                let tracks: Vec<_> = video_tracks.into_iter().map(|name| model::TrackInput { kind: model::TrackKind::Video, name }).chain(audio_tracks.into_iter().map(|name| model::TrackInput { kind: model::TrackKind::Audio, name })).collect();
                context.request("sequences.create", json!({"name":name,"settings":settings,"tracks":tracks}))
            }
            Self::Inspect { context, sequence, projection, tracks, start, end, limit, cursor } => {
                let range = match (start, end) { (Some(start), Some(end)) => Some(json!({"start":rational(&start)?,"end":rational(&end)?})), (None, None) => None, _ => return Err(model::invalid("Viewport requires both start and end").into()) };
                read_request(context, "sequences.inspect", json!({"sequence_id":sequence,"projection":projection,"track_ids":tracks,"range":range,"limit":limit,"cursor":cursor}))
            }
        }
    }
}
#[derive(Subcommand)]
pub enum Clips {
    Place {
        #[command(flatten)] context: WriteContext,
        #[arg(long)] sequence: String, #[arg(long)] track: String,
        #[arg(long, conflicts_with = "rgba", requires = "stream_index")] asset: Option<String>,
        #[arg(long, requires = "asset")] stream_index: Option<u32>,
        /// Four comma-separated normalized finite channels.
        #[arg(long, conflicts_with = "asset", requires = "source_duration")] rgba: Option<String>,
        #[arg(long, requires = "rgba")] source_duration: Option<String>,
        #[arg(long, allow_hyphen_values = true)] source_start: String, #[arg(long, allow_hyphen_values = true)] source_end: String, #[arg(long)] sequence_start: String,
    },
    Move { #[command(flatten)] context: WriteContext, #[arg(long = "clip", required = true)] clips: Vec<String>, #[arg(long, allow_hyphen_values = true)] delta: String },
    Split { #[command(flatten)] context: WriteContext, #[arg(long)] clip: String, #[arg(long)] at: String },
    Remove { #[command(flatten)] context: WriteContext, #[arg(long = "clip", required = true)] clips: Vec<String> },
}
impl Clips {
    pub(super) fn prepare(self) -> Result<Request> {
        match self {
            Self::Place { context, sequence, track, asset, stream_index, rgba, source_duration, source_start, source_end, sequence_start } => {
                let source = match (asset, stream_index, rgba, source_duration) {
                    (Some(asset_id), Some(stream_index), None, None) => model::PlaceSource::Registered { asset_id, stream_index },
                    (None, None, Some(rgba), Some(duration)) => {
                        let channels = rgba.split(',').map(str::parse::<f64>).collect::<std::result::Result<Vec<_>, _>>()?;
                        let rgba: [f64; 4] = channels.try_into().map_err(|_| model::invalid("RGBA requires exactly four channels"))?;
                        if rgba.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) { return Err(model::invalid("RGBA channels must be finite and normalized").into()); }
                        model::PlaceSource::SolidColor { rgba, duration: rational(&duration)? }
                    }
                    _ => return Err(model::invalid("Choose an asset/stream or finite solid-color source").into()),
                };
                context.request("clips.place", json!({"sequence_id":sequence,"track_id":track,"source":source,"source_range":{"start":rational(&source_start)?,"end":rational(&source_end)?},"sequence_start":rational(&sequence_start)?,"collision":"reject"}))
            }
            Self::Move { context, clips, delta } => context.request("clips.move", json!({"clip_ids":clips,"delta":rational(&delta)?,"collision":"reject","link_policy":"expand"})),
            Self::Split { context, clip, at } => context.request("clips.split", json!({"clip_id":clip,"at_sequence":rational(&at)?,"link_policy":"expand"})),
            Self::Remove { context, clips } => context.request("clips.remove", json!({"clip_ids":clips,"mode":"lift","link_policy":"expand"})),
        }
    }
}
#[derive(Subcommand)]
pub enum History { Undo { #[command(flatten)] context: WriteContext }, Redo { #[command(flatten)] context: WriteContext } }
impl History { pub(super) fn prepare(self) -> Result<Request> { match self { Self::Undo { context } => context.request("history.undo", json!({})), Self::Redo { context } => context.request("history.redo", json!({})) } } }
