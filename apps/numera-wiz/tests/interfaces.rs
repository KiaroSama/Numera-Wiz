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
