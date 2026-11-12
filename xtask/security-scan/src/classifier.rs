//! Source-reviewed full historical file identities, never scanner suppression rules.
use crate::{Result, ScanFailure, classifier_git::Session, private, process, report, require};
use std::{collections::BTreeMap, time::Instant};

pub struct Document {
    pub path: &'static str,
    pub length: u64,
    pub sha256: &'static str,
}
pub const DOCUMENTS: [Document; 4] = [
    Document { path: "docs/research-pack/handoff/02-2-functional-scope-and-migration.md", length: 27483,
        sha256: "02a9a4eb5a09d04d089dfbf25728af963772af8016a6cc62495c5358ec1d4bed" },
    Document { path: "docs/research-pack/handoff/05-5-domain-model-and-interface-contracts.md", length: 96791,
        sha256: "01c780f92cebe1e2f50c2b880a76cfd658c6bc1613804533f0884adf912b00f4" },
    Document { path: "docs/research-pack/Numera-Wiz-Implementation-Mandate-EN.md", length: 772543,
        sha256: "126c8aa8aa7f874802a09600040a1986cff650ad70b70f58f30a441d87fbef06" },
    Document { path: "docs/research-pack/research_v7/filmcraft-source-commands.json", length: 344827,
        sha256: "dc79337d44c786b037dabe5c18ecfac9cfeb3ea6912f761d3dc0d189590f1ed5" },
];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition { Clean, ReviewedPublicData, UnreviewedFindings }
pub struct Counts {
    pub total: usize,
    pub reviewed: usize,
    pub unreviewed: usize,
    pub disposition: Disposition,
}
impl Counts {
    pub fn checked(total: usize, reviewed: usize, unreviewed: usize) -> Result<Self> {
        require(reviewed.checked_add(unreviewed) == Some(total),"SECURITY_CLASSIFIER_COUNT_DISAGREEMENT")?;
        Ok(Self { total, reviewed, unreviewed, disposition: if total == 0 { Disposition::Clean }
            else if unreviewed == 0 { Disposition::ReviewedPublicData } else { Disposition::UnreviewedFindings } })
    }
}
pub fn active(deadline: Instant, cancelled: bool) -> Result<()> {
    require(Instant::now() < deadline && !cancelled,"SECURITY_DEADLINE")
}
pub fn positive_cache_room(current: usize) -> Result<()> {
    require(current < DOCUMENTS.len(),"SECURITY_CLASSIFIER_CACHE_LIMIT")
}
pub struct Plan {
    total: usize,
    unknown: usize,
    candidates: BTreeMap<(String,usize),usize>,
    report_hash: String,
}
impl Plan {
    pub fn from_report(bytes: &[u8], expected: usize, deadline: Instant) -> Result<Self> {
        active(deadline,process::cancelled())?;
        let mut plan = Self { total: 0, unknown: 0, candidates: BTreeMap::new(), report_hash: private::hash(bytes) };
        let count = report::visit_rows(bytes, |row| {
            active(deadline,process::cancelled())?;
            plan.total = plan.total.checked_add(1).ok_or(ScanFailure("SECURITY_CLASSIFIER_COUNT_DISAGREEMENT"))?;
            let path = DOCUMENTS.iter().position(|document| document.path == row.file);
            let entry = match (path,row.rule.as_str(),row.line.as_str()) {
                (Some(0),"generic-api-key","111") => Some(0),
                (Some(1),"generic-api-key","254") => Some(1),
                (Some(2),"generic-api-key","301" | "1070") => Some(2),
                (Some(3),"sourcegraph-access-token","2") => Some(3),
                _ => None,
            };
            if let Some(path) = entry.filter(|_| crate::protocol::revision(&row.commit)) {
                let key = (row.commit.to_ascii_lowercase(),path);
                if !plan.candidates.contains_key(&key) {
                    require(plan.candidates.len() < 256,"SECURITY_CLASSIFIER_RESOLUTION_LIMIT")?;
                }
                let count = plan.candidates.entry(key).or_default();
                *count = count.checked_add(1).ok_or(ScanFailure("SECURITY_CLASSIFIER_COUNT_DISAGREEMENT"))?;
            } else {
                plan.unknown = plan.unknown.checked_add(1).ok_or(ScanFailure("SECURITY_CLASSIFIER_COUNT_DISAGREEMENT"))?;
            }
            Ok(())
        })?;
        active(deadline,process::cancelled())?;
        require(count == expected && count == plan.total,"SECURITY_CLASSIFIER_COUNT_DISAGREEMENT")?;
        Ok(plan)
    }
    pub fn total(&self) -> usize { self.total }
    pub fn unknown(&self) -> usize { self.unknown }
    pub fn resolutions(&self) -> usize { self.candidates.len() }
    pub fn evaluate(self, session: &mut Session<'_>) -> Result<Counts> {
        session.bind_report(&self.report_hash)?;
        let mut reviewed = 0usize;
        let mut unreviewed = self.unknown;
        for ((commit,path),count) in self.candidates {
            let target = if session.resolve(&commit,path)? { &mut reviewed } else { &mut unreviewed };
            *target = target.checked_add(count).ok_or(ScanFailure("SECURITY_CLASSIFIER_COUNT_DISAGREEMENT"))?;
        }
        session.verify()?;
        Counts::checked(self.total,reviewed,unreviewed)
    }
}
