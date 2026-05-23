fn main() {
    println!("cargo:rerun-if-changed=../api/src/lib.rs");
    oauth2_demo_api::generate_googleoauth2service_openrpc_to_file()
        .expect("failed to generate OAuth2 demo OpenRPC document");
}
