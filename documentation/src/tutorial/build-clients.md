# Build clients

Client crates depend on the same API crate with `features = ["client"]`.
Enable the API crate's `fs` feature too when you use the generated file-upload helpers
that read parts from disk.

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["client"] }
```

## The Rust REST client

The generated REST client turns path parameters, query values and request bodies
into typed method arguments.

```rust,ignore
use workspace_api::{CreateTaskRequest, TaskServiceClient};

let mut client = TaskServiceClient::builder("https://workspace.example.com")
    .with_timeout(std::time::Duration::from_secs(10))
    .build()?;

client.set_bearer_token(Some(token));

let tasks = client
    .get_projects_by_project_id_tasks("project-123".to_string())
    .await?;

let created = client
    .post_projects_by_project_id_tasks(
        "project-123".to_string(),
        CreateTaskRequest {
            title: "Write release notes".to_string(),
            assignee_id: None,
        },
    )
    .await?;
```

The client method names match the generated handler names,
so a contract change shows up as a compiler error in the client.

## The Rust file client

The generated file client builds the multipart upload requests and the download requests.

```rust,ignore
use workspace_api::{AttachmentServiceClient, AttachmentServiceTasksByTaskIdUploadMultipart};

let mut client = AttachmentServiceClient::builder("https://workspace.example.com")
    .with_timeout(std::time::Duration::from_secs(30))
    .build()?;

client.set_bearer_token(Some(token));

let form = AttachmentServiceTasksByTaskIdUploadMultipart::new()
    .file("notes.pdf", Some("notes.pdf"), Some("application/pdf"))
    .await?;

let uploaded = client
    .tasks_by_task_id_upload("task-123".to_string(), form)
    .await?;

let response = client
    .download_by_attachment_id(uploaded.attachment_id)
    .await?;
let bytes = response.bytes().await?;
```

`bytes()` stops at 16 MiB and fails with `TransportError::BodyTooLarge`,
so a server cannot make the client buffer without end.
For a larger file, raise the limit on that response with `response.with_body_limit(n)`,
or write the body out as it arrives with `response.into_body_stream()`.

The disk-streaming `file(...)` helper above needs both `client` and `fs` on the API crate:

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["client", "fs"] }
```

For tests, or content already in memory as in a browser,
each file part also gets a `*_bytes` helper, such as `file_bytes(bytes, "notes.pdf", None)`.
It needs no `fs` feature and works in WASM builds.

## TypeScript clients from OpenAPI

If your browser app is TypeScript, generate a fetch client from the OpenAPI
files that the server build writes.
The function names come from each operation's `operationId`.
Most generators take one config object per call:

```typescript
import {
  getProjectsProjectIdTasks,
  postProjectsProjectIdTasks,
} from './generated/task-client';

const baseUrl = 'https://workspace.example.com/api/v1';

const tasks = await getProjectsProjectIdTasks({
  baseUrl,
  headers: { Authorization: `Bearer ${token}` },
  path: { project_id: 'project-123' },
});

const created = await postProjectsProjectIdTasks({
  baseUrl,
  headers: { Authorization: `Bearer ${token}` },
  path: { project_id: 'project-123' },
  body: {
    title: 'Write release notes',
    assignee_id: null,
  },
});
```

File-service operation IDs start with `upload` or `download` instead of the HTTP method.
Uploads use `FormData` or the generator's multipart object shape:

```typescript
await uploadTasksTaskIdUpload({
  baseUrl: 'https://workspace.example.com/api/v1/attachments',
  headers: { Authorization: `Bearer ${token}` },
  path: { task_id: 'task-123' },
  body: { file },
});
```

## WebSocket notifications

The bidirectional client registers typed notification handlers before it connects:

```rust,ignore
let mut activity = ActivityServiceClientBuilder::new("wss://workspace.example.com/ws")
    .with_jwt_token(token)
    .build()
    .await?;

activity.on_task_changed(|event| {
    println!("task changed: {}", event.task_id);
});

activity.connect().await?;
activity.subscribe_project("project-123".to_string()).await?;
```

Use the generated clients directly at the edges of the application,
and wrap them in small domain-specific adapters if the UI needs a simpler interface.
