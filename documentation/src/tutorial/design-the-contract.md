# 1. Design The Contract

Start with workflows, not with Axum routes or database tables. Write down what
clients need to do and which operations need identity.

For the team workspace, the first pass looks like this:

| Workflow | Macro | Reason |
| --- | --- | --- |
| list projects, read tasks, create tasks | [`rest_service!`](../macros/rest-service.md) | conventional JSON resources, OpenAPI, browser clients |
| upload and download task attachments | [`file_service!`](../macros/file-service.md) | streaming, multipart validation, early auth checks |
| live task activity | [`jsonrpc_bidirectional_service!`](../macros/bidirectional-jsonrpc-service.md) | typed WebSocket notifications |
| command-heavy workflows | [`jsonrpc_service!`](../macros/jsonrpc-service.md) | optional alternative for RPC-style APIs |

## Name Permissions Early

Permissions should be stable application concepts, not incidental handler
details. Good permission names usually describe the capability:

```text
project:read
project:write
task:write
attachment:read
attachment:write
admin
```

Each protected operation declares those requirements in the API definition:

```rust,ignore
GET WITH_PERMISSIONS(["project:read"]) projects() -> ProjectsResponse,
POST WITH_PERMISSIONS(["task:write"]) projects/{project_id: String}/tasks(CreateTaskRequest) -> Task,
DELETE WITH_PERMISSIONS(["admin"] | ["project:owner"]) projects/{project_id: String}() -> (),
```

`WITH_PERMISSIONS(["a", "b"])` means the authenticated user needs both
permissions. `WITH_PERMISSIONS(["a"] | ["b", "c"])` means either the first group
or the second group is enough. `WITH_PERMISSIONS([])` means authenticated, with
no extra permission requirement. `UNAUTHORIZED` is fully public, and
`OPTIONAL_AUTH` is public but hands the handler a `Caller` so it can tailor the
response when a valid credential is present (see
[Auth In The API Contract](../auth-in-api-contract.md)).

## Keep DTOs Boring

DTOs should be explicit, serializable, and independent of storage models. Avoid
exposing database-specific fields just because they exist.

```rust,ignore
#[cfg(feature = "server")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(JsonSchema))]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(JsonSchema))]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(JsonSchema))]
pub struct CreateTaskRequest {
    pub title: String,
    pub assignee_id: Option<String>,
}
```

The `JsonSchema` derive is gated because only server/spec generation needs it.
Shared serialization stays available with no transport feature enabled.

## Sketch The Service

A REST task service definition can stay close to the client workflow:

```rust,ignore
rest_service!({
    service_name: TaskService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    endpoints: [
        GET WITH_PERMISSIONS(["project:read"]) projects() -> ProjectsResponse,
        GET WITH_PERMISSIONS(["project:read"]) projects/{project_id: String}/tasks() -> TasksResponse,
        POST WITH_PERMISSIONS(["task:write"]) projects/{project_id: String}/tasks(CreateTaskRequest) -> Task,
        PATCH WITH_PERMISSIONS(["task:write"]) tasks/{task_id: String}(UpdateTaskRequest) -> Task,
    ]
});
```

At this point you have made the most important design decisions: operation
names, path parameters, request/response types, and auth requirements. The
server implementation can change later without changing this contract.

