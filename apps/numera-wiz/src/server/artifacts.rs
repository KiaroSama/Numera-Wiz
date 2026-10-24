//! Authenticated attachment delivery with one bounded read in flight.
use super::*;
use axum::extract::Path;
use nw_domain::{DomainError, artifacts::ARTIFACT_CHUNK_BYTES};
use nw_service::ArtifactTransfer;
use std::{
    pin::Pin,
    task::{Context as TaskContext, Poll},
};

#[derive(Clone)]
pub(super) struct DownloadAuthority {
    pub owner: Arc<Owner>,
    pub token: Option<String>,
    pub origin: Option<String>,
    pub context: Context,
    pub listener_stop: CancellationToken,
}
impl DownloadAuthority {
    fn check(&self) -> nw_domain::Result<()> {
        if self.owner.stop.is_cancelled() || self.listener_stop.is_cancelled() {
            return Err(DomainError::new(
                "OWNER_STOPPING",
                "Artifact listener is draining",
            ));
        }
        if let Some(token) = &self.token {
            let current =
                self.owner
                    .service
                    .authenticate_at(token, self.origin.as_deref(), false)?;
            if current.principal != self.context.principal
                || current.profile != self.context.profile
            {
                return Err(DomainError::new(
                    "AUTHENTICATION_REQUIRED",
                    "Artifact transport authority changed",
                ));
            }
        }
        Ok(())
    }
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
struct ArtifactBody {
    transfer: Option<ArtifactTransfer>,
    pending: Option<tokio::task::JoinHandle<(ArtifactTransfer, nw_domain::Result<Vec<u8>>)>>,
    authority: DownloadAuthority,
    position: u64,
    end: u64,
    finished: bool,
    _cancel: CancelOnDrop,
}
fn body_error(error: DomainError) -> axum::Error {
    // Transport errors contain safe codes, never paths or credentials.
    axum::Error::new(std::io::Error::other(error.code))
}
impl http_body::Body for ArtifactBody {
    type Data = bytes::Bytes;
    type Error = axum::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let state = self.get_mut();
        if state.finished || state.position >= state.end {
            state.finished = true;
            return Poll::Ready(None);
        }
        if let Err(error) = state.authority.check() {
            state.finished = true;
            return Poll::Ready(Some(Err(body_error(error))));
        }
        if state.pending.is_none() {
            let Some(mut transfer) = state.transfer.take() else {
                state.finished = true;
                return Poll::Ready(Some(Err(body_error(DomainError::new(
                    "INTERNAL_ERROR",
                    "Artifact reader state is absent",
                )))));
            };
            let offset = state.position;
            let length = (state.end - offset).min(ARTIFACT_CHUNK_BYTES as u64) as usize;
            state.pending = Some(tokio::task::spawn_blocking(move || {
                let result = transfer.read_range(offset, length);
                (transfer, result)
            }));
        }
        let result = match std::future::Future::poll(
            Pin::new(state.pending.as_mut().expect("Pending reader was set")),
            cx,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        state.pending = None;
        match result {
            Ok((transfer, Ok(bytes))) => {
                if let Err(error) = state
                    .authority
                    .check()
                    .and_then(|()| transfer.ensure_current())
                {
                    state.finished = true;
                    return Poll::Ready(Some(Err(body_error(error))));
                }
                if bytes.is_empty() {
                    state.finished = true;
                    return Poll::Ready(Some(Err(body_error(DomainError::new(
                        "ARTIFACT_CHANGED",
                        "Artifact ended before the admitted range",
                    )))));
                }
                state.position += bytes.len() as u64;
                state.transfer = Some(transfer);
                Poll::Ready(Some(Ok(http_body::Frame::data(bytes.into()))))
            }
            Ok((_, Err(error))) => {
                state.finished = true;
                Poll::Ready(Some(Err(body_error(error))))
            }
            Err(_) => {
                state.finished = true;
                Poll::Ready(Some(Err(body_error(DomainError::new(
                    "WORKER_FAILED",
                    "Artifact worker failed",
                )))))
            }
        }
    }
    fn is_end_stream(&self) -> bool {
        self.finished || self.position >= self.end
    }
    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.end.saturating_sub(self.position))
    }
}
fn range(header: Option<&str>, total: u64) -> nw_domain::Result<(u64, u64)> {
    let Some(header) = header else {
        return Ok((0, total));
    };
    let invalid = || {
        DomainError::new(
            "INVALID_RANGE",
            "Only one satisfiable bytes range is admitted",
        )
    };
    let spec = header
        .strip_prefix("bytes=")
        .filter(|s| !s.contains(','))
        .ok_or_else(invalid)?;
    let (start, end) = spec.split_once('-').ok_or_else(invalid)?;
    if total == 0 {
        return Err(invalid());
    }
    if start.is_empty() {
        let suffix = nw_domain::artifacts::decimal(end)?;
        if suffix == 0 {
            return Err(invalid());
        }
        return Ok((total.saturating_sub(suffix), total));
    }
    let start = nw_domain::artifacts::decimal(start)?;
    if start >= total {
        return Err(invalid());
    }
    let end = if end.is_empty() {
        total
    } else {
        nw_domain::artifacts::decimal(end)?
            .min(total - 1)
            .checked_add(1)
            .ok_or_else(invalid)?
    };
    if end <= start {
        return Err(invalid());
    }
    Ok((start, end))
}
fn rejected(error: DomainError) -> Response {
    let status = match error.code.as_str() {
        "OBJECT_NOT_FOUND" | "ARTIFACT_UNAVAILABLE" => StatusCode::NOT_FOUND,
        "ARTIFACT_CHANGED" => StatusCode::PRECONDITION_FAILED,
        "PERMISSION_DENIED" => StatusCode::FORBIDDEN,
        "RESOURCE_LIMIT" => StatusCode::SERVICE_UNAVAILABLE,
        "AUTHENTICATION_REQUIRED" => StatusCode::UNAUTHORIZED,
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"error":error})),
    )
        .into_response()
}
pub(super) async fn receive(
    State(owner): State<Arc<Owner>>,
    Path(id): Path<String>,
    axum::Extension(authority): axum::Extension<DownloadAuthority>,
    request: Request,
) -> Response {
    if request.uri().query().is_some() || request.method() != axum::http::Method::GET {
        return failure(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "Artifact bytes require GET with bearer-header authentication and no query parameters",
        );
    }
    let headers = request.headers();
    if headers.get_all(header::RANGE).iter().count() > 1
        || headers.get_all(header::IF_MATCH).iter().count() > 1
    {
        return rejected(DomainError::new(
            "INVALID_RANGE",
            "Repeated range or precondition headers are not admitted",
        ));
    }
    let expected = match headers.get(header::IF_MATCH) {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|s| s.strip_prefix('"'))
            .and_then(|s| s.strip_suffix('"'))
            .filter(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }) {
            Some(value) => Some(value.to_owned()),
            None => {
                return rejected(DomainError::new(
                    "ARTIFACT_CHANGED",
                    "If-Match requires one strong SHA-256 ETag",
                ));
            }
        },
        None => None,
    };
    let range_header = match headers.get(header::RANGE).map(|v| v.to_str()) {
        Some(Err(_)) => {
            return rejected(DomainError::new(
                "INVALID_RANGE",
                "Range header must be ASCII",
            ));
        }
        value => value.map(Result::unwrap).map(str::to_owned),
    };
    let cancel = owner.stop.child_token();
    let guard = CancelOnDrop(cancel.clone());
    let service = owner.service.clone();
    let context = authority.context.clone();
    let selected_id = id.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        service.begin_artifact_transfer(context, &selected_id, expected.as_deref(), cancel)
    })
    .await;
    let transfer = match prepared {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => return rejected(error),
        Err(_) => {
            return rejected(DomainError::new(
                "WORKER_FAILED",
                "Artifact preflight worker failed",
            ));
        }
    };
    if let Err(error) = authority.check().and_then(|()| transfer.ensure_current()) {
        return rejected(error);
    }
    let total = transfer.bytes();
    let (start, end) = match range(range_header.as_deref(), total) {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [
                    (header::CONTENT_RANGE, format!("bytes */{total}")),
                    (header::CACHE_CONTROL, "no-store".into()),
                ],
            )
                .into_response();
        }
    };
    let etag = format!("\"{}\"", transfer.sha256());
    let mut response = Response::new(axum::body::Body::new(ArtifactBody {
        transfer: Some(transfer),
        pending: None,
        authority,
        position: start,
        end,
        finished: false,
        _cancel: guard,
    }));
    *response.status_mut() = if range_header.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
        (header::CACHE_CONTROL, "no-store".into()),
        (header::ACCEPT_RANGES, "bytes".into()),
        (header::ETAG, etag),
        (header::CONTENT_LENGTH, (end - start).to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"numera-{id}.bin\""),
        ),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
    ] {
        match value.parse() {
            Ok(value) => {
                headers.insert(name, value);
            }
            Err(_) => {
                return rejected(DomainError::new(
                    "INTERNAL_ERROR",
                    "Artifact response header could not be encoded",
                ));
            }
        }
    }
    if range_header.is_some() {
        headers.insert(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{total}", end - 1)
                .parse()
                .expect("Validated decimal range is a header value"),
        );
    }
    response
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_single_ranges_and_invalid_amplification() {
        assert_eq!(range(None, 10).unwrap(), (0, 10));
        assert_eq!(range(Some("bytes=2-5"), 10).unwrap(), (2, 6));
        assert_eq!(range(Some("bytes=8-"), 10).unwrap(), (8, 10));
        assert_eq!(range(Some("bytes=-3"), 10).unwrap(), (7, 10));
        assert_eq!(range(Some("bytes=0-100"), 10).unwrap(), (0, 10));
        for header in [
            "bytes=0-1,4-5",
            "bytes=10-",
            "bytes=5-2",
            "bytes=-0",
            "bytes=01-3",
            "bytes=0-18446744073709551616",
        ] {
            assert!(range(Some(header), 10).is_err(), "{header}");
        }
        assert!(range(Some("bytes=0-"), 0).is_err());
    }
}
