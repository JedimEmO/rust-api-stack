use proc_macro::TokenStream;
use quote::quote;
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
    let (openapi_code, schema_checks) = if definition.openapi.is_some() {
        let openapi_config = definition.openapi.as_ref().unwrap();
        (
            openapi::generate_openapi_code(&definition, openapi_config),
            openapi::generate_schema_impl_checks(&definition),
        )
    } else {
        (quote! {}, quote! {})
    };

    // Only include server code when not targeting wasm32
    let expanded = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        mod server_impl {
            use super::*;
            #server_code
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub use server_impl::*;

        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            #schema_checks
        };

        #[cfg(not(target_arch = "wasm32"))]
        mod openapi_impl {
            use super::*;
            #openapi_code
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub use openapi_impl::*;

        #client_code
    };

    TokenStream::from(expanded)
}
