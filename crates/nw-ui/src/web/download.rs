//! Bounded authenticated browser delivery through a real user-selected writable file.
use super::*;
use nw_domain::{
    artifacts::{ARTIFACT_CHUNK_BYTES, ArtifactManifest},
    download::{DownloadVerifier, SaveControl, SaveReceipt},
};

fn call(this: &JsValue, name: &str, argument: Option<&JsValue>) -> Result<js_sys::Promise, String> {
    let method = js_sys::Reflect::get(this, &JsValue::from_str(name))
        .map_err(js_error)?
        .dyn_into::<js_sys::Function>()
        .map_err(js_error)?;
    let result = match argument {
        Some(argument) => method.call1(this, argument),
        None => method.call0(this),
    }
    .map_err(js_error)?;
    result.dyn_into::<js_sys::Promise>().map_err(js_error)
}
pub(super) fn select_destination(manifest: &ArtifactManifest) -> Result<js_sys::Promise, String> {
    DownloadVerifier::new(manifest).map_err(|error| error.to_string())?;
    let window = web_sys::window().ok_or("Browser window unavailable")?;
    let picker = js_sys::Reflect::get(window.as_ref(), &JsValue::from_str("showSaveFilePicker"))
        .map_err(js_error)?;
    if !picker.is_function() {
        return Err("This browser lacks streamed file saving. Use a browser with the File System Access save picker, or the desktop/CLI artifact save command.".into());
    }
    let options = js_sys::Object::new();
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("suggestedName"),
        &JsValue::from_str(&manifest.suggested_file_name()),
    )
    .map_err(js_error)?;
    call(
        window.as_ref(),
        "showSaveFilePicker",
        Some(options.as_ref()),
    )
}
async fn response(
    origin: &str,
    bearer: &str,
    manifest: &ArtifactManifest,
) -> Result<(web_sys::Response, Deadline), String> {
    let window = web_sys::window().ok_or("Browser window unavailable")?;
    if window.location().origin().map_err(js_error)? != origin {
        return Err("Foreign service origins are refused".into());
    }
    let controller = web_sys::AbortController::new().map_err(js_error)?;
    let cancel = controller.clone();
    let callback = Closure::new(move || cancel.abort());
    let timer = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            130000,
        )
        .map_err(js_error)?;
    let deadline = Deadline {
        window: window.clone(),
        timer,
        controller: controller.clone(),
        _callback: callback,
    };
    let init = web_sys::RequestInit::new();
    init.set_method("GET");
    init.set_redirect(web_sys::RequestRedirect::Error);
    init.set_credentials(web_sys::RequestCredentials::Omit);
    init.set_mode(web_sys::RequestMode::SameOrigin);
    init.set_signal(Some(&controller.signal()));
    let request = web_sys::Request::new_with_str_and_init(
        &format!(
            "{origin}{}",
            manifest
                .bytes_route
                .as_deref()
                .ok_or("Artifact byte route missing")?
        ),
        &init,
    )
    .map_err(js_error)?;
    request
        .headers()
        .set("Authorization", &format!("Bearer {bearer}"))
        .map_err(js_error)?;
    let etag = format!("\"{}\"", manifest.sha256);
    request.headers().set("If-Match", &etag).map_err(js_error)?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;
    if response.status() != 200
        || response.headers().get("ETag").map_err(js_error)?.as_deref() != Some(etag.as_str())
        || response
            .headers()
            .get("Content-Length")
            .map_err(js_error)?
            .as_deref()
            != Some(manifest.encoded_byte_count.as_str())
        || response.headers().has("Content-Range").map_err(js_error)?
        || response
            .headers()
            .get("Content-Encoding")
            .map_err(js_error)?
            .is_some_and(|v| v != "identity")
    {
        return Err(format!(
            "Artifact response did not match the selected identity (HTTP {})",
            response.status()
        ));
    }
    Ok((response, deadline))
}
pub(super) async fn save(
    origin: &str,
    bearer: &str,
    manifest: ArtifactManifest,
    picker: js_sys::Promise,
    control: &SaveControl,
) -> Result<SaveReceipt, String> {
    // Do not report cancellation while the OS picker still owns a pending selection.
    let handle = JsFuture::from(picker).await.map_err(js_error)?;
    control.check().map_err(|error| error.to_string())?;
    let name = js_sys::Reflect::get(&handle, &JsValue::from_str("name"))
        .map_err(js_error)?
        .as_string()
        .filter(|name| name.len() <= 1024)
        .ok_or("Browser file name unavailable")?;
    let options = js_sys::Object::new();
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("keepExistingData"),
        &JsValue::FALSE,
    )
    .map_err(js_error)?;
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("mode"),
        &JsValue::from_str("exclusive"),
    )
    .map_err(js_error)?;
    let writer = JsFuture::from(call(&handle, "createWritable", Some(options.as_ref()))?)
        .await
        .map_err(js_error)?;
    let result = copy(origin, bearer, &manifest, &writer, &name, control).await;
    if result.is_err() {
        // An in-flight write has already drained. Await abort before reporting completion.
        match call(&writer, "abort", None) {
            Ok(abort) => {
                if JsFuture::from(abort).await.is_err() {
                    return Err("Browser save failed and abort could not be confirmed; inspect the selected client file".into());
                }
            }
            Err(_) => return Err(
                "Browser save failed and abort was unavailable; inspect the selected client file"
                    .into(),
            ),
        }
    }
    result
}
async fn copy(
    origin: &str,
    bearer: &str,
    manifest: &ArtifactManifest,
    writer: &JsValue,
    name: &str,
    control: &SaveControl,
) -> Result<SaveReceipt, String> {
    let mut verifier = DownloadVerifier::new(manifest).map_err(|error| error.to_string())?;
    let (response, _deadline) = control
        .interruptible(response(origin, bearer, manifest))
        .await
        .map_err(|error| error.to_string())??;
    let reader: web_sys::ReadableStreamDefaultReader = response
        .body()
        .ok_or("Artifact stream missing")?
        .get_reader()
        .dyn_into()
        .map_err(|error: js_sys::Object| js_error(error.into()))?;
    loop {
        let frame = control
            .interruptible(JsFuture::from(reader.read()))
            .await
            .map_err(|error| error.to_string())?
            .map_err(js_error)?;
        if js_sys::Reflect::get(&frame, &JsValue::from_str("done"))
            .map_err(js_error)?
            .as_bool()
            == Some(true)
        {
            break;
        }
        let frame = js_sys::Uint8Array::new(
            &js_sys::Reflect::get(&frame, &JsValue::from_str("value")).map_err(js_error)?,
        );
        let mut offset = 0u32;
        while offset < frame.length() {
            control.check().map_err(|error| error.to_string())?;
            let end = frame
                .length()
                .min(offset.saturating_add(ARTIFACT_CHUNK_BYTES as u32));
            let chunk = frame.subarray(offset, end);
            verifier
                .push(&chunk.to_vec())
                .map_err(|error| error.to_string())?;
            // Await writable-stream backpressure. A cancellation never drops an admitted write.
            JsFuture::from(call(writer, "write", Some(chunk.as_ref()))?)
                .await
                .map_err(js_error)?;
            control.set_copied(verifier.received_bytes());
            control.check().map_err(|error| error.to_string())?;
            offset = end;
        }
    }
    let receipt = verifier
        .finish(format!("Browser file: {name}"))
        .map_err(|error| error.to_string())?;
    control.begin_commit().map_err(|error| error.to_string())?;
    JsFuture::from(call(writer, "close", None)?)
        .await
        .map_err(js_error)?;
    Ok(receipt)
}
