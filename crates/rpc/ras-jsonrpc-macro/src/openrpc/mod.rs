//! OpenRPC document generation module
//!
//! This module provides functionality to generate OpenRPC specification documents
//! from the jsonrpc_service macro definitions.

use crate::{OpenRpcConfig, ServiceDefinition};
use proc_macro2::TokenStream;
use quote::quote;
mod examples;
mod methods;
mod schema;
pub use schema::generate_schema_impl_checks;

/// Generates OpenRPC document creation code
pub fn generate_openrpc_code(
    service_def: &ServiceDefinition,
    config: &OpenRpcConfig,
) -> TokenStream {
    let service_name = &service_def.service_name;
    let openrpc_fn_name = quote::format_ident!(
        "generate_{}_openrpc",
        service_name.to_string().to_lowercase()
    );
    let openrpc_to_file_fn_name = quote::format_ident!(
        "generate_{}_openrpc_to_file",
        service_name.to_string().to_lowercase()
    );
    let method_info_struct_name = quote::format_ident!("{}OpenRpcMethodInfo", service_name);

    let output_path_code = match config {
        OpenRpcConfig::Enabled => {
            let service_name_lower = service_name.to_string().to_lowercase();
            quote! {
                format!("target/openrpc/{}.json", #service_name_lower)
            }
        }
        OpenRpcConfig::WithPath(path) => {
            quote! {
                #path.to_string()
            }
        }
    };

    let flatten_fn_name = quote::format_ident!(
        "_flatten_schema_defs_{}",
        service_name.to_string().to_lowercase()
    );
    let update_refs_fn_name = quote::format_ident!(
        "_update_refs_recursive_{}",
        service_name.to_string().to_lowercase()
    );
    let generate_example_fn_name = quote::format_ident!(
        "_generate_example_from_schema_{}",
        service_name.to_string().to_lowercase()
    );

    let unique_types = schema::collect_types(service_def);
    let (schema_fns, schema_insertions) =
        schema::generate_schemas(service_name, &flatten_fn_name, &unique_types);
    let method_infos = methods::generate_method_infos(service_def, &method_info_struct_name);
    let normalization = schema::generate_normalization(&flatten_fn_name, &update_refs_fn_name);
    let examples = examples::generate_examples(&generate_example_fn_name);
    let methods = methods::generate_methods(&generate_example_fn_name);

    quote! {
        #[derive(serde::Serialize)]
        struct #method_info_struct_name {
            name: String,
            summary: Option<String>,
            description: Option<String>,
            auth_required: bool,
            auth_optional: bool,
            permissions: Vec<String>,
            permission_groups: Vec<Vec<String>>,
            request_type_name: String,
            response_type_name: String,
            version: Option<String>,
            canonical_version: Option<String>,
            canonical_method: String,
        }

        #normalization
        #examples

        #(#schema_fns)*

        /// Generate OpenRPC document for this service
        pub fn #openrpc_fn_name() -> serde_json::Value {
            use serde_json::json;
            use schemars::{schema_for, JsonSchema};
            use std::collections::HashMap;

            let methods = vec![
                #(#method_infos),*
            ];

            let mut schemas = HashMap::new();

            #(#schema_insertions)*

            #methods

            json!({
                "openrpc": "1.3.2",
                "info": {
                    "title": format!("{} JSON-RPC API", stringify!(#service_name)),
                    "version": "1.0.0",
                    "description": format!("OpenRPC specification for the {} service", stringify!(#service_name))
                },
                "methods": openrpc_methods,
                "components": {
                    "schemas": schemas,
                    "errors": {
                        "ParseError": {
                            "code": -32700,
                            "message": "Parse error"
                        },
                        "InvalidRequest": {
                            "code": -32600,
                            "message": "Invalid Request"
                        },
                        "MethodNotFound": {
                            "code": -32601,
                            "message": "Method not found"
                        },
                        "InvalidParams": {
                            "code": -32602,
                            "message": "Invalid params"
                        },
                        "InternalError": {
                            "code": -32603,
                            "message": "Internal error"
                        },
                        "AuthenticationRequired": {
                            "code": -32001,
                            "message": "Authentication required"
                        },
                        "InsufficientPermissions": {
                            "code": -32002,
                            "message": "Insufficient permissions"
                        },
                        "TokenExpired": {
                            "code": -32003,
                            "message": "Token expired"
                        }
                    }
                }
            })
        }

        /// Write OpenRPC document to the target directory
        pub fn #openrpc_to_file_fn_name() -> std::io::Result<()> {
            let doc = #openrpc_fn_name();
            let output_path = #output_path_code;

            if let Some(parent) = std::path::Path::new(&output_path).parent() {
                std::fs::create_dir_all(parent)?;
            }

            let json_string = serde_json::to_string_pretty(&doc)?;
            std::fs::write(&output_path, &json_string)?;

            println!("Generated OpenRPC document at: {}", output_path);

            Ok(())
        }
    }
}
