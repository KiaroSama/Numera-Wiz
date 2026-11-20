//! Qualify the real producer against a deliberately paced presentation clock.
use nw_domain::{
    preview::native_wire::{self, Descriptor, Format, Header, PacketOrder},
    projects::{EntityId, ProjectId, Revision},
    time::{Rational, TimeRange},
};
use nw_engine::{Engine, NativePreviewPipe, NativePreviewProfile};
use nw_platform::process::{self, AsyncChunkSink, StreamLimits};
use std::{path::PathBuf, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const GENERATION: u64 = 11;
const WIDTH: u32 = 160;
const HEIGHT: u32 = 90;

fn t(num: i64, den: i64) -> Rational {
    Rational::new(num, den).unwrap()
}

fn profile(length: i64, audio: bool) -> NativePreviewProfile {
    NativePreviewProfile {
        generation: GENERATION,
        range: TimeRange {
            start: t(3, 1),
            end: t(3 + length, 1),
        },
        frame_rate: t(24, 1),
        width: WIDTH,
        height: HEIGHT,
        audio,
    }
}

fn producer(
    engine: &Engine,
    length: i64,
    audio: bool,
    sink: Box<dyn AsyncChunkSink>,
    cancel: CancellationToken,
) -> JoinHandle<nw_domain::Result<process::RunOutput>> {
    let ffmpeg = PathBuf::from(&engine.ffmpeg.path);
    let mut args = [
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-xerror",
        "-f",
        "lavfi",
        "-i",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(format!(
        "testsrc2=size={WIDTH}x{HEIGHT}:rate=24:duration={length}"
    ));
    if audio {
        args.extend(["-f", "lavfi", "-i"].map(str::to_owned));
        args.push(format!("sine=f=650:r=48000:d={length}"));
    }
    args.extend(
        [
            "-map",
            "0:v:0",
            "-c:v",
            "rawvideo",
            "-threads:v",
            "1",
            "-pix_fmt",
            "rgba",
            "-fps_mode:v",
            "passthrough",
            "-enc_time_base:v",
            "filter",
        ]
        .map(str::to_owned),
    );
    if audio {
        args.extend(
            [
                "-map",
                "1:a:0",
                "-c:a",
                "pcm_f32le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-bsf:a",
                "pcm_rechunk=n=480:p=0",
            ]
            .map(str::to_owned),
        );
    }
    args.extend(
        [
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-avoid_negative_ts",
            "disabled",
            "-f",
            "nut",
            "-write_index",
            "0",
            "-syncpoints",
            "default",
            "pipe:1",
        ]
        .map(str::to_owned),
    );
    tokio::spawn(async move {
        process::stream_with_sink(
            &ffmpeg,
            &args,
            Duration::from_secs(15),
            StreamLimits {
                stdout_bytes: 16 * 1024 * 1024,
                stderr_bytes: 4096,
            },
            sink,
            cancel,
        )
        .await
    })
}

pub async fn exercise(engine: &Engine, token: &CancellationToken) {
    let selected = profile(8, false);
    let (mut pipe, sink) = NativePreviewPipe::start(selected, token).unwrap();
    let producing = producer(engine, 8, false, sink, pipe.cancellation());
    let first = tokio::time::timeout(Duration::from_secs(4), pipe.video.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.sequence, 0);
    assert_eq!(first.pts, selected.range.start);
    assert_eq!(first.duration, t(1, 24));
    assert_eq!(first.bytes.len(), (WIDTH * HEIGHT * 4) as usize);
    let id = "55215fa4-840f-41ee-b794-cb5143fa0f0c";
    let format = Format::Rgba8 {
        width: WIDTH,
        height: HEIGHT,
        frame_rate: selected.frame_rate,
    };
    let descriptor = Descriptor {
        version: 1,
        session_id: EntityId::parse(id).unwrap(),
        generation: selected.generation,
        project_id: ProjectId::parse(id).unwrap(),
        sequence_id: EntityId::parse(id).unwrap(),
        revision: Revision(41),
        range: selected.range,
        format,
    };
    let wire = native_wire::encode_header(&descriptor).unwrap();
    let header = wire[..4].try_into().unwrap();
    let decoded: Descriptor = native_wire::decode_header(header, &wire[4..]).unwrap();
    assert_eq!(decoded, descriptor);
    assert!(native_wire::header_length((8193u32).to_be_bytes()).is_err());
    assert!(native_wire::decode_header::<Descriptor>(header, &wire[4..wire.len() - 1]).is_err());

    let mut order = PacketOrder::new(decoded).unwrap();
    let packet_header = first.wire_header(format).unwrap();
    let mut stale = packet_header.clone();
    stale.generation -= 1;
    stale.payload_bytes = u32::MAX;
    assert!(order.admit(&stale).is_err());
    let wire = native_wire::encode_header(&packet_header).unwrap();
    let decoded: Header =
        native_wire::decode_header(wire[..4].try_into().unwrap(), &wire[4..]).unwrap();
    assert_eq!(decoded, packet_header);
    assert_eq!(order.admit(&decoded).unwrap(), first.bytes.len());
    assert!(order.admit(&decoded).is_err());
    assert!(pipe.advance(GENERATION - 1, t(4, 1)).is_err());
    assert!(pipe.advance(GENERATION, t(2, 1)).is_err());

    let frozen = 7u64;
    for sequence in 1..frozen {
        let packet = tokio::time::timeout(Duration::from_secs(2), pipe.video.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(packet.sequence, sequence);
        assert_eq!(
            order.admit(&packet.wire_header(format).unwrap()).unwrap(),
            packet.bytes.len()
        );
        assert!(packet.pts <= t(13, 4), "Future frame escaped frozen clock");
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(90), pipe.video.recv())
            .await
            .is_err()
    );
    assert!(
        !producing.is_finished(),
        "Paused clock must backpressure the active producer"
    );
    // Stir the virtual clock a frame at a time, retaining every future frame.
    let mut count = frozen;
    pipe.advance(
        GENERATION,
        t(3, 1).checked_add(t(count as i64, 24)).unwrap(),
    )
    .unwrap();
    while let Some(packet) = tokio::time::timeout(Duration::from_secs(2), pipe.video.recv())
        .await
        .unwrap()
    {
        assert_eq!(packet.generation, GENERATION);
        assert_eq!(packet.sequence, count);
        assert_eq!(
            packet.pts,
            t(3, 1).checked_add(t(count as i64, 24)).unwrap()
        );
        pipe.advance(GENERATION, packet.pts).unwrap();
        count += 1;
    }
    assert_eq!(count, 192);
    assert!(pipe.audio.recv().await.is_none());
    producing.await.unwrap().unwrap();
    let stats = pipe.settled().await.unwrap();
    assert_eq!(stats.video_frames, 192);
    assert_eq!(stats.late_video_frames, 0);
    assert_eq!(stats.audio_packets, 0);
    assert!(stats.wire_bytes > WIDTH as u64 * HEIGHT as u64 * 4 * 192);

    let (mut pipe, sink) = NativePreviewPipe::start(profile(2, true), token).unwrap();
    let producing = producer(engine, 2, true, sink, pipe.cancellation());
    let mut video = 0;
    let mut samples = 0;
    let mut peak: f32 = 0.0;
    let mut ended = [false; 2];
    while ended != [true; 2] {
        let next = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                frame = pipe.video.recv(), if !ended[0] => (0, frame),
                frame = pipe.audio.recv(), if !ended[1] => (1, frame),
            }
        })
        .await
        .unwrap();
        match next {
            (0, Some(packet)) => {
                assert_eq!(packet.pts, t(3, 1).checked_add(t(video, 24)).unwrap());
                video += 1;
            }
            (1, Some(packet)) => {
                assert_eq!(packet.pts, t(3, 1).checked_add(t(samples, 48_000)).unwrap());
                let (values, remainder) = packet.bytes.as_chunks::<4>();
                assert!(remainder.is_empty());
                for value in values {
                    let value = f32::from_le_bytes(*value);
                    assert!(value.is_finite());
                    peak = peak.max(value.abs());
                }
                samples += (values.len() / 2) as i64;
                pipe.advance(GENERATION, packet.pts).unwrap();
            }
            (index, None) => ended[index] = true,
            _ => unreachable!(),
        }
    }
    producing.await.unwrap().unwrap();
    let stats = pipe.settled().await.unwrap();
    assert_eq!((video, samples), (48, 96_000));
    assert_eq!(
        (
            stats.video_frames,
            stats.audio_packets,
            stats.audio_sample_frames
        ),
        (48, 200, 96_000)
    );
    assert!(peak > 0.065);
    assert_eq!(stats.late_video_frames, 0);

    let (pipe, sink) = NativePreviewPipe::start(profile(8, false), token).unwrap();
    let producing = producer(engine, 8, false, sink, pipe.cancellation());
    let failure = tokio::time::timeout(Duration::from_secs(4), pipe.settled())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.code, "PREVIEW_STALLED");
    assert!(producing.await.unwrap().is_err());

    let (mut pipe, sink) = NativePreviewPipe::start(profile(8, false), token).unwrap();
    let producing = producer(engine, 8, false, sink, pipe.cancellation());
    assert!(
        tokio::time::timeout(Duration::from_secs(4), pipe.video.recv())
            .await
            .unwrap()
            .is_some()
    );
    pipe.cancellation().cancel();
    assert_eq!(pipe.settled().await.unwrap_err().code, "CANCELLED");
    assert_eq!(producing.await.unwrap().unwrap_err().code, "CANCELLED");
}
