//! Stream verified MP4 fragments under the paired client's current authority.
use super::{
    artifacts::{DownloadAuthority, rejected},
    *,
};
use axum::{Extension, body::Body, extract::OriginalUri, http::HeaderValue};
use bytes::Bytes;
use http_body::Frame;
use nw_domain::{DomainError, preview::ProgramPlaybackRequest};
use nw_engine::BrowserMp4Part;
use nw_service::BrowserProgramTransfer;
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll},
};

type PartFuture = Pin<
    Box<
        dyn Future<
                Output = (
                    BrowserProgramTransfer,
                    nw_domain::Result<Option<BrowserMp4Part>>,
                ),
            > + Send,
    >,
>;

struct ProgramBody {
    initialization: Option<Bytes>,
    transfer: Option<BrowserProgramTransfer>,
    next: Option<PartFuture>,
    authority: DownloadAuthority,
    ended: bool,
}

impl ProgramBody {
    fn fail(&mut self, error: DomainError) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        self.ended = true;
        self.initialization = None;
        self.transfer = None;
        self.next = None;
        Poll::Ready(Some(Err(axum::Error::new(std::io::Error::other(
            error.code,
        )))))
    }
}

impl http_body::Body for ProgramBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        if self.ended {
            return Poll::Ready(None);
        }
        if let Err(error) = self.authority.check() {
            return self.fail(error);
        }
        if let Some(initialization) = self.initialization.take() {
            return Poll::Ready(Some(Ok(Frame::data(initialization))));
        }
        if let Some(mut transfer) = self.transfer.take() {
            self.next = Some(Box::pin(async move {
                let part = transfer.next_part().await;
                (transfer, part)
            }));
        }
        let pending = self
            .next
            .as_mut()
            .expect("Program stream owns one fragment read");
        let (transfer, part) = std::task::ready!(pending.as_mut().poll(context));
        self.next = None;
        self.transfer = Some(transfer);
        if let Err(error) = self.authority.check() {
            return self.fail(error);
        }
        match part {
            Ok(Some(BrowserMp4Part::Fragment(bytes))) => {
                Poll::Ready(Some(Ok(Frame::data(bytes.into()))))
            }
            Ok(None) => {
                self.ended = true;
                self.transfer = None;
                Poll::Ready(None)
            }
            Ok(Some(BrowserMp4Part::Initialization { .. })) => self.fail(DomainError::new(
                "INVALID_BROWSER_PREVIEW",
                "Repeated program initialization",
            )),
            Err(error) => self.fail(error),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.ended
    }
}

pub(super) async fn open(
    State(owner): State<Arc<Owner>>,
    Extension(authority): Extension<DownloadAuthority>,
    OriginalUri(uri): OriginalUri,
    Json(input): Json<ProgramPlaybackRequest>,
) -> Response {
    if uri.query().is_some() {
        return rejected(DomainError::new(
            "VALIDATION_ERROR",
            "Program preview does not accept query parameters",
        ));
    }
    let start = tokio::time::timeout(Duration::from_secs(22), async {
        authority.check()?;
        let mut transfer = owner
            .service
            .open_browser_program(
                authority.context.clone(),
                input,
                authority.listener_stop.clone(),
            )
            .await?;
        let Some(BrowserMp4Part::Initialization { mime_type, bytes }) =
            transfer.next_part().await?
        else {
            return Err(DomainError::new(
                "INVALID_BROWSER_PREVIEW",
                "Program preview has no initialization",
            ));
        };
        let content_type = HeaderValue::from_str(&mime_type).map_err(|_| {
            DomainError::new("INVALID_BROWSER_PREVIEW", "Invalid program video settings")
        })?;
        authority.check()?;
        Ok::<_, DomainError>((transfer, content_type, bytes))
    })
    .await;
    match start {
        Ok(Ok((transfer, content_type, bytes))) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            Body::new(ProgramBody {
                initialization: Some(bytes.into()),
                transfer: Some(transfer),
                next: None,
                authority,
                ended: false,
            }),
        )
            .into_response(),
        Ok(Err(error)) => rejected(error),
        Err(_) => failure(
            StatusCode::REQUEST_TIMEOUT,
            "PROCESS_TIMEOUT",
            "Program playback did not prepare before the deadline",
        ),
    }
}
