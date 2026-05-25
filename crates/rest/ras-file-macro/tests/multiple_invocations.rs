use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct UploadResponse {
    id: String,
}

file_service!({
    service_name: FirstFileService,
    base_path: "/first-files",
    openapi: true,
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 1024,
            parts: [
                file file {
                    required: true,
                    max_bytes: 1024,
                    filename: optional,
                },
            ],
        } -> UploadResponse,
    ]
});

file_service!({
    service_name: SecondFileService,
    base_path: "/second-files",
    openapi: true,
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 1024,
            parts: [
                file file {
                    required: true,
                    max_bytes: 1024,
                    filename: optional,
                },
            ],
        } -> UploadResponse,
    ]
});

#[test]
fn multiple_file_services_can_share_a_module() {
    let _ = std::any::type_name::<FirstFileServiceUploadPart<'static>>();
    let _ = std::any::type_name::<SecondFileServiceUploadPart<'static>>();
    let _ = std::any::type_name::<FirstFileServiceClient>();
    let _ = std::any::type_name::<SecondFileServiceClient>();

    assert_eq!(
        generate_firstfileservice_openapi()["info"]["title"],
        "FirstFileService File Service API"
    );
    assert_eq!(
        generate_secondfileservice_openapi()["info"]["title"],
        "SecondFileService File Service API"
    );
}
