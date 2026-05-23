use syn::{
    Error, Ident, LitStr, Result, Token, Type, braced,
    parse::{Parse, ParseStream},
    token,
};

#[derive(Debug)]
pub struct FileServiceDefinition {
    pub service_name: Ident,
    pub base_path: LitStr,
    pub body_limit: Option<u64>,
    pub openapi: Option<OpenApiConfig>,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug)]
pub enum OpenApiConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
pub struct Endpoint {
    pub operation: Operation,
    pub auth: AuthRequirement,
    pub name: Ident,
    pub path: Option<LitStr>,
    pub path_params: Vec<PathParam>,
    pub response_type: Option<Type>,
}

#[derive(Debug)]
pub enum Operation {
    Upload,
    Download,
}

#[derive(Debug)]
pub enum AuthRequirement {
    Unauthorized,
    WithPermissions(Vec<Vec<String>>),
}

#[derive(Debug)]
pub struct PathParam {
    pub name: Ident,
    pub ty: Type,
}

impl Parse for FileServiceDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        braced!(content in input);

        let mut service_name = None;
        let mut base_path = None;
        let mut body_limit = None;
        let mut openapi = None;
        let mut endpoints = Vec::new();

        while !content.is_empty() {
            let field_name: Ident = content.parse()?;
            content.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "service_name" => {
                    service_name = Some(content.parse()?);
                }
                "base_path" => {
                    base_path = Some(content.parse()?);
                }
                "body_limit" => {
                    let lit: syn::LitInt = content.parse()?;
                    body_limit = Some(lit.base10_parse()?);
                }
                "openapi" => {
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
                        let key = openapi_content.parse::<Ident>()?;
                        if key != "output" {
                            return Err(Error::new(key.span(), "Expected openapi output field"));
                        }
                        openapi_content.parse::<Token![:]>()?;
                        let path = openapi_content.parse::<LitStr>()?;
                        if !openapi_content.is_empty() {
                            openapi_content.parse::<Token![,]>()?;
                        }
                        if !openapi_content.is_empty() {
                            return Err(Error::new(
                                openapi_content.span(),
                                "Unexpected field in openapi config",
                            ));
                        }
                        openapi = Some(OpenApiConfig::WithPath(path.value()));
                    }
                }
                "endpoints" => {
                    let endpoints_content;
                    syn::bracketed!(endpoints_content in content);

                    while !endpoints_content.is_empty() {
                        endpoints.push(endpoints_content.parse()?);

                        if !endpoints_content.is_empty() {
                            endpoints_content.parse::<Token![,]>()?;
                        }
                    }
                }
                _ => {
                    return Err(Error::new(
                        field_name.span(),
                        format!("Unknown field: {}", field_name),
                    ));
                }
            }

            if !content.is_empty() {
                content.parse::<Token![,]>()?;
            }
        }

        Ok(FileServiceDefinition {
            service_name: service_name
                .ok_or_else(|| Error::new(input.span(), "Missing service_name"))?,
            base_path: base_path.ok_or_else(|| Error::new(input.span(), "Missing base_path"))?,
            body_limit,
            openapi,
            endpoints,
        })
    }
}

impl Parse for Endpoint {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse operation type (UPLOAD or DOWNLOAD)
        let operation = if input.peek(kw::UPLOAD) {
            input.parse::<kw::UPLOAD>()?;
            Operation::Upload
        } else if input.peek(kw::DOWNLOAD) {
            input.parse::<kw::DOWNLOAD>()?;
            Operation::Download
        } else {
            return Err(Error::new(input.span(), "Expected UPLOAD or DOWNLOAD"));
        };

        // Parse auth requirement
        let auth = if input.peek(kw::UNAUTHORIZED) {
            input.parse::<kw::UNAUTHORIZED>()?;
            AuthRequirement::Unauthorized
        } else if input.peek(kw::WITH_PERMISSIONS) {
            input.parse::<kw::WITH_PERMISSIONS>()?;

            let content;
            syn::parenthesized!(content in input);

            let perms_content;
            syn::bracketed!(perms_content in content);

            let mut permission_groups = Vec::new();

            while !perms_content.is_empty() {
                if perms_content.peek(syn::LitStr) {
                    // Single permission
                    let perm: LitStr = perms_content.parse()?;
                    permission_groups.push(vec![perm.value()]);
                } else if perms_content.peek(token::Bracket) {
                    // Permission group
                    let group_content;
                    syn::bracketed!(group_content in perms_content);

                    let mut group = Vec::new();
                    while !group_content.is_empty() {
                        let perm: LitStr = group_content.parse()?;
                        group.push(perm.value());

                        if !group_content.is_empty() {
                            group_content.parse::<Token![,]>()?;
                        }
                    }
                    if group.is_empty() {
                        return Err(Error::new(
                            group_content.span(),
                            "Permission groups cannot be empty",
                        ));
                    }
                    permission_groups.push(group);
                }

                if !perms_content.is_empty() {
                    perms_content.parse::<Token![,]>()?;
                }
            }

            if permission_groups.is_empty() {
                return Err(Error::new(
                    perms_content.span(),
                    "WITH_PERMISSIONS requires at least one permission",
                ));
            }

            AuthRequirement::WithPermissions(permission_groups)
        } else {
            return Err(Error::new(
                input.span(),
                "Expected UNAUTHORIZED or WITH_PERMISSIONS",
            ));
        };

        // Parse endpoint path and name
        let (name, path, path_params) = if input.peek(Ident) && input.peek2(Token![/]) {
            // Has custom path
            let mut segments = Vec::new();
            let mut params = Vec::new();

            // Parse path segments
            while input.peek(Ident) || input.peek(Token![/]) {
                if input.peek(Token![/]) {
                    input.parse::<Token![/]>()?;
                    segments.push("/".to_string());
                }

                if input.peek(Ident) {
                    let ident: Ident = input.parse()?;
                    segments.push(ident.to_string());
                } else if input.peek(token::Brace) {
                    // Path parameter
                    let content;
                    braced!(content in input);

                    let param_name: Ident = content.parse()?;
                    content.parse::<Token![:]>()?;
                    let param_type: Type = content.parse()?;

                    segments.push(format!("{{{}}}", param_name));
                    params.push(PathParam {
                        name: param_name,
                        ty: param_type,
                    });
                }
            }

            // Extract the method name from the path
            let method_name = segments
                .iter()
                .filter(|s| !s.starts_with('/') && !s.starts_with('{'))
                .cloned()
                .collect::<Vec<_>>()
                .join("_");

            let name = Ident::new(&method_name, input.span());
            let path = LitStr::new(&segments.join(""), input.span());

            (name, Some(path), params)
        } else {
            // Just method name
            let name: Ident = input.parse()?;
            (name, None, Vec::new())
        };

        // Parse parameters and response
        // For file operations, we expect empty parentheses
        let _content;
        syn::parenthesized!(_content in input);

        let response_type = if input.peek(Token![->]) {
            input.parse::<Token![->]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        Ok(Endpoint {
            operation,
            auth,
            name,
            path,
            path_params,
            response_type,
        })
    }
}

mod kw {
    syn::custom_keyword!(UPLOAD);
    syn::custom_keyword!(DOWNLOAD);
    syn::custom_keyword!(UNAUTHORIZED);
    syn::custom_keyword!(WITH_PERMISSIONS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::{ToTokens, quote};

    fn parse_definition(tokens: proc_macro2::TokenStream) -> FileServiceDefinition {
        syn::parse2(tokens).expect("definition should parse")
    }

    fn parse_endpoint(tokens: proc_macro2::TokenStream) -> Endpoint {
        syn::parse2(tokens).expect("endpoint should parse")
    }

    fn parse_definition_error(tokens: proc_macro2::TokenStream) -> String {
        syn::parse2::<FileServiceDefinition>(tokens)
            .unwrap_err()
            .to_string()
    }

    fn parse_endpoint_error(tokens: proc_macro2::TokenStream) -> String {
        syn::parse2::<Endpoint>(tokens).unwrap_err().to_string()
    }

    fn type_tokens(ty: &Type) -> String {
        ty.to_token_stream().to_string()
    }

    #[test]
    fn definition_parses_body_limit_openapi_path_and_endpoint_variants() {
        let definition = parse_definition(quote!({
            service_name: FilesApi,
            base_path: "/api/files",
            body_limit: 1048576,
            openapi: { output: "target/openapi/files.json", },
            endpoints: [
                UPLOAD WITH_PERMISSIONS(["files:write"]) upload() -> UploadResponse,
                DOWNLOAD UNAUTHORIZED files/{bucket: String}/download/{id: u64}() -> axum::response::Response,
            ],
        }));

        assert_eq!(definition.service_name.to_string(), "FilesApi");
        assert_eq!(definition.base_path.value(), "/api/files");
        assert_eq!(definition.body_limit, Some(1_048_576));
        assert!(matches!(
            definition.openapi,
            Some(OpenApiConfig::WithPath(ref path)) if path == "target/openapi/files.json"
        ));
        assert_eq!(definition.endpoints.len(), 2);

        let upload = &definition.endpoints[0];
        assert!(matches!(upload.operation, Operation::Upload));
        assert!(matches!(
            upload.auth,
            AuthRequirement::WithPermissions(ref groups) if groups == &vec![vec!["files:write".to_string()]]
        ));
        assert_eq!(upload.name.to_string(), "upload");
        assert!(upload.path.is_none());
        assert_eq!(
            type_tokens(upload.response_type.as_ref().unwrap()),
            "UploadResponse"
        );

        let download = &definition.endpoints[1];
        assert!(matches!(download.operation, Operation::Download));
        assert!(matches!(download.auth, AuthRequirement::Unauthorized));
        assert_eq!(download.name.to_string(), "files_download");
        assert_eq!(
            download.path.as_ref().map(LitStr::value).as_deref(),
            Some("files/{bucket}/download/{id}")
        );
        assert_eq!(download.path_params.len(), 2);
        assert_eq!(download.path_params[0].name.to_string(), "bucket");
        assert_eq!(type_tokens(&download.path_params[0].ty), "String");
        assert_eq!(download.path_params[1].name.to_string(), "id");
        assert_eq!(type_tokens(&download.path_params[1].ty), "u64");
        assert_eq!(
            type_tokens(download.response_type.as_ref().unwrap()),
            "axum :: response :: Response"
        );
    }

    #[test]
    fn definition_parses_boolean_openapi_modes() {
        let enabled = parse_definition(quote!({
            service_name: FilesApi,
            base_path: "/api/files",
            openapi: true,
            endpoints: [],
        }));
        assert!(matches!(enabled.openapi, Some(OpenApiConfig::Enabled)));

        let disabled = parse_definition(quote!({
            service_name: FilesApi,
            base_path: "/api/files",
            openapi: false,
            endpoints: [],
        }));
        assert!(disabled.openapi.is_none());
    }

    #[test]
    fn endpoint_parses_permission_singletons_and_groups() {
        let endpoint = parse_endpoint(quote! {
            UPLOAD WITH_PERMISSIONS(["read", ["write", "verified"]]) upload()
        });

        assert!(matches!(
            endpoint.auth,
            AuthRequirement::WithPermissions(ref groups)
                if groups == &vec![
                    vec!["read".to_string()],
                    vec!["write".to_string(), "verified".to_string()],
                ]
        ));
        assert!(endpoint.response_type.is_none());
    }

    #[test]
    fn definition_rejects_missing_required_and_unknown_fields() {
        let err = parse_definition_error(quote!({
            base_path: "/api",
            endpoints: [],
        }));
        assert!(err.contains("Missing service_name"));

        let err = parse_definition_error(quote!({
            service_name: FilesApi,
            endpoints: [],
        }));
        assert!(err.contains("Missing base_path"));

        let err = parse_definition_error(quote!({
            service_name: FilesApi,
            base_path: "/api",
            unexpected: true,
            endpoints: [],
        }));
        assert!(err.contains("Unknown field"));
    }

    #[test]
    fn openapi_object_rejects_unknown_keys_and_leftover_fields() {
        let err = parse_definition_error(quote!({
            service_name: FilesApi,
            base_path: "/api",
            openapi: { path: "target/openapi.json" },
            endpoints: [],
        }));
        assert!(err.contains("Expected openapi output field"));

        let err = parse_definition_error(quote!({
            service_name: FilesApi,
            base_path: "/api",
            openapi: { output: "target/openapi.json", extra: "ignored" },
            endpoints: [],
        }));
        assert!(err.contains("Unexpected field in openapi config"));
    }

    #[test]
    fn endpoint_rejects_missing_operation_auth_and_empty_permission_groups() {
        let err = parse_endpoint_error(quote! {
            STREAM UNAUTHORIZED upload()
        });
        assert!(err.contains("Expected UPLOAD or DOWNLOAD"));

        let err = parse_endpoint_error(quote! {
            UPLOAD upload()
        });
        assert!(err.contains("Expected UNAUTHORIZED or WITH_PERMISSIONS"));

        let err = parse_endpoint_error(quote! {
            UPLOAD WITH_PERMISSIONS([]) upload()
        });
        assert!(err.contains("requires at least one permission"));

        let err = parse_endpoint_error(quote! {
            UPLOAD WITH_PERMISSIONS([[]]) upload()
        });
        assert!(err.contains("Permission groups cannot be empty"));
    }
}
