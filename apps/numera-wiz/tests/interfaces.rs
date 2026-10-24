//! CLI and actual official-SDK stdio parity and authorization regressions.
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use std::process::Command;
// These cases launch independent unoptimized owners. Isolate those workloads so
// hosted VM contention does not consume the unchanged 20-second bootstrap budget.
// The dedicated same-root case below still starts three frontends concurrently.
static OWNER_CASE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[path = "support/evidence.rs"]
mod evidence;
fn retain_owner_logs(data: &std::path::Path, label: &str) -> evidence::RetainLogs {
    evidence::RetainLogs {
        source: data.join("logs"),
        destination: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/interface-evidence")
            .join(format!("{label}-{}", uuid::Uuid::new_v4())),
    }
}
#[test]
fn cli_stdout_is_canonical_json_and_index_zero_survives() {
    let _case = OWNER_CASE.blocking_lock();
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clean-resolve.json");
    let data = tempfile::tempdir().unwrap();
    let _logs = retain_owner_logs(data.path(), "cli");
    let output = Command::new(env!("CARGO_BIN_EXE_numera-wiz"))
        .arg("--data-dir")
        .arg(data.path())
        .args(["request", "--file"])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "CLI owner failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(
        response["result"]["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 3]
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("[INFO]"));
}
#[tokio::test]
async fn official_stdio_discovery_is_clean_and_does_not_inherit_owner() {
    let _case = OWNER_CASE.lock().await;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
    let data = tempfile::tempdir().unwrap();
    let _logs = retain_owner_logs(data.path(), "discovery");
    command.arg("--data-dir").arg(data.path()).arg("bridge");
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "jobs.submit"));
    assert!(!tools.iter().any(|tool| tool.name == "shell.execute"));
    let listed = client
        .call_tool(
            CallToolRequestParams::new("commands.list")
                .with_arguments(rmcp::object!({"params":{}})),
        )
        .await
        .unwrap();
    let value = serde_json::to_value(listed).unwrap();
    assert_eq!(
        value["structuredContent"]["ok"], true,
        "Owner command error: {}",
        value["structuredContent"]["error"]
    );
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/clean-resolve.json")).unwrap();
    let denied = client
        .call_tool(
            CallToolRequestParams::new("streams.clean.resolve")
                .with_arguments(rmcp::object!({"params":fixture["params"].clone()})),
        )
        .await
        .unwrap();
    let value = serde_json::to_value(denied).unwrap();
    assert_eq!(
        value["structuredContent"]["error"]["code"],
        "PERMISSION_DENIED"
    );
    client.cancel().await.unwrap();
}

async fn wait_for_idle_owner_unlock(root: &std::path::Path) {
    let start = std::time::Instant::now();
    loop {
        match nw_platform::files::RootLock::acquire(&root.join("state/generations/1")) {
            Ok(lock) => {
                drop(lock);
                return;
            }
            Err(error) => assert_eq!(error.code, "OWNER_BUSY"),
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(35),
            "Idle owner did not drain while its stdio bridge remained alive"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
#[tokio::test]
async fn official_stdio_reconnects_after_idle_without_changing_its_principal() {
    let _case = OWNER_CASE.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use serde_json::Value;
        let data = tempfile::tempdir().unwrap();
        let _logs = retain_owner_logs(data.path(), "reattach");
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command.arg("--data-dir").arg(data.path()).arg("bridge");
        let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        macro_rules! scope {
            ($client:expr) => {{
                let response = $client.call_tool(CallToolRequestParams::new("access.replay_scope.create")
                    .with_arguments(rmcp::object!({"params":{}}))).await.unwrap();
                let response = serde_json::to_value(response).unwrap();
                assert_eq!(response["structuredContent"]["ok"], true, "Owner scope error: {}",response["structuredContent"]["error"]);
                let scope = response["structuredContent"]["result"]["replay_scope"].as_str().unwrap();
                let payload = URL_SAFE_NO_PAD.decode(scope.split_once('.').unwrap().0).unwrap();
                serde_json::from_slice::<Value>(&payload).unwrap()
            }};
        }
        let first = scope!(client);
        let runtime_path = data.path().join("runtime/owner.json");
        let original: Value = serde_json::from_slice(&nw_platform::security::read_private(&runtime_path,16384).unwrap()).unwrap();
        wait_for_idle_owner_unlock(data.path()).await;
        let next = scope!(client);
        let restarted: Value = serde_json::from_slice(&nw_platform::security::read_private(&runtime_path,16384).unwrap()).unwrap();
        assert_eq!(first["principal"], next["principal"]);
        assert_eq!(first["root"], next["root"]);
        assert_eq!(original["root_id"], restarted["root_id"]);
        assert_ne!(original["epoch"], restarted["epoch"]);
        assert_ne!(original["bearer"], restarted["bearer"]);
        wait_for_idle_owner_unlock(data.path()).await;
        let replacement_principal = uuid::Uuid::new_v4().to_string();
        {
            // Simulate a replacement profile while no owner/database writer is alive.
            let store = nw_store::Store::open(&data.path().join("state/generations/1")).unwrap();
            let mut profile: nw_domain::access::AccessProfile = store.get("profile/agent").unwrap().unwrap();
            assert_eq!(profile.principal, first["principal"].as_str().unwrap());
            profile.principal = replacement_principal.clone();
            profile.revision += 1;
            store.put("profile/agent", &profile).unwrap();
        }
        let denied = client.call_tool(CallToolRequestParams::new("access.replay_scope.create")
            .with_arguments(rmcp::object!({"params":{},"context":{"request_id":"original-bridge-principal-guard"}}))).await.unwrap();
        let denied = serde_json::to_value(denied).unwrap();
        assert_eq!(denied["structuredContent"]["ok"], false);
        assert_eq!(denied["structuredContent"]["request_id"], "original-bridge-principal-guard");
        assert_eq!(denied["structuredContent"]["error"]["code"], "PRINCIPAL_CHANGED");
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command.arg("--data-dir").arg(data.path()).arg("bridge");
        let fresh = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        let selected = scope!(fresh);
        assert_eq!(selected["principal"], replacement_principal);
        assert_ne!(selected["principal"], first["principal"]);
        fresh.cancel().await.unwrap();
        client.cancel().await.unwrap();
        wait_for_idle_owner_unlock(data.path()).await;
    }).await.expect("Actual stdio restart/principal boundary deadline");
}

#[tokio::test]
async fn concurrent_native_frontends_boot_one_owner_for_the_same_root() {
    let _case = OWNER_CASE.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(75), async {
        let data = tempfile::tempdir().unwrap();
        let _logs = retain_owner_logs(data.path(), "concurrent-start");
        let mut children = tokio::task::JoinSet::new();
        for _ in 0..3 {
            let root = data.path().to_owned();
            children.spawn(async move {
                tokio::time::timeout(
                    std::time::Duration::from_secs(45),
                    tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"))
                        .arg("--data-dir")
                        .arg(root)
                        .arg("commands")
                        .stdin(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .output(),
                )
                .await
                .unwrap()
                .unwrap()
            });
        }
        let mut spawned = 0;
        while let Some(output) = children.join_next().await {
            let output = output.unwrap();
            spawned += String::from_utf8_lossy(&output.stderr)
                .lines()
                .filter(|line| line.contains("Protected owner child spawned"))
                .count();
            assert!(
                output.status.success(),
                "Concurrent owner attachment failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(response["ok"], true);
        }
        assert_eq!(
            spawned, 1,
            "Concurrent frontends launched redundant owner children"
        );
        wait_for_idle_owner_unlock(data.path()).await;
        let lines = nw_platform::security::read_private(
            &data.path().join("logs/owner.jsonl"),
            2 * 1024 * 1024 + 1024,
        )
        .unwrap();
        let owners = lines
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .filter(|row| {
                row["fields"]["message"] == "Protected structured owner logging activated"
            })
            .filter_map(|row| row["process_id"].as_u64())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            owners.len(),
            1,
            "Concurrent startup initialized redundant owner processes"
        );
    })
    .await
    .expect("Concurrent native attachment deadline");
}
