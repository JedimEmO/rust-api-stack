//! Response type wrapping a single-consumption streaming body.

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;

use crate::ByteStream;
use crate::error::TransportError;

/// An HTTP response with a lazily-consumed streaming body.
///
/// Not `Clone`: the body is a single-consumption [`ByteStream`].
pub struct TransportResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: ByteStream,
}

impl TransportResponse {
    /// Construct a response from its parts.
    pub fn new(status: StatusCode, headers: HeaderMap, body: ByteStream) -> Self {
        TransportResponse {
            status,
            headers,
            body,
        }
    }

    /// The HTTP status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The response headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Whether the status is in the 2xx range.
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Consume the response and collect the full body into [`Bytes`].
    pub async fn bytes(self) -> Result<Bytes, TransportError> {
        let mut stream = self.body;
        let mut buf = BytesMut::new();
        while let Some(chunk) = stream.next().await {
            buf.extend_from_slice(&chunk?);
        }
        Ok(buf.freeze())
    }

    /// Consume the response and deserialize the body as JSON.
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, TransportError> {
        let bytes = self.bytes().await?;
        crate::deserialize_json(&bytes)
    }

    /// Consume the response and decode the body as UTF-8 text (lossy).
    pub async fn text(self) -> Result<String, TransportError> {
        let bytes = self.bytes().await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Take the raw streaming body, leaving status/headers behind.
    pub fn into_body_stream(self) -> ByteStream {
        self.body
    }

    /// Return `self` if the status is success, otherwise collect the body and
    /// produce a [`TransportError::Status`].
    pub async fn error_for_status(self) -> Result<Self, TransportError> {
        if self.is_success() {
            Ok(self)
        } else {
            let status = self.status;
            let body = self.text().await.unwrap_or_default();
            Err(TransportError::http_status(status, body))
        }
    }
}

impl std::fmt::Debug for TransportResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &"ByteStream(..)")
            .finish()
    }
}
