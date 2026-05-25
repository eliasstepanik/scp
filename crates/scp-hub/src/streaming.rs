use axum::response::{
    sse::{Event, KeepAlive},
    IntoResponse, Response, Sse,
};
use futures::stream::{self, Stream};
use std::convert::Infallible;

/// Convert a completed MCP JSON-RPC response into an SSE stream.
/// Sends a single "message" event with the JSON payload, then closes.
pub fn sse_response_from_json(json: serde_json::Value) -> Response {
    let event = Event::default().event("message").data(json.to_string());

    let s = stream::once(async move { Ok::<Event, Infallible>(event) });

    Sse::new(s).keep_alive(KeepAlive::default()).into_response()
}

/// Streaming SSE wrapper: takes a stream of JSON chunks and emits each as an SSE event.
/// Used when the backend itself supports streaming (future use).
#[allow(dead_code)]
pub fn sse_response_from_stream<S>(chunk_stream: S) -> Response
where
    S: Stream<Item = Result<serde_json::Value, String>> + Send + 'static,
{
    use futures::StreamExt;

    let event_stream = chunk_stream.map(|chunk| {
        chunk
            .map(|v| Event::default().event("message").data(v.to_string()))
            .map_err(std::io::Error::other)
    });

    Sse::new(event_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::post,
        Router as AxumRouter,
    };
    use serde_json::json;
    use tower::ServiceExt;

    // Helper: build a tiny axum app that calls sse_response_from_json on every POST.
    fn make_sse_app() -> AxumRouter {
        AxumRouter::new().route(
            "/test",
            post(|| async {
                let payload = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {"content": [{"type": "text", "text": "hello"}]}
                });
                sse_response_from_json(payload)
            }),
        )
    }

    #[tokio::test]
    async fn test_sse_response_content_type() {
        let app = make_sse_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/test")
                    .header("Accept", "text/event-stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .expect("content-type header missing")
            .to_str()
            .unwrap();
        assert!(
            ct.contains("text/event-stream"),
            "Expected text/event-stream, got: {ct}"
        );
    }

    #[tokio::test]
    async fn test_sse_response_contains_valid_json_payload() {
        use axum::body::to_bytes;

        let app = make_sse_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Read the SSE body and verify it contains a "data:" line with valid JSON
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body_str = std::str::from_utf8(&bytes).unwrap();

        // SSE format: "event: message\r\ndata: <json>\r\n\r\n"
        let data_line = body_str
            .lines()
            .find(|l| l.starts_with("data:"))
            .expect("No data: line found in SSE response");

        let json_part = data_line.trim_start_matches("data:").trim();
        let parsed: serde_json::Value =
            serde_json::from_str(json_part).expect("data field is not valid JSON");

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
    }
}
