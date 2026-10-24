//! Bounded FFmpeg progress parsing. The selected muxer's counters are observations.
use crate::{ExecutionEvent, Observer};
use nw_domain::{DomainError, Result, execution::RemuxProgress};
use std::{collections::BTreeSet, path::Path, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio_util::sync::CancellationToken;

fn invalid(message: &str) -> DomainError {
    DomainError::new("INVALID_PROGRESS", message)
}
#[derive(Default)]
struct Block {
    seen: BTreeSet<String>,
    bytes: usize,
    time: Option<String>,
    output: Option<String>,
    frames: Option<String>,
}
struct Parser {
    line: Vec<u8>,
    block: Block,
    latest: Option<RemuxProgress>,
    sequence: u64,
    ended: bool,
    started: Instant,
}
impl Parser {
    fn new() -> Self {
        Self { line: Vec::new(), block: Block::default(), latest: None, sequence: 0, ended: false, started: Instant::now() }
    }
    fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        for byte in bytes {
            if *byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                let text = std::str::from_utf8(&line).map_err(|_| invalid("FFmpeg progress is not UTF-8"))?;
                self.record(text.trim_end_matches('\r'))?;
            } else {
                if self.line.len() >= 4096 { return Err(invalid("FFmpeg progress line exceeds 4096 bytes")); }
                self.line.push(*byte);
            }
        }
        Ok(())
    }
    fn record(&mut self, line: &str) -> Result<()> {
        if line.is_empty() { return Ok(()); }
        if self.ended { return Err(invalid("FFmpeg progress continued after its terminal block")); }
        let (key, value) = line.split_once('=').ok_or_else(|| invalid("Malformed FFmpeg progress line"))?;
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || value.len() > 1024 || self.block.seen.len() >= 1024 || !self.block.seen.insert(key.into())
        {
            return Err(invalid("FFmpeg progress keys/values are duplicate, invalid or excessive"));
        }
        self.block.bytes += line.len();
        if self.block.bytes > 128 * 1024 { return Err(invalid("FFmpeg progress block exceeds 128 KiB")); }
        let unsigned = |value: &str| -> Result<Option<String>> {
            if value == "N/A" { return Ok(None); }
            let parsed = value.parse::<u64>().map_err(|_| invalid("Invalid FFmpeg unsigned counter"))?;
            if parsed.to_string() != value { return Err(invalid("Noncanonical FFmpeg counter")); }
            Ok(Some(value.into()))
        };
        match key {
            "out_time_us" => {
                self.block.time = if value == "N/A" { None } else {
                    let parsed = value.parse::<i64>().map_err(|_| invalid("Invalid FFmpeg progress clock"))?;
                    if parsed.to_string() != value { return Err(invalid("Noncanonical FFmpeg progress clock")); }
                    Some(value.into())
                };
            }
            "total_size" => self.block.output = unsigned(value)?,
            "frame" => self.block.frames = unsigned(value)?,
            "progress" => {
                self.ended = match value {
                    "continue" => false, "end" => true,
                    _ => return Err(invalid("Unknown FFmpeg progress block terminator")),
                };
                let progress = RemuxProgress {
                    reported_out_time_us: self.block.time.take(), output_bytes: self.block.output.take(),
                    reported_frame_count: self.block.frames.take(),
                    elapsed_ms: self.started.elapsed().as_millis().min(u128::from(u64::MAX)).to_string(),
                    coalesced_updates: "0".into(), process_progress_ended: self.ended,
                };
                progress.validate()?;
                self.sequence = self.sequence.checked_add(1).ok_or_else(|| invalid("Progress sequence overflow"))?;
                self.latest = Some(progress);
                self.block = Block::default();
            }
            _ => {}
        }
        Ok(())
    }
    fn finish(&self) -> Result<()> {
        if !self.line.is_empty() || !self.block.seen.is_empty() || !self.ended {
            return Err(invalid("FFmpeg progress stream is truncated or lacks its terminal block"));
        }
        Ok(())
    }
    fn snapshot(&self) -> (u64, Option<RemuxProgress>) {
        (self.sequence, self.latest.clone())
    }
}
fn snapshot(parser: &Mutex<Parser>) -> Result<(u64, Option<RemuxProgress>)> {
    Ok(parser.lock().map_err(|_| invalid("Progress parser lock poisoned"))?.snapshot())
}
async fn publish(
    parser: &Mutex<Parser>, observer: &Observer, last: &mut u64, emitted: &mut u64,
) -> Result<()> {
    let (sequence, progress) = snapshot(parser)?;
    if sequence == *last { return Ok(()); }
    if let Some(mut progress) = progress {
        *emitted = emitted.checked_add(1).ok_or_else(|| invalid("Progress publication overflow"))?;
        progress.coalesced_updates = sequence.saturating_sub(*emitted).to_string();
        observer(ExecutionEvent::RemuxProgress(progress)).await?;
        *last = sequence;
    }
    Ok(())
}
pub async fn run(
    program: &Path, args: &[String], deadline: nw_domain::execution::DeadlinePolicy,
    input: std::fs::File, cancel: CancellationToken, observer: &Observer,
) -> Result<nw_platform::process::RunOutput> {
    let parser = Arc::new(Mutex::new(Parser::new()));
    let consumer = parser.clone();
    let owned_cancel = cancel.child_token();
    let process = nw_platform::process::stream_readonly_with_policy(
        program, args, deadline, input,
        Box::new(move |bytes| consumer.lock().map_err(|_| invalid("Progress parser lock poisoned"))?.feed(bytes)),
        owned_cancel.clone(),
    );
    tokio::pin!(process);
    let mut interval = tokio::time::interval(Duration::from_millis(nw_domain::execution::PROGRESS_INTERVAL_MS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    let mut last = 0;
    let mut emitted = 0;
    let result = loop {
        tokio::select! {
            result = &mut process => break result,
            _ = interval.tick() => {
                if let Err(error) = publish(&parser, observer, &mut last, &mut emitted).await {
                    owned_cancel.cancel();
                    let drained = (&mut process).await;
                    if let Err(termination) = drained && termination.code == "TREE_TERMINATION_UNCERTAIN" {
                        return Err(termination);
                    }
                    return Err(error);
                }
            }
        }
    }?;
    parser.lock().map_err(|_| invalid("Progress parser lock poisoned"))?.finish()?;
    publish(&parser, observer, &mut last, &mut emitted).await?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_is_incremental_bounded_and_unknown_is_not_zero() {
        let mut parser = Parser::new();
        let wire = b"frame=2\nout_time_us=N/A\ntotal_size=N/A\nfps=0.00\nprogress=continue\nout_time_us=-1200\ntotal_size=9007199254740993\nframe=4\nprogress=end\n";
        for byte in wire { parser.feed(&[*byte]).unwrap(); }
        parser.finish().unwrap();
        let latest = parser.latest.unwrap();
        assert_eq!(latest.reported_out_time_us.as_deref(), Some("-1200"));
        assert_eq!(latest.output_bytes.as_deref(), Some("9007199254740993"));
        assert!(latest.process_progress_ended);
        let mut unknown = Parser::new();
        unknown.feed(b"out_time_us=N/A\ntotal_size=N/A\nprogress=end\n").unwrap();
        assert!(unknown.latest.as_ref().unwrap().reported_out_time_us.is_none());
        assert!(unknown.latest.as_ref().unwrap().output_bytes.is_none());
    }
    #[test]
    fn malformed_duplicate_nonfinite_and_truncated_progress_is_rejected() {
        for wire in [
            b"frame=1\nframe=2\n".as_slice(), b"out_time_us=NaN\n",
            b"total_size=-1\n", b"progress=unexpected\n",
            b"progress=end\nframe=3\n",
        ] { assert!(Parser::new().feed(wire).is_err()); }
        let mut parser = Parser::new();
        assert!(parser.feed(&[b'x'; 4097]).is_err());
        let mut parser = Parser::new();
        parser.feed(b"frame=2\nprogress=continue\n").unwrap();
        assert!(parser.finish().is_err());
    }
}
