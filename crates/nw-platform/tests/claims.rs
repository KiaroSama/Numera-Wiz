//! Real OS claims across different executable copies and process environments.
use nw_platform::{claims::{Destination, DestinationClaims}, files::DirectoryStamp};
use std::{path::{Path, PathBuf}, process::Stdio, time::Duration};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::Command};
use tokio_util::sync::CancellationToken;

fn command(executable: &Path, directory: &Path) -> Command {
    let mut command = Command::new(executable);
    command.current_dir(directory)
        .env("HOME", directory).env("TMPDIR", directory).env("TEMP", directory)
        .env("APPDATA", directory).env("LOCALAPPDATA", directory)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}
async fn contender(executable: &Path, directory: &Path, output: &Path) -> std::process::Output {
    tokio::time::timeout(Duration::from_secs(15),
        command(executable, directory).arg("try-destination").arg(output).output())
        .await.expect("Independent destination contender deadline").unwrap()
}
fn copy_fixture(directory: &Path) -> PathBuf {
    std::fs::create_dir(directory).unwrap();
    let path = directory.join(if cfg!(windows) { "portable-fixture.exe" } else { "portable-fixture" });
    std::fs::copy(env!("CARGO_BIN_EXE_nw-process-fixture"), &path).unwrap();
    path
}
#[tokio::test]
async fn independent_portable_copies_share_absent_destination_and_alias_claims() {
    tokio::time::timeout(Duration::from_secs(105), async {
        let root = tempfile::tempdir().unwrap();
        let install_a = root.path().join("portable-a");
        let install_b = root.path().join("portable-b");
        let a = copy_fixture(&install_a);
        let b = copy_fixture(&install_b);
        let exports = root.path().join("exports");
        std::fs::create_dir(&exports).unwrap();
        let exports = std::fs::canonicalize(exports).unwrap();
        let output = exports.join("résumé.mkv");
        let mut holder = command(&a, &install_a).arg("hold-destination").arg(&output).spawn().unwrap();
        let stdout = holder.stdout.take().unwrap();
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(20), BufReader::new(stdout).read_line(&mut line))
            .await.expect("Actual claim holder readiness deadline").unwrap();
        assert_eq!(line.trim(), "CLAIM_READY");
        assert!(!output.exists(), "The claim covers an absent target");
        for target in [&output, &exports.join("RÉSUMÉ.mkv")] {
            let rejected = contender(&b, &install_b, target).await;
            assert_eq!(rejected.status.code(), Some(4), "Contender: {}", String::from_utf8_lossy(&rejected.stderr));
            assert_eq!(String::from_utf8_lossy(&rejected.stdout).trim(), "PUBLICATION_BUSY");
        }
        let parent = DirectoryStamp::capture_resolved(&exports).unwrap();
        let destination = Destination::reviewed(&output, &parent.object_identity().unwrap()).unwrap();
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let waiting = tokio::spawn(async move {
            DestinationClaims::acquire(vec![destination], token).await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(5), waiting)
            .await.expect("Claim cancellation deadline").unwrap();
        match result {
            Err(error) => assert_eq!(error.code, "CANCELLED"),
            Ok(_) => panic!("A separate live owner still holds this publication claim"),
        }
        holder.kill().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), holder.wait()).await.unwrap().unwrap();
        let admitted = contender(&b, &install_b, &output).await;
        assert!(admitted.status.success(), "Crash release: {}", String::from_utf8_lossy(&admitted.stderr));
        assert!(!output.exists(), "Acquiring a claim alone must never create output bytes");
    }).await.expect("Independent process publication claim deadline");
}
