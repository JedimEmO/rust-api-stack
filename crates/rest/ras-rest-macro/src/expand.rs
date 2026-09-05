//! Assemble protocol, server, client, and specification expansions.

use crate::{ast::*, openapi, permissions, static_hosting};
use quote::{format_ident, quote};

pub(crate) fn generate_service_code(
    service_def: ServiceDefinition,
) -> syn::Result<proc_macro2::TokenStream> {
    let service_name = &service_def.service_name;
    let service_name_lower = service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_rest_{}_server", service_name_lower);
    let client_mod = format_ident!("__ras_rest_{}_client", service_name_lower);

    let (openapi_code, schema_checks) = if let Some(openapi_config) = &service_def.openapi {
        (
            openapi::generate_openapi_code(&service_def, openapi_config),
            openapi::generate_schema_impl_checks(&service_def),
        )
    } else {
        (quote! {}, quote! {})
    };

    let static_hosting_code = if service_def.static_hosting.serve_docs {
        static_hosting::generate_static_hosting_code(&service_def, &service_def.static_hosting)
    } else {
        quote! {}
    };

    let client_impl = crate::client::generate_client_code(&service_def);
    let permissions_code = if cfg!(feature = "permissions") {
        permissions::generate_permissions_code(&service_def)
    } else {
        quote! {}
    };

    // `cfg!(feature = ...)` below evaluates the MACRO crate's features, which
    // Cargo unifies across the whole workspace — one crate enabling `client`
    // forces client codegen into every consumer's expansion. With
    // `feature_gated: true` the generated code is instead wrapped in
    // `#[cfg(feature = ...)]` attributes that resolve against the CONSUMER
    // crate's own `server`/`client` features, immune to unification.
    let feature_gated = service_def.feature_gated;
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

    let server_code = if feature_gated || cfg!(feature = "server") {
        let server_impl = crate::server::generate_server_code(
            &service_def,
            schema_checks,
            openapi_code,
            static_hosting_code,
        );
        quote! {
            #cfg_server
            mod #server_mod { use super::*; #server_impl }
            #cfg_server
            pub use #server_mod::*;
        }
    } else {
        quote! {}
    };

    let client_code = if feature_gated || cfg!(feature = "client") {
        quote! {
        #cfg_client
        mod #client_mod {
            use super::*;

            #client_impl
        }

        #cfg_client
        pub use #client_mod::*;
        }
    } else {
        quote! {}
    };

    let output = quote! {
        #permissions_code
        #server_code
        #client_code
    };

    Ok(output)
}
