use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, LitStr, Token, Type, parse::Parse, parse_macro_input};

mod client;
mod openrpc;
mod permissions;
mod static_hosting;

/// Macro to generate a JSON-RPC service with authentication support
///
/// This macro generates a service trait and builder that integrates with axum
/// for handling JSON-RPC requests with authentication and authorization.
///
/// See the tests for usage examples.
#[proc_macro]
pub fn jsonrpc_service(input: TokenStream) -> TokenStream {
    let service_definition = parse_macro_input!(input as ServiceDefinition);

    match generate_service_code(service_definition) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Debug)]
struct ServiceDefinition {
    service_name: Ident,
    openrpc: Option<OpenRpcConfig>,
    explorer: Option<ExplorerConfig>,
    methods: Vec<MethodDefinition>,
}

#[derive(Debug)]
enum OpenRpcConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
enum ExplorerConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
struct MethodDefinition {
    docs: Option<DocComment>,
    auth: AuthRequirement,
    name: Ident,
    request_type: Type,
    response_type: Type,
    version: Option<String>,
    wire_name: Option<String>,
    versions: Vec<MethodVersionDefinition>,
}

#[derive(Debug)]
struct MethodVersionDefinition {
    version: String,
    wire_name: String,
    request_type: Type,
    response_type: Type,
    migration_type: Type,
}

#[derive(Debug)]
struct DocComment {
    summary: String,
    description: String,
}

impl DocComment {
    fn from_lines(lines: Vec<String>) -> Option<Self> {
        let lines: Vec<String> = lines
            .into_iter()
            .map(|line| line.trim().to_string())
            .collect();
        let start = lines.iter().position(|line| !line.is_empty())?;
        let end = lines.iter().rposition(|line| !line.is_empty())?;
        let lines = &lines[start..=end];

        Some(Self {
            summary: lines[0].clone(),
            description: lines.join("\n"),
        })
    }
}

#[derive(Debug)]
enum AuthRequirement {
    Unauthorized,
    WithPermissions(Vec<Vec<String>>), // Vec of permission groups - OR between groups, AND within groups
}

const DOC_COMMENT_EXPECTED: &str = "Expected doc comment in the form `/// ...`";

fn parse_label(input: syn::parse::ParseStream) -> syn::Result<String> {
    if input.peek(LitStr) {
        Ok(input.parse::<LitStr>()?.value())
    } else {
        Ok(input.parse::<Ident>()?.to_string())
    }
}

fn parse_doc_comment_attrs(
    attrs: Vec<syn::Attribute>,
    entry_kind: &str,
) -> syn::Result<Option<DocComment>> {
    let lines = attrs
        .into_iter()
        .map(|attr| parse_doc_comment_attr(attr, entry_kind))
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(DocComment::from_lines(lines))
}

fn parse_doc_comment_attr(attr: syn::Attribute, entry_kind: &str) -> syn::Result<String> {
    if !attr.path().is_ident("doc") {
        return Err(syn::Error::new_spanned(
            attr,
            format!("Only doc comments (`/// ...`) are supported before {entry_kind} definitions"),
        ));
    }

    if let syn::Meta::NameValue(name_value) = &attr.meta
        && let syn::Expr::Lit(expr_lit) = &name_value.value
        && let syn::Lit::Str(doc_line) = &expr_lit.lit
    {
        return Ok(doc_line.value());
    }

    Err(syn::Error::new_spanned(attr, DOC_COMMENT_EXPECTED))
}

impl Parse for ServiceDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse the opening brace
        let content;
        syn::braced!(content in input);

        // Parse service_name: Ident
        let _ = content.parse::<Ident>()?; // "service_name"
        let _ = content.parse::<Token![:]>()?;
        let service_name = content.parse::<Ident>()?;
        let _ = content.parse::<Token![,]>()?;

        // Check if openrpc field is present
        let mut openrpc = None;
        let mut explorer = None;

        // Parse optional fields until we hit "methods"
        while content.peek(Ident) {
            let field_name = content.fork().parse::<Ident>()?;
            if field_name == "methods" {
                break;
            }

            let _ = content.parse::<Ident>()?; // field name
            let _ = content.parse::<Token![:]>()?;

            if field_name == "openrpc" {
                // Parse openrpc value - can be true/false or { output: "path" }
                if content.peek(syn::LitBool) {
                    let enabled = content.parse::<syn::LitBool>()?;
                    if enabled.value() {
                        openrpc = Some(OpenRpcConfig::Enabled);
                    }
                } else if content.peek(syn::token::Brace) {
                    let openrpc_content;
                    syn::braced!(openrpc_content in content);

                    // Parse output: "path"
                    let _ = openrpc_content.parse::<Ident>()?; // "output"
                    let _ = openrpc_content.parse::<Token![:]>()?;
                    let path = openrpc_content.parse::<LitStr>()?;
                    openrpc = Some(OpenRpcConfig::WithPath(path.value()));
                }
            } else if field_name == "explorer" {
                // Parse explorer value - can be true/false or { path: "/custom-path" }
                if content.peek(syn::LitBool) {
                    let enabled = content.parse::<syn::LitBool>()?;
                    if enabled.value() {
                        explorer = Some(ExplorerConfig::Enabled);
                    }
                } else if content.peek(syn::token::Brace) {
                    let explorer_content;
                    syn::braced!(explorer_content in content);

                    // Parse path: "/custom-path"
                    let _ = explorer_content.parse::<Ident>()?; // "path"
                    let _ = explorer_content.parse::<Token![:]>()?;
                    let path = explorer_content.parse::<LitStr>()?;
                    explorer = Some(ExplorerConfig::WithPath(path.value()));
                }
            }

            let _ = content.parse::<Token![,]>()?;
        }

        // Parse methods: [...]
        let _ = content.parse::<Ident>()?; // "methods"
        let _ = content.parse::<Token![:]>()?;

        let methods_content;
        syn::bracketed!(methods_content in content);

        let mut methods = Vec::new();
        while !methods_content.is_empty() {
            let method = methods_content.parse::<MethodDefinition>()?;
            methods.push(method);

            // Handle optional trailing comma
            if methods_content.peek(Token![,]) {
                let _ = methods_content.parse::<Token![,]>()?;
            }
        }

        Ok(ServiceDefinition {
            service_name,
            openrpc,
            explorer,
            methods,
        })
    }
}

impl Parse for MethodDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let docs = parse_doc_comment_attrs(input.call(syn::Attribute::parse_outer)?, "method")?;

        // Parse auth requirement (UNAUTHORIZED or WITH_PERMISSIONS([...]))
        let auth = if input.peek(syn::Ident) {
            let auth_ident = input.parse::<Ident>()?;
            match auth_ident.to_string().as_str() {
                "UNAUTHORIZED" => AuthRequirement::Unauthorized,
                "WITH_PERMISSIONS" => {
                    // Parse ([...] | [...] | ...)
                    let perms_content;
                    syn::parenthesized!(perms_content in input);

                    let mut permission_groups = Vec::new();

                    // Parse first permission group
                    let first_group_content;
                    syn::bracketed!(first_group_content in perms_content);

                    let mut first_group = Vec::new();
                    while !first_group_content.is_empty() {
                        let perm = first_group_content.parse::<LitStr>()?;
                        first_group.push(perm.value());

                        if first_group_content.peek(Token![,]) {
                            let _ = first_group_content.parse::<Token![,]>()?;
                        }
                    }
                    permission_groups.push(first_group);

                    // Parse additional permission groups separated by |
                    while perms_content.peek(Token![|]) {
                        let _ = perms_content.parse::<Token![|]>()?;

                        let group_content;
                        syn::bracketed!(group_content in perms_content);

                        let mut group = Vec::new();
                        while !group_content.is_empty() {
                            let perm = group_content.parse::<LitStr>()?;
                            group.push(perm.value());

                            if group_content.peek(Token![,]) {
                                let _ = group_content.parse::<Token![,]>()?;
                            }
                        }
                        permission_groups.push(group);
                    }

                    AuthRequirement::WithPermissions(permission_groups)
                }
                _ => {
                    return Err(syn::Error::new(
                        auth_ident.span(),
                        "Expected UNAUTHORIZED or WITH_PERMISSIONS",
                    ));
                }
            }
        } else {
            return Err(syn::Error::new(
                input.span(),
                "Expected authentication requirement",
            ));
        };

        // Parse method name
        let name = input.parse::<Ident>()?;

        // Parse (RequestType)
        let request_content;
        syn::parenthesized!(request_content in input);
        let request_type = request_content.parse::<Type>()?;

        // Parse -> ResponseType
        let _ = input.parse::<Token![->]>()?;
        let response_type = input.parse::<Type>()?;

        let mut version = None;
        let mut wire_name = None;
        let mut versions = Vec::new();

        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);

            while !content.is_empty() {
                let field_name = content.parse::<Ident>()?;
                let _ = content.parse::<Token![:]>()?;

                match field_name.to_string().as_str() {
                    "version" => {
                        version = Some(parse_label(&content)?);
                    }
                    "wire" => {
                        wire_name = Some(content.parse::<LitStr>()?.value());
                    }
                    "versions" => {
                        let versions_content;
                        syn::bracketed!(versions_content in content);

                        while !versions_content.is_empty() {
                            versions.push(versions_content.parse::<MethodVersionDefinition>()?);

                            if versions_content.peek(Token![,]) {
                                let _ = versions_content.parse::<Token![,]>()?;
                            }
                        }
                    }
                    _ => {
                        return Err(syn::Error::new(
                            field_name.span(),
                            "Expected version, wire, or versions",
                        ));
                    }
                }

                if content.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                }
            }
        }

        Ok(MethodDefinition {
            docs,
            auth,
            name,
            request_type,
            response_type,
            version,
            wire_name,
            versions,
        })
    }
}

impl Parse for MethodVersionDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let version = parse_label(input)?;

        let content;
        syn::braced!(content in input);

        let mut wire_name = None;
        let mut request_type = None;
        let mut response_type = None;
        let mut migration_type = None;

        while !content.is_empty() {
            let field_name = content.parse::<Ident>()?;
            let _ = content.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "wire" => {
                    wire_name = Some(content.parse::<LitStr>()?.value());
                }
                "request" => {
                    request_type = Some(content.parse::<Type>()?);
                }
                "response" => {
                    response_type = Some(content.parse::<Type>()?);
                }
                "migration" => {
                    migration_type = Some(content.parse::<Type>()?);
                }
                _ => {
                    return Err(syn::Error::new(
                        field_name.span(),
                        "Expected wire, request, response, or migration",
                    ));
                }
            }

            if content.peek(Token![,]) {
                let _ = content.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            version,
            wire_name: wire_name
                .ok_or_else(|| syn::Error::new(input.span(), "Version entry is missing wire"))?,
            request_type: request_type
                .ok_or_else(|| syn::Error::new(input.span(), "Version entry is missing request"))?,
            response_type: response_type.ok_or_else(|| {
                syn::Error::new(input.span(), "Version entry is missing response")
            })?,
            migration_type: migration_type.ok_or_else(|| {
                syn::Error::new(input.span(), "Version entry is missing migration")
            })?,
        })
    }
}

fn generate_service_code(service_def: ServiceDefinition) -> syn::Result<proc_macro2::TokenStream> {
    let service_name_lower = service_def.service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_jsonrpc_{}_server", service_name_lower);
    let client_mod = format_ident!("__ras_jsonrpc_{}_client", service_name_lower);

    // Generate OpenRPC code if enabled in the macro input
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

        // JSON-RPC services in this macro expose the explorer next to a single endpoint.
        // The static host generator still accepts a base path for future reuse.
        static_hosting::generate_static_hosting_code(
            &explorer_config,
            &service_def.service_name,
            "",
        )
    } else {
        quote! {}
    };

    let server_code = if cfg!(feature = "server") {
        quote! {
        mod #server_mod {
            use super::*;

            #server_impl
            #explorer_code
        }

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

    let client_code = if cfg!(feature = "client") {
        quote! {
        mod #client_mod {
            use super::*;

            #client_impl
        }

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

fn generate_server_code(service_def: &ServiceDefinition) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let service_trait_name = quote::format_ident!("{}Trait", service_name);
    let builder_name = quote::format_ident!("{}Builder", service_name);

    // Generate explorer route integration if enabled
    let explorer_route_integration =
        if service_def.explorer.is_some() && service_def.openrpc.is_some() {
            let service_name_str = service_name.to_string();
            let service_name_lower = service_name_str.to_lowercase();
            let explorer_routes_fn_str = [&service_name_lower, "_explorer_routes"].concat();
            let explorer_routes_fn = syn::Ident::new(&explorer_routes_fn_str, service_name.span());
            quote! { router = router.merge(#explorer_routes_fn(&base_url)); }
        } else {
            quote! {}
        };

    // Generate trait methods
    let trait_methods = service_def.methods.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        match &method.auth {
            AuthRequirement::Unauthorized => {
                quote! {
                    fn #method_name(&self, request: #request_type) -> impl std::future::Future<Output = Result<#response_type, Box<dyn std::error::Error + Send + Sync>>> + Send;
                }
            }
            AuthRequirement::WithPermissions(_) => {
                quote! {
                    fn #method_name(&self, user: &ras_jsonrpc_core::AuthenticatedUser, request: #request_type) -> impl std::future::Future<Output = Result<#response_type, Box<dyn std::error::Error + Send + Sync>>> + Send;
                }
            }
        }
    });

    // Generate method dispatch logic for the JSON-RPC handler
    let method_dispatch = service_def
        .methods
        .iter()
        .flat_map(generate_jsonrpc_method_dispatches);

    quote! {
        /// Generated service trait
        #[allow(private_interfaces, private_bounds)]
        pub trait #service_trait_name: Send + Sync + 'static {
            #(#trait_methods)*
        }

        /// Generated builder for the JSON-RPC service
        pub struct #builder_name<T: #service_trait_name> {
            base_url: String,
            service: std::sync::Arc<T>,
            auth_provider: Option<Box<dyn ras_jsonrpc_core::AuthProvider>>,
            auth_transport: ras_jsonrpc_core::AuthTransportConfig,
            usage_tracker: Option<Box<dyn Fn(&axum::http::HeaderMap, Option<&ras_jsonrpc_core::AuthenticatedUser>, &ras_jsonrpc_types::JsonRpcRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
            method_duration_tracker: Option<Box<dyn Fn(&str, Option<&ras_jsonrpc_core::AuthenticatedUser>, std::time::Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
        }

        impl<T: #service_trait_name> #builder_name<T> {
            /// Create a new builder with the service implementation.
            ///
            /// The JSON-RPC route defaults to `/rpc`; use `base_url` to override it.
            pub fn new(service: T) -> Self {
                Self {
                    base_url: "/rpc".to_string(),
                    service: std::sync::Arc::new(service),
                    auth_provider: None,
                    auth_transport: ras_jsonrpc_core::AuthTransportConfig::default(),
                    usage_tracker: None,
                    method_duration_tracker: None,
                }
            }

            /// Override the JSON-RPC route path.
            pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
                self.base_url = base_url.into();
                self
            }

            /// Set the auth provider
            pub fn auth_provider<A: ras_jsonrpc_core::AuthProvider>(mut self, provider: A) -> Self {
                self.auth_provider = Some(Box::new(provider));
                self
            }

            /// Enable cookie authentication alongside bearer tokens.
            pub fn auth_cookie(mut self, cookie: ras_jsonrpc_core::AuthCookieConfig) -> Self {
                self.auth_transport.cookie = Some(cookie);
                self
            }

            /// Replace the full auth transport configuration.
            pub fn auth_transport(mut self, transport: ras_jsonrpc_core::AuthTransportConfig) -> Self {
                self.auth_transport = transport;
                self
            }

            /// Require CSRF validation for cookie-authenticated JSON-RPC requests.
            pub fn csrf_protection(mut self, csrf: ras_jsonrpc_core::CsrfConfig) -> Self {
                self.auth_transport.csrf = Some(csrf);
                self
            }

            /// Set the usage tracker function
            /// This function will be called for each request with headers, authenticated user (if any), and the JSON-RPC request
            pub fn with_usage_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&axum::http::HeaderMap, Option<&ras_jsonrpc_core::AuthenticatedUser>, &ras_jsonrpc_types::JsonRpcRequest) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.usage_tracker = Some(Box::new(move |headers, user, request| {
                    Box::pin(tracker(headers, user, request))
                }));
                self
            }

            /// Set the method duration tracker function
            /// This function will be called after each method completes with the method name, authenticated user (if any), and the duration
            pub fn with_method_duration_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&str, Option<&ras_jsonrpc_core::AuthenticatedUser>, std::time::Duration) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.method_duration_tracker = Some(Box::new(move |method, user, duration| {
                    Box::pin(tracker(method, user, duration))
                }));
                self
            }

            /// Build the axum router for the JSON-RPC service
            pub fn build(self) -> Result<axum::Router, String> {
                self.auth_transport
                    .validate()
                    .map_err(|err| err.to_string())?;

                let base_url = self.base_url.clone();
                let service = std::sync::Arc::new(self);

                let rpc_handler = axum::routing::post(move |headers: axum::http::HeaderMap, body: String| {
                    let service = service.clone();
                    async move {
                        let response = service.handle_request(headers, body).await;

                        // Determine HTTP status code based on JSON-RPC error code
                        // Map authentication/authorization errors to appropriate HTTP status codes
                        // while maintaining JSON-RPC protocol compatibility
                        let status_code = if let Some(ref error) = response.error {
                            match error.code {
                                ras_jsonrpc_types::error_codes::AUTHENTICATION_REQUIRED => axum::http::StatusCode::UNAUTHORIZED,
                                ras_jsonrpc_types::error_codes::INSUFFICIENT_PERMISSIONS => axum::http::StatusCode::FORBIDDEN,
                                ras_jsonrpc_types::error_codes::TOKEN_EXPIRED => axum::http::StatusCode::UNAUTHORIZED,
                                ras_jsonrpc_types::error_codes::CSRF_VALIDATION_FAILED => axum::http::StatusCode::FORBIDDEN,
                                _ => axum::http::StatusCode::OK, // Other JSON-RPC errors still return 200 OK
                            }
                        } else {
                            axum::http::StatusCode::OK
                        };

                        (
                            status_code,
                            [("Content-Type", "application/json")],
                            serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string())
                        )
                    }
                });

                let mut router = axum::Router::new();

                // Add the JSON-RPC endpoint
                router = router.route(&base_url, rpc_handler);

                // Include explorer routes if explorer is enabled
                #explorer_route_integration

                Ok(router)
            }

            async fn handle_request(&self, headers: axum::http::HeaderMap, body: String) -> ras_jsonrpc_types::JsonRpcResponse {
                // Parse JSON-RPC request
                let request: ras_jsonrpc_types::JsonRpcRequest = match serde_json::from_str(&body) {
                    Ok(req) => req,
                    Err(_) => return ras_jsonrpc_types::JsonRpcResponse::error(ras_jsonrpc_types::JsonRpcError::parse_error(), None),
                };

                let request_id = request.id.clone();

                // Validate JSON-RPC version
                if request.jsonrpc != "2.0" {
                    return ras_jsonrpc_types::JsonRpcResponse::error(ras_jsonrpc_types::JsonRpcError::invalid_request(), request_id);
                }

                // Try to authenticate user if auth provider is available
                let auth_result = if let Some(auth_provider) = &self.auth_provider {
                    match ras_jsonrpc_core::extract_auth_credential(&headers, &self.auth_transport) {
                        Ok(credential) => {
                            if let Err(_) = ras_jsonrpc_core::validate_csrf_for_credential("POST", &headers, &credential, &self.auth_transport) {
                                return ras_jsonrpc_types::JsonRpcResponse::error(
                                    ras_jsonrpc_types::JsonRpcError::csrf_validation_failed(),
                                    request_id
                                );
                            }

                            Some(auth_provider.authenticate(credential.token().to_string()).await)
                        },
                        Err(ras_jsonrpc_core::AuthTransportError::MissingCredentials) => None,
                        Err(_) => Some(Err(ras_jsonrpc_core::AuthError::AuthenticationRequired)),
                    }
                } else {
                    None
                };

                let authenticated_user = match auth_result {
                    Some(Ok(user)) => Some(user),
                    Some(Err(ras_jsonrpc_core::AuthError::TokenExpired)) => {
                        return ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::token_expired(),
                            request_id
                        );
                    }
                    Some(Err(_)) => {
                        return ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::authentication_required(),
                            request_id
                        );
                    }
                    None => None,
                };

                // Call usage tracker if configured
                if let Some(tracker) = &self.usage_tracker {
                    let user_ref = authenticated_user.as_ref();
                    let tracker_headers =
                        ras_jsonrpc_core::redact_sensitive_headers_for_auth_transport(&headers, &self.auth_transport);
                    tracker(&tracker_headers, user_ref, &request).await;
                }

                // Dispatch method
                match request.method.as_str() {
                    #(#method_dispatch)*
                    _ => ras_jsonrpc_types::JsonRpcResponse::error(
                        ras_jsonrpc_types::JsonRpcError::method_not_found(&request.method),
                        request_id
                    )
                }
            }
        }
    }
}

fn jsonrpc_method_wire_name(method: &MethodDefinition) -> String {
    method
        .wire_name
        .clone()
        .unwrap_or_else(|| method.name.to_string())
}

fn jsonrpc_permission_groups_code(auth: &AuthRequirement) -> proc_macro2::TokenStream {
    let permission_groups = match auth {
        AuthRequirement::Unauthorized => Vec::new(),
        AuthRequirement::WithPermissions(groups) => groups.clone(),
    };

    if permission_groups.is_empty() {
        quote! { Vec::<Vec<String>>::new() }
    } else {
        let groups = permission_groups.iter().map(|group| {
            let perms = group.iter();
            quote! { vec![#(#perms.to_string()),*] }
        });
        quote! { vec![#(#groups),*] as Vec<Vec<String>> }
    }
}

fn jsonrpc_auth_check_code(
    auth: &AuthRequirement,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    match auth {
        AuthRequirement::Unauthorized => (quote! {}, quote! { None }),
        AuthRequirement::WithPermissions(_) => {
            let permission_groups_code = jsonrpc_permission_groups_code(auth);
            (
                quote! {
                    let user = match &authenticated_user {
                        Some(u) => u,
                        None => return ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::authentication_required(),
                            request.id.clone()
                        ),
                    };

                    // OR-of-AND permission check (shared ras-auth-core implementation)
                    let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
                    let provider = self.auth_provider.as_ref().expect("auth provider required for WITH_PERMISSIONS methods");
                    if let Err(error) = ras_jsonrpc_core::check_permission_groups(provider.as_ref(), user, &required_permission_groups) {
                        let (required, has) = match error {
                            ras_jsonrpc_core::AuthError::InsufficientPermissions { required, has } => (required, has),
                            _ => (Vec::new(), Vec::new()),
                        };
                        return ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::insufficient_permissions(required, has),
                            request.id.clone()
                        );
                    }
                },
                quote! { Some(user) },
            )
        }
    }
}

fn jsonrpc_parse_params_code(
    params_ident: &Ident,
    request_type: &Type,
) -> proc_macro2::TokenStream {
    quote! {
        let #params_ident: #request_type = match request.params {
            Some(params) => match serde_json::from_value(params) {
                Ok(p) => p,
                Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::invalid_params(e.to_string()),
                    request.id.clone()
                ),
            },
            None => match serde_json::from_value(serde_json::Value::Null) {
                Ok(p) => p,
                Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::invalid_params(e.to_string()),
                    request.id.clone()
                ),
            }
        };
    }
}

fn generate_jsonrpc_method_dispatches(method: &MethodDefinition) -> Vec<proc_macro2::TokenStream> {
    let mut dispatches = vec![generate_jsonrpc_canonical_dispatch(method)];
    dispatches.extend(
        method
            .versions
            .iter()
            .map(|version| generate_jsonrpc_legacy_dispatch(method, version)),
    );
    dispatches
}

fn generate_jsonrpc_canonical_dispatch(method: &MethodDefinition) -> proc_macro2::TokenStream {
    let method_name = &method.name;
    let method_wire = jsonrpc_method_wire_name(method);
    let request_type = &method.request_type;
    let params_ident = quote::format_ident!("params");
    let parse_params = jsonrpc_parse_params_code(&params_ident, request_type);
    let (auth_check, tracker_user) = jsonrpc_auth_check_code(&method.auth);

    let handler_call = match &method.auth {
        AuthRequirement::Unauthorized => quote! { self.service.#method_name(#params_ident).await },
        AuthRequirement::WithPermissions(_) => {
            quote! { self.service.#method_name(user, #params_ident).await }
        }
    };

    quote! {
        #method_wire => {
            #auth_check
            #parse_params

            let start_time = std::time::Instant::now();
            let handler_result = #handler_call;
            let duration = start_time.elapsed();

            if let Some(duration_tracker) = &self.method_duration_tracker {
                duration_tracker(#method_wire, #tracker_user, duration).await;
            }

            match handler_result {
                Ok(result) => {
                    match serde_json::to_value(result) {
                        Ok(result_value) => ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id.clone()),
                        Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                            request.id.clone()
                        ),
                    }
                }
                Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                    request.id.clone()
                ),
            }
        }
    }
}

fn generate_jsonrpc_legacy_dispatch(
    method: &MethodDefinition,
    version: &MethodVersionDefinition,
) -> proc_macro2::TokenStream {
    let method_name = &method.name;
    let method_wire = &version.wire_name;
    let canonical_request_type = &method.request_type;
    let canonical_response_type = &method.response_type;
    let legacy_request_type = &version.request_type;
    let legacy_response_type = &version.response_type;
    let migration_type = &version.migration_type;
    let legacy_params_ident = quote::format_ident!("legacy_params");
    let params_ident = quote::format_ident!("params");
    let parse_params = jsonrpc_parse_params_code(&legacy_params_ident, legacy_request_type);
    let (auth_check, tracker_user) = jsonrpc_auth_check_code(&method.auth);

    let handler_call = match &method.auth {
        AuthRequirement::Unauthorized => quote! { self.service.#method_name(#params_ident).await },
        AuthRequirement::WithPermissions(_) => {
            quote! { self.service.#method_name(user, #params_ident).await }
        }
    };

    quote! {
        #method_wire => {
            #auth_check
            #parse_params

            let #params_ident: #canonical_request_type =
                match <#migration_type as ras_jsonrpc_core::VersionMigration<#legacy_request_type, #canonical_request_type>>::migrate(#legacy_params_ident) {
                    Ok(params) => params,
                    Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                        ras_jsonrpc_types::JsonRpcError::invalid_params(e.to_string()),
                        request.id.clone()
                    ),
                };

            let start_time = std::time::Instant::now();
            let handler_result = #handler_call;
            let duration = start_time.elapsed();

            if let Some(duration_tracker) = &self.method_duration_tracker {
                duration_tracker(#method_wire, #tracker_user, duration).await;
            }

            match handler_result {
                Ok(result) => {
                    let result: #legacy_response_type =
                        match <#migration_type as ras_jsonrpc_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(result) {
                            Ok(result) => result,
                            Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                                ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                                request.id.clone()
                            ),
                        };

                    match serde_json::to_value(result) {
                        Ok(result_value) => ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id.clone()),
                        Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                            request.id.clone()
                        ),
                    }
                }
                Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                    request.id.clone()
                ),
            }
        }
    }
}
