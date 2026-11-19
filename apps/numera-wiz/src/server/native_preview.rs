//! Separate private binary responses deliver native picture and sound.
use super::{artifacts::DownloadAuthority, *};
use axum::{Extension, body::Body, extract::Path};
use bytes::Bytes;
use http_body::Frame;
use nw_domain::{
    DomainError,
    preview::native_wire::{self, ClockUpdate, OpenRequest},
};
use nw_service::{NativePreviewTrack, NativePreviewTransfer};
use std::{future::Future, pin::Pin, task::{Context as TaskContext, Poll}};

fn rejected(error: DomainError) -> Response {
    let status = match error.code.as_str() {
        "OBJECT_NOT_FOUND" => StatusCode::NOT_FOUND,
        "PERMISSION_DENIED" => StatusCode::FORBIDDEN,
        "AUTHENTICATION_REQUIRED" => StatusCode::UNAUTHORIZED,
        "STALE_REVISION" | "OBJECT_CONFLICT" => StatusCode::CONFLICT,
        "OWNER_BUSY" | "OWNER_STOPPING" | "RESOURCE_LIMIT" => StatusCode::SERVICE_UNAVAILABLE,
        "WORKER_FAILED" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    failure(status, &error.code, &error.message)
}

pub(super) async fn open(
    State(owner): State<Arc<Owner>>,
    Extension(authority): Extension<DownloadAuthority>,
    Json(input): Json<OpenRequest>,
) -> Response {
    if let Err(error) = authority.check() {
        return rejected(error);
    }
    match owner.service.open_native_preview(
        authority.context,
        input,
        authority.listener_stop,
    ).await {
        Ok(opened) => ([(header::CACHE_CONTROL, "no-store")], Json(opened)).into_response(),
        Err(error) => rejected(error),
    }
}

pub(super) async fn clock(
    State(owner): State<Arc<Owner>>,
    Extension(authority): Extension<DownloadAuthority>,
    Path(id): Path<String>,
    Json(update): Json<ClockUpdate>,
) -> Response {
    let advanced = tokio::task::spawn_blocking(move || {
        authority.check()?;
        owner.service.update_native_preview_clock(&authority.context, &id, update)
    }).await;
    match advanced {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => rejected(error),
        Err(_) => rejected(DomainError::new("WORKER_FAILED", "Native playback clock worker failed")),
    }
}

pub(super) async fn close(
    State(owner): State<Arc<Owner>>,
    Extension(authority): Extension<DownloadAuthority>,
    Path(id): Path<String>,
) -> Response {
    match authority.check().and_then(|()| owner.service.close_native_preview(&authority.context, &id)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => rejected(error),
    }
}

struct NextPacket {
    transfer: NativePreviewTransfer,
    result: nw_domain::Result<Option<(Bytes, Bytes)>>,
}

type PacketFuture = Pin<Box<dyn Future<Output = NextPacket> + Send>>;

struct PreviewBody {
    descriptor: Option<Bytes>,
    transfer: Option<NativePreviewTransfer>,
    packet: Option<PacketFuture>,
    payload: Option<Bytes>,
    authority: DownloadAuthority,
    ended: bool,
}

impl http_body::Body for PreviewBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        if self.ended {
            return Poll::Ready(None);
        }
        if let Err(error) = self.authority.check() {
            return self.interrupt(error);
        }
        if let Some(bytes) = self.descriptor.take().or_else(|| self.payload.take()) {
            return Poll::Ready(Some(Ok(Frame::data(bytes))));
        }
        if let Some(mut transfer) = self.transfer.take() {
            self.packet = Some(Box::pin(async move {
                let result = async {
                    let Some(packet) = transfer.next_packet().await? else {
                        return Ok(None);
                    };
                    let header = packet.wire_header(transfer.descriptor().format)?;
                    header.validate(transfer.descriptor())?;
                    let encoded = native_wire::encode_header(&header)?;
                    Ok(Some((Bytes::from(encoded), Bytes::from(packet.bytes))))
                }.await;
                NextPacket { transfer, result }
            }));
        }
        let next = self.packet.as_mut().expect("Native body has an owned packet future");
        let result = std::task::ready!(next.as_mut().poll(context));
        self.packet = None;
        self.transfer = Some(result.transfer);
        match result.result {
            Ok(Some((header, bytes))) => {
                self.payload = Some(bytes);
                Poll::Ready(Some(Ok(Frame::data(header))))
            }
            Ok(None) => {
                self.ended = true;
                self.transfer = None;
                Poll::Ready(None)
            }
            Err(error) => self.interrupt(error),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.ended
    }
}

impl PreviewBody {
    fn interrupt(&mut self, error: DomainError) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        self.ended = true;
        self.transfer = None;
        self.packet = None;
        self.payload = None;
        Poll::Ready(Some(Err(axum::Error::new(std::io::Error::other(error.code)))))
    }
}

pub(super) async fn receive(
    State(owner): State<Arc<Owner>>,
    Extension(authority): Extension<DownloadAuthority>,
    Path((id, track)): Path<(String, String)>,
    request: Request,
) -> Response {
    let track = match track.as_str() {
        "picture" => NativePreviewTrack::Picture,
        "sound" => NativePreviewTrack::Sound,
        _ => return rejected(DomainError::object_not_found()),
    };
    if request.uri().query().is_some() || request.method() != axum::http::Method::GET {
        return rejected(DomainError::new("VALIDATION_ERROR", "Native preview requires a bare GET"));
    }
    let auth = authority.clone();
    let transfer = tokio::task::spawn_blocking(move || {
        auth.check()?;
        owner.service.claim_native_preview(auth.context, &id, track)
    }).await;
    match transfer {
        Ok(Ok(transfer)) => match native_wire::encode_header(transfer.descriptor()) {
            Ok(wire) => (
                [(header::CONTENT_TYPE, "application/octet-stream"), (header::CACHE_CONTROL, "no-store")],
                Body::new(PreviewBody {
                    descriptor: Some(Bytes::from(wire)), transfer: Some(transfer), packet: None,
                    payload: None, authority, ended: false,
                }),
            ).into_response(),
            Err(error) => rejected(error),
        },
        Ok(Err(error)) => rejected(error),
        Err(_) => rejected(DomainError::new("WORKER_FAILED", "Native preview stream worker failed")),
    }
}
