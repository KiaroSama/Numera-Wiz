//! Typed finite timeline intent lowers through the shared canonical request port.
use super::*;
use nw_domain::{projects as model, time::TimeRange};

#[derive(Subcommand)]
pub enum Timeline {
    /// Decode the visible source at one exact sequence time.
    PreviewFrame {
        #[arg(long)]
        project: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        sequence: String,
        #[arg(long)]
        at: String,
    },
    /// Review a lossless FFV1/PCM export.
    Render {
        #[arg(long)]
        project: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        sequence: String,
        /// Exact nonnegative sequence time, as numerator/denominator.
        #[arg(long, default_value = "0")]
        start: String,
        /// Exclusive endpoint, as numerator/denominator.
        #[arg(long)]
        end: String,
        /// Include registered video and overlapping audio from a mixed-media generation.
        #[arg(long)]
        mixed: bool,
    },
}

impl Timeline {
    pub(super) fn prepare(self) -> Result<Request> {
        match self {
            Self::PreviewFrame {
                project,
                revision,
                sequence,
                at,
            } => {
                model::validate_id(&project)?;
                model::validate_id(&sequence)?;
                model::Revision::parse(&revision)?;
                let at = super::projects::rational(&at)?;
                let mut request = query(
                    "timeline.preview_frame",
                    json!({"sequence_id":sequence,"revision":revision,"at":at}),
                );
                request.project = Some(commands::ProjectRef {
                    id: project,
                    expected_revision: None,
                });
                Ok(request)
            }
            Self::Render {
                project,
                revision,
                sequence,
                start,
                end,
                mixed,
            } => {
                model::validate_id(&project)?;
                model::validate_id(&sequence)?;
                model::Revision::parse(&revision)?;
                let range = TimeRange {
                    start: super::projects::rational(&start)?,
                    end: super::projects::rational(&end)?,
                };
                range.validate()?;
                let mut params = json!({"sequence_id":sequence,"revision":revision,"range":range,
                    "output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le",
                        "destination":{"kind":"download"}}});
                let mut request = if mixed {
                    params["contract_kind"] = json!("media_v2");
                    let input: nw_domain::media::RenderContractV2 = commands::parse(params)?;
                    query("timeline.render_mixed", serde_json::to_value(input)?)
                } else {
                    params["output"]["audio"] = json!("silence");
                    let input: nw_domain::media::RenderInput = commands::parse(params)?;
                    query("timeline.render", serde_json::to_value(input)?)
                };
                request.project = Some(commands::ProjectRef {
                    id: project,
                    expected_revision: None,
                });
                Ok(request)
            }
        }
    }
}
