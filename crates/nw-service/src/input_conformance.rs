//! Independent literal input controls run inside an existing service test identity.
use super::*;
use serde_json::{Value, json};

const ID: &str = "11111111-1111-4111-8111-111111111111";

pub(crate) fn literal_cases() -> Vec<(&'static str, Value, bool)> {
    // These are input documents, never dispatched edits or fabricated successes.
    let mut cases: Vec<(&str, Value, bool)> = Vec::new();
    let mut add = |command, params, valid| cases.push((command, params, valid));
    for params in [json!({}), json!({"limit":1}), json!({"limit":512}),
        json!({"cursor":null,"replace_snapshot":"bounded-old-id"}),
        json!({"cursor":"opaque","replace_snapshot":null})] {
        add("projects.list", params, true);
    }
    for params in [json!({"limit":0}), json!({"limit":513}), json!({"limit":null}),
        json!({"cursor":"opaque","replace_snapshot":"old"}), json!({"cursor":"x".repeat(257)}),
        json!({"replace_snapshot":"x".repeat(129)}), json!({"unexpected":true})] {
        add("projects.list", params, false);
    }
    for params in [json!({"projection":"summary"}),
        json!({"projection":"summary","entity_id":null,"bin_id":null,"cursor":null,"replace_snapshot":ID}),
        json!({"projection":"entity","entity_id":ID}),
        json!({"projection":"bin_page","bin_id":ID,"cursor":"opaque","replace_snapshot":null,"limit":512})] {
        add("projects.inspect", params, true);
    }
    for params in [json!({}), json!({"projection":"other"}), json!({"projection":"entity"}),
        json!({"projection":"entity","entity_id":null}), json!({"projection":"bin_page","bin_id":null}),
        json!({"projection":"summary","entity_id":ID}), json!({"projection":"entity","entity_id":ID,"cursor":"opaque"}),
        json!({"projection":"summary","sort":"name"}), json!({"projection":"summary","replace_snapshot":"old"}),
        json!({"projection":"summary","revision":"01"}),
        json!({"projection":"bin_page","bin_id":ID,"cursor":"opaque","replace_snapshot":ID}),
        json!({"projection":"bin_page","bin_id":ID,"cursor":"x".repeat(2049)})] {
        add("projects.inspect", params, false);
    }
    add("projects.inspect", json!({"projection":"bin_page","bin_id":ID,"cursor":"x".repeat(2048)}), true);
    add("projects.inspect", json!({"projection":"summary","limit":0}), false);
    let range = json!({"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}});
    for params in [json!({"sequence_id":ID,"projection":"entity"}),
        json!({"sequence_id":ID,"projection":"entity","track_ids":[],"range":null,"cursor":null}),
        json!({"sequence_id":ID,"projection":"track_page","cursor":"opaque","replace_snapshot":null}),
        json!({"sequence_id":ID,"projection":"viewport","track_ids":[ID],"range":range})] {
        add("sequences.inspect", params, true);
    }
    for params in [json!({"sequence_id":ID}), json!({"sequence_id":ID,"projection":"summary"}),
        json!({"sequence_id":ID,"projection":"viewport","track_ids":[ID],"range":null}),
        json!({"sequence_id":ID,"projection":"viewport","track_ids":[],"range":range}),
        json!({"sequence_id":ID,"projection":"viewport","track_ids":[ID,ID],"range":range}),
        json!({"sequence_id":ID,"projection":"entity","track_ids":null}),
        json!({"sequence_id":ID,"projection":"entity","range":range}),
        json!({"sequence_id":ID,"projection":"track_page","track_ids":[ID]}),
        json!({"sequence_id":ID,"projection":"entity","cursor":"opaque"}),
        json!({"sequence_id":"BAD","projection":"entity"})] {
        add("sequences.inspect", params, false);
    }
    let ids: Vec<_> = (0..129).map(|index| format!("00000000-0000-4000-8000-{index:012}")).collect();
    add("sequences.inspect", json!({"sequence_id":ID,"projection":"viewport","track_ids":&ids[..64],"range":range}), true);
    add("sequences.inspect", json!({"sequence_id":ID,"projection":"viewport","track_ids":&ids[..65],"range":range}), false);
    for params in [json!({"name":"Project"}), json!({"name":"é".repeat(64)})] { add("projects.create", params, true); }
    for params in [json!({"name":""}), json!({"name":"x".repeat(129)}), json!({"name":"line\nbreak"})] { add("projects.create", params, false); }
    let sequence = json!({"name":"Sequence","settings":{"width":64,"height":48,"frame_rate":{"num":"+04","den":"01"},"sample_rate":48000,"audio_layout":"stereo"},"tracks":[{"kind":"video","name":"Video"}]});
    add("sequences.create", sequence.clone(), true);
    for (pointer, bad) in [("/settings/width",json!(0)),("/settings/height",json!(16385)),
        ("/settings/sample_rate",json!(7999)),("/settings/audio_layout",json!("unknown")),
        ("/tracks",json!([])),("/tracks/0/kind",json!("text")),("/tracks/0/name",json!("")),
        ("/settings/frame_rate/num",json!(4))] {
        let mut copy = sequence.clone(); *copy.pointer_mut(pointer).unwrap() = bad;
        add("sequences.create", copy, false);
    }
    for (count, valid) in [(64,true),(65,false)] {
        let mut copy = sequence.clone(); copy["tracks"] = json!(vec![sequence["tracks"][0].clone();count]);
        add("sequences.create", copy, valid);
    }
    let mut maximum = sequence.clone();
    maximum["settings"]["width"] = json!(16384);
    maximum["settings"]["height"] = json!(16384);
    maximum["settings"]["sample_rate"] = json!(384000);
    maximum["settings"]["audio_layout"] = json!("7.1");
    add("sequences.create", maximum.clone(), true);
    maximum["settings"]["sample_rate"] = json!(384001);
    add("sequences.create", maximum, false);
    let place = json!({"sequence_id":ID,"track_id":ID,"source":{"kind":"solid_color","rgba":[0,0,0,1],"duration":{"num":"1","den":"1"}},"source_range":range,"sequence_start":{"num":"0","den":"1"},"collision":"reject"});
    add("clips.place", place.clone(), true);
    for (pointer, bad) in [("/collision",json!("overwrite")),("/source/rgba/0",json!(-0.1)),("/source/rgba/3",json!(1.1))] {
        let mut copy = place.clone(); *copy.pointer_mut(pointer).unwrap() = bad;
        add("clips.place", copy, false);
    }
    for command in ["clips.move","clips.remove"] {
        let mut params = if command == "clips.move" {
            json!({"clip_ids":[ID,ID],"delta":{"num":"+00","den":"01"},"collision":"reject","link_policy":"expand"})
        } else { json!({"clip_ids":[ID,ID],"mode":"lift","link_policy":"expand"}) };
        add(command, params.clone(), true); // Existing resolvers deduplicate these selections.
        params["clip_ids"] = json!([]); add(command, params.clone(), false);
        params["clip_ids"] = json!([ID]); params["link_policy"] = json!("isolate"); add(command, params, false);
        for (count, valid) in [(128,true),(129,false)] {
            let params = if command == "clips.move" {
                json!({"clip_ids":vec![ID;count],"delta":{"num":"0","den":"1"},"collision":"reject","link_policy":"expand"})
            } else { json!({"clip_ids":vec![ID;count],"mode":"lift","link_policy":"expand"}) };
            add(command, params, valid);
        }
    }
    for command in ["clips.link","clips.unlink"] {
        add(command, json!({"clip_ids":[ID],"link_policy":"expand"}), true);
        add(command, json!({"clip_ids":[ID,ID],"link_policy":"expand"}), false);
        add(command, json!({"clip_ids":[ID],"link_policy":"isolate"}), false);
        add(command, json!({"clip_ids":&ids[..128],"link_policy":"expand"}), true);
        add(command, json!({"clip_ids":&ids[..129],"link_policy":"expand"}), false);
    }
    add("clips.split", json!({"clip_id":ID,"at_sequence":{"num":"+04","den":"01"},"link_policy":"expand"}), true);
    add("clips.split", json!({"clip_id":ID,"at_sequence":{"num":"4","den":"1"},"link_policy":"isolate"}), false);
    for command in ["history.undo","history.redo"] {
        add(command, json!({}), true); add(command, json!({"unexpected":true}), false);
    }
    assert_eq!(cases.len(), 94, "R1_LITERAL_INPUT_CASE_SET_CHANGED");
    cases
}

pub(super) fn run() {
    let cases = literal_cases();
    let sequence = cases.iter().find(|(command, _, valid)| *command == "sequences.create" && *valid).unwrap().1.clone();
    let expected: Vec<_> = (1..=94).collect();
    let schemas = schema_validation::COMMANDS.into_iter().map(|id| {
        CommandService.dispatch(Principal::DiscoveryAgent, Request::query("commands.schema", json!({"id":id})))
            .result.expect("R1_INPUT_DESCRIPTOR")["params_schema"].clone()
    }).collect();
    let mut batch = schema_validation::Batch::new(8, &expected, schemas);
    for (index, (command, params, valid)) in cases.iter().enumerate() {
        let schema = schema_validation::COMMANDS.iter().position(|id| id == command).unwrap();
        batch.whole(index as u16 + 1, schema, serde_json::to_string(params).unwrap(), *valid);
    }
    // Schema refinement never replaces the old permissive String DTOs or normalizes Request.params.
    let mut request = Request::query("sequences.create", sequence.clone());
    let digest = request.structural_digest();
    let parsed: nw_domain::projects::SequenceInput = commands::parse(request.params.clone()).unwrap();
    assert_eq!(parsed.settings.frame_rate, nw_domain::time::Rational::new(4,1).unwrap());
    assert!(request.params == sequence && request.structural_digest() == digest);
    request.params["settings"]["frame_rate"]["num"] = json!("4");
    assert!(request.structural_digest() != digest, "R1_RAW_PARAM_DIGEST_NORMALIZED");
    let parsed: nw_domain::projects::views::InspectInput = commands::parse(json!({"projection":"summary","cursor":null})).unwrap();
    assert!(parsed.cursor.is_none() && parsed.limit == 128 && parsed.sort == "label");
    assert!(commands::parse::<nw_domain::projects::views::InspectInput>(json!({})).is_err());
    assert!(commands::parse::<nw_domain::projects::views::InspectInput>(json!({"projection":"future-string"})).is_ok());
    assert!(nw_domain::projects::label(&"é".repeat(65)).is_err(), "R1_UTF8_BYTES_ARE_RUNTIME_CHECK");
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(batch.finish());
}
