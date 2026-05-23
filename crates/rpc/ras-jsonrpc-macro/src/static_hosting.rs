use proc_macro2::TokenStream;
use quote::quote;

/// Configuration for static hosting of the JSON-RPC explorer
#[derive(Debug, Clone)]
pub struct StaticHostingConfig {
    /// Whether to serve the explorer
    pub serve_explorer: bool,
    /// Path where the explorer will be served
    pub explorer_path: String,
}

impl Default for StaticHostingConfig {
    fn default() -> Self {
        Self {
            serve_explorer: false,
            explorer_path: "/explorer".to_string(),
        }
    }
}

/// Generate code for static hosting of the JSON-RPC explorer
pub fn generate_static_hosting_code(
    config: &StaticHostingConfig,
    service_name: &syn::Ident,
    _base_path: &str,
) -> TokenStream {
    if !config.serve_explorer {
        return TokenStream::new();
    }

    const TEMPLATE_CONTENT: &str =
        include_str!("../../../rest/ras-rest-macro/src/api_explorer_template.html");

    let explorer_path_suffix = normalize_explorer_path(&config.explorer_path);
    let service_name_str = service_name.to_string();
    let service_name_lower = service_name_str.to_lowercase();
    let openrpc_fn_name_str = ["generate_", &service_name_lower, "_openrpc"].concat();
    let explorer_routes_fn_str = [&service_name_lower, "_explorer_routes"].concat();
    let openrpc_fn_name = syn::Ident::new(&openrpc_fn_name_str, service_name.span());
    let explorer_routes_fn = syn::Ident::new(&explorer_routes_fn_str, service_name.span());

    // Embed the template as a string literal
    let template_lit = syn::LitStr::new(TEMPLATE_CONTENT, proc_macro2::Span::call_site());

    quote! {
        /// Routes for the JSON-RPC explorer
        pub fn #explorer_routes_fn(base_path: &str) -> ::axum::Router {
            use ::axum::{response::Html, routing::get, Json};

            let explorer_path = format!("{}{}", base_path.trim_end_matches('/'), #explorer_path_suffix);
            let openrpc_path = format!("{}/openrpc.json", &explorer_path);

            let explorer_html = {
                const TEMPLATE: &str = #template_lit;
                let config_json = ::serde_json::json!({
                    "serviceName": #service_name_str,
                    "protocol": "jsonrpc",
                    "specPath": &openrpc_path,
                    "apiBasePath": base_path
                })
                .to_string()
                .replace("<", "\\u003c");

                ::std::sync::Arc::new(TEMPLATE.replace("{EXPLORER_CONFIG_JSON}", &config_json))
            };

            let serve_explorer = {
                let explorer_html = explorer_html.clone();
                move || {
                    let explorer_html = explorer_html.clone();
                    async move {
                        Html((*explorer_html).clone())
                    }
                }
            };

            async fn serve_openrpc() -> Json<::serde_json::Value> {
                let doc = #openrpc_fn_name();
                Json(::serde_json::to_value(doc).unwrap())
            }

            ::axum::Router::new()
                .route(&explorer_path, get(serve_explorer))
                .route(&openrpc_path, get(serve_openrpc))
        }
    }
}

fn normalize_explorer_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::format_ident;

    #[test]
    fn default_config_disables_explorer_on_default_path() {
        let config = StaticHostingConfig::default();
        assert!(!config.serve_explorer);
        assert_eq!(config.explorer_path, "/explorer");
    }

    #[test]
    fn disabled_config_generates_no_tokens() {
        let config = StaticHostingConfig::default();
        let tokens = generate_static_hosting_code(&config, &format_ident!("UserService"), "");
        assert!(tokens.is_empty());
    }

    #[test]
    fn explorer_path_is_normalized_before_code_generation() {
        assert_eq!(normalize_explorer_path("api/docs/"), "/api/docs");
        assert_eq!(normalize_explorer_path("/api/docs/"), "/api/docs");
        assert_eq!(normalize_explorer_path("/explorer"), "/explorer");
    }

    #[test]
    fn enabled_config_generates_explorer_and_openrpc_routes() {
        let config = StaticHostingConfig {
            serve_explorer: true,
            explorer_path: "api/docs/".to_string(),
        };

        let tokens =
            generate_static_hosting_code(&config, &format_ident!("UserService"), "").to_string();

        assert!(tokens.contains("userservice_explorer_routes"));
        assert!(tokens.contains("generate_userservice_openrpc"));
        assert!(tokens.contains("/api/docs"));
        assert!(tokens.contains("openrpc.json"));
        assert!(!tokens.contains("api/docs/"));
    }
}
