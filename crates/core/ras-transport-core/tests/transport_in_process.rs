//! End-to-end coverage of `AxumTestTransport` against a real `axum::Router`
//! driven through `axum-test`'s in-process mock transport — no sockets.

#![cfg(all(not(target_arch = "wasm32"), feature = "axum-test"))]

use axum::Router;
use axum::body::Bytes as AxumBytes;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use bytes::Bytes;
use futures_util::stream;
use ras_transport_core::request::RequestBody;
use ras_transport_core::{
    AxumTestTransport, HttpTransport, TransportError, TransportRequest, byte_stream_from,
};

async fn echo_body(headers: HeaderMap, body: AxumBytes) -> (StatusCode, String) {
    // Echo the body, and surface a header so header forwarding is observable.
    let seen_auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("none")
        .to_string();
    (
        StatusCode::OK,
        format!("{}|auth={}", String::from_utf8_lossy(&body), seen_auth),
    )
}

fn router() -> Router {
    Router::new()
        .route("/ping", get(|| async { "pong" }))
        .route("/echo", post(echo_body))
        .route(
            "/boom",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "kaboom") }),
        )
        .route(
            "/items/{id}",
            get(|Path(id): Path<String>| async move { format!("item:{id}") }),
        )
}

fn transport() -> AxumTestTransport {
    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(router())
        .expect("build mock-transport TestServer");
    AxumTestTransport::new(server)
}

#[tokio::test]
async fn get_with_absolute_url_strips_origin_and_returns_body() {
    let t = transport();
    let resp = t
        .execute(TransportRequest::new(http::Method::GET, "http://api.example/ping"))
        .await
        .unwrap();
    assert_eq!(resp.status(), http::StatusCode::OK);
    assert_eq!(resp.text().await.unwrap(), "pong");
}

#[tokio::test]
async fn get_with_bare_path_url_also_routes() {
    // strip_origin fallback branch: no "scheme://", url used as-is.
    let t = transport();
    let resp = t
        .execute(TransportRequest::new(http::Method::GET, "/ping"))
        .await
        .unwrap();
    assert_eq!(resp.text().await.unwrap(), "pong");
}

#[tokio::test]
async fn absolute_url_without_path_falls_back_to_root() {
    // strip_origin: "scheme://authority" with no trailing slash -> "/".
    // Root isn't routed, so we just assert it dispatches and yields a status.
    let t = transport();
    let resp = t
        .execute(TransportRequest::new(http::Method::GET, "http://api.example"))
        .await
        .unwrap();
    assert_eq!(resp.status(), http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_bytes_body_and_headers_are_forwarded() {
    let t = transport();
    let req = TransportRequest::new(http::Method::POST, "http://api.example/echo")
        .bearer("sekret")
        .body(RequestBody::Bytes(Bytes::from_static(b"payload")));
    let resp = t.execute(req).await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "payload|auth=Bearer sekret");
}

#[tokio::test]
async fn post_streaming_body_is_collected_before_dispatch() {
    // Exercises the RequestBody::Stream collection branch in AxumTestTransport.
    let t = transport();
    let chunks: Vec<&'static [u8]> = vec![b"strea", b"ming", b"-body"];
    let body = byte_stream_from(stream::iter(
        chunks
            .into_iter()
            .map(|c| Ok::<Bytes, TransportError>(Bytes::from_static(c))),
    ));
    let req = TransportRequest::new(http::Method::POST, "http://api.example/echo")
        .body(RequestBody::Stream(body));
    let resp = t.execute(req).await.unwrap();
    assert_eq!(resp.text().await.unwrap(), "streaming-body|auth=none");
}

#[tokio::test]
async fn empty_body_get_does_not_set_a_body() {
    // path param route + empty body branch.
    let t = transport();
    let resp = t
        .execute(TransportRequest::new(
            http::Method::GET,
            "http://api.example/items/42",
        ))
        .await
        .unwrap();
    assert_eq!(resp.text().await.unwrap(), "item:42");
}

#[tokio::test]
async fn non_success_maps_through_error_for_status() {
    let t = transport();
    let resp = t
        .execute(TransportRequest::new(http::Method::GET, "http://api.example/boom"))
        .await
        .unwrap();
    let err = resp.error_for_status().await.unwrap_err();
    match err {
        TransportError::Status { status, body } => {
            assert_eq!(status, http::StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body, "kaboom");
        }
        other => panic!("expected Status, got {other:?}"),
    }
}

#[tokio::test]
async fn from_arc_constructor_shares_the_server() {
    let server = std::sync::Arc::new(
        axum_test::TestServer::builder()
            .mock_transport()
            .build(router())
            .unwrap(),
    );
    let t = AxumTestTransport::from_arc(server);
    let resp = t
        .execute(TransportRequest::new(http::Method::GET, "/ping"))
        .await
        .unwrap();
    assert_eq!(resp.text().await.unwrap(), "pong");
}
