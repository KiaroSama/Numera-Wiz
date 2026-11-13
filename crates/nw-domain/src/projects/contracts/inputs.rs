//! Schema refinements preserve the existing serde/parser/digest path exactly.
use serde_json::{Value, json};

const UUID: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const REVISION: &str = "^(0|[1-9][0-9]*)$";
const NAME: &str =
    "Name is 1-128 UTF-8 bytes without controls; the byte bound is checked at runtime.";

fn properties(schema: &mut Value) -> &mut serde_json::Map<String, Value> {
    schema["properties"]
        .as_object_mut()
        .expect("Generated input properties")
}

fn field<'a>(schema: &'a mut Value, name: &str) -> &'a mut Value {
    properties(schema)
        .get_mut(name)
        .expect("Generated input field")
}

fn enumeration(schema: &mut Value, name: &str, values: &[&str]) {
    field(schema, name)["enum"] = json!(values);
}

fn bounded_array(schema: &mut Value, name: &str, minimum: usize, maximum: usize) {
    let array = field(schema, name);
    array["minItems"] = json!(minimum);
    array["maxItems"] = json!(maximum);
}

fn label(schema: &mut Value) {
    schema["minLength"] = json!(1);
    schema["maxLength"] = json!(128);
    schema["pattern"] = json!("^[^\\u0000-\\u001f\\u007f-\\u009f]+$");
    schema["description"] = json!(NAME);
}

fn page(schema: &mut Value, detail: bool) {
    field(schema, "limit")["minimum"] = json!(1);
    field(schema, "limit")["maximum"] = json!(512);
    let cursor = field(schema, "cursor");
    cursor["maxLength"] = json!(if detail { 2048 } else { 256 });
    cursor["description"] =
        json!("Opaque cursor; the advertised maximum also applies to UTF-8 bytes at runtime.");
    let replacement = field(schema, "replace_snapshot");
    if detail {
        replacement["pattern"] = json!(UUID);
    } else {
        replacement["maxLength"] = json!(128);
        replacement["description"] =
            json!("Replacement identity is bounded to 128 UTF-8 bytes at runtime.");
    }
    // An explicit null is equivalent to absence for both existing Option fields.
    schema["allOf"] = json!([{"not":{
        "required":["cursor","replace_snapshot"],
        "properties":{"cursor":{"not":{"type":"null"}},"replace_snapshot":{"not":{"type":"null"}}}
    }}]);
}

fn inspect(schema: &mut Value, sequence: bool) {
    page(schema, true);
    if sequence {
        enumeration(schema, "projection", &["entity", "track_page", "viewport"]);
        bounded_array(schema, "track_ids", 0, 64);
        field(schema, "track_ids")["uniqueItems"] = json!(true);
        schema["oneOf"] = json!([
            {"properties":{"projection":{"const":"entity"},"track_ids":{"maxItems":0},"range":{"type":"null"},"cursor":{"type":"null"}}},
            {"properties":{"projection":{"const":"track_page"},"track_ids":{"maxItems":0},"range":{"type":"null"}}},
            {"required":["track_ids","range"],"properties":{"projection":{"const":"viewport"},"track_ids":{"minItems":1},"range":{"not":{"type":"null"}}}}
        ]);
    } else {
        enumeration(schema, "projection", &["summary", "entity", "bin_page"]);
        enumeration(schema, "sort", &["label"]);
        schema["oneOf"] = json!([
            {"properties":{"projection":{"const":"summary"},"entity_id":{"type":"null"},"bin_id":{"type":"null"},"cursor":{"type":"null"}}},
            {"required":["entity_id"],"properties":{"projection":{"const":"entity"},"entity_id":{"not":{"type":"null"}},"bin_id":{"type":"null"},"cursor":{"type":"null"}}},
            {"required":["bin_id"],"properties":{"projection":{"const":"bin_page"},"bin_id":{"not":{"type":"null"}},"entity_id":{"type":"null"}}}
        ]);
    }
}

/// Refine generated schemas only for the implemented project command family.
pub fn refine(command: &str, mut schema: Value) -> Value {
    if !super::super::authorization::PROJECT_TARGET_COMMANDS.contains(&command)
        && !matches!(command, "projects.create" | "projects.list")
    {
        return schema;
    }
    if let Some(defs) = schema.get_mut("$defs").and_then(Value::as_object_mut) {
        if let Some(id) = defs.get_mut("EntityId") {
            id["pattern"] = json!(UUID);
        }
        if let Some(revision) = defs.get_mut("Revision") {
            revision["pattern"] = json!(REVISION);
            revision["description"] = json!(
                "Canonical unsigned decimal; the u64 bound is checked by the runtime parser."
            );
        }
    }
    match command {
        "projects.create" => label(field(&mut schema, "name")),
        "projects.list" => page(&mut schema, false),
        "projects.inspect" => inspect(&mut schema, false),
        "sequences.inspect" => inspect(&mut schema, true),
        "sequences.create" => {
            label(field(&mut schema, "name"));
            bounded_array(&mut schema, "tracks", 1, 64);
            label(&mut schema["$defs"]["TrackInput"]["properties"]["name"]);
            let settings = &mut schema["$defs"]["SequenceSettings"];
            for (name, minimum, maximum) in [
                ("width", 1, 16384),
                ("height", 1, 16384),
                ("sample_rate", 8000, 384000),
            ] {
                field(settings, name)["minimum"] = json!(minimum);
                field(settings, name)["maximum"] = json!(maximum);
            }
            enumeration(
                settings,
                "audio_layout",
                &["mono", "stereo", "2.1", "3.0", "4.0", "5.1", "7.1"],
            );
            field(settings, "frame_rate")["description"] = json!(
                "Exact positive frame rate <=1000; arithmetic and normalization remain runtime checks."
            );
        }
        "clips.place" => {
            enumeration(&mut schema, "collision", &["reject"]);
            let branches = schema["$defs"]["PlaceSource"]["oneOf"]
                .as_array_mut()
                .expect("Generated source variants");
            for branch in branches {
                match branch["properties"]["kind"]["const"].as_str() {
                    Some("solid_color") => {
                        let items = &mut branch["properties"]["rgba"]["items"];
                        items["minimum"] = json!(0);
                        items["maximum"] = json!(1);
                    }
                    Some("registered") => {
                        branch["properties"]["stream_index"]["maximum"] = json!(u32::MAX);
                    }
                    _ => unreachable!("Unknown generated source variant"),
                }
            }
        }
        "clips.move" => {
            bounded_array(&mut schema, "clip_ids", 1, 128);
            enumeration(&mut schema, "collision", &["reject"]);
            enumeration(&mut schema, "link_policy", &["expand"]);
        }
        "clips.link" | "clips.unlink" => {} // Existing typed policy and unique selection are retained.
        "clips.split" => enumeration(&mut schema, "link_policy", &["expand"]),
        "clips.remove" => {
            bounded_array(&mut schema, "clip_ids", 1, 128);
            enumeration(&mut schema, "mode", &["lift"]);
            enumeration(&mut schema, "link_policy", &["expand"]);
        }
        "history.undo" | "history.redo" => {}
        _ => unreachable!("Unknown implemented project command"),
    }
    schema
}
