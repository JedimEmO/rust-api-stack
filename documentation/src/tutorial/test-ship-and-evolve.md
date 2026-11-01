# Test, ship and evolve

Strict API definitions pay off when CI builds the important feature combinations,
and when tests assert that the auth metadata stays visible in the generated specs.

## Contract tests

Test DTO serialization, so the wire format does not change by accident:

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

Test the generated OpenAPI or OpenRPC document for the route shape and the permission metadata.
These tests run with the API crate's `server` feature, which is where the REST document is generated:

```rust,ignore
#[test]
fn openapi_documents_task_permissions() {
    let doc = workspace_api::try_generate_taskservice_openapi()
        .expect("task OpenAPI document");
    let create = &doc["paths"]["/projects/{project_id}/tasks"]["post"];

    assert_eq!(create["security"][0]["bearerAuth"], serde_json::json!([]));
    assert_eq!(create["x-permissions"], serde_json::json!(["task:write"]));
}
```

These tests catch an accidental auth change before a client discovers it.
Calling `try_generate_taskservice_openapi` also checks every authored example.
It returns a `SpecGenerationError` naming the endpoint and the example
when an example cannot be serialized or does not read back through its request type.

## Server tests

Run the generated routes on an in-memory Axum test server:

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

File-service tests should cover oversized requests, missing required parts,
wrong content types, and auth rejection before upload handling begins.

## The feature matrix

Add CI checks for the API crate itself:

```bash
cargo check -p workspace-api --no-default-features --locked
cargo check -p workspace-api --no-default-features --features server --locked
cargo check -p workspace-api --no-default-features --features client --locked
cargo check -p workspace-api --target wasm32-unknown-unknown --no-default-features --features client --locked
```

This proves that the DTO-only, server-only, native client and browser client builds
each compile on their own.

## Deployment shape

A typical release pipeline does three things:

- build and test the Rust workspace;
- generate the OpenAPI or OpenRPC documents from the API crate;
- publish generated docs and client inputs alongside the server artifact.

This guide is built the same way:
CI builds the mdBook and publishes it to GitHub Pages from the `master` branch.
An application repository can use the same pattern for its own API documentation.

## Evolving the API

Prefer additive changes:

- add optional request fields instead of requiring new fields immediately;
- add response fields that old clients can ignore;
- add new operations before removing old operations;
- preserve permission names unless the capability truly changed.

When a wire shape must change, use the versioning support in `rest_service!` and `jsonrpc_service!`.
Keep the service implementation canonical,
and let the API boundary migrate legacy request and response shapes.
That makes compatibility an explicit part of the contract instead of an untested handler branch.

Before removing a legacy operation, check the generated specs, client usage and server logs.
The contract tells you what still exists; telemetry tells you what is still used.

