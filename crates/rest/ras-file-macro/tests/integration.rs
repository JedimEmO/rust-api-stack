use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UploadMetadata {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct UploadResponse {
    id: String,
}

file_service!({
    service_name: IntegrationService,
    base_path: "/integration",
    openapi: true,
    endpoints: [
        UPLOAD WITH_PERMISSIONS(["admin", "moderator"]) upload/{bucket: String} multipart {
            max_total_bytes: unlimited,
            reject_unknown_fields: false,
            parts: [
                file file {
                    required: true,
                    max_count: 2,
                    max_bytes: 1024,
                    content_types: ["application/octet-stream"],
                    filename: required,
                },
                json metadata: UploadMetadata {
                    required: false,
                    max_bytes: 128,
                    content_types: ["application/json"],
                },
            ],
        } -> UploadResponse,
        DOWNLOAD WITH_PERMISSIONS(["admin"]) download/{id: String},
    ]
});

#[test]
fn generated_openapi_includes_permission_groups_and_unlimited_upload() {
    let doc = generate_integrationservice_openapi();

    let upload = &doc["paths"]["/upload/{bucket}"]["post"];
    assert_eq!(upload["security"][0]["bearerAuth"], serde_json::json!([]));
    assert_eq!(
        upload["x-permissions"],
        serde_json::json!(["admin", "moderator"])
    );
    assert_eq!(
        upload["x-ras-file"]["maxTotalBytes"],
        serde_json::Value::Null
    );
}
