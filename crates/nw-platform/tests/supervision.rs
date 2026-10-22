use std::{path::Path,time::Duration};
use tokio_util::sync::CancellationToken;
fn fixture()->&'static Path{Path::new(env!("CARGO_BIN_EXE_nw-process-fixture"))}
#[tokio::test]
async fn independent_pipe_draining_stays_bounded_under_flood(){
    let result=nw_platform::process::run(fixture(),&["flood".into()],Duration::from_secs(10),1024,CancellationToken::new()).await;
    assert_eq!(result.unwrap_err().code,"RESOURCE_LIMIT");
}
#[tokio::test]
async fn cancellation_waits_for_termination_and_reaping(){
    let token=CancellationToken::new();let trigger=token.clone();
    tokio::spawn(async move{tokio::time::sleep(Duration::from_millis(200)).await;trigger.cancel();});
    let result=nw_platform::process::run(fixture(),&["sleep".into()],Duration::from_secs(10),1024,token).await;
    assert_eq!(result.unwrap_err().code,"CANCELLED");
}
#[tokio::test]
async fn launch_failure_and_child_failure_remain_distinct(){
    let fail=nw_platform::process::run(fixture(),&["failure".into()],Duration::from_secs(10),1024,CancellationToken::new()).await.unwrap_err();assert_eq!(fail.code,"PROCESS_FAILED");
    let missing=fixture().with_file_name("does-not-exist-numera-fixture");
    let launch=nw_platform::process::run(&missing,&[],Duration::from_secs(10),1024,CancellationToken::new()).await.unwrap_err();assert_eq!(launch.code,"PROCESS_LAUNCH_FAILED");
}
