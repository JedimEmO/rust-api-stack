//! In-process test transport wrapping an `axum_test::TestServer`.
//!
//! This transport fully buffers both request and response bodies: `axum-test`
//! drives the router directly and has no streaming request/response API. (The
//! native `ReqwestTransport` streams both; the wasm one also buffers.) It
//! exists so generated clients can be exercised end-to-end with no sockets.

#![cfg(all(not(target_arch = "wasm32"), feature = "axum-test"))]

use std::sync::Arc;

use axum_test::TestServer;
use bytes::BytesMut;
use futures_util::StreamExt;
use futures_util::stream;

use crate::error::TransportError;
use crate::request::{RequestBody, TransportRequest};
use crate::response::TransportResponse;
use crate::{HttpTransport, byte_stream_from};

/// A [`HttpTransport`] that dispatches into an `axum_test::TestServer`.
///
/// `TestServer` is **not** `Clone`, so it is held behind an `Arc`.
#[derive(Clone)]
pub struct AxumTestTransport {
    server: Arc<TestServer>,
}

impl AxumTestTransport {
    /// Construct from an owned `TestServer`.
    pub fn new(server: TestServer) -> Self {
        AxumTestTransport {
            server: Arc::new(server),
        }
    }

    /// Construct from a shared `TestServer`.
    pub fn from_arc(server: Arc<TestServer>) -> Self {
        AxumTestTransport { server }
    }
}

/// Strip scheme + authority from an absolute URL, leaving `path[?query]`.
///
/// `axum-test` routes against a path, not a full URL. Falls back to returning
/// the input unchanged when it does not look like an absolute URL.
fn strip_origin(url: &str) -> String {
    // Find "scheme://", then the first '/' after the authority.
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        match after.find('/') {
            Some(slash) => after[slash..].to_string(),
            None => "/".to_string(),
        }
    } else {
        url.to_string()
    }
}

#[async_trait::async_trait]
impl HttpTransport for AxumTestTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let path = strip_origin(&request.url);
        let mut req = self.server.method(request.method, &path);

        for (name, value) in request.headers.iter() {
            req = req.add_header(name.clone(), value.clone());
        }

        // Collect the (possibly streaming) request body — axum-test buffers.
        let body_bytes = match request.body {
            RequestBody::Empty => bytes::Bytes::new(),
            RequestBody::Bytes(b) => b,
            RequestBody::Stream(mut s) => {
                let mut buf = BytesMut::new();
                while let Some(chunk) = s.next().await {
                    buf.extend_from_slice(&chunk?);
                }
                buf.freeze()
            }
        };
        if !body_bytes.is_empty() {
            req = req.bytes(body_bytes);
        }

        let resp = req.await;
        let status = resp.status_code();
        let headers = resp.headers().clone();
        let bytes = resp.into_bytes();

        // Single-chunk response stream.
        let body_stream = byte_stream_from(stream::once(async move {
            Ok::<bytes::Bytes, TransportError>(bytes)
        }));

        Ok(TransportResponse::new(status, headers, body_stream))
    }
}
