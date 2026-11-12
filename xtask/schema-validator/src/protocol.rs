//! Private version-one transport. No application types or dependencies cross this boundary.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const INPUT_LIMIT: usize = 4 * 1024 * 1024;
pub const INSTANCE_LIMIT: usize = 262_144;
pub const SCHEMA_LIMIT: usize = 512 * 1024;
pub const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub protocol: u8,
    pub suite: u16,
    pub schemas: Vec<Schema>,
    pub cases: Vec<Case>,
    pub schema_controls: Vec<SchemaControl>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub id: u16,
    pub schema_json: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: u16,
    pub schema_id: u16,
    pub instance_json: String,
    pub select: Select,
    pub valid: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Select {
    Result,
    Whole,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaControl {
    pub id: u16,
    pub schema_json: String,
    pub outcome: SchemaOutcome,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaOutcome {
    MetaInvalid,
    BuildInvalid,
}
#[derive(Serialize)]
pub struct Summary {
    pub protocol: u8,
    pub suite: u16,
    pub source: &'static str,
    pub target: &'static str,
    pub lock_sha256: &'static str,
    pub schema_ids: Vec<u16>,
    pub case_ids: Vec<u16>,
    pub control_ids: Vec<u16>,
    pub passed: usize,
    pub failed: usize,
    pub infrastructure: bool,
    pub failures: Vec<Failure>,
}
#[derive(Serialize)]
pub struct Failure {
    pub code: &'static str,
    pub schema_id: Option<u16>,
    pub case_id: Option<u16>,
    pub control_id: Option<u16>,
}
impl Summary {
    pub fn new() -> Self {
        Self {
            protocol: 1,
            suite: 0,
            source: option_env!("NW_SCHEMA_VALIDATOR_SOURCE").unwrap_or("unqualified"),
            target: option_env!("NW_SCHEMA_VALIDATOR_TARGET").unwrap_or("unqualified"),
            lock_sha256: option_env!("NW_SCHEMA_VALIDATOR_LOCK_SHA256").unwrap_or("unqualified"),
            schema_ids: Vec::new(),
            case_ids: Vec::new(),
            control_ids: Vec::new(),
            passed: 0,
            failed: 0,
            infrastructure: false,
            failures: Vec::new(),
        }
    }
    pub fn failure(&mut self, failure: Failure, infrastructure: bool) {
        self.failed += 1;
        self.infrastructure |= infrastructure;
        if self.failures.len() < 16 {
            self.failures.push(failure);
        }
    }
}

// Lexical pass precedes Value deserialization: duplicate keys cannot disappear in a map,
// and nesting is bounded before allocating or recursively traversing a JSON tree.
// serde_json remains the grammar and numeric parser after this pass.
pub fn preflight(bytes: &[u8]) -> Result<(), ()> {
    struct Scan<'a> {
        bytes: &'a [u8],
        offset: usize,
    }
    impl Scan<'_> {
        fn whitespace(&mut self) {
            while self
                .bytes
                .get(self.offset)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.offset += 1;
            }
        }
        fn string(&mut self) -> Result<&[u8], ()> {
            let start = self.offset;
            if self.bytes.get(self.offset) != Some(&b'"') {
                return Err(());
            }
            self.offset += 1;
            while let Some(byte) = self.bytes.get(self.offset) {
                match byte {
                    b'"' => {
                        self.offset += 1;
                        return Ok(&self.bytes[start..self.offset]);
                    }
                    b'\\' => self.offset += 2,
                    0..=31 => return Err(()),
                    _ => self.offset += 1,
                }
            }
            Err(())
        }
        fn value(&mut self, depth: usize) -> Result<(), ()> {
            if depth > 64 {
                return Err(());
            }
            self.whitespace();
            match self.bytes.get(self.offset) {
                Some(b'{') => {
                    self.offset += 1;
                    self.whitespace();
                    let mut keys = BTreeSet::new();
                    if self.bytes.get(self.offset) != Some(&b'}') {
                        loop {
                            self.whitespace();
                            let key: String =
                                serde_json::from_slice(self.string()?).map_err(|_| ())?;
                            if !keys.insert(key) {
                                return Err(());
                            }
                            self.whitespace();
                            if self.bytes.get(self.offset) != Some(&b':') {
                                return Err(());
                            }
                            self.offset += 1;
                            self.value(depth + 1)?;
                            self.whitespace();
                            if self.bytes.get(self.offset) != Some(&b',') {
                                break;
                            }
                            self.offset += 1;
                        }
                    }
                    if self.bytes.get(self.offset) != Some(&b'}') {
                        return Err(());
                    }
                    self.offset += 1;
                }
                Some(b'[') => {
                    self.offset += 1;
                    self.whitespace();
                    if self.bytes.get(self.offset) != Some(&b']') {
                        loop {
                            self.value(depth + 1)?;
                            self.whitespace();
                            if self.bytes.get(self.offset) != Some(&b',') {
                                break;
                            }
                            self.offset += 1;
                        }
                    }
                    if self.bytes.get(self.offset) != Some(&b']') {
                        return Err(());
                    }
                    self.offset += 1;
                }
                Some(b'"') => {
                    self.string()?;
                }
                Some(_) => {
                    let start = self.offset;
                    while self.bytes.get(self.offset).is_some_and(|b| {
                        !b.is_ascii_whitespace() && ![b',', b']', b'}'].contains(b)
                    }) {
                        self.offset += 1;
                    }
                    if self.offset == start {
                        return Err(());
                    }
                }
                None => return Err(()),
            }
            Ok(())
        }
    }
    let mut scan = Scan { bytes, offset: 0 };
    scan.value(0)?;
    scan.whitespace();
    if scan.offset != bytes.len() {
        return Err(());
    }
    Ok(())
}
