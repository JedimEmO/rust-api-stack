//! Request types: the body enum, the `TransportRequest` value, and its builders.

use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde::Serialize;

use crate::ByteStream;
use crate::error::TransportError;

/// The body of an outgoing request.
///
/// `Stream` carries a real streaming body on native; on wasm the
/// [`crate::HttpTransport`] implementations collect it before sending because
/// the fetch API cannot stream request bodies.
pub enum RequestBody {
    /// No body.
    Empty,
    /// A fully-buffered body.
    Bytes(Bytes),
    /// A streaming body (multipart uploads, file uploads).
    Stream(ByteStream),
}

impl RequestBody {
    /// An empty body.
    pub fn empty() -> Self {
        RequestBody::Empty
    }

    /// Serialize a value as a JSON body.
    ///
    /// The caller is responsible for setting `Content-Type: application/json`
    /// (see [`TransportRequest::json`], which does both).
    pub fn from_json<T: Serialize>(value: &T) -> Result<Self, TransportError> {
        let bytes = serde_json::to_vec(value).map_err(TransportError::Serialize)?;
        Ok(RequestBody::Bytes(Bytes::from(bytes)))
    }
}

impl std::fmt::Debug for RequestBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestBody::Empty => f.write_str("RequestBody::Empty"),
            RequestBody::Bytes(b) => f.debug_tuple("RequestBody::Bytes").field(&b.len()).finish(),
            RequestBody::Stream(_) => f.write_str("RequestBody::Stream(..)"),
        }
    }
}

/// A fully-resolved HTTP request handed to a transport.
///
/// `url` is always the full absolute URL the client builds today;
/// [`crate::AxumTestTransport`] strips the scheme+authority down to a
/// path+query.
#[derive(Debug)]
pub struct TransportRequest {
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL (scheme + authority + path + query).
    pub url: String,
    /// Request headers.
    pub headers: HeaderMap,
    /// Request body.
    pub body: RequestBody,
    /// Optional per-request timeout (ignored on wasm).
    pub timeout: Option<Duration>,
}

impl TransportRequest {
    /// Construct a new request with an empty body and no headers.
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        TransportRequest {
            method,
            url: url.into(),
            headers: HeaderMap::new(),
            body: RequestBody::Empty,
            timeout: None,
        }
    }

    /// Add a header. Invalid header names/values are silently dropped.
    pub fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_ref()),
            HeaderValue::try_from(value.as_ref()),
        ) {
            self.headers.insert(name, value);
        }
        self
    }

    /// Set the `Authorization: Bearer <token>` header.
    pub fn bearer(self, token: impl AsRef<str>) -> Self {
        self.header("authorization", format!("Bearer {}", token.as_ref()))
    }

    /// Serialize `value` as a JSON body and set `Content-Type: application/json`.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, TransportError> {
        self.body = RequestBody::from_json(value)?;
        Ok(self.header("content-type", "application/json"))
    }

    /// Set the body directly.
    pub fn body(mut self, body: RequestBody) -> Self {
        self.body = body;
        self
    }

    /// Set the per-request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}
