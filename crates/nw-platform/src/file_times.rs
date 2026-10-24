//! Apply access/modification timestamps only to an owned verified staging handle.
use crate::files::{SourceStamp, io_error};
use nw_domain::{
    DomainError, Result,
    file_times::{FileTimeOutcome, FileTimePlan, FileTimeRequest, FileTimesSnapshot, SCOPE},
};
use std::{
    fs::{File, FileTimes},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn encode(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_nanos().to_string(),
        Err(error) => format!("-{}", error.duration().as_nanos()),
    }
}
fn decode(value: &str) -> Result<SystemTime> {
    let value = value.parse::<i128>().map_err(io_error)?;
    let magnitude = value.unsigned_abs();
    let seconds = u64::try_from(magnitude / 1_000_000_000)
        .map_err(|_| unavailable("File time exceeds the native clock range"))?;
    let duration = Duration::new(seconds, (magnitude % 1_000_000_000) as u32);
    let time = if value < 0 {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    }
    .ok_or_else(|| unavailable("File time exceeds the native clock range"))?;
    if encode(time) != value.to_string() {
        return Err(unavailable("File time exceeds the native clock precision"));
    }
    Ok(time)
}
fn unavailable(error: impl std::fmt::Display) -> DomainError {
    DomainError::new("NATIVE_FILE_TIMES_UNAVAILABLE", error.to_string())
}
pub fn capture(file: &File) -> Result<FileTimesSnapshot> {
    capture_metadata(&file.metadata().map_err(unavailable)?)
}
pub fn capture_metadata(metadata: &std::fs::Metadata) -> Result<FileTimesSnapshot> {
    Ok(FileTimesSnapshot {
        accessed_ns: encode(metadata.accessed().map_err(unavailable)?),
        modified_ns: encode(metadata.modified().map_err(unavailable)?),
    })
}
pub fn plan(source: &SourceStamp, request: FileTimeRequest) -> Result<FileTimePlan> {
    source.revalidate()?;
    Ok(match request {
        FileTimeRequest::New => FileTimePlan::New,
        FileTimeRequest::PreserveBestEffort => FileTimePlan::PreserveBestEffort,
        FileTimeRequest::RequireReviewed => FileTimePlan::RequireReviewed {
            times: capture(source.held_file())?,
        },
    })
}
fn set(file: &File, times: &FileTimesSnapshot) -> Result<()> {
    times.validate()?;
    file.set_times(
        FileTimes::new()
            .set_accessed(decode(&times.accessed_ns)?)
            .set_modified(decode(&times.modified_ns)?),
    )
    .map_err(unavailable)?;
    file.sync_all().map_err(unavailable)
}
pub fn apply(source: &SourceStamp, staging: &File, plan: &FileTimePlan) -> Result<FileTimeOutcome> {
    apply_with(source, staging, plan, set)
}
fn apply_with(
    source: &SourceStamp,
    staging: &File,
    plan: &FileTimePlan,
    setter: impl FnOnce(&File, &FileTimesSnapshot) -> Result<()>,
) -> Result<FileTimeOutcome> {
    plan.validate()?;
    source.revalidate()?;
    let mut result = FileTimeOutcome {
        policy: plan.request(),
        desired: None,
        observed: None,
        preserved: None,
        warnings: Vec::new(),
        scope: SCOPE.into(),
    };
    if matches!(plan, FileTimePlan::New) {
        result.observed = capture(staging).ok();
        return Ok(result);
    }
    let desired = match plan {
        FileTimePlan::RequireReviewed { times } => Ok(times.clone()),
        _ => capture(source.held_file()),
    };
    let applied = desired.and_then(|desired| {
        result.desired = Some(desired.clone());
        setter(staging, &desired)?;
        let observed = capture(staging)?;
        result.observed = Some(observed.clone());
        if observed != desired {
            return Err(unavailable(
                "Filesystem timestamp precision did not retain the requested values",
            ));
        }
        Ok(())
    });
    source.revalidate()?;
    match applied {
        Ok(()) => result.preserved = Some(true),
        Err(error) if matches!(plan, FileTimePlan::RequireReviewed { .. }) => {
            return Err(DomainError::new(
                "FILE_TIMES_REQUIRED",
                format!(
                    "Required reviewed file timestamps were not retained: {}",
                    error.message
                ),
            )
            .field("file_times"));
        }
        Err(error) => {
            result.preserved = Some(false);
            result.warnings.push(
                DomainError::new("FILE_TIMES_NOT_PRESERVED", error.message).field("file_times"),
            );
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio_util::sync::CancellationToken;
    fn source(root: &std::path::Path) -> SourceStamp {
        let path = root.join("source.bin");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"Timestamp fixture, byte identity is independent")
            .unwrap();
        let base = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        file.set_times(
            FileTimes::new()
                .set_accessed(base + Duration::from_nanos(111_111_100))
                .set_modified(base + Duration::from_nanos(123_456_700)),
        )
        .unwrap();
        drop(file);
        SourceStamp::capture_resolved(&std::fs::canonicalize(path).unwrap()).unwrap()
    }
    #[test]
    fn exact_reviewed_times_survive_verified_copy_and_no_replace_publication() {
        let root = tempfile::tempdir().unwrap();
        let original = source(root.path());
        let reviewed = plan(&original, FileTimeRequest::RequireReviewed).unwrap();
        let expected = match &reviewed {
            FileTimePlan::RequireReviewed { times } => times.clone(),
            _ => unreachable!(),
        };
        let input = std::fs::canonicalize(root.path().join("source.bin")).unwrap();
        let identity = crate::files::identity(&input, &CancellationToken::new()).unwrap();
        let staging = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        let (staging, actual, held) = crate::files::copy_into_staging_bound_held(
            &identity,
            &original.object_identity().unwrap(),
            staging,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(actual.sha256, identity.sha256);
        let outcome = apply(&held, staging.as_file(), &reviewed).unwrap();
        assert_eq!(outcome.preserved, Some(true));
        let output = root.path().join("copied.bin");
        drop(staging.persist_noclobber(&output).unwrap());
        assert_eq!(
            capture_metadata(&std::fs::metadata(output).unwrap()).unwrap(),
            expected
        );
    }
    #[test]
    fn timestamp_failure_warns_only_for_the_explicit_best_effort_policy() {
        let root = tempfile::tempdir().unwrap();
        let source = source(root.path());
        let staged = tempfile::tempfile().unwrap();
        let fail = |_: &File, _: &FileTimesSnapshot| {
            Err(unavailable("Injected metadata permission denial"))
        };
        let best = apply_with(&source, &staged, &FileTimePlan::PreserveBestEffort, fail).unwrap();
        assert_eq!(best.preserved, Some(false));
        assert_eq!(best.warnings[0].code, "FILE_TIMES_NOT_PRESERVED");
        let required = plan(&source, FileTimeRequest::RequireReviewed).unwrap();
        assert_eq!(
            apply_with(&source, &staged, &required, fail)
                .unwrap_err()
                .code,
            "FILE_TIMES_REQUIRED"
        );
    }
    #[test]
    fn new_timestamp_policy_never_invokes_a_timestamp_writer() {
        let root = tempfile::tempdir().unwrap();
        let source = source(root.path());
        let staged = tempfile::tempfile().unwrap();
        let result = apply_with(&source, &staged, &FileTimePlan::New, |_, _| {
            panic!("New timestamps do not request metadata replacement")
        })
        .unwrap();
        assert_eq!(result.preserved, None);
        assert!(result.warnings.is_empty());
    }
    #[test]
    fn exact_negative_epoch_values_round_trip_without_float() {
        for value in ["-100", "0", "1700000000123456700"] {
            assert_eq!(encode(decode(value).unwrap()), value);
        }
    }
}
