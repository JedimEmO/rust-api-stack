//! Build script to generate OpenAPI specification at compile time

fn main() {
    println!("cargo:rerun-if-changed=../rest-api/src/lib.rs");

    // Import and call the OpenAPI generation function from the API crate
    // This will generate the OpenAPI spec at compile time
    match rest_api::generate_userservice_openapi_to_file() {
        Ok(()) => {}
        Err(e) => {
            println!(
                "cargo:warning=Failed to generate OpenAPI specification: {}",
                e
            );
            // Don't fail the build, just warn
        }
    }
}
