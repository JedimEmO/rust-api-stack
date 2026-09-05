//! Assemble server, client, and specification expansions.

use crate::{ast::*, openrpc, permissions, server::generate_server_code, static_hosting};
use quote::{format_ident, quote};

pub(crate) fn generate_service_code(
    service_def: ServiceDefinition,
) -> syn::Result<proc_macro2::TokenStream> {
    let service_name_lower = service_def.service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_jsonrpc_{}_server", service_name_lower);
    let client_mod = format_ident!("__ras_jsonrpc_{}_client", service_name_lower);

    let (openrpc_code, schema_checks) = if let Some(openrpc_config) = &service_def.openrpc {
        (
            openrpc::generate_openrpc_code(&service_def, openrpc_config),
            openrpc::generate_schema_impl_checks(&service_def),
        )
    } else {
        (quote! {}, quote! {})
    };

    let server_impl = generate_server_code(&service_def);

    let explorer_code = if service_def.explorer.is_some() && service_def.openrpc.is_some() {
        let explorer_config = match &service_def.explorer {
            Some(ExplorerConfig::Enabled) => static_hosting::StaticHostingConfig {
                serve_explorer: true,
                explorer_path: "/explorer".to_string(),
            },
            Some(ExplorerConfig::WithPath(path)) => static_hosting::StaticHostingConfig {
                serve_explorer: true,
                explorer_path: path.clone(),
            },
            None => static_hosting::StaticHostingConfig::default(),
        };

        // The explorer and RPC endpoint share the service root.
        static_hosting::generate_static_hosting_code(
            &explorer_config,
            &service_def.service_name,
            "",
        )
    } else {
        quote! {}
    };

    // With `feature_gated: true` the generated code is wrapped in
    // `#[cfg(feature = ...)]` attributes resolved against the CONSUMER
    // crate's features, immune to workspace feature unification of the
    // macro crate's own features (which `cfg!` evaluates).
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
        quote! {
        #cfg_server
        mod #server_mod {
            use super::*;

            #server_impl
            #explorer_code
        }

        #cfg_server
        pub use #server_mod::*;
        }
    } else {
        quote! {}
    };

    let client_impl = crate::client::generate_client_code(&service_def);
    let permissions_code = if cfg!(feature = "permissions") {
        permissions::generate_permissions_code(&service_def)
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
        #openrpc_code
        #schema_checks
        #server_code
        #client_code
    };

    Ok(output)
}
