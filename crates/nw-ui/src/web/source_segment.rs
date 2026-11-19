//! Append authenticated, bounded MP4 units to the browser's media decoder.
use super::{Deadline, js_error};
use js_sys::{Promise, Reflect, Uint8Array};
use nw_domain::preview::{
    MAX_SOURCE_SEGMENT_BYTES, MAX_SOURCE_SEGMENT_FRAGMENTS, SegmentBytes, SourceSegment,
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, AbortSignal, Event, EventTarget, HtmlVideoElement, MediaSource,
    ReadableStreamDefaultReader, Request, RequestInit, RequestMode, RequestRedirect, Response, Url,
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
    manifest: &SourceSegment,
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

async fn play(
    origin: &str,
    bearer: &str,
    manifest: SourceSegment,
    controller: AbortController,
    id: Rc<()>,
) -> Result<Playback, JsValue> {
    let window =
        web_sys::window().ok_or_else(|| JsValue::from_str("Browser window is unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("Browser document is unavailable"))?;
    if !supported() || !MediaSource::is_type_supported(&manifest.mime_type) {
        return Err(JsValue::from_str(
            "This browser does not admit the selected AVC and AAC preview profile",
        ));
    }
    if document.hidden() || !is_active(&id) {
        return Err(JsValue::from_str("Browser preview was closed"));
    }
    if !(1..=MAX_SOURCE_SEGMENT_FRAGMENTS).contains(&manifest.fragments.len()) {
        return Err(JsValue::from_str("Invalid MP4 fragment count"));
    }
    let panel = document.create_element("section")?;
    panel.set_attribute("aria-label", "Source preview")?;
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
        20000,
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
    let mut bytes = fetch_segment(&window, origin, bearer, &manifest, &signal).await?;
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
    let buffer = media.add_source_buffer(&manifest.mime_type)?;
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

pub(super) fn open(origin: &str, bearer: &str, manifest: SourceSegment) {
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
        let result = play(&origin, &bearer, manifest, controller, id.clone()).await;
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
