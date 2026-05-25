//! Static API explorer hosting for REST services

use crate::ServiceDefinition;
use proc_macro2::TokenStream;
use quote::quote;

/// Configuration for static file hosting.
#[derive(Debug, Clone)]
pub struct StaticHostingConfig {
    /// Whether to enable static hosting.
    pub serve_docs: bool,
    /// URL path for documentation (default "/docs").
    pub docs_path: String,
    /// UI theme selection retained for macro compatibility.
    pub ui_theme: String,
}

impl Default for StaticHostingConfig {
    fn default() -> Self {
        Self {
            serve_docs: false,
            docs_path: "/docs".to_string(),
            ui_theme: "default".to_string(),
        }
    }
}

/// Generates static API explorer handler code.
pub fn generate_static_hosting_code(
    service_def: &ServiceDefinition,
    static_config: &StaticHostingConfig,
) -> TokenStream {
    if !static_config.serve_docs {
        return quote! {};
    }

    const TEMPLATE_CONTENT: &str = include_str!("api_explorer_template.html");

    let service_name = &service_def.service_name;
    let base_path = service_def.base_path.trim_end_matches('/').to_string();
    let docs_path = ensure_leading_slash(&static_config.docs_path);
    let openapi_route = format!("{}/openapi.json", docs_path.trim_end_matches('/'));
    let spec_path = join_paths(&base_path, &openapi_route);
    let api_base_path = if base_path.is_empty() {
        "/".to_string()
    } else {
        base_path
    };

    let openapi_fn_name = quote::format_ident!(
        "generate_{}_openapi",
        service_name.to_string().to_lowercase()
    );
    let docs_handler_name =
        quote::format_ident!("{}_docs_handler", service_name.to_string().to_lowercase());
    let template_lit = syn::LitStr::new(TEMPLATE_CONTENT, proc_macro2::Span::call_site());

    quote! {
        async fn #docs_handler_name() -> ::axum::response::Html<String> {
            static HTML: ::std::sync::OnceLock<String> = ::std::sync::OnceLock::new();

            let html = HTML.get_or_init(|| {
                const TEMPLATE: &str = #template_lit;
                let config_json = ::serde_json::json!({
                    "serviceName": stringify!(#service_name),
                    "protocol": "rest",
                    "specPath": #spec_path,
                    "apiBasePath": #api_base_path
                })
                .to_string()
                .replace("<", "\\u003c");

                TEMPLATE.replace("{EXPLORER_CONFIG_JSON}", &config_json)
            });

            ::axum::response::Html(html.clone())
        }

        async fn openapi_json_handler() -> ::axum::Json<::serde_json::Value> {
            ::axum::Json(#openapi_fn_name())
        }
    }
}

/// Generates route registrations for static hosting.
pub fn generate_static_routes(
    service_def: &ServiceDefinition,
    static_config: &StaticHostingConfig,
) -> TokenStream {
    if !static_config.serve_docs {
        return quote! {};
    }

    let docs_path = ensure_leading_slash(&static_config.docs_path);
    let openapi_path = format!("{}/openapi.json", docs_path.trim_end_matches('/'));
    let docs_handler_name = quote::format_ident!(
        "{}_docs_handler",
        service_def.service_name.to_string().to_lowercase()
    );

    quote! {
        {
            router = router
                .route(#docs_path, ::axum::routing::get(#docs_handler_name))
                .route(#openapi_path, ::axum::routing::get(openapi_json_handler));
        }
    }
}

fn ensure_leading_slash(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn join_paths(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = ensure_leading_slash(path);

    if base.is_empty() {
        path
    } else {
        format!("{base}{path}")
    }
}
