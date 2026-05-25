fn main() {
    println!("cargo:rerun-if-changed=../api/src/lib.rs");
    oauth2_demo_api::generate_googleoauth2service_openrpc_to_file()
        .expect("failed to generate OAuth2 demo OpenRPC document");

    let manifest = ras_permission_manifest::PermissionManifest::from_services([
        oauth2_demo_api::generate_googleoauth2service_permission_manifest(),
    ]);
    ras_permission_manifest::write_manifest("target/ras-permissions/oauth2-demo.json", &manifest)
        .expect("failed to generate OAuth2 demo permission manifest");
}
