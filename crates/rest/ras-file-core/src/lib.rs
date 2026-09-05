//! Core runtime types for generated file upload and download services.

use std::{pin::Pin, time::SystemTime};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use ras_auth_core::AuthenticatedUser;
use thiserror::Error;

pub use bytes;
pub use futures_core;
pub use futures_util;
pub use http;
pub use ras_auth_core::sanitize_log_detail;
pub use tracing;

/// Result type used by generated file services.
pub type FileResult<T> = Result<T, FileError>;

/// Stream of byte chunks used by file upload and download abstractions.
pub type FileByteStream<'a> = Pin<Box<dyn Stream<Item = Result<Bytes, FileError>> + Send + 'a>>;

/// Owned stream for download responses.
pub type OwnedFileByteStream = FileByteStream<'static>;

/// Errors surfaced by generated file services.
#[derive(Debug, Error)]
pub enum FileError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Authentication required")]
    Unauthorized,
    #[error("Forbidden")]
    Forbidden,
    #[error("Unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("Payload too large")]
    PayloadTooLarge,
    #[error("File not found")]
    NotFound,
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Precondition failed: {0}")]
    PreconditionFailed(String),
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("Download failed: {0}")]
    DownloadFailed(String),
    #[error("Handler contract violation: {0}")]
    HandlerContract(String),
    #[error("Internal server error")]
    Internal,
}

impl FileError {
    /// HTTP status code associated with this error.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) | Self::HandlerContract(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            Self::UploadFailed(_) => StatusCode::BAD_REQUEST,
            Self::DownloadFailed(_) | Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Sanitized client-facing message.
    pub fn client_message(&self) -> String {
        match self {
            Self::BadRequest(message)
            | Self::UnsupportedMediaType(message)
            | Self::Conflict(message)
            | Self::PreconditionFailed(message)
            | Self::HandlerContract(message) => message.clone(),
            Self::Unauthorized => "Authentication required".to_string(),
            Self::Forbidden => "Forbidden".to_string(),
            Self::PayloadTooLarge => "Payload too large".to_string(),
            Self::NotFound => "File not found".to_string(),
            Self::UploadFailed(_) => "Upload failed".to_string(),
            Self::DownloadFailed(_) | Self::Internal => "Internal server error".to_string(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn unsupported_media_type(message: impl Into<String>) -> Self {
        Self::UnsupportedMediaType(message.into())
    }

    pub fn upload_failed(message: impl Into<String>) -> Self {
        Self::UploadFailed(message.into())
    }

    pub fn download_failed(message: impl Into<String>) -> Self {
        Self::DownloadFailed(message.into())
    }

    pub fn handler_contract(message: impl Into<String>) -> Self {
        Self::HandlerContract(message.into())
    }
}

/// Request metadata passed to file-service handlers.
pub struct FileRequestContext<'a> {
    pub method: &'static str,
    pub request_path: &'a str,
    pub matched_path: &'static str,
    pub headers: &'a HeaderMap,
    pub user: Option<&'a AuthenticatedUser>,
}

impl<'a> FileRequestContext<'a> {
    pub fn new(
        method: &'static str,
        request_path: &'a str,
        matched_path: &'static str,
        headers: &'a HeaderMap,
        user: Option<&'a AuthenticatedUser>,
    ) -> Self {
        Self {
            method,
            request_path,
            matched_path,
            headers,
            user,
        }
    }

    pub fn range(&self) -> Option<&'a str> {
        self.headers.get(header::RANGE)?.to_str().ok()
    }

    pub fn if_none_match(&self) -> Option<&'a str> {
        self.headers.get(header::IF_NONE_MATCH)?.to_str().ok()
    }

    pub fn if_match(&self) -> Option<&'a str> {
        self.headers.get(header::IF_MATCH)?.to_str().ok()
    }
}

/// Streaming upload file part passed to service implementations.
pub struct IncomingFile<'a> {
    field_name: String,
    file_name: Option<String>,
    content_type: Option<String>,
    headers: HeaderMap,
    limit: u64,
    bytes_read: u64,
    finished: bool,
    stream: FileByteStream<'a>,
}

impl<'a> IncomingFile<'a> {
    pub fn new(
        field_name: impl Into<String>,
        file_name: Option<String>,
        content_type: Option<String>,
        headers: HeaderMap,
        limit: u64,
        stream: FileByteStream<'a>,
    ) -> Self {
        Self {
            field_name: field_name.into(),
            file_name,
            content_type,
            headers,
            limit,
            bytes_read: 0,
            finished: false,
            stream,
        }
    }

    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub async fn next_chunk(&mut self) -> FileResult<Option<Bytes>> {
        if self.finished {
            return Ok(None);
        }

        let Some(chunk) = self.stream.next().await.transpose()? else {
            self.finished = true;
            return Ok(None);
        };

        let next_total = self
            .bytes_read
            .checked_add(chunk.len() as u64)
            .ok_or(FileError::PayloadTooLarge)?;

        if next_total > self.limit {
            return Err(FileError::PayloadTooLarge);
        }

        self.bytes_read = next_total;
        Ok(Some(chunk))
    }

    pub async fn drain(&mut self) -> FileResult<()> {
        while self.next_chunk().await?.is_some() {}
        Ok(())
    }
}

/// Summary of accepted upload fields.
#[derive(Debug, Clone, Default)]
pub struct UploadSummary {
    pub total_parts: usize,
    pub total_bytes: u64,
    pub fields: Vec<UploadFieldSummary>,
}

impl UploadSummary {
    pub fn record(&mut self, field_name: impl Into<String>, bytes: u64) {
        self.total_parts += 1;
        self.total_bytes += bytes;
        self.fields.push(UploadFieldSummary {
            field_name: field_name.into(),
            bytes,
        });
    }
}

#[derive(Debug, Clone)]
pub struct UploadFieldSummary {
    pub field_name: String,
    pub bytes: u64,
}

/// JSON response returned by upload lifecycle finish handlers.
#[derive(Debug, Clone)]
pub struct JsonResponse<T> {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: T,
}

impl<T> JsonResponse<T> {
    pub fn ok(body: T) -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body,
        }
    }

    pub fn created(body: T) -> Self {
        Self {
            status: StatusCode::CREATED,
            headers: HeaderMap::new(),
            body,
        }
    }

    pub fn with_status(status: StatusCode, body: T) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body,
        }
    }

    pub fn header(mut self, name: header::HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn into_parts(self) -> (StatusCode, HeaderMap, T) {
        (self.status, self.headers, self.body)
    }
}

impl<T> From<T> for JsonResponse<T> {
    fn from(body: T) -> Self {
        Self::ok(body)
    }
}

/// Download body data.
pub enum DownloadBody {
    Empty,
    Bytes(Bytes),
    Stream(OwnedFileByteStream),
}

/// Streaming download response returned by download handlers.
pub struct DownloadResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: DownloadBody,
}

impl DownloadResponse {
    pub fn empty(status: StatusCode) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body: DownloadBody::Empty,
        }
    }

    pub fn bytes(bytes: impl Into<Bytes>) -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: DownloadBody::Bytes(bytes.into()),
        }
    }

    pub fn stream(stream: OwnedFileByteStream) -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: DownloadBody::Stream(stream),
        }
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: header::HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn content_type(self, value: impl AsRef<str>) -> FileResult<Self> {
        let value = HeaderValue::from_str(value.as_ref())
            .map_err(|e| FileError::bad_request(format!("invalid content type: {e}")))?;
        Ok(self.header(header::CONTENT_TYPE, value))
    }

    pub fn content_length(self, value: u64) -> FileResult<Self> {
        let value = HeaderValue::from_str(&value.to_string())
            .map_err(|e| FileError::bad_request(format!("invalid content length: {e}")))?;
        Ok(self.header(header::CONTENT_LENGTH, value))
    }

    /// Set `Content-Disposition: attachment` for `filename`.
    ///
    /// Control characters are stripped, then the name is emitted twice per
    /// RFC 6266: as a quoted-string `filename="..."` with `"` and `\`
    /// backslash-escaped and non-ASCII replaced by `_` for legacy agents, and
    /// as an RFC 5987 `filename*=UTF-8''...` percent-encoded parameter that
    /// preserves the original Unicode name for modern agents. Neither form can
    /// break out of the header value.
    ///
    /// Path separators are *not* stripped here (browsers only use the final
    /// component when saving); use [`sanitize_filename`] first if the name
    /// came from an untrusted upload and you want a single path component.
    pub fn attachment(self, filename: impl AsRef<str>) -> FileResult<Self> {
        let name: String = filename
            .as_ref()
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let quoted = quote_disposition_filename(&name);
        let encoded = rfc5987_encode(&name);
        let value = HeaderValue::from_str(&format!(
            "attachment; filename=\"{quoted}\"; filename*=UTF-8''{encoded}"
        ))
        .map_err(|e| FileError::bad_request(format!("invalid filename: {e}")))?;
        Ok(self.header(header::CONTENT_DISPOSITION, value))
    }

    pub fn etag(self, value: impl AsRef<str>) -> FileResult<Self> {
        let value = HeaderValue::from_str(value.as_ref())
            .map_err(|e| FileError::bad_request(format!("invalid etag: {e}")))?;
        Ok(self.header(header::ETAG, value))
    }

    pub fn last_modified(self, value: HeaderValue) -> Self {
        self.header(header::LAST_MODIFIED, value)
    }

    pub fn last_modified_system_time(self, _value: SystemTime) -> Self {
        self
    }
}

/// Maximum length in bytes of a sanitized filename.
pub const MAX_FILENAME_BYTES: usize = 255;

/// Reduce an untrusted filename (e.g. a multipart `filename=` parameter) to a
/// single safe path component.
///
/// * Keeps only the final path component, splitting on both `/` and `\`.
/// * Strips NUL and all other control characters (including DEL).
/// * Rejects names that consist only of dots (`.`, `..`, `...`).
/// * Falls back to `"upload"` when nothing usable remains.
/// * Truncates to [`MAX_FILENAME_BYTES`] on a UTF-8 char boundary.
///
/// Unicode is preserved; this is not a transliteration step. Handlers that
/// use the name on a filesystem should still combine it with a trusted
/// directory and never trust it as a full path.
pub fn sanitize_filename(raw: &str) -> String {
    let last_component = raw.rsplit(['/', '\\']).next().unwrap_or("");

    let mut name: String = last_component.chars().filter(|c| !c.is_control()).collect();

    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        return "upload".to_string();
    }
    if trimmed.len() != name.len() {
        name = trimmed.to_string();
    }

    if name.len() > MAX_FILENAME_BYTES {
        let mut cut = MAX_FILENAME_BYTES;
        while !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name.truncate(cut);
        // Truncation can leave a dots-only or empty remainder (e.g. many
        // leading dots); re-check so the result is still a usable name.
        if name.is_empty() || name.chars().all(|c| c == '.') {
            return "upload".to_string();
        }
    }

    name
}

/// Build the legacy `filename="..."` quoted-string form: `"` and `\` are
/// backslash-escaped and any non-ASCII or control byte is replaced by `_` so
/// the value is a valid `HeaderValue` and cannot break out of the quotes.
fn quote_disposition_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push('_'),
        }
    }
    out
}

/// Percent-encode `name` per RFC 5987 (`attr-char` is left as-is, everything
/// else — including `"`, `\`, `;`, space and all non-ASCII bytes — is encoded).
fn rfc5987_encode(name: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(name.len() * 3);
    for byte in name.bytes() {
        let is_attr_char = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            );
        if is_attr_char {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disposition(name: &str) -> String {
        DownloadResponse::empty(StatusCode::OK)
            .attachment(name)
            .expect("attachment header")
            .headers
            .get(header::CONTENT_DISPOSITION)
            .expect("content-disposition set")
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn f1_sanitize_filename_takes_final_component_of_traversal_names() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/etc/passwd"), "passwd");
        assert_eq!(
            sanitize_filename("..\\..\\windows\\system.ini"),
            "system.ini"
        );
        assert_eq!(
            sanitize_filename("C:\\Users\\victim\\report.pdf"),
            "report.pdf"
        );
        assert_eq!(sanitize_filename("mixed/sep\\name.txt"), "name.txt");
        // Trailing separator leaves nothing usable.
        assert_eq!(sanitize_filename("dir/"), "upload");
        assert_eq!(sanitize_filename("dir\\"), "upload");
    }

    #[test]
    fn f1_sanitize_filename_rejects_dot_only_and_empty_names() {
        assert_eq!(sanitize_filename(""), "upload");
        assert_eq!(sanitize_filename("   "), "upload");
        assert_eq!(sanitize_filename("."), "upload");
        assert_eq!(sanitize_filename(".."), "upload");
        assert_eq!(sanitize_filename("..."), "upload");
        assert_eq!(sanitize_filename("a/.."), "upload");
        // A dotfile with a real name is fine.
        assert_eq!(sanitize_filename(".env"), ".env");
    }

    #[test]
    fn f1_sanitize_filename_strips_control_characters() {
        assert_eq!(sanitize_filename("evil\0.txt"), "evil.txt");
        assert_eq!(sanitize_filename("a\r\nb.txt"), "ab.txt");
        assert_eq!(sanitize_filename("\x7fdel.txt"), "del.txt");
        assert_eq!(sanitize_filename("\0\0"), "upload");
        // Leading/trailing whitespace is trimmed; interior kept.
        assert_eq!(sanitize_filename("  my file.txt  "), "my file.txt");
    }

    #[test]
    fn f1_sanitize_filename_preserves_unicode_and_caps_on_char_boundary() {
        assert_eq!(sanitize_filename("résumé.pdf"), "résumé.pdf");
        assert_eq!(sanitize_filename("日本語/ファイル.txt"), "ファイル.txt");

        // 'é' is 2 bytes; 130 of them = 260 bytes, cap is 255 bytes (odd) so
        // the cut must land on a boundary at 254.
        let long = "é".repeat(130);
        let out = sanitize_filename(&long);
        assert_eq!(out.len(), 254);
        assert!(out.chars().all(|c| c == 'é'));

        let ascii = "a".repeat(300);
        assert_eq!(sanitize_filename(&ascii).len(), MAX_FILENAME_BYTES);
        assert_eq!(sanitize_filename(&"a".repeat(255)).len(), 255);
    }

    #[test]
    fn f1_attachment_escapes_quotes_and_backslashes() {
        let value = disposition(r#"a"b\c.txt"#);
        assert_eq!(
            value,
            r#"attachment; filename="a\"b\\c.txt"; filename*=UTF-8''a%22b%5Cc.txt"#
        );
        // Injection attempt: cannot terminate the quoted string / add params.
        let value = disposition("x\"; filename=\"evil.exe");
        assert!(value.starts_with("attachment; filename=\"x\\\"; filename=\\\"evil.exe\";"));
        assert!(value.ends_with("filename*=UTF-8''x%22%3B%20filename%3D%22evil.exe"));
    }

    #[test]
    fn f1_attachment_of_sanitized_traversal_name_is_single_component() {
        assert_eq!(
            disposition(&sanitize_filename("../../etc/passwd")),
            "attachment; filename=\"passwd\"; filename*=UTF-8''passwd"
        );
        assert_eq!(
            disposition(&sanitize_filename("..\\..\\boot.ini")),
            "attachment; filename=\"boot.ini\"; filename*=UTF-8''boot.ini"
        );
        // Unsanitized, separators are escaped/encoded but never break the header.
        assert_eq!(
            disposition("..\\..\\boot.ini"),
            "attachment; filename=\"..\\\\..\\\\boot.ini\"; filename*=UTF-8''..%5C..%5Cboot.ini"
        );
    }

    #[test]
    fn f1_attachment_encodes_unicode_with_rfc5987_and_ascii_fallback() {
        assert_eq!(
            disposition("résumé.pdf"),
            "attachment; filename=\"r_sum_.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"
        );
        // Control chars are stripped before the header is built, so the header
        // value is always valid.
        assert_eq!(
            disposition("a\r\nSet-Cookie: x.txt"),
            "attachment; filename=\"aSet-Cookie: x.txt\"; filename*=UTF-8''aSet-Cookie%3A%20x.txt"
        );
    }
}
