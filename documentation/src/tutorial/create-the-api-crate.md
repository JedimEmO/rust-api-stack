# Create the API crate

The API crate owns the DTOs and the service declarations.
It should not own database connections, runtime configuration or concrete auth logic.

## Cargo features

The generated code refers to runtime crates by path,
so the API crate depends on them itself.
Its features forward to the macro crates' features and switch on the matching runtime dependencies:

```toml
[package]
name = "workspace-api"
edition = "2024"

[dependencies]
ras-rest-macro = { version = "0.4.0", default-features = false }
ras-file-macro = { version = "0.3.0", default-features = false }
ras-jsonrpc-bidirectional-macro = { version = "0.3.0", default-features = false }
ras-jsonrpc-types = "0.3.0"
ras-jsonrpc-bidirectional-types = "0.3.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = { version = "1.2", optional = true }
async-trait = { version = "0.1", optional = true }
tokio = { version = "1.0", features = ["sync"], optional = true }
ras-transport-core = { version = "0.2.0", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-auth-core = { version = "0.3.0", optional = true }
ras-rest-core = { version = "0.2.1", optional = true }
ras-file-core = { version = "0.2.1", optional = true }
ras-jsonrpc-bidirectional-server = { version = "0.4.0", optional = true }
ras-jsonrpc-bidirectional-client = { version = "0.4.0", optional = true }
axum = { version = "0.8", optional = true }
axum-extra = { version = "0.10", features = ["query"], optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
ras-jsonrpc-bidirectional-client = { version = "0.4.0", default-features = false, features = ["wasm"], optional = true }

[features]
default = []
server = [
    "ras-rest-macro/server",
    "ras-file-macro/server",
    "ras-jsonrpc-bidirectional-macro/server",
    "dep:schemars",
    "dep:async-trait",
    "dep:ras-auth-core",
    "dep:ras-rest-core",
    "dep:ras-file-core",
    "dep:ras-jsonrpc-bidirectional-server",
    "dep:axum",
    "dep:axum-extra",
]
client = [
    "ras-rest-macro/reqwest",
    "ras-file-macro/reqwest",
    "ras-jsonrpc-bidirectional-macro/client",
    "ras-transport-core/reqwest",
    "dep:ras-jsonrpc-bidirectional-client",
    "dep:tokio",
]
fs = ["ras-file-macro/fs", "ras-transport-core/fs"]
```

Server crates enable `workspace-api/server`,
and Rust or WASM clients enable `workspace-api/client`.
The macro crates' features decide which code is generated;
the API crate's features are a convenient way to select them from downstream crates.
Notice that the bidirectional client appears twice:
the native WebSocket client by default, and its `wasm` feature in browser builds.
The generated bidirectional client also uses `tokio::sync` channels,
which is why `client` turns on `tokio` with only the `sync` feature.

Enable `workspace-api/fs` for the native file-client helpers that stream upload
parts from disk.

One caveat applies when more than one crate in a workspace uses the same service macro.
Cargo unifies a macro crate's features across everything it builds in one invocation,
so a workspace build that enables `ras-rest-macro/server` for one crate
enables it for every crate that uses `ras-rest-macro`.
If that matters to you, set `feature_gated: true` in the service declaration.
The generated code is then wrapped in the API crate's own `server` and `client` features,
and the default `build()` constructors in a `reqwest` feature the API crate must declare.
The macro guides list the option.

## Source layout

Split the source by service boundary:

```text
src/
  lib.rs
  tasks.rs
  attachments.rs
  activity.rs
```

`lib.rs` re-exports the DTOs and the generated types:

```rust,ignore
pub mod activity;
pub mod attachments;
pub mod tasks;

pub use activity::*;
pub use attachments::*;
pub use tasks::*;
```

## The task service

`tasks.rs` holds the task DTOs from the previous chapter, a list response, and the REST declaration:

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

## The attachment service

`attachments.rs` uses the file macro,
because attachments should be streamed and validated before service code sees them:

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

## Activity notifications

`activity.rs` defines the live notifications.
The server sends typed events, and the client registers typed handlers for them:

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

The API crate describes the whole externally visible boundary of the application.
The server crate can focus on persistence, auth and business rules.
