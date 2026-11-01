//! OpenAPI 3.0 document generation module
//!
//! This module provides functionality to generate OpenAPI 3.0 specification documents
//! from the rest_service macro definitions.

use crate::{OpenApiConfig, ServiceDefinition};
use proc_macro2::{Ident, TokenStream};
use quote::quote;
mod operations;
mod schema;
pub use schema::generate_schema_impl_checks;

/// Emits the shared component-assembly code as a private module named `module`.
fn embedded_components(module: &Ident) -> TokenStream {
    let mut file: syn::File = syn::parse_str(ras_spec_components::SOURCE)
        .expect("ras-spec-components source is valid Rust");
    file.attrs.clear();
    quote! {
        #[doc(hidden)]
        #[allow(dead_code, unused_imports, missing_docs, clippy::all)]
        mod #module {
            #file
        }
    }
}

/// Generates OpenAPI document creation code
pub fn generate_openapi_code(
    service_def: &ServiceDefinition,
    config: &OpenApiConfig,
) -> TokenStream {
    let service_name = &service_def.service_name;
    let openapi_fn_name = quote::format_ident!(
        "generate_{}_openapi",
        service_name.to_string().to_lowercase()
    );
    let openapi_to_file_fn_name = quote::format_ident!(
        "generate_{}_openapi_to_file",
        service_name.to_string().to_lowercase()
    );
    let endpoint_info_struct_name = quote::format_ident!("{}OpenApiEndpointInfo", service_name);
    let components_mod = quote::format_ident!(
        "__ras_openapi_components_{}",
        service_name.to_string().to_lowercase()
    );

    let output_path_code = match config {
        OpenApiConfig::Enabled => {
            let service_name_lower = service_name.to_string().to_lowercase();
            quote! {
                format!("target/openapi/{}.json", #service_name_lower)
            }
        }
        OpenApiConfig::WithPath(path) => {
            quote! {
                #path.to_string()
            }
        }
    };

    let embedded = embedded_components(&components_mod);
    let endpoint_infos =
        operations::generate_endpoint_infos(service_def, &endpoint_info_struct_name);
    let paths = operations::generate_paths();

    quote! {
        #embedded

        struct #endpoint_info_struct_name {
            method: String,
            path: String,
            summary: Option<String>,
            description: Option<String>,
            auth_required: bool,
            auth_optional: bool,
            permissions: Vec<String>,
            permission_groups: Vec<Vec<String>>,
            /// `None` when the endpoint takes no body.
            request_schema: Option<serde_json::Value>,
            /// `None` for `()`, which is sent as JSON `null`.
            response_schema: Option<serde_json::Value>,
            /// (name, type as written, schema)
            path_params: Vec<(String, String, serde_json::Value)>,
            /// (name, type as written, required, schema)
            query_params: Vec<(String, String, bool, serde_json::Value)>,
            version: Option<String>,
            canonical_version: Option<String>,
            canonical_path: String,
        }

        /// Generate OpenAPI 3.0 document for this service
        pub fn #openapi_fn_name() -> serde_json::Value {
            use serde_json::json;

            let mut generators = #components_mod::Generators::new(#components_mod::Dialect::OpenApi3);

            let mut endpoints: Vec<#endpoint_info_struct_name> = vec![
                #(#endpoint_infos),*
            ];

            let components = generators.into_components().unwrap_or_else(|error| {
                panic!("OpenAPI document for {}: {}", stringify!(#service_name), error)
            });
            for endpoint in &mut endpoints {
                if let Some(schema) = endpoint.response_schema.as_mut() {
                    components.fix_serialized(schema);
                }
            }
            let final_schemas = components.schemas;

            #paths

            json!({
                "openapi": "3.0.3",
                "info": {
                    "title": format!("{} REST API", stringify!(#service_name)),
                    "version": "1.0.0",
                    "description": format!("OpenAPI 3.0 specification for the {} service", stringify!(#service_name))
                },
                "paths": paths,
                "components": {
                    "schemas": final_schemas,
                    "securitySchemes": {
                        "bearerAuth": {
                            "type": "http",
                            "scheme": "bearer",
                            "description": "Bearer token for authentication"
                        }
                    }
                }
            })
        }

        /// Write OpenAPI document to the target directory
        pub fn #openapi_to_file_fn_name() -> std::io::Result<()> {
            let doc = #openapi_fn_name();
            let output_path = #output_path_code;

            if let Some(parent) = std::path::Path::new(&output_path).parent() {
                std::fs::create_dir_all(parent)?;
            }

            let json_string = serde_json::to_string_pretty(&doc)?;
            std::fs::write(&output_path, &json_string)?;

            println!("Generated OpenAPI document at: {}", output_path);

            Ok(())
        }

    }
}
