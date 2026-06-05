//! Security regression: path parameters substituted into the URL template must
//! be percent-encoded for their segment, so a `/`, `?`, `#`, etc. in a
//! caller-supplied value cannot break out of its slot and alter the request's
//! path or query. Drives the generated client through a capturing transport and
//! inspects the exact wire URL it produced.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use ras_rest_macro::rest_service;
use ras_transport_core::{
    ByteStream, HttpTransport, TransportError, TransportRequest, TransportResponse,
    byte_stream_from,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Item {
    id: u32,
    name: String,
}

rest_service!({
    service_name: PathDemo,
    base_path: "/api",
    openapi: false,
    serve_docs: false,
    endpoints: [
        GET UNAUTHORIZED files/{name: String}() -> Item,
    ]
});

/// A transport that records the request URL and returns a canned 200 response.
#[derive(Clone, Default)]
struct CapturingTransport {
    last_url: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl HttpTransport for CapturingTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        *self.last_url.lock().unwrap() = Some(request.url.clone());

        // A valid body so the client method returns Ok; the URL is what we
        // assert on, but returning a parseable body keeps the test honest.
        let body: ByteStream = byte_stream_from(futures::stream::once(async {
            Ok::<Bytes, TransportError>(Bytes::from_static(b"{\"id\":1,\"name\":\"x\"}"))
        }));
        Ok(TransportResponse::new(
            ras_transport_core::http::StatusCode::OK,
            ras_transport_core::http::HeaderMap::new(),
            body,
        ))
    }
}

fn client_capturing() -> (PathDemoClient, Arc<Mutex<Option<String>>>) {
    let transport = CapturingTransport::default();
    let captured = transport.last_url.clone();
    let client = PathDemoClientBuilder::new("http://in-memory.test")
        .build_with_transport(Arc::new(transport))
        .expect("build PathDemoClient");
    (client, captured)
}

#[tokio::test]
async fn path_param_with_slash_cannot_escape_its_segment() {
    let (client, captured) = client_capturing();

    // A value containing '/' must not introduce extra path segments.
    let _ = client.get_files_by_name("a/b".to_string()).await;
    let url = captured.lock().unwrap().clone().expect("url captured");

    assert!(
        url.ends_with("/api/files/a%2Fb"),
        "path param '/' leaked into the URL path: {url}"
    );
    assert!(
        !url.contains("/api/files/a/b"),
        "path param was not encoded: {url}"
    );
}

#[tokio::test]
async fn path_param_with_query_and_fragment_chars_is_encoded() {
    let (client, captured) = client_capturing();

    // '?' would otherwise inject a query string; '#' a fragment.
    let _ = client
        .get_files_by_name("x?role=admin#frag".to_string())
        .await;
    let url = captured.lock().unwrap().clone().expect("url captured");

    assert!(
        url.ends_with("/api/files/x%3Frole%3Dadmin%23frag"),
        "query/fragment chars leaked unencoded into the URL: {url}"
    );
    // The URL must carry no real query or fragment delimiter.
    assert!(!url.contains('?'), "injected '?' present: {url}");
    assert!(!url.contains('#'), "injected '#' present: {url}");
}
