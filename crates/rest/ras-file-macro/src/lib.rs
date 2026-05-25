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

    let expanded = quote! {
        #[cfg(feature = "server")]
        mod #server_mod {
            use super::*;
            #server_code
        }

        #[cfg(feature = "server")]
        pub use #server_mod::*;

        #[cfg(feature = "server")]
        const _: () = {
            #schema_checks
        };

        #[cfg(feature = "server")]
        mod #openapi_mod {
            use super::*;
            #openapi_code
        }

        #[cfg(feature = "server")]
        pub use #openapi_mod::*;

        #[cfg(feature = "client")]
        mod #client_mod {
            use super::*;
            #client_code
        }

        #[cfg(feature = "client")]
        pub use #client_mod::*;
    };

    TokenStream::from(expanded)
}
