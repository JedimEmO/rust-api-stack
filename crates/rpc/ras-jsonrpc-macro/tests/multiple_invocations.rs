use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PingRequest {
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PingResponse {
    value: String,
}

jsonrpc_service!({
    service_name: FirstRpcService,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
    ]
});

jsonrpc_service!({
    service_name: SecondRpcService,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
    ]
});

#[test]
fn multiple_jsonrpc_services_can_share_a_module() {
    let _ = std::any::type_name::<FirstRpcServiceClient>();
    let _ = std::any::type_name::<SecondRpcServiceClient>();

    fn _first_service_trait_exists<T: FirstRpcServiceTrait>() {}
    fn _second_service_trait_exists<T: SecondRpcServiceTrait>() {}

    assert_eq!(
        generate_firstrpcservice_openrpc()["info"]["title"],
        "FirstRpcService JSON-RPC API"
    );
    assert_eq!(
        generate_secondrpcservice_openrpc()["info"]["title"],
        "SecondRpcService JSON-RPC API"
    );

    let _ = firstrpcservice_explorer_routes("");
    let _ = secondrpcservice_explorer_routes("");
}
