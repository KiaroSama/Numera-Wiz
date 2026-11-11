//! Validate opaque JSON recursively without ever decoding a number as f64/Value.
use super::{Result, invalid, limit};
use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use serde_json::value::RawValue;
use std::collections::BTreeSet;

pub(super) fn validate(raw: &str) -> Result<()> {
    visit(raw, 0)
}
fn visit(raw: &str, depth: usize) -> Result<()> {
    // Together with the existing 64 KiB payload limit this bounds parser work and stack.
    if depth >= 128 {
        return Err(limit("Opaque JSON nesting exceeds 128 levels"));
    }
    let raw: Box<RawValue> = serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
    let text = raw.get();
    let mut decoder = serde_json::Deserializer::from_str(text);
    let visitor = Containers { depth };
    match text.as_bytes().first() {
        Some(b'{') => decoder.deserialize_map(visitor).map_err(|e| invalid(e.to_string()))?,
        Some(b'[') => decoder.deserialize_seq(visitor).map_err(|e| invalid(e.to_string()))?,
        _ => return Ok(()), // RawValue already checked scalar syntax, preserving numeric text.
    }
    decoder.end().map_err(|e| invalid(e.to_string()))
}
struct Containers { depth: usize }
impl<'de> Visitor<'de> for Containers {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON with unique object member names")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<(), M::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(M::Error::custom("Duplicate opaque JSON object member"));
            }
            let value: Box<RawValue> = map.next_value()?;
            visit(value.get(), self.depth + 1).map_err(M::Error::custom)?;
        }
        Ok(())
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> std::result::Result<(), S::Error> {
        while let Some(value) = seq.next_element::<Box<RawValue>>()? {
            visit(value.get(), self.depth + 1).map_err(S::Error::custom)?;
        }
        Ok(())
    }
}
