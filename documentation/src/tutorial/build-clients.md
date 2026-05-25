# 4. Build Clients

Client crates depend on the same API crate with `features = ["client"]`.

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["client"] }
```

## Rust REST Client

The generated REST client turns paths, query values, and request bodies into
typed method arguments.

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

The client method names mirror the generated handler names, so compiler errors
surface contract changes immediately.

## Rust File Client

The generated file client builds multipart requests and download requests.

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

For tests or browser-like buffered content, generated multipart builders also
provide `*_bytes` helpers where file parts are declared.

## TypeScript Clients From OpenAPI

If your browser app is TypeScript, generate a fetch client from the OpenAPI
files emitted by the server build. Generated clients usually accept one config
object per call:

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

File uploads use `FormData` or the generator's multipart object shape:

```typescript
await postTasksTaskIdUpload({
  baseUrl: 'https://workspace.example.com/api/v1/attachments',
  headers: { Authorization: `Bearer ${token}` },
  path: { task_id: 'task-123' },
  body: { file },
});
```

## WebSocket Notifications

The bidirectional client registers typed notification handlers before
connecting:

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

Use generated clients directly at application edges, then wrap them in small
domain-specific adapters if the UI needs a simpler interface.

