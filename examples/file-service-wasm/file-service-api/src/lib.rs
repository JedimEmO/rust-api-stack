use ras_file_macro::file_service;
#[cfg(not(target_arch = "wasm32"))]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(JsonSchema))]
pub struct UploadResponse {
    pub file_id: String,
    pub file_name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(JsonSchema))]
pub struct FileMetadata {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub uploaded_at: String,
}

file_service!({
    service_name: DocumentService,
    base_path: "/api/documents",
    openapi: true,
    body_limit: 104857600, // 100 MB
    endpoints: [
        UPLOAD UNAUTHORIZED upload() -> UploadResponse,
        UPLOAD WITH_PERMISSIONS(["user"]) upload_profile_picture() -> UploadResponse,
        DOWNLOAD UNAUTHORIZED download/{file_id:String}() -> (),
        DOWNLOAD WITH_PERMISSIONS(["user"]) download_secure/{file_id:String}() -> (),
    ]
});

// Re-export the macro-generated WASM client when the feature is enabled
#[cfg(all(target_arch = "wasm32", feature = "wasm-client"))]
pub use wasm_client::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn upload_response_serializes_file_identity_and_size() {
        let response = UploadResponse {
            file_id: "file-123".to_string(),
            file_name: "report.pdf".to_string(),
            size: 4096,
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "file_id": "file-123",
                "file_name": "report.pdf",
                "size": 4096
            })
        );
    }

    #[test]
    fn file_metadata_serializes_optional_content_type() {
        let metadata = FileMetadata {
            id: "file-123".to_string(),
            name: "report.pdf".to_string(),
            size: 4096,
            content_type: Some("application/pdf".to_string()),
            uploaded_at: "2026-05-23T12:00:00Z".to_string(),
        };

        assert_eq!(
            serde_json::to_value(metadata).unwrap(),
            json!({
                "id": "file-123",
                "name": "report.pdf",
                "size": 4096,
                "content_type": "application/pdf",
                "uploaded_at": "2026-05-23T12:00:00Z"
            })
        );
    }

    #[test]
    fn file_metadata_preserves_absent_content_type_as_null() {
        let metadata = FileMetadata {
            id: "file-123".to_string(),
            name: "archive.bin".to_string(),
            size: 8192,
            content_type: None,
            uploaded_at: "2026-05-23T12:00:00Z".to_string(),
        };

        assert_eq!(
            serde_json::to_value(metadata).unwrap(),
            json!({
                "id": "file-123",
                "name": "archive.bin",
                "size": 8192,
                "content_type": null,
                "uploaded_at": "2026-05-23T12:00:00Z"
            })
        );
    }

    #[test]
    fn upload_response_deserializes_generated_client_payload() {
        let response: UploadResponse = serde_json::from_value(json!({
            "file_id": "file-123",
            "file_name": "report.pdf",
            "size": 4096
        }))
        .unwrap();

        assert_eq!(response.file_id, "file-123");
        assert_eq!(response.file_name, "report.pdf");
        assert_eq!(response.size, 4096);
    }

    fn parameter<'a>(operation: &'a Value, name: &str) -> &'a Value {
        operation["parameters"]
            .as_array()
            .expect("parameters array")
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap_or_else(|| panic!("missing parameter {name}"))
    }

    #[test]
    fn generated_openapi_documents_upload_routes_and_multipart_body() {
        let doc = generate_documentservice_openapi();

        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "DocumentService File Service API");
        assert_eq!(doc["servers"][0]["url"], "/api/documents");

        let public_upload = &doc["paths"]["/upload"]["post"];
        assert_eq!(
            public_upload["requestBody"]["content"]["multipart/form-data"]["schema"]["$ref"],
            "#/components/schemas/FileUploadRequest"
        );
        assert!(public_upload.get("security").is_none());

        let profile_upload = &doc["paths"]["/upload_profile_picture"]["post"];
        assert_eq!(profile_upload["security"][0]["bearerAuth"], json!([]));
        assert_eq!(profile_upload["x-permissions"], json!(["user"]));
    }

    #[test]
    fn generated_openapi_documents_download_path_parameters_and_auth() {
        let doc = generate_documentservice_openapi();

        let public_download = &doc["paths"]["/download/{file_id}"]["get"];
        assert_eq!(parameter(public_download, "file_id")["in"], json!("path"));
        assert_eq!(
            parameter(public_download, "file_id")["required"],
            json!(true)
        );
        assert_eq!(
            public_download["responses"]["200"]["content"]["application/octet-stream"]["schema"]["$ref"],
            "#/components/schemas/BinaryFileResponse"
        );
        assert!(public_download.get("security").is_none());

        let secure_download = &doc["paths"]["/download_secure/{file_id}"]["get"];
        assert_eq!(secure_download["security"][0]["bearerAuth"], json!([]));
        assert_eq!(secure_download["x-permissions"], json!(["user"]));
    }

    #[test]
    fn generated_openapi_includes_file_operation_component_schemas() {
        let doc = generate_documentservice_openapi();

        let upload_schema = &doc["components"]["schemas"]["FileUploadRequest"];
        assert_eq!(upload_schema["required"], json!(["file"]));
        assert_eq!(
            upload_schema["properties"]["file"]["format"],
            json!("binary")
        );

        let download_schema = &doc["components"]["schemas"]["BinaryFileResponse"];
        assert_eq!(download_schema["type"], json!("string"));
        assert_eq!(download_schema["format"], json!("binary"));

        let upload_response_schema = &doc["components"]["schemas"]["UploadResponse"];
        assert_eq!(
            upload_response_schema["properties"]["file_id"]["type"],
            json!("string")
        );
        assert_eq!(
            upload_response_schema["properties"]["file_name"]["type"],
            json!("string")
        );
        assert!(upload_response_schema["properties"]["size"].is_object());
    }
}
