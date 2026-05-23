# ras-file-macro Usage Documentation

The `ras-file-macro` crate provides a procedural macro for building type-safe file upload and download services with built-in authentication, native Rust clients, OpenAPI documents, and optional browser bindings.

## Table of Contents
- [Overview](#overview)
- [Installation](#installation)
- [Basic Usage](#basic-usage)
- [Macro Syntax](#macro-syntax)
- [Server Implementation](#server-implementation)
- [Client Usage](#client-usage)
- [TypeScript and WASM Clients](#typescript-and-wasm-clients)
- [Authentication and Permissions](#authentication-and-permissions)
- [Error Handling](#error-handling)
- [Advanced Features](#advanced-features)
- [File API Example](#file-api-example)

## Overview

The `file_service!` macro generates:
- A trait for implementing file operations
- Axum router with upload/download endpoints
- Native Rust client with streaming support
- OpenAPI 3.0 specification for TypeScript client generation
- Optional WASM client bindings for direct browser file APIs
- Built-in authentication and permission handling
- File-specific error types

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
ras-file-macro = "0.1.0"
ras-auth-core = "0.1.0"
serde = { version = "1.0", features = ["derive"] }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
axum = { version = "0.8", features = ["multipart"] }
tokio = { version = "1.0", features = ["full"] }
tokio-util = { version = "0.7", features = ["io"] }
serde_json = "1.0"
schemars = "1.0.0-alpha.20"
async-trait = "0.1"
thiserror = "2"
reqwest = { version = "0.12", features = ["json", "multipart", "stream"] }
uuid = { version = "1", features = ["v4"] }

# Optional: only when compiling direct WASM bindings or the generated
# browser-oriented client for wasm32.
[target.'cfg(target_arch = "wasm32")'.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json", "multipart"] }
wasm-bindgen = { version = "0.2", optional = true }
wasm-bindgen-futures = { version = "0.4", optional = true }
js-sys = { version = "0.3", optional = true }
web-sys = { version = "0.3", features = ["File", "FormData", "Blob"], optional = true }
serde-wasm-bindgen = { version = "0.6", optional = true }

[features]
default = []
wasm-client = ["wasm-bindgen", "wasm-bindgen-futures", "js-sys", "web-sys", "serde-wasm-bindgen"]
```

## Basic Usage

### 1. Define Your File Service

```rust
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;  // Required for OpenAPI generation

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct FileMetadata {
    pub id: String,
    pub filename: String,
    pub size: usize,
    pub content_type: String,
}

file_service!({
    service_name: FileStorage,
    base_path: "/api/files",
    openapi: true,  // Enable OpenAPI generation
    body_limit: 52428800,  // 50MB limit
    endpoints: [
        UPLOAD WITH_PERMISSIONS(["upload"]) upload() -> FileMetadata,
        DOWNLOAD UNAUTHORIZED download/{file_id: String}(),
    ]
});
```

### 2. Implement the Service Trait

```rust
use axum::extract::Multipart;
use axum::response::IntoResponse;
use async_trait::async_trait;
use ras_auth_core::AuthenticatedUser;
use std::path::PathBuf;

pub struct MyFileStorage {
    upload_dir: PathBuf,
}

impl MyFileStorage {
    pub fn new(upload_dir: impl Into<PathBuf>) -> Self {
        Self {
            upload_dir: upload_dir.into(),
        }
    }
}

#[async_trait]
impl FileStorageTrait for MyFileStorage {
    async fn upload(
        &self,
        _user: &AuthenticatedUser,
        mut multipart: Multipart,
    ) -> Result<FileMetadata, FileStorageFileError> {
        // Extract file from multipart
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?
        {
            if field.name() == Some("file") {
                let filename = field.file_name()
                    .ok_or(FileStorageFileError::InvalidFormat)?
                    .to_string();
                let content_type = field.content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let data = field.bytes().await
                    .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?;
                
                // Store file and return metadata
                let id = uuid::Uuid::new_v4().to_string();
                tokio::fs::create_dir_all(&self.upload_dir)
                    .await
                    .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?;
                tokio::fs::write(self.upload_dir.join(&id), &data)
                    .await
                    .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?;
                
                return Ok(FileMetadata {
                    id,
                    filename,
                    size: data.len(),
                    content_type,
                });
            }
        }
        Err(FileStorageFileError::InvalidFormat)
    }
    
    async fn download(
        &self,
        file_id: String,
    ) -> Result<impl IntoResponse, FileStorageFileError> {
        // Retrieve file
        let file = tokio::fs::File::open(self.upload_dir.join(&file_id)).await
            .map_err(|_| FileStorageFileError::NotFound)?;
        
        let stream = tokio_util::io::ReaderStream::new(file);
        let body = axum::body::Body::from_stream(stream);
        
        Ok(axum::response::Response::builder()
            .header("Content-Type", "application/octet-stream")
            .header("Content-Disposition", format!("attachment; filename=\"{}\"", file_id))
            .body(body)
            .unwrap())
    }
}
```

### 3. Set Up the Server

```rust
use axum::Router;

#[tokio::main]
async fn main() {
    let storage = MyFileStorage::new("./uploads");
    let auth_provider = MyAuthProvider::new();
    
    let file_router = FileStorageBuilder::new(storage)
        .auth_provider(auth_provider)
        .build();
    
    let app = Router::new()
        .merge(file_router);
    
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    
    axum::serve(listener, app).await.unwrap();
}
```

## Macro Syntax

```rust
file_service!({
    service_name: ServiceName,
    base_path: "/api/path",
    body_limit: 10485760,  // Optional, in bytes
    endpoints: [
        OPERATION AUTH_REQUIREMENT endpoint_name() -> ResponseType,
        OPERATION AUTH_REQUIREMENT endpoint_name/{param: Type}() -> ResponseType,
    ]
})
```

### Parameters

- **`service_name`**: Name of the service (used for trait and struct generation)
- **`base_path`**: Base URL path for all endpoints
- **`body_limit`** (optional): Maximum upload size in bytes
- **`endpoints`**: List of endpoints with their configuration

### Operations

- **`UPLOAD`**: Creates a POST endpoint accepting multipart/form-data
- **`DOWNLOAD`**: Creates a GET endpoint returning file data

### Authentication Requirements

- **`UNAUTHORIZED`**: No authentication required
- **`WITH_PERMISSIONS(["perm1", "perm2"])`**: Requires any of the listed permissions (OR logic)
- **`WITH_PERMISSIONS([["perm1", "perm2"]])`**: Requires all permissions in inner array (AND logic)

### Path Parameters

Dynamic path segments are supported:
```rust
DOWNLOAD UNAUTHORIZED download/{file_id: String}()
UPLOAD WITH_PERMISSIONS(["admin"]) upload_to/{folder: String}() -> FileMetadata
```

## Server Implementation

### Generated Trait

The macro generates a trait with methods for each endpoint:

```rust
#[async_trait]
pub trait FileStorageTrait: Send + Sync {
    // For UPLOAD endpoints
    async fn upload(
        &self,
        user: &AuthenticatedUser,  // Only if auth required
        param: Type,                // Path parameters if any
        multipart: Multipart
    ) -> Result<ResponseType, FileStorageFileError>;
    
    // For DOWNLOAD endpoints
    async fn download(
        &self,
        user: &AuthenticatedUser,  // Only if auth required
        param: Type,                // Path parameters if any
    ) -> Result<impl IntoResponse, FileStorageFileError>;
}
```

### Service Builder

Configure your service with authentication and observability:

```rust
let router = FileStorageBuilder::new(my_service)
    .auth_provider(auth_provider)
    .with_usage_tracker(|_headers, method, path| {
        // Track API usage
    })
    .with_duration_tracker(|method, path, duration| {
        // Track request duration
    })
    .build();
```

### Error Handling

A custom error enum is generated:

```rust
pub enum FileStorageFileError {
    NotFound,
    UploadFailed(String),
    DownloadFailed(String),
    InvalidFormat,
    FileTooLarge,
    Internal(String),
}
```

Each variant maps to appropriate HTTP status codes.

## Client Usage

### Native Rust Client

```rust
// Create client
let client = FileStorageClient::builder("http://localhost:3000")
    .timeout(Duration::from_secs(30))  // Optional
    .build()?;

// Set authentication
client.set_bearer_token(Some("validtoken"));

// Upload file
let metadata = client.upload(
    "./fixtures/report.pdf",
    None,  // Optional: override filename
    None   // Optional: override content type
).await?;

// Download file
let response = client.download("file-id-123").await?;
let bytes = response.bytes().await?;
```

### Client Builder Options

```rust
let client = FileStorageClient::builder("http://localhost:3000")
    .client(custom_reqwest_client)  // Optional: custom client
    .timeout(Duration::from_secs(60))  // Optional: request timeout
    .build()?;
```

## TypeScript and WASM Clients

### OpenAPI-Generated TypeScript Client

For browser apps, the recommended path is to generate a TypeScript fetch client from the OpenAPI document emitted by the Rust API crate.

Enable OpenAPI generation in the service definition:

```rust
file_service!({
    service_name: DocumentService,
    base_path: "/api/documents",
    openapi: true,
    endpoints: [
        UPLOAD UNAUTHORIZED upload() -> UploadResponse,
        UPLOAD WITH_PERMISSIONS(["user"]) upload_profile_picture() -> UploadResponse,
        DOWNLOAD UNAUTHORIZED download/{file_id: String}() -> (),
        DOWNLOAD WITH_PERMISSIONS(["user"]) download_secure/{file_id: String}() -> (),
    ]
});
```

Build the API or backend crate so the build script writes the OpenAPI document:

```bash
cargo check -p file-service-backend --locked
```

Generate a TypeScript fetch client from that OpenAPI document with your
preferred OpenAPI generator. The examples below assume the generated client
exports methods from `./generated`.

Use the generated functions directly:

```typescript
import {
  downloadDownloadFileId,
  uploadUpload,
} from './generated';

const file = new File(['hello from TypeScript'], 'hello.txt', {
  type: 'text/plain',
});

const baseUrl = 'http://localhost:3000/api/documents';

const uploaded = await uploadUpload({
  baseUrl,
  body: { file },
});
if (uploaded.error || !uploaded.data) throw uploaded.error;

const downloaded = await downloadDownloadFileId({
  baseUrl,
  path: { file_id: uploaded.data.file_id },
});
if (downloaded.error || !downloaded.data) throw downloaded.error;
```

The runnable usage sample lives in `examples/file-service-wasm/typescript-example`
and intentionally avoids a frontend framework or npm project.

### Optional WASM Bindings

If you need direct `wasm-bindgen` bindings instead of an OpenAPI-generated fetch client, add a `wasm-client` feature to your API crate and enable it when building for `wasm32`. The feature belongs to the API crate because the generated WASM module compiles inside that crate.

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
ras-file-macro = "0.1.0"
serde = { version = "1.0", features = ["derive"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json", "multipart"] }
wasm-bindgen = { version = "0.2", optional = true }
wasm-bindgen-futures = { version = "0.4", optional = true }
js-sys = { version = "0.3", optional = true }
web-sys = { version = "0.3", features = ["File", "Blob", "FormData"], optional = true }
serde-wasm-bindgen = { version = "0.6", optional = true }

[features]
default = []
wasm-client = ["wasm-bindgen", "wasm-bindgen-futures", "js-sys", "web-sys", "serde-wasm-bindgen"]
```

Build the module with `wasm-pack`:

```bash
wasm-pack build --target web --out-dir pkg --features wasm-client
```

The generated definitions expose a WASM client:

```typescript
// pkg/my_file_api.d.ts
export class WasmFileStorageClient {
  constructor(base_url: string);
  set_bearer_token(token?: string | null): void;
  upload(file: File): Promise<any>;
  download(file_id: string): Promise<any>;
}
```

Use it from browser code:

```typescript
import init, { WasmFileStorageClient } from './pkg/my_file_api';

// Initialize WASM module
await init();

// Create client
const client = new WasmFileStorageClient('http://localhost:3000');
client.set_bearer_token('validtoken');

// Upload file from input
const fileInput = document.getElementById('file-input') as HTMLInputElement;
const file = fileInput.files[0];
const metadata = await client.upload(file);
console.log('Uploaded:', metadata);

// Download file
const data = await client.download('file-id-123');
const blob = new Blob([data], { type: 'application/octet-stream' });
const url = URL.createObjectURL(blob);
window.open(url);
```

## Authentication and Permissions

### Integration with ras-auth-core

The macro integrates with the `ras-auth-core` authentication system:

```rust
use ras_identity_session::JwtAuthProvider;

// Use any cloneable AuthProvider implementation.
let auth_provider = JwtAuthProvider::new(session_service.clone());

let router = FileStorageBuilder::new(storage)
    .auth_provider(auth_provider)
    .build();
```

### Permission Checking

Permissions are automatically validated before calling your trait methods:

```rust
// Any listed permission (OR logic)
UPLOAD WITH_PERMISSIONS(["upload", "admin"]) upload() -> FileMetadata

// Multiple permissions required (AND logic)
UPLOAD WITH_PERMISSIONS([["upload", "verified"]]) upload_verified() -> FileMetadata

// Complex permission logic
UPLOAD WITH_PERMISSIONS([["admin"], ["upload", "premium"]]) special_upload() -> FileMetadata
// Requires: admin OR (upload AND premium)
```

### Bearer Token Handling

Tokens are extracted from the `Authorization` header:
```
Authorization: Bearer validtoken
```

## Error Handling

### Server-Side Errors

The generated error enum provides semantic error types:

```rust
match result {
    Err(FileStorageFileError::NotFound) => {
        // Handle 404
    }
    Err(FileStorageFileError::FileTooLarge) => {
        // Handle 413
    }
    Err(FileStorageFileError::UploadFailed(msg)) => {
        // Handle upload error
    }
    _ => {}
}
```

### Client-Side Errors

```typescript
try {
  await client.upload(file);
} catch (error) {
  if (error.message.includes('413')) {
    console.error('File too large');
  } else if (error.message.includes('401')) {
    console.error('Authentication required');
  }
}
```

## Advanced Features

### Streaming Large Files

The generated client supports streaming for efficient large file handling:

```rust
// Server implementation
async fn download(&self, file_id: String) -> Result<impl IntoResponse, FileStorageFileError> {
    let file = tokio::fs::File::open(path).await?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);
    
    Ok(Response::builder()
        .header("Content-Type", content_type)
        .body(body)
        .unwrap())
}

// Client usage - stream to file
let mut response = client.download("large-file").await?;
let mut file = tokio::fs::File::create("output.bin").await?;
while let Some(chunk) = response.chunk().await? {
    file.write_all(&chunk).await?;
}
```

### Custom Response Headers

```rust
async fn download(&self, file_id: String) -> Result<impl IntoResponse, _> {
    Ok(Response::builder()
        .header("Content-Type", "application/pdf")
        .header("Content-Disposition", "inline; filename=\"document.pdf\"")
        .header("Cache-Control", "public, max-age=3600")
        .body(body)
        .unwrap())
}
```

### Progress Tracking

```typescript
// TypeScript with progress
const formData = new FormData();
formData.append('file', file);

const xhr = new XMLHttpRequest();
xhr.upload.addEventListener('progress', (e) => {
  if (e.lengthComputable) {
    const percentComplete = (e.loaded / e.total) * 100;
    console.log(`Upload progress: ${percentComplete}%`);
  }
});

xhr.open('POST', `${baseUrl}/api/files/upload`);
xhr.setRequestHeader('Authorization', `Bearer ${token}`);
xhr.send(formData);
```

### Multipart Field Handling

```rust
async fn upload(
    &self,
    user: &AuthenticatedUser,
    mut multipart: Multipart,
) -> Result<FileMetadata, FileStorageFileError> {
    let mut file_data = None;
    let mut metadata = HashMap::new();
    
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?
    {
        let name = field.name().unwrap_or("");
        
        match name {
            "file" => {
                let filename = field.file_name().unwrap_or("unnamed").to_string();
                let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?;
                file_data = Some((filename, content_type, data));
            }
            "description" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| FileStorageFileError::UploadFailed(e.to_string()))?;
                metadata.insert("description".to_string(), value);
            }
            _ => {}
        }
    }
    
    let (filename, content_type, data) = file_data.ok_or(FileStorageFileError::InvalidFormat)?;
    Ok(FileMetadata {
        id: uuid::Uuid::new_v4().to_string(),
        filename,
        size: data.len(),
        content_type,
    })
}
```

## File API Example

Here is a compact file service example with authentication:

### API Definition

```rust
// file-api/src/lib.rs
use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UploadResponse {
    pub id: String,
    pub filename: String,
    pub size: usize,
    pub url: String,
}

file_service!({
    service_name: DocumentService,
    base_path: "/api/documents",
    openapi: true,
    body_limit: 104857600, // 100 MB
    endpoints: [
        UPLOAD UNAUTHORIZED upload() -> UploadResponse,
        UPLOAD WITH_PERMISSIONS(["user"]) upload_secure() -> UploadResponse,
        DOWNLOAD UNAUTHORIZED download/{file_id: String}(),
        DOWNLOAD WITH_PERMISSIONS(["user"]) download_secure/{file_id: String}(),
    ]
});
```

### Backend Implementation

```rust
// backend/src/main.rs
use axum::Router;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::HashSet;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::{file_service::FileServiceImpl, storage::FileStorage};

#[derive(Clone)]
struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token != "validtoken" {
                return Err(AuthError::InvalidToken);
            }

            Ok(AuthenticatedUser {
                user_id: "demo-user".to_string(),
                permissions: HashSet::from(["user".to_string()]),
                metadata: None,
            })
        })
    }
}

#[tokio::main]
async fn main() {
    // Initialize storage and auth
    let storage = Arc::new(FileStorage::new("./uploads"));
    let service = FileServiceImpl::new(storage);
    let auth_provider = DemoAuthProvider;
    
    // Build file service
    let file_router = DocumentServiceBuilder::new(service)
        .auth_provider(auth_provider)
        .with_usage_tracker(|_headers, method, path| {
            println!("File API accessed: {} {}", method, path);
        })
        .build();
    
    // Create app with CORS
    let app = Router::new()
        .merge(file_router)
        .layer(CorsLayer::permissive());
    
    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    
    println!("File service running on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
```

### Frontend Usage (TypeScript)

```typescript
import {
  downloadDownloadSecureFileId,
  uploadUploadProfilePicture,
} from './generated';

const baseUrl = 'http://localhost:3000/api/documents';

export async function uploadSelectedFile(file: File, token: string) {
  const { data, error } = await uploadUploadProfilePicture({
    baseUrl,
    headers: { Authorization: `Bearer ${token}` },
    body: { file },
  });

  if (error || !data) {
    throw new Error(`Upload failed: ${JSON.stringify(error)}`);
  }

  return data;
}

export async function downloadPrivateFile(fileId: string, token: string) {
  const { data, error } = await downloadDownloadSecureFileId({
    baseUrl,
    headers: { Authorization: `Bearer ${token}` },
    path: { file_id: fileId },
  });

  if (error || !data) {
    throw new Error(`Download failed: ${JSON.stringify(error)}`);
  }

  return data;
}
```

## Best Practices

1. **File Size Limits**: Always set appropriate `body_limit` values to prevent abuse
2. **Content Type Validation**: Validate file types in your implementation
3. **OpenAPI Generation**: Enable `openapi: true` for TypeScript client generation
4. **Type Definitions**: Add `JsonSchema` derive to all types used in endpoints
5. **Virus Scanning**: Consider integrating virus scanning for uploaded files
6. **Storage Strategy**: Use cloud storage (S3, etc.) for production deployments
7. **Cleanup**: Implement file retention policies and cleanup routines
8. **Monitoring**: Use the callback functions to track usage and performance
9. **Security**: Always validate permissions and sanitize file names
10. **CORS**: Configure CORS appropriately for your frontend domains

## Troubleshooting

### Common Issues

1. **"File too large" errors**: Check `body_limit` configuration
2. **CORS errors**: Ensure CORS is configured on the server
3. **Authentication failures**: Verify token format and auth provider setup
4. **OpenAPI generation errors**: Ensure `JsonSchema` is derived for all types
5. **TypeScript generation errors**: Check OpenAPI spec is valid JSON
6. **WASM build errors**: Ensure `wasm-pack` is installed and features are enabled
7. **TypeScript type errors**: Regenerate client after API changes

### Debug Tips

- Enable debug logging in your auth provider
- Use browser dev tools to inspect multipart requests
- Check server logs for detailed error messages
- Verify file permissions on upload directory

This guide covers the core pieces needed to implement a file service with `ras-file-macro`. Production deployments still need project-specific storage, retention, scanning, authentication, and CORS decisions.
