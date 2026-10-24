//! Bounded source inventory and selection previews; never publication evidence.
use crate::{
    DomainError, Result,
    mux::{self, CleanAction, CleanIntent, Stream, StreamKind},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_REPORT_ASSETS: usize = 64;
const MAX_SUMMARY_ROWS: usize = 512;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_REPORT_BYTES: usize = 224 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportInput {
    pub asset_ids: Vec<String>,
    pub intent: CleanIntent,
}
impl ReportInput {
    pub fn validate(&self) -> Result<()> {
        if self.asset_ids.is_empty()
            || self.asset_ids.len() > MAX_REPORT_ASSETS
            || self
                .asset_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128)
            || self.asset_ids.iter().collect::<BTreeSet<_>>().len() != self.asset_ids.len()
        {
            return Err(DomainError::new(
                "VALIDATION_ERROR",
                "Selection report requires 1-64 distinct bounded asset identifiers",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StreamSummary {
    pub kind: StreamKind,
    pub source_index: Option<u32>,
    pub attached_picture: bool,
    pub language: String,
    pub title: String,
    pub codec: String,
    pub source_count: u32,
    pub retained_count: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileSelection {
    pub asset_id: String,
    pub display_name: String,
    pub action: Option<CleanAction>,
    pub selected_original_indices: Vec<u32>,
    pub matched_audio_count: Option<u32>,
    pub matched_subtitle_count: Option<u32>,
    pub warnings: Vec<String>,
    pub error: Option<DomainError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelectionReport {
    pub files: Vec<FileSelection>,
    pub unique_streams: Vec<StreamSummary>,
    pub original_index_streams: Vec<StreamSummary>,
    pub resolved_files: u32,
    pub skipped_files: u32,
    pub rejected_files: u32,
    pub scope: String,
}
type SummaryKey = (u8, Option<u32>, String, String, String);
#[derive(Default)]
pub struct ReportBuilder {
    files: Vec<FileSelection>,
    rows: BTreeMap<SummaryKey, StreamSummary>,
    text_bytes: usize,
}
impl ReportBuilder {
    /// Callers can load, authorize and release one asset at a time.
    pub fn add(
        &mut self,
        asset_id: &str,
        display_name: &str,
        streams: &[Stream],
        intent: &CleanIntent,
    ) -> Result<()> {
        if self.files.len() >= MAX_REPORT_ASSETS
            || streams.len() > 256
            || self.files.iter().any(|file| file.asset_id == asset_id)
        {
            return Err(limit());
        }
        self.charge(asset_id.len().saturating_add(display_name.len()))?;
        let mut file = FileSelection {
            asset_id: asset_id.into(),
            display_name: display_name.into(),
            action: None,
            selected_original_indices: Vec::new(),
            matched_audio_count: None,
            matched_subtitle_count: None,
            warnings: Vec::new(),
            error: None,
        };
        match mux::resolve(streams, intent) {
            Ok(resolved) => {
                file.matched_audio_count = Some(
                    resolved
                        .streams
                        .iter()
                        .filter(|s| s.kind == StreamKind::Audio)
                        .count() as u32,
                );
                file.matched_subtitle_count = Some(
                    resolved
                        .streams
                        .iter()
                        .filter(|s| s.kind == StreamKind::Subtitle)
                        .count() as u32,
                );
                file.selected_original_indices = resolved
                    .outcomes
                    .iter()
                    .filter(|outcome| outcome.output_index.is_some())
                    .map(|outcome| outcome.source_index)
                    .collect();
                file.warnings = resolved.warnings;
                file.action = Some(resolved.action);
            }
            Err(error) => file.error = Some(error),
        }
        for warning in &file.warnings {
            self.charge(warning.len())?;
        }
        if let Some(error) = &file.error {
            self.charge(error.message.len().saturating_add(error.code.len()))?;
        }
        for stream in streams {
            let language =
                mux::normalize_language(stream.tags.get("language").map_or("und", String::as_str));
            let title = stream.tags.get("title").cloned().unwrap_or_default();
            let retained = u32::from(file.selected_original_indices.contains(&stream.index));
            for source_index in [None, Some(stream.index)] {
                let key = (
                    kind_key(stream),
                    source_index,
                    language.to_lowercase(),
                    title.to_lowercase(),
                    stream.codec.to_lowercase(),
                );
                let row_count = self.rows.len();
                let text_bytes = language
                    .len()
                    .saturating_add(title.len())
                    .saturating_add(stream.codec.len())
                    .saturating_add(key.2.len())
                    .saturating_add(key.3.len())
                    .saturating_add(key.4.len());
                let summary = match self.rows.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        if row_count >= MAX_SUMMARY_ROWS {
                            return Err(limit());
                        }
                        self.text_bytes =
                            self.text_bytes.checked_add(text_bytes).ok_or_else(limit)?;
                        if self.text_bytes > MAX_TEXT_BYTES {
                            return Err(limit());
                        }
                        entry.insert(StreamSummary {
                            kind: stream.kind.clone(),
                            source_index,
                            attached_picture: stream.attached_picture(),
                            language: language.clone(),
                            title: title.clone(),
                            codec: stream.codec.clone(),
                            source_count: 0,
                            retained_count: 0,
                        })
                    }
                };
                summary.source_count += 1;
                summary.retained_count += retained;
            }
        }
        self.files.push(file);
        Ok(())
    }
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.text_bytes = self.text_bytes.checked_add(bytes).ok_or_else(limit)?;
        if self.text_bytes > MAX_TEXT_BYTES {
            return Err(limit());
        }
        Ok(())
    }
    pub fn finish(self) -> Result<SelectionReport> {
        let resolved_files = self
            .files
            .iter()
            .filter(|file| {
                matches!(
                    file.action,
                    Some(CleanAction::CopyUnchanged | CleanAction::Remux)
                )
            })
            .count() as u32;
        let skipped_files = self
            .files
            .iter()
            .filter(|file| file.action == Some(CleanAction::Skip))
            .count() as u32;
        let rejected_files = self
            .files
            .iter()
            .filter(|file| file.error.is_some())
            .count() as u32;
        let (unique_streams, original_index_streams) = self
            .rows
            .into_values()
            .partition(|row| row.source_index.is_none());
        let report = SelectionReport {
            files: self.files,
            unique_streams,
            original_index_streams,
            resolved_files,
            skipped_files,
            rejected_files,
            scope: "Registered source inventory and pure selection preview; no fresh probe, output compatibility admission, byte verification or produced artifact".into(),
        };
        if serde_json::to_vec(&report)
            .map_err(|error| DomainError::new("SERIALIZATION_FAILED", error.to_string()))?
            .len()
            > MAX_REPORT_BYTES
        {
            return Err(limit());
        }
        Ok(report)
    }
}
fn kind_key(stream: &Stream) -> u8 {
    if stream.attached_picture() {
        return 6;
    }
    match stream.kind {
        StreamKind::Video => 0,
        StreamKind::Audio => 1,
        StreamKind::Subtitle => 2,
        StreamKind::Attachment => 3,
        StreamKind::Data => 4,
        StreamKind::Unknown => 5,
    }
}
fn limit() -> DomainError {
    DomainError::new(
        "RESOURCE_LIMIT",
        "Selection report exceeds its bounded inventory; request a smaller explicit asset set",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::{NoMatch, Preservation, Selection, Selector};
    fn intent() -> CleanIntent {
        CleanIntent {
            audio_selection: Selection {
                selector: Selector::Languages {
                    values: vec!["fas".into()],
                },
                no_match: NoMatch::SkipFile,
            },
            subtitle_selection: Selection {
                selector: Selector::None,
                no_match: NoMatch::AllowEmpty,
            },
            audio_order: vec![],
            subtitle_order: vec![],
            stream_tag_patches: vec![],
            preservation: Preservation::default(),
        }
    }
    fn stream(index: u32, kind: StreamKind, language: &str, title: &str) -> Stream {
        Stream {
            index,
            kind,
            codec: "fixture".into(),
            tags: BTreeMap::from([
                ("language".into(), language.into()),
                ("title".into(), title.into()),
            ]),
            dispositions: BTreeSet::new(),
        }
    }
    #[test]
    fn grouping_preserves_original_indices_and_counts_skip_and_rejection_separately() {
        let mut report = ReportBuilder::default();
        report
            .add(
                "a",
                "first.mkv",
                &[
                    stream(0, StreamKind::Video, "und", ""),
                    stream(2, StreamKind::Audio, "FAS", "Dialogue"),
                ],
                &intent(),
            )
            .unwrap();
        report
            .add(
                "b",
                "second.mkv",
                &[
                    stream(0, StreamKind::Video, "unknown", ""),
                    stream(5, StreamKind::Audio, "fas", "dialogue"),
                ],
                &intent(),
            )
            .unwrap();
        report
            .add(
                "c",
                "unmatched.mkv",
                &[
                    stream(0, StreamKind::Video, "und", ""),
                    stream(1, StreamKind::Audio, "eng", "Other"),
                ],
                &intent(),
            )
            .unwrap();
        report
            .add(
                "d",
                "audio-only.mkv",
                &[stream(0, StreamKind::Audio, "fas", "Dialogue")],
                &intent(),
            )
            .unwrap();
        let report = report.finish().unwrap();
        assert_eq!(
            (
                report.resolved_files,
                report.skipped_files,
                report.rejected_files
            ),
            (2, 1, 1)
        );
        assert!(report.files[2].selected_original_indices.is_empty());
        assert_eq!(
            report.files[3].error.as_ref().unwrap().code,
            "VIDEO_REQUIRED"
        );
        let summary = report
            .unique_streams
            .iter()
            .find(|row| row.kind == StreamKind::Audio && row.language == "fas")
            .unwrap();
        assert_eq!((summary.source_count, summary.retained_count), (3, 2));
        assert_eq!(summary.title, "Dialogue");
        for index in [0, 2, 5] {
            assert!(
                report
                    .original_index_streams
                    .iter()
                    .any(|row| row.kind == StreamKind::Audio && row.source_index == Some(index))
            );
        }
    }
    #[test]
    fn missing_and_unknown_languages_share_one_summary_without_erasing_original_titles() {
        let mut report = ReportBuilder::default();
        let mut value = stream(0, StreamKind::Video, "uknown", "Exact title");
        report.add("a", "a", &[value.clone()], &intent()).unwrap();
        value.tags.remove("language");
        report.add("b", "b", &[value], &intent()).unwrap();
        let report = report.finish().unwrap();
        assert_eq!(report.unique_streams.len(), 1);
        assert_eq!(report.unique_streams[0].language, "und");
        assert_eq!(report.unique_streams[0].title, "Exact title");
        assert_eq!(report.unique_streams[0].source_count, 2);
    }
    #[test]
    fn cover_pictures_are_not_merged_with_ordinary_video_in_the_inventory() {
        let mut rules = intent();
        rules.audio_selection.selector = Selector::None;
        let video = stream(0, StreamKind::Video, "und", "");
        let mut cover = stream(1, StreamKind::Video, "und", "");
        cover.dispositions.insert("attached_pic".into());
        let mut report = ReportBuilder::default();
        report
            .add("a", "covers.mkv", &[video, cover], &rules)
            .unwrap();
        let report = report.finish().unwrap();
        assert_eq!(report.unique_streams.len(), 2);
        assert_eq!(
            report
                .unique_streams
                .iter()
                .filter(|row| row.attached_picture)
                .count(),
            1
        );
        assert!(
            report
                .unique_streams
                .iter()
                .all(|row| row.retained_count == 1)
        );
    }
    #[test]
    fn inventory_backpressure_never_silently_truncates_source_metadata() {
        let mut report = ReportBuilder::default();
        let huge = stream(0, StreamKind::Video, "und", &"x".repeat(MAX_TEXT_BYTES));
        assert_eq!(
            report.add("a", "a", &[huge], &intent()).unwrap_err().code,
            "RESOURCE_LIMIT"
        );
        let mut input = ReportInput {
            asset_ids: vec!["a".into(), "a".into()],
            intent: intent(),
        };
        assert!(input.validate().is_err());
        input.asset_ids = vec![];
        assert!(input.validate().is_err());
    }
}
