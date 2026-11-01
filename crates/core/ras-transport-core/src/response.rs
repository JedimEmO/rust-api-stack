//! Response type wrapping a single-consumption streaming body.

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;

use crate::ByteStream;
use crate::error::TransportError;

/// The most bytes of an error body [`TransportResponse::error_for_status`]
/// keeps for the diagnostic message.
const ERROR_BODY_LIMIT: usize = 64 * 1024;

/// An HTTP response with a lazily-consumed streaming body.
///
/// Not `Clone`: the body is a single-consumption [`ByteStream`].
///
/// [`bytes`](Self::bytes), [`json`](Self::json) and [`text`](Self::text)
/// collect the body into memory and fail with
/// [`TransportError::BodyTooLarge`] past [`body_limit`](Self::body_limit), so
/// a hostile or broken server cannot make the client buffer without bound.
/// [`into_body_stream`](Self::into_body_stream) has no limit.
pub struct TransportResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: ByteStream,
    body_limit: usize,
}

impl TransportResponse {
    /// The default for [`body_limit`](Self::body_limit): 16 MiB.
    pub const DEFAULT_BODY_LIMIT: usize = 16 * 1024 * 1024;

    /// Construct a response from its parts, with
    /// [`DEFAULT_BODY_LIMIT`](Self::DEFAULT_BODY_LIMIT).
    pub fn new(status: StatusCode, headers: HeaderMap, body: ByteStream) -> Self {
        TransportResponse {
            status,
            headers,
            body,
            body_limit: Self::DEFAULT_BODY_LIMIT,
        }
    }

    /// Set the most bytes [`bytes`](Self::bytes), [`json`](Self::json) and
    /// [`text`](Self::text) collect before they fail.
    ///
    /// A transport sets this from its own configuration, such as
    /// `ReqwestTransport::with_body_limit`. Raise it here to collect one
    /// large download, or stream it with
    /// [`into_body_stream`](Self::into_body_stream) instead.
    pub fn with_body_limit(mut self, limit: usize) -> Self {
        self.body_limit = limit;
        self
    }

    /// The most bytes the collecting methods read before they fail.
    pub fn body_limit(&self) -> usize {
        self.body_limit
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
    ///
    /// # Errors
    ///
    /// [`TransportError::BodyTooLarge`] when the body is longer than
    /// [`body_limit`](Self::body_limit), and any error from the body stream.
    pub async fn bytes(self) -> Result<Bytes, TransportError> {
        let limit = self.body_limit;
        match collect_up_to(self.body, limit).await? {
            Collected::Complete(bytes) => Ok(bytes),
            Collected::Truncated(_) => Err(TransportError::BodyTooLarge { limit }),
        }
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
            let limit = self.body_limit.min(ERROR_BODY_LIMIT);
            // Surface a failed body read rather than silently blanking the
            // diagnostic — an empty `http status 500:` hides why the body was
            // unreadable. A long body is cut short: it is only a diagnostic.
            let body = match collect_up_to(self.body, limit).await {
                Ok(Collected::Complete(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
                Ok(Collected::Truncated(bytes)) => {
                    format!("{}<truncated>", String::from_utf8_lossy(&bytes))
                }
                Err(e) => format!("<failed to read response body: {e}>"),
            };
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
            .field("body_limit", &self.body_limit)
            .finish()
    }
}

/// A body read by [`collect_up_to`].
enum Collected {
    /// The whole body, at most `limit` bytes.
    Complete(Bytes),
    /// The first `limit` bytes of a longer body; the rest was not read.
    Truncated(Bytes),
}

/// Read `stream` into memory, stopping once it passes `limit` bytes.
async fn collect_up_to(mut stream: ByteStream, limit: usize) -> Result<Collected, TransportError> {
    let mut buf = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let room = limit - buf.len();
        if chunk.len() > room {
            buf.extend_from_slice(&chunk[..room]);
            return Ok(Collected::Truncated(buf.freeze()));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(Collected::Complete(buf.freeze()))
}
