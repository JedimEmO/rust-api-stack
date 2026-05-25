# 5. Test, Ship, And Evolve

Strict API definitions are most useful when CI checks the important feature
combinations and when tests assert that auth metadata stays visible in the
generated specs.

## Contract Tests

Test DTO serialization for wire stability:

```rust,ignore
#[test]
fn create_task_request_serializes_expected_shape() {
    let value = serde_json::to_value(CreateTaskRequest {
        title: "Write release notes".to_string(),
        assignee_id: None,
    })
    .unwrap();

    assert_eq!(
        value,
        serde_json::json!({
            "title": "Write release notes",
            "assignee_id": null
        })
    );
}
```

Test generated OpenAPI or OpenRPC output for route shape and permission
metadata:

```rust,ignore
#[test]
fn openapi_documents_task_permissions() {
    let doc = workspace_api::generate_taskservice_openapi();
    let create = &doc["paths"]["/projects/{project_id}/tasks"]["post"];

    assert_eq!(create["security"][0]["bearerAuth"], serde_json::json!([]));
    assert_eq!(create["x-permissions"], serde_json::json!(["task:write"]));
}
```

These tests catch accidental auth changes before a client discovers them.

## Server Tests

Use an in-memory Axum test server for generated routes:

```rust,ignore
#[tokio::test]
async fn create_task_requires_write_permission() {
    let app = build_app_with_test_auth();
    let server = axum_test::TestServer::new(app).unwrap();

    let response = server
        .post("/api/v1/projects/project-123/tasks")
        .authorization_bearer("read-only-token")
        .json(&CreateTaskRequest {
            title: "Write release notes".to_string(),
            assignee_id: None,
        })
        .await;

    response.assert_status_forbidden();
}
```

File-service tests should include oversized requests, missing required parts,
wrong content types, and auth rejection before upload handling begins.

## Feature Matrix

Add CI checks for the API crate itself:

```bash
cargo check -p workspace-api --no-default-features --locked
cargo check -p workspace-api --no-default-features --features server --locked
cargo check -p workspace-api --no-default-features --features client --locked
cargo check -p workspace-api --target wasm32-unknown-unknown --no-default-features --features client --locked
```

This proves DTO-only, server-only, native client, and browser client builds stay
separate.

## Deployment Shape

A typical release pipeline does three things:

- build and test the Rust workspace;
- generate OpenAPI/OpenRPC documents from the API crate;
- publish generated docs and client inputs alongside the server artifact.

The mdBook in this repository is built in CI and published to GitHub Pages from
the `master` branch. Application repositories can use the same pattern for
project-specific API documentation.

## Evolving The API

Prefer additive changes when possible:

- add optional request fields instead of requiring new fields immediately;
- add response fields that old clients can ignore;
- add new operations before removing old operations;
- preserve permission names unless the capability truly changed.

When a wire shape must change, use versioning support where the macro provides
it. Keep the service implementation canonical and let the API boundary migrate
legacy request and response shapes. That makes compatibility an explicit part
of the contract instead of an untested handler branch.

Before removing a legacy operation, check generated specs, client usage, and
server logs. The contract should tell you what still exists; telemetry should
tell you what is still used.

