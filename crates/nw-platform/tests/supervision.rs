use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;
fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_nw-process-fixture"))
}
#[tokio::test]
async fn independent_pipe_draining_stays_bounded_under_flood() {
    let result = nw_platform::process::run(
        fixture(),
        &["flood".into()],
        Duration::from_secs(10),
        1024,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(result.unwrap_err().code, "RESOURCE_LIMIT");
}
#[tokio::test]
async fn cancellation_waits_for_termination_and_reaping() {
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        trigger.cancel();
    });
    let result = nw_platform::process::run(
        fixture(),
        &["sleep".into()],
        Duration::from_secs(10),
        1024,
        token,
    )
    .await;
    assert_eq!(result.unwrap_err().code, "CANCELLED");
}

#[tokio::test]
async fn rejected_streamed_stdout_terminates_and_reaps_the_producer() {
    let source = tempfile::NamedTempFile::new().unwrap();
    let input = std::fs::File::open(source.path()).unwrap();
    let start = std::time::Instant::now();
    let result = nw_platform::process::stream_readonly(
        fixture(),
        &["stream-sleep".into()],
        Duration::from_secs(15),
        input,
        Box::new(|_| {
            Err(nw_domain::DomainError::new(
                "VERIFICATION_FAILED",
                "Rejected deterministic stream record",
            ))
        }),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(result.code, "VERIFICATION_FAILED");
    assert!(start.elapsed() < Duration::from_secs(8));
}
#[tokio::test]
async fn launch_failure_and_child_failure_remain_distinct() {
    let fail = nw_platform::process::run(
        fixture(),
        &["failure".into()],
        Duration::from_secs(10),
        1024,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(fail.code, "PROCESS_FAILED");
    let missing = fixture().with_file_name("does-not-exist-numera-fixture");
    let launch = nw_platform::process::run(
        &missing,
        &[],
        Duration::from_secs(10),
        1024,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(launch.code, "PROCESS_LAUNCH_FAILED");
}

#[tokio::test]
async fn explicit_finite_policy_times_out_and_disabled_policy_still_cancels() {
    use nw_domain::execution::DeadlinePolicy;
    for disabled in [false, true] {
        let source = tempfile::NamedTempFile::new().unwrap();
        let input = std::fs::File::open(source.path()).unwrap();
        let token = CancellationToken::new();
        let trigger = token.clone();
        let cancellation = if disabled {
            Some(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                trigger.cancel();
            }))
        } else {
            None
        };
        let policy = if disabled {
            DeadlinePolicy::Disabled
        } else {
            DeadlinePolicy::Finite { seconds: 1 }
        };
        let started = std::time::Instant::now();
        let result = nw_platform::process::run_readonly_with_policy(
            fixture(),
            &["sleep".into()],
            policy,
            1024,
            input,
            token,
        )
        .await
        .unwrap_err();
        assert_eq!(
            result.code,
            if disabled {
                "CANCELLED"
            } else {
                "PROCESS_TIMEOUT"
            }
        );
        assert!(started.elapsed() < Duration::from_secs(8));
        if let Some(task) = cancellation {
            task.await.unwrap();
        }
    }
}
