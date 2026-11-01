# Design the contract

Start with workflows, not with Axum routes or database tables.
Write down what clients need to do, and which operations need to know who is calling.

For the team workspace, the first pass looks like this:

| Workflow | Macro | Reason |
| --- | --- | --- |
| list projects, read tasks, create tasks | [`rest_service!`](../macros/rest-service.md) | conventional JSON resources, OpenAPI, browser clients |
| upload and download task attachments | [`file_service!`](../macros/file-service.md) | streaming, multipart validation, early auth checks |
| live task activity | [`jsonrpc_bidirectional_service!`](../macros/bidirectional-jsonrpc-service.md) | typed WebSocket notifications |
| command-heavy workflows | [`jsonrpc_service!`](../macros/jsonrpc-service.md) | optional alternative for RPC-style APIs |

## Name permissions early

Permissions should be stable application concepts, not incidental handler details.
A good permission name describes a capability:

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

`WITH_PERMISSIONS(["a", "b"])` means the authenticated user needs both permissions.
`WITH_PERMISSIONS(["a"] | ["b", "c"])` means either group is enough.
`WITH_PERMISSIONS([])` means any authenticated user, with no specific permission.
`UNAUTHORIZED` is fully public.
`OPTIONAL_AUTH` is public too, but hands the handler a `Caller`,
so it can tailor the response when a valid credential is present.
[Auth in the API contract](../auth-in-api-contract.md) has the details.

## Keep DTOs boring

DTOs should be explicit, serializable and independent of storage models.
Do not expose database-specific fields just because they exist.

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

The `JsonSchema` derive is gated on the `server` feature,
because only the server side generates the OpenAPI documents.
Serialization stays available with no transport feature enabled.

## Sketch the service

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

At this point you have made the most important design decisions:
operation names, path parameters, request and response types, and auth requirements.
The server implementation can change later without changing this contract.

## Show a request that works

The schemas say what a request may contain.
A reader of the generated docs also wants to see one request that works, and what comes back.
Add examples to the operations people will try first:

```rust,ignore
POST WITH_PERMISSIONS(["task:write"]) projects/{project_id: String}/tasks(CreateTaskRequest) -> Task {
    examples: [
        /// A task with a title only.
        minimal {
            path: { project_id: "proj-1".to_string() },
            body: CreateTaskRequest {
                title: "Write the launch notes".to_string(),
                assignee_id: None,
            },
            status: 201,
            response: Task {
                id: "task-1".to_string(),
                project_id: "proj-1".to_string(),
                title: "Write the launch notes".to_string(),
                status: TaskStatus::Open,
                assignee_id: None,
            },
        },
        /// The project does not exist.
        unknown_project {
            path: { project_id: "proj-404".to_string() },
            body: CreateTaskRequest {
                title: "Write the launch notes".to_string(),
                assignee_id: None,
            },
            error: { status: 404, message: "Project not found" },
        },
    ],
},
```

The examples are typed Rust values, so they compile only while they match the contract.
They appear in the OpenAPI document and in the API explorer, where "Use" fills in the request.
A success example states its status when it is not 200;
the handler in a later chapter answers this request with `201 Created`.
An error example gives the status and message the handler's `RestError` carries.
The [`rest_service!` guide](../macros/rest-service.md#examples) lists the rules.

