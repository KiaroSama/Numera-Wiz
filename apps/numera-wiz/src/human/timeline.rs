//! Typed finite timeline intent lowers through the shared canonical request port.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nw_domain::interchange::{self, BinaryTrackImport, OtioImportInput};
use nw_domain::{projects as model, time::TimeRange};

#[derive(Args)]
pub struct TrackExport {
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
    pub(super) out: Option<PathBuf>,
}

impl TrackExport {
    fn prepare(self, command: &str) -> Result<Request> {
        model::validate_id(&self.project)?;
        let input = nw_domain::interchange::EdlInput {
            sequence_id: model::EntityId::parse(self.sequence)?,
            track_id: model::EntityId::parse(self.track)?,
            revision: model::Revision::parse(&self.revision)?,
            range: TimeRange {
                start: super::projects::rational(&self.start)?,
                end: super::projects::rational(&self.end)?,
            },
        };
        input.range.validate()?;
        let mut request = query(command, serde_json::to_value(input)?);
        request.project = Some(commands::ProjectRef {
            id: self.project,
            expected_revision: None,
        });
        Ok(request)
    }
}

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
    ExportEdl(TrackExport),
    /// Export one selected video or audio track as OpenTimelineIO.
    ExportOtio(TrackExport),
    /// Export one selected video or frame-aligned audio track as Final Cut Pro 7 XML.
    ExportFcp7(TrackExport),
    /// Export one selected track as Final Cut Pro XML.
    ExportFcpxml(TrackExport),
    /// Export one selected linked picture or PCM audio track as AAF.
    ExportAaf(TrackExport),
    /// Export one selected linked PCM audio track as OMF 2.
    ExportOmf(TrackExport),
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
    /// Import one OpenTimelineIO track as one reversible edit.
    ImportOtio(TrackImport),
    /// Import one Final Cut Pro 7 XML track as one reversible edit.
    ImportFcp7(TrackImport),
    /// Import one Final Cut Pro XML primary story as one reversible edit.
    ImportFcpxml(TrackImport),
    /// Import one linked AAF track as one reversible edit.
    ImportAaf(TrackImport),
    /// Import one linked OMF 2 audio track as one reversible edit.
    ImportOmf(TrackImport),
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
            Self::ExportEdl(args) => args.prepare("interchange.edl.export"),
            Self::ExportOtio(args) => args.prepare("interchange.otio.export"),
            Self::ExportFcp7(args) => args.prepare("interchange.fcp7.export"),
            Self::ExportFcpxml(args) => args.prepare("interchange.fcpxml.export"),
            Self::ExportAaf(args) => args.prepare("interchange.aaf.export"),
            Self::ExportOmf(args) => args.prepare("interchange.omf.export"),
            Self::ImportEdl {
                context,
                sequence,
                track,
                fps,
                file,
                reels,
            } => {
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
            Self::ImportOtio(args) => args.prepare("interchange.otio.import", "OTIO_INVALID"),
            Self::ImportFcp7(args) => args.prepare("interchange.fcp7.import", "FCP7_INVALID"),
            Self::ImportFcpxml(args) => args.prepare("interchange.fcpxml.import", "FCPXML_INVALID"),
            Self::ImportAaf(args) => args.prepare_binary("interchange.aaf.import"),
            Self::ImportOmf(args) => args.prepare_binary("interchange.omf.import"),
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

#[derive(Args)]
pub struct TrackImport {
    #[command(flatten)]
    context: super::projects::WriteContext,
    #[arg(long)]
    sequence: String,
    #[arg(long)]
    track: String,
    #[arg(long, default_value = "0")]
    start: String,
    #[arg(long)]
    file: PathBuf,
    /// JSON array binding each media URL to a registered asset, stream and digest.
    #[arg(long)]
    sources: Option<PathBuf>,
}

impl TrackImport {
    fn prepare(self, command: &str, code: &'static str) -> Result<Request> {
        let sources = read_track_bindings(self.sources)?;
        let document = read_cut_file(
            self.file,
            interchange::MAX_OTIO_BYTES,
            "Track interchange input exceeds 96 KiB",
        )?;
        let document = String::from_utf8(document)
            .map_err(|_| DomainError::new(code, "Track interchange document must be UTF-8"))?;
        let input = OtioImportInput {
            sequence_id: model::EntityId::parse(self.sequence)?,
            track_id: model::EntityId::parse(self.track)?,
            start: Some(super::projects::rational(&self.start)?),
            document,
            sources,
        };
        self.context.request(command, serde_json::to_value(input)?)
    }

    fn prepare_binary(self, command: &str) -> Result<Request> {
        let bytes = read_cut_file(
            self.file,
            interchange::MAX_BINARY_IMPORT_BYTES,
            "Binary track document exceeds 512 KiB",
        )?;
        let input = BinaryTrackImport {
            sequence_id: model::EntityId::parse(self.sequence)?,
            track_id: model::EntityId::parse(self.track)?,
            start: Some(super::projects::rational(&self.start)?),
            document_base64: STANDARD.encode(bytes),
            sources: read_track_bindings(self.sources)?,
        };
        self.context.request(command, serde_json::to_value(input)?)
    }
}

fn read_track_bindings(path: Option<PathBuf>) -> Result<Vec<interchange::OtioSource>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let bytes = read_cut_file(
        path,
        interchange::MAX_OTIO_BYTES,
        "Track media bindings exceed 96 KiB",
    )?;
    Ok(commands::parse(commands::decode_json(&bytes)?)?)
}

fn read_cmx_file(path: PathBuf) -> Result<Vec<u8>> {
    read_cut_file(
        path,
        nw_domain::interchange::MAX_DOCUMENT_BYTES,
        "EDL input file exceeds 16 KiB",
    )
}

fn read_cut_file(path: PathBuf, limit: usize, message: &'static str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(DomainError::new("RESOURCE_LIMIT", message).into());
    }
    Ok(bytes)
}

pub(super) async fn save_interchange(
    client: &NativeClient,
    request: Request,
    path: &Path,
) -> Result<()> {
    let response = client.request(&request).await?;
    if response.ok {
        let saved: Result<(), DomainError> = (|| {
            let value = response.result.as_ref().ok_or_else(|| {
                DomainError::new("INVALID_RESPONSE", "Interchange result is missing")
            })?;
            let digest = value["document_sha256"].as_str().ok_or_else(|| {
                DomainError::new("INVALID_RESPONSE", "Interchange digest is missing")
            })?;
            if matches!(
                request.command.id.as_str(),
                "interchange.aaf.export" | "interchange.omf.export"
            ) {
                let encoded = value["document_base64"].as_str().ok_or_else(|| {
                    DomainError::new("INVALID_RESPONSE", "Binary interchange document is missing")
                })?;
                let bytes =
                    interchange::decode_binary_document(encoded, interchange::MAX_BINARY_BYTES)?;
                nw_platform::download::save_verified_document(path, &bytes, digest)
            } else {
                let text = value["document"].as_str().ok_or_else(|| {
                    DomainError::new("INVALID_RESPONSE", "Interchange document is missing")
                })?;
                nw_platform::download::save_verified_document(path, text.as_bytes(), digest)
            }
        })();
        if let Err(error) = saved {
            return crate::print_response_value(Response::error(&request.request_id, error));
        }
    }
    crate::print_response_value(response)
}
