//! Hand-rolled streaming `multipart/form-data` builder (RFC 7578).
//!
//! Replaces `reqwest::multipart::Form`. The body is assembled as a flattened
//! stream of segments so that file/stream parts never have to be buffered in
//! memory on native targets: each part contributes a header segment, the value
//! (which may itself be a stream), and a trailing CRLF; the whole thing is
//! `futures_util::stream::iter(segments).flatten()`.

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::{self};
#[cfg(not(target_arch = "wasm32"))]
use futures_util::stream::BoxStream;
#[cfg(target_arch = "wasm32")]
use futures_util::stream::LocalBoxStream;

use crate::error::TransportError;
use crate::request::RequestBody;
use crate::{ByteStream, byte_stream_from};

/// A single multipart part, captured before framing.
struct Part {
    /// Field name; escaped for the `Content-Disposition` quoted-string at framing
    /// time by [`escape_disposition_param`].
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    value: PartValue,
}

enum PartValue {
    Bytes(Bytes),
    // Only constructed via the `fs`-gated stream/file part methods; the `build`
    // match arm always references it.
    #[allow(dead_code)]
    Stream(ByteStream),
}

/// Builder for a streaming `multipart/form-data` body.
///
/// Call [`MultipartBuilder::build`] to obtain the streaming body and the
/// `Content-Type` header value (including the generated boundary).
pub struct MultipartBuilder {
    boundary: String,
    parts: Vec<Part>,
}

impl MultipartBuilder {
    /// Create a builder with an auto-generated boundary.
    pub fn new() -> Self {
        MultipartBuilder {
            boundary: generate_boundary(),
            parts: Vec::new(),
        }
    }

    /// Create a builder with an explicit boundary (used by tests for
    /// deterministic wire output).
    pub fn with_boundary(boundary: impl Into<String>) -> Self {
        MultipartBuilder {
            boundary: boundary.into(),
            parts: Vec::new(),
        }
    }

    /// The full `Content-Type` header value, including the boundary.
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    /// Add a plain text field.
    pub fn text(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.parts.push(Part {
            name: name.into(),
            filename: None,
            content_type: None,
            value: PartValue::Bytes(Bytes::from(value.into().into_bytes())),
        });
        self
    }

    /// Add a JSON field (serialized, `Content-Type: application/json`).
    pub fn json<T: serde::Serialize>(
        mut self,
        name: impl Into<String>,
        value: &T,
    ) -> Result<Self, TransportError> {
        let bytes = serde_json::to_vec(value).map_err(TransportError::Serialize)?;
        self.parts.push(Part {
            name: name.into(),
            filename: None,
            content_type: Some("application/json".to_string()),
            value: PartValue::Bytes(Bytes::from(bytes)),
        });
        Ok(self)
    }

    /// Add a binary part with an explicit filename and content type.
    pub fn bytes_part(
        mut self,
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: impl Into<Bytes>,
    ) -> Self {
        self.parts.push(Part {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: Some(content_type.into()),
            value: PartValue::Bytes(bytes.into()),
        });
        self
    }

    /// Add a streaming part directly from a [`ByteStream`].
    #[cfg(all(feature = "fs", not(target_arch = "wasm32")))]
    pub fn stream_part(
        mut self,
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        stream: ByteStream,
    ) -> Self {
        self.parts.push(Part {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: Some(content_type.into()),
            value: PartValue::Stream(stream),
        });
        self
    }

    /// Add a part streamed from a file on disk.
    ///
    /// The `tokio::fs::File` -> `ReaderStream` conversion lives here (under the
    /// `fs` feature) so consumers need not depend on `tokio-util` themselves.
    #[cfg(all(feature = "fs", not(target_arch = "wasm32")))]
    pub async fn file_path(
        self,
        name: impl Into<String>,
        content_type: impl Into<String>,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Self, TransportError> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| TransportError::Body(e.to_string()))?;
        let reader = tokio_util::io::ReaderStream::new(file);
        let stream: ByteStream = byte_stream_from(
            reader.map(|res| res.map_err(|e| TransportError::Body(e.to_string()))),
        );
        Ok(self.stream_part(name, filename, content_type, stream))
    }

    /// Build the streaming body and its `Content-Type` value.
    pub fn build(self) -> (RequestBody, String) {
        let content_type = self.content_type();
        let boundary = self.boundary;

        // Each part is flattened into: [header segment, value stream, CRLF segment].
        // A final closing-boundary segment is appended after all parts.
        let mut segments: Vec<ByteStream> = Vec::new();
        for part in self.parts {
            let header = part_header(&boundary, &part);
            segments.push(byte_stream_from(stream::once(async move {
                Ok::<Bytes, TransportError>(Bytes::from(header.into_bytes()))
            })));
            match part.value {
                PartValue::Bytes(b) => {
                    segments.push(byte_stream_from(stream::once(async move { Ok(b) })));
                }
                PartValue::Stream(s) => {
                    segments.push(s);
                }
            }
            segments.push(byte_stream_from(stream::once(async move {
                Ok(Bytes::from_static(b"\r\n"))
            })));
        }
        let trailer = format!("--{boundary}--\r\n");
        segments.push(byte_stream_from(stream::once(async move {
            Ok(Bytes::from(trailer.into_bytes()))
        })));

        let body_stream = flatten_segments(segments);
        (RequestBody::Stream(body_stream), content_type)
    }
}

impl Default for MultipartBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape a `Content-Disposition` quoted-string parameter so that a `"`, CR, or
/// LF in a (potentially user-supplied) field name or filename cannot terminate
/// the quoted string or inject extra header lines / corrupt the multipart frame.
/// Mirrors the percent-escaping browsers and `reqwest::multipart` apply.
fn escape_disposition_param(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Build the RFC 7578 header block for a part (boundary line + disposition +
/// optional content type + the blank line that ends the header block).
fn part_header(boundary: &str, part: &Part) -> String {
    let mut s = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"",
        escape_disposition_param(&part.name)
    );
    if let Some(filename) = &part.filename {
        s.push_str(&format!(
            "; filename=\"{}\"",
            escape_disposition_param(filename)
        ));
    }
    s.push_str("\r\n");
    if let Some(ct) = &part.content_type {
        s.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    s.push_str("\r\n");
    s
}

/// Flatten the per-part segment streams into one body stream, respecting the
/// conditional `Send` bound on [`ByteStream`].
#[cfg(not(target_arch = "wasm32"))]
fn flatten_segments(segments: Vec<ByteStream>) -> ByteStream {
    let flat: BoxStream<'static, Result<Bytes, TransportError>> =
        stream::iter(segments).flatten().boxed();
    Box::pin(flat)
}

#[cfg(target_arch = "wasm32")]
fn flatten_segments(segments: Vec<ByteStream>) -> ByteStream {
    let flat: LocalBoxStream<'static, Result<Bytes, TransportError>> =
        stream::iter(segments).flatten().boxed_local();
    Box::pin(flat)
}

/// Generate a random-ish boundary. Uses the address of a stack allocation and
/// a monotonic counter to avoid pulling in `rand`.
fn generate_boundary() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("ras-boundary-{nanos:x}-{n:x}")
}
