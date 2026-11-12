//! Finite public topology qualification inside the existing hosted transport test.
//! Only supported solid clips are used: no source registration, jobs or raw store reads.
use super::owner::Cli;
#[path = "project_topology/output_contracts.rs"]
mod output_contracts;
#[path = "../../../../test-support/schema_validation.rs"]
mod schema_validation;
use nw_domain::commands::{MAX_RESULT_BYTES, ProjectRef, Request};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, RoleClient, ServiceExt,
    model::{CallToolRequestParams, ProtocolVersion},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

// A saved invocation owns every byte needed for its one retained-receipt retry.
struct Invocation {
    command: &'static str,
    params: Value,
    context: Value,
    cli_args: Vec<String>,
}
impl Invocation {
    fn new(command: &'static str, params: Value, context: Value, args: Vec<String>) -> Self {
        // Count the serialized SDK wrapper and equivalent canonical envelope too.
        bounded(&json!({"params":params,"context":context}));
        bounded(&args);
        let mut request = Request::query(command, params.clone());
        if let Some(project) = context.get("project") {
            request.project = Some(serde_json::from_value::<ProjectRef>(project.clone()).unwrap());
        }
        for (name, target) in [
            ("replay_scope", &mut request.replay_scope),
            ("idempotency_key", &mut request.idempotency_key),
        ] {
            *target = context
                .get(name)
                .map(|value| value.as_str().unwrap().to_owned());
        }
        if let Some(id) = context.get("request_id") {
            request.request_id = id.as_str().unwrap().to_owned();
        }
        bounded(&request);
        Self {
            command,
            params,
            context,
            cli_args: args,
        }
    }
}
fn bounded(value: &impl serde::Serialize) {
    assert!(
        serde_json::to_vec(value).unwrap().len() <= MAX_RESULT_BYTES,
        "Topology request/captured response exceeds MAX_RESULT_BYTES"
    );
}
fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| panic!("Topology response lacks string field {key}"))
        .to_owned()
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
fn flag(values: &mut Vec<String>, key: &str, value: &str) {
    values.extend([key.to_owned(), value.to_owned()]);
}
fn rational(num: u64) -> Value {
    json!({"num":num.to_string(),"den":"1"})
}
fn sorted(values: &[String]) -> Value {
    let mut values = values.to_vec();
    values.sort();
    assert_eq!(values.iter().collect::<BTreeSet<_>>().len(), values.len());
    json!(values)
}

enum Adapter<'a> {
    Cli(&'a Cli),
    Sdk(&'a RunningService<RoleClient, ()>),
}
impl Adapter<'_> {
    async fn invoke(&self, saved: &Invocation) -> Value {
        let response = match self {
            Self::Cli(cli) => {
                let refs = saved
                    .cli_args
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                // Cli::human parses the whole stdout as exactly one JSON value.
                let (response, exit) = cli.human(&refs).await;
                assert_eq!(exit, 0, "Topology typed CLI exit for {}", saved.command);
                response
            }
            Self::Sdk(client) => {
                let wrapper = json!({"params":saved.params,"context":saved.context});
                let result = client
                    .call_tool(
                        CallToolRequestParams::new(saved.command)
                            .with_arguments(wrapper.as_object().unwrap().clone()),
                    )
                    .await
                    .unwrap_or_else(|_| panic!("Topology SDK call failed for {}", saved.command));
                let result = serde_json::to_value(result).unwrap();
                assert!(
                    result
                        .get("isError")
                        .is_none_or(|value| value == &json!(false)),
                    "Topology SDK reported tool error for {}",
                    saved.command
                );
                result
                    .get("structuredContent")
                    .expect("SDK canonical structuredContent")
                    .clone()
            }
        };
        // These are after-capture bounds, not a streaming-memory qualification.
        bounded(&response);
        let error = response
            .get("error")
            .and_then(|v| v.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("absent");
        assert_eq!(
            response.get("ok"),
            Some(&json!(true)),
            "Topology command {} rejected (code {error}); context withheld",
            saved.command
        );
        assert!(
            response.get("error").is_none_or(Value::is_null),
            "Topology success must have no canonical error"
        );
        assert!(
            response.get("result").is_some(),
            "Topology canonical result missing"
        );
        assert!(!text(&response, "request_id").is_empty());
        if let Some(id) = saved.context.get("request_id") {
            assert_eq!(&response["request_id"], id);
        }
        response
    }
    async fn schema_documents(&self) -> Vec<Value> {
        let listing = self
            .invoke(&Invocation::new(
                "commands.list",
                json!({}),
                json!({}),
                args(&["--access-profile", "topology", "commands", "list"]),
            ))
            .await;
        let rows = listing["result"]["commands"]
            .as_array()
            .expect("SCHEMA_TRANSPORT_DISCOVERY_ROWS");
        let mut schemas = Vec::new();
        for id in schema_validation::COMMANDS {
            let response = self
                .invoke(&Invocation::new(
                    "commands.schema",
                    json!({"id":id}),
                    json!({}),
                    args(&["--access-profile", "topology", "commands", "schema", id]),
                ))
                .await;
            let row = rows
                .iter()
                .find(|row| row["id"] == id)
                .expect("SCHEMA_TRANSPORT_DISCOVERY_ID");
            assert!(
                &response["result"] == row,
                "SCHEMA_TRANSPORT_DISCOVERY_EQUAL"
            );
            schemas.push(
                response["result"]
                    .get("result_schema")
                    .expect("SCHEMA_TRANSPORT_DOCUMENT")
                    .clone(),
            );
        }
        schemas
    }
    async fn scope(&self, tag: &str) -> String {
        let cli_args = args(&[
            "--access-profile",
            "topology",
            "access",
            "replay-scope",
            "create",
            "--json",
        ]);
        // The scope bootstrap is a public query under this selected profile.
        let saved = Invocation::new(
            "access.replay_scope.create",
            json!({}),
            json!({"request_id":format!("topology-{tag}-scope")}),
            cli_args,
        );
        // This typed bootstrap has no request-id option; SDK context still supplies it.
        let saved = if matches!(self, Self::Cli(_)) {
            Invocation::new(saved.command, saved.params, json!({}), saved.cli_args)
        } else {
            saved
        };
        let response = self.invoke(&saved).await;
        text(&response["result"], "replay_scope")
    }
}

struct Journey<'a, 'b> {
    conformance: &'b mut schema_validation::Batch,
    case_base: u16,
    initial_projections: u16,
    adapter: Adapter<'a>,
    tag: &'static str,
    scope: String,
    accepted: Vec<(Invocation, Value)>,
    receipts: BTreeSet<String>,
    request_ids: BTreeSet<String>,
    snapshot: Option<String>,
}
impl Journey<'_, '_> {
    async fn write(
        &mut self,
        step: &str,
        command: &'static str,
        params: Value,
        mut cli_args: Vec<String>,
        project: Option<(&str, u64)>,
    ) -> Value {
        let identity = format!("topology-{}-{step}", self.tag);
        assert!(
            self.request_ids.insert(identity.clone()),
            "Fresh topology request identity reused"
        );
        let mut context =
            json!({"request_id":identity,"replay_scope":self.scope,"idempotency_key":identity});
        let mut selected = args(&["--access-profile", "topology"]);
        selected.append(&mut cli_args);
        if let Some((id, base)) = project {
            context["project"] = json!({"id":id,"expected_revision":base.to_string()});
            flag(&mut selected, "--project", id);
            flag(&mut selected, "--expected-revision", &base.to_string());
        }
        flag(&mut selected, "--replay-scope", &self.scope);
        flag(&mut selected, "--idempotency-key", &identity);
        flag(&mut selected, "--request-id", &identity);
        let saved = Invocation::new(command, params, context, selected);
        let response = self.conformance.capture(
            self.case_base + self.accepted.len() as u16 + 1,
            command,
            self.adapter.invoke(&saved).await,
        );
        assert!(
            self.receipts.insert(text(&response, "receipt_id")),
            "Fresh topology receipt reused"
        );
        let result = response["result"].clone();
        if let Some((id, base)) = project {
            assert_eq!(result["project_id"], id);
            assert_eq!(result["base_revision"], base.to_string());
            assert_eq!(result["revision"], (base + 1).to_string());
            assert!(
                result.get("no_change").is_none(),
                "Durable topology edit must advance"
            );
            assert!(!text(&result, "operation_id").is_empty());
        } else {
            assert_eq!(result["revision"], "0");
            assert_eq!(
                result["history"],
                json!({"can_undo":false,"can_redo":false})
            );
        }
        self.accepted.push((saved, response));
        result
    }
    async fn projection(
        &mut self,
        project: &str,
        revision: u64,
        command: &'static str,
        mut params: Value,
        mut cli_args: Vec<String>,
    ) -> Value {
        // Match the real typed query lowering, including its optional defaults.
        let fields = params.as_object_mut().unwrap();
        fields.entry("limit").or_insert(json!(128));
        fields.entry("cursor").or_insert(Value::Null);
        if command == "projects.inspect" {
            fields.entry("entity_id").or_insert(Value::Null);
            fields.entry("bin_id").or_insert(Value::Null);
        } else {
            fields.entry("track_ids").or_insert(json!([]));
            fields.entry("range").or_insert(Value::Null);
        }
        params["revision"] = json!(revision.to_string());
        flag(&mut cli_args, "--project", project);
        flag(&mut cli_args, "--revision", &revision.to_string());
        if let Some(snapshot) = self.snapshot.take() {
            params["replace_snapshot"] = json!(snapshot);
            flag(&mut cli_args, "--replace-snapshot", &snapshot);
        }
        let mut selected = args(&["--access-profile", "topology"]);
        selected.append(&mut cli_args);
        // A query's expected_revision is absent (never JSON null).
        let context = json!({"project":{"id":project}});
        assert!(context["project"].get("expected_revision").is_none());
        let response = self
            .adapter
            .invoke(&Invocation::new(command, params, context, selected))
            .await;
        // Select the complete first checkpoint explicitly; later checkpoints retain their
        // original equality oracles without creating unbounded repeated observations.
        if revision == 3 {
            self.conformance.observe(
                self.case_base + 20 + self.initial_projections,
                command,
                &response,
            );
            self.initial_projections += 1;
        }
        assert!(response.get("receipt_id").is_none_or(Value::is_null));
        let result = response["result"].clone();
        assert_eq!(result["project_revision"], revision.to_string());
        self.snapshot = Some(text(&result, "snapshot_id"));
        result
    }
    async fn replay(&self) {
        assert_eq!(self.accepted.len(), 9);
        for (saved, accepted) in &self.accepted {
            // All original bases are stale now. No rebase, new scope, key or ID.
            let retry = self.adapter.invoke(saved).await;
            for field in ["ok", "request_id", "receipt_id", "result"] {
                assert_eq!(
                    retry.get(field),
                    accepted.get(field),
                    "Topology retained mutation field {field} differs for {}",
                    saved.command
                );
            }
            assert_eq!(&retry, accepted, "Topology full canonical replay differs");
        }
    }
    async fn retire(&mut self) {
        let snapshot = self.snapshot.take().expect("Final topology project ticket");
        let mut cli_args = args(&[
            "--access-profile",
            "topology",
            "projects",
            "list",
            "--limit",
            "64",
        ]);
        flag(&mut cli_args, "--replace-snapshot", &snapshot);
        let response = self
            .adapter
            .invoke(&Invocation::new(
                "projects.list",
                json!({"limit":64,"cursor":null,"replace_snapshot":snapshot}),
                json!({}),
                cli_args,
            ))
            .await;
        self.conformance
            .observe(self.case_base + 40, "projects.list", &response);
        assert!(response.get("receipt_id").is_none_or(Value::is_null));
        assert_eq!(response["result"]["projection"], "projects.summary.v1");
        assert!(
            response["result"]["next_cursor"].is_null(),
            "Finite topology catalog must fit one page"
        );
    }
}

struct Project {
    id: String,
    bin: String,
    sequence: String,
    tracks: [String; 2],
    sources: [String; 2],
    clips: [String; 2],
}
impl Project {
    fn entities(&self) -> Vec<String> {
        vec![
            self.bin.clone(),
            self.sequence.clone(),
            self.tracks[0].clone(),
            self.tracks[1].clone(),
            self.sources[0].clone(),
            self.sources[1].clone(),
            self.clips[0].clone(),
            self.clips[1].clone(),
        ]
    }
}
struct Facts {
    clips: [Value; 2],
    nonclips: BTreeMap<String, Value>,
}
fn clip_facts(mut entity: Value) -> Value {
    let fields = entity.as_object_mut().expect("Projected clip object");
    // Hash changes with the two independently asserted mutable payload fields.
    for field in ["payload_sha256", "link_group_id", "sequence_start"] {
        fields.remove(field);
    }
    entity
}
async fn checkpoint(
    journey: &mut Journey<'_, '_>,
    project: &Project,
    revision: u64,
    group: Option<&str>,
    baseline: Option<&Facts>,
) -> Facts {
    let summary = journey
        .projection(
            &project.id,
            revision,
            "projects.inspect",
            json!({"projection":"summary"}),
            args(&["projects", "inspect", "--projection", "summary"]),
        )
        .await;
    assert_eq!(summary["project_id"], project.id);
    assert_eq!(summary["root_bin_id"], project.bin);
    assert_eq!(summary["name"], format!("Topology {}", journey.tag));
    assert_eq!(summary["entity_count"], 8);
    assert_eq!(summary["sequence_count"], 1);
    assert_eq!(summary["schema_major"], 1);
    assert_eq!(summary["envelope_version"], 1);
    assert_eq!(summary["compatibility"], "supported");
    assert_eq!(summary["history"]["can_undo"], true);
    assert_eq!(summary["history"]["can_redo"], revision == 7);
    let mut entities = BTreeMap::new();
    assert_eq!(project.entities().iter().collect::<BTreeSet<_>>().len(), 8);
    for id in project.entities() {
        let result = journey
            .projection(
                &project.id,
                revision,
                "projects.inspect",
                json!({"projection":"entity","entity_id":id}),
                args(&[
                    "projects",
                    "inspect",
                    "--projection",
                    "entity",
                    "--entity",
                    &id,
                ]),
            )
            .await;
        assert_eq!(result["entity"]["id"], id);
        entities.insert(id, result["entity"].clone());
    }
    if let Some(group) = group {
        assert!(
            !entities.contains_key(group),
            "The opaque group is not a ninth entity"
        );
        assert!(!group.is_empty());
    }
    assert_eq!(entities[&project.bin]["kind"], "bin");
    let sequence = &entities[&project.sequence];
    assert_eq!(sequence["kind"], "sequence");
    assert_eq!(
        sequence["settings"],
        json!({"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"})
    );
    assert_eq!(sequence["track_count"], 2);
    let mut clips = [Value::Null, Value::Null];
    for (index, clip_slot) in clips.iter_mut().enumerate() {
        let track = &entities[&project.tracks[index]];
        assert_eq!(track["kind"], "video");
        assert_eq!(track["sequence_id"], project.sequence);
        assert_eq!(track["grid"], rational(4));
        assert_eq!(track["clip_count"], 1);
        let source = &entities[&project.sources[index]];
        assert_eq!(source["kind"], "source_binding");
        assert_eq!(source["source_kind"], "solid_color");
        assert_eq!(source["duration"], rational(4));
        assert_eq!(
            source["rgba"],
            if index == 0 {
                json!([0.0, 0.0, 0.0, 1.0])
            } else {
                json!([1.0, 0.0, 0.0, 1.0])
            }
        );
        let clip = &entities[&project.clips[index]];
        assert_eq!(clip["kind"], "clip");
        assert_eq!(clip["sequence_id"], project.sequence);
        assert_eq!(clip["track_id"], project.tracks[index]);
        assert_eq!(clip["source_id"], project.sources[index]);
        assert!(!text(clip, "occurrence_id").is_empty());
        assert_eq!(clip["duration"], rational(if index == 0 { 4 } else { 2 }));
        assert_eq!(
            clip["source_range"],
            json!({"start":rational(index as u64),"end":rational(if index == 0 {4} else {3})})
        );
        assert_eq!(
            clip["sequence_start"],
            rational(index as u64 + if revision >= 5 { 2 } else { 0 })
        );
        // Missing key must fail; Value indexing alone would turn it into null.
        assert_eq!(
            clip.get("link_group_id"),
            Some(&json!(group)),
            "Supported clip must explicitly project its persisted group"
        );
        if let Some(group) = group {
            assert_ne!(text(clip, "occurrence_id"), group);
        }
        *clip_slot = clip.clone();
        if let Some(baseline) = baseline {
            assert_eq!(
                clip_facts(clip.clone()),
                clip_facts(baseline.clips[index].clone())
            );
        }
    }
    assert_ne!(
        text(&entities[&project.clips[0]], "occurrence_id"),
        text(&entities[&project.clips[1]], "occurrence_id")
    );
    let tracks = journey
        .projection(
            &project.id,
            revision,
            "sequences.inspect",
            json!({"sequence_id":project.sequence,"projection":"track_page","limit":128}),
            args(&[
                "sequences",
                "inspect",
                "--sequence",
                &project.sequence,
                "--projection",
                "track_page",
                "--limit",
                "128",
            ]),
        )
        .await;
    assert!(tracks["next_cursor"].is_null());
    let rows = tracks["items"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        sorted(&rows.iter().map(|row| text(row, "id")).collect::<Vec<_>>()),
        sorted(&project.tracks)
    );
    assert!(rows.iter().all(|row| row["clip_count"] == 1));
    let viewport = journey.projection(&project.id, revision, "sequences.inspect",
        json!({"sequence_id":project.sequence,"projection":"viewport","track_ids":project.tracks,"range":{"start":rational(0),"end":rational(8)},"limit":128}),
        args(&["sequences", "inspect", "--sequence", &project.sequence, "--projection", "viewport", "--track", &project.tracks[0], "--track", &project.tracks[1], "--start", "0/1", "--end", "8/1", "--limit", "128"])).await;
    assert!(viewport["next_cursor"].is_null());
    let rows = viewport["items"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        sorted(&rows.iter().map(|row| text(row, "id")).collect::<Vec<_>>()),
        sorted(&project.clips)
    );
    for (index, clip_id) in project.clips.iter().enumerate() {
        let row = rows
            .iter()
            .find(|row| row["id"].as_str() == Some(clip_id.as_str()))
            .unwrap();
        assert_eq!(row["track_id"], project.tracks[index]);
        assert_eq!(row["source_id"], project.sources[index]);
        assert_eq!(
            row["sequence_start"],
            entities[&project.clips[index]]["sequence_start"]
        );
        assert_eq!(row["duration"], entities[&project.clips[index]]["duration"]);
    }
    for id in &project.clips {
        entities.remove(id);
    }
    if let Some(baseline) = baseline {
        assert_eq!(entities, baseline.nonclips);
    }
    Facts {
        clips,
        nonclips: entities,
    }
}

async fn journey(
    adapter: Adapter<'_>,
    tag: &'static str,
    conformance: &mut schema_validation::Batch,
    case_base: u16,
    extra_receipts: &mut BTreeSet<String>,
) -> BTreeSet<String> {
    let scope = adapter.scope(tag).await;
    let mut journey = Journey {
        conformance,
        case_base,
        initial_projections: 0,
        adapter,
        tag,
        scope,
        accepted: Vec::new(),
        receipts: BTreeSet::new(),
        request_ids: BTreeSet::new(),
        snapshot: None,
    };
    let name = format!("Topology {tag}");
    let created = journey
        .write(
            "create",
            "projects.create",
            json!({"name":name}),
            args(&["projects", "create", "--name", &name]),
            None,
        )
        .await;
    let id = text(&created, "project_id");
    let bin = text(&created, "root_bin_id");
    let sequence = journey.write("sequence", "sequences.create",
        json!({"name":"Sequence","settings":{"width":64,"height":48,"frame_rate":{"num":"4","den":"1"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"A"},{"kind":"video","name":"B"}]}),
        args(&["sequences", "create", "--name", "Sequence", "--width", "64", "--height", "48", "--frame-rate", "4/1", "--sample-rate", "48000", "--audio-layout", "stereo", "--video-track", "A", "--video-track", "B"]), Some((&id, 0))).await;
    let sequence_id = text(&sequence["created"], "sequence_id");
    let rows = sequence["created"]["tracks"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let tracks = [text(&rows[0], "id"), text(&rows[1], "id")];
    assert_eq!(
        sequence["created"],
        json!({"sequence_id":sequence_id,"tracks":[{"id":tracks[0],"kind":"video"},{"id":tracks[1],"kind":"video"}]})
    );
    assert_eq!(
        sequence["changed_ids"],
        sorted(&[
            bin.clone(),
            sequence_id.clone(),
            tracks[0].clone(),
            tracks[1].clone()
        ])
    );
    let mut sources = [String::new(), String::new()];
    let mut clips = [String::new(), String::new()];
    for (index, track) in tracks.iter().enumerate() {
        let rgba = if index == 0 {
            json!([0.0, 0.0, 0.0, 1.0])
        } else {
            json!([1.0, 0.0, 0.0, 1.0])
        };
        let placed = journey.write(if index == 0 {"place-a"} else {"place-b"}, "clips.place",
            json!({"sequence_id":sequence_id,"track_id":track,"source":{"kind":"solid_color","rgba":rgba,"duration":rational(4)},"source_range":{"start":rational(index as u64),"end":rational(if index == 0 {4} else {3})},"sequence_start":rational(index as u64),"collision":"reject"}),
            args(&["clips", "place", "--sequence", &sequence_id, "--track", track, "--rgba", if index == 0 {"0,0,0,1"} else {"1,0,0,1"}, "--source-duration", "4/1", "--source-start", if index == 0 {"0/1"} else {"1/1"}, "--source-end", if index == 0 {"4/1"} else {"3/1"}, "--sequence-start", if index == 0 {"0/1"} else {"1/1"}]), Some((&id, index as u64 + 1))).await;
        sources[index] = text(&placed["created"], "source_id");
        let placed_ids = placed["created"]["clip_ids"].as_array().unwrap();
        assert_eq!(placed_ids.len(), 1);
        clips[index] = placed_ids[0].as_str().unwrap().to_owned();
        assert_eq!(
            placed["created"],
            json!({"source_id":sources[index],"clip_ids":[clips[index]]})
        );
        assert_eq!(
            placed["changed_ids"],
            sorted(&[
                bin.clone(),
                track.clone(),
                sources[index].clone(),
                clips[index].clone()
            ])
        );
    }
    let project = Project {
        id,
        bin,
        sequence: sequence_id,
        tracks,
        sources,
        clips,
    };
    let baseline = checkpoint(&mut journey, &project, 3, None, None).await;
    let members = sorted(&project.clips);
    let linked = journey
        .write(
            "link",
            "clips.link",
            json!({"clip_ids":project.clips,"link_policy":"expand"}),
            args(&[
                "clips",
                "link",
                "--clip",
                &project.clips[0],
                "--clip",
                &project.clips[1],
            ]),
            Some((&project.id, 3)),
        )
        .await;
    let group = text(&linked["created"], "link_group_id");
    assert!(!project.entities().contains(&group));
    assert_eq!(
        linked["created"],
        json!({"link_group_id":group,"clip_ids":members})
    );
    assert_eq!(linked["changed_ids"], members);
    checkpoint(&mut journey, &project, 4, Some(&group), Some(&baseline)).await;
    let moved = journey.write("move", "clips.move",
        json!({"clip_ids":[project.clips[0]],"delta":rational(2),"collision":"reject","link_policy":"expand"}),
        args(&["clips", "move", "--clip", &project.clips[0], "--delta", "2/1"]), Some((&project.id, 4))).await;
    assert_eq!(moved["created"], json!({}));
    assert_eq!(moved["changed_ids"], members);
    checkpoint(&mut journey, &project, 5, Some(&group), Some(&baseline)).await;
    let unlinked = journey
        .write(
            "unlink",
            "clips.unlink",
            json!({"clip_ids":[project.clips[0]],"link_policy":"expand"}),
            args(&["clips", "unlink", "--clip", &project.clips[0]]),
            Some((&project.id, 5)),
        )
        .await;
    assert_eq!(unlinked["created"], json!({"clip_ids":members}));
    assert_eq!(unlinked["changed_ids"], members);
    checkpoint(&mut journey, &project, 6, None, Some(&baseline)).await;
    let undone = journey
        .write(
            "undo",
            "history.undo",
            json!({}),
            args(&["history", "undo"]),
            Some((&project.id, 6)),
        )
        .await;
    assert_eq!(undone["created"], json!({}));
    assert_eq!(undone["changed_ids"], members);
    checkpoint(&mut journey, &project, 7, Some(&group), Some(&baseline)).await;
    let redone = journey
        .write(
            "redo",
            "history.redo",
            json!({}),
            args(&["history", "redo"]),
            Some((&project.id, 7)),
        )
        .await;
    assert_eq!(redone["created"], json!({}));
    assert_eq!(redone["changed_ids"], members);
    journey.replay().await;
    checkpoint(&mut journey, &project, 8, None, Some(&baseline)).await;
    journey.retire().await;
    assert_eq!(
        journey.initial_projections, 11,
        "SCHEMA_TRANSPORT_PROJECTION_SET"
    );
    // These two additive writes follow all original nine-receipt/replay/checkpoint oracles.
    let split_key = format!("topology-{tag}-schema-split");
    let context = json!({"request_id":split_key,"replay_scope":journey.scope,"idempotency_key":split_key,"project":{"id":project.id,"expected_revision":"8"}});
    let selected = args(&[
        "--access-profile",
        "topology",
        "clips",
        "split",
        "--project",
        &project.id,
        "--expected-revision",
        "8",
        "--clip",
        &project.clips[0],
        "--at",
        "3/1",
        "--replay-scope",
        &journey.scope,
        "--idempotency-key",
        &split_key,
        "--request-id",
        &split_key,
    ]);
    let split = journey
        .adapter
        .invoke(&Invocation::new(
            "clips.split",
            json!({"clip_id":project.clips[0],"at_sequence":rational(3),"link_policy":"expand"}),
            context,
            selected,
        ))
        .await;
    journey
        .conformance
        .observe(case_base + 41, "clips.split", &split);
    assert!(
        split["result"]["base_revision"] == "8",
        "SCHEMA_EXTRA_REVISION"
    );
    assert!(split["result"]["revision"] == "9", "SCHEMA_EXTRA_REVISION");
    assert_eq!(
        split["result"]["created"]["clip_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let right = split["result"]["created"]["clip_ids"][0].as_str().unwrap();
    let remove_key = format!("topology-{tag}-schema-remove");
    let context = json!({"request_id":remove_key,"replay_scope":journey.scope,"idempotency_key":remove_key,"project":{"id":project.id,"expected_revision":"9"}});
    let selected = args(&[
        "--access-profile",
        "topology",
        "clips",
        "remove",
        "--project",
        &project.id,
        "--expected-revision",
        "9",
        "--clip",
        right,
        "--replay-scope",
        &journey.scope,
        "--idempotency-key",
        &remove_key,
        "--request-id",
        &remove_key,
    ]);
    let removed = journey
        .adapter
        .invoke(&Invocation::new(
            "clips.remove",
            json!({"clip_ids":[right],"mode":"lift","link_policy":"expand"}),
            context,
            selected,
        ))
        .await;
    journey
        .conformance
        .observe(case_base + 42, "clips.remove", &removed);
    assert!(
        removed["result"]["base_revision"] == "9",
        "SCHEMA_EXTRA_REVISION"
    );
    assert!(
        removed["result"]["revision"] == "10",
        "SCHEMA_EXTRA_REVISION"
    );
    for response in [&split, &removed] {
        let receipt = text(response, "receipt_id");
        assert!(
            !journey.receipts.contains(&receipt),
            "SCHEMA_EXTRA_RECEIPT_DISJOINT"
        );
        assert!(
            extra_receipts.insert(receipt),
            "SCHEMA_EXTRA_RECEIPT_UNIQUE"
        );
    }
    journey
        .conformance
        .reject_copy(case_base + 50, case_base + 1, |v| {
            v["extra"] = json!(true);
        });
    journey.receipts
}
fn retain_receipts(all: &mut BTreeSet<String>, fresh: BTreeSet<String>) {
    assert_eq!(fresh.len(), 9);
    for receipt in fresh {
        assert!(
            all.insert(receipt),
            "Fresh topology receipt reused across adapters"
        );
    }
}

/// The caller bounds this entire phase to 120s inside its existing 20-minute job.
/// Return the new private bearer solely for the caller's protected-log secret checks.
pub async fn run(cli: &Cli, owner_scope: &str, port: u16, token_file: &Path) -> String {
    let profile = json!({"name":"topology","expected_revision":null,"enabled":true,"mode":"operate","owned_object_access":true,"allowed_command_ids":["commands.list","commands.schema","access.replay_scope.create","projects.create","projects.list","projects.inspect","sequences.create","sequences.inspect","clips.place","clips.link","clips.move","clips.unlink","clips.split","clips.remove","history.undo","history.redo"],"object_grants":[],"read_roots":[],"write_roots":[]});
    let mut profile_request = Request::query("access.profiles.put", profile);
    profile_request.request_id = "topology-profile-put".to_owned();
    profile_request.replay_scope = Some(owner_scope.to_owned());
    profile_request.idempotency_key = Some("create-topology-profile".to_owned());
    bounded(&profile_request);
    let response = cli.raw(&profile_request).await;
    bounded(&response);
    assert_eq!(
        response["ok"], true,
        "Public topology profile setup failed; response withheld"
    );
    assert_eq!(response["request_id"], profile_request.request_id);
    let profile_receipt = text(&response, "receipt_id");
    let replay = cli.raw(&profile_request).await;
    bounded(&replay);
    assert!(
        replay == response,
        "Topology profile retained response differs; response withheld"
    );
    let mut receipts = BTreeSet::new();
    let mut extra_receipts = BTreeSet::new();
    let case_ids = |bases: &[u16]| {
        let mut ids = vec![1000, 1001, 1002, 1003, 1004, 1005, 1006];
        for base in bases {
            ids.extend(
                (1..=9)
                    .chain(20..=30)
                    .chain([40, 41, 42, 50])
                    .map(|id| base + id),
            );
        }
        ids
    };
    let cli_adapter = Adapter::Cli(cli);
    let mut cli_batch =
        schema_validation::Batch::new(11, &case_ids(&[0]), cli_adapter.schema_documents().await);
    cli_batch.engine_controls();
    retain_receipts(
        &mut receipts,
        journey(cli_adapter, "cli", &mut cli_batch, 0, &mut extra_receipts).await,
    );
    let cli_responses = output_contracts::capture(&cli_batch);
    cli_batch.finish().await;
    let mut command = cli.command();
    command.args(["bridge", "--access-profile", "topology"]);
    let stdio = ()
        .serve(TokioChildProcess::new(command).unwrap())
        .await
        .unwrap_or_else(|_| panic!("Topology official stdio startup failed"));
    let stdio_adapter = Adapter::Sdk(&stdio);
    let mut stdio_batch =
        schema_validation::Batch::new(12, &case_ids(&[0]), stdio_adapter.schema_documents().await);
    stdio_batch.engine_controls();
    retain_receipts(
        &mut receipts,
        journey(
            stdio_adapter,
            "stdio",
            &mut stdio_batch,
            0,
            &mut extra_receipts,
        )
        .await,
    );
    let stdio_responses = output_contracts::capture(&stdio_batch);
    stdio_batch.finish().await;
    stdio.cancel().await.unwrap();
    let port_string = port.to_string();
    let issue_args = [
        "access",
        "tokens",
        "issue",
        "--for-access-profile",
        "topology",
        "--port",
        &port_string,
        "--out",
        token_file.to_str().unwrap(),
        "--json",
    ];
    bounded(&issue_args);
    let (issued, exit) = cli.human(&issue_args).await;
    assert_eq!(
        exit, 0,
        "Topology private token issuance failed; response withheld"
    );
    bounded(&issued);
    let token =
        String::from_utf8(nw_platform::security::read_private(token_file, 256).unwrap()).unwrap();
    assert!(!token.is_empty());
    assert!(
        !issued.to_string().contains(&token),
        "Topology token issuance disclosed bearer"
    );
    let mut http_batch = None;
    let mut http_schemas = None;
    for (tag, lifecycle) in [
        ("http-initialize", ClientLifecycleMode::Initialize),
        (
            "http-discover",
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        ),
    ] {
        let transport = StreamableHttpClientTransport::with_client(
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
                .auth_header(&token),
        );
        let http = ()
            .serve_with_lifecycle(transport, lifecycle)
            .await
            .unwrap_or_else(|_| panic!("Topology official HTTP startup failed"));
        let adapter = Adapter::Sdk(&http);
        let schemas = adapter.schema_documents().await;
        let base = if tag == "http-initialize" { 0 } else { 100 };
        if let Some(expected) = &http_schemas {
            assert!(&schemas == expected, "SCHEMA_HTTP_DISCOVERY_EQUAL");
        } else {
            http_schemas = Some(schemas.clone());
            let mut batch = schema_validation::Batch::new(13, &case_ids(&[0, 100]), schemas);
            batch.engine_controls();
            http_batch = Some(batch);
        }
        retain_receipts(
            &mut receipts,
            journey(
                adapter,
                tag,
                http_batch.as_mut().expect("SCHEMA_HTTP_BATCH"),
                base,
                &mut extra_receipts,
            )
            .await,
        );
        http.cancel().await.unwrap();
    }
    let http_batch = http_batch.expect("SCHEMA_HTTP_BATCH");
    let http_responses = output_contracts::capture(&http_batch);
    http_batch.finish().await;
    assert_eq!(receipts.len(), 36);
    assert!(
        !receipts.contains(&profile_receipt),
        "Topology project receipt reused profile setup receipt"
    );
    assert_eq!(extra_receipts.len(), 8, "SCHEMA_EXTRA_EIGHT_RECEIPTS");
    assert!(
        extra_receipts.is_disjoint(&receipts) && !extra_receipts.contains(&profile_receipt),
        "SCHEMA_EXTRA_RECEIPT_GLOBAL_DISJOINT"
    );
    // 36 original plus eight additive project writes; profile setup is separate.
    assert_eq!(receipts.len() + extra_receipts.len(), 44);
    output_contracts::run(
        cli,
        port,
        &token,
        cli_responses,
        stdio_responses,
        http_responses,
    )
    .await;
    token
}
