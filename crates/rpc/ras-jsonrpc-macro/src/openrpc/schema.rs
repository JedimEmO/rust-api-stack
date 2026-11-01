use crate::ServiceDefinition;
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;

/// Generates code to include schema generation for types when schemars is available
pub fn generate_schema_impl_checks(service_def: &ServiceDefinition) -> TokenStream {
    let mut unique_types = HashMap::new();

    for method in &service_def.methods {
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
        unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

        for version in &method.versions {
            let request_type = &version.request_type;
            let response_type = &version.response_type;

            unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
            unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));
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
