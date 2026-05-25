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

    pub fn attachment(self, filename: impl AsRef<str>) -> FileResult<Self> {
        let escaped = filename.as_ref().replace('"', "");
        let value = HeaderValue::from_str(&format!("attachment; filename=\"{escaped}\""))
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
