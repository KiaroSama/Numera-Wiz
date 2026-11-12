//! Whole canonical responses against the actual published MCP output schemas.
use super::*;

pub(super) struct Observation {
    id: u16,
    schema: usize,
    wire: String,
    valid: bool,
}

pub(super) fn capture(batch: &schema_validation::Batch) -> Vec<Observation> {
    batch.observations().filter(|case| case["select"] == "result").map(|case| Observation {
        id: case["id"].as_u64().unwrap() as u16,
        schema: case["schema_id"].as_u64().unwrap() as usize,
        wire: case["instance_json"].as_str().unwrap().to_owned(),
        valid: case["valid"].as_bool().unwrap(),
    }).collect()
}

async fn documents(client: &RunningService<RoleClient, ()>) -> Vec<Value> {
    // A new dedicated SDK peer supplies this first, uncached complete page.
    let page = client.list_tools(None).await.expect("MCP_OUTPUT_DISCOVERY_FAILED");
    bounded(&page);
    assert!(page.next_cursor.is_none(), "MCP_OUTPUT_UNEXPECTED_PAGE");
    let ids: BTreeSet<_> = page.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(ids.len() == page.tools.len() && ids == nw_service::ACTIVE_COMMANDS.iter().copied().collect(), "MCP_OUTPUT_ACTIVE_IDS");
    schema_validation::COMMANDS.into_iter().map(|id| {
        let tool = page.tools.iter().find(|tool| tool.name == id).expect("MCP_OUTPUT_TOOL_MISSING");
        Value::Object(tool.output_schema.as_ref().expect("MCP_OUTPUT_SCHEMA_MISSING").as_ref().clone())
    }).collect()
}

fn expected_ids(bases: &[u16], errors: bool) -> Vec<u16> {
    let mut ids = Vec::new();
    for base in bases {
        ids.extend((1..=9).chain(20..=30).chain([40,41,42,50]).map(|id| base + id));
        if errors { ids.extend((3001..=3013).map(|id| base + id)); }
    }
    ids.extend(4001..=4007);
    if errors { ids.extend(4101..=4106); }
    ids
}

fn batch(suite: u16, schemas: Vec<Value>, responses: Vec<Observation>, bases: &[u16], errors: bool) -> schema_validation::Batch {
    assert_eq!(responses.len(), bases.len() * 24, "MCP_WHOLE_CAPTURE_CASE_COUNT");
    let source = responses.iter().find(|case| case.id == 1).expect("MCP_ACTUAL_CREATE_RESPONSE_MISSING");
    assert!(source.schema == 0 && source.valid, "MCP_COPIED_CONTROL_SOURCE");
    let original: Value = serde_json::from_str(&source.wire).unwrap();
    assert_eq!(original.get("ok"), Some(&json!(true)));
    let mut batch = schema_validation::Batch::new(suite, &expected_ids(bases, errors), schemas);
    for response in responses { batch.whole(response.id, response.schema, response.wire, response.valid); }
    for (id, mutation) in (4001..=4007).zip(0..7) {
        let mut copy = original.clone();
        match mutation {
            0 => { copy.as_object_mut().unwrap().remove("request_id"); },
            1 => copy["ok"] = json!(false),
            2 => copy["result"] = Value::Null,
            3 => copy["error"] = json!({"code":"CONTROL","message":"copied control","field":null,"retryable":false}),
            4 => copy["extra"] = json!(true),
            5 => copy["receipt_id"] = Value::Null,
            6 => { copy.as_object_mut().unwrap().remove("result"); },
            _ => unreachable!(),
        }
        batch.whole(id, 0, serde_json::to_string(&copy).unwrap(), false);
    }
    batch
}

fn rejected_params(id: &str) -> Value {
    const NIL: &str = "00000000-0000-0000-0000-000000000000";
    match id {
        "projects.create" => json!({"name":"No-scope rejection control"}),
        "projects.list" => json!({"limit":1,"cursor":"invalid-authenticated-cursor"}),
        "projects.inspect" => json!({"projection":"summary"}),
        "sequences.create" => json!({"name":"No-scope rejection control","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"}]}),
        "sequences.inspect" => json!({"sequence_id":NIL,"projection":"entity"}),
        "clips.place" => json!({"sequence_id":NIL,"track_id":NIL,"source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"4","den":"1"}},"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},"sequence_start":{"num":"0","den":"1"},"collision":"reject"}),
        "clips.move" => json!({"clip_ids":[NIL],"delta":{"num":"0","den":"1"},"collision":"reject","link_policy":"expand"}),
        "clips.link" | "clips.unlink" => json!({"clip_ids":[NIL],"link_policy":"expand"}),
        "clips.split" => json!({"clip_id":NIL,"at_sequence":{"num":"1","den":"1"},"link_policy":"expand"}),
        "clips.remove" => json!({"clip_ids":[NIL],"mode":"lift","link_policy":"expand"}),
        "history.undo" | "history.redo" => json!({}),
        _ => unreachable!("Unknown project output-schema command"),
    }
}

async fn failures(client: &RunningService<RoleClient, ()>, tag: &str, base: u16, batch: &mut schema_validation::Batch) -> Value {
    let mut first = None;
    for (index, id) in schema_validation::COMMANDS.into_iter().enumerate() {
        let request_id = format!("mcp-schema-rejection-{tag}-{id}");
        let mut context = json!({"request_id":request_id});
        if !matches!(id, "projects.create" | "projects.list") {
            let mut project = json!({"id":"00000000-0000-0000-0000-000000000000"});
            if !matches!(id,"projects.inspect"|"sequences.inspect") { project["expected_revision"] = json!("0"); }
            context["project"] = project;
        }
        // Valid input shapes, but no durable identity and no existing nil-UUID project.
        let arguments = json!({"params":rejected_params(id),"context":context});
        bounded(&arguments);
        let reply = client.call_tool(CallToolRequestParams::new(id).with_arguments(arguments.as_object().unwrap().clone()))
            .await.expect("MCP_EXPECTED_CANONICAL_REJECTION");
        let reply = serde_json::to_value(reply).unwrap();
        assert_eq!(reply.get("isError"), Some(&json!(true)), "MCP_REJECTION_TOOL_FLAG");
        let response = reply.get("structuredContent").expect("MCP_REJECTION_CANONICAL_RESPONSE");
        bounded(response);
        assert!(response.get("ok") == Some(&json!(false)) && response.get("result").is_none() && response.get("receipt_id").is_none(), "MCP_REJECTION_WAS_ACCEPTED");
        assert_eq!(response.get("request_id"), Some(&json!(request_id)));
        assert!(response.get("error").is_some_and(Value::is_object));
        batch.whole(3001 + base + index as u16, index, serde_json::to_string(response).unwrap(), true);
        first.get_or_insert_with(|| response.clone());
    }
    first.expect("MCP_REJECTION_CASE_SET")
}

fn reject_error_copies(batch: &mut schema_validation::Batch, original: Value) {
    for (id, mutation) in (4101..=4106).zip(0..6) {
        let mut copy = original.clone();
        match mutation {
            0 => { copy["error"].as_object_mut().unwrap().remove("field"); },
            1 => copy["error"]["field"] = json!(7),
            2 => copy["ok"] = json!(true),
            3 => { copy.as_object_mut().unwrap().remove("error"); },
            4 => copy["result"] = json!({}),
            5 => copy["receipt_id"] = Value::Null,
            _ => unreachable!(),
        }
        batch.whole(id, 0, serde_json::to_string(&copy).unwrap(), false);
    }
}

pub(super) async fn run(cli: &Cli, port: u16, token: &str, cli_responses: Vec<Observation>, stdio_responses: Vec<Observation>, http_responses: Vec<Observation>) {
    // All original journeys, child batches, cancels and 44 receipt oracles precede this phase.
    let mut command = cli.command();
    command.args(["bridge","--access-profile","topology"]);
    let stdio = ().serve(TokioChildProcess::new(command).unwrap()).await.expect("MCP_OUTPUT_STDIO_START");
    let schemas = documents(&stdio).await;
    batch(14, schemas.clone(), cli_responses, &[0], false).finish().await;
    let mut stdio_batch = batch(15, schemas, stdio_responses, &[0], true);
    let first = failures(&stdio, "stdio", 0, &mut stdio_batch).await;
    reject_error_copies(&mut stdio_batch, first);
    stdio_batch.finish().await;
    stdio.cancel().await.unwrap();
    let mut batch = None;
    let mut expected_schemas = None;
    let mut http_responses = Some(http_responses);
    for (tag, base, lifecycle) in [
        ("http-initialize",0,ClientLifecycleMode::Initialize),
        ("http-discover",100,ClientLifecycleMode::Discover {preferred_versions:vec![ProtocolVersion::V_2026_07_28]}),
    ] {
        let transport = StreamableHttpClientTransport::with_client(
            reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap(),
            StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp")).auth_header(token),
        );
        let client = ().serve_with_lifecycle(transport,lifecycle).await.expect("MCP_OUTPUT_HTTP_START");
        let schemas = documents(&client).await;
        if let Some(expected) = &expected_schemas {
            assert!(&schemas == expected, "MCP_HTTP_OUTPUT_DOCUMENTS_DIFFER");
        } else {
            expected_schemas = Some(schemas.clone());
            batch = Some(self::batch(16,schemas,http_responses.take().unwrap(),&[0,100],true));
        }
        let current = batch.as_mut().unwrap();
        let first = failures(&client,tag,base,current).await;
        if base == 0 { reject_error_copies(current, first); }
        client.cancel().await.unwrap();
    }
    batch.unwrap().finish().await;
}
