//! CLI and actual official-SDK stdio parity and authorization regressions.
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use std::process::Command;
#[test]
fn cli_stdout_is_canonical_json_and_index_zero_survives() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clean-resolve.json");
    let data = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_numera-wiz"))
        .arg("--data-dir")
        .arg(data.path())
        .args(["request", "--file"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
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
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
    let data = tempfile::tempdir().unwrap();
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
    assert_eq!(value["structuredContent"]["ok"], true);
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
            Ok(lock) => { drop(lock); return; }
            Err(error) => assert_eq!(error.code, "OWNER_BUSY"),
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(35), "Idle owner did not drain while its stdio bridge remained alive");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
#[tokio::test]
async fn official_stdio_reconnects_after_idle_without_changing_its_principal() {
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use serde_json::Value;
        let data = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command.arg("--data-dir").arg(data.path()).arg("bridge");
        let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        macro_rules! scope {
            ($client:expr) => {{
                let response = $client.call_tool(CallToolRequestParams::new("access.replay_scope.create")
                    .with_arguments(rmcp::object!({"params":{}}))).await.unwrap();
                let response = serde_json::to_value(response).unwrap();
                assert_eq!(response["structuredContent"]["ok"], true);
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
