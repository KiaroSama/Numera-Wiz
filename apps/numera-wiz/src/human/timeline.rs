//! Typed finite timeline intent lowers through the shared canonical request port.
use super::*;
use nw_domain::{projects as model, time::TimeRange};

#[derive(Subcommand)]
pub enum Timeline {
    /// Close an empty range on the selected tracks and move the later clips left.
    RemoveGap {
        #[command(flatten)]
        context: super::projects::WriteContext,
        #[arg(long)]
        sequence: String,
        #[arg(long = "track", required = true)]
        tracks: Vec<String>,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// Export one selected picture track as a CMX 3600 cut list.
    ExportEdl {
        #[arg(long)]
        project: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        sequence: String,
        #[arg(long)]
        track: String,
        #[arg(long, default_value = "0")]
        start: String,
        #[arg(long)]
        end: String,
        /// Save the verified cut list to a new client-side file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import a UTF-8 CMX 3600 cut list as one reversible edit.
    ImportEdl {
        #[command(flatten)]
        context: super::projects::WriteContext,
        #[arg(long)]
        sequence: String,
        #[arg(long)]
        track: String,
        #[arg(long)]
        fps: u32,
        #[arg(long)]
        file: PathBuf,
        /// JSON array binding every non-black reel to a registered asset, stream and digest.
        #[arg(long)]
        reels: Option<PathBuf>,
    },
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
    /// Prepare a short, authenticated program preview manifest.
    PreviewSegment {
        #[arg(long)]
        project: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        sequence: String,
        #[arg(long, default_value = "0")]
        start: String,
        #[arg(long)]
        end: String,
        /// Return the continuous playback request for the visible range.
        #[arg(long)]
        continuous: bool,
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
            Self::RemoveGap {
                context,
                sequence,
                tracks,
                start,
                end,
            } => {
                let range = TimeRange {
                    start: super::projects::rational(&start)?,
                    end: super::projects::rational(&end)?,
                };
                range.validate()?;
                let input: model::RemoveGapInput = commands::parse(json!({
                    "sequence_id":sequence,"track_ids":tracks,"range":range
                }))?;
                context.request("timeline.remove_gap", serde_json::to_value(input)?)
            }
            Self::ExportEdl {
                project,
                revision,
                sequence,
                track,
                start,
                end,
                out: _,
            } => {
                model::validate_id(&project)?;
                let input = nw_domain::interchange::EdlInput {
                    sequence_id: model::EntityId::parse(sequence)?,
                    track_id: model::EntityId::parse(track)?,
                    revision: model::Revision::parse(&revision)?,
                    range: TimeRange {
                        start: super::projects::rational(&start)?,
                        end: super::projects::rational(&end)?,
                    },
                };
                input.range.validate()?;
                let mut request = query("interchange.edl.export", serde_json::to_value(input)?);
                request.project = Some(commands::ProjectRef {
                    id: project,
                    expected_revision: None,
                });
                Ok(request)
            }
            Self::ImportEdl { context, sequence, track, fps, file, reels } => {
                let reels = if let Some(path) = reels {
                    commands::parse(commands::decode_json(&read_cmx_file(path)?)?)?
                } else {
                    Vec::new()
                };
                let document = String::from_utf8(read_cmx_file(file)?)
                    .map_err(|_| DomainError::new("EDL_INVALID", "EDL document must be UTF-8"))?;
                let input = nw_domain::interchange::EdlImportInput {
                    sequence_id: model::EntityId::parse(sequence)?,
                    track_id: model::EntityId::parse(track)?,
                    frames_per_second: fps,
                    document,
                    reels,
                };
                context.request("interchange.edl.import", serde_json::to_value(input)?)
            }
            Self::PreviewSegment {
                project,
                revision,
                sequence,
                start,
                end,
                continuous,
            } => {
                model::validate_id(&project)?;
                let input = nw_domain::preview::ProgramSegmentInput {
                    sequence_id: model::EntityId::parse(sequence)?,
                    revision: model::Revision::parse(&revision)?,
                    range: TimeRange {
                        start: super::projects::rational(&start)?,
                        end: super::projects::rational(&end)?,
                    },
                };
                input.range.validate()?;
                let id = if continuous {
                    nw_domain::preview::PROGRAM_PLAYBACK_COMMAND
                } else {
                    nw_domain::preview::PROGRAM_SEGMENT_COMMAND
                };
                let mut request = query(id, serde_json::to_value(input)?);
                request.project = Some(commands::ProjectRef {
                    id: project,
                    expected_revision: None,
                });
                Ok(request)
            }
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

fn read_cmx_file(path: PathBuf) -> Result<Vec<u8>> {
    let limit = nw_domain::interchange::MAX_DOCUMENT_BYTES;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(DomainError::new("RESOURCE_LIMIT", "EDL input file exceeds 16 KiB").into());
    }
    Ok(bytes)
}

pub(super) async fn save_edl(client: &NativeClient, request: Request, path: &Path) -> Result<()> {
    let response = client.request(&request).await?;
    if response.ok {
        let saved: Result<(), DomainError> = (|| {
            let value = response
                .result
                .as_ref()
                .ok_or_else(|| DomainError::new("INVALID_RESPONSE", "EDL result is missing"))?;
            let text = value["document"]
                .as_str()
                .ok_or_else(|| DomainError::new("INVALID_RESPONSE", "EDL document is missing"))?;
            let digest = value["document_sha256"]
                .as_str()
                .ok_or_else(|| DomainError::new("INVALID_RESPONSE", "EDL digest is missing"))?;
            nw_platform::download::save_verified_document(path, text.as_bytes(), digest)
        })();
        if let Err(error) = saved {
            return crate::print_response_value(Response::error(&request.request_id, error));
        }
    }
    crate::print_response_value(response)
}
