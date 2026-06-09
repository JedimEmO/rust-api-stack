use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

mod client;
mod openapi;
mod parser;
mod permissions;
mod server;

use parser::FileServiceDefinition;

#[proc_macro]
pub fn file_service(input: TokenStream) -> TokenStream {
    let definition = parse_macro_input!(input as FileServiceDefinition);

    let server_code = server::generate_server(&definition);
    let client_code = client::generate_client(&definition);

    // Generate OpenAPI code if enabled
    let (openapi_code, schema_checks) = if let Some(openapi_config) = &definition.openapi {
        (
            openapi::generate_openapi_code(&definition, openapi_config),
            openapi::generate_schema_impl_checks(&definition),
        )
    } else {
        (quote! {}, quote! {})
    };

    let service_name_lower = definition.service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_file_{}_server", service_name_lower);
    let openapi_mod = format_ident!("__ras_file_{}_openapi", service_name_lower);
    let client_mod = format_ident!("__ras_file_{}_client", service_name_lower);
    let permissions_code = if cfg!(feature = "permissions") {
        permissions::generate_permissions_code(&definition)
    } else {
        quote! {}
    };

    // With `feature_gated: true` the generated code is wrapped in
    // `#[cfg(feature = ...)]` attributes resolved against the CONSUMER
    // crate's features, immune to workspace feature unification of the
    // macro crate's own features (which `cfg!` evaluates).
    let feature_gated = definition.feature_gated;
    let cfg_server = if feature_gated {
        quote! { #[cfg(feature = "server")] }
    } else {
        quote! {}
    };
    let cfg_client = if feature_gated {
        quote! { #[cfg(feature = "client")] }
    } else {
        quote! {}
    };

    let server_output = if feature_gated || cfg!(feature = "server") {
        quote! {
        #cfg_server
        mod #server_mod {
            use super::*;
            #server_code
        }

        #cfg_server
        pub use #server_mod::*;

        #cfg_server
        const _: () = {
            #schema_checks
        };

        #cfg_server
        mod #openapi_mod {
            use super::*;
            #openapi_code
        }

        #cfg_server
        pub use #openapi_mod::*;
        }
    } else {
        quote! {}
    };

    let client_output = if feature_gated || cfg!(feature = "client") {
        quote! {
        #cfg_client
        mod #client_mod {
            use super::*;
            #client_code
        }

        #cfg_client
        pub use #client_mod::*;
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #permissions_code
        #server_output
        #client_output
    };

    TokenStream::from(expanded)
}
