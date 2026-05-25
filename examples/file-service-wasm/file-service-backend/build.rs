fn main() {
    println!("cargo:rerun-if-changed=../file-service-api/src/lib.rs");

    // Generate OpenAPI spec using the file service API
    match file_service_api::generate_documentservice_openapi_to_file() {
        Ok(_) => println!("Generated OpenAPI specification"),
        Err(e) => eprintln!("Failed to generate OpenAPI spec: {}", e),
    }

    let manifest = ras_permission_manifest::PermissionManifest::from_services([
        file_service_api::generate_documentservice_permission_manifest(),
    ]);
    if let Err(e) = ras_permission_manifest::write_manifest(
        "target/ras-permissions/file-service-wasm.json",
        &manifest,
    ) {
        eprintln!("Failed to generate permission manifest: {}", e);
    }
}
