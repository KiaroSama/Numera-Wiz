//! Shared test-only JSON boundary. Never dispatch result-copy controls or expose private inputs.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub const COMMANDS: [&str; 13] = [
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

pub struct Batch {
    suite: u16,
    expected: BTreeSet<u16>,
    schemas: Vec<Value>,
    cases: Vec<Value>,
    controls: Vec<Value>,
    ids: BTreeSet<u16>,
}
impl Batch {
    pub fn new(suite: u16, expected: &[u16], schemas: Vec<Value>) -> Self {
        assert_eq!(schemas.len(), COMMANDS.len(), "SCHEMA_DOCUMENT_COUNT");
        assert_eq!(
            expected.iter().copied().collect::<BTreeSet<_>>().len(),
            expected.len(),
            "SCHEMA_EXPECTED_DUPLICATE"
        );
        let schemas = schemas.into_iter().enumerate().map(|(id, schema)| {
            json!({"id":id,"schema_json":serde_json::to_string(&schema).expect("SCHEMA_SERIALIZE")})
        }).collect();
        Self {
            suite,
            expected: expected.iter().copied().collect(),
            schemas,
            cases: Vec::new(),
            controls: Vec::new(),
            ids: BTreeSet::new(),
        }
    }
    pub fn capture<T: serde::Serialize>(&mut self, id: u16, command: &str, response: T) -> T {
        self.observe(id, command, &response);
        response
    }
    pub fn whole(&mut self, id: u16, schema_id: usize, instance_json: String, valid: bool) {
        self.case(id, schema_id, instance_json, "whole", valid);
    }
    pub fn observations(&self) -> impl Iterator<Item = &Value> {
        self.cases.iter()
    }
    pub fn observe(&mut self, id: u16, command: &str, response: &impl serde::Serialize) {
        let bytes = serde_json::to_vec(response).expect("SCHEMA_RESPONSE_SERIALIZE");
        assert!(bytes.len() <= 262_144, "SCHEMA_WHOLE_RESPONSE_BOUND");
        let schema_id = COMMANDS
            .iter()
            .position(|candidate| *candidate == command)
            .expect("SCHEMA_COMMAND_ID");
        let response: Value = serde_json::from_slice(&bytes).expect("SCHEMA_RESPONSE_JSON");
        assert_eq!(
            response.get("ok"),
            Some(&json!(true)),
            "SCHEMA_ACTUAL_SUCCESS"
        );
        assert!(
            response.get("result").is_some_and(|v| !v.is_null()),
            "SCHEMA_ACTUAL_RESULT"
        );
        self.case(
            id,
            schema_id,
            String::from_utf8(bytes).expect("SCHEMA_RESPONSE_UTF8"),
            "result",
            true,
        );
    }
    fn case(
        &mut self,
        id: u16,
        schema_id: usize,
        instance_json: String,
        select: &str,
        valid: bool,
    ) {
        assert!(self.ids.insert(id), "SCHEMA_CASE_DUPLICATE");
        assert!(instance_json.len() <= 262_144, "SCHEMA_INSTANCE_BOUND");
        self.cases.push(json!({"id":id,"schema_id":schema_id,"instance_json":instance_json,"select":select,"valid":valid}));
        assert!(self.cases.len() <= 128, "SCHEMA_CASE_BOUND");
    }
    pub fn reject_copy(&mut self, id: u16, source: u16, mutate: impl FnOnce(&mut Value)) {
        let case = self
            .cases
            .iter()
            .find(|case| case["id"] == source)
            .expect("SCHEMA_NEGATIVE_SOURCE");
        let schema_id = case["schema_id"].as_u64().expect("SCHEMA_NEGATIVE_SCHEMA") as usize;
        let mut response: Value = serde_json::from_str(
            case["instance_json"]
                .as_str()
                .expect("SCHEMA_NEGATIVE_JSON"),
        )
        .expect("SCHEMA_NEGATIVE_PARSE");
        mutate(response.get_mut("result").expect("SCHEMA_NEGATIVE_RESULT"));
        self.case(
            id,
            schema_id,
            serde_json::to_string(&response).expect("SCHEMA_NEGATIVE_SERIALIZE"),
            "result",
            false,
        );
    }
    pub fn engine_controls(&mut self) {
        for (id, schema) in [
            (
                100,
                r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"integer","const":9007199254740993}"#,
            ),
            (
                101,
                r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"number","multipleOf":0.00000000000000000001}"#,
            ),
            (
                102,
                r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"string","format":"uuid"}"#,
            ),
        ] {
            self.schemas.push(json!({"id":id,"schema_json":schema}));
        }
        for (id, schema, instance, valid) in [
            (1000, 100, "9007199254740993", true),
            (1001, 100, "9007199254740992", false),
            (1002, 100, "9007199254740994", false),
            (1003, 101, "0.12345678901234567890", true),
            (1004, 101, "0.123456789012345678901", false),
            (1005, 102, r#""550e8400-e29b-41d4-a716-446655440000""#, true),
            (1006, 102, r#""invalid-uuid""#, false),
        ] {
            self.whole(id, schema, instance.into(), valid);
        }
        for (id, schema, outcome) in [
            (
                200,
                r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","required":7}"#,
                "meta_invalid",
            ),
            (
                201,
                r##"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/missing"}"##,
                "build_invalid",
            ),
            (
                202,
                r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"https://unregistered.invalid/schema"}"#,
                "build_invalid",
            ),
        ] {
            self.controls
                .push(json!({"id":id,"schema_json":schema,"outcome":outcome}));
        }
    }
    pub async fn finish(self) {
        assert_eq!(
            self.ids, self.expected,
            "SCHEMA_EXPECTED_CASE_SET suite={}",
            self.suite
        );
        assert_eq!(
            std::env::var("NW_SCHEMA_VALIDATOR_REQUIRED")
                .ok()
                .as_deref(),
            Some("1"),
            "SCHEMA_REQUIRED_CONFIG"
        );
        let exe = configured_path("NW_SCHEMA_VALIDATOR_EXE");
        let evidence = configured_path("NW_SCHEMA_VALIDATOR_EVIDENCE_DIR");
        let source = configured_identity("NW_SCHEMA_VALIDATOR_SOURCE", 40);
        let target = std::env::var("NW_SCHEMA_VALIDATOR_TARGET").expect("SCHEMA_TARGET_CONFIG");
        assert!(
            [
                "x86_64-unknown-linux-gnu",
                "x86_64-pc-windows-msvc",
                "aarch64-apple-darwin",
                "x86_64-apple-darwin"
            ]
            .contains(&target.as_str()),
            "SCHEMA_TARGET_CONFIG"
        );
        assert!(exe.is_file(), "SCHEMA_EXE_MISSING");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        // Both inclusion sites live two directories beneath the repository root.
        let root = root
            .parent()
            .and_then(Path::parent)
            .expect("SCHEMA_REPO_ROOT");
        let lock = std::fs::read(root.join("xtask/schema-validator/Cargo.lock"))
            .expect("SCHEMA_OWN_LOCK_MISSING");
        let lock_hash = nw_domain::projects::hash(&lock);
        let expected_lock = configured_identity("NW_SCHEMA_VALIDATOR_LOCK_SHA256", 64);
        assert_eq!(lock_hash, expected_lock, "SCHEMA_LOCK_IDENTITY");
        let expected_exe = root
            .join("target/schema-validator/debug")
            .join(if cfg!(windows) {
                "nw-schema-validator.exe"
            } else {
                "nw-schema-validator"
            });
        assert!(
            std::fs::canonicalize(&exe).expect("SCHEMA_EXE_CANONICAL")
                == std::fs::canonicalize(expected_exe).expect("SCHEMA_EXE_LOCATION"),
            "SCHEMA_EXE_LOCATION"
        );
        let exe_hash = nw_domain::projects::hash(&std::fs::read(&exe).expect("SCHEMA_EXE_READ"));
        let schema_hashes:Vec<Value> = self.schemas.iter().map(|schema| json!({"id":schema["id"],"sha256":nw_domain::projects::hash(schema["schema_json"].as_str().expect("SCHEMA_TEXT").as_bytes())})).collect();
        let schema_ids = numeric_ids(&self.schemas);
        let control_ids = numeric_ids(&self.controls);
        let expected_passed = self.schemas.len() + self.cases.len() + self.controls.len();
        let case_schemas: BTreeMap<u16, u16> = self
            .observations()
            .map(|case| {
                (
                    case["id"].as_u64().expect("SCHEMA_CASE_ID") as u16,
                    case["schema_id"].as_u64().expect("SCHEMA_CASE_SCHEMA") as u16,
                )
            })
            .collect();
        let request = json!({"protocol":1,"suite":self.suite,"schemas":self.schemas,"cases":self.cases,"schema_controls":self.controls});
        let bytes = serde_json::to_vec(&request).expect("SCHEMA_BATCH_SERIALIZE");
        assert!(bytes.len() <= 4 * 1024 * 1024, "SCHEMA_INPUT_BOUND");
        let schema_bytes: usize = request["schemas"]
            .as_array()
            .expect("SCHEMA_LIST")
            .iter()
            .chain(
                request["schema_controls"]
                    .as_array()
                    .expect("SCHEMA_CONTROLS"),
            )
            .map(|s| s["schema_json"].as_str().expect("SCHEMA_TEXT").len())
            .sum();
        assert!(
            schema_ids.len() + control_ids.len() <= 64 && schema_bytes <= 512 * 1024,
            "SCHEMA_DOCUMENT_BOUND"
        );
        let mut input = tempfile::tempfile().expect("SCHEMA_PRIVATE_INPUT");
        input.write_all(&bytes).expect("SCHEMA_PRIVATE_WRITE");
        input
            .seek(SeekFrom::Start(0))
            .expect("SCHEMA_PRIVATE_REWIND");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let child_capture = captured.clone();
        let started = std::time::Instant::now();
        let output = nw_platform::process::stream_readonly(
            &exe,
            &[],
            Duration::from_secs(10),
            input,
            Box::new(move |chunk| {
                let mut bytes = child_capture
                    .lock()
                    .map_err(|_| nw_domain::DomainError::new("SCHEMA_CAPTURE", "SCHEMA_CAPTURE"))?;
                if bytes.len() + chunk.len() > 16_384 {
                    return Err(nw_domain::DomainError::new(
                        "SCHEMA_STDOUT_BOUND",
                        "SCHEMA_STDOUT_BOUND",
                    ));
                }
                bytes.extend_from_slice(chunk);
                Ok(())
            }),
            CancellationToken::new(),
        )
        .await;
        // The unnamed regular tempfile is closed and removed by its owner on every exit.
        let captured = captured.lock().expect("SCHEMA_CAPTURE_LOCK");
        let summary = serde_json::from_slice::<Summary>(&captured).ok();
        let valid_summary = summary.as_ref().is_some_and(|summary| {
            summary.protocol == 1
                && summary.suite == self.suite
                && summary.source == source
                && summary.target == target
                && summary.lock_sha256 == expected_lock
                && summary.schema_ids == schema_ids
                && summary.case_ids == self.expected.iter().copied().collect::<Vec<_>>()
                && summary.control_ids == control_ids
                && summary.passed <= expected_passed
                && summary.failed <= expected_passed
                && summary.passed.checked_add(summary.failed) == Some(expected_passed)
                && summary.failures.len() <= 16
                && summary.failures.len() == summary.failed.min(16)
                && summary
                    .failures
                    .iter()
                    .all(|failure| failure.checked(&schema_ids, &case_schemas, &control_ids))
                && summary
                    .failures
                    .iter()
                    .map(|f| (f.code.tag(), f.schema_id, f.case_id, f.control_id))
                    .collect::<BTreeSet<_>>()
                    .len()
                    == summary.failures.len()
        });
        if valid_summary {
            let summary = summary.as_ref().expect("SCHEMA_SUMMARY");
            std::fs::create_dir_all(&evidence).expect("SCHEMA_EVIDENCE_CREATE");
            let identity = json!({"protocol":1,"suite":self.suite,"source":source,"target":target,"exe_sha256":exe_hash,"lock_sha256":lock_hash,"manifest_sha256":nw_domain::projects::hash(&std::fs::read(root.join("xtask/schema-validator/Cargo.toml")).expect("SCHEMA_MANIFEST_READ")),"schema_hashes":schema_hashes,"summary":serde_json::from_slice::<Value>(&captured).expect("SCHEMA_SUMMARY_JSON"),"elapsed_ms":started.elapsed().as_millis()});
            let identity = serde_json::to_vec_pretty(&identity).expect("SCHEMA_EVIDENCE_SERIALIZE");
            assert!(identity.len() <= 32_768, "SCHEMA_EVIDENCE_BOUND");
            std::fs::write(
                evidence.join(format!("suite-{}.json", self.suite)),
                identity,
            )
            .expect("SCHEMA_EVIDENCE_WRITE");
            for failure in &summary.failures {
                eprintln!(
                    "SCHEMA_FAILURE code={} suite={} schema={:?} case={:?} control={:?}",
                    failure.code.tag(),
                    self.suite,
                    failure.schema_id,
                    failure.case_id,
                    failure.control_id
                );
            }
        }
        let output = output.unwrap_or_else(|_| panic!("SCHEMA_CHILD_FAILED suite={}", self.suite));
        assert!(
            output.exit_code == Some(0) && output.diagnostic_bytes <= 4096,
            "SCHEMA_CHILD_STATUS suite={}",
            self.suite
        );
        assert!(
            valid_summary,
            "SCHEMA_SUMMARY_REJECTED suite={}",
            self.suite
        );
        let summary = summary.expect("SCHEMA_SUMMARY_MISSING");
        assert!(
            summary.failed == 0
                && !summary.infrastructure
                && summary.passed == expected_passed
                && summary.failures.is_empty(),
            "SCHEMA_CONFORMANCE suite={}",
            self.suite
        );
    }
}
fn numeric_ids(values: &[Value]) -> Vec<u16> {
    let mut ids: Vec<_> = values
        .iter()
        .map(|v| u16::try_from(v["id"].as_u64().expect("SCHEMA_ID")).expect("SCHEMA_ID_BOUND"))
        .collect();
    ids.sort_unstable();
    assert!(
        ids.windows(2).all(|pair| pair[0] != pair[1]),
        "SCHEMA_ID_DUPLICATE"
    );
    ids
}
fn configured_path(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("SCHEMA_PATH_CONFIG"));
    assert!(path.is_absolute(), "SCHEMA_ABSOLUTE_CONFIG");
    path
}
fn configured_identity(name: &str, len: usize) -> String {
    let value = std::env::var(name).expect("SCHEMA_IDENTITY_CONFIG");
    assert!(
        value.len() == len
            && value
                .bytes()
                .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase()),
        "SCHEMA_IDENTITY_CONFIG"
    );
    value
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    protocol: u8,
    suite: u16,
    source: String,
    target: String,
    lock_sha256: String,
    schema_ids: Vec<u16>,
    case_ids: Vec<u16>,
    control_ids: Vec<u16>,
    passed: usize,
    failed: usize,
    infrastructure: bool,
    failures: Vec<Failure>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    code: FailureCode,
    #[serde(deserialize_with = "required_id")]
    schema_id: Option<u16>,
    #[serde(deserialize_with = "required_id")]
    case_id: Option<u16>,
    #[serde(deserialize_with = "required_id")]
    control_id: Option<u16>,
}
fn required_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<u16>, D::Error> {
    Option::<u16>::deserialize(deserializer)
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum FailureCode {
    Protocol,
    SchemaMetaOrRef,
    SchemaBuild,
    SchemaUnavailable,
    Outcome,
    SchemaControl,
}
impl FailureCode {
    fn tag(&self) -> &'static str {
        match self {
            Self::Protocol => "PROTOCOL",
            Self::SchemaMetaOrRef => "SCHEMA_META_OR_REF",
            Self::SchemaBuild => "SCHEMA_BUILD",
            Self::SchemaUnavailable => "SCHEMA_UNAVAILABLE",
            Self::Outcome => "OUTCOME",
            Self::SchemaControl => "SCHEMA_CONTROL",
        }
    }
}
impl Failure {
    fn checked(&self, schemas: &[u16], cases: &BTreeMap<u16, u16>, controls: &[u16]) -> bool {
        match self.code {
            FailureCode::SchemaMetaOrRef | FailureCode::SchemaBuild => {
                self.schema_id.is_some_and(|id| schemas.contains(&id))
                    && self.case_id.is_none()
                    && self.control_id.is_none()
            }
            FailureCode::SchemaUnavailable | FailureCode::Outcome => {
                self.schema_id.is_some_and(|id| schemas.contains(&id))
                    && self
                        .case_id
                        .is_some_and(|id| cases.get(&id) == self.schema_id.as_ref())
                    && self.control_id.is_none()
            }
            FailureCode::SchemaControl => {
                self.schema_id.is_none()
                    && self.case_id.is_none()
                    && self.control_id.is_some_and(|id| controls.contains(&id))
            }
            FailureCode::Protocol => false,
        }
    }
}
