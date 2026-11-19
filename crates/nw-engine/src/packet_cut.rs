//! A Matroska packet cut uses original keyframes and preserves the source clock.
use super::{Engine, Result, blocking, lower_remux, plan_digest, registered_object};
use nw_domain::{
    DomainError,
    mux::{self, CleanAction, CleanIntent},
    packet_cut,
    records::{Asset, Plan},
    time::TimeRange,
};
use nw_platform::files;
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::output_profile::OutputProfile;

impl Engine {
    pub async fn inspect_packet_cut(
        &self,
        asset: &Asset,
        range: TimeRange,
        cancel: CancellationToken,
    ) -> Result<files::SourceStamp> {
        packet_cut::validate(range)?;
        let stamp = cut_source(asset, &cancel).await?;
        crate::preservation::check_cut(self, asset, range, cancel).await?;
        stamp.revalidate()?;
        Ok(stamp)
    }

    pub fn packet_cut_plan_at(
        &self,
        source: Asset,
        output: &Path,
        range: TimeRange,
        creator: &str,
        now: u64,
    ) -> Result<Plan> {
        packet_cut::validate(range)?;
        let mut plan = self.review_stream_copy(source, output, Default::default(), creator, now, |streams| {
            let mut intent = CleanIntent::preserving_all();
            intent.preservation.chapters = false;
            let mut result = mux::resolve(streams, &intent)?;
            result.reasons.push("Copy the requested original packet-clock interval".into());
            Ok(result)
        })?;
        plan.packet_cut = Some(range);
        plan.argv_template = invocation(&plan, OutputProfile::Matroska)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }
}

async fn cut_source(asset: &Asset, cancel: &CancellationToken) -> Result<files::SourceStamp> {
    let source = asset.identity.clone();
    let object = registered_object(asset)?.clone();
    let cancel = cancel.clone();
    blocking(move || {
        let stamp = files::SourceStamp::capture_resolved(Path::new(&source.path))?;
        stamp.require_object(&object)?;
        files::revalidate(&source, &cancel)?;
        stamp.revalidate()?;
        Ok(stamp)
    }).await
}

pub(super) fn invocation(plan: &Plan, profile: OutputProfile) -> Result<Vec<String>> {
    let range = plan.packet_cut.ok_or_else(|| DomainError::new("INVALID_PLAN", "Missing packet cut interval"))?;
    if profile != OutputProfile::Matroska
        || crate::admission::frozen_demuxer(&plan.source.probe.format)? != "matroska"
        || plan.resolution.action != CleanAction::Remux
        || plan.source.probe.streams != plan.resolution.streams
        || plan.resolution.preservation.chapters
    {
        return Err(DomainError::new("COPY_BOUNDARY_UNQUALIFIED", "Verified packet cuts require an ordinary Matroska input and output with chapters dropped explicitly"));
    }
    let mut args = lower_remux(&plan.source, &plan.resolution, profile)?;
    let input = args.iter().position(|arg| arg == "-protocol_whitelist").expect("Shared copy input");
    args.splice(input..input, ["-seek_timestamp".into(), "1".into(), "-ss".into(), packet_cut::ffmpeg_time(range.start)?]);
    let output = args.iter().position(|arg| arg == "-map").expect("Shared copy stream map");
    args.splice(output..output, ["-to".into(), packet_cut::ffmpeg_time(range.end)?]);
    Ok(args)
}
