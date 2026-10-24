//! Upload payloads stream to owned staging, separately from JSON command limits.
use super::*;
use axum::extract::Path;
fn rejected(error: nw_domain::DomainError) -> Response {
    let status = match error.code.as_str() {
        "PERMISSION_DENIED" => StatusCode::FORBIDDEN,
        "RESOURCE_LIMIT" => StatusCode::PAYLOAD_TOO_LARGE,
        "INGEST_NOT_FOUND" => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(json!({"error":error}))).into_response()
}
pub(super) async fn create(
    State(owner): State<Arc<Owner>>,
    axum::Extension(context): axum::Extension<Context>,
    Json(input): Json<nw_domain::ingest::UploadInput>,
) -> Response {
    let service = owner.service.clone();
    match tokio::task::spawn_blocking(move || service.create_upload(&context, input)).await {
        Ok(Ok(ticket)) => Json(ticket).into_response(),
        Ok(Err(error)) => rejected(error),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WORKER_FAILED",
            "Intake admission worker failed",
        ),
    }
}
pub(super) async fn receive(
    State(owner): State<Arc<Owner>>,
    axum::Extension(context): axum::Extension<Context>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("application/octet-stream")
    {
        return failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "VALIDATION_ERROR",
            "Upload bytes require application/octet-stream",
        );
    }
    let service = owner.service.clone();
    let mut writer =
        match tokio::task::spawn_blocking(move || service.begin_upload(&context, &id)).await {
            Ok(Ok(writer)) => writer,
            Ok(Err(error)) => return rejected(error),
            Err(_) => {
                return failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "WORKER_FAILED",
                    "Intake writer worker failed",
                );
            }
        };
    let mut body = Box::pin(request.into_body());
    loop {
        let frame = tokio::time::timeout(
            Duration::from_secs(10),
            std::future::poll_fn(|context| http_body::Body::poll_frame(body.as_mut(), context)),
        )
        .await;
        let frame = match frame {
            Ok(Some(Ok(frame))) => frame,
            Ok(None) => break,
            _ => {
                return failure(
                    StatusCode::REQUEST_TIMEOUT,
                    "INGEST_INTERRUPTED",
                    "Upload disconnected or exceeded the 10 second inactivity deadline",
                );
            }
        };
        if let Ok(bytes) = frame.into_data()
            && let Err(error) = writer.write(&bytes).await
        {
            return rejected(error);
        }
    }
    match writer.finish().await {
        Ok(ticket) => Json(ticket).into_response(),
        Err(error) => rejected(error),
    }
}
