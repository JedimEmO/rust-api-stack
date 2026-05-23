//! Criterion bench measuring per-call latency of an authenticated REST GET
//! through the in-memory axum-test stack: request -> axum router -> handler.
//!
//! Run with `cargo bench -p ras-rest-macro`.

use criterion::{Criterion, criterion_group, criterion_main};
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestResponse, RestResult};
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::runtime::Runtime;

#[path = "../tests/support/mod.rs"]
mod support;
use support::{MockAuthProvider, mock_http_server};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Item {
    id: u32,
    name: String,
}

rest_service!({
    service_name: BenchSvc,
    base_path: "/api",
    openapi: false,
    serve_docs: false,
    endpoints: [
        GET WITH_PERMISSIONS(["user"]) items/{id: u32}() -> Item,
    ]
});

struct BenchImpl;

#[async_trait::async_trait]
impl BenchSvcTrait for BenchImpl {
    async fn get_items_by_id(&self, _user: &AuthenticatedUser, id: u32) -> RestResult<Item> {
        Ok(RestResponse::ok(Item {
            id,
            name: "x".into(),
        }))
    }
}

fn build_router() -> axum::Router {
    BenchSvcBuilder::new(BenchImpl)
        .auth_provider(MockAuthProvider::default())
        .build()
}

fn bench_dispatch(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let server = Arc::new(mock_http_server(build_router()));

    c.bench_function("rest_get_dispatch", |b| {
        b.to_async(&rt).iter(|| {
            let server = Arc::clone(&server);
            async move {
                let response = server
                    .get("/api/items/1")
                    .authorization_bearer("user-token")
                    .await;
                response.assert_status_ok();
                let r: Item = response.json();
                std::hint::black_box(r);
            }
        });
    });
}

criterion_group!(benches, bench_dispatch);
criterion_main!(benches);
