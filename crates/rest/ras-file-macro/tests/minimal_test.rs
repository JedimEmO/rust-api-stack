use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct UploadResponse {
    ok: bool,
}

file_service!({
    service_name: MinimalService,
    base_path: "/minimal",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 512,
            parts: [
                file file {
                    required: true,
                    max_bytes: 512,
                    filename: optional,
                },
            ],
        } -> UploadResponse,
    ]
});

#[test]
fn generated_names_are_available() {
    let _ = std::any::type_name::<MinimalServiceUploadPath>();
    let _ = std::any::type_name::<MinimalServiceUploadPart<'static>>();
}
