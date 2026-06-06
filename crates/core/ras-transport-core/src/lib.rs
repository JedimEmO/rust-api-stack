//! HTTP transport abstraction for generated Rust Agent Stack clients.
//!
//! Generated REST / JSON-RPC / File clients dispatch through the
//! [`HttpTransport`] trait instead of hard-coding `reqwest::Client`. Two impls
//! ship here: [`ReqwestTransport`] (production, `reqwest` feature) and
//! [`AxumTestTransport`] (in-process, wraps `axum_test::TestServer`, native +
//! `axum-test` feature) so clients can be exercised end-to-end against a server
//! with no sockets.
//!
//! # Relationship to `WebSocketTransport`
//!
//! This trait is the HTTP sibling of the `WebSocketTransport` abstraction in
//! `ras-jsonrpc-bidirectional-client`
//! (`crates/rpc/bidirectional/ras-jsonrpc-bidirectional-client/src/lib.rs`).
//! Both follow the same dyn-dispatch + conditional-`Send` pattern:
//! `#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]` together with the
//! [`TransportThreadBounds`] marker, so a single `Arc<dyn _>` works on both
//! native and wasm targets. They are intentionally separate: bidirectional RPC
//! is WebSocket (full-duplex frames), not request/response HTTP, and must not
//! be routed through `HttpTransport`.
//!
//! On wasm, request bodies cannot be streamed (the fetch API has no streaming
//! request body), so [`RequestBody::Stream`] is collected before sending;
//! response bodies still stream.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use serde::Serialize;
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

// --- Thread-bound marker, mirroring the bidirectional-client precedent. ---

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

// --- Byte stream alias, conditionally `Send`. ---

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

// --- The transport trait. ---

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

// --- Query / JSON helpers. ---

/// Serialize a single query value to its `application/x-www-form-urlencoded`
/// form for one `key`, returning zero or more `(key, value)` pairs.
///
/// Serializing one key/value at a time (rather than a whole struct) means a
/// `Vec<T>` produces repeated keys and `#[serde(rename = ...)]` enum variants
/// encode by their rename — matching reqwest's old `.query()` behavior exactly.
/// `Option::None` produces no pairs.
pub fn serialize_query_value<T: Serialize>(
    key: &str,
    value: &T,
) -> Result<Vec<(String, String)>, TransportError> {
    // We collect each scalar value into its own `(key, encoded_value)` entry so
    // that scalars produce one pair, sequences produce repeated keys, and
    // `None` produces nothing. Each scalar is rendered through
    // `serde_urlencoded` (one key=value pair) so enum `#[serde(rename)]`s and
    // numeric/bool formatting match reqwest's old `.query()` byte-for-byte.
    let mut collector = QueryValueCollector { values: Vec::new() };
    value
        .serialize(&mut collector)
        .map_err(|e| TransportError::Serialize(serde::ser::Error::custom(e.to_string())))?;
    Ok(collector
        .values
        .into_iter()
        .map(|v| (key.to_string(), v))
        .collect())
}

/// Serialize several `(key, value)` query parameters and join them into a
/// single query string (without a leading `?`). Empty result if no pairs.
///
/// The final byte-level encoding is delegated to `serde_urlencoded` (the same
/// crate reqwest's `.query()` uses internally), so the produced wire query
/// string matches the pre-transport reqwest client exactly — including its
/// `application/x-www-form-urlencoded` unreserved set (`*` stays raw, `~`
/// becomes `%7E`, space becomes `+`). Keys are emitted in order, so repeated
/// keys (from `Vec<T>`) preserve their sequence.
///
/// Returns [`TransportError::Serialize`] on encoding failure rather than
/// silently yielding an empty string — generated clients append the result
/// after a `?`/`&` separator, so a swallowed failure would send a different
/// (unfiltered) query than the caller asked for.
pub fn serialize_query_pairs(pairs: &[(String, String)]) -> Result<String, TransportError> {
    serde_urlencoded::to_string(pairs)
        .map_err(|e| TransportError::Serialize(serde::ser::Error::custom(e.to_string())))
}

/// Deserialize JSON bytes into `T`, mapping failures to
/// [`TransportError::Deserialize`].
pub fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, TransportError> {
    serde_json::from_slice(bytes).map_err(TransportError::Deserialize)
}

/// Percent-encode a value for safe interpolation into a single URL **path
/// segment**.
///
/// Generated clients substitute path parameters into a URL template by string
/// replacement (`/items/{id}` -> `/items/<value>`). Without encoding, a value
/// containing `/`, `?`, `#`, `%`, or control bytes could break out of its
/// segment and alter the request's path, query, or fragment (e.g. an `id` of
/// `../admin` or `x?role=admin`). Only RFC 3986 `unreserved` characters
/// (`ALPHA`/`DIGIT`/`-`/`.`/`_`/`~`) pass through unescaped; every other byte is
/// `%XX`-encoded. The result is what servers (e.g. axum's `Path` extractor)
/// percent-decode back to the original value.
pub fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
    out
}

/// Upper-case hex digit for a nibble (0..=15).
fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}

// --- internal helpers ---

/// Render one scalar value through `serde_urlencoded` and return the decoded
/// value string (the part after `=` of a single `k=v` pair), so enum renames
/// and scalar formatting match reqwest's old `.query()` exactly.
fn encode_scalar<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    use serde::ser::Error as _;
    // serde_urlencoded serializes a sequence of (key, value) tuples.
    let encoded = serde_urlencoded::to_string([("v", value)])
        .map_err(|e| serde_json::Error::custom(e.to_string()))?;
    // `encoded` looks like "v=<encoded-value>"; strip the "v=" prefix and decode.
    let raw = encoded.strip_prefix("v=").unwrap_or(&encoded);
    Ok(percent_decode(raw))
}

/// A serde `Serializer` that collects scalar query values, expanding sequences
/// into multiple values and treating `Option::None`/unit as empty.
struct QueryValueCollector {
    values: Vec<String>,
}

type QueryResult = Result<(), serde_json::Error>;

macro_rules! collect_scalar {
    ($method:ident, $ty:ty) => {
        fn $method(self, v: $ty) -> QueryResult {
            self.values.push(encode_scalar(&v)?);
            Ok(())
        }
    };
}

impl serde::Serializer for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = serde::ser::Impossible<(), serde_json::Error>;
    type SerializeMap = serde::ser::Impossible<(), serde_json::Error>;
    type SerializeStruct = serde::ser::Impossible<(), serde_json::Error>;
    type SerializeStructVariant = serde::ser::Impossible<(), serde_json::Error>;

    collect_scalar!(serialize_bool, bool);
    collect_scalar!(serialize_i8, i8);
    collect_scalar!(serialize_i16, i16);
    collect_scalar!(serialize_i32, i32);
    collect_scalar!(serialize_i64, i64);
    collect_scalar!(serialize_u8, u8);
    collect_scalar!(serialize_u16, u16);
    collect_scalar!(serialize_u32, u32);
    collect_scalar!(serialize_u64, u64);
    collect_scalar!(serialize_f32, f32);
    collect_scalar!(serialize_f64, f64);
    collect_scalar!(serialize_char, char);

    fn serialize_str(self, v: &str) -> QueryResult {
        self.values.push(v.to_string());
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> QueryResult {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "bytes are not a valid query value",
        ))
    }

    fn serialize_none(self) -> QueryResult {
        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> QueryResult {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> QueryResult {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> QueryResult {
        // Honor `#[serde(rename = ...)]`: `variant` is already the renamed form.
        self.values.push(variant.to_string());
        Ok(())
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "tuple variants are not valid query values",
        ))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom("maps are not valid query values"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "structs are not valid query values",
        ))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "struct variants are not valid query values",
        ))
    }
}

impl serde::ser::SerializeSeq for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_element<T>(&mut self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }
    fn end(self) -> QueryResult {
        Ok(())
    }
}

impl serde::ser::SerializeTuple for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_element<T>(&mut self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }
    fn end(self) -> QueryResult {
        Ok(())
    }
}

impl serde::ser::SerializeTupleStruct for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_field<T>(&mut self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }
    fn end(self) -> QueryResult {
        Ok(())
    }
}

/// Decode `application/x-www-form-urlencoded` text (`+` -> space, `%XX`).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h << 4) | l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
