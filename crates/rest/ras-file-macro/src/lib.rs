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

    let server_output = if cfg!(feature = "server") {
        quote! {
        mod #server_mod {
            use super::*;
            #server_code
        }

        pub use #server_mod::*;

        const _: () = {
            #schema_checks
        };

        mod #openapi_mod {
            use super::*;
            #openapi_code
        }

        pub use #openapi_mod::*;
        }
    } else {
        quote! {}
    };

    let client_output = if cfg!(feature = "client") {
        quote! {
        mod #client_mod {
            use super::*;
            #client_code
        }

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
