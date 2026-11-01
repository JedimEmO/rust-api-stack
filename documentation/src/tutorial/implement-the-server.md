# Implement the server

The server crate depends on the API crate with `features = ["server"]`
and implements the generated traits.

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["server"] }
ras-auth-core = "0.3.0"
ras-rest-core = "0.2.1"
ras-file-core = "0.2.1"
axum = "0.8"
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
```

## The auth provider

An auth provider turns a credential into an `AuthenticatedUser`.
The permission checks declared in the API definition run after authentication
and before the handler.

```rust,ignore
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::HashSet;

#[derive(Clone)]
pub struct AppAuthProvider {
    sessions: SessionStore,
}

impl AuthProvider for AppAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let session = self
                .sessions
                .lookup(&token)
                .await
                .map_err(|_| AuthError::InvalidToken)?;

            Ok(AuthenticatedUser {
                user_id: session.user_id,
                permissions: session.permissions.into_iter().collect::<HashSet<_>>(),
                metadata: None,
            })
        })
    }
}
```

Handlers do not parse tokens.
A protected generated method receives the authenticated user as a typed argument.

## The REST handlers

Generated REST handler methods return `RestResult<T>`,
which carries either a `RestResponse` with its status or a `RestError`.

```rust,ignore
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestResponse, RestResult};
use workspace_api::{
    CreateTaskRequest, Task, TaskServiceTrait, TasksResponse,
};

#[derive(Clone)]
pub struct TaskHandlers {
    tasks: TaskRepository,
}

#[async_trait::async_trait]
impl TaskServiceTrait for TaskHandlers {
    async fn get_projects_by_project_id_tasks(
        &self,
        user: &AuthenticatedUser,
        project_id: String,
    ) -> RestResult<TasksResponse> {
        let tasks = self.tasks.visible_to(user, &project_id).await?;
        Ok(RestResponse::ok(TasksResponse { tasks }))
    }

    async fn post_projects_by_project_id_tasks(
        &self,
        user: &AuthenticatedUser,
        project_id: String,
        request: CreateTaskRequest,
    ) -> RestResult<Task> {
        let task = self.tasks.create(user, project_id, request).await?;
        Ok(RestResponse::created(task))
    }
}
```

The generated signature mirrors the API declaration.
Protected endpoints get `&AuthenticatedUser`, path parameters are typed arguments,
and request bodies are typed structs.
The method name is the HTTP method followed by the path, with `by_` before each path parameter.

The `?` operators assume the repository's error type converts into `RestError`.
That `From` impl is where you decide the status and message a client sees;
`RestError::with_internal` keeps the underlying error for the server log
without sending it to the client.

## The file handlers

An upload runs in phases: `begin`, then `part` for each multipart field, then `finish`.
The phases let the generated code authenticate before any bytes are read,
enforce size limits, reject unknown fields and make sure every file stream is consumed.
An optional `abort` method cleans up when an upload fails part-way.

```rust,ignore
use ras_file_core::{FileRequestContext, FileResult, JsonResponse};
use workspace_api::{
    AttachmentServiceTrait, AttachmentServiceTasksByTaskIdUploadPart,
    AttachmentServiceTasksByTaskIdUploadPath, AttachmentUploadResponse,
};

pub struct UploadState {
    attachment_id: Option<String>,
    file_name: Option<String>,
    size: u64,
}

#[async_trait::async_trait]
impl AttachmentServiceTrait for AttachmentHandlers {
    type TasksByTaskIdUploadState = UploadState;

    async fn tasks_by_task_id_upload_begin(
        &self,
        ctx: &FileRequestContext<'_>,
        path: &AttachmentServiceTasksByTaskIdUploadPath,
    ) -> FileResult<Self::TasksByTaskIdUploadState> {
        self.attachments.reserve(ctx.user, &path.task_id).await
    }

    async fn tasks_by_task_id_upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &AttachmentServiceTasksByTaskIdUploadPath,
        state: &mut Self::TasksByTaskIdUploadState,
        part: &mut AttachmentServiceTasksByTaskIdUploadPart<'_>,
    ) -> FileResult<()> {
        match part {
            AttachmentServiceTasksByTaskIdUploadPart::File(file) => {
                while let Some(chunk) = file.next_chunk().await? {
                    state.size += chunk.len() as u64;
                    self.attachments.write_chunk(state, &chunk).await?;
                }
                state.file_name = file.file_name().map(str::to_owned);
            }
        }

        Ok(())
    }

    async fn tasks_by_task_id_upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &AttachmentServiceTasksByTaskIdUploadPath,
        state: Self::TasksByTaskIdUploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> FileResult<JsonResponse<AttachmentUploadResponse>> {
        Ok(JsonResponse::ok(state.into_response()?))
    }
}
```

Generated names include the path segments, so several uploads can coexist in one service.
The authenticated user arrives in `ctx.user`,
an `Option` because the same context type serves public endpoints.

## Mount the app

Build the generated routers and merge them into one Axum app:

```rust,ignore
let auth = AppAuthProvider::new(session_store);

let task_routes = workspace_api::TaskServiceBuilder::new(TaskHandlers { tasks })
    .auth_provider(auth.clone())
    .build();

let attachment_routes =
    workspace_api::AttachmentServiceBuilder::new(AttachmentHandlers { attachments })
        .auth_provider(auth.clone())
        .build();

let app = axum::Router::new()
    .merge(task_routes)
    .merge(attachment_routes);
```

For the WebSocket service, mount the generated service on an Axum route,
as the [bidirectional macro guide](../macros/bidirectional-jsonrpc-service.md) shows.

## Generate the specs during the build

A server `build.rs` can write the OpenAPI documents for frontends.
It needs the API crate as a build dependency with the `server` feature:

```toml
[build-dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["server"] }
```

```rust,ignore
fn main() {
    workspace_api::generate_taskservice_openapi_to_file()
        .expect("generate task OpenAPI");
    workspace_api::generate_attachmentservice_openapi_to_file()
        .expect("generate attachment OpenAPI");
}
```

Each `_to_file` function writes `target/openapi/<service name in lowercase>.json`,
such as `target/openapi/taskservice.json`.
The path is relative to the directory the build script runs in, which is the server crate's own directory.
Give `openapi` an `output` path, as in `openapi: { output: "../../openapi/tasks.json" }`,
to choose the location.
With the `expect` calls above, an authored example that cannot go into the document fails the build,
and the message names the endpoint and the example.

This keeps the generated client input tied to the exact contract the server compiled against.

