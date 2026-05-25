use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

mod client;
mod openapi;
mod parser;
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

    // Only include server code when not targeting wasm32
    let expanded = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        mod #server_mod {
            use super::*;
            #server_code
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub use #server_mod::*;

        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            #schema_checks
        };

        #[cfg(not(target_arch = "wasm32"))]
        mod #openapi_mod {
            use super::*;
            #openapi_code
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub use #openapi_mod::*;

        mod #client_mod {
            use super::*;
            #client_code
        }

        pub use #client_mod::*;
    };

    TokenStream::from(expanded)
}
