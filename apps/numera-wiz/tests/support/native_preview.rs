//! Read actual bounded native picture and sound through the private owner HTTP route.
use super::Cli;
use bytes::Bytes;
use nw_domain::{
    preview::native_wire::{self, Descriptor, Header, Opened, PacketOrder},
    time::Rational,
};
use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

struct Body {
    response: Response,
    remaining: Bytes,
}

impl Body {
    async fn take(&mut self, count: usize, allow_end: bool) -> Option<Vec<u8>> {
        let mut bytes = Vec::with_capacity(count);
        while bytes.len() < count {
            if self.remaining.is_empty() {
                let Some(next) = self.response.chunk().await.unwrap() else {
                    assert!(allow_end && bytes.is_empty(), "Truncated native response");
                    return None;
                };
                self.remaining = next;
            }
            let take = (count - bytes.len()).min(self.remaining.len());
            bytes.extend_from_slice(&self.remaining.split_to(take));
        }
        Some(bytes)
    }

    async fn header<T: DeserializeOwned>(&mut self) -> Option<T> {
        let prefix = self.take(4, true).await?.try_into().unwrap();
        let length = native_wire::header_length(prefix).unwrap();
        let body = self.take(length, false).await.unwrap();
        Some(native_wire::decode_header(prefix, &body).unwrap())
    }

    async fn packet(&mut self, order: &mut PacketOrder) -> Option<(Header, Vec<u8>)> {
        let header: Header = self.header().await?;
        let length = order.admit(&header).unwrap();
        let bytes = self.take(length, false).await.unwrap();
        Some((header, bytes))
    }
}

pub async fn exercise(cli: &Cli, project: &str, sequence: &str) {
    let owner: Value = serde_json::from_slice(&std::fs::read(cli.data.join("runtime/owner.json")).unwrap()).unwrap();
    let bearer = owner["bearer"].as_str().unwrap();
    let port = owner["port"].as_u64().unwrap();
    let route = format!("http://127.0.0.1:{port}/internal/native-previews");
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let input = json!({
        "project_id":project,"sequence_id":sequence,"revision":"4","width":160,"height":90,
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
        "audio":true,
    });
    let denied = client.post(&route).json(&input).send().await.unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let opened: Opened = client.post(&route).bearer_auth(bearer).json(&input).send()
        .await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert_eq!(opened.picture.project_id.as_str(), project);
    assert_eq!(opened.picture.sequence_id.as_str(), sequence);
    assert_eq!(opened.picture.revision.to_string(), "4");
    let sound = opened.sound.as_ref().expect("Requested stereo preview");
    assert_eq!(opened.picture.session_id, sound.session_id);
    assert_eq!(opened.picture.generation, sound.generation);
    let session = format!("{route}/{}", opened.picture.session_id);
    let clock = format!("{session}/clock");
    let stale = client.put(&clock).bearer_auth(bearer)
        .json(&json!({"generation":opened.picture.generation + 1,"at":Rational::ZERO}))
        .send().await.unwrap();
    assert_eq!(stale.status(), StatusCode::BAD_REQUEST);
    let fetch = |suffix| {
        client.get(format!("{session}/{suffix}")).bearer_auth(bearer).send()
    };
    let (picture, audio) = tokio::join!(fetch("picture"), fetch("sound"));
    let body = |response: reqwest::Result<Response>| {
        let response = response.unwrap().error_for_status().unwrap();
        assert_eq!(response.headers()["Content-Type"], "application/octet-stream");
        assert_eq!(response.headers()["Cache-Control"], "no-store");
        Body { response, remaining: Bytes::new() }
    };
    let mut picture = body(picture);
    let mut audio = body(audio);
    assert_eq!(picture.header::<Descriptor>().await.unwrap(), opened.picture);
    assert_eq!(&audio.header::<Descriptor>().await.unwrap(), sound);
    let mut picture_order = PacketOrder::new(opened.picture.clone()).unwrap();
    let mut audio_order = PacketOrder::new(sound.clone()).unwrap();
    let read_picture = async {
        let mut frames = 0u64;
        while let Some((header, bytes)) = picture.packet(&mut picture_order).await {
            assert_eq!(header.sequence, frames);
            assert_eq!(header.pts, Rational::new(frames as i64, 4).unwrap());
            if frames == 0 {
                let center = 4 * (45 * 160 + 80);
                assert_eq!(&bytes[center..center + 4], &[51, 102, 204, 255]);
            }
            frames += 1;
        }
        frames
    };
    let read_audio = async {
        let mut samples = 0i64;
        let mut peak: f32 = 0.0;
        while let Some((header, bytes)) = audio.packet(&mut audio_order).await {
            assert_eq!(header.pts, Rational::new(samples, 48_000).unwrap());
            for value in bytes.as_chunks::<4>().0 {
                peak = peak.max(f32::from_le_bytes(*value).abs());
            }
            samples += (bytes.len() / 8) as i64;
            let response = client.put(&clock).bearer_auth(bearer)
                .json(&json!({"generation":header.generation,"at":header.pts}))
                .send().await.unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
        (samples, peak)
    };
    let (frames, (samples, peak)) = tokio::join!(read_picture, read_audio);
    assert_eq!((frames, samples), (4, 48_000));
    assert!(peak > 0.06);
    let closed = client.delete(&session).bearer_auth(bearer).send().await.unwrap();
    assert_eq!(closed.status(), StatusCode::NO_CONTENT);
}
