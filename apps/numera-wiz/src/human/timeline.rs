//! Typed finite timeline intent lowers through the shared canonical request port.
use super::*;
use nw_domain::{projects as model, time::TimeRange};

#[derive(Subcommand)]
pub enum Timeline {
    /// Review a finite lossless FFV1/PCM export; execution remains in development.
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
    },
}

impl Timeline {
    pub(super) fn prepare(self) -> Result<Request> {
        match self {
            Self::Render { project, revision, sequence, start, end } => {
                model::validate_id(&project)?;
                model::validate_id(&sequence)?;
                model::Revision::parse(&revision)?;
                let range = TimeRange {
                    start: super::projects::rational(&start)?,
                    end: super::projects::rational(&end)?,
                };
                range.validate()?;
                let params = json!({"sequence_id":sequence,"revision":revision,"range":range,
                    "output":{"container":"matroska","video_codec":"ffv1","audio_codec":"pcm_s16le",
                        "audio":"silence","destination":{"kind":"download"}}});
                let input: nw_domain::media::RenderInput = commands::parse(params)?;
                let mut request = query("timeline.render", serde_json::to_value(input)?);
                request.project = Some(commands::ProjectRef { id: project, expected_revision: None });
                Ok(request)
            }
        }
    }
}
