use crate::ServiceDefinition;
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;

/// Generates code to include schema generation for types when schemars is available
pub fn generate_schema_impl_checks(service_def: &ServiceDefinition) -> TokenStream {
    let mut unique_types = HashMap::new();

    for endpoint in &service_def.endpoints {
        if let Some(request_type) = &endpoint.request_type {
            unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
        }

        let response_type = &endpoint.response_type;
        unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

        for path_param in &endpoint.path_params {
            let param_type = &path_param.param_type;
            unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
        }

        for query_param in &endpoint.query_params {
            let param_type = &query_param.param_type;
            unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
        }

        for version in &endpoint.versions {
            if let Some(request_type) = &version.request_type {
                unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
            }

            let response_type = &version.response_type;
            unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

            for path_param in &version.path_params {
                let param_type = &path_param.param_type;
                unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
            }

            for query_param in &version.query_params {
                let param_type = &query_param.param_type;
                unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
            }
        }
    }

    let type_checks: Vec<TokenStream> = unique_types
        .values()
        .map(|type_tokens| {
            quote! {
                const _: () = {
                    fn _assert_json_schema<T: schemars::JsonSchema>() {}
                    fn _check() {
                        _assert_json_schema::<#type_tokens>();
                    }
                };
            }
        })
        .collect();

    quote! {
        #(#type_checks)*
    }
}
