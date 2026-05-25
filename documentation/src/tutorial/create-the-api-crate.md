# 2. Create The API Crate

The API crate owns DTOs and service declarations. It should not own database
connections, runtime configuration, or concrete auth logic.

## Cargo Features

Use API-crate features to forward macro-crate features and enable the runtime
dependencies referenced by generated transport code:

```toml
[package]
name = "workspace-api"
edition = "2024"

[dependencies]
ras-rest-macro = { version = "0.2.1", default-features = false }
ras-file-macro = { version = "0.1.0", default-features = false }
ras-jsonrpc-bidirectional-macro = { version = "0.1.0", default-features = false }
serde = { version = "1.0", features = ["derive"] }
schemars = { version = "1.0.0-alpha.20", optional = true }
serde_json = { version = "1.0", optional = true }
async-trait = { version = "0.1", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-auth-core = { version = "0.1.0", optional = true }
ras-rest-core = { version = "0.1.1", optional = true }
ras-file-core = { version = "0.1.0", optional = true }
ras-jsonrpc-bidirectional-server = { version = "0.1.0", optional = true }
axum = { version = "0.8", optional = true }
axum-extra = { version = "0.10", optional = true }
tokio = { version = "1.0", optional = true }
tokio-util = { version = "0.7", optional = true }
reqwest = { version = "0.12", optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json", "multipart"], optional = true }

[features]
default = []
server = [
    "ras-rest-macro/server",
    "ras-file-macro/server",
    "ras-jsonrpc-bidirectional-macro/server",
    "dep:schemars",
    "dep:serde_json",
    "dep:async-trait",
    "dep:ras-auth-core",
    "dep:ras-rest-core",
    "dep:ras-file-core",
    "dep:ras-jsonrpc-bidirectional-server",
    "dep:axum",
    "dep:axum-extra",
    "dep:tokio",
]
client = [
    "ras-rest-macro/client",
    "ras-file-macro/client",
    "ras-jsonrpc-bidirectional-macro/client",
    "dep:reqwest",
    "dep:tokio",
    "dep:tokio-util",
]
```

Server crates enable `workspace-api/server`. Rust or WASM clients enable
`workspace-api/client`. The proc macro crate features decide which generated
code is emitted; the API-crate features are just a convenient way to select
those macro features from downstream crates.

## Source Layout

Split by service boundary:

```text
src/
  lib.rs
  tasks.rs
  attachments.rs
  activity.rs
```

`lib.rs` re-exports the generated surface:

```rust,ignore
pub mod activity;
pub mod attachments;
pub mod tasks;

pub use activity::*;
pub use attachments::*;
pub use tasks::*;
```

## Task Service

`tasks.rs` contains DTOs and the REST declaration:

```rust,ignore
use ras_rest_macro::rest_service;
#[cfg(feature = "server")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(JsonSchema))]
pub struct TasksResponse {
    pub tasks: Vec<Task>,
}

rest_service!({
    service_name: TaskService,
    base_path: "/api/v1",
    openapi: true,
    endpoints: [
        GET WITH_PERMISSIONS(["project:read"]) projects/{project_id: String}/tasks() -> TasksResponse,
        POST WITH_PERMISSIONS(["task:write"]) projects/{project_id: String}/tasks(CreateTaskRequest) -> Task,
    ]
});
```

## Attachment Service

`attachments.rs` uses the file macro because attachments should be streamed and
validated before service code sees them:

```rust,ignore
use ras_file_macro::file_service;
#[cfg(feature = "server")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(JsonSchema))]
pub struct AttachmentUploadResponse {
    pub attachment_id: String,
    pub file_name: String,
    pub size: u64,
}

file_service!({
    service_name: AttachmentService,
    base_path: "/api/v1/attachments",
    openapi: true,
    endpoints: [
        UPLOAD WITH_PERMISSIONS(["attachment:write"]) tasks/{task_id: String}/upload multipart {
            max_total_bytes: 52428800,
            reject_unknown_fields: true,
            parts: [
                file file {
                    required: true,
                    max_count: 1,
                    max_bytes: 52428800,
                    filename: required,
                },
            ],
        } -> AttachmentUploadResponse,

        DOWNLOAD WITH_PERMISSIONS(["attachment:read"]) download/{attachment_id: String} {
            content_types: ["application/octet-stream"],
            ranges: true,
        },
    ]
});
```

## Activity Notifications

`activity.rs` defines live notifications. The server sends typed events and the
client registers typed handlers:

```rust,ignore
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChanged {
    pub task_id: String,
    pub project_id: String,
}

jsonrpc_bidirectional_service!({
    service_name: ActivityService,
    client_to_server: [
        WITH_PERMISSIONS(["project:read"]) subscribe_project(String) -> (),
    ],
    server_to_client: [
        task_changed(TaskChanged),
    ],
    server_to_client_calls: [
    ]
});
```

The API crate now describes the externally visible application boundary. The
server crate can focus on persistence, auth, and business rules.
