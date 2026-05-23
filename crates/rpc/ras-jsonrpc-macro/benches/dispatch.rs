//! Criterion bench measuring per-call latency of an authenticated JSON-RPC
//! method through the in-memory axum-test stack: request -> axum router ->
//! handler.
//!
//! Run with `cargo bench -p ras-jsonrpc-macro`.

use criterion::{Criterion, criterion_group, criterion_main};
use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::runtime::Runtime;

#[path = "../tests/support/mod.rs"]
mod support;
use support::{MockAuthProvider, mock_http_server};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddRequest {
    a: i64,
    b: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddResponse {
    sum: i64,
}

jsonrpc_service!({
    service_name: BenchSvc,
    openrpc: false,
    methods: [
        WITH_PERMISSIONS(["user"]) add(AddRequest) -> AddResponse,
    ]
});

struct BenchSvcImpl;

impl BenchSvcTrait for BenchSvcImpl {
    async fn add(
        &self,
        _user: &ras_jsonrpc_core::AuthenticatedUser,
        req: AddRequest,
    ) -> Result<AddResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(AddResponse { sum: req.a + req.b })
    }
}

fn build_router() -> axum::Router {
    BenchSvcBuilder::new(BenchSvcImpl)
        .base_url("/rpc")
        .auth_provider(MockAuthProvider::default())
        .build()
        .expect("router build")
}

fn bench_dispatch(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let server = Arc::new(mock_http_server(build_router()));

    c.bench_function("jsonrpc_add_dispatch", |b| {
        b.to_async(&rt).iter(|| {
            let server = Arc::clone(&server);
            async move {
                let response = server
                    .post("/rpc")
                    .authorization_bearer("user-token")
                    .json(&json!({
                        "jsonrpc": "2.0",
                        "method": "add",
                        "params": AddRequest { a: 1, b: 2 },
                        "id": 1,
                    }))
                    .await;
                response.assert_status_ok();
                let payload: Value = response.json();
                let r: AddResponse =
                    serde_json::from_value(payload["result"].clone()).expect("result json");
                std::hint::black_box(r);
            }
        });
    });
}

criterion_group!(benches, bench_dispatch);
criterion_main!(benches);
