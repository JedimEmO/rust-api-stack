//! Production transport backed by `reqwest`.
//!
//! reqwest re-exports the `http` crate's `Method`/`StatusCode`/`HeaderMap`, so
//! these cross the boundary with no conversion. The transport is a **dumb
//! pipe**: it never inspects status — the generated client calls
//! [`crate::TransportResponse::error_for_status`].

#![cfg(feature = "reqwest")]

use futures_util::StreamExt;

use crate::error::TransportError;
use crate::request::{RequestBody, TransportRequest};
use crate::response::TransportResponse;
use crate::{HttpTransport, byte_stream_from};

/// A [`HttpTransport`] backed by a `reqwest::Client`.
#[derive(Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Construct with a default `reqwest::Client`.
    pub fn new() -> Self {
        ReqwestTransport {
            client: reqwest::Client::new(),
        }
    }

    /// Construct from an existing `reqwest::Client`.
    pub fn from_client(client: reqwest::Client) -> Self {
        ReqwestTransport { client }
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl HttpTransport for ReqwestTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let mut builder = self
            .client
            .request(request.method, &request.url)
            .headers(request.headers);

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }

        builder = match request.body {
            RequestBody::Empty => builder,
            RequestBody::Bytes(bytes) => builder.body(bytes),
            #[cfg(not(target_arch = "wasm32"))]
            RequestBody::Stream(stream) => {
                builder.body(reqwest::Body::wrap_stream(stream))
            }
            #[cfg(target_arch = "wasm32")]
            RequestBody::Stream(mut stream) => {
                // wasm fetch cannot stream request bodies; collect first.
                let mut buf = bytes::BytesMut::new();
                while let Some(chunk) = stream.next().await {
                    buf.extend_from_slice(&chunk?);
                }
                builder.body(buf.freeze())
            }
        };

        let resp = builder.send().await?;
        let status = resp.status();
        let headers = resp.headers().clone();

        // Native streams the response body; wasm reqwest lacks the `stream`
        // feature, so collect into a single chunk (response streaming on wasm
        // is bounded by the fetch implementation regardless).
        #[cfg(not(target_arch = "wasm32"))]
        let body_stream =
            byte_stream_from(resp.bytes_stream().map(|res| res.map_err(TransportError::from)));

        #[cfg(target_arch = "wasm32")]
        let body_stream = {
            let bytes = resp.bytes().await?;
            byte_stream_from(futures_util::stream::once(async move {
                Ok::<bytes::Bytes, TransportError>(bytes)
            }))
        };

        Ok(TransportResponse::new(status, headers, body_stream))
    }
}
