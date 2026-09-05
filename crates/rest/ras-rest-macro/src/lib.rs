use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, LitStr, Token, Type, parse::Parse, parse_macro_input};

mod client;
mod openapi;
mod permissions;
mod static_hosting;

/// Macro to generate a REST service with authentication support
///
/// This macro generates a service trait and builder that integrates with axum
/// for handling REST requests with authentication and authorization.
///
/// Supports HTTP methods: GET, POST, PUT, DELETE, PATCH
/// Supports path parameters and request bodies
/// Generates OpenAPI 3.0 documents using schemars
///
/// # Auth levels
///
/// Each endpoint declares one of three auth levels:
///
/// * `UNAUTHORIZED` — public; the handler receives no caller.
/// * `OPTIONAL_AUTH` — public, but opportunistically identified: the route is
///   never rejected for auth reasons and the handler receives a
///   [`ras_auth_core::Caller`] (`Anonymous`, or `Authenticated(user)` when a
///   valid credential is present). A present-but-bad credential (invalid/expired
///   token, or a cookie that fails CSRF on an unsafe method) resolves to
///   `Anonymous` rather than rejecting.
/// * `WITH_PERMISSIONS([...])` — authenticated and gated; a missing or
///   insufficient credential is rejected before the handler runs.
///
/// # Request bodies and `Content-Type`
///
/// Endpoints that declare a body type read and JSON-decode it only **after** the
/// auth/CSRF/permission checks pass, so unauthenticated callers cannot make the
/// server buffer or parse payloads. By default a request whose `Content-Type` is
/// not `application/json` (ignoring parameters such as `; charset=utf-8`) is
/// rejected with `415 Unsupported Media Type` before the body is read. This is
/// defense-in-depth: requiring `application/json` forces a CORS preflight for
/// cross-origin requests, closing the simple-request CSRF shape (a cross-origin
/// `text/plain` POST). Set `require_json_content_type: false` at the service
/// level to accept any content type (e.g. for clients that cannot set the
/// header). A malformed body is logged (category + line/column, never the
/// rejected value) and answered with `400`; a body over the size limit is `413`,
/// distinct from an unreadable stream (`400`).
///
/// # Service options
///
/// * `body_limit: <bytes>` — maximum request body size (default 2 MiB).
/// * `require_json_content_type: <bool>` — enforce `application/json` on bodied
///   endpoints (default `true`).
/// * `serve_docs: <bool>` / `docs_path: "..."` — host the API explorer and
///   `openapi.json`.
/// * `docs_require_auth: <bool>` — when `serve_docs` is enabled, gate the docs
///   page and `openapi.json` behind authentication (any authenticated user).
///   Default `false`: docs are public, matching conventional API explorers.
/// * `feature_gated: <bool>` — wrap the generated server/client behind the
///   consumer crate's own `server`/`client` features.
///
/// # Per-endpoint options
///
/// A trailing `{ ... }` block after the response type accepts:
///
/// * `body_limit: <bytes>` — override the service body limit for this endpoint.
/// * `headers: true` — pass the request [`axum::http::HeaderMap`] to the handler
///   as an extra parameter, immediately after the caller/user and before the
///   path parameters. The map is unredacted (it contains the caller's
///   `Authorization`/`Cookie`/CSRF headers), so must not be logged or forwarded
///   verbatim; redact with
///   `ras_auth_core::redact_sensitive_headers_for_auth_transport` first.
/// * `version: "..."` / `versions: [ ... ]` — see Versioning.
///
/// # Versioning
///
/// An endpoint may serve older payload shapes at legacy paths and migrate them
/// to the canonical request/response types. Provide a canonical `version:` label
/// and one or more `versions: [ "vN" { path: ..., request: T, response: U,
/// migration: M }, ... ]` entries, where `M` implements
/// [`ras_rest_core::VersionMigration`] for both the request (legacy → canonical)
/// and the response (canonical → legacy). Each legacy path is registered as its
/// own route sharing the endpoint's auth level.
///
/// # Example
///
/// ```rust
/// use ras_rest_macro::rest_service;
/// use serde::{Deserialize, Serialize};
/// use schemars::JsonSchema;
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct UsersResponse {
///     users: Vec<()>,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct CreateUserRequest {
///     name: String,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct UserResponse {
///     id: String,
///     name: String,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct UpdateUserRequest {
///     name: String,
/// }
///
/// rest_service!({
///     service_name: UserService,
///     base_path: "/api/v1",
///     openapi: true,
///     serve_docs: true,
///     docs_path: "/docs",
///     ui_theme: "default",
///     endpoints: [
///         GET UNAUTHORIZED users() -> UsersResponse,
///         GET OPTIONAL_AUTH feed() -> UsersResponse,
///         POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> UserResponse,
///         GET WITH_PERMISSIONS(["user"]) users/{id: String}() -> UserResponse,
///         PUT WITH_PERMISSIONS(["admin"]) users/{id: String}(UpdateUserRequest) -> UserResponse,
///         DELETE WITH_PERMISSIONS(["admin"]) users/{id: String}() -> (),
///     ]
/// });
///
/// # fn main() {}
/// ```
#[proc_macro]
pub fn rest_service(input: TokenStream) -> TokenStream {
    let service_definition = parse_macro_input!(input as ServiceDefinition);

    match generate_service_code(service_definition) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Debug)]
struct ServiceDefinition {
    service_name: Ident,
    base_path: String,
    openapi: Option<OpenApiConfig>,
    static_hosting: static_hosting::StaticHostingConfig,
    body_limit: Option<usize>,
    feature_gated: bool,
    /// Require an `application/json` request `Content-Type` on every endpoint
    /// that declares a body. Defaults to `true`. Set `require_json_content_type:
    /// false` to opt out (e.g. for clients that cannot set the header).
    require_json_content_type: bool,
    /// Gate the generated docs page and `openapi.json` behind authentication
    /// (any authenticated user). Defaults to `false` — docs are public when
    /// `serve_docs` is enabled, matching conventional API-explorer behavior.
    docs_require_auth: bool,
    endpoints: Vec<EndpointDefinition>,
}

/// Default maximum JSON body size in bytes (matches axum's default).
const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug)]
enum OpenApiConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
struct EndpointDefinition {
    docs: Option<DocComment>,
    method: HttpMethod,
    auth: AuthRequirement,
    path: String,
    path_params: Vec<PathParam>,
    query_params: Vec<QueryParam>,
    request_type: Option<Type>,
    response_type: Type,
    handler_name: Ident,
    version: Option<String>,
    versions: Vec<EndpointVersionDefinition>,
    /// Per-endpoint request body size cap (bytes). Overrides the service-level
    /// `body_limit` for this endpoint when set.
    body_limit: Option<usize>,
    /// When `true`, the handler receives the request `HeaderMap` as an extra
    /// parameter (immediately after the caller/user, before path params).
    with_headers: bool,
}

#[derive(Debug)]
struct EndpointVersionDefinition {
    version: String,
    path: String,
    path_params: Vec<PathParam>,
    query_params: Vec<QueryParam>,
    request_type: Option<Type>,
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

#[derive(Debug, Clone)]
enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    fn as_axum_method(&self) -> proc_macro2::TokenStream {
        match self {
            HttpMethod::Get => quote! { axum::routing::get },
            HttpMethod::Post => quote! { axum::routing::post },
            HttpMethod::Put => quote! { axum::routing::put },
            HttpMethod::Delete => quote! { axum::routing::delete },
            HttpMethod::Patch => quote! { axum::routing::patch },
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
        }
    }
}

#[derive(Debug, Clone)]
struct PathParam {
    name: Ident,
    param_type: Type,
}

#[derive(Debug, Clone)]
struct QueryParam {
    name: Ident,
    param_type: Type,
}

#[derive(Debug)]
enum AuthRequirement {
    Unauthorized,
    /// Public route that opportunistically identifies its caller. Never rejected
    /// for auth reasons; the handler receives a `ras_auth_core::Caller`.
    OptionalAuth,
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

        // Parse base_path: "string"
        let _ = content.parse::<Ident>()?; // "base_path"
        let _ = content.parse::<Token![:]>()?;
        let base_path_lit = content.parse::<LitStr>()?;
        let base_path = base_path_lit.value();
        let _ = content.parse::<Token![,]>()?;

        // Parse optional fields (openapi, serve_docs, docs_path, ui_theme, body_limit)
        let mut openapi = None;
        let mut static_hosting = static_hosting::StaticHostingConfig::default();
        let mut body_limit = None;
        let mut feature_gated = false;
        let mut require_json_content_type = true;
        let mut docs_require_auth = false;

        // Parse optional fields
        while content.peek(Ident) {
            let field_name = content.fork().parse::<Ident>()?;

            if field_name == "openapi" {
                let _ = content.parse::<Ident>()?; // "openapi"
                let _ = content.parse::<Token![:]>()?;

                // Parse openapi value - can be true/false or { output: "path" }
                if content.peek(syn::LitBool) {
                    let enabled = content.parse::<syn::LitBool>()?;
                    if enabled.value() {
                        openapi = Some(OpenApiConfig::Enabled);
                    }
                } else if content.peek(syn::token::Brace) {
                    let openapi_content;
                    syn::braced!(openapi_content in content);

                    // Parse output: "path"
                    let _ = openapi_content.parse::<Ident>()?; // "output"
                    let _ = openapi_content.parse::<Token![:]>()?;
                    let path = openapi_content.parse::<LitStr>()?;
                    openapi = Some(OpenApiConfig::WithPath(path.value()));
                }

                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "serve_docs" {
                let _ = content.parse::<Ident>()?; // "serve_docs"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                static_hosting.serve_docs = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "docs_path" {
                let _ = content.parse::<Ident>()?; // "docs_path"
                let _ = content.parse::<Token![:]>()?;
                let path = content.parse::<LitStr>()?;
                static_hosting.docs_path = path.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "ui_theme" {
                let _ = content.parse::<Ident>()?; // "ui_theme"
                let _ = content.parse::<Token![:]>()?;
                let theme = content.parse::<LitStr>()?;
                static_hosting.ui_theme = theme.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "body_limit" {
                let _ = content.parse::<Ident>()?; // "body_limit"
                let _ = content.parse::<Token![:]>()?;
                let limit = content.parse::<syn::LitInt>()?;
                body_limit = Some(limit.base10_parse::<usize>()?);
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "feature_gated" {
                let _ = content.parse::<Ident>()?; // "feature_gated"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                feature_gated = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "require_json_content_type" {
                let _ = content.parse::<Ident>()?; // "require_json_content_type"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                require_json_content_type = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "docs_require_auth" {
                let _ = content.parse::<Ident>()?; // "docs_require_auth"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                docs_require_auth = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "endpoints" {
                break; // Start parsing endpoints
            } else {
                return Err(syn::Error::new(
                    field_name.span(),
                    format!("Unknown field: {}", field_name),
                ));
            }
        }

        // Parse endpoints: [...]
        let _ = content.parse::<Ident>()?; // "endpoints"
        let _ = content.parse::<Token![:]>()?;

        let endpoints_content;
        syn::bracketed!(endpoints_content in content);

        let mut endpoints = Vec::new();
        while !endpoints_content.is_empty() {
            let endpoint = endpoints_content.parse::<EndpointDefinition>()?;
            endpoints.push(endpoint);

            // Handle optional trailing comma
            if endpoints_content.peek(Token![,]) {
                let _ = endpoints_content.parse::<Token![,]>()?;
            }
        }

        Ok(ServiceDefinition {
            service_name,
            base_path,
            openapi,
            static_hosting,
            body_limit,
            feature_gated,
            require_json_content_type,
            docs_require_auth,
            endpoints,
        })
    }
}

fn parse_endpoint_path(
    input: syn::parse::ParseStream,
) -> syn::Result<(String, Vec<PathParam>, Vec<String>)> {
    let mut path_segments = Vec::new();
    let mut path_params = Vec::new();
    let mut handler_name_parts = Vec::new();

    let first_segment = input.parse::<Ident>()?;
    path_segments.push(first_segment.to_string());
    handler_name_parts.push(first_segment.to_string());

    while input.peek(Token![/]) {
        let _ = input.parse::<Token![/]>()?;

        if input.peek(syn::token::Brace) {
            let param_content;
            syn::braced!(param_content in input);

            let param_name = param_content.parse::<Ident>()?;
            let _ = param_content.parse::<Token![:]>()?;
            let param_type = param_content.parse::<Type>()?;

            path_segments.push(format!("{{{}}}", param_name));
            path_params.push(PathParam {
                name: param_name.clone(),
                param_type,
            });
            handler_name_parts.push(format!("by_{}", param_name));
        } else {
            let segment = input.parse::<Ident>()?;
            path_segments.push(segment.to_string());
            handler_name_parts.push(segment.to_string());
        }
    }

    Ok((
        format!("/{}", path_segments.join("/")),
        path_params,
        handler_name_parts,
    ))
}

fn parse_query_params(input: syn::parse::ParseStream) -> syn::Result<Vec<QueryParam>> {
    let mut query_params = Vec::new();

    if input.is_empty() {
        return Ok(query_params);
    }

    let param_name = input.parse::<Ident>()?;
    let _ = input.parse::<Token![:]>()?;
    let param_type = input.parse::<Type>()?;
    query_params.push(QueryParam {
        name: param_name,
        param_type,
    });

    while input.peek(Token![&]) || input.peek(Token![,]) {
        if input.peek(Token![&]) {
            let _ = input.parse::<Token![&]>()?;
        } else {
            let _ = input.parse::<Token![,]>()?;
        }

        if input.is_empty() {
            break;
        }

        let param_name = input.parse::<Ident>()?;
        let _ = input.parse::<Token![:]>()?;
        let param_type = input.parse::<Type>()?;
        query_params.push(QueryParam {
            name: param_name,
            param_type,
        });
    }

    Ok(query_params)
}

impl Parse for EndpointDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let docs = parse_doc_comment_attrs(input.call(syn::Attribute::parse_outer)?, "endpoint")?;

        // Parse HTTP method (GET, POST, PUT, DELETE, PATCH)
        let method_ident = input.parse::<Ident>()?;
        let method = match method_ident.to_string().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "DELETE" => HttpMethod::Delete,
            "PATCH" => HttpMethod::Patch,
            _ => {
                return Err(syn::Error::new(
                    method_ident.span(),
                    "Expected GET, POST, PUT, DELETE, or PATCH",
                ));
            }
        };

        // Parse auth requirement (UNAUTHORIZED, OPTIONAL_AUTH, or WITH_PERMISSIONS([...]))
        let auth = if input.peek(syn::Ident) {
            let auth_ident = input.parse::<Ident>()?;
            match auth_ident.to_string().as_str() {
                "UNAUTHORIZED" => AuthRequirement::Unauthorized,
                "OPTIONAL_AUTH" => AuthRequirement::OptionalAuth,
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

                    if permission_groups.len() > 1
                        && permission_groups.iter().any(|group| group.is_empty())
                    {
                        return Err(syn::Error::new(
                            auth_ident.span(),
                            "an empty permission group is only valid as the entire requirement \
                             (WITH_PERMISSIONS([]), meaning any authenticated user); mixing an \
                             empty group with non-empty groups would silently grant access to any \
                             authenticated user",
                        ));
                    }

                    AuthRequirement::WithPermissions(permission_groups)
                }
                _ => {
                    return Err(syn::Error::new(
                        auth_ident.span(),
                        "Expected UNAUTHORIZED, OPTIONAL_AUTH, or WITH_PERMISSIONS",
                    ));
                }
            }
        } else {
            return Err(syn::Error::new(
                input.span(),
                "Expected authentication requirement",
            ));
        };

        // Parse path with potential path parameters (e.g., users/{id: String}/posts/{post_id: i32})
        let (path, path_params, handler_name_parts) = parse_endpoint_path(input)?;

        // Parse query parameters if present (? param1:Type & param2:Type)
        let mut query_params = Vec::new();
        if input.peek(Token![?]) {
            let _ = input.parse::<Token![?]>()?;
            query_params = parse_query_params(input)?;
        }

        // Generate handler name based on method and path
        let method_str = method.as_str().to_lowercase();
        let path_str = handler_name_parts.join("_");
        let handler_name = syn::parse_str::<Ident>(&format!("{}_{}", method_str, path_str))?;

        // Parse (RequestType) - optional for GET/DELETE
        let request_type = if input.peek(syn::token::Paren) {
            let request_content;
            syn::parenthesized!(request_content in input);
            if !request_content.is_empty() {
                Some(request_content.parse::<Type>()?)
            } else {
                None
            }
        } else {
            None
        };

        // Parse -> ResponseType
        let _ = input.parse::<Token![->]>()?;
        let response_type = input.parse::<Type>()?;

        let mut version = None;
        let mut versions = Vec::new();
        let mut body_limit = None;
        let mut with_headers = false;

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
                    "versions" => {
                        let versions_content;
                        syn::bracketed!(versions_content in content);

                        while !versions_content.is_empty() {
                            versions.push(versions_content.parse::<EndpointVersionDefinition>()?);

                            if versions_content.peek(Token![,]) {
                                let _ = versions_content.parse::<Token![,]>()?;
                            }
                        }
                    }
                    "body_limit" => {
                        let limit = content.parse::<syn::LitInt>()?;
                        body_limit = Some(limit.base10_parse::<usize>()?);
                    }
                    "headers" => {
                        with_headers = content.parse::<syn::LitBool>()?.value();
                    }
                    _ => {
                        return Err(syn::Error::new(
                            field_name.span(),
                            "Expected version, versions, body_limit, or headers",
                        ));
                    }
                }

                if content.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                }
            }
        }

        Ok(EndpointDefinition {
            docs,
            method,
            auth,
            path,
            path_params,
            query_params,
            request_type,
            response_type,
            handler_name,
            version,
            versions,
            body_limit,
            with_headers,
        })
    }
}

impl Parse for EndpointVersionDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let version = parse_label(input)?;

        let content;
        syn::braced!(content in input);

        let mut path = None;
        let mut path_params = Vec::new();
        let mut query_params = Vec::new();
        let mut request_type = None;
        let mut response_type = None;
        let mut migration_type = None;

        while !content.is_empty() {
            let field_name = content.parse::<Ident>()?;
            let _ = content.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "path" => {
                    let (parsed_path, parsed_path_params, _) = parse_endpoint_path(&content)?;
                    path = Some(parsed_path);
                    path_params = parsed_path_params;
                }
                "query" => {
                    let query_content;
                    syn::bracketed!(query_content in content);
                    query_params = parse_query_params(&query_content)?;
                }
                "body" | "request" => {
                    let parsed_type = content.parse::<Type>()?;
                    if quote!(#parsed_type).to_string() != "()" {
                        request_type = Some(parsed_type);
                    }
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
                        "Expected path, query, body, request, response, or migration",
                    ));
                }
            }

            if content.peek(Token![,]) {
                let _ = content.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            version,
            path: path
                .ok_or_else(|| syn::Error::new(input.span(), "Version entry is missing path"))?,
            path_params,
            query_params,
            request_type,
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
    let service_name = &service_def.service_name;
    let service_trait_name = quote::format_ident!("{}Trait", service_name);
    let builder_name = quote::format_ident!("{}Builder", service_name);
    let base_path = &service_def.base_path;
    let service_name_lower = service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_rest_{}_server", service_name_lower);
    let client_mod = format_ident!("__ras_rest_{}_client", service_name_lower);

    // Generate OpenAPI code if enabled in the macro input
    let (openapi_code, schema_checks) = if let Some(openapi_config) = &service_def.openapi {
        (
            openapi::generate_openapi_code(&service_def, openapi_config),
            openapi::generate_schema_impl_checks(&service_def),
        )
    } else {
        (quote! {}, quote! {})
    };

    // Generate static hosting code if enabled
    let static_hosting_code = if service_def.static_hosting.serve_docs {
        static_hosting::generate_static_hosting_code(&service_def, &service_def.static_hosting)
    } else {
        quote! {}
    };

    // Generate client code
    let client_impl = crate::client::generate_client_code(&service_def);
    let permissions_code = if cfg!(feature = "permissions") {
        permissions::generate_permissions_code(&service_def)
    } else {
        quote! {}
    };

    // Generate trait methods
    let trait_methods = service_def.endpoints.iter().map(|endpoint| {
        let handler_name = &endpoint.handler_name;
        let response_type = &endpoint.response_type;

        // Build parameter list based on auth requirements and path params
        let mut params = Vec::new();
        // Add authenticated user parameter if needed
        match &endpoint.auth {
            AuthRequirement::Unauthorized => {}
            AuthRequirement::OptionalAuth => {
                params.push(quote! { caller: ras_auth_core::Caller });
            }
            AuthRequirement::WithPermissions(_) => {
                params.push(quote! { user: &ras_auth_core::AuthenticatedUser });
            }
        }

        // Opt-in request headers (immediately after the caller/user)
        if endpoint.with_headers {
            params.push(quote! { headers: axum::http::HeaderMap });
        }

        // Add path parameters
        for path_param in &endpoint.path_params {
            let param_name = &path_param.name;
            let param_type = &path_param.param_type;
            params.push(quote! { #param_name: #param_type });
        }

        // Add query parameters
        for query_param in &endpoint.query_params {
            let param_name = &query_param.name;
            let param_type = &query_param.param_type;
            params.push(quote! { #param_name: #param_type });
        }

        // Add request body parameter if present
        if let Some(request_type) = &endpoint.request_type {
            params.push(quote! { request: #request_type });
        }

        quote! {
            async fn #handler_name(&self, #(#params),*) -> ras_rest_core::RestResult<#response_type>;
        }
    });

    let request_part_structs = generate_rest_request_part_structs(&service_def);

    let mut query_structs: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut route_registrations: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut route_idx = 0usize;

    for endpoint in &service_def.endpoints {
        let query_struct_name = quote::format_ident!("QueryParams{}", route_idx);
        query_structs.push(generate_query_struct(
            &query_struct_name,
            &endpoint.query_params,
        ));
        route_registrations.push(generate_canonical_route_registration(
            endpoint,
            &query_struct_name,
            service_def.require_json_content_type,
        ));
        route_idx += 1;

        for version in &endpoint.versions {
            let query_struct_name = quote::format_ident!("QueryParams{}", route_idx);
            query_structs.push(generate_query_struct(
                &query_struct_name,
                &version.query_params,
            ));
            route_registrations.push(generate_legacy_route_registration(
                &service_def.service_name,
                endpoint,
                version,
                &query_struct_name,
                service_def.require_json_content_type,
            ));
            route_idx += 1;
        }
    }

    // Generate static hosting route registration - only if docs are enabled
    let static_routes = if service_def.static_hosting.serve_docs {
        static_hosting::generate_static_routes(&service_def, &service_def.static_hosting)
    } else {
        quote! {}
    };

    let body_limit = service_def.body_limit.unwrap_or(DEFAULT_BODY_LIMIT);

    // Startup assertion: a service with any WITH_PERMISSIONS route needs an auth
    // provider, otherwise every such route returns a runtime 500 (NoAuthProvider)
    // on first request. Catch the misconfiguration at build() instead.
    let any_route_requires_auth = service_def
        .endpoints
        .iter()
        .any(|endpoint| matches!(endpoint.auth, AuthRequirement::WithPermissions(_)))
        || (service_def.static_hosting.serve_docs && service_def.docs_require_auth);
    let service_name_str = service_name.to_string();
    let provider_assertion = if any_route_requires_auth {
        quote! {
            if self.auth_provider.is_none() {
                panic!(concat!(
                    "REST service `",
                    #service_name_str,
                    "` has endpoints requiring authorization (WITH_PERMISSIONS) but no ",
                    "auth_provider was configured; call .auth_provider(...) before build()"
                ));
            }
        }
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
        quote! {
        #cfg_server
        mod #server_mod {
            use super::*;

        /// Maximum accepted JSON body size in bytes
        #[allow(dead_code)]
        const __RAS_BODY_LIMIT: usize = #body_limit;

        /// Map a shared authorization failure to this service's JSON error shape
        #[allow(dead_code)]
        fn __ras_authorize_error_response(error: ras_auth_core::AuthorizeError) -> axum::response::Response {
            use axum::response::IntoResponse;
            let (status, message) = match &error {
                ras_auth_core::AuthorizeError::MissingCredential => (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Missing or invalid Authorization header",
                ),
                ras_auth_core::AuthorizeError::CsrfValidationFailed => (
                    axum::http::StatusCode::FORBIDDEN,
                    "CSRF validation failed",
                ),
                ras_auth_core::AuthorizeError::AuthenticationFailed(_) => (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Authentication failed",
                ),
                ras_auth_core::AuthorizeError::NoAuthProvider => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "No auth provider configured",
                ),
                ras_auth_core::AuthorizeError::InsufficientPermissions(_) => (
                    axum::http::StatusCode::FORBIDDEN,
                    "Insufficient permissions",
                ),
            };
            // Rejections are otherwise invisible to the usage/duration trackers,
            // which only run on the post-auth happy path; log them here so a
            // client hammering an endpoint with a bad credential is observable.
            // The `error` detail is logged server-side only; the client gets the
            // generic `message`.
            ras_rest_core::tracing::warn!(
                status = status.as_u16(),
                error = ?error,
                "request rejected during authorization"
            );
            (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
        }

        /// Build a success response, omitting the JSON body for status codes that
        /// must not carry one (`204 No Content`, `205 Reset Content`,
        /// `304 Not Modified`). Without this, `RestResponse::no_content()` would
        /// emit a `204` with a serialized `null` body and
        /// `Content-Type: application/json`, which violates RFC 9110 and is
        /// rejected by some proxies and clients.
        #[allow(dead_code)]
        fn __ras_success_response<T: serde::Serialize>(
            status: axum::http::StatusCode,
            body: T,
        ) -> axum::response::Response {
            use axum::response::IntoResponse;
            if status == axum::http::StatusCode::NO_CONTENT
                || status == axum::http::StatusCode::RESET_CONTENT
                || status == axum::http::StatusCode::NOT_MODIFIED
            {
                status.into_response()
            } else {
                (status, axum::Json(body)).into_response()
            }
        }

        /// Generated service trait
        #[async_trait::async_trait]
        #[allow(private_interfaces, private_bounds)]
        pub trait #service_trait_name: Send + Sync + 'static {
            #(#trait_methods)*
        }

        /// Generated builder for the REST service
        pub struct #builder_name<T: #service_trait_name> {
            service: std::sync::Arc<T>,
            auth_provider: Option<std::sync::Arc<dyn ras_auth_core::AuthProvider>>,
            auth_transport: ras_auth_core::AuthTransportConfig,
            with_usage_tracker: Option<std::sync::Arc<dyn Fn(&axum::http::HeaderMap, Option<&ras_auth_core::AuthenticatedUser>, &str, &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
            with_method_duration_tracker: Option<std::sync::Arc<dyn Fn(&str, &str, Option<&ras_auth_core::AuthenticatedUser>, std::time::Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
        }

        const _: () = {
            #schema_checks
        };

        // Generate OpenAPI function at module level if serve_docs is enabled
        #openapi_code

        #static_hosting_code

        // Define query parameter structs
        use self::query_params::*;

        mod query_params {
            #[allow(unused_imports)]
            use super::*;

            #(#query_structs)*
        }

        #request_part_structs

        impl<T: #service_trait_name> #builder_name<T> {
            /// Create a new builder with the service implementation
            pub fn new(service: T) -> Self {
                Self {
                    service: std::sync::Arc::new(service),
                    auth_provider: None,
                    auth_transport: ras_auth_core::AuthTransportConfig::default(),
                    with_usage_tracker: None,
                    with_method_duration_tracker: None,
                }
            }

            /// Set the auth provider
            pub fn auth_provider<A: ras_auth_core::AuthProvider>(mut self, provider: A) -> Self {
                self.auth_provider = Some(std::sync::Arc::new(provider));
                self
            }

            /// Enable cookie authentication alongside bearer tokens.
            ///
            /// Installs a default double-submit CSRF config when none is set,
            /// because cookie credentials are CSRF-exploitable on unsafe methods.
            /// Override with `csrf_protection`.
            pub fn auth_cookie(mut self, cookie: ras_auth_core::AuthCookieConfig) -> Self {
                self.auth_transport.cookie = Some(cookie);
                if self.auth_transport.csrf.is_none() {
                    self.auth_transport.csrf = Some(ras_auth_core::CsrfConfig::default());
                }
                self
            }

            /// Replace the full auth transport configuration.
            pub fn auth_transport(mut self, transport: ras_auth_core::AuthTransportConfig) -> Self {
                self.auth_transport = transport;
                self
            }

            /// Require CSRF validation for cookie-authenticated unsafe requests.
            pub fn csrf_protection(mut self, csrf: ras_auth_core::CsrfConfig) -> Self {
                self.auth_transport.csrf = Some(csrf);
                self
            }

            /// Set the usage tracker - called before each request
            /// The tracker receives the headers, authenticated user (if any), HTTP method, and path
            pub fn with_usage_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&axum::http::HeaderMap, Option<&ras_auth_core::AuthenticatedUser>, &str, &str) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.with_usage_tracker = Some(std::sync::Arc::new(move |headers, user, method, path| {
                    Box::pin(tracker(headers, user, method, path))
                }));
                self
            }

            /// Set the method duration tracker - called after each request completes
            /// The tracker receives the HTTP method, path, authenticated user (if any), and execution duration
            pub fn with_method_duration_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&str, &str, Option<&ras_auth_core::AuthenticatedUser>, std::time::Duration) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.with_method_duration_tracker = Some(std::sync::Arc::new(move |method, path, user, duration| {
                    Box::pin(tracker(method, path, user, duration))
                }));
                self
            }

            /// Build the axum router for the REST service
            pub fn build(self) -> axum::Router {
                self.auth_transport
                    .validate()
                    .expect("invalid auth transport configuration");

                #provider_assertion

                let mut router = axum::Router::new();

                #(#route_registrations)*

                // Add static hosting routes if enabled
                #static_routes

                // Handle empty or root base path
                if #base_path.is_empty() || #base_path == "/" {
                    router
                } else {
                    axum::Router::new().nest(#base_path, router)
                }
            }
        }

        }

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

fn rest_permission_groups_code(auth: &AuthRequirement) -> proc_macro2::TokenStream {
    let permission_groups = match auth {
        AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => Vec::new(),
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

fn pascal_ident_segment(value: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = true;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                out.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                out.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }

    if out.is_empty() {
        "Version".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("V{out}")
    } else {
        out
    }
}

fn rest_request_part_idents(
    service_name: &Ident,
    handler_name: &Ident,
    version: &str,
) -> (Ident, Ident, Ident) {
    let service = service_name.to_string();
    let handler = pascal_ident_segment(&handler_name.to_string());
    let version = pascal_ident_segment(version);
    let request_ident = quote::format_ident!("{}{}{}Request", service, handler, version);
    let path_ident = quote::format_ident!("{}{}{}Path", service, handler, version);
    let query_ident = quote::format_ident!("{}{}{}Query", service, handler, version);
    (request_ident, path_ident, query_ident)
}

fn rest_body_type_tokens(request_type: Option<&Type>) -> proc_macro2::TokenStream {
    match request_type {
        Some(request_type) => quote! { #request_type },
        None => quote! { () },
    }
}

fn generate_rest_request_part_structs(service_def: &ServiceDefinition) -> proc_macro2::TokenStream {
    let structs = service_def.endpoints.iter().flat_map(|endpoint| {
        if endpoint.versions.is_empty() {
            return Vec::new();
        }

        let canonical_version = endpoint.version.as_deref().unwrap_or("current");
        let mut structs = vec![generate_rest_request_part_struct(
            &service_def.service_name,
            &endpoint.handler_name,
            canonical_version,
            &endpoint.path_params,
            &endpoint.query_params,
            endpoint.request_type.as_ref(),
        )];

        structs.extend(endpoint.versions.iter().map(|version| {
            generate_rest_request_part_struct(
                &service_def.service_name,
                &endpoint.handler_name,
                &version.version,
                &version.path_params,
                &version.query_params,
                version.request_type.as_ref(),
            )
        }));

        structs
    });

    quote! {
        #(#structs)*
    }
}

fn generate_rest_request_part_struct(
    service_name: &Ident,
    handler_name: &Ident,
    version: &str,
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
) -> proc_macro2::TokenStream {
    let (request_ident, path_ident, query_ident) =
        rest_request_part_idents(service_name, handler_name, version);
    let path_fields = path_params.iter().map(|param| {
        let name = &param.name;
        let param_type = &param.param_type;
        quote! { pub #name: #param_type }
    });
    let query_fields = query_params.iter().map(|param| {
        let name = &param.name;
        let param_type = &param.param_type;
        quote! { pub #name: #param_type }
    });
    let body_type = rest_body_type_tokens(request_type);

    quote! {
        pub struct #path_ident {
            #(#path_fields),*
        }

        pub struct #query_ident {
            #(#query_fields),*
        }

        pub struct #request_ident {
            pub path: #path_ident,
            pub query: #query_ident,
            pub body: #body_type,
        }
    }
}

fn generate_rest_parts_init(
    service_name: &Ident,
    handler_name: &Ident,
    version: &str,
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
) -> proc_macro2::TokenStream {
    let (request_ident, path_ident, query_ident) =
        rest_request_part_idents(service_name, handler_name, version);

    let path_values = path_params.iter().enumerate().map(|(idx, param)| {
        let name = &param.name;
        if path_params.len() == 1 {
            quote! { #name: path_params }
        } else {
            let idx = syn::Index::from(idx);
            quote! { #name: path_params.#idx }
        }
    });

    let query_values = query_params.iter().map(|param| {
        let name = &param.name;
        quote! { #name: query_params.#name }
    });

    let body_value = if request_type.is_some() {
        quote! { body }
    } else {
        quote! { () }
    };

    quote! {
        #request_ident {
            path: #path_ident {
                #(#path_values),*
            },
            query: #query_ident {
                #(#query_values),*
            },
            body: #body_value,
        }
    }
}

fn rest_canonical_args_from_parts(
    endpoint: &EndpointDefinition,
    parts_ident: &Ident,
) -> Vec<proc_macro2::TokenStream> {
    let mut args = Vec::new();

    for path_param in &endpoint.path_params {
        let name = &path_param.name;
        args.push(quote! { #parts_ident.path.#name });
    }

    for query_param in &endpoint.query_params {
        let name = &query_param.name;
        args.push(quote! { #parts_ident.query.#name });
    }

    if endpoint.request_type.is_some() {
        args.push(quote! { #parts_ident.body });
    }

    args
}

fn generate_query_struct(
    struct_name: &Ident,
    query_params: &[QueryParam],
) -> proc_macro2::TokenStream {
    if query_params.is_empty() {
        return quote! {};
    }

    let fields = query_params.iter().map(|param| {
        let name = &param.name;
        let param_type = &param.param_type;
        quote! { pub #name: #param_type }
    });

    quote! {
        #[derive(serde::Deserialize)]
        pub(super) struct #struct_name {
            #(#fields),*
        }
    }
}

fn generate_canonical_route_registration(
    endpoint: &EndpointDefinition,
    query_struct_name: &Ident,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let method_routing = endpoint.method.as_axum_method();
    let path = &endpoint.path;
    let handler_name = &endpoint.handler_name;
    let method_str = endpoint.method.as_str();
    let AxumHandlerParts {
        extractors: axum_handler,
        prelude: extractor_prelude,
    } = generate_axum_handler(
        &endpoint.path_params,
        &endpoint.query_params,
        endpoint.request_type.as_ref(),
        query_struct_name,
        method_str,
        path,
    );
    let handler_body =
        generate_handler_body(endpoint, handler_name, method_str, path, require_json);
    let permission_groups_code = rest_permission_groups_code(&endpoint.auth);

    quote! {
        {
            let service = self.service.clone();
            let auth_provider = self.auth_provider.clone();
            let auth_transport = self.auth_transport.clone();
            let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
            let with_usage_tracker = self.with_usage_tracker.clone();
            let with_method_duration_tracker = self.with_method_duration_tracker.clone();

            router = router.route(#path, #method_routing({
                move |#axum_handler| {
                    let service = service.clone();
                    let auth_provider = auth_provider.clone();
                    let auth_transport = auth_transport.clone();
                    let required_permission_groups: Vec<Vec<String>> = required_permission_groups.clone();
                    let with_usage_tracker = with_usage_tracker.clone();
                    let with_method_duration_tracker = with_method_duration_tracker.clone();

                    async move {
                        #extractor_prelude
                        #handler_body
                    }
                }
            }));
        }
    }
}

fn generate_legacy_route_registration(
    service_name: &Ident,
    endpoint: &EndpointDefinition,
    version: &EndpointVersionDefinition,
    query_struct_name: &Ident,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let method_routing = endpoint.method.as_axum_method();
    let path = &version.path;
    let AxumHandlerParts {
        extractors: axum_handler,
        prelude: extractor_prelude,
    } = generate_axum_handler(
        &version.path_params,
        &version.query_params,
        version.request_type.as_ref(),
        query_struct_name,
        endpoint.method.as_str(),
        path,
    );
    let handler_body = generate_legacy_handler_body(service_name, endpoint, version, require_json);
    let permission_groups_code = rest_permission_groups_code(&endpoint.auth);

    quote! {
        {
            let service = self.service.clone();
            let auth_provider = self.auth_provider.clone();
            let auth_transport = self.auth_transport.clone();
            let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
            let with_usage_tracker = self.with_usage_tracker.clone();
            let with_method_duration_tracker = self.with_method_duration_tracker.clone();

            router = router.route(#path, #method_routing({
                move |#axum_handler| {
                    let service = service.clone();
                    let auth_provider = auth_provider.clone();
                    let auth_transport = auth_transport.clone();
                    let required_permission_groups: Vec<Vec<String>> = required_permission_groups.clone();
                    let with_usage_tracker = with_usage_tracker.clone();
                    let with_method_duration_tracker = with_method_duration_tracker.clone();

                    async move {
                        #extractor_prelude
                        #handler_body
                    }
                }
            }));
        }
    }
}

fn generate_legacy_handler_body(
    service_name: &Ident,
    endpoint: &EndpointDefinition,
    version: &EndpointVersionDefinition,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let handler_name = &endpoint.handler_name;
    let method = endpoint.method.as_str();
    let path = &version.path;
    let body_limit_tokens = effective_body_limit_tokens(endpoint);
    let migration_type = &version.migration_type;
    let canonical_response_type = &endpoint.response_type;
    let legacy_response_type = &version.response_type;
    let canonical_version = endpoint.version.as_deref().unwrap_or("current");
    let (canonical_request_ident, _, _) =
        rest_request_part_idents(service_name, handler_name, canonical_version);
    let (legacy_request_ident, _, _) =
        rest_request_part_idents(service_name, handler_name, &version.version);
    let legacy_parts_init = generate_rest_parts_init(
        service_name,
        handler_name,
        &version.version,
        &version.path_params,
        &version.query_params,
        version.request_type.as_ref(),
    );
    let canonical_parts_ident = quote::format_ident!("canonical_parts");
    let mut canonical_args = rest_canonical_args_from_parts(endpoint, &canonical_parts_ident);

    // Opt-in request headers, inserted before the auth arg is prepended below so
    // the final order is [caller/user?, headers, path.., query.., body?].
    if endpoint.with_headers {
        canonical_args.insert(0, quote! { headers.clone() });
    }

    let json_handling = if version.request_type.is_some() {
        generate_body_extraction(require_json, &body_limit_tokens, method, path)
    } else {
        quote! {}
    };

    match &endpoint.auth {
        AuthRequirement::Unauthorized => quote! {
            #json_handling

            if let Some(tracker) = &with_usage_tracker {
                let tracker_headers =
                    ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                tracker(&tracker_headers, None, #method, #path).await;
            }

            let legacy_parts: #legacy_request_ident = #legacy_parts_init;
            let #canonical_parts_ident: #canonical_request_ident =
                match <#migration_type as ras_rest_core::VersionMigration<#legacy_request_ident, #canonical_request_ident>>::migrate(legacy_parts) {
                    Ok(parts) => parts,
                    Err(e) => {
                        use axum::response::IntoResponse;
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({
                                "error": e.to_string()
                            }))
                        ).into_response();
                    },
                };

            let start_time = std::time::Instant::now();

            let result = match service.#handler_name(#(#canonical_args),*).await {
                Ok(rest_response) => {
                    use axum::response::IntoResponse;
                    let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                        .unwrap_or(axum::http::StatusCode::OK);
                    let body: #legacy_response_type =
                        match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                            Ok(body) => body,
                            Err(e) => {
                                ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                                return (
                                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    axum::Json(serde_json::json!({
                                        "error": "Internal server error"
                                    }))
                                ).into_response();
                            },
                        };
                    __ras_success_response(status_code, body)
                },
                Err(rest_error) => {
                    use axum::response::IntoResponse;

                    if let Some(internal) = &rest_error.internal_error {
                        ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                    }

                    let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                    (
                        status_code,
                        axum::Json(serde_json::json!({
                            "error": &rest_error.message
                        }))
                    ).into_response()
                },
            };

            let duration = start_time.elapsed();
            if let Some(tracker) = &with_method_duration_tracker {
                tracker(#method, #path, None, duration).await;
            }

            result
        },
        AuthRequirement::OptionalAuth => {
            canonical_args.insert(0, quote! { caller });

            quote! {
                // Best-effort authentication for an OPTIONAL_AUTH route — never
                // rejected: resolves to Caller::Anonymous for a missing/invalid
                // credential, Caller::Authenticated for a valid one.
                let caller = ras_auth_core::resolve_caller(
                    #method,
                    &headers,
                    &auth_transport,
                    auth_provider.as_deref(),
                ).await;
                // Snapshot the user for tracking; `caller` is moved into the handler.
                let __ras_caller_user = caller.authenticated().cloned();

                #json_handling

                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, __ras_caller_user.as_ref(), #method, #path).await;
                }

                let legacy_parts: #legacy_request_ident = #legacy_parts_init;
                let #canonical_parts_ident: #canonical_request_ident =
                    match <#migration_type as ras_rest_core::VersionMigration<#legacy_request_ident, #canonical_request_ident>>::migrate(legacy_parts) {
                        Ok(parts) => parts,
                        Err(e) => {
                            use axum::response::IntoResponse;
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                axum::Json(serde_json::json!({
                                    "error": e.to_string()
                                }))
                            ).into_response();
                        },
                    };

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#canonical_args),*).await {
                    Ok(rest_response) => {
                        use axum::response::IntoResponse;
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        let body: #legacy_response_type =
                            match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                                Ok(body) => body,
                                Err(e) => {
                                    ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                                    return (
                                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                        axum::Json(serde_json::json!({
                                            "error": "Internal server error"
                                        }))
                                    ).into_response();
                                },
                            };
                        __ras_success_response(status_code, body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, __ras_caller_user.as_ref(), duration).await;
                }

                result
            }
        }
        AuthRequirement::WithPermissions(_) => {
            canonical_args.insert(0, quote! { &user });

            quote! {
                // Authenticate and authorize: credential → CSRF → authenticate
                // → OR-of-AND permission groups (shared ras-auth-core pipeline)
                let user = match ras_auth_core::authorize_request(
                    #method,
                    &headers,
                    &auth_transport,
                    auth_provider.as_deref(),
                    &required_permission_groups,
                ).await {
                    Ok(user) => user,
                    Err(error) => return __ras_authorize_error_response(error),
                };

                // Read and parse the body only after auth has succeeded
                #json_handling

                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, Some(&user), #method, #path).await;
                }

                let legacy_parts: #legacy_request_ident = #legacy_parts_init;
                let #canonical_parts_ident: #canonical_request_ident =
                    match <#migration_type as ras_rest_core::VersionMigration<#legacy_request_ident, #canonical_request_ident>>::migrate(legacy_parts) {
                        Ok(parts) => parts,
                        Err(e) => {
                            use axum::response::IntoResponse;
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                axum::Json(serde_json::json!({
                                    "error": e.to_string()
                                }))
                            ).into_response();
                        },
                    };

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#canonical_args),*).await {
                    Ok(rest_response) => {
                        use axum::response::IntoResponse;
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        let body: #legacy_response_type =
                            match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                                Ok(body) => body,
                                Err(e) => {
                                    ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                                    return (
                                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                        axum::Json(serde_json::json!({
                                            "error": "Internal server error"
                                        }))
                                    ).into_response();
                                },
                            };
                        __ras_success_response(status_code, body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, Some(&user), duration).await;
                }

                result
            }
        }
    }
}

/// Generated handler signature plus the prelude that unwraps fallible
/// extractors inside the handler body.
struct AxumHandlerParts {
    /// Closure parameter list (`headers`, path/query extractors, raw request).
    extractors: proc_macro2::TokenStream,
    /// Emitted at the top of the handler body. Path and query extractors are
    /// taken as `Result<_, Rejection>` so the axum default rejection body —
    /// which echoes the offending value verbatim, e.g.
    /// ``Invalid URL: Cannot parse `abc` to a `i32` `` — is never sent to the
    /// client. The detail is logged at `warn` and a fixed message is returned.
    prelude: proc_macro2::TokenStream,
}

fn generate_axum_handler(
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
    query_struct_name: &Ident,
    method: &str,
    path: &str,
) -> AxumHandlerParts {
    let mut extractors = Vec::new();
    let mut prelude = Vec::new();

    // Always add headers extraction for tracking purposes
    extractors.push(quote! { headers: axum::http::HeaderMap });

    // Add path parameter extractors
    if !path_params.is_empty() {
        let path_param_types = path_params.iter().map(|param| &param.param_type);
        let path_ty = if path_params.len() == 1 {
            quote! { axum::extract::Path<#(#path_param_types)*> }
        } else {
            quote! { axum::extract::Path<(#(#path_param_types),*)> }
        };
        extractors.push(quote! {
            __ras_path: Result<#path_ty, axum::extract::rejection::PathRejection>
        });
        prelude.push(quote! {
            let axum::extract::Path(path_params) = match __ras_path {
                Ok(path) => path,
                Err(__ras_rejection) => {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        status = __ras_rejection.status().as_u16(),
                        detail = %__ras_rejection.body_text(),
                        "rejected request: invalid path parameters"
                    );
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": "Invalid path parameters" }))
                    ).into_response();
                }
            };
        });
    }

    // Add query parameter extractors
    if !query_params.is_empty() {
        extractors.push(quote! {
            __ras_query: Result<
                ::axum_extra::extract::Query<query_params::#query_struct_name>,
                ::axum_extra::extract::QueryRejection,
            >
        });
        prelude.push(quote! {
            let ::axum_extra::extract::Query(query_params) = match __ras_query {
                Ok(query) => query,
                Err(__ras_rejection) => {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        status = __ras_rejection.status().as_u16(),
                        detail = %__ras_rejection.body_text(),
                        "rejected request: invalid query parameters"
                    );
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": "Invalid query parameters" }))
                    ).into_response();
                }
            };
        });
    }

    // Take the raw request when a body is declared. The body is read and
    // deserialized inside the handler AFTER auth/CSRF/permission checks, so
    // unauthenticated clients cannot make the server buffer or parse payloads.
    if request_type.is_some() {
        extractors.push(quote! { request: axum::extract::Request });
    }

    AxumHandlerParts {
        extractors: quote! { #(#extractors),* },
        prelude: quote! { #(#prelude)* },
    }
}

/// Generated code that reads and JSON-deserializes the request body from the
/// raw `request` extractor, bounded by `limit`.
///
/// For authenticated endpoints this must be emitted AFTER the
/// auth/CSRF/permission block so unauthenticated clients cannot make the
/// server buffer or parse payloads.
///
/// Behavior:
/// * When `require_json` is set, a request whose `Content-Type` is not
///   `application/json` (ignoring parameters like `; charset=utf-8`) is rejected
///   with `415 Unsupported Media Type` before the body is read. Requiring
///   `application/json` also forces a CORS preflight for cross-origin requests,
///   which no CORS layer answers by default — defense-in-depth against
///   simple-request CSRF on cookie-authenticated endpoints.
/// * A declared `Content-Length` over `limit` is rejected with `413` up front so
///   a subsequent `to_bytes` error is unambiguously a read failure (`400`),
///   rather than the two being conflated as "too large".
/// * A malformed JSON body is logged (category + line/column, never the rejected
///   value) at `warn` before returning `400`, matching the handler-error logging
///   convention.
fn generate_body_extraction(
    require_json: bool,
    limit: &proc_macro2::TokenStream,
    method: &str,
    path: &str,
) -> proc_macro2::TokenStream {
    let content_type_check = if require_json {
        quote! {
            {
                let __ras_content_type_ok = headers
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| {
                        value
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .eq_ignore_ascii_case("application/json")
                    })
                    .unwrap_or(false);
                if !__ras_content_type_ok {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        "rejected request: Content-Type is not application/json"
                    );
                    return (
                        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        axum::Json(serde_json::json!({
                            "error": "Unsupported Media Type: expected application/json"
                        }))
                    ).into_response();
                }
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #content_type_check

        let body = {
            // Reject an over-declared Content-Length up front so a 413 is
            // unambiguous without reading the body. A chunked body with no
            // declared length is still capped by `to_bytes`; that error is then
            // classified below (over-limit -> 413, genuine read error -> 400).
            if let Some(__ras_declared_len) = headers
                .get(axum::http::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
            {
                if __ras_declared_len > #limit {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        declared_len = __ras_declared_len,
                        limit = #limit,
                        "rejected request: body exceeds limit"
                    );
                    return (
                        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                        axum::Json(serde_json::json!({
                            "error": "Request body too large"
                        }))
                    ).into_response();
                }
            }

            let body_bytes = match ::axum::body::to_bytes(request.into_body(), #limit).await {
                Ok(bytes) => bytes,
                Err(__ras_body_err) => {
                    use axum::response::IntoResponse;
                    // `to_bytes` fails for both an over-limit body and a genuine
                    // stream read error. axum wraps http_body_util's
                    // `LengthLimitError` (Display: "length limit exceeded") for
                    // the former; classify on it so a read failure is a 400 and
                    // only a real overflow is a 413 — the two are no longer
                    // conflated. (A body carrying a declared `Content-Length` over
                    // the limit is already rejected above without being read.)
                    let (__ras_status, __ras_client_msg) =
                        if __ras_body_err.to_string().contains("length limit exceeded") {
                            (
                                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                                "Request body too large",
                            )
                        } else {
                            (
                                axum::http::StatusCode::BAD_REQUEST,
                                "Could not read request body",
                            )
                        };
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        status = __ras_status.as_u16(),
                        "rejected request: {}",
                        __ras_client_msg
                    );
                    return (
                        __ras_status,
                        axum::Json(serde_json::json!({ "error": __ras_client_msg }))
                    ).into_response();
                },
            };
            match serde_json::from_slice(&body_bytes) {
                Ok(body) => body,
                Err(__ras_json_err) => {
                    use axum::response::IntoResponse;
                    // Log the classification and location so the reason is
                    // recoverable server-side; never log the rejected value.
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        category = ?__ras_json_err.classify(),
                        line = __ras_json_err.line(),
                        column = __ras_json_err.column(),
                        "rejected request: malformed JSON body"
                    );
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({
                            "error": "Invalid JSON"
                        }))
                    ).into_response();
                },
            }
        };
    }
}

/// The effective body-size limit expression for an endpoint: its per-endpoint
/// `body_limit` override when set, otherwise the service-level `__RAS_BODY_LIMIT`.
fn effective_body_limit_tokens(endpoint: &EndpointDefinition) -> proc_macro2::TokenStream {
    match endpoint.body_limit {
        Some(limit) => quote! { #limit },
        None => quote! { __RAS_BODY_LIMIT },
    }
}

fn generate_handler_body(
    endpoint: &EndpointDefinition,
    handler_name: &Ident,
    method: &str,
    path: &str,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let body_limit_tokens = effective_body_limit_tokens(endpoint);
    // Handle authentication if required
    match &endpoint.auth {
        AuthRequirement::Unauthorized => {
            // Build argument list for unauthorized endpoint
            let mut args = Vec::new();

            // Opt-in request headers (before path params)
            if endpoint.with_headers {
                args.push(quote! { headers.clone() });
            }

            // Add path parameters
            if endpoint.path_params.len() == 1 {
                args.push(quote! { path_params });
            } else {
                for (i, _) in endpoint.path_params.iter().enumerate() {
                    let idx = syn::Index::from(i);
                    args.push(quote! { path_params.#idx });
                }
            }

            // Add query parameters
            for query_param in &endpoint.query_params {
                let param_name = &query_param.name;
                args.push(quote! { query_params.#param_name });
            }

            // Handle JSON body extraction with error handling
            let json_handling = if endpoint.request_type.is_some() {
                args.push(quote! { body });
                generate_body_extraction(require_json, &body_limit_tokens, method, path)
            } else {
                quote! {}
            };

            quote! {
                #json_handling

                // Call usage tracker if configured (for unauthorized endpoints, headers come from handler params)
                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, None, #method, #path).await;
                }

                // Track duration
                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#args),*).await {
                    Ok(rest_response) => {
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        __ras_success_response(status_code, rest_response.body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        // Log internal error if present
                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                // Call duration tracker if configured
                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, None, duration).await;
                }

                result
            }
        }
        AuthRequirement::OptionalAuth => {
            // Build argument list; the caller is passed by value as the first arg.
            let mut args = vec![quote! { caller }];

            // Opt-in request headers (after the caller, before path params)
            if endpoint.with_headers {
                args.push(quote! { headers.clone() });
            }

            // Add path parameters
            if endpoint.path_params.len() == 1 {
                args.push(quote! { path_params });
            } else {
                for (i, _) in endpoint.path_params.iter().enumerate() {
                    let idx = syn::Index::from(i);
                    args.push(quote! { path_params.#idx });
                }
            }

            // Add query parameters
            for query_param in &endpoint.query_params {
                let param_name = &query_param.name;
                args.push(quote! { query_params.#param_name });
            }

            // Handle JSON body extraction with error handling
            let json_handling = if endpoint.request_type.is_some() {
                args.push(quote! { body });
                generate_body_extraction(require_json, &body_limit_tokens, method, path)
            } else {
                quote! {}
            };

            quote! {
                // Best-effort authentication for an OPTIONAL_AUTH route: never
                // rejected — Caller::Anonymous when no/invalid credential is
                // present, Caller::Authenticated otherwise.
                let caller = ras_auth_core::resolve_caller(
                    #method,
                    &headers,
                    &auth_transport,
                    auth_provider.as_deref(),
                ).await;
                // Snapshot the user for tracking; `caller` is moved into the handler.
                let __ras_caller_user = caller.authenticated().cloned();

                #json_handling

                // Call usage tracker if configured
                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, __ras_caller_user.as_ref(), #method, #path).await;
                }

                // Track duration
                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#args),*).await {
                    Ok(rest_response) => {
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        __ras_success_response(status_code, rest_response.body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                // Call duration tracker if configured
                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, __ras_caller_user.as_ref(), duration).await;
                }

                result
            }
        }
        AuthRequirement::WithPermissions(_) => {
            // Build argument list for authenticated endpoint
            let mut args = vec![quote! { &user }];

            // Opt-in request headers (after the user, before path params)
            if endpoint.with_headers {
                args.push(quote! { headers.clone() });
            }

            // Add path parameters
            if endpoint.path_params.len() == 1 {
                args.push(quote! { path_params });
            } else {
                for (i, _) in endpoint.path_params.iter().enumerate() {
                    let idx = syn::Index::from(i);
                    args.push(quote! { path_params.#idx });
                }
            }

            // Add query parameters
            for query_param in &endpoint.query_params {
                let param_name = &query_param.name;
                args.push(quote! { query_params.#param_name });
            }

            // Handle JSON body extraction with error handling
            let json_handling = if endpoint.request_type.is_some() {
                args.push(quote! { body });
                generate_body_extraction(require_json, &body_limit_tokens, method, path)
            } else {
                quote! {}
            };

            quote! {
                // Authenticate and authorize: credential → CSRF → authenticate
                // → OR-of-AND permission groups (shared ras-auth-core pipeline)
                let user = match ras_auth_core::authorize_request(
                    #method,
                    &headers,
                    &auth_transport,
                    auth_provider.as_deref(),
                    &required_permission_groups,
                ).await {
                    Ok(user) => user,
                    Err(error) => return __ras_authorize_error_response(error),
                };

                // Read and parse the body only after auth has succeeded
                #json_handling

                // Call usage tracker if configured
                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, Some(&user), #method, #path).await;
                }

                // Track duration
                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#args),*).await {
                    Ok(rest_response) => {
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        __ras_success_response(status_code, rest_response.body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        // Log internal error if present
                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                // Call duration tracker if configured
                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, Some(&user), duration).await;
                }

                result
            }
        }
    }
}
