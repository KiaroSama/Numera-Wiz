//! Real two-source cleanup batches through the typed CLI and official SDK transports.
//! Hosted CI only: no subprocess, media or application execution belongs in local authoring.
#[path = "../../../test-support/media.rs"]
mod fixture;
#[path = "support/owner.rs"]
mod owner;
use nw_domain::{execution::CleanupLimits, mux::Selector};
use owner::Cli;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio_util::sync::CancellationToken;

macro_rules! sdk {
    ($client:expr, $id:expr, $params:expr, $context:expr) => {{
        let arguments = json!({"params": $params, "context": $context});
        let reply = $client.call_tool(CallToolRequestParams::new($id)
            .with_arguments(arguments.as_object().unwrap().clone())).await.unwrap();
        serde_json::to_value(reply).unwrap()["structuredContent"].clone()
    }};
}
macro_rules! call {
    ($client:expr, $id:expr, $params:expr, $context:expr) => {{
        let reply = sdk!($client, $id, $params, $context);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["result"].clone()
    }};
}
macro_rules! finished {
    ($client:expr, $id:expr) => {{
        let started = std::time::Instant::now();
        loop {
            let result = call!($client, "batches.inspect", json!({"id":$id}), json!({}));
            if result["complete"] == true { break result; }
            assert!(started.elapsed() < Duration::from_secs(30), "{result}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }};
}
#[path = "support/lost_ack.rs"]
mod lost_ack;
fn input(
    assets: &[Value],
    outputs: &[std::path::PathBuf],
    intent: &nw_domain::mux::CleanIntent,
    failure: &str,
) -> Value {
    json!({
        "items": assets.iter().zip(outputs).map(|(asset,output)| json!({"asset_id":asset["id"],"output":output})).collect::<Vec<_>>(),
        "intent":intent, "file_times":"new", "execution_limits":CleanupLimits::default(),
        "failure_policy":failure, "unresolved_policy":"reject"
    })
}
fn folder_batch_input(snapshot: &Value, output_root: &Path) -> Value {
    json!({
        "snapshot_id":snapshot["id"], "snapshot_hash":snapshot["hash"], "output_root":output_root,
        "intent":fixture::rules(), "file_times":"new", "execution_limits":CleanupLimits::default(),
        "failure_policy":"reject_all", "unresolved_policy":"reject"
    })
}
async fn packets(bin: &Path, path: &Path) -> BTreeMap<u64, Vec<Value>> {
    let mut args = [
        "-v",
        "error",
        "-show_packets",
        "-show_data_hash",
        "sha256",
        "-show_entries",
        "packet=stream_index,pts,dts,duration,size,flags,data_hash:packet_side_data",
        "-of",
        "json",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(path.to_str().unwrap().into());
    let result = nw_platform::process::run(
        &bin.join("ffprobe"),
        &args,
        Duration::from_secs(30),
        4 * 1024 * 1024,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    let mut grouped = BTreeMap::new();
    for packet in value["packets"].as_array().unwrap() {
        grouped
            .entry(packet["stream_index"].as_u64().unwrap())
            .or_insert_with(Vec::new)
            .push(packet.clone());
    }
    grouped
}
async fn accepted(cli: &Cli, scope: &str, key: &str, command: &str, params: Value) -> Value {
    let response = cli.mutation(scope, key, command, params).await;
    assert_eq!(response["ok"], true, "{command}: {response}");
    assert!(response["receipt_id"].is_string(), "{command}: {response}");
    response["result"].clone()
}

async fn succeeded(cli: &Cli, job: &Value) {
    let result = cli.completed(job["job_id"].as_str().unwrap()).await;
    assert_eq!(result["state"], "succeeded", "{result}");
}

async fn caption_frame(components: &Path, path: &Path, time: &str) -> Vec<u8> {
    let arguments = [
        "-v",
        "error",
        "-i",
        path.to_str().unwrap(),
        "-ss",
        time,
        "-map",
        "0:v:0",
        "-frames:v",
        "1",
        "-pix_fmt",
        "rgb24",
        "-c:v",
        "rawvideo",
        "-threads",
        "1",
        "-f",
        "rawvideo",
        "pipe:1",
    ]
    .map(str::to_owned);
    nw_platform::process::run(
        &components.join("ffmpeg"),
        &arguments,
        Duration::from_secs(30),
        320 * 160 * 3,
        CancellationToken::new(),
    )
    .await
    .unwrap()
    .stdout
}

async fn caption_burn(parent: &Path) -> Value {
    let components = std::path::PathBuf::from(
        std::env::var_os("NW_CAPTION_COMPONENTS")
            .expect("Hosted caption proof needs the same-source authenticated CPU media profile"),
    );
    let directory = parent.join("caption 'unicode [راست]:0");
    std::fs::create_dir(&directory).unwrap();
    let subtitles = directory.join("ترجمه فارسی.srt");
    std::fs::write(
        &subtitles,
        "1\n00:00:00,375 --> 00:00:01,125\nسلام دنیا و العربية!\n\n",
    )
    .unwrap();
    let cli = Cli {
        data: directory.join("state"),
        components: components.clone(),
        request_timeout: Duration::from_secs(60),
    };
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"]
        .as_str()
        .unwrap()
        .to_owned();
    let external = accepted(
        &cli,
        &scope,
        "register-external-caption",
        "assets.register",
        json!({"path":subtitles}),
    )
    .await;
    let mut cases = Vec::new();
    for transfer in ["bt709", "smpte2084", "arib-std-b67"] {
        let hdr = transfer != "bt709";
        let input = directory.join(format!("original-{transfer}.mkv"));
        let output = directory.join(format!("burned-{transfer}.mkv"));
        let (matrix, primaries, pixel_format) = if hdr {
            ("bt2020nc", "bt2020", "yuv420p10le")
        } else {
            ("bt709", "bt709", "yuv420p")
        };
        let args = [
            "-v",
            "error",
            "-filter_threads",
            "1",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=8:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=550:sample_rate=48000:duration=2",
            "-i",
            subtitles.to_str().unwrap(),
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:s",
            "-vf",
            "setsar=4/3",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            pixel_format,
            "-color_range",
            "tv",
            "-colorspace",
            matrix,
            "-color_primaries",
            primaries,
            "-color_trc",
            transfer,
            "-c:a:0",
            "pcm_s16le",
            "-c:a:1",
            "flac",
            "-c:s",
            "copy",
            input.to_str().unwrap(),
        ]
        .map(str::to_owned);
        nw_platform::process::run(
            &components.join("ffmpeg"),
            &args,
            Duration::from_secs(30),
            4096,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let registered = accepted(
            &cli,
            &scope,
            &format!("register-caption-{transfer}"),
            "assets.register",
            json!({"path":input}),
        )
        .await;
        let subtitle = if hdr {
            json!({"kind":"embedded","stream_index":3})
        } else {
            json!({"kind":"external","asset_id":external["id"]})
        };
        let color = if hdr {
            json!({"kind":"tone_map_bt709","peak_nits":1000})
        } else {
            json!({"kind":"preserve_sdr"})
        };
        let params = json!({"asset_id":registered["id"],"output":output,"video_stream":0,"subtitle":subtitle,
            "color":color,"audio":{"kind":"copy_selected","indices":[2,1]},"quality":"lossless_ffv1"});
        let review = cli.query("subtitles.burn", params.clone()).await;
        let document = directory.join(format!("burn-{transfer}.json"));
        std::fs::write(&document, serde_json::to_vec(&params).unwrap()).unwrap();
        let (typed, code) = cli
            .human(&[
                "media",
                "burn-subtitles",
                "--file",
                document.to_str().unwrap(),
            ])
            .await;
        assert_eq!(code, 0, "{typed}");
        assert_eq!(typed["result"]["hash"], review["hash"]);
        assert_eq!(
            review["subtitle_burn"]["font_sha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        let job = accepted(
            &cli,
            &scope,
            &format!("burn-caption-{transfer}"),
            "jobs.submit",
            json!({"plan_id":review["id"],"plan_hash":review["hash"]}),
        )
        .await;
        succeeded(&cli, &job).await;
        let probe = fixture::probe(&components, &output).await;
        assert_eq!(probe["streams"].as_array().unwrap().len(), 3);
        let video = &probe["streams"][0];
        assert_eq!(video["sample_aspect_ratio"], "4:3");
        assert_eq!(video["pix_fmt"], "yuv420p");
        assert_eq!(video["color_range"], "tv");
        let original_probe = fixture::probe(&components, &input).await;
        for key in ["color_primaries", "color_space", "color_transfer"] {
            if hdr {
                assert_eq!(video[key], "bt709");
            } else {
                assert_eq!(video[key], original_probe["streams"][0][key]);
            }
        }
        assert!(video.get("side_data_list").is_none());
        assert_eq!(probe["streams"][1]["codec_name"], "flac");
        assert_eq!(probe["streams"][2]["codec_name"], "pcm_s16le");
        let original_packets = packets(&components, &input).await;
        let burned_packets = packets(&components, &output).await;
        for (input_index, output_index) in [(2, 1), (1, 2)] {
            let expected: Vec<_> = original_packets[&input_index]
                .iter()
                .cloned()
                .map(|mut packet| {
                    packet["stream_index"] = json!(output_index);
                    packet
                })
                .collect();
            assert_eq!(burned_packets[&output_index], expected);
        }
        let frame = caption_frame(&components, &output, "0.500").await;
        assert_eq!(frame.len(), 96 * 64 * 3);
        let original = caption_frame(&components, &input, "0.500").await;
        assert_ne!(frame, original);
        if !hdr {
            assert_eq!(
                caption_frame(&components, &output, "0").await,
                caption_frame(&components, &input, "0").await
            );
            assert_eq!(
                caption_frame(&components, &output, "1.500").await,
                caption_frame(&components, &input, "1.500").await
            );
        }
        cases.push(
            json!({"job":job["job_id"],"source_transfer":transfer,"correct_sdr_tags":true,
            "non_square_sar_preserved":true,"independent_audio_packet_equality":true,
            "caption_picture_sha256":hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&frame))}),
        );
    }
    let black = directory.join("punctuation-reference.mkv");
    let generate = [
        "-v", "error", "-f", "lavfi", "-i", "color=black:s=320x160:r=8:d=2",
        "-c:v", "ffv1", black.to_str().unwrap(),
    ].map(str::to_owned);
    nw_platform::process::run(&components.join("ffmpeg"), &generate, Duration::from_secs(30),
        4096, CancellationToken::new()).await.unwrap();
    let picture = accepted(&cli, &scope, "register-punctuation-reference", "assets.register", json!({"path":black})).await;
    for (language, caption, mark) in [
        ("fa", "سلام این آزمون است؟", '\u{200f}'),
        ("he", "שלום זה מבחן?", '\u{200f}'),
        ("en", "Hello, this works?", '\u{200e}'),
    ] {
        let sidecar = directory.join(format!("reading-order-{language}.srt"));
        let content = format!("1\n00:00:00,000 --> 00:00:00,950\n{caption}\n\n2\n00:00:01,000 --> 00:00:01,950\n{mark}{caption}{mark}\n");
        std::fs::write(&sidecar, content).unwrap();
        let captions = accepted(&cli, &scope, &format!("register-punctuation-{language}"),
            "assets.register", json!({"path":sidecar})).await;
        let output = directory.join(format!("reading-order-{language}.mkv"));
        let params = json!({"asset_id":picture["id"],"video_stream":0,"output":output,
            "subtitle":{"kind":"external","asset_id":captions["id"]},"audio":{"kind":"none"},
            "color":{"kind":"preserve_sdr"},"quality":"lossless_ffv1"});
        let plan = cli.query("subtitles.burn", params).await;
        let job = accepted(&cli, &scope, &format!("render-punctuation-{language}"), "jobs.submit",
            json!({"plan_id":plan["id"],"plan_hash":plan["hash"]})).await;
        succeeded(&cli, &job).await;
        let plain = caption_frame(&components, &output, "0.5").await;
        let explicit = caption_frame(&components, &output, "1.5").await;
        let black_frame = caption_frame(&components, &black, "0.5").await;
        assert!(plain != black_frame && plain == explicit, "Punctuation placement differs in {language}");
        cases.push(json!({"language":language,"reading_order":"whole_event","matched_direction_mark_reference":true}));
    }
    json!(cases)
}

async fn multirange_audio_edits(parent: &Path) -> Value {
    let components = std::path::PathBuf::from(std::env::var_os("NW_CAPTION_COMPONENTS")
        .expect("Hosted sample edits need the authenticated FFmpeg tools"));
    let directory = parent.join("sample-domain-edit");
    std::fs::create_dir(&directory).unwrap();
    let raw_path = directory.join("numbered-samples.raw");
    let original = directory.join("sample-order-5.1.wav");
    let output = directory.join("ترتیب-ویرایش.wav");
    let cli = Cli { data: directory.join("state"), components: components.clone(), request_timeout: Duration::from_secs(60) };
    let samples: Vec<u8> = (0..15000).flat_map(|frame| {
        (0..6).flat_map(move |channel| {
            i16::try_from((frame * 37 + 4096 * channel) % 30000 - 15000).unwrap().to_le_bytes()
        })
    }).collect();
    std::fs::write(&raw_path, &samples).unwrap();
    let args = [
        "-v", "error", "-f", "s16le", "-ar", "48000", "-ch_layout", "5.1",
        "-i", raw_path.to_str().unwrap(), "-c:a", "pcm_s16le", original.to_str().unwrap(),
    ].map(str::to_owned);
    nw_platform::process::run(&components.join("ffmpeg"), &args, Duration::from_secs(30),
        4096, CancellationToken::new()).await.unwrap();
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"].as_str().unwrap().to_owned();
    let registered = accepted(&cli, &scope, "register-sample-order", "assets.register", json!({"path":original})).await;
    let pieces = [(120,2560,false),(5700,9801,true),(11000,11047,false)];
    let params = json!({"asset_id":registered["id"],"output":output,"stream_index":0,
        "pieces":pieces.iter().map(|(start,end,reverse)| json!({"start_sample":start,"end_sample":end,
            "reverse":reverse,"tempo_hundredths":100})).collect::<Vec<_>>()});
    let review = cli.query("audio.edit", params.clone()).await;
    assert_eq!(review["audio_edit"]["source_samples"], 15000);
    assert_eq!(review["audio_edit"]["channel_layout"], "5.1");
    let document = directory.join("audio-edit.json");
    std::fs::write(&document, serde_json::to_vec(&params).unwrap()).unwrap();
    let (checked, code) = cli.human(&["media", "edit-audio", "--file", document.to_str().unwrap()]).await;
    assert_eq!(code, 0, "{checked}");
    assert_eq!(checked["result"]["hash"], review["hash"]);
    let job = accepted(&cli, &scope, "execute-numbered-sample-edit", "jobs.submit",
        json!({"plan_id":review["id"],"plan_hash":review["hash"]})).await;
    succeeded(&cli, &job).await;
    let decoded = [
        "-v", "error", "-i", output.to_str().unwrap(), "-c:a", "pcm_s16le", "-f", "s16le", "pipe:1",
    ].map(str::to_owned);
    let output_bytes = nw_platform::process::run(&components.join("ffmpeg"), &decoded, Duration::from_secs(30),
        128 * 1024, CancellationToken::new()).await.unwrap().stdout;
    let mut expected = Vec::new();
    for (begin,end,reverse) in pieces {
        if reverse {
            for frame in (begin..end).rev() { expected.extend_from_slice(&samples[frame * 12..(frame + 1) * 12]); }
        } else {
            expected.extend_from_slice(&samples[begin * 12..end * 12]);
        }
    }
    assert_eq!(output_bytes.len(), 6588 * 12);
    assert!(output_bytes == expected, "Incorrect sample or channel order");
    let probe = fixture::probe(&components, &output).await;
    assert_eq!(probe["streams"][0]["channel_layout"], "5.1");
    assert_eq!(probe["streams"][0]["sample_rate"], "48000");
    assert_eq!(probe["streams"][0]["bits_per_sample"], 16);
    let tone = directory.join("tone-1000.wav");
    let make_tone = [
        "-v", "error", "-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=48000:duration=2",
        "-c:a", "pcm_s16le", tone.to_str().unwrap(),
    ].map(str::to_owned);
    nw_platform::process::run(&components.join("ffmpeg"), &make_tone, Duration::from_secs(30),
        4096, CancellationToken::new()).await.unwrap();
    let tone_asset = accepted(&cli, &scope, "register-tone-original", "assets.register", json!({"path":tone})).await;
    let mut tempo_facts = Vec::new();
    for percent in [10u16, 800] {
        let target = directory.join(format!("tone-{percent}.wav"));
        let params = json!({"asset_id":tone_asset["id"],"output":target,"stream_index":0,
            "pieces":[{"start_sample":0,"end_sample":96000,"reverse":true,"tempo_hundredths":percent}]});
        let reviewed = cli.query("audio.edit", params).await;
        let tempo_job = accepted(&cli, &scope, &format!("execute-tempo-{percent}"), "jobs.submit",
            json!({"plan_id":reviewed["id"],"plan_hash":reviewed["hash"]})).await;
        succeeded(&cli, &tempo_job).await;
        let decoded = [
            "-v", "error", "-i", target.to_str().unwrap(), "-c:a", "pcm_s16le", "-f", "s16le", "pipe:1",
        ].map(str::to_owned);
        let actual = nw_platform::process::run(&components.join("ffmpeg"), &decoded, Duration::from_secs(30),
            2 * 1024 * 1024, CancellationToken::new()).await.unwrap().stdout;
        let (chunks, remainder) = actual.as_chunks::<2>();
        assert!(remainder.is_empty());
        let tones: Vec<_> = chunks.iter().map(|sample| i16::from_le_bytes(*sample)).collect();
        let middle = &tones[tones.len() / 4..tones.len() * 3 / 4];
        let cycles = middle.windows(2).filter(|pair| pair[0] <= 0 && pair[1] > 0).count();
        let measured = cycles * 48000 / middle.len();
        let nominal = 96000 * 100 / usize::from(percent);
        assert!(tones.len().abs_diff(nominal) * 100 < nominal * 4);
        assert!(measured.abs_diff(1000) <= 15);
        tempo_facts.push(json!({"tempo_percent":percent,"observed_hz":measured,"output_samples":tones.len()}));
    }
    json!({"job":job["job_id"],"samples":6588,"channel_layout":"5.1","bit_exact_expected_pcm":true,"tempo":tempo_facts})
}

async fn packet_join(parent: &Path) -> Value {
    let components = std::path::PathBuf::from(std::env::var_os("NW_CAPTION_COMPONENTS")
        .expect("Hosted joins need authenticated media tools"));
    let directory = parent.join("join نگارش");
    std::fs::create_dir(&directory).unwrap();
    let cli = Cli { data: directory.join("state"), components: components.clone(), request_timeout: Duration::from_secs(60) };
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"].as_str().unwrap().to_owned();
    let mut paths = Vec::new();
    let mut assets = Vec::new();
    for (index, (color, tone, sar)) in [("red",440,1),("blue",660,1),("green",550,2)].into_iter().enumerate() {
        let input = directory.join(format!("segment-{index}.mkv"));
        let picture = format!("color=c={color}:s=96x64:r=8:d=1");
        let audio = format!("sine=frequency={tone}:sample_rate=48000:duration=1");
        let aspect = format!("setsar={sar}");
        let args = [
            "-v", "error", "-f", "lavfi", "-i", &picture, "-f", "lavfi", "-i", &audio,
            "-map", "0:v", "-map", "1:a", "-vf", &aspect, "-c:v", "ffv1", "-g", "4",
            "-c:a", "pcm_s16le", input.to_str().unwrap(),
        ].map(str::to_owned);
        nw_platform::process::run(&components.join("ffmpeg"), &args, Duration::from_secs(30),
            4096, CancellationToken::new()).await.unwrap();
        let asset = accepted(&cli, &scope, &format!("register-join-part-{index}"), "assets.register",
            json!({"path":input})).await;
        paths.push(input);
        assets.push(asset);
    }
    let destination = directory.join("joined.mkv");
    let rejected = json!({"inputs_in_order":[assets[0]["id"],assets[2]["id"]],"output":destination,
        "audio":"copy_all","metadata":"first_input","strategy":"copy"});
    let reply = cli.raw(&nw_domain::commands::Request::query("media.join", rejected)).await;
    assert_eq!(reply["error"]["code"], "JOIN_INCOMPATIBLE", "{reply}");
    assert!(!destination.exists());
    let first = packets(&components, &paths[0]).await;
    let following = packets(&components, &paths[1]).await;
    let mut runs = Vec::new();
    for audio in ["copy_all","drop"] {
        let output = directory.join(format!("joined-{audio}.mkv"));
        let request = json!({"inputs_in_order":[assets[0]["id"],assets[1]["id"]],"output":output,
            "audio":audio,"metadata":"first_input","strategy":"copy"});
        let plan = cli.query("media.join", request.clone()).await;
        assert_eq!(plan["media_join"][1]["offset"],json!({"num":"1","den":"1"}));
        let document = directory.join(format!("join-{audio}.json"));
        std::fs::write(&document, serde_json::to_vec(&request).unwrap()).unwrap();
        let (checked, code) = cli.human(&["media","join","--file",document.to_str().unwrap()]).await;
        assert_eq!(code,0,"{checked}");
        assert_eq!(checked["result"]["hash"],plan["hash"]);
        let job = accepted(&cli, &scope,&format!("publish-join-{audio}"),"jobs.submit",
            json!({"plan_id":plan["id"],"plan_hash":plan["hash"]})).await;
        succeeded(&cli, &job).await;
        let joined = packets(&components,&output).await;
        assert_eq!(joined.len(), if audio=="copy_all" {2} else {1});
        for (stream, packets) in joined {
            let mut expected = first[&stream].clone();
            expected.extend(following[&stream].iter().cloned().map(|mut packet| {
                for field in ["pts","dts"] {
                    packet[field]=json!(packet[field].as_i64().unwrap()+1000);
                }
                packet
            }));
            assert_eq!(packets,expected);
        }
        runs.push(json!({"job":job["job_id"],"audio_policy":audio,"verified_original_packets":true}));
    }
    json!({"outputs":runs,"sar_mismatch_refused":true})
}

async fn variable_picture_retime(parent: &Path) -> Value {
    let components = std::path::PathBuf::from(std::env::var_os("NW_CAPTION_COMPONENTS")
        .expect("Hosted picture retiming needs authenticated media tools"));
    let directory = parent.join("variable-picture-clock");
    std::fs::create_dir(&directory).unwrap();
    let cli = Cli { data: directory.join("state"), components: components.clone(), request_timeout: Duration::from_secs(60) };
    let scope = cli.query("access.replay_scope.create", json!({})).await["replay_scope"].as_str().unwrap().to_owned();
    let path = directory.join("vfr-original.mkv");
    let args = [
        "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=96x64:rate=1000:duration=0.520",
        "-vf", r"select=eq(n\,0)+eq(n\,32)+eq(n\,120)+eq(n\,224)+eq(n\,376)+eq(n\,512)",
        "-fps_mode", "passthrough", "-enc_time_base:v", "1/1000", "-c:v", "ffv1",
        path.to_str().unwrap(),
    ].map(str::to_owned);
    nw_platform::process::run(&components.join("ffmpeg"), &args, Duration::from_secs(30),
        4096, CancellationToken::new()).await.unwrap();
    let original = accepted(&cli, &scope,"register-vfr-original","assets.register",json!({"path":path})).await;
    let decode = |path: &Path| {
        ["-v","error","-i",path.to_str().unwrap(),"-map","0:v:0","-fps_mode","passthrough",
         "-pix_fmt","yuv420p","-c:v","rawvideo","-threads","1","-f","rawvideo","pipe:1"].map(str::to_owned)
    };
    let input = nw_platform::process::run(&components.join("ffmpeg"),&decode(&path),Duration::from_secs(30),
        64*1024,CancellationToken::new()).await.unwrap().stdout;
    let size=96*64*3/2;
    assert_eq!(input.len(),size*6);
    let mut decisions=Vec::new();
    for (tempo,reverse) in [(10, false),(100, false),(800,false),(100,true)] {
        let output = directory.join(format!("retime-{tempo}-{reverse}.mkv"));
        let range = json!({"start":{"num":"0","den":"1"},"end":{"num":"513","den":"1000"}});
        let params=json!({"asset_id":original["id"],"output":output,"range":range,
            "video_stream":0,"tempo_hundredths":tempo,"reverse":reverse,"audio":"drop"});
        let plan=cli.query("media.retime",params.clone()).await;
        let legacy_speed = format!("{}.{:02}", tempo / 100, tempo % 100);
        let legacy = format!(
            "input_path={}\noutput_path={}\noutput_format=mkv\nvideo_codec=ffv1\naudio_tracks=none\nsubtitle_tracks=none\nvideo_speed={legacy_speed}\nreverse_video={}\n",
            path.display(),
            output.display(),
            if reverse { "y" } else { "n" },
        );
        let imported = cli.query("config.import.ffmwiz", json!({
            "source":legacy,"base_directory":directory,"compatibility_profile":"ffmwiz_v1",
        })).await;
        assert_eq!(imported["input_valid"], true, "{imported}");
        let compiled = cli.query("jobs.plan", json!({"asset_id":original["id"],"job_document":imported["job"]})).await;
        assert_eq!(compiled["hash"],plan["hash"]);

        let doc=directory.join(format!("retime-{tempo}-{reverse}.json"));
        std::fs::write(&doc,serde_json::to_vec(&params).unwrap()).unwrap();
        let (typed,status)=cli.human(&["media","retime","--file",doc.to_str().unwrap()]).await;
        assert_eq!(status,0,"{typed}");
        assert_eq!(typed["result"]["hash"],plan["hash"]);
        let job=accepted(&cli, &scope,&format!("run-retime-{tempo}-{reverse}"),"jobs.submit",
            json!({"plan_id":plan["id"],"plan_hash":plan["hash"]})).await;
        succeeded(&cli, &job).await;
        let picture=packets(&components,&output).await;
        let observed:Vec<_>=picture[&0].iter().map(|packet|packet["pts"].as_i64().unwrap()).collect();
        let expected = if reverse {
            vec![0,1,137,289,393,481]
        } else {
            [0,32,120,224,376,512].map(|ticks|ticks*100/tempo).to_vec()
        };
        assert_eq!(observed,expected);
        let data = nw_platform::process::run(&components.join("ffmpeg"),&decode(&output),Duration::from_secs(30),
            64*1024,CancellationToken::new()).await.unwrap().stdout;
        assert_eq!(data.len(),input.len());
        for (index,picture) in data.chunks_exact(size).enumerate() {
            let original_index=if reverse {5-index} else {index};
            assert!(picture==&input[original_index*size..(original_index+1)*size], "Lossless frame changed");
        }
        decisions.push(json!({"job":job["job_id"],"tempo":tempo,"reverse":reverse,
            "exact_variable_clock_ms":observed,"lossless_picture_equality":true}));
    }
    json!(decisions)
}

#[tokio::test]
#[ignore = "Actual hosted FFmpeg, durable owner, typed CLI and official SDK batch journeys"]
async fn real_batches_preserve_members_retries_partial_results_and_confirmed_cancellation() {
    let media = fixture::generate().await;
    let caption_facts = caption_burn(media.directory.path()).await;
    let sample_edit_facts = multirange_audio_edits(media.directory.path()).await;
    let join_facts = packet_join(media.directory.path()).await;
    let retime_facts = variable_picture_retime(media.directory.path()).await;
    let second = media.directory.path().join("second-é.mkv");
    let args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        media.source.to_str().unwrap(),
        "-map",
        "0:0",
        "-map",
        "0:1",
        "-c",
        "copy",
        "-f",
        "matroska",
        second.to_str().unwrap(),
    ]
    .map(str::to_owned);
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &args,
        Duration::from_secs(30),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let cli = Cli {
        data: media.directory.path().join("data"),
        components: media.bin.clone(),
        request_timeout: Duration::from_secs(60),
    };
    let (scope, status) = cli
        .human(&["access", "replay-scope", "create", "--json"])
        .await;
    assert_eq!(status, 0, "{scope}");
    let scope = scope["result"]["replay_scope"].as_str().unwrap().to_owned();
    let intake = media.directory.path().join("intake-folder");
    let second_relative = "nested-é/-first-فارسی.mkv";
    std::fs::create_dir_all(intake.join("nested-é")).unwrap();
    std::fs::create_dir_all(intake.join("excluded")).unwrap();
    std::fs::copy(&media.source, intake.join("-first-فارسی.mkv")).unwrap();
    std::fs::copy(&second, intake.join(second_relative)).unwrap();
    std::fs::write(
        intake.join("broken.mkv"),
        b"Unsupported media remains visible",
    )
    .unwrap();
    std::fs::write(intake.join("notes.txt"), b"Companion byte inventory only").unwrap();
    std::fs::write(intake.join("excluded/hidden.mkv"), b"Must not be probed").unwrap();
    let folder_params = json!({"path":intake,"recursive":true,"exclude_relative":["excluded"],
        "companion_policy":"inventory","probe_timeout":{"mode":"finite","seconds":30}});
    let (folder_reply, status) = cli
        .human(&[
            "assets",
            "register-folder",
            intake.to_str().unwrap(),
            "--recursive",
            "--exclude-relative",
            "excluded",
            "--companion-policy",
            "inventory",
            "--probe-timeout-seconds",
            "30",
            "--replay-scope",
            &scope,
            "--idempotency-key",
            "folder-native",
        ])
        .await;
    assert_eq!(status, 0, "{folder_reply}");
    let owner_folder = folder_reply["result"].clone();
    assert_eq!(owner_folder["complete"], true);
    let folder_entries = owner_folder["entries"].as_array().unwrap();
    assert_eq!(folder_entries.len(), 5);
    assert_eq!(
        folder_entries
            .iter()
            .filter(|entry| entry["outcome"] == "registered")
            .count(),
        2
    );
    assert!(
        folder_entries
            .iter()
            .any(|entry| entry["relative_path"] == "broken.mkv" && entry["error"].is_object())
    );
    assert!(
        folder_entries
            .iter()
            .any(|entry| entry["relative_path"] == "excluded" && entry["outcome"] == "excluded")
    );
    assert!(
        folder_entries
            .iter()
            .any(|entry| entry["relative_path"] == "notes.txt"
                && entry["outcome"] == "companion_inventoried")
    );
    let folder_replay = cli
        .mutation(
            &scope,
            "folder-native",
            "assets.register_folder",
            folder_params.clone(),
        )
        .await;
    assert_eq!(folder_replay["receipt_id"], folder_reply["receipt_id"]);
    assert_eq!(folder_replay["result"], owner_folder);
    let inspected_folder = cli
        .human(&[
            "assets",
            "folder-inspect",
            owner_folder["id"].as_str().unwrap(),
        ])
        .await;
    assert_eq!(inspected_folder.1, 0);
    assert_eq!(inspected_folder.0["result"], owner_folder);
    let mut assets = Vec::new();
    for (ordinal, path) in [&media.source, &second].into_iter().enumerate() {
        let reply = cli
            .mutation(
                &scope,
                &format!("register-{ordinal}"),
                "assets.register",
                json!({"path":path,"probe_timeout":{"mode":"finite","seconds":30}}),
            )
            .await;
        assert_eq!(reply["ok"], true, "{reply}");
        assets.push(reply["result"].clone());
    }
    let (profile, status) = cli
        .human(&[
            "access",
            "profiles",
            "create",
            "--name",
            "batch-operator",
            "--read-root",
            media.directory.path().to_str().unwrap(),
            "--write-root",
            media.directory.path().to_str().unwrap(),
            "--json",
        ])
        .await;
    assert_eq!(status, 0, "{profile}");
    let mut command = cli.command();
    command.args(["bridge", "--access-profile", "batch-operator"]);
    let stdio = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let sdk_scope =
        call!(stdio, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
    let mut sdk_assets = Vec::new();
    for (ordinal, path) in [&media.source, &second].into_iter().enumerate() {
        sdk_assets.push(call!(
            stdio,
            "assets.register",
            json!({"path":path}),
            json!({"replay_scope":sdk_scope,"idempotency_key":format!("register-{ordinal}")})
        ));
    }
    let sdk_folder = call!(
        stdio,
        "assets.register_folder",
        folder_params,
        json!({"replay_scope":sdk_scope,"idempotency_key":"folder-sdk"})
    );
    let sdk_folder_inspected = call!(
        stdio,
        "assets.folder_inspect",
        json!({"id":sdk_folder["id"]}),
        json!({})
    );
    assert_eq!(sdk_folder_inspected, sdk_folder);
    let sdk_folder_entries = sdk_folder["entries"].as_array().unwrap();
    let native_projection = folder_entries
        .iter()
        .map(|entry| {
            json!({
                "path":entry["relative_path"],"kind":entry["kind"],"outcome":entry["outcome"],
                "sha256":entry["identity"]["sha256"],"error_code":entry["error"]["code"]
            })
        })
        .collect::<Vec<_>>();
    let sdk_projection = sdk_folder_entries
        .iter()
        .map(|entry| {
            json!({
                "path":entry["relative_path"],"kind":entry["kind"],"outcome":entry["outcome"],
                "sha256":entry["identity"]["sha256"],"error_code":entry["error"]["code"]
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(native_projection, sdk_projection);
    assert!(
        sdk_folder_entries
            .iter()
            .filter(|entry| entry["error"].is_object())
            .all(|entry| entry["error"]["message"].is_null())
    );
    let first_folder_asset = sdk_folder_entries
        .iter()
        .find(|entry| entry["relative_path"] == "-first-فارسی.mkv")
        .unwrap();
    let folder_output = media.directory.path().join("folder-copy.mkv");
    let folder_plan = call!(
        stdio,
        "streams.clean",
        json!({"asset_id":first_folder_asset["asset_id"],
        "output":folder_output,"intent":fixture::rules()}),
        json!({})
    );
    assert_eq!(folder_plan["resolution"]["action"], "copy_unchanged");
    let folder_accepted = call!(
        stdio,
        "jobs.submit",
        json!({"plan_id":folder_plan["id"],"plan_hash":folder_plan["hash"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"folder-copy-job"})
    );
    let (folder_completed, status) = cli
        .human(&[
            "jobs",
            "wait",
            folder_accepted["job_id"].as_str().unwrap(),
            "--timeout-seconds",
            "30",
        ])
        .await;
    assert_eq!(status, 0, "{folder_completed}");
    assert_eq!(folder_completed["result"]["state"], "succeeded");
    assert_eq!(
        std::fs::read(&folder_output).unwrap(),
        std::fs::read(&media.source).unwrap()
    );
    let folder_destination = media.directory.path().join("folder-batch-output");
    std::fs::create_dir(&folder_destination).unwrap();
    let rejected_intake = sdk!(
        stdio,
        "streams.clean.folder_batch_plan",
        folder_batch_input(&sdk_folder, &folder_destination),
        json!({})
    );
    assert_eq!(rejected_intake["error"]["code"], "FOLDER_INTAKE_FAILED");
    let foreign_intake = sdk!(
        stdio,
        "streams.clean.folder_batch_plan",
        folder_batch_input(&owner_folder, &folder_destination),
        json!({})
    );
    assert_eq!(foreign_intake["error"]["code"], "OBJECT_NOT_FOUND");
    let clean_folder = call!(
        stdio,
        "assets.register_folder",
        json!({"path":intake,"recursive":true,"exclude_relative":["broken.mkv","excluded"],
            "companion_policy":"inventory","probe_timeout":{"mode":"finite","seconds":30}}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"folder-batch-snapshot"})
    );
    let missing_output = folder_destination.join("missing");
    let source_child = intake.join("excluded");
    let ungranted_output = tempfile::tempdir().unwrap();
    for (path, expected) in [
        (intake.as_path(), "SOURCE_OUTPUT_ALIAS"),
        (source_child.as_path(), "SOURCE_OUTPUT_ALIAS"),
        (media.directory.path(), "SOURCE_OUTPUT_ALIAS"),
        (missing_output.as_path(), "PATH_DENIED"),
        (ungranted_output.path(), "PATH_DENIED"),
    ] {
        let result = sdk!(
            stdio,
            "streams.clean.folder_batch_plan",
            folder_batch_input(&clean_folder, path),
            json!({})
        );
        assert_eq!(result["error"]["code"], expected, "{result}");
    }
    assert!(!missing_output.exists());
    let mut bad_digest = folder_batch_input(&clean_folder, &folder_destination);
    bad_digest["snapshot_hash"] = "00".repeat(32).into();
    let mismatch = sdk!(
        stdio,
        "streams.clean.folder_batch_plan",
        bad_digest,
        json!({})
    );
    assert_eq!(mismatch["error"]["code"], "FOLDER_SNAPSHOT_MISMATCH");
    let companions = media.directory.path().join("companions");
    std::fs::create_dir(&companions).unwrap();
    for count in [1, 33] {
        for number in 0..count {
            std::fs::write(companions.join(format!("{number}.txt")), b"inventory").unwrap();
        }
        let snapshot = call!(
            stdio,
            "assets.register_folder",
            json!({"path":companions,"recursive":false,"exclude_relative":[],
                "companion_policy":"exclude"}),
            json!({"replay_scope":sdk_scope,"idempotency_key":format!("folder-bound-{count}")})
        );
        let response = sdk!(
            stdio,
            "streams.clean.folder_batch_plan",
            folder_batch_input(&snapshot, &folder_destination),
            json!({})
        );
        let expected = if count == 1 {
            "FOLDER_EMPTY"
        } else {
            "FOLDER_INTAKE_INCOMPLETE"
        };
        assert_eq!(response["error"]["code"], expected, "{response}");
    }
    let folder_input = folder_batch_input(&clean_folder, &folder_destination);
    let folder_document = media.directory.path().join("folder-batch.json");
    std::fs::write(&folder_document, serde_json::to_vec(&folder_input).unwrap()).unwrap();
    let (folder_cli, status) = cli
        .human(&[
            "--access-profile",
            "batch-operator",
            "batches",
            "plan-folder",
            "--file",
            folder_document.to_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 0, "{folder_cli}");
    let sdk_folder_batch = call!(
        stdio,
        "streams.clean.folder_batch_plan",
        folder_input.clone(),
        json!({})
    );
    assert_eq!(
        folder_cli["result"]["plan"]["hash"],
        sdk_folder_batch["plan"]["hash"]
    );
    assert_eq!(sdk_folder_batch["snapshot_id"], clean_folder["id"]);
    assert_eq!(sdk_folder_batch["snapshot_hash"], clean_folder["hash"]);
    let exclusions = sdk_folder_batch["excluded"].as_array().unwrap();
    assert_eq!(exclusions.len(), 3);
    assert!(exclusions.iter().any(|entry| {
        let index = entry["index"].as_u64().unwrap() as usize;
        clean_folder["entries"][index]["relative_path"] == "notes.txt"
            && entry["outcome"] == "companion_inventoried"
    }));
    let folder_items = sdk_folder_batch["plan"]["items"].as_array().unwrap();
    assert_eq!(folder_items.len(), 2);
    assert_ne!(
        folder_items[0]["output_name"],
        folder_items[1]["output_name"]
    );
    for item in folder_items {
        assert_eq!(item["action"], "copy_unchanged");
        let name = item["output_name"].as_str().unwrap();
        assert!(name.is_ascii() && name.len() <= 96 && name.ends_with(".mkv"));
        assert!(!name.contains(['/', '\\']));
    }
    assert_eq!(std::fs::read_dir(&folder_destination).unwrap().count(), 0);
    let retained_folder_batch = call!(
        stdio,
        "batches.plan_inspect",
        json!({"id":sdk_folder_batch["plan"]["id"]}),
        json!({})
    );
    assert_eq!(retained_folder_batch, sdk_folder_batch["plan"]);

    let mut intent = fixture::rules();
    intent.audio_selection.selector = Selector::Languages {
        values: vec!["eng".into()],
    };
    let outputs = [
        media.directory.path().join("cli-remux.mkv"),
        media.directory.path().join("cli-copy.mkv"),
    ];
    let document = media.directory.path().join("batch.json");
    std::fs::write(
        &document,
        serde_json::to_vec(&input(&assets, &outputs, &intent, "reject_all")).unwrap(),
    )
    .unwrap();
    let (review, status) = cli
        .human(&["batches", "plan", "--file", document.to_str().unwrap()])
        .await;
    assert_eq!(status, 0, "{review}");
    let plan = &review["result"];
    assert_eq!(plan["items"][0]["action"], "remux");
    assert_eq!(plan["items"][1]["action"], "copy_unchanged");
    let sdk_plan = call!(
        stdio,
        "streams.clean.batch_plan",
        input(&sdk_assets, &outputs, &intent, "reject_all"),
        json!({})
    );
    assert_eq!(
        plan["hash"], sdk_plan["hash"],
        "Equivalent execution retains one hash across registration, creator and completed probe audit"
    );
    let submission = [
        "batches",
        "submit",
        "--plan",
        plan["id"].as_str().unwrap(),
        "--plan-hash",
        plan["hash"].as_str().unwrap(),
        "--replay-scope",
        &scope,
        "--idempotency-key",
        "first-batch",
    ];
    let (accepted, status) = cli.human(&submission).await;
    assert_eq!(status, 0, "{accepted}");
    let (replayed, status) = cli.human(&submission).await;
    assert_eq!(status, 0, "{replayed}");
    assert_eq!(accepted["receipt_id"], replayed["receipt_id"]);
    assert_eq!(accepted["result"], replayed["result"]);
    let batch_id = accepted["result"]["batch_id"].as_str().unwrap();
    let (completed, status) = cli.human(&["batches", "wait", batch_id]).await;
    assert_eq!(status, 0, "{completed}");
    assert_eq!(completed["result"]["finished"], 2);
    assert_eq!(completed["result"]["succeeded"], 2);
    let child = cli
        .completed(completed["result"]["items"][0]["job_id"].as_str().unwrap())
        .await;
    assert_eq!(child["state"], "succeeded");
    assert_eq!(
        child["artifact"]["bytes"],
        completed["result"]["items"][0]["output_bytes"]
    );

    assert_eq!(completed["result"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&outputs[1]).unwrap()
    );
    let observed = fixture::probe(&media.bin, &outputs[0]).await;
    assert_eq!(observed["streams"].as_array().unwrap().len(), 2);
    assert_eq!(observed["streams"][1]["tags"]["language"], "eng");
    let original_packets = packets(&media.bin, &media.source).await;
    let retained_packets = packets(&media.bin, &outputs[0]).await;
    assert_eq!(original_packets.get(&0), retained_packets.get(&0));
    assert_eq!(original_packets.get(&1), retained_packets.get(&1));
    assert!(!retained_packets.contains_key(&2));

    let reordered = media.directory.path().join("reordered.mkv");
    let remap = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        media.source.to_str().unwrap(),
        "-map",
        "0:1",
        "-map",
        "0:0",
        "-map",
        "0:2",
        "-c",
        "copy",
        "-f",
        "matroska",
        reordered.to_str().unwrap(),
    ]
    .map(str::to_owned);
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &remap,
        Duration::from_secs(30),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let moved = cli
        .mutation(
            &scope,
            "register-reordered",
            "assets.register",
            json!({"path":reordered}),
        )
        .await;
    assert_eq!(moved["ok"], true, "{moved}");
    let remove_outputs = [
        media.directory.path().join("remove-first-audio-1.mkv"),
        media.directory.path().join("remove-first-audio-2.mkv"),
    ];
    let mut remove = fixture::rules();
    remove.audio_selection.selector = Selector::ExcludeTypeOrdinals { values: vec![0] };
    remove.audio_selection.no_match = nw_domain::mux::NoMatch::Reject;
    let review = cli
        .query(
            "streams.clean.batch_plan",
            input(
                &[assets[0].clone(), moved["result"].clone()],
                &remove_outputs,
                &remove,
                "reject_all",
            ),
        )
        .await;
    assert!(
        review["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["action"] == "remux")
    );
    let submitted = cli
        .mutation(
            &scope,
            "remove-first-audio",
            "batches.submit",
            json!({"plan_id":review["id"],"plan_hash":review["hash"]}),
        )
        .await;
    assert_eq!(submitted["ok"], true, "{submitted}");
    let batch = submitted["result"]["batch_id"].as_str().unwrap();
    let (removed, status) = cli.human(&["batches", "wait", batch]).await;
    assert_eq!(status, 0, "{removed}");
    assert_eq!(removed["result"]["succeeded"], 2);
    let expected_audio: Vec<_> = original_packets[&2]
        .iter()
        .map(|packet| &packet["data_hash"])
        .collect();
    for path in remove_outputs {
        let probe = fixture::probe(&media.bin, &path).await;
        assert_eq!(probe["streams"].as_array().unwrap().len(), 2);
        assert_eq!(probe["streams"][1]["tags"]["language"], "fas");
        let captured = packets(&media.bin, &path).await;
        assert_eq!(
            captured[&1]
                .iter()
                .map(|packet| &packet["data_hash"])
                .collect::<Vec<_>>(),
            expected_audio
        );
    }

    // An operator cannot borrow the native owner's retry namespace.
    let denied = sdk!(
        stdio,
        "batches.submit",
        json!({"plan_id":sdk_plan["id"],"plan_hash":sdk_plan["hash"]}),
        json!({"replay_scope":scope,"idempotency_key":"first-batch"})
    );
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["error"]["code"], "REPLAY_SCOPE_INVALID");

    let sdk_outputs = [
        media.directory.path().join("sdk-remux.mkv"),
        media.directory.path().join("sdk-copy.mkv"),
    ];
    let sdk_plan = call!(
        stdio,
        "streams.clean.batch_plan",
        input(&sdk_assets, &sdk_outputs, &intent, "reject_all"),
        json!({})
    );
    let submit_params = json!({"plan_id":sdk_plan["id"],"plan_hash":sdk_plan["hash"]});
    let submit_context = json!({"replay_scope":sdk_scope,"idempotency_key":"sdk-batch"});
    let accepted_sdk = sdk!(
        stdio,
        "batches.submit",
        submit_params.clone(),
        submit_context.clone()
    );
    assert_eq!(accepted_sdk["ok"], true, "{accepted_sdk}");
    let retry_sdk = sdk!(stdio, "batches.submit", submit_params, submit_context);
    assert_eq!(accepted_sdk["receipt_id"], retry_sdk["receipt_id"]);
    assert_eq!(accepted_sdk["result"], retry_sdk["result"]);
    let completed_sdk = finished!(stdio, accepted_sdk["result"]["batch_id"]);
    assert_eq!(completed_sdk["succeeded"], 2);
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&sdk_outputs[1]).unwrap()
    );

    // A reviewed rejected candidate is retained and requires explicit continuation.
    let partial_outputs = [
        media.directory.path().join("partial.mkv"),
        media.directory.path().join("unsupported.mp4"),
    ];
    let rejected = cli
        .query(
            "streams.clean.batch_plan",
            input(&assets, &partial_outputs, &intent, "reject_all"),
        )
        .await;
    assert!(
        rejected["items"][1]["error"]["code"].is_string(),
        "{rejected}"
    );
    let refusal = cli
        .mutation(
            &scope,
            "rejected-batch",
            "batches.submit",
            json!({"plan_id":rejected["id"],"plan_hash":rejected["hash"]}),
        )
        .await;
    assert_eq!(refusal["ok"], false);
    assert_eq!(refusal["error"]["code"], "BATCH_REJECTED");
    let partial = cli
        .query(
            "streams.clean.batch_plan",
            input(&assets, &partial_outputs, &intent, "continue_with_rejected"),
        )
        .await;
    let accepted_partial = cli
        .mutation(
            &scope,
            "partial-batch",
            "batches.submit",
            json!({"plan_id":partial["id"],"plan_hash":partial["hash"]}),
        )
        .await;
    assert_eq!(accepted_partial["ok"], true, "{accepted_partial}");
    let (partial_result, status) = cli
        .human(&[
            "batches",
            "wait",
            accepted_partial["result"]["batch_id"].as_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 9, "{partial_result}");
    assert_eq!(partial_result["error"]["code"], "PARTIAL_BATCH_FAILURE");
    assert_eq!(partial_result["result"]["finished"], 2);
    assert_eq!(partial_result["result"]["succeeded"], 1);
    assert_eq!(partial_result["result"]["failed"], 1);
    assert_eq!(
        partial_result["result"]["successful_video_input_bytes"],
        std::fs::metadata(&media.source).unwrap().len().to_string()
    );
    assert!(partial_result["result"]["items"][1]["job_id"].is_null());
    assert!(!partial_outputs[1].exists());

    let folder_asset = |relative| {
        folder_entries
            .iter()
            .find(|entry| entry["relative_path"] == relative)
            .unwrap_or_else(|| panic!("Folder snapshot is missing {relative}"))["asset_id"]
            .clone()
    };
    let extract_outputs = [
        media.directory.path().join("extracted-video.mkv"),
        media.directory.path().join("extracted-audio.mkv"),
        media.directory.path().join("missing-audio.mkv"),
    ];
    let extract_file = media.directory.path().join("extract-batch.json");
    let first_id = folder_asset("-first-فارسی.mkv");
    let second_id = folder_asset(second_relative);
    let extraction_input = json!({
        "items":[
            {"target":{"asset_id":first_id,"output":extract_outputs[0]},"selector":{"kind":"video","ordinal":1}},
            {"target":{"asset_id":first_id,"output":extract_outputs[1]},"selector":{"kind":"audio","ordinal":2}},
            {"target":{"asset_id":second_id,"output":extract_outputs[2]},"selector":{"kind":"audio","ordinal":2}}
        ],
        "failure_policy":"continue_with_rejected"
    });
    std::fs::write(
        &extract_file,
        serde_json::to_vec(&extraction_input).unwrap(),
    )
    .unwrap();
    let (extraction, status) = cli
        .human(&[
            "streams",
            "extract-batch",
            "--file",
            extract_file.to_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 0, "{extraction}");
    let extraction = &extraction["result"];
    assert_eq!(extraction["items"][0]["extraction"]["source_index"], 0);
    assert_eq!(extraction["items"][1]["extraction"]["source_index"], 2);
    assert_eq!(extraction["items"][2]["error"]["code"], "STREAM_NOT_FOUND");
    assert!(extraction["items"][2]["extraction"]["source_index"].is_null());
    let extract_submit = cli
        .mutation(
            &scope,
            "extract-folder",
            "batches.submit",
            json!({"plan_id":extraction["id"],"plan_hash":extraction["hash"]}),
        )
        .await;
    assert_eq!(extract_submit["ok"], true, "{extract_submit}");
    let (extract_result, status) = cli
        .human(&[
            "batches",
            "wait",
            extract_submit["result"]["batch_id"].as_str().unwrap(),
        ])
        .await;
    assert_eq!(status, 9, "{extract_result}");
    assert_eq!(extract_result["result"]["succeeded"], 2);
    assert_eq!(extract_result["result"]["failed"], 1);
    assert!(!extract_outputs[2].exists());
    let source_packets = packets(&media.bin, &media.source).await;
    for (output, original) in extract_outputs.iter().zip([0_u64, 2]) {
        let copied = packets(&media.bin, output).await;
        assert_eq!(copied.len(), 1);
        let mut expected = source_packets[&original].clone();
        for packet in &mut expected {
            packet["stream_index"] = 0.into();
        }
        assert_eq!(copied[&0], expected);
    }
    let sdk_extraction_input = json!({
        "items":[
            {"target":{"asset_id":sdk_assets[0]["id"],"output":media.directory.path().join("sdk-extracted-audio.mkv")},
             "selector":{"kind":"audio","ordinal":2}},
            {"target":{"asset_id":sdk_assets[1]["id"],"output":media.directory.path().join("sdk-missing-audio.mkv")},
             "selector":{"kind":"audio","ordinal":2}}
        ],
        "failure_policy":"reject_all"
    });
    let sdk_extraction = call!(
        stdio,
        "streams.extract.batch_plan",
        sdk_extraction_input.clone(),
        json!({})
    );
    assert_eq!(sdk_extraction["items"][0]["extraction"]["source_index"], 2);
    assert_eq!(
        sdk_extraction["items"][1]["error"]["code"],
        "STREAM_NOT_FOUND"
    );
    let denied_extract = sdk!(
        stdio,
        "batches.submit",
        json!({"plan_id":sdk_extraction["id"],"plan_hash":sdk_extraction["hash"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"extract-reject-all"})
    );
    assert_eq!(denied_extract["error"]["code"], "BATCH_REJECTED");

    let mut skip_intent = intent.clone();
    skip_intent.audio_selection.selector = Selector::Languages {
        values: vec!["zzz".into()],
    };
    let skip_outputs = [
        media.directory.path().join("skip-a.mkv"),
        media.directory.path().join("skip-b.mkv"),
    ];
    let skip = call!(
        stdio,
        "streams.clean.batch_plan",
        input(&sdk_assets, &skip_outputs, &skip_intent, "reject_all"),
        json!({})
    );
    assert!(
        skip["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["action"] == "skip")
    );
    let skip = call!(
        stdio,
        "batches.submit",
        json!({"plan_id":skip["id"],"plan_hash":skip["hash"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"skip-batch"})
    );
    let skipped = finished!(stdio, skip["batch_id"]);
    assert_eq!(skipped["skipped"], 2);
    assert_eq!(skipped["succeeded"], 0);
    assert_eq!(skipped["successful_video_input_bytes"], "0");
    assert_eq!(skipped["successful_video_output_bytes"], "0");
    assert!(skip_outputs.iter().all(|path| !path.exists()));

    // Real publication coordination holds the first worker before the fence.
    // The second remains queued under the same serial utility scheduler.
    let cancel_outputs = [
        media.directory.path().join("cancel-a.mkv"),
        media.directory.path().join("cancel-b.mkv"),
    ];
    let stamp =
        nw_platform::files::DirectoryStamp::capture_resolved(media.directory.path()).unwrap();
    let claims = nw_platform::claims::DestinationClaims::try_acquire(vec![
        nw_platform::claims::Destination::reviewed(
            &cancel_outputs[0],
            &stamp.object_identity().unwrap(),
        )
        .unwrap(),
    ])
    .unwrap();
    let cancel_plan = call!(
        stdio,
        "streams.clean.batch_plan",
        input(
            &sdk_assets,
            &cancel_outputs,
            &fixture::rules(),
            "reject_all"
        ),
        json!({})
    );
    let cancel_batch = call!(
        stdio,
        "batches.submit",
        json!({"plan_id":cancel_plan["id"],"plan_hash":cancel_plan["hash"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"cancel-batch"})
    );
    let cancellation = call!(
        stdio,
        "batches.cancel",
        json!({"id":cancel_batch["batch_id"]}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"cancel-members"})
    );
    assert!(
        cancellation["members"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["cancel_requested"] == true)
    );
    let cancelled = finished!(stdio, cancel_batch["batch_id"]);
    assert_eq!(cancelled["cancelled"], 2, "{cancelled}");
    assert_eq!(cancelled["succeeded"], 0);
    assert!(cancel_outputs.iter().all(|path| !path.exists()));
    assert!(
        cancelled["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["artifact_id"].is_null())
    );
    drop(claims);
    let page = call!(stdio, "batches.list", json!({"limit":1}), json!({}));
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_string(), "{page}");
    let next = call!(
        stdio,
        "batches.list",
        json!({"limit":1,"cursor":page["next_cursor"]}),
        json!({})
    );
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);

    let bind = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = bind.local_addr().unwrap().port();
    drop(bind);
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/owner-evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let token_file = media.directory.path().join("auth/batch-http-token");
    let server_log = evidence.join("batch-http-controller-stderr.log");
    let mut server = cli
        .command()
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--access-profile",
            "batch-operator",
            "--token-file",
        ])
        .arg(&token_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&server_log).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let startup = std::time::Instant::now();
    while !token_file.exists() {
        assert!(
            server.try_wait().unwrap().is_none(),
            "HTTP owner exited before credential export"
        );
        assert!(
            startup.elapsed() < Duration::from_secs(60),
            "HTTP owner startup exceeded bound"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let token =
        String::from_utf8(nw_platform::security::read_private(&token_file, 256).unwrap()).unwrap();
    let transport = StreamableHttpClientTransport::with_client(
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(&token),
    );
    let http = ().serve(transport).await.unwrap();
    let http_scope =
        call!(http, "access.replay_scope.create", json!({}), json!({}))["replay_scope"]
            .as_str()
            .unwrap()
            .to_owned();
    let http_outputs = [
        media.directory.path().join("http-remux.mkv"),
        media.directory.path().join("http-copy.mkv"),
    ];
    let http_plan = call!(
        http,
        "streams.clean.batch_plan",
        input(&sdk_assets, &http_outputs, &intent, "reject_all"),
        json!({})
    );
    let http_batch = call!(
        http,
        "batches.submit",
        json!({"plan_id":http_plan["id"],"plan_hash":http_plan["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"http-batch"})
    );
    let http_result = finished!(http, http_batch["batch_id"]);
    assert_eq!(http_result["succeeded"], 2);
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&http_outputs[1]).unwrap()
    );
    let http_extraction = call!(
        http,
        "streams.extract.batch_plan",
        sdk_extraction_input,
        json!({})
    );
    assert_eq!(http_extraction["hash"], sdk_extraction["hash"]);
    // The retained snapshot stays authoritative when the live directory gains another file.
    std::fs::copy(&media.source, intake.join("new-after-snapshot.mkv")).unwrap();
    let http_folder_batch = call!(
        http,
        "streams.clean.folder_batch_plan",
        folder_input.clone(),
        json!({})
    );
    assert_eq!(
        http_folder_batch["plan"]["hash"],
        sdk_folder_batch["plan"]["hash"]
    );
    assert_eq!(http_folder_batch["excluded"], sdk_folder_batch["excluded"]);
    let folder_group = call!(
        http,
        "batches.submit",
        json!({"plan_id":http_folder_batch["plan"]["id"],"plan_hash":http_folder_batch["plan"]["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"folder-batch"})
    );
    let folder_group_result = finished!(http, folder_group["batch_id"]);
    assert_eq!(folder_group_result["succeeded"], 2);
    for (item, source) in folder_items.iter().zip([&media.source, &second]) {
        let output = folder_destination.join(item["output_name"].as_str().unwrap());
        assert_eq!(
            std::fs::read(output).unwrap(),
            std::fs::read(source).unwrap()
        );
    }
    let destination_entries = || {
        std::fs::read_dir(&folder_destination)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                (entry.file_name().into_string().unwrap(), kind)
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let published = destination_entries();
    let output_names: std::collections::BTreeSet<_> = folder_items
        .iter()
        .map(|item| item["output_name"].as_str().unwrap())
        .collect();
    assert_eq!(published.len(), output_names.len() * 2);
    for (name, kind) in &published {
        if output_names.contains(name.as_str()) {
            assert!(kind.is_file(), "Published output must be a regular file");
        } else {
            assert!(kind.is_dir());
            assert!(
                name.strip_prefix(".numera-job-")
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
                "Unexpected output-directory entry {name}"
            );
        }
    }
    let destination_collision = call!(
        http,
        "streams.clean.folder_batch_plan",
        folder_input,
        json!({})
    );
    assert!(
        destination_collision["plan"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["error"]["code"] == "OUTPUT_COLLISION" && item["plan"].is_null())
    );
    assert_eq!(destination_entries(), published);

    let append_audio = media.directory.path().join("append-external-audio.mkv");
    std::fs::copy(&media.source, &append_audio).unwrap();
    let append_captions = media.directory.path().join("زیرنویس-append.srt");
    std::fs::write(
        &append_captions,
        "1\n00:00:00,125 --> 00:00:00,950\nنمونهٔ فارسی\n",
    )
    .unwrap();
    let append_translation = media.directory.path().join("append-translation.srt");
    std::fs::write(
        &append_translation,
        "1\n00:00:00,250 --> 00:00:00,875\nEnglish translation\n",
    )
    .unwrap();
    let mut added_assets = Vec::new();
    for (ordinal, file) in [&append_audio, &append_captions, &append_translation]
        .into_iter()
        .enumerate()
    {
        added_assets.push(call!(
            stdio,
            "assets.register",
            json!({"path":file}),
            json!({"replay_scope":sdk_scope,"idempotency_key":format!("append-file-{ordinal}")})
        ));
    }
    let appended = media.directory.path().join("append-verified.mkv");
    let append_request = json!({"target":sdk_assets[0]["id"],"output":appended,"conversion":"forbid",
    "additions":[
        {"asset_id":added_assets[0]["id"],"selector":{"kind":"explicit","tracks":[
            {"stream_index":1,"language":"eng","title":"External one","disposition":[]},
            {"stream_index":2,"language":"fas","title":"صدای دوم","disposition":["comment"]}]}},
        {"asset_id":added_assets[1]["id"],"selector":{"kind":"explicit","tracks":[
            {"stream_index":0,"language":"fas","title":"زیرنویس","disposition":[]}]}},
        {"asset_id":added_assets[2]["id"],"selector":{"kind":"explicit","tracks":[
            {"stream_index":0,"language":"eng","title":"English","disposition":["forced"]}]}}
    ]});
    let append_document = media.directory.path().join("append-tracks.json");
    std::fs::write(
        &append_document,
        serde_json::to_vec(&append_request).unwrap(),
    )
    .unwrap();
    let (cli_append, code) = cli
        .human(&[
            "--access-profile",
            "batch-operator",
            "streams",
            "add",
            "--file",
            append_document.to_str().unwrap(),
        ])
        .await;
    assert_eq!(code, 0, "{cli_append}");
    let stdio_append = call!(stdio, "streams.add", append_request.clone(), json!({}));
    let http_append = call!(http, "streams.add", append_request.clone(), json!({}));
    assert_eq!(cli_append["result"]["hash"], stdio_append["hash"]);
    assert_eq!(http_append["hash"], stdio_append["hash"]);
    assert_eq!(http_append["added_tracks"], stdio_append["added_tracks"]);
    assert_eq!(http_append["added_tracks"][0]["first_output_index"], 3);
    assert_eq!(http_append["added_tracks"][1]["first_output_index"], 5);
    assert_eq!(http_append["added_tracks"][2]["first_output_index"], 6);
    let mut forbidden = append_request.clone();
    forbidden["additions"][0]["selector"]["tracks"][0]["stream_index"] = json!(0);
    let refused = sdk!(http, "streams.add", forbidden, json!({}));
    assert_eq!(refused["error"]["code"], "INVALID_TRACK_ADDITION");
    let unqualified = media.directory.path().join("append-refused.mp4");
    let mut forbidden_container = append_request;
    forbidden_container["output"] = json!(unqualified);
    let refused = sdk!(http, "streams.add", forbidden_container, json!({}));
    assert_eq!(refused["error"]["code"], "INCOMPATIBLE_PRESERVATION");
    assert!(!unqualified.exists());
    assert!(!appended.exists());
    let append_job = call!(
        http,
        "jobs.submit",
        json!({"plan_id":http_append["id"],"plan_hash":http_append["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"append-verified"})
    );
    cli.completed(append_job["job_id"].as_str().unwrap()).await;
    let appended_probe = fixture::probe(&media.bin, &appended).await;
    assert_eq!(appended_probe["streams"].as_array().unwrap().len(), 7);
    assert_eq!(
        appended_probe["streams"][3]["tags"]["title"],
        "External one"
    );
    assert_eq!(appended_probe["streams"][4]["tags"]["title"], "صدای دوم");
    assert_eq!(appended_probe["streams"][4]["disposition"]["comment"], 1);
    assert_eq!(appended_probe["streams"][5]["tags"]["language"], "fas");
    assert_eq!(appended_probe["streams"][5]["tags"]["title"], "زیرنویس");
    assert_eq!(appended_probe["streams"][6]["tags"]["title"], "English");
    assert_eq!(appended_probe["streams"][6]["disposition"]["forced"], 1);
    let appended_packets = packets(&media.bin, &appended).await;
    let subrip = packets(&media.bin, &append_captions).await;
    let translation = packets(&media.bin, &append_translation).await;
    for (output, (original, index)) in [
        (&original_packets, 0u64),
        (&original_packets, 1),
        (&original_packets, 2),
        (&original_packets, 1),
        (&original_packets, 2),
        (&subrip, 0),
        (&translation, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut expected = original[&index].clone();
        for packet in &mut expected {
            packet["stream_index"] = json!(output);
        }
        assert_eq!(appended_packets[&(output as u64)], expected);
    }

    let gop_source = media.directory.path().join("cut-keyframe-grid.mkv");
    let mut gop_args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=96x64:rate=8:duration=2",
        "-c:v",
        "ffv1",
        "-g",
        "4",
        "-f",
        "matroska",
    ]
    .map(str::to_owned)
    .to_vec();
    gop_args.push(gop_source.to_str().unwrap().into());
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &gop_args,
        Duration::from_secs(30),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let cut_source = call!(
        stdio,
        "assets.register",
        json!({"path":gop_source}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"register-cut-grid"})
    );
    let cut_output = media.directory.path().join("verified-packet-cut.mkv");
    let cut = json!({"asset_id":cut_source["id"], "output":cut_output, "chapters":"drop",
        "range":{"start":{"num":"1","den":"2"},"end":{"num":"3","den":"2"}}});
    let mut mid_gop = cut.clone();
    mid_gop["range"]["start"] = json!({"num":"3","den":"4"});
    let rejected_cut = sdk!(http, "media.cut", mid_gop, json!({}));
    assert_eq!(rejected_cut["error"]["code"], "COPY_BOUNDARY_UNQUALIFIED");
    assert!(!cut_output.exists());
    let (typed_cut, code) = cli
        .human(&[
            "--access-profile",
            "batch-operator",
            "media",
            "cut",
            "--asset",
            cut_source["id"].as_str().unwrap(),
            "--start",
            "1/2",
            "--end",
            "3/2",
            "--out",
            cut_output.to_str().unwrap(),
            "--drop-chapters",
        ])
        .await;
    assert_eq!(code, 0, "{typed_cut}");
    let stdio_cut = call!(stdio, "media.cut", cut.clone(), json!({}));
    let http_cut = call!(http, "media.cut", cut.clone(), json!({}));
    assert_eq!(http_cut["packet_cut"], cut["range"]);
    assert_eq!(typed_cut["result"]["hash"], stdio_cut["hash"]);
    assert_eq!(stdio_cut["hash"], http_cut["hash"]);
    let cut_job = call!(
        http,
        "jobs.submit",
        json!({"plan_id":http_cut["id"],"plan_hash":http_cut["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"keyframe-copy-cut"})
    );
    cli.completed(cut_job["job_id"].as_str().unwrap()).await;
    let source_packets = packets(&media.bin, &gop_source).await;
    let cut_packets = packets(&media.bin, &cut_output).await;
    let expected_cut: Vec<_> = source_packets[&0]
        .iter()
        .filter(|packet| (500..1500).contains(&packet["pts"].as_i64().unwrap()))
        .cloned()
        .collect();
    assert_eq!(expected_cut.len(), 8);
    assert_eq!(cut_packets[&0], expected_cut);

    let split_outputs = [
        media.directory.path().join("split-اول.mkv"),
        media.directory.path().join("split-دوم.mkv"),
    ];
    let split = json!({"asset_id":cut_source["id"],"chapters":"drop","outputs":[
        {"output":split_outputs[0],"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}}},
        {"output":split_outputs[1],"range":{"start":{"num":"1","den":"1"},"end":{"num":"2","den":"1"}}}
    ]});
    let mut rejected_split = split.clone();
    rejected_split["outputs"][1]["range"]["start"] = json!({"num":"5","den":"4"});
    let rejected = call!(http, "media.split_plan", rejected_split, json!({}));
    assert_eq!(
        rejected["items"][1]["error"]["code"],
        "COPY_BOUNDARY_UNQUALIFIED"
    );
    assert_eq!(
        sdk!(
            http,
            "batches.submit",
            json!({"plan_id":rejected["id"],"plan_hash":rejected["hash"]}),
            json!({"replay_scope":http_scope,"idempotency_key":"reject-unqualified-packet-split"})
        )["error"]["code"],
        "BATCH_REJECTED",
    );
    assert!(split_outputs.iter().all(|path| !path.exists()));
    let split_document = media.directory.path().join("packet-split.json");
    std::fs::write(&split_document, serde_json::to_vec(&split).unwrap()).unwrap();
    let (typed_split, code) = cli
        .human(&[
            "--access-profile",
            "batch-operator",
            "media",
            "split",
            "--file",
            split_document.to_str().unwrap(),
        ])
        .await;
    assert_eq!(code, 0, "{typed_split}");
    let stdio_split = call!(stdio, "media.split_plan", split.clone(), json!({}));
    let http_split = call!(http, "media.split_plan", split.clone(), json!({}));
    assert_eq!(typed_split["result"]["hash"], stdio_split["hash"]);
    assert_eq!(stdio_split["hash"], http_split["hash"]);
    for index in 0..split_outputs.len() {
        assert_eq!(
            http_split["items"][index]["packet_cut"],
            split["outputs"][index]["range"]
        );
    }
    let split_batch = call!(
        http,
        "batches.submit",
        json!({"plan_id":http_split["id"],"plan_hash":http_split["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"verified-original-clock-split"})
    );
    let split_result = finished!(http, split_batch["batch_id"]);
    assert_eq!(split_result["succeeded"], 2, "{split_result}");
    for (index, path) in split_outputs.iter().enumerate() {
        let start = i64::try_from(index).unwrap() * 1000;
        let expected: Vec<_> = source_packets[&0]
            .iter()
            .filter(|packet| (start..start + 1000).contains(&packet["pts"].as_i64().unwrap()))
            .cloned()
            .collect();
        assert_eq!(expected.len(), 8);
        assert_eq!(packets(&media.bin, path).await[&0], expected);
    }

    let replacement_output = media.directory.path().join("track-replacement.mkv");
    let replacement = json!({"target":sdk_assets[0]["id"],"output":replacement_output,"conversion":"forbid",
    "metadata":"strip_global","remove":[{"kind":"type_ordinal","track_kind":"audio","ordinal":1}],
    "additions":[{"asset_id":added_assets[0]["id"],"selector":{"kind":"explicit","tracks":[
        {"stream_index":1,"title":"Alternate mix","language":"fas","disposition":[]}
    ]}}]});
    let replacement_document = media.directory.path().join("track-replacement.json");
    std::fs::write(
        &replacement_document,
        serde_json::to_vec(&replacement).unwrap(),
    )
    .unwrap();
    let (typed_replace, code) = cli
        .human(&[
            "--access-profile",
            "batch-operator",
            "streams",
            "replace",
            "--file",
            replacement_document.to_str().unwrap(),
        ])
        .await;
    assert_eq!(code, 0, "{typed_replace}");
    let stdio_replace = call!(stdio, "streams.replace", replacement.clone(), json!({}));
    let http_replace = call!(http, "streams.replace", replacement, json!({}));
    assert_eq!(typed_replace["result"]["hash"], stdio_replace["hash"]);
    assert_eq!(stdio_replace["hash"], http_replace["hash"]);
    assert!(http_replace["resolution"]["outcomes"][2]["output_index"].is_null());
    assert_eq!(http_replace["added_tracks"][0]["first_output_index"], 2);
    let replace_job = call!(
        http,
        "jobs.submit",
        json!({"plan_id":http_replace["id"],"plan_hash":http_replace["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"verified-track-replacement"})
    );
    cli.completed(replace_job["job_id"].as_str().unwrap()).await;
    let replacement_probe = fixture::probe(&media.bin, &replacement_output).await;
    assert!(replacement_probe["format"]["tags"]["title"].is_null());
    assert_eq!(
        replacement_probe["streams"][2]["tags"]["title"],
        "Alternate mix"
    );
    assert_eq!(replacement_probe["streams"][2]["tags"]["language"], "fas");
    let replacement_packets = packets(&media.bin, &replacement_output).await;
    assert_eq!(replacement_packets[&0], original_packets[&0]);
    assert_eq!(replacement_packets[&1], original_packets[&1]);
    let mut expected_replacement = original_packets[&1].clone();
    for packet in &mut expected_replacement {
        packet["stream_index"] = json!(2);
    }
    assert_eq!(replacement_packets[&2], expected_replacement);

    let strip_output = media.directory.path().join("metadata-only.mkv");
    let strip = call!(
        http,
        "streams.replace",
        json!({"target":sdk_assets[0]["id"],
        "output":strip_output,"conversion":"forbid","metadata":"strip_global"}),
        json!({})
    );
    assert_eq!(strip["resolution"]["action"], "remux");
    let strip_job = call!(
        http,
        "jobs.submit",
        json!({"plan_id":strip["id"],"plan_hash":strip["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"verified-metadata-only"})
    );
    cli.completed(strip_job["job_id"].as_str().unwrap()).await;
    assert!(fixture::probe(&media.bin, &strip_output).await["format"]["tags"]["title"].is_null());
    assert_eq!(packets(&media.bin, &strip_output).await, original_packets);

    let normalize_source = media.directory.path().join("different-loudness.mkv");
    let mut sampled = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x48:rate=4:duration=5",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=5",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=950:sample_rate=48000:duration=5",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=650:sample_rate=48000:duration=5",
        "-filter_complex",
        "[2:a]volume=0.1[quiet]",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-map",
        "[quiet]",
        "-map",
        "3:a",
        "-c:v",
        "ffv1",
        "-c:a",
        "pcm_s16le",
        "-f",
        "matroska",
    ]
    .map(str::to_owned)
    .to_vec();
    sampled.push(normalize_source.to_str().unwrap().into());
    nw_platform::process::run(
        &media.bin.join("ffmpeg"),
        &sampled,
        Duration::from_secs(30),
        4096,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let audio = call!(
        stdio,
        "assets.register",
        json!({"path":normalize_source}),
        json!({"replay_scope":sdk_scope,"idempotency_key":"distinct-normalization-tracks"})
    );
    let normalize_output = media.directory.path().join("normalized-independently.mkv");
    let normalize = json!({"asset_id":audio["id"],"output":normalize_output,
        "tracks":[{"stream_index":1},{"stream_index":2}]});
    let normalize_document = media.directory.path().join("normalize.json");
    std::fs::write(&normalize_document, serde_json::to_vec(&normalize).unwrap()).unwrap();
    let (typed_normalize, code) = cli
        .human(&[
            "--access-profile",
            "batch-operator",
            "media",
            "normalize",
            "--file",
            normalize_document.to_str().unwrap(),
        ])
        .await;
    assert_eq!(code, 0, "{typed_normalize}");
    let stdio_normalize = call!(stdio, "audio.normalize", normalize.clone(), json!({}));
    let http_normalize = call!(http, "audio.normalize", normalize, json!({}));
    assert_eq!(typed_normalize["result"]["hash"], stdio_normalize["hash"]);
    assert_eq!(stdio_normalize["hash"], http_normalize["hash"]);
    let measurements = http_normalize["audio_normalization"].as_array().unwrap();
    assert_eq!(measurements.len(), 2);
    let first_lufs = measurements[0]["measurement"]["integrated_centi_lufs"]
        .as_i64()
        .unwrap();
    let second_lufs = measurements[1]["measurement"]["integrated_centi_lufs"]
        .as_i64()
        .unwrap();
    assert!((first_lufs - second_lufs).abs() > 1500);
    let normalized_job = call!(
        http,
        "jobs.submit",
        json!({"plan_id":http_normalize["id"],"plan_hash":http_normalize["hash"]}),
        json!({"replay_scope":http_scope,"idempotency_key":"verified-independent-normalization"})
    );
    cli.completed(normalized_job["job_id"].as_str().unwrap())
        .await;
    let input_packets = packets(&media.bin, &normalize_source).await;
    let output_packets = packets(&media.bin, &normalize_output).await;
    assert_eq!(input_packets[&0], output_packets[&0]);
    assert_eq!(input_packets[&3], output_packets[&3]);
    let mut independently_measured = Vec::new();
    for index in [1, 2] {
        let bytes = |packets: &[Value]| {
            packets
                .iter()
                .map(|packet| packet["size"].as_str().unwrap().parse::<u64>().unwrap())
                .sum::<u64>()
        };
        assert_eq!(
            bytes(&input_packets[&index]),
            bytes(&output_packets[&index])
        );
        let args = [
            "-hide_banner".into(),
            "-nostdin".into(),
            "-nostats".into(),
            "-v".into(),
            "info".into(),
            "-i".into(),
            normalize_output.to_str().unwrap().into(),
            "-map".into(),
            format!("0:{index}"),
            "-af".into(),
            "loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ];
        let result = nw_platform::process::run(
            &media.bin.join("ffmpeg"),
            &args,
            Duration::from_secs(20),
            4096,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let start = result.stderr_tail.rfind("\n{").unwrap() + 1;
        let end = result.stderr_tail.rfind("\n}").unwrap() + 2;
        let stats: Value = serde_json::from_str(&result.stderr_tail[start..end]).unwrap();
        let lufs = stats["input_i"].as_str().unwrap().parse::<f64>().unwrap();
        let peak = stats["input_tp"].as_str().unwrap().parse::<f64>().unwrap();
        assert!((lufs + 16.0).abs() <= 0.4 && peak <= -1.5);
        independently_measured
            .push(json!({"stream":index,"integrated_lufs":lufs,"true_peak_dbtp":peak}));
    }
    http.cancel().await.unwrap();
    server.start_kill().unwrap();
    server.wait().await.unwrap();

    let mut command = cli.command();
    command.arg("bridge");
    let agent = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let denied = sdk!(agent, "batches.list", json!({}), json!({}));
    assert_eq!(denied["error"]["code"], "PERMISSION_DENIED");
    agent.cancel().await.unwrap();
    stdio.cancel().await.unwrap();
    let report = json!({"source_commit":option_env!("NW_SOURCE_SHA").unwrap_or(env!("GITHUB_SHA")),"case":"two_source_cleanup_batch",
        "transports":["typed_cli","official_sdk_stdio","official_sdk_streamable_http"],
        "cli_batch":batch_id,"sdk_batch":completed_sdk["id"],"http_batch":http_result["id"],
        "independent_retained_packet_equality":true,"copy_full_byte_equality":true,
        "partial_exit":9,"confirmed_cancelled":cancelled["cancelled"],"skipped":skipped["skipped"],
        "stream_extraction":{"batch":extract_submit["result"]["batch_id"],"published":2,"missing_selector":1,
            "independent_packet_equality":true,"matching_stdio_and_http_review":true},
        "folder_intake":{"native_cli":owner_folder["id"],"official_sdk_stdio":sdk_folder["id"],
            "same_normalized_outcomes":true,"retained_probe_failures":1,"registered_media":2,
            "companion_inventory_only":true,"jobs_wait_cli":true,"verified_copy_job":folder_accepted["job_id"]},
        "folder_batch":{"batch":folder_group["batch_id"],"published":2,"match_cli_stdio_http":true,
            "intentional_exclusions":3,"no_replace":true,"copy_full_byte_equality":true},
        "track_append":{"job":append_job["job_id"],"kept_original_streams":3,"added_audio":2,"added_subtitles":2,
            "original_packet_timing_and_digest_match":true,"matching_plan_across_cli_stdio_http":true},
        "packet_cut":{"job":cut_job["job_id"],"original_clock_packets":8,"non_keyframe_start_refused":true},
        "packet_split":{"batch":split_batch["batch_id"],"verified_parts":2,"original_clock_packets":16,"rejected_member_blocks_batch":true},
        "caption_burn":caption_facts,
        "sample_edit":sample_edit_facts,
        "packet_join":join_facts,
        "picture_retime":retime_facts,
        "track_replacement":{"job":replace_job["job_id"],"metadata_only_job":strip_job["job_id"],"relative_track_replacement_verified":true},
        "normalization":{"job":normalized_job["job_id"],"output":independently_measured,
            "unchanged_picture_and_unselected_audio_packets":true,"original_pcm_sample_counts":true},
        "scope":"Retained video-folder cleanup to a disjoint destination; companion copying, output trees, mixed audio/encode workflows, replacement and GUI batching remain separate qualification"});
    std::fs::write(
        evidence.join("batch-media-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NW_BATCH_MEDIA_REPORT={report}");
    lost_ack::verify(&media, Some(&second)).await;
}
