# 3. Implement The Server

The server crate depends on the API crate with `features = ["server"]` and
implements the generated traits.

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["server"] }
ras-auth-core = "0.1.0"
ras-rest-core = "0.1.1"
ras-file-core = "0.1.0"
axum = "0.8"
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
```

## Auth Provider

RAS auth providers turn credentials into an `AuthenticatedUser`. Permission
checks declared in the API definition run after authentication and before the
handler.

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

Handlers do not parse tokens. Protected generated methods receive the
authenticated user as a typed argument.

## REST Handler Implementation

Generated REST traits return `RestResult<T>`.

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

The generated signature reflects the API declaration: protected endpoints get
`&AuthenticatedUser`, path parameters are typed arguments, and request bodies
are typed structs.

## File Handler Implementation

Uploads run in phases. This lets generated code authenticate first, enforce
size limits, reject unknown fields, and ensure file streams are consumed.

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

Generated names include path segments so multiple uploads can coexist in one
service.

## Mount The App

Build generated routers and merge them into one Axum app:

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

For WebSocket services, mount the generated service state on an Axum route as
shown in the [bidirectional macro guide](../macros/bidirectional-jsonrpc-service.md).

## Generate Specs During Build

For REST and file services, a server `build.rs` can write OpenAPI documents for
frontends:

```rust,ignore
fn main() {
    workspace_api::generate_taskservice_openapi_to_file()
        .expect("generate task OpenAPI");
    workspace_api::generate_attachmentservice_openapi_to_file()
        .expect("generate attachment OpenAPI");
}
```

That keeps generated client input tied to the exact Rust API contract the server
compiled against.

