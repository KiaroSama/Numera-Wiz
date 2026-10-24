undefined
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
        } else { None };
        let policy = if disabled { DeadlinePolicy::Disabled } else { DeadlinePolicy::Finite { seconds: 1 } };
        let started = std::time::Instant::now();
        let result = nw_platform::process::run_readonly_with_policy(
            fixture(), &["sleep".into()], policy, 1024, input, token,
        ).await.unwrap_err();
        assert_eq!(result.code, if disabled { "CANCELLED" } else { "PROCESS_TIMEOUT" });
        assert!(started.elapsed() < Duration::from_secs(8));
        if let Some(task) = cancellation { task.await.unwrap(); }
    }
}
