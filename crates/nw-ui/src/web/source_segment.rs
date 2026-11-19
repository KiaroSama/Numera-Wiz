//! Append authenticated, bounded MP4 units to the browser's media decoder.
use super::{Deadline, js_error};
use js_sys::{Promise, Reflect, Uint8Array};
use nw_domain::preview::{
    MAX_PROGRAM_PLAYBACK_BYTES, MAX_SOURCE_SEGMENT_BYTES, MAX_SOURCE_SEGMENT_FRAGMENTS,
    ProgramPlaybackRequest, SegmentBytes, SegmentPlayback,
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, AbortSignal, Event, EventTarget, HtmlVideoElement, MediaSource,
    ReadableStreamDefaultReader, Request, RequestCredentials, RequestInit, RequestMode, RequestRedirect, Response, SourceBuffer, Url,
};

struct Listener {
    target: EventTarget,
    name: &'static str,
    closure: Closure<dyn FnMut(Event)>,
}

impl Listener {
    fn new(
        target: &EventTarget,
        name: &'static str,
        handler: impl FnMut(Event) + 'static,
    ) -> Result<Self, JsValue> {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>);
        target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
        Ok(Self {
            target: target.clone(),
            name,
            closure,
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.closure.as_ref().unchecked_ref());
    }
}

struct Playback {
    video: HtmlVideoElement,
    panel: web_sys::Element,
    object_url: String,
    _close: Listener,
    _visibility: Listener,
}

impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.video.pause();
        self.video.remove_attribute("src").ok();
        self.video.load();
        self.panel.remove();
        let _ = Url::revoke_object_url(&self.object_url);
    }
}

struct Session {
    id: Rc<()>,
    controller: AbortController,
    playback: Option<Playback>,
}

enum PreviewRequest {
    Segment(SegmentPlayback),
    Program(ProgramPlaybackRequest),
}

enum PlaybackBytes {
    Segment { bytes: Vec<u8>, manifest: SegmentPlayback },
    Program { reader: ReadableStreamDefaultReader, duration: f64 },
}

impl Drop for Session {
    fn drop(&mut self) {
        self.controller.abort();
        drop(self.playback.take());
    }
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

pub(crate) fn supported() -> bool {
    Reflect::get(&js_sys::global(), &JsValue::from_str("MediaSource"))
        .is_ok_and(|source| source.is_function())
}

pub(super) fn close() {
    drop(SESSION.with(|active| active.take()));
}

fn is_active(id: &Rc<()>) -> bool {
    SESSION.with(|active| {
        active
            .borrow()
            .as_ref()
            .is_some_and(|session| Rc::ptr_eq(&session.id, id))
    })
}

fn defer_close(id: Rc<()>) {
    // The listener must return before dropping the DOM and its wasm callbacks.
    wasm_bindgen_futures::spawn_local(async move {
        if is_active(&id) {
            close();
        }
    });
}

async fn transition(
    target: &EventTarget,
    ready_event: &'static str,
    failure_event: &'static str,
    signal: &AbortSignal,
    operate: impl FnOnce() -> Result<(), JsValue>,
) -> Result<(), JsValue> {
    if signal.aborted() {
        return Err(JsValue::from_str("Browser preview was closed"));
    }
    let mut callbacks = None;
    let promise = Promise::new(&mut |resolve, reject| callbacks = Some((resolve, reject)));
    let (resolve, reject) = callbacks.expect("Promise executor runs immediately");
    let _ready = Listener::new(target, ready_event, move |_| {
        let _ = resolve.call0(&JsValue::UNDEFINED);
    })?;
    let failure = reject.clone();
    let _error = Listener::new(target, failure_event, move |_| {
        let _ = failure.call1(
            &JsValue::UNDEFINED,
            &JsValue::from_str("Browser media decoding failed"),
        );
    })?;
    let _abort = Listener::new(signal.unchecked_ref(), "abort", move |_| {
        let _ = reject.call1(
            &JsValue::UNDEFINED,
            &JsValue::from_str("Browser preview was closed"),
        );
    })?;
    operate()?;
    JsFuture::from(promise).await?;
    Ok(())
}

async fn fetch_segment(
    window: &web_sys::Window,
    origin: &str,
    bearer: &str,
    manifest: &SegmentPlayback,
    signal: &AbortSignal,
) -> Result<Vec<u8>, JsValue> {
    let expected = manifest.bytes as usize;
    if window.location().origin()? != origin
        || !(1..=MAX_SOURCE_SEGMENT_BYTES).contains(&expected)
        || manifest.segment_id.len() != 36
        || !manifest
            .segment_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(JsValue::from_str("Invalid browser preview manifest"));
    }
    let options = RequestInit::new();
    options.set_method("GET");
    options.set_mode(RequestMode::SameOrigin);
    options.set_redirect(RequestRedirect::Error);
    options.set_signal(Some(signal));
    let request = Request::new_with_str_and_init(
        &format!("{origin}/api/v1/previews/{}/mp4", manifest.segment_id),
        &options,
    )?;
    request
        .headers()
        .set("Authorization", &format!("Bearer {bearer}"))?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request))
        .await?
        .dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(
            "Preview bytes are unavailable or the grant expired",
        ));
    }
    if response.headers().get("Content-Type")?.as_deref() != Some("video/mp4")
        || response
            .headers()
            .get("Content-Length")?
            .and_then(|value| value.parse().ok())
            != Some(manifest.bytes)
    {
        return Err(JsValue::from_str(
            "Preview MIME or length differs from its manifest",
        ));
    }
    let reader: ReadableStreamDefaultReader = response
        .body()
        .ok_or_else(|| JsValue::from_str("Browser preview body is missing"))?
        .get_reader()
        .dyn_into()
        .map_err(JsValue::from)?;
    let mut bytes = Vec::with_capacity(expected);
    loop {
        let chunk = JsFuture::from(reader.read()).await?;
        if Reflect::get(&chunk, &JsValue::from_str("done"))?.as_bool() == Some(true) {
            break;
        }
        let data: Uint8Array = Reflect::get(&chunk, &JsValue::from_str("value"))?.dyn_into()?;
        if data.length() as usize > expected - bytes.len() {
            return Err(JsValue::from_str(
                "Browser preview exceeds its accepted byte count",
            ));
        }
        let offset = bytes.len();
        bytes.resize(offset + data.length() as usize, 0);
        data.copy_to(&mut bytes[offset..]);
    }
    if bytes.len() != expected || nw_domain::projects::hash(&bytes) != manifest.sha256 {
        return Err(JsValue::from_str(
            "Preview bytes differ from their accepted digest",
        ));
    }
    Ok(bytes)
}

async fn fetch_program(
    window: &web_sys::Window, origin: &str, bearer: &str,
    input: ProgramPlaybackRequest, signal: &AbortSignal,
) -> Result<(String, PlaybackBytes), JsValue> {
    if window.location().origin()? != origin {
        return Err(JsValue::from_str("Foreign program playback origin"));
    }
    let duration = input.selection.range.end.checked_sub(input.selection.range.start)
        .map_err(|error| JsValue::from_str(&error.message))?;
    let duration = duration.numerator() as f64 / duration.denominator() as f64;
    let data = serde_json::to_vec(&input).map_err(|_| JsValue::from_str("Could not serialize the program range"))?;
    let body = Uint8Array::from(data.as_slice());
    let options = RequestInit::new();
    options.set_method("POST");
    options.set_mode(RequestMode::SameOrigin);
    options.set_redirect(RequestRedirect::Error);
    options.set_credentials(RequestCredentials::Omit);
    options.set_signal(Some(signal));
    options.set_body(body.as_ref());
    let request = Request::new_with_str_and_init(&format!("{origin}/api/v1/previews/program"), &options)?;
    request.headers().set("Authorization", &format!("Bearer {bearer}"))?;
    request.headers().set("Content-Type", "application/json")?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request)).await?.dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!("Program playback returned HTTP {}", response.status())));
    }
    let mime = response.headers().get("Content-Type")?
        .filter(|mime| mime.starts_with("video/mp4; codecs=\"avc1.") && mime.ends_with(", mp4a.40.2\"") && mime.len() < 100)
        .ok_or_else(|| JsValue::from_str("The owner did not return the admitted program media profile"))?;
    let reader = response.body().ok_or_else(|| JsValue::from_str("Program media stream is missing"))?
        .get_reader().dyn_into().map_err(JsValue::from)?;
    Ok((mime, PlaybackBytes::Program { reader, duration }))
}

async fn stream_program(
    reader: ReadableStreamDefaultReader, video: &HtmlVideoElement, buffer: &SourceBuffer,
    signal: &AbortSignal, ready: impl FnOnce(),
) -> Result<(), JsValue> {
    let mut received = 0u64;
    let mut ready = Some(ready);
    loop {
        let position = video.current_time();
        let ranges = buffer.buffered()?;
        if ranges.length() > 0 && position - ranges.start(0)? > 12.0 {
            transition(buffer.unchecked_ref(), "updateend", "error", signal,
                || buffer.remove(0.0, position - 8.0)).await?;
        }
        if ranges.length() > 0 && ranges.end(ranges.length() - 1)? - position >= 4.0 {
            transition(video.unchecked_ref(), "timeupdate", "error", signal, || Ok(())).await?;
            continue;
        }
        let chunk = JsFuture::from(reader.read()).await?;
        if Reflect::get(&chunk, &JsValue::from_str("done"))?.as_bool() == Some(true) {
            break;
        }
        let data = Reflect::get(&chunk, &JsValue::from_str("value"))?.dyn_into::<Uint8Array>()?;
        received += u64::from(data.length());
        if received > MAX_PROGRAM_PLAYBACK_BYTES || data.length() as usize > MAX_SOURCE_SEGMENT_BYTES {
            return Err(JsValue::from_str("Program playback exceeded the browser byte budget"));
        }
        let mut bytes = data.to_vec();
        transition(buffer.unchecked_ref(), "updateend", "error", signal,
            || buffer.append_buffer_with_u8_array(&mut bytes)).await?;
        if video.ready_state() >= 2 && let Some(ready) = ready.take() { ready(); }
    }
    Ok(())
}

async fn play(
    origin: &str,
    bearer: &str,
    source: PreviewRequest,
    label: &'static str,
    controller: AbortController,
    id: Rc<()>,
) -> Result<Playback, JsValue> {
    let window =
        web_sys::window().ok_or_else(|| JsValue::from_str("Browser window is unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("Browser document is unavailable"))?;
    if !supported() {
        return Err(JsValue::from_str(
            "This browser does not admit the selected AVC and AAC preview profile",
        ));
    }
    if document.hidden() || !is_active(&id) {
        return Err(JsValue::from_str("Browser preview was closed"));
    }
    let panel = document.create_element("section")?;
    panel.set_attribute("aria-label", label)?;
    panel.set_attribute("style", "position:fixed;right:20px;bottom:20px;z-index:1000;background:#202020;color:#eee;padding:12px;border:1px solid #555;max-width:calc(100vw - 40px)")?;
    let video: HtmlVideoElement = document.create_element("video")?.dyn_into()?;
    video.set_attribute(
        "style",
        "display:block;width:320px;max-width:100%;background:#000",
    )?;
    video.set_attribute("playsinline", "")?;
    video.set_controls(true);
    panel.append_child(&video)?;
    let close_button = document.create_element("button")?;
    close_button.set_text_content(Some("Close preview"));
    panel.append_child(&close_button)?;
    let status = document.create_element("p")?;
    status.set_text_content(Some("Preparing playback…"));
    panel.append_child(&status)?;
    let signal = controller.signal();
    let stop = controller.clone();
    let callback = Closure::new(move || stop.abort());
    let timer = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        25000,
    )?;
    let mut deadline = Deadline {
        window: window.clone(),
        timer,
        controller: controller.clone(),
        completed: false,
        _callback: callback,
    };
    let media = MediaSource::new()?;
    let close_listener = {
        let id = id.clone();
        Listener::new(close_button.unchecked_ref(), "click", move |_| {
            defer_close(id.clone())
        })?
    };
    let visibility = {
        let document = document.clone();
        let target: EventTarget = document.clone().unchecked_into();
        Listener::new(&target, "visibilitychange", move |_| {
            if document.hidden() {
                defer_close(id.clone());
            }
        })?
    };
    let object_url = Url::create_object_url_with_source(&media)?;
    let playback = Playback {
        video: video.clone(),
        panel: panel.clone(),
        object_url,
        _close: close_listener,
        _visibility: visibility,
    };
    document
        .body()
        .ok_or_else(|| JsValue::from_str("Browser body is unavailable"))?
        .append_child(&panel)?;
    let (mime, delivery) = match source {
        PreviewRequest::Segment(manifest) => {
            if !(1..=MAX_SOURCE_SEGMENT_FRAGMENTS).contains(&manifest.fragments.len()) {
                return Err(JsValue::from_str("Invalid MP4 fragment count"));
            }
            let bytes = fetch_segment(&window, origin, bearer, &manifest, &signal).await?;
            (manifest.mime_type.clone(), PlaybackBytes::Segment { bytes, manifest })
        }
        PreviewRequest::Program(request) => fetch_program(&window, origin, bearer, request, &signal).await?,
    };
    if !MediaSource::is_type_supported(&mime) {
        return Err(JsValue::from_str("This browser does not admit the selected AVC and AAC preview profile"));
    }
    transition(
        media.unchecked_ref(),
        "sourceopen",
        "sourceclose",
        &signal,
        || {
            video.set_src(&playback.object_url);
            Ok(())
        },
    )
    .await?;
    let buffer = media.add_source_buffer(&mime)?;
    let (mut bytes, manifest) = match delivery {
        PlaybackBytes::Segment { bytes, manifest } => (bytes, manifest),
        PlaybackBytes::Program { reader, duration } => {
            Reflect::set(media.as_ref(), &JsValue::from_str("duration"), &JsValue::from_f64(duration))?;
            stream_program(reader, &video, &buffer, &signal, || {
                deadline.complete();
                status.remove();
            }).await?;
            media.end_of_stream()?;
            deadline.complete();
            status.remove();
            return Ok(playback);
        }
    };
    let mut expected = 0;
    for SegmentBytes { offset, length } in
        std::iter::once(manifest.initialization).chain(manifest.fragments)
    {
        let end = offset
            .checked_add(length)
            .ok_or_else(|| JsValue::from_str("Invalid MP4 unit bounds"))?;
        if offset != expected || length == 0 {
            return Err(JsValue::from_str("Invalid MP4 unit order"));
        }
        let part = bytes
            .get_mut(offset as usize..end as usize)
            .ok_or_else(|| JsValue::from_str("MP4 unit exceeds the captured bytes"))?;
        transition(
            buffer.unchecked_ref(),
            "updateend",
            "error",
            &signal,
            || buffer.append_buffer_with_u8_array(part),
        )
        .await?;
        expected = end;
    }
    if expected != manifest.bytes {
        return Err(JsValue::from_str("MP4 units leave unaccounted bytes"));
    }
    media.end_of_stream()?;
    status.remove();
    deadline.complete();
    Ok(playback)
}

pub(super) fn open(origin: &str, bearer: &str, manifest: SegmentPlayback, label: &'static str) {
    start(origin, bearer, PreviewRequest::Segment(manifest), label);
}

pub(super) fn open_program(origin: &str, bearer: &str, request: ProgramPlaybackRequest) {
    start(origin, bearer, PreviewRequest::Program(request), "Program playback");
}

fn start(origin: &str, bearer: &str, source: PreviewRequest, label: &'static str) {
    let Ok(controller) = AbortController::new() else {
        return;
    };
    let id = Rc::new(());
    let previous = SESSION.with(|active| {
        active.replace(Some(Session {
            id: id.clone(),
            controller: controller.clone(),
            playback: None,
        }))
    });
    drop(previous);
    let (origin, bearer) = (origin.to_owned(), bearer.to_owned());
    wasm_bindgen_futures::spawn_local(async move {
        let result = play(&origin, &bearer, source, label, controller, id.clone()).await;
        if !is_active(&id) {
            return;
        }
        match result {
            Ok(playback) => SESSION.with(|active| {
                active
                    .borrow_mut()
                    .as_mut()
                    .expect("current playback session")
                    .playback = Some(playback);
            }),
            Err(error) => {
                close();
                if let Some(window) = web_sys::window() {
                    let _ = window.alert_with_message(&js_error(error));
                }
            }
        }
    });
}
