//! Verify picture-only retiming against the original decoded frame identities.
use crate::{Engine, Result, blocking, io_error, lower_remux, plan_digest, registered_object};
use crate::output_profile::OutputProfile;
use nw_domain::{
    DomainError,
    mux::{CleanResolution, Stream, StreamKind},
    records::{Asset, MediaProbe, Plan},
    stream_observations::{self, Kind},
    video_retime::{MAX_FRAMES, MAX_REVERSE_BYTES, Retime, RetimeInput, milliseconds},
};
use nw_platform::{files, process};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) const SCOPE: &str = "Verify every output picture against the original decoded frame digest and reviewed variable-rate millisecond clock. The output is lossless FFV1 without audio or chapters.";

fn unavailable(message: &str) -> DomainError {
    DomainError::new("RETIME_UNQUALIFIED", message)
}
fn failed(message: &str) -> DomainError {
    DomainError::new("VERIFICATION_FAILED", message)
}

fn selected_picture(probe: &MediaProbe, index: u32) -> Result<(u64, u64)> {
    let track = probe.streams.iter().find(|track| track.index == index)
        .filter(|track| track.kind == StreamKind::Video && !track.attached_picture() && track.codec == "ffv1")
        .ok_or_else(|| unavailable("Choose an ordinary original FFV1 picture"))?;
    let facts = probe.stream_details.get(&track.index).ok_or_else(|| unavailable("Missing original picture format"))?;
    let dimensions = |field: &str, max| facts[field].as_u64().filter(|value| (2..=max).contains(value) && value % 2 == 0);
    if facts["time_base"] != "1/1000" || facts["pix_fmt"] != "yuv420p"
        || facts["side_data_list"].as_array().is_some_and(|rows| !rows.is_empty())
    {
        return Err(unavailable("Retiming admits ordinary yuv420p picture with a millisecond source clock"));
    }
    dimensions("width",4096).zip(dimensions("height",2160)).ok_or_else(|| unavailable("Missing or excessive source picture dimensions"))
}

fn validate_profile(probe: &MediaProbe, edit: &Retime) -> Result<()> {
    let (width,height) = selected_picture(probe, edit.video_stream)?;
    edit.output_clock()?;
    if edit.reverse && width * height * 3 / 2 * edit.input_clock_ms.len() as u64 > MAX_REVERSE_BYTES {
        return Err(DomainError::new("RESOURCE_LIMIT", "Reverse selection exceeds the 64 MiB decoded frame budget"));
    }
    Ok(())
}

fn resolution(streams: &[Stream], index: u32) -> Result<CleanResolution> {
    let mut resolution = nw_domain::mux::extract_one(streams, index)?;
    resolution.reasons = vec!["Render the reviewed frame order and presentation clock as lossless FFV1".into()];
    if let Some(outcome) = resolution.outcomes.iter_mut().find(|outcome| outcome.source_index == index) {
        outcome.reason = "Apply the reviewed picture frame clock and playback order".into();
    }
    Ok(resolution)
}

impl Engine {
    pub async fn inspect_video_retime(
        &self,
        source: &Asset,
        request: &RetimeInput,
        cancel: CancellationToken,
    ) -> Result<(files::SourceStamp, Retime)> {
        request.range.validate()?;
        milliseconds(request.range.start)?;
        milliseconds(request.range.end)?;
        if source.id != request.asset_id || !(10..=800).contains(&request.tempo_hundredths)
            || crate::admission::frozen_demuxer(&source.probe.format)? != "matroska"
            || source.probe.container_duration.is_none_or(|duration| request.range.end > duration)
        {
            return Err(unavailable("Choose bounded original Matroska frame times within the registered source extent"));
        }
        selected_picture(&source.probe, request.video_stream)?;
        let identity = source.identity.clone();
        let object = registered_object(source)?.clone();
        let token = cancel.clone();
        let (stamp, scan_stamp) = blocking(move || {
            let path = Path::new(&identity.path);
            let stamp = files::SourceStamp::capture_resolved(path)?;
            stamp.require_object(&object)?;
            files::revalidate(&identity, &token)?;
            let scan_stamp = files::SourceStamp::capture_open_resolved(path, stamp.try_clone_file()?)?;
            stamp.revalidate()?;
            Ok((stamp, scan_stamp))
        }).await?;
        let observations = self.observe_stream(source, scan_stamp, stream_observations::Input {
            asset_id: source.id.clone(), stream_index: request.video_stream, kind: Kind::Frames,
            first: 0, limit: MAX_FRAMES as u32,
        }, cancel).await?;
        if observations.observed.parse::<usize>().ok() != Some(observations.rows.len()) {
            return Err(unavailable("The current picture-retime profile admits an original clip of at most 64 decoded frames"));
        }
        let mut clock = Vec::new();
        for frame in observations.rows {
            let pts = frame.pts.ok_or_else(|| unavailable("A decoded original frame has no presentation timestamp"))?;
            if request.range.contains(pts) {
                clock.push(milliseconds(pts)?);
            }
        }
        let edit = Retime {
            video_stream: request.video_stream, range: request.range,
            reverse: request.reverse, tempo_hundredths: request.tempo_hundredths, input_clock_ms: clock,
        };
        validate_profile(&source.probe, &edit)?;
        stamp.revalidate()?;
        Ok((stamp, edit))
    }

    pub fn video_retime_plan_at(
        &self, source: Asset, output: &Path, edit: Retime, creator: &str, now: u64,
    ) -> Result<Plan> {
        let mut plan = self.review_stream_copy(source, output, Default::default(), creator, now,
            |streams| resolution(streams, edit.video_stream))?;
        plan.video_retime = Some(edit);
        plan.verification_scope = SCOPE.into();
        plan.argv_template = invocation(&plan, OutputProfile::Matroska)?;
        plan.hash = plan_digest(&plan)?;
        self.validate_plan(&plan)?;
        Ok(plan)
    }

    pub(super) async fn verify_video_retime(
        &self, plan: &Plan, path: &Path, output: &MediaProbe, cancel: CancellationToken,
    ) -> Result<String> {
        let edit = plan.video_retime.as_ref().expect("Reviewed picture retime");
        let expected_shape = selected_picture(&plan.source.probe, edit.video_stream)?;
        if selected_picture(output, 0)? != expected_shape {
            return Err(failed("Rendered picture dimensions changed"));
        }
        let source_shape = &plan.source.probe.stream_details[&edit.video_stream];
        let output_shape = &output.stream_details[&0];
        if ["sample_aspect_ratio","color_space","color_range","color_transfer","color_primaries"].iter()
            .any(|&key| source_shape[key] != output_shape[key])
        {
            return Err(failed("Rendered picture aspect ratio or color signaling changed"));
        }
        let (first, last) = (milliseconds(edit.range.start)?, milliseconds(edit.range.end)?);
        let trim = format!("settb=1/1000,trim=start_pts={first}:end_pts={last}");
        let mut original = self.frame_hashes(Path::new(&plan.source.identity.path), edit.video_stream, Some(trim), cancel.clone()).await?;
        if edit.reverse {
            original.reverse();
        }
        let rendered = self.frame_hashes(path, 0, None, cancel).await?;
        let clock = edit.output_clock()?;
        if original.len() != rendered.len() || rendered.len() != clock.len()
            || !rendered.iter().zip(original).zip(&clock).all(|((actual, source), timestamp)| {
                actual.0 == *timestamp && actual.1 == source.1
            })
        {
            return Err(failed("Rendered frame digests or variable-rate presentation times differ from the reviewed sequence"));
        }
        Ok(format!("Verified {} lossless decoded frame digests and variable-rate presentation times at {}% tempo.", clock.len(), edit.tempo_hundredths))
    }

    async fn frame_hashes(
        &self, path: &Path, video_stream: u32, filter: Option<String>, cancel: CancellationToken,
    ) -> Result<Vec<(u32, String)>> {
        let source = path.to_owned();
        let token = cancel.clone();
        let input = blocking(move || crate::admission::open(&source, &token)).await?;
        let mut args = ["-v","error","-nostdin","-copyts","-noautorotate"].map(str::to_owned).to_vec();
        args.extend(input.options());
        args.extend(["-i".into(),"fd:".into(),"-map".into(),format!("0:{video_stream}")]);
        if let Some(filter) = filter {
            args.extend(["-vf".into(),filter]);
        }
        args.extend(["-fps_mode","passthrough","-enc_time_base:v","1/1000","-pix_fmt","yuv420p",
            "-c:v","rawvideo","-threads","1","-f","framehash","-hash","sha256","pipe:1"].map(str::to_owned));
        let ticket = self.process_budget.acquire(&cancel).await?;
        let result = process::run_readonly(Path::new(&self.ffmpeg.path), &args, Duration::from_secs(60),
            64 * 1024, input.file, cancel).await;
        let result = ticket.finish(result)?;
        let text = std::str::from_utf8(&result.stdout).map_err(|_| failed("Decoded frame inventory is not UTF-8"))?;
        if !text.lines().any(|line| line == "#tb 0: 1/1000") {
            return Err(failed("Decoded frame inventory time base differs from the reviewed clock"));
        }
        text.lines().filter(|line| !line.starts_with('#') && !line.trim().is_empty()).map(|line| {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            let [_, _, pts, _, _, hash] = fields.as_slice() else { return Err(failed("Invalid frame hash record")); };
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(failed("Invalid decoded frame digest"));
            }
            let pts = pts.parse::<u32>().map_err(|_| failed("Invalid decoded frame presentation time"))?;
            Ok((pts, (*hash).to_owned()))
        }).collect()
    }
}

fn expression(clock: &[u32], first: usize) -> String {
    if let [timestamp] = clock {
        return timestamp.to_string();
    }
    let middle = clock.len()/2;
    let (before, after) = clock.split_at(middle);
    format!("if(lt(N\\,{})\\,{}\\,{})", first + middle, expression(before, first), expression(after, first + middle))
}

pub(super) fn invocation(plan: &Plan, profile: OutputProfile) -> Result<Vec<String>> {
    let edit = plan.video_retime.as_ref().ok_or_else(|| unavailable("Missing reviewed picture frame schedule"))?;
    validate_profile(&plan.source.probe, edit)?;
    if profile != OutputProfile::Matroska || crate::admission::frozen_demuxer(&plan.source.probe.format)? != "matroska"
        || plan.resolution.streams != resolution(&plan.source.probe.streams, edit.video_stream)?.streams
    {
        return Err(unavailable("The bounded picture-retime profile needs original FFV1 and a new Matroska output"));
    }
    let (first, last) = (milliseconds(edit.range.start)?, milliseconds(edit.range.end)?);
    let reverse = if edit.reverse { "reverse," } else { "" };
    let graph = format!("settb=1/1000,trim=start_pts={first}:end_pts={last},{reverse}setpts={}",expression(&edit.output_clock()?,0));
    let mut args = lower_remux(&plan.source, &plan.resolution, profile)?;
    let input = args.iter().position(|arg| arg == "-i").expect("Shared held source");
    args.splice(input..input, ["-noautorotate", "-threads", "2"].map(str::to_owned));
    let offset = args.len()-3;
    args.splice(offset..offset, [
        "-filter_threads".into(),"1".into(),"-vf".into(),graph,"-fps_mode:v".into(),"passthrough".into(),
        "-enc_time_base:v".into(),"1/1000".into(),"-c:v".into(),"ffv1".into(),
        "-threads:v".into(),"2".into(),"-pix_fmt".into(),"yuv420p".into(),
    ]);
    Ok(args)
}
