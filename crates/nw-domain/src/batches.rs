//! Explicit bounded cleanup batches. Every candidate retains its own outcome and job.
use crate::{DomainError, Result, commands::CleanInput, execution::CleanupLimits, file_times::FileTimeRequest, mux::{CleanAction, CleanIntent}};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_BATCH_ITEMS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    RejectAll,
    ContinueWithRejected,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedPolicy {
    Reject,
    PerFileNoMatch,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnresolvedIndex {
    pub kind: crate::mux::StreamKind,
    pub source_index: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemInput {
    pub asset_id: String,
    pub output: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CleanBatchInput {
    pub items: Vec<ItemInput>,
    pub intent: CleanIntent,
    pub file_times: FileTimeRequest,
    pub execution_limits: CleanupLimits,
    pub failure_policy: FailurePolicy,
    pub unresolved_policy: UnresolvedPolicy,
}
impl CleanBatchInput {
    pub fn validate(&self) -> Result<()> {
        if self.items.is_empty() || self.items.len() > MAX_BATCH_ITEMS {
            return Err(DomainError::new("VALIDATION_ERROR", "A cleanup batch requires 1-64 explicit candidates"));
        }
        self.execution_limits.validate()?;
        crate::mux::validate_intent(&self.intent)?;
        let mut outputs = BTreeSet::new();
        for item in &self.items {
            let name = item.output.rsplit(['/', '\\']).next().unwrap_or("");
            if !identifier(&item.asset_id) || name.is_empty() || name.len() > 1024 || item.output.len() > 16384
                || !outputs.insert(&item.output)
            {
                return Err(DomainError::new("VALIDATION_ERROR", "Batch candidates need valid asset IDs and distinct bounded output paths"));
            }
        }
        Ok(())
    }
    pub fn clean_input(&self, item: &ItemInput) -> CleanInput {
        CleanInput {
            asset_id: item.asset_id.clone(), output: item.output.clone(),
            intent: self.intent.clone(), file_times: self.file_times,
            execution_limits: self.execution_limits.clone(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanReference {
    pub id: String,
    pub hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlannedItem {
    pub index: u32,
    pub asset_id: String,
    pub source_sha256: String,
    pub source_bytes: String,
    pub output_name: String,
    pub requested_output: String,
    pub plan: Option<PlanReference>,
    pub action: Option<CleanAction>,
    pub error: Option<DomainError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchPlan {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub data_root: String,
    pub hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub failure_policy: FailurePolicy,
    pub unresolved_policy: UnresolvedPolicy,
    pub unresolved: Vec<UnresolvedIndex>,
    pub items: Vec<PlannedItem>,
}
fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}
fn sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn exact_count(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|number| number.to_string() == value)
}
impl BatchPlan {
    pub fn digest(&self) -> Result<String> {
        if self.schema_version != 1 || !identifier(&self.id) || self.creator.is_empty()
            || self.data_root.is_empty() || self.items.is_empty() || self.items.len() > MAX_BATCH_ITEMS
            || !exact_count(&self.created_at) || !exact_count(&self.expires_at)
        {
            return Err(DomainError::new("INVALID_PLAN", "Invalid cleanup batch identity/version/bounds"));
        }
        let encoded_len = serde_json::to_vec(self).map_err(|_| DomainError::new("INVALID_PLAN", "Cannot encode batch record"))?.len();
        if encoded_len.saturating_sub(self.hash.len()).saturating_add(64) > 224 * 1024 {
            return Err(DomainError::new("RESOURCE_LIMIT", "Private batch proposal exceeds its bounded record budget"));
        }
        if self.unresolved.len() > 512 {
            return Err(DomainError::new("RESOURCE_LIMIT", "Unresolved selector manifest exceeds its bound"));
        }
        let mut unresolved = BTreeSet::new();
        for index in &self.unresolved {
            let kind = match index.kind {
                crate::mux::StreamKind::Audio => 0, crate::mux::StreamKind::Subtitle => 1,
                _ => return Err(DomainError::new("INVALID_PLAN", "Unresolved selectors must declare audio/subtitle kind")),
            };
            if !unresolved.insert((kind,index.source_index)) {
                return Err(DomainError::new("INVALID_PLAN", "Unresolved selector manifest contains duplicates"));
            }
        }
        let mut plans = BTreeSet::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.index as usize != index || !identifier(&item.asset_id)
                || !sha(&item.source_sha256) || !exact_count(&item.source_bytes)
                || item.requested_output.is_empty() || item.requested_output.len() > 16384
                || item.output_name.is_empty() || item.output_name.len() > 1024
                || item.output_name.contains(['/', '\\'])
            {
                return Err(DomainError::new("INVALID_PLAN", "Invalid batch candidate identity or outcome"));
            }
            match (&item.plan, &item.action, &item.error) {
                (Some(plan), Some(_), None) if identifier(&plan.id) && sha(&plan.hash) && plans.insert(&plan.id) => {}
                (None, None, Some(error)) if !error.code.is_empty() && error.code.len() <= 128
                    && error.code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    && error.message.len() <= 2048 && error.field.as_ref().is_none_or(|field| field.len() <= 256) => {}
                _ => return Err(DomainError::new("INVALID_PLAN", "Each batch candidate needs exactly one frozen plan or rejection")),
            }
        }
        let semantic_items = self.items.iter().map(|item| serde_json::json!({
            "index":item.index,"source_sha256":item.source_sha256,"source_bytes":item.source_bytes,
            "output":item.requested_output,"action":item.action,
            "plan_hash":item.plan.as_ref().map(|plan| &plan.hash),
            "error_code":item.error.as_ref().map(|error| &error.code),
        })).collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&(
            self.schema_version, &self.data_root, self.failure_policy, self.unresolved_policy, &self.unresolved, semantic_items,
        )).map_err(|_| DomainError::new("INVALID_PLAN", "Cannot encode cleanup batch semantics"))?;
        if bytes.len() > 224 * 1024 {
            return Err(DomainError::new("RESOURCE_LIMIT", "Cleanup batch plan exceeds its bounded record budget"));
        }
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    pub fn validate_hash(&self) -> Result<()> {
        if self.digest()? != self.hash {
            return Err(DomainError::new("INVALID_PLAN", "Cleanup batch plan hash differs from its frozen candidates"));
        }
        Ok(())
    }
    pub fn admits_submission(&self) -> Result<()> {
        self.validate_hash()?;
        if self.unresolved_policy == UnresolvedPolicy::Reject && !self.unresolved.is_empty() {
            return Err(DomainError::new("UNRESOLVED_SOURCE_INDEX", "A requested original index matches no eligible stream in this batch; review an explicit per-file no-match policy"));
        }
        if self.failure_policy == FailurePolicy::RejectAll && self.items.iter().any(|item| item.error.is_some()) {
            return Err(DomainError::new("BATCH_REJECTED", "Batch preview has rejected candidates; choose an explicit continue policy or review corrected candidates"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlannedItemView {
    pub index: u32,
    pub asset_id: String,
    pub source_sha256: String,
    pub source_bytes: String,
    pub output_name: String,
    pub output_path: Option<String>,
    pub plan: Option<PlanReference>,
    pub action: Option<CleanAction>,
    pub error: Option<crate::views::ErrorView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BatchPlanReview {
    pub schema_version: u32,
    pub id: String,
    pub hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub failure_policy: FailurePolicy,
    pub unresolved_policy: UnresolvedPolicy,
    pub unresolved: Vec<UnresolvedIndex>,
    pub items: Vec<PlannedItemView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitBatchInput {
    pub plan_id: String,
    pub plan_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchMember {
    pub index: u32,
    pub job_id: Option<String>,
}
#[derive(Debug, Clone)]
pub struct CancelMember {
    pub job_id: String,
    pub expected_state: crate::records::JobState,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema_version: u32,
    pub id: String,
    pub creator: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub members: Vec<BatchMember>,
    pub accepted_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BatchSummary {
    pub id: String,
    pub plan_id: String,
    pub total: usize,
    pub accepted_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ItemResult {
    pub index: u32,
    pub asset_id: String,
    pub output_name: String,
    pub job_id: Option<String>,
    pub state: Option<crate::records::JobState>,
    pub action: Option<CleanAction>,
    pub artifact_id: Option<String>,
    pub source_bytes: String,
    pub output_bytes: Option<String>,
    pub error: Option<crate::views::ErrorView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BatchResult {
    pub schema_version: u32,
    pub id: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub total: usize,
    pub finished: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub interrupted: usize,
    pub complete: bool,
    pub outcome: String,
    pub successful_video_input_bytes: String,
    pub successful_video_output_bytes: String,
    pub successful_video_size_delta_bytes: String,
    pub items: Vec<ItemResult>,
}
impl BatchResult {
    pub fn finish_counts(&mut self) -> Result<()> {
        if self.total != self.items.len() || self.total == 0 || self.total > MAX_BATCH_ITEMS {
            return Err(DomainError::new("INVALID_BATCH", "Batch result must retain every bounded candidate"));
        }
        self.finished = 0;
        self.succeeded = 0;
        self.skipped = 0;
        self.failed = 0;
        self.cancelled = 0;
        self.interrupted = 0;
        let mut input = 0u128;
        let mut output = 0u128;
        for item in &self.items {
            if item.state.is_none() && item.error.is_none() {
                return Err(DomainError::new("INVALID_BATCH", "A candidate without a job must retain its typed rejection"));
            }
            let state = item.state.as_ref();
            if state.is_none_or(crate::records::JobState::terminal) { self.finished += 1; }
            match state {
                Some(crate::records::JobState::Succeeded) => {
                    self.succeeded += 1;
                    input += item.source_bytes.parse::<u64>().map_err(|_| DomainError::new("INVALID_BATCH", "Invalid successful input size"))? as u128;
                    output += item.output_bytes.as_ref().and_then(|bytes| bytes.parse::<u64>().ok())
                        .ok_or_else(|| DomainError::new("INVALID_BATCH", "Successful batch member has no verified output size"))? as u128;
                }
                Some(crate::records::JobState::Skipped) => self.skipped += 1,
                Some(crate::records::JobState::Failed) | None => self.failed += 1,
                Some(crate::records::JobState::Cancelled) => self.cancelled += 1,
                Some(crate::records::JobState::Interrupted) => self.interrupted += 1,
                _ => {}
            }
        }
        self.complete = self.finished == self.total;
        self.outcome = if !self.complete { "running" }
            else if self.failed > 0 || self.cancelled > 0 || self.interrupted > 0 { "partial_failure" }
            else { "completed" }.into();
        self.successful_video_input_bytes = input.to_string();
        self.successful_video_output_bytes = output.to_string();
        self.successful_video_size_delta_bytes = (output as i128 - input as i128).to_string();
        Ok(())
    }
}

impl Batch {
    pub fn validate(&self, plan: &BatchPlan) -> Result<()> {
        plan.validate_hash()?;
        if self.schema_version != 1 || !identifier(&self.id) || self.creator.is_empty()
            || self.plan_id != plan.id || self.plan_hash != plan.hash
            || self.members.len() != plan.items.len() || !exact_count(&self.accepted_at)
        {
            return Err(DomainError::new("INVALID_BATCH", "Batch identity does not match its frozen plan"));
        }
        let mut jobs = BTreeSet::new();
        for (member, item) in self.members.iter().zip(&plan.items) {
            if member.index != item.index {
                return Err(DomainError::new("INVALID_BATCH", "Batch candidate order changed"));
            }
            match (&member.job_id, &item.plan) {
                (Some(id), Some(_)) if identifier(id) && jobs.insert(id) => {}
                (None, None) => {}
                _ => return Err(DomainError::new("INVALID_BATCH", "Batch member/job mapping is incomplete or duplicated")),
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn result() -> BatchResult {
        let items = [
            (None, "1000", None),
            (Some(crate::records::JobState::Succeeded), "100", Some("40")),
            (Some(crate::records::JobState::Failed), "9000000", None),
            (Some(crate::records::JobState::Skipped), "7000000", None),
            (Some(crate::records::JobState::Cancelled), "2000000", None),
            (Some(crate::records::JobState::Interrupted), "3000000", None),
        ].into_iter().enumerate().map(|(index, (state, source, output))| ItemResult {
            index: index as u32, asset_id: format!("asset-{index}"), output_name: format!("output-{index}.mkv"),
            job_id: state.as_ref().map(|_| format!("job-{index}")), state,
            action: None, artifact_id: output.map(|_| format!("job-{index}")), source_bytes: source.into(),
            output_bytes: output.map(String::from),
            error: if index == 0 { Some(crate::views::ErrorView { code: "INCOMPATIBLE_PRESERVATION".into(), message: None, retryable: false }) } else { None },
        }).collect::<Vec<_>>();
        BatchResult {
            schema_version: 1, id: "batch".into(), plan_id: "plan".into(), plan_hash: "ab".repeat(32),
            total: items.len(), finished: 0, succeeded: 0, skipped: 0, failed: 0, cancelled: 0, interrupted: 0,
            complete: false, outcome: String::new(), successful_video_input_bytes: String::new(),
            successful_video_output_bytes: String::new(), successful_video_size_delta_bytes: String::new(), items,
        }
    }
    #[test]
    fn every_terminal_candidate_finishes_but_only_verified_success_changes_size_totals() {
        let mut batch = result();
        batch.finish_counts().unwrap();
        assert_eq!((batch.finished, batch.succeeded, batch.failed, batch.skipped, batch.cancelled, batch.interrupted), (6,1,2,1,1,1));
        assert_eq!(batch.successful_video_input_bytes, "100");
        assert_eq!(batch.successful_video_output_bytes, "40");
        assert_eq!(batch.successful_video_size_delta_bytes, "-60");
        assert_eq!(batch.outcome, "partial_failure");
        batch.finish_counts().unwrap();
        assert_eq!(batch.finished, 6, "Inspection never accumulates repeated counts");
        batch.items[1].output_bytes = None;
        assert!(batch.finish_counts().is_err(), "Success cannot lack verified output bytes");
    }
    #[test]
    fn rejected_candidates_require_an_explicit_continue_policy() {
        let mut plan = BatchPlan {
            schema_version: 1, id: "plan".into(), creator: "operator".into(), data_root: "root".into(),
            hash: String::new(), created_at: "100".into(), expires_at: "200".into(),
            failure_policy: FailurePolicy::RejectAll, unresolved_policy: UnresolvedPolicy::Reject, unresolved: Vec::new(),
            items: vec![PlannedItem { index: 0, asset_id: "asset".into(), source_sha256: "ab".repeat(32),
                source_bytes: "100".into(), output_name: "output.mkv".into(), requested_output: "/exports/output.mkv".into(), plan: None, action: None,
                error: Some(DomainError::new("INCOMPATIBLE_PRESERVATION", "Explicit rejected candidate")) }],
        };
        plan.hash = plan.digest().unwrap();
        assert_eq!(plan.admits_submission().unwrap_err().code, "BATCH_REJECTED");
        let before = plan.hash.clone();
        plan.failure_policy = FailurePolicy::ContinueWithRejected;
        plan.hash = plan.digest().unwrap();
        assert_ne!(plan.hash, before);
        plan.admits_submission().unwrap();
        let semantic = plan.hash.clone();
        plan.creator = "another-planner".into();
        plan.items[0].asset_id = "another-registration".into();
        plan.items[0].error.as_mut().unwrap().message = "Redacted or translated display text".into();
        assert_eq!(plan.digest().unwrap(), semantic, "Creation and display audit does not change batch execution identity");
        let mut raw = serde_json::to_value(&plan).unwrap();
        raw["items"][0]["future_selection"] = true.into();
        assert!(serde_json::from_value::<BatchPlan>(raw).is_err());
        plan.unresolved.push(UnresolvedIndex { kind: crate::mux::StreamKind::Audio, source_index: 0 });
        plan.hash = plan.digest().unwrap();
        assert_eq!(plan.admits_submission().unwrap_err().code, "UNRESOLVED_SOURCE_INDEX");
        let rejected_hash = plan.hash.clone();
        plan.unresolved_policy = UnresolvedPolicy::PerFileNoMatch;
        plan.hash = plan.digest().unwrap();
        assert_ne!(plan.hash, rejected_hash);
        plan.admits_submission().unwrap();
        assert_eq!(plan.unresolved[0].source_index, 0, "Original index zero is never a sentinel");

    }
}
