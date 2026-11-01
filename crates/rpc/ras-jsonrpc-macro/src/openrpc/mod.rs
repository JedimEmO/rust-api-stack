//! OpenRPC document generation module
//!
//! This module provides functionality to generate OpenRPC specification documents
//! from the jsonrpc_service macro definitions.

use crate::{OpenRpcConfig, ServiceDefinition};
use proc_macro2::{Ident, TokenStream};
use quote::quote;
mod methods;
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

/// Generates OpenRPC document creation code
pub fn generate_openrpc_code(
    service_def: &ServiceDefinition,
    config: &OpenRpcConfig,
) -> TokenStream {
    let service_name = &service_def.service_name;
    let service_name_lower = service_name.to_string().to_lowercase();
    let openrpc_fn_name = quote::format_ident!("generate_{}_openrpc", service_name_lower);
    let try_openrpc_fn_name = quote::format_ident!("try_generate_{}_openrpc", service_name_lower);
    let service_str = service_name.to_string();
    let openrpc_to_file_fn_name =
        quote::format_ident!("generate_{}_openrpc_to_file", service_name_lower);
    let method_info_struct_name = quote::format_ident!("{}OpenRpcMethodInfo", service_name);
    let components_mod = quote::format_ident!("__ras_openrpc_components_{}", service_name_lower);

    let output_path_code = match config {
        OpenRpcConfig::Enabled => {
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

    let embedded = embedded_components(&components_mod);
    let method_infos =
        methods::generate_method_infos(service_def, &method_info_struct_name, &components_mod);
    let methods = methods::generate_methods(&service_str, &components_mod);

    quote! {
        #embedded

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
            /// `None` for a `()` request, which takes no parameters.
            params_schema: Option<serde_json::Value>,
            result_schema: serde_json::Value,
            /// Authored examples; only the canonical method has them.
            examples: Vec<#components_mod::Example>,
            version: Option<String>,
            canonical_version: Option<String>,
            canonical_method: String,
        }

        /// Generate the OpenRPC document for this service.
        ///
        /// Panics when an authored example cannot be serialized; see the
        /// fallible variant next to it.
        pub fn #openrpc_fn_name() -> serde_json::Value {
            #try_openrpc_fn_name().unwrap_or_else(|error| panic!("{error}"))
        }

        /// Generate the OpenRPC document for this service, or say which example
        /// or part of the document could not be built.
        pub fn #try_openrpc_fn_name() -> Result<serde_json::Value, ras_jsonrpc_types::SpecGenerationError> {
            use serde_json::json;

            let mut generators = #components_mod::Generators::new(#components_mod::Dialect::OpenRpc);

            let mut methods = vec![
                #(#method_infos),*
            ];

            let components = generators.into_components().map_err(|error| {
                ras_jsonrpc_types::SpecGenerationError::document(#service_str, error)
            })?;
            for method in &mut methods {
                components.fix_serialized(&mut method.result_schema);
            }
            let schemas = components.schemas;

            #methods

            Ok(json!({
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
            }))
        }

        /// Write OpenRPC document to the target directory
        pub fn #openrpc_to_file_fn_name() -> std::io::Result<()> {
            let doc = #try_openrpc_fn_name().map_err(std::io::Error::other)?;
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
