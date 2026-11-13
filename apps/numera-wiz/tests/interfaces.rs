//! CLI and actual official-SDK stdio parity and authorization regressions.
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use std::process::Command;
// These cases launch independent unoptimized owners. Isolate those workloads so
// hosted VM contention does not consume the unchanged 20-second bootstrap budget.
// The dedicated same-root case below still starts three frontends concurrently.
static OWNER_CASE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[path = "support/project_compatibility.rs"]
mod project_compatibility;

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
    // Capture the complete standard page from a fresh SDK session before list_all_tools flattens it.
    let first_page = client.list_tools(None).await.unwrap();
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
    project_mcp_output_contract(&first_page);
    project_mcp_wire_page(data.path(), &first_page).await;
    let owner_profiles = native_owner_mcp_query(data.path(), "access.profiles.list", serde_json::json!({})).await;
    assert_eq!(owner_profiles["ok"], true);
    assert!(owner_profiles["result"].as_array().unwrap().len() >= 2);
    let still_scoped = client
        .call_tool(
            CallToolRequestParams::new("access.profiles.list")
                .with_arguments(rmcp::object!({"params":{}})),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(still_scoped).unwrap()["structuredContent"]["error"]["code"],
        "PERMISSION_DENIED",
        "Explicit owner access on another bridge must not change the original scoped session"
    );
    let forbidden = data.path().join("must-not-create-credentials/token");
    let refused = client.call_tool(
        CallToolRequestParams::new("access.credentials.issue").with_arguments(rmcp::object!({
            "params":{"profile":"agent"},"credential_file":forbidden.to_str().unwrap(),
        })),
    ).await.unwrap();
    assert_eq!(serde_json::to_value(refused).unwrap()["structuredContent"]["error"]["code"], "PERMISSION_DENIED");
    assert!(!forbidden.parent().unwrap().exists());
    native_owner_credential_files(data.path()).await;
    reject_ambiguous_owner_bridge().await;
    client.cancel().await.unwrap();
}

async fn native_owner_credential_files(root: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(40), async {
        use serde_json::json;
        use sha2::{Digest, Sha256};
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command.arg("--data-dir").arg(root).args(["bridge", "--native-owner"]).kill_on_drop(true);
        let owner = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        macro_rules! call {
            ($client:expr, $id:expr, $arguments:expr) => {{
                let result = $client.call_tool(
                    CallToolRequestParams::new($id).with_arguments($arguments),
                ).await.unwrap();
                serde_json::to_value(result).unwrap()
            }};
        }
        let record: serde_json::Value = serde_json::from_slice(
            &nw_platform::security::read_private(&root.join("runtime/owner.json"), 16384).unwrap(),
        ).unwrap();
        let native = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(10)).build().unwrap();
        let private_url = format!("http://127.0.0.1:{}", record["port"].as_u64().unwrap());
        let port_probe = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let public_port = port_probe.local_addr().unwrap().port();
        drop(port_probe);
        native.post(format!("{private_url}/internal/public"))
            .bearer_auth(record["bearer"].as_str().unwrap()).json(&json!({"port":public_port}))
            .send().await.unwrap().error_for_status().unwrap();
        let mut delivered = Vec::new();
        for (ordinal, (id, params)) in [
            ("access.credentials.issue", json!({"profile":"agent"})),
            ("access.tokens.issue", json!({"for_access_profile":"agent","port":public_port})),
        ].into_iter().enumerate() {
            let missing = call!(owner, id, rmcp::object!({"params":params.clone()}));
            assert_eq!(missing["structuredContent"]["error"]["code"], "VALIDATION_ERROR");
            let file = root.join(format!("mcp-credentials/issued-{ordinal}"));
            let output = call!(owner, id, rmcp::object!({
                "params":params.clone(),"credential_file":file.to_str().unwrap(),
            }));
            assert_eq!(output["structuredContent"]["ok"], true);
            assert!(output["structuredContent"]["result"].get("bearer").is_none());
            let bytes = nw_platform::security::read_private(&file, 256).unwrap();
            let bearer = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("Credential file encoding failure"));
            assert_eq!(bearer.len(), 64);
            assert!(!output.to_string().contains(&bearer), "Credential value appeared in protocol output");
            let token_id = hex::encode(Sha256::digest(&bytes));
            assert_eq!(output["structuredContent"]["result"]["token_id"], token_id);
            let collision = call!(owner, id, rmcp::object!({
                "params":params,"credential_file":file.to_str().unwrap(),
            }));
            assert_eq!(collision["structuredContent"]["error"]["code"], "OUTPUT_COLLISION");
            assert!(nw_platform::security::read_private(&file, 256).unwrap() == bytes);
            delivered.push((token_id, bearer));
        }
        let queried: serde_json::Value = native.post(format!("{private_url}/api/v1/commands"))
            .bearer_auth(&delivered[0].1).json(&nw_domain::commands::Request::query("commands.list", json!({})))
            .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        assert_eq!(queried["ok"], true, "Delivered private credential did not authenticate");
        let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
            reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap(),
            rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
                format!("http://127.0.0.1:{public_port}/mcp"),
            ).auth_header(&delivered[1].1),
        );
        let http = ().serve(transport).await.unwrap();
        let forbidden = root.join("http-must-not-create/token");
        let refused = call!(http, "access.credentials.issue", rmcp::object!({
            "params":{"profile":"agent"},"credential_file":forbidden.to_str().unwrap(),
        }));
        assert_eq!(refused["structuredContent"]["error"]["code"], "PERMISSION_DENIED");
        assert!(!forbidden.parent().unwrap().exists());
        http.cancel().await.unwrap();
        let scope = call!(owner, "access.replay_scope.create", rmcp::object!({"params":{}}));
        let scope = scope["structuredContent"]["result"]["replay_scope"].as_str().unwrap();
        for (token_id, _) in delivered {
            let revoked = call!(owner, "access.tokens.revoke", rmcp::object!({
                "params":{"token_id":token_id},"context":{
                    "replay_scope":scope,"idempotency_key":uuid::Uuid::new_v4().to_string(),
                },
            }));
            assert_eq!(revoked["structuredContent"]["ok"], true, "Delivered credential was not actually revocable");
        }
        owner.cancel().await.unwrap();
    }).await.expect("Native owner credential file controls exceeded their40-second bound");
}

async fn native_owner_mcp_query(
    root: &std::path::Path,
    id: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    tokio::time::timeout(std::time::Duration::from_secs(40), async {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command.arg("--data-dir").arg(root).args(["bridge", "--native-owner"]).kill_on_drop(true);
        let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
        let result = client.call_tool(
            CallToolRequestParams::new(id.to_owned()).with_arguments(rmcp::object!({"params":params})),
        ).await.unwrap();
        let result = serde_json::to_value(result).unwrap()["structuredContent"].clone();
        assert!(result.is_object(), "Native owner MCP must return a canonical Response");
        client.cancel().await.unwrap();
        result
    }).await.expect("Native owner SDK query exceeded its40-second fixture bound")
}

async fn reject_ambiguous_owner_bridge() {
    for args in [
        vec!["bridge", "--native-owner", "--transport", "http"],
        vec!["--access-profile", "agent", "bridge", "--native-owner"],
        vec!["bridge", "--native-owner", "--access-profile", "agent"],
        vec!["bridge", "--native-owner", "--token-file", "must-not-use-a-token"],
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("must-not-bootstrap");
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command.arg("--data-dir").arg(&root).args(args).kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(5), command.output())
            .await.unwrap().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "Rejected bridge configuration polluted protocol stdout");
        assert!(!root.exists(), "Rejected bridge configuration must not initialize or attach an owner");
    }
}

fn project_mcp_output_contract(page: &rmcp::model::ListToolsResult) {
    use serde_json::Value;
    use std::collections::BTreeSet;
    assert!(page.next_cursor.is_none(), "MCP_UNEXPECTED_TOOL_PAGINATION");
    let encoded = serde_json::to_vec(page).unwrap();
    assert!(
        encoded.len() <= nw_domain::commands::MAX_RESULT_BYTES,
        "MCP_COMPLETE_TOOL_PAGE_BYTE_BOUND"
    );
    let actual: BTreeSet<_> = page.tools.iter().map(|tool| tool.name.as_ref()).collect();
    let expected: BTreeSet<_> = nw_service::ACTIVE_COMMANDS.iter().copied().collect();
    assert!(
        page.tools.len() == actual.len() && actual == expected,
        "MCP_COMPLETE_ACTIVE_ID_SET"
    );
    let family = [
        "projects.create",
        "projects.list",
        "projects.inspect",
        "sequences.create",
        "sequences.inspect",
        "clips.place",
        "clips.move",
        "clips.link",
        "clips.unlink",
        "clips.split",
        "clips.remove",
        "history.undo",
        "history.redo",
    ];
    for id in family {
        let tool = page.tools.iter().find(|tool| tool.name == id).unwrap();
        let schema = tool
            .output_schema
            .as_ref()
            .expect("MCP_CANONICAL_OUTPUT_SCHEMA_MISSING");
        let root = Value::Object(schema.as_ref().clone());
        assert_eq!(root["type"], "object");
        assert_eq!(root["additionalProperties"], false);
        let required: BTreeSet<_> = root["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect();
        assert_eq!(required, BTreeSet::from(["request_id", "ok"]));
        assert_eq!(root["properties"]["result"]["$ref"], "#/$defs/Result");
        assert_eq!(root["properties"]["error"]["$ref"], "#/$defs/Error");
        assert_eq!(root["oneOf"].as_array().unwrap().len(), 2);
        assert!(root["$defs"]["Result"].is_object() && root["$defs"]["Error"].is_object());
        project_mcp_local_references(&root).expect("MCP_OUTPUT_REFERENCE_CONTRACT");
        let serialized = serde_json::to_value(tool).unwrap();
        let requirement = &serialized["_meta"]["numera-wiz/authorization_requirements"];
        assert_eq!(requirement["schema_version"], 1);
        assert_eq!(requirement["command_id"], id);
        assert!(
            requirement.get("policy_revision").is_none()
                && serialized["_meta"].get("authorization").is_none()
        );
    }
    eprintln!(
        "MCP_SCHEMA_SIZE kind=standard_list_tools_result bytes={} project_outputs={}",
        encoded.len(),
        family.len()
    );
    let literal_data = serde_json::json!({
        "$defs":{"value":{"type":"string"}},
        "properties":{"$ref":{"type":"string"},"real":{"$ref":"#/$defs/value"}},
        "const":{"$ref":"literal text"},
        "enum":[{"$dynamicRef":"literal text"}],
        "default":{"$ref":"#/data-is-not-a-schema"},
        "examples":[{"$ref":"literal text"}],
        "anyOf":[true,{"not":false}]
    });
    assert!(project_mcp_local_references(&literal_data).is_ok());
    for invalid in [
        serde_json::json!({"properties":{"value":{"$ref":"https://example.invalid/schema"}}}),
        serde_json::json!({"items":{"$ref":"#/$defs/missing"}}),
        serde_json::json!({"allOf":[{"$dynamicRef":"#/$defs/missing"}]}),
    ] {
        assert!(project_mcp_local_references(&invalid).is_err());
    }
}

fn project_mcp_local_references(root: &serde_json::Value) -> Result<(), &'static str> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.is_boolean() {
            continue;
        }
        let object = node.as_object().ok_or("MCP_INVALID_OUTPUT_SCHEMA")?;
        for (keyword, value) in object {
            match keyword.as_str() {
                "$ref" | "$dynamicRef" => {
                    let pointer = value
                        .as_str()
                        .and_then(|text| text.strip_prefix('#'))
                        .ok_or("MCP_NONLOCAL_OUTPUT_REF")?;
                    if !pointer.is_empty() && root.pointer(pointer).is_none() {
                        return Err("MCP_DANGLING_OUTPUT_REF");
                    }
                }
                "$defs" | "definitions" | "properties" | "patternProperties"
                | "dependentSchemas" => {
                    pending.extend(
                        value
                            .as_object()
                            .ok_or("MCP_INVALID_OUTPUT_SCHEMA_MAP")?
                            .values(),
                    );
                }
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                    pending.extend(value.as_array().ok_or("MCP_INVALID_OUTPUT_SCHEMA_ARRAY")?);
                }
                "items"
                | "additionalProperties"
                | "unevaluatedProperties"
                | "propertyNames"
                | "contains"
                | "not"
                | "if"
                | "then"
                | "else"
                | "unevaluatedItems"
                | "contentSchema" => {
                    pending.push(value);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

async fn project_mcp_wire_page(root: &std::path::Path, expected: &rmcp::model::ListToolsResult) {
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"))
            .arg("--data-dir").arg(root).arg("bridge")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null()).kill_on_drop(true).spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = tokio::io::BufReader::new(child.stdout.take().unwrap());
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"r4-initialize\",\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2026-07-28\",\"capabilities\":{},\"clientInfo\":{\"name\":\"numera-wiz-wire-contract\",\"version\":\"1\"}}}\n").await.unwrap();
        input.flush().await.unwrap();
        let initialized = project_mcp_wire_line(&mut output).await;
        let initialized: serde_json::Value = serde_json::from_slice(&initialized).unwrap();
        assert_eq!(initialized["id"], "r4-initialize");
        assert!(initialized.get("error").is_none() && initialized["result"].is_object());
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":\"r4-tools\",\"method\":\"tools/list\"}\n").await.unwrap();
        input.flush().await.unwrap();
        let frame = project_mcp_wire_line(&mut output).await;
        let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], "r4-tools");
        assert!(value.get("error").is_none());
        assert!(value["result"] == serde_json::to_value(expected).unwrap(), "MCP_ACTUAL_WIRE_STATIC_PAGE_DIFFERS");
        eprintln!("MCP_SCHEMA_SIZE kind=actual_stdio_jsonrpc_line bytes={}", frame.len());
        let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/interface-evidence/mcp-list-tools-wire.json");
        std::fs::create_dir_all(evidence.parent().unwrap()).unwrap();
        std::fs::write(evidence, &frame).unwrap();
        drop(input);
        assert!(child.wait().await.unwrap().success(), "MCP_WIRE_BRIDGE_DID_NOT_EXIT");
    }).await.expect("MCP_ACTUAL_WIRE_DEADLINE");
}

async fn project_mcp_wire_line<T: tokio::io::AsyncBufRead + Unpin>(reader: &mut T) -> Vec<u8> {
    use tokio::io::AsyncBufReadExt;
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await.unwrap();
        assert!(!available.is_empty(), "MCP_WIRE_UNEXPECTED_EOF");
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        assert!(
            frame.len() + take <= nw_domain::commands::MAX_RESULT_BYTES,
            "MCP_ACTUAL_JSONRPC_BYTE_BOUND"
        );
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return frame;
        }
    }
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
fn initialize_legacy_owner_fixture(root: &std::path::Path) {
    let lease = nw_store::generations::RootLease::acquire(root).unwrap();
    drop(nw_store::Store::open_selected(&lease).unwrap());
}
#[tokio::test]
async fn official_stdio_reconnects_after_idle_without_changing_its_principal() {
    let _case = OWNER_CASE.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use serde_json::Value;
        let data = tempfile::tempdir().unwrap();
        initialize_legacy_owner_fixture(data.path());
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
        let log_lease = nw_platform::files::RootLock::acquire(&data.path().join("logs"))
            .expect("Drained owner must release its log lease before its generation fence");
        drop(log_lease);
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

async fn migration_cli(root: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let started = std::time::Instant::now();
    let label = args
        .iter()
        .copied()
        .take_while(|arg| {
            matches!(
                *arg,
                "maintenance"
                    | "promotion"
                    | "initialization"
                    | "restore"
                    | "plan"
                    | "accept"
                    | "inspect"
                    | "migrate"
                    | "access"
                    | "replay-scope"
                    | "create"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("NW_MIGRATION_CLI_STEP_START:{label}");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
    command
        .arg("--data-dir")
        .arg(root)
        .args(args)
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(60), command.output())
        .await
        .unwrap_or_else(|_| panic!("Native CLI step {label} exceeded its 60-second deadline"))
        .unwrap();
    assert!(
        output.stdout.len() <= nw_domain::commands::MAX_RESULT_BYTES,
        "CLI result exceeded bounded JSON"
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "CLI JSON failed:{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(
        output.status.success() && response["ok"] == true,
        "CLI migration failed:{response};{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!(
        "NW_MIGRATION_CLI_STEP_FINISH:{label}:{}ms",
        started.elapsed().as_millis()
    );
    response
}
async fn migration_owner_http(
    root: &std::path::Path,
    request: &nw_domain::commands::Request,
) -> serde_json::Value {
    let runtime = root.join("runtime/owner.json");
    let record: serde_json::Value =
        serde_json::from_slice(&nw_platform::security::read_private(&runtime, 16384).unwrap())
            .unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap();
    let started = std::time::Instant::now();
    eprintln!("NW_MIGRATION_HTTP_STEP_START:{}", request.command.id);
    let mut response = client
        .post(format!(
            "http://127.0.0.1:{}/api/v1/commands",
            record["port"].as_u64().unwrap()
        ))
        .bearer_auth(record["bearer"].as_str().unwrap())
        .json(request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(bytes.len().saturating_add(chunk.len()) <= nw_domain::commands::MAX_RESULT_BYTES);
        bytes.extend_from_slice(&chunk);
    }
    let response: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        response["ok"], true,
        "Actual protected owner request failed:{response}"
    );
    eprintln!(
        "NW_MIGRATION_HTTP_STEP_FINISH:{}:{}ms",
        request.command.id,
        started.elapsed().as_millis()
    );
    response
}
#[tokio::test]
async fn actual_typed_migration_cli_replays_before_and_after_real_owner_restart() {
    let _case = OWNER_CASE.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        use nw_domain::commands::Request;
        use serde_json::json;
        let data = tempfile::tempdir().unwrap();
        // This journey migrates a genuine legacy factory store. True absent
        // application roots now use the separately exercised MediaV1 genesis path.
        initialize_legacy_owner_fixture(data.path());
        let _logs = retain_owner_logs(data.path(), "migration-cli");
        let input =
            nw_platform::security::private_directory(&data.path().join("requests")).unwrap();
        let issued = migration_cli(data.path(), &["access", "replay-scope", "create"]).await;
        let scope = issued["result"]["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut original = Request::query(
            "access.profiles.create",
            json!({"name":"before-migration","read_roots":[],"write_roots":[]}),
        );
        original.request_id = "original-profile".into();
        original.replay_scope = Some(scope.clone());
        original.idempotency_key = Some("original-profile-key".into());
        let before = migration_owner_http(data.path(), &original).await;
        let reviewed = migration_cli(data.path(), &["maintenance", "plan"]).await;
        let plan = input.join("review-λ.json");
        nw_platform::security::write_private(
            &plan,
            &serde_json::to_vec(&reviewed["result"]).unwrap(),
            false,
        )
        .unwrap();
        let submit = [
            "maintenance",
            "migrate",
            "--plan-file",
            plan.to_str().unwrap(),
            "--replay-scope",
            &scope,
            "--idempotency-key",
            "one-generation",
            "--request-id",
            "actual-migration",
        ];
        let accepted = migration_cli(data.path(), &submit).await;
        assert!(accepted["receipt_id"].is_string());
        let id = accepted["result"]["migration_id"].as_str().unwrap();
        let state = migration_cli(data.path(), &["maintenance", "inspect", id]).await;
        assert_eq!(state["result"]["phase"], "succeeded");
        let replay = migration_owner_http(data.path(), &original).await;
        assert_eq!(replay["receipt_id"], before["receipt_id"]);
        assert_eq!(replay["result"], before["result"]);
        let runtime = data.path().join("runtime/owner.json");
        let prior: serde_json::Value =
            serde_json::from_slice(&nw_platform::security::read_private(&runtime, 16384).unwrap())
                .unwrap();
        eprintln!("NW_MIGRATION_STEP:waiting_for_idle_owner");
        wait_for_idle_owner_unlock(data.path()).await;
        let retried = migration_cli(data.path(), &submit).await;
        assert_eq!(retried["receipt_id"], accepted["receipt_id"]);
        assert_eq!(retried["result"], accepted["result"]);
        let current: serde_json::Value =
            serde_json::from_slice(&nw_platform::security::read_private(&runtime, 16384).unwrap())
                .unwrap();
        assert_ne!(
            prior["epoch"], current["epoch"],
            "The test must reconnect to a new real owner"
        );
        assert_eq!(prior["root_id"], current["root_id"]);
        let replay = migration_owner_http(data.path(), &original).await;
        assert_eq!(replay["receipt_id"], before["receipt_id"]);
        assert_eq!(replay["result"], before["result"]);
        let profiles = migration_owner_http(
            data.path(),
            &Request::query("access.profiles.list", json!({})),
        )
        .await;
        assert_eq!(
            profiles["result"].as_array().unwrap().len(),
            3,
            "Replay must not create a second profile"
        );
        wait_for_idle_owner_unlock(data.path()).await;
        let lease = nw_store::generations::RootLease::acquire(data.path()).unwrap();
        let selected = lease.selection().unwrap();
        assert_eq!(selected.generation, 3);
        let store = nw_store::Store::open(&lease.path(&selected).unwrap()).unwrap();
        assert_eq!(
            store
                .adoption_revision(id, selected.activation_id.as_deref().unwrap())
                .unwrap(),
            "0"
        );
        assert_eq!(
            store
                .list::<nw_domain::maintenance::MigrationRecord>("generation-migration/", 16)
                .unwrap()
                .len(),
            1
        );
        drop(store);
        drop(lease);
        // Bootstrap a real native owner, adopt once, then use the actual typed review CLI.
        let issued = migration_cli(data.path(), &["access", "replay-scope", "create"]).await;
        let mut business = Request::query(
            "access.profiles.create",
            json!({"name":"native-promotion-review","read_roots":[],"write_roots":[]}),
        );
        business.replay_scope = Some(issued["result"]["replay_scope"].as_str().unwrap().into());
        business.idempotency_key = Some("native-promotion-review-adoption".into());
        let accepted = migration_owner_http(data.path(), &business).await;
        assert!(accepted["receipt_id"].is_string());
        let reviewed = migration_cli(data.path(), &["maintenance", "promotion", "plan"]).await;
        assert!(reviewed["receipt_id"].is_null());
        let plan: nw_domain::maintenance::promotion::GenerationPromotionPlan =
            serde_json::from_value(reviewed["result"].clone()).unwrap();
        plan.validate_hash().unwrap();
        assert_eq!(plan.source_generation, 3);
        assert_eq!(plan.target_generation, 5);
        assert_eq!(plan.source_guard.migration_id, id);
        assert_eq!(plan.source_guard.revision, "1");
        let mcp = native_owner_mcp_query(data.path(), "maintenance.promotion.plan", json!({})).await;
        assert_eq!(mcp["ok"], true);
        assert!(mcp["receipt_id"].is_null());
        let mcp_plan: nw_domain::maintenance::promotion::GenerationPromotionPlan =
            serde_json::from_value(mcp["result"].clone()).unwrap();
        mcp_plan.validate_hash().unwrap();
        assert_eq!(mcp_plan.source_guard, plan.source_guard);
        assert_eq!(mcp_plan.source_records_sha256, plan.source_records_sha256);
        assert_eq!(mcp_plan.hash, plan.hash, "Native CLI and explicit owner MCP must review the same actual source");
        let media_started = std::time::Instant::now();
        let promotion_file = input.join("promotion-review.json");
        nw_platform::security::write_private(&promotion_file, &serde_json::to_vec(&reviewed["result"]).unwrap(), false).unwrap();
        let native_scope = issued["result"]["replay_scope"].as_str().unwrap();
        let promote = [
            "maintenance", "promotion", "accept", "--plan-file", promotion_file.to_str().unwrap(),
            "--replay-scope", native_scope, "--idempotency-key", "native-media-promotion",
            "--request-id", "native-media-promotion-request",
        ];
        let promoted = migration_cli(data.path(), &promote).await;
        let promotion_id = promoted["result"]["promotion_id"].as_str().unwrap();
        assert_eq!(promotion_id, plan.id);
        let promotion_state = migration_cli(data.path(), &["maintenance", "promotion", "inspect", promotion_id]).await;
        assert_eq!(promotion_state["result"]["phase"], "succeeded");
        let retry = migration_cli(data.path(), &promote).await;
        assert_eq!(retry["receipt_id"], promoted["receipt_id"]);
        assert_eq!(retry["result"], promoted["result"]);
        let reviewed_restore = migration_cli(data.path(), &["maintenance", "promotion", "restore", "plan", promotion_id]).await;
        let restore_plan: nw_domain::maintenance::media_lifecycle::GenerationRestorationPlan =
            serde_json::from_value(reviewed_restore["result"].clone()).unwrap();
        restore_plan.validate_hash().unwrap();
        assert_eq!(restore_plan.source_generation, plan.source_generation);
        assert_eq!(restore_plan.source_activation_id, plan.source_activation_id);
        assert_eq!(restore_plan.source_operation_id, id);
        assert_eq!(restore_plan.source_guard_revision, "1");
        let restore_file = input.join("restoration-review.json");
        nw_platform::security::write_private(&restore_file, &serde_json::to_vec(&reviewed_restore["result"]).unwrap(), false).unwrap();
        let restore = [
            "maintenance", "promotion", "restore", "accept", "--plan-file", restore_file.to_str().unwrap(),
            "--replay-scope", native_scope, "--idempotency-key", "native-media-restoration",
            "--request-id", "native-media-restoration-request",
        ];
        let restored = migration_cli(data.path(), &restore).await;
        let restoration_id = restored["result"]["restoration_id"].as_str().unwrap();
        assert_eq!(restoration_id, restore_plan.id);
        let restoration_state = migration_cli(data.path(), &["maintenance", "promotion", "restore", "inspect", restoration_id]).await;
        assert_eq!(restoration_state["result"]["id"], restoration_id);
        let promotion_state = migration_cli(data.path(), &["maintenance", "promotion", "inspect", promotion_id]).await;
        assert_eq!(promotion_state["result"]["phase"], "restored");
        let retry = migration_cli(data.path(), &restore).await;
        assert_eq!(retry["receipt_id"], restored["receipt_id"]);
        assert_eq!(retry["result"], restored["result"]);
        let old_accepted = migration_cli(data.path(), &promote).await;
        assert_eq!(old_accepted["receipt_id"], promoted["receipt_id"]);
        assert_eq!(old_accepted["result"], promoted["result"]);
        let original_replay = migration_owner_http(data.path(), &original).await;
        assert_eq!(original_replay["receipt_id"], before["receipt_id"]);
        assert_eq!(original_replay["result"], before["result"]);
        let mut after_restore = Request::query("access.profiles.create", json!({
            "name":"after-media-restoration","read_roots":[],"write_roots":[],
        }));
        after_restore.replay_scope = Some(native_scope.into());
        after_restore.idempotency_key = Some("native-old-business-after-restoration".into());
        let written = migration_owner_http(data.path(), &after_restore).await;
        assert!(written["receipt_id"].is_string());
        let next = migration_cli(data.path(), &["maintenance", "promotion", "plan"]).await;
        let next_plan: nw_domain::maintenance::promotion::GenerationPromotionPlan =
            serde_json::from_value(next["result"].clone()).unwrap();
        assert_eq!(next_plan.source_generation, plan.source_generation);
        assert_eq!(next_plan.source_activation_id, plan.source_activation_id);
        assert_eq!(next_plan.source_guard.migration_id, id);
        assert_eq!(next_plan.source_guard.revision, "2");
        if let Some(directory) = std::env::var_os("NW_SCHEMA_VALIDATOR_EVIDENCE_DIR") {
            let evidence = json!({
                "schema_version":1,"status":"passed",
                "scope":"Actual current native CLI promotion/restoration/replay and one post-restoration business adoption; not pinned-old executable or fault/power-loss qualification",
                "source_commit":std::env::var("NW_SCHEMA_VALIDATOR_SOURCE").ok(),
                "target":std::env::var("NW_SCHEMA_VALIDATOR_TARGET").ok(),
                "added_elapsed_ms":media_started.elapsed().as_millis(),
                "source_generation":plan.source_generation,"source_activation_id":plan.source_activation_id,
                "original_operation_id":id,"source_guard_before":"1","source_guard_after_old_business":"2",
                "promotion_id":promotion_id,"restoration_id":restoration_id,
                "promotion_receipt":promoted["receipt_id"],"restoration_receipt":restored["receipt_id"],
                "promotion_replayed_without_reactivation":true,"restoration_replayed_without_reoverlay":true,
                "pre_migration_receipt_unchanged":true,
            });
            let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
            assert!(bytes.len() <= 4096);
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("native-media-lifecycle.json"), bytes).unwrap();
        }
    })
    .await
    .expect("Actual CLI migration/restart deadline");
    project_compatibility::run().await;
}
