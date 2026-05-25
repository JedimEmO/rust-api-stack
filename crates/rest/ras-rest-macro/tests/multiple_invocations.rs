use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Item {
    id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct ItemResponse {
    item: Item,
}

rest_service!({
    service_name: FirstRestService,
    base_path: "/first",
    openapi: true,
    serve_docs: true,
    endpoints: [
        GET UNAUTHORIZED health ? verbose: Option<bool> () -> ItemResponse,
    ]
});

rest_service!({
    service_name: SecondRestService,
    base_path: "/second",
    openapi: true,
    serve_docs: true,
    endpoints: [
        GET UNAUTHORIZED health ? verbose: Option<bool> () -> ItemResponse,
    ]
});

#[test]
fn multiple_rest_services_can_share_a_module() {
    let _ = std::any::type_name::<FirstRestServiceClient>();
    let _ = std::any::type_name::<SecondRestServiceClient>();

    fn _first_service_trait_exists<T: FirstRestServiceTrait>() {}
    fn _second_service_trait_exists<T: SecondRestServiceTrait>() {}

    assert_eq!(
        generate_firstrestservice_openapi()["info"]["title"],
        "FirstRestService REST API"
    );
    assert_eq!(
        generate_secondrestservice_openapi()["info"]["title"],
        "SecondRestService REST API"
    );
}
