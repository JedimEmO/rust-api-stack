//! HTTP transport abstraction for generated Rust Agent Stack clients.
//!
//! Generated REST, JSON-RPC, and file clients use [`HttpTransport`]. The
//! `reqwest` feature provides a network adapter; the native `axum-test` feature
//! provides an in-process adapter for exercising clients without sockets.
//!
//! # Relationship to `WebSocketTransport`
//!
//! `ras-jsonrpc-bidirectional-client` owns the separate `WebSocketTransport`
//! abstraction because full-duplex frames have a different lifecycle from HTTP
//! requests. Both transport traits support native and WASM implementations.
//!
//! `ReqwestTransport` streams request and response bodies on native targets
//! and buffers both on WASM. `AxumTestTransport` buffers both bodies.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use serde::de::DeserializeOwned;

pub mod error;
pub mod multipart;
pub mod request;
pub mod response;

#[cfg(feature = "reqwest")]
pub mod reqwest_transport;

#[cfg(all(not(target_arch = "wasm32"), feature = "axum-test"))]
pub mod axum_test_transport;

pub use error::TransportError;
pub use multipart::MultipartBuilder;
pub use request::{RequestBody, TransportRequest};
pub use response::TransportResponse;

#[cfg(feature = "reqwest")]
pub use reqwest_transport::ReqwestTransport;

#[cfg(all(not(target_arch = "wasm32"), feature = "axum-test"))]
pub use axum_test_transport::AxumTestTransport;

/// Re-export of the `http` crate so generated code can refer to
/// `::ras_transport_core::http::Method` etc. without a direct dependency.
pub use http;

/// Marker for the thread bounds a transport (and its streams) must satisfy.
///
/// `Send + Sync` on native; unconstrained on wasm (single-threaded).
#[cfg(not(target_arch = "wasm32"))]
pub trait TransportThreadBounds: Send + Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> TransportThreadBounds for T {}

/// Marker for the thread bounds a transport (and its streams) must satisfy.
#[cfg(target_arch = "wasm32")]
pub trait TransportThreadBounds {}

#[cfg(target_arch = "wasm32")]
impl<T> TransportThreadBounds for T {}

/// A streaming sequence of body chunks. `Send` on native, not on wasm.
#[cfg(not(target_arch = "wasm32"))]
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send>>;

/// A streaming sequence of body chunks.
#[cfg(target_arch = "wasm32")]
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>>>>;

/// Box a stream into a [`ByteStream`], applying the conditional `Send` bound.
#[cfg(not(target_arch = "wasm32"))]
pub fn byte_stream_from<S>(stream: S) -> ByteStream
where
    S: Stream<Item = Result<Bytes, TransportError>> + Send + 'static,
{
    Box::pin(stream)
}

/// Box a stream into a [`ByteStream`].
#[cfg(target_arch = "wasm32")]
pub fn byte_stream_from<S>(stream: S) -> ByteStream
where
    S: Stream<Item = Result<Bytes, TransportError>> + 'static,
{
    Box::pin(stream)
}

/// Abstraction over the wire transport used by a generated HTTP client.
///
/// See the [crate-level docs](crate) for the relationship with
/// `WebSocketTransport`.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait HttpTransport: TransportThreadBounds {
    /// Execute a request and return the (streaming) response.
    ///
    /// Implementations are dumb pipes: they MUST NOT inspect the status code.
    /// Callers map non-success statuses via
    /// [`TransportResponse::error_for_status`].
    async fn execute(&self, request: TransportRequest)
    -> Result<TransportResponse, TransportError>;
}

mod path;
mod query;
pub use path::encode_path_segment;
pub use query::{serialize_query_pairs, serialize_query_value};

/// Deserialize JSON bytes into `T`, mapping failures to
/// [`TransportError::Deserialize`].
pub fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, TransportError> {
    serde_json::from_slice(bytes).map_err(TransportError::Deserialize)
}
