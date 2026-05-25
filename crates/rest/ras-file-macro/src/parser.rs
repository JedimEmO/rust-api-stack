use syn::{
    Error, Ident, LitStr, Result, Token, Type, braced,
    parse::{Parse, ParseStream},
    token,
};

#[derive(Debug)]
pub struct FileServiceDefinition {
    pub service_name: Ident,
    pub base_path: LitStr,
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
    pub path: LitStr,
    pub path_params: Vec<PathParam>,
}

#[derive(Debug)]
pub enum Operation {
    Upload {
        config: UploadConfig,
        response_type: Box<Type>,
    },
    Download {
        config: DownloadConfig,
    },
}

#[derive(Debug)]
pub enum AuthRequirement {
    Unauthorized,
    WithPermissions(Vec<Vec<String>>),
}

#[derive(Debug, Clone)]
pub struct PathParam {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug)]
pub struct UploadConfig {
    pub max_total_bytes: MaxBytes,
    pub reject_unknown_fields: bool,
    pub parts: Vec<UploadPart>,
}

#[derive(Debug, Clone)]
pub enum MaxBytes {
    Limited(u64),
    Unlimited,
}

#[derive(Debug)]
pub struct UploadPart {
    pub kind: UploadPartKind,
    pub name: Ident,
    pub ty: Option<Type>,
    pub required: bool,
    pub max_count: usize,
    pub max_bytes: u64,
    pub content_types: Vec<String>,
    pub filename: FilenamePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadPartKind {
    File,
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilenamePolicy {
    Optional,
    Required,
    Forbidden,
}

#[derive(Debug, Default)]
pub struct DownloadConfig {
    pub content_types: Vec<String>,
    pub ranges: bool,
}

impl Parse for FileServiceDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        braced!(content in input);

        let mut service_name = None;
        let mut base_path = None;
        let mut openapi = None;
        let mut endpoints = Vec::new();

        while !content.is_empty() {
            let field_name: Ident = content.parse()?;
            content.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "service_name" => service_name = Some(content.parse()?),
                "base_path" => base_path = Some(content.parse()?),
                "body_limit" => {
                    return Err(Error::new(
                        field_name.span(),
                        "body_limit was removed in file_service v2; use per-upload max_total_bytes",
                    ));
                }
                "openapi" => {
                    if content.peek(syn::LitBool) {
                        let enabled = content.parse::<syn::LitBool>()?;
                        if enabled.value() {
                            openapi = Some(OpenApiConfig::Enabled);
                        }
                    } else if content.peek(syn::token::Brace) {
                        let openapi_content;
                        syn::braced!(openapi_content in content);

                        let key = openapi_content.parse::<Ident>()?;
                        if key != "output" {
                            return Err(Error::new(key.span(), "Expected openapi output field"));
                        }
                        openapi_content.parse::<Token![:]>()?;
                        let path = openapi_content.parse::<LitStr>()?;

                        if openapi_content.peek(Token![,]) {
                            openapi_content.parse::<Token![,]>()?;
                        }
                        if !openapi_content.is_empty() {
                            return Err(Error::new(
                                openapi_content.span(),
                                "Unexpected field in openapi config",
                            ));
                        }

                        openapi = Some(OpenApiConfig::WithPath(path.value()));
                    } else {
                        return Err(Error::new(
                            content.span(),
                            "Expected true, false, or { output: ... }",
                        ));
                    }
                }
                "endpoints" => {
                    let endpoints_content;
                    syn::bracketed!(endpoints_content in content);

                    while !endpoints_content.is_empty() {
                        endpoints.push(endpoints_content.parse()?);

                        if endpoints_content.peek(Token![,]) {
                            endpoints_content.parse::<Token![,]>()?;
                        }
                    }
                }
                _ => {
                    return Err(Error::new(
                        field_name.span(),
                        format!("Unknown field: {field_name}"),
                    ));
                }
            }

            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        Ok(FileServiceDefinition {
            service_name: service_name
                .ok_or_else(|| Error::new(input.span(), "Missing service_name"))?,
            base_path: base_path.ok_or_else(|| Error::new(input.span(), "Missing base_path"))?,
            openapi,
            endpoints,
        })
    }
}

impl Parse for Endpoint {
    fn parse(input: ParseStream) -> Result<Self> {
        let operation_ident: Ident = input.parse()?;
        let operation_name = operation_ident.to_string();

        let auth = parse_auth(input)?;
        let (name, path, path_params) = parse_endpoint_path(input)?;

        let operation = match operation_name.as_str() {
            "UPLOAD" => {
                if input.peek(token::Paren) {
                    return Err(Error::new(
                        input.span(),
                        "Expected `multipart { ... }` after UPLOAD path",
                    ));
                }
                let multipart_ident: Ident = input.parse()?;
                if multipart_ident != "multipart" {
                    return Err(Error::new(
                        multipart_ident.span(),
                        "Expected `multipart { ... }` after UPLOAD path",
                    ));
                }
                let config = input.parse::<UploadConfig>()?;
                input.parse::<Token![->]>()?;
                let response_type = input.parse::<Type>()?;
                Operation::Upload {
                    config,
                    response_type: Box::new(response_type),
                }
            }
            "DOWNLOAD" => {
                let config = if input.peek(token::Brace) {
                    input.parse::<DownloadConfig>()?
                } else {
                    DownloadConfig::default()
                };

                if input.peek(Token![->]) {
                    return Err(Error::new(
                        input.span(),
                        "DOWNLOAD response types were removed in file_service v2; return ras_file_core::DownloadResponse from the generated trait",
                    ));
                }

                Operation::Download { config }
            }
            _ => {
                return Err(Error::new(
                    operation_ident.span(),
                    "Expected UPLOAD or DOWNLOAD",
                ));
            }
        };

        Ok(Self {
            operation,
            auth,
            name,
            path,
            path_params,
        })
    }
}

impl Parse for UploadConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        braced!(content in input);

        let mut max_total_bytes = None;
        let mut reject_unknown_fields = true;
        let mut parts = Vec::new();

        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![:]>()?;

            match key.to_string().as_str() {
                "max_total_bytes" => max_total_bytes = Some(parse_max_bytes(&content)?),
                "reject_unknown_fields" => {
                    reject_unknown_fields = content.parse::<syn::LitBool>()?.value();
                }
                "parts" => {
                    let parts_content;
                    syn::bracketed!(parts_content in content);
                    while !parts_content.is_empty() {
                        parts.push(parts_content.parse()?);
                        if parts_content.peek(Token![,]) {
                            parts_content.parse::<Token![,]>()?;
                        }
                    }
                }
                _ => {
                    return Err(Error::new(
                        key.span(),
                        "Expected max_total_bytes, reject_unknown_fields, or parts",
                    ));
                }
            }

            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        if parts.is_empty() {
            return Err(Error::new(
                input.span(),
                "UPLOAD multipart requires at least one part",
            ));
        }

        Ok(Self {
            max_total_bytes: max_total_bytes.ok_or_else(|| {
                Error::new(input.span(), "UPLOAD multipart requires max_total_bytes")
            })?,
            reject_unknown_fields,
            parts,
        })
    }
}

impl Parse for UploadPart {
    fn parse(input: ParseStream) -> Result<Self> {
        let kind_ident: Ident = input.parse()?;
        let kind = match kind_ident.to_string().as_str() {
            "file" => UploadPartKind::File,
            "json" => UploadPartKind::Json,
            "text" => UploadPartKind::Text,
            _ => {
                return Err(Error::new(
                    kind_ident.span(),
                    "Expected file, json, or text",
                ));
            }
        };

        let name: Ident = input.parse()?;
        let ty = if kind == UploadPartKind::Json {
            input.parse::<Token![:]>()?;
            Some(input.parse::<Type>()?)
        } else {
            None
        };

        if kind != UploadPartKind::Json && input.peek(Token![:]) {
            return Err(Error::new(
                input.span(),
                "Only json parts declare a Rust type",
            ));
        }

        let content;
        braced!(content in input);

        let mut required = false;
        let mut max_count = 1usize;
        let mut max_bytes = None;
        let mut content_types = Vec::new();
        let mut filename = FilenamePolicy::Optional;

        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![:]>()?;

            match key.to_string().as_str() {
                "required" => required = content.parse::<syn::LitBool>()?.value(),
                "max_count" => {
                    let lit = content.parse::<syn::LitInt>()?;
                    max_count = lit.base10_parse()?;
                    if max_count == 0 {
                        return Err(Error::new(
                            lit.span(),
                            "max_count must be greater than zero",
                        ));
                    }
                }
                "max_bytes" => {
                    let lit = content.parse::<syn::LitInt>()?;
                    max_bytes = Some(lit.base10_parse()?);
                }
                "content_types" => content_types = parse_string_array(&content)?,
                "filename" => {
                    let value: Ident = content.parse()?;
                    filename = match value.to_string().as_str() {
                        "optional" => FilenamePolicy::Optional,
                        "required" => FilenamePolicy::Required,
                        "forbidden" => FilenamePolicy::Forbidden,
                        _ => {
                            return Err(Error::new(
                                value.span(),
                                "Expected optional, required, or forbidden",
                            ));
                        }
                    };
                }
                _ => {
                    return Err(Error::new(
                        key.span(),
                        "Expected required, max_count, max_bytes, content_types, or filename",
                    ));
                }
            }

            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        let max_bytes = max_bytes
            .ok_or_else(|| Error::new(input.span(), "Every multipart part requires max_bytes"))?;

        if kind != UploadPartKind::File && filename != FilenamePolicy::Optional {
            return Err(Error::new(
                input.span(),
                "filename policy is only valid for file parts",
            ));
        }

        Ok(Self {
            kind,
            name,
            ty,
            required,
            max_count,
            max_bytes,
            content_types,
            filename,
        })
    }
}

impl Parse for DownloadConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        braced!(content in input);

        let mut config = DownloadConfig::default();

        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![:]>()?;

            match key.to_string().as_str() {
                "content_types" => config.content_types = parse_string_array(&content)?,
                "ranges" => config.ranges = content.parse::<syn::LitBool>()?.value(),
                _ => {
                    return Err(Error::new(key.span(), "Expected content_types or ranges"));
                }
            }

            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        Ok(config)
    }
}

fn parse_auth(input: ParseStream) -> Result<AuthRequirement> {
    let auth_ident: Ident = input.parse()?;
    match auth_ident.to_string().as_str() {
        "UNAUTHORIZED" => Ok(AuthRequirement::Unauthorized),
        "WITH_PERMISSIONS" => {
            let content;
            syn::parenthesized!(content in input);

            let perms_content;
            syn::bracketed!(perms_content in content);

            let mut permission_groups = Vec::new();

            while !perms_content.is_empty() {
                if perms_content.peek(LitStr) {
                    let perm: LitStr = perms_content.parse()?;
                    permission_groups.push(vec![perm.value()]);
                } else if perms_content.peek(token::Bracket) {
                    let group_content;
                    syn::bracketed!(group_content in perms_content);

                    let mut group = Vec::new();
                    while !group_content.is_empty() {
                        let perm: LitStr = group_content.parse()?;
                        group.push(perm.value());
                        if group_content.peek(Token![,]) {
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
                } else {
                    return Err(Error::new(
                        perms_content.span(),
                        "Expected permission string or group",
                    ));
                }

                if perms_content.peek(Token![,]) {
                    perms_content.parse::<Token![,]>()?;
                }
            }

            if permission_groups.is_empty() {
                return Err(Error::new(
                    perms_content.span(),
                    "WITH_PERMISSIONS requires at least one permission",
                ));
            }

            Ok(AuthRequirement::WithPermissions(permission_groups))
        }
        _ => Err(Error::new(
            auth_ident.span(),
            "Expected UNAUTHORIZED or WITH_PERMISSIONS",
        )),
    }
}

fn parse_endpoint_path(input: ParseStream) -> Result<(Ident, LitStr, Vec<PathParam>)> {
    let mut segments = Vec::new();
    let mut params = Vec::new();
    let mut method_parts = Vec::new();

    let first: Ident = input.parse()?;
    segments.push(first.to_string());
    method_parts.push(first.to_string());

    while input.peek(Token![/]) {
        input.parse::<Token![/]>()?;

        if input.peek(token::Brace) {
            let content;
            braced!(content in input);

            let param_name: Ident = content.parse()?;
            content.parse::<Token![:]>()?;
            let param_type: Type = content.parse()?;

            segments.push(format!("{{{}}}", param_name));
            method_parts.push(format!("by_{}", param_name));
            params.push(PathParam {
                name: param_name,
                ty: param_type,
            });
        } else {
            let segment: Ident = input.parse()?;
            segments.push(segment.to_string());
            method_parts.push(segment.to_string());
        }
    }

    let name = Ident::new(&method_parts.join("_"), first.span());
    let path = LitStr::new(&format!("/{}", segments.join("/")), first.span());

    Ok((name, path, params))
}

fn parse_max_bytes(input: ParseStream) -> Result<MaxBytes> {
    if input.peek(syn::LitInt) {
        let lit = input.parse::<syn::LitInt>()?;
        Ok(MaxBytes::Limited(lit.base10_parse()?))
    } else {
        let ident: Ident = input.parse()?;
        if ident == "unlimited" {
            Ok(MaxBytes::Unlimited)
        } else {
            Err(Error::new(
                ident.span(),
                "Expected byte limit integer or unlimited",
            ))
        }
    }
}

fn parse_string_array(input: ParseStream) -> Result<Vec<String>> {
    let content;
    syn::bracketed!(content in input);

    let mut values = Vec::new();
    while !content.is_empty() {
        values.push(content.parse::<LitStr>()?.value());
        if content.peek(Token![,]) {
            content.parse::<Token![,]>()?;
        }
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_error(input: &str) -> String {
        syn::parse_str::<FileServiceDefinition>(input)
            .expect_err("definition should fail to parse")
            .to_string()
    }

    fn assert_parse_error_contains(input: &str, expected: &str) {
        let error = parse_error(input);
        assert!(
            error.contains(expected),
            "expected parse error to contain `{expected}`, got `{error}`"
        );
    }

    #[test]
    fn rejects_removed_body_limit_field() {
        assert_parse_error_contains(
            r#"{
                service_name: Files,
                base_path: "/files",
                body_limit: 1024,
                endpoints: []
            }"#,
            "body_limit was removed",
        );
    }

    #[test]
    fn rejects_v1_upload_without_multipart_contract() {
        assert_parse_error_contains(
            r#"{
                service_name: Files,
                base_path: "/files",
                endpoints: [
                    UPLOAD UNAUTHORIZED upload() -> UploadResponse,
                ]
            }"#,
            "Expected `multipart { ... }` after UPLOAD path",
        );
    }

    #[test]
    fn rejects_upload_without_total_limit() {
        assert_parse_error_contains(
            r#"{
                service_name: Files,
                base_path: "/files",
                endpoints: [
                    UPLOAD UNAUTHORIZED upload multipart {
                        parts: [
                            file file {
                                required: true,
                                max_bytes: 1024,
                            },
                        ],
                    } -> UploadResponse,
                ]
            }"#,
            "UPLOAD multipart requires max_total_bytes",
        );
    }

    #[test]
    fn rejects_upload_part_without_byte_limit() {
        assert_parse_error_contains(
            r#"{
                service_name: Files,
                base_path: "/files",
                endpoints: [
                    UPLOAD UNAUTHORIZED upload multipart {
                        max_total_bytes: 1024,
                        parts: [
                            file file {
                                required: true,
                            },
                        ],
                    } -> UploadResponse,
                ]
            }"#,
            "Every multipart part requires max_bytes",
        );
    }

    #[test]
    fn rejects_filename_policy_on_non_file_part() {
        assert_parse_error_contains(
            r#"{
                service_name: Files,
                base_path: "/files",
                endpoints: [
                    UPLOAD UNAUTHORIZED upload multipart {
                        max_total_bytes: 1024,
                        parts: [
                            text note {
                                required: true,
                                max_bytes: 128,
                                filename: forbidden,
                            },
                        ],
                    } -> UploadResponse,
                ]
            }"#,
            "filename policy is only valid for file parts",
        );
    }

    #[test]
    fn rejects_download_response_type() {
        assert_parse_error_contains(
            r#"{
                service_name: Files,
                base_path: "/files",
                endpoints: [
                    DOWNLOAD UNAUTHORIZED download/{file_id: String} -> (),
                ]
            }"#,
            "DOWNLOAD response types were removed",
        );
    }

    #[test]
    fn parses_unlimited_upload_and_reject_unknown_default() {
        let definition = syn::parse_str::<FileServiceDefinition>(
            r#"{
                service_name: Files,
                base_path: "/files",
                endpoints: [
                    UPLOAD UNAUTHORIZED upload multipart {
                        max_total_bytes: unlimited,
                        parts: [
                            text note {
                                required: false,
                                max_bytes: 128,
                            },
                        ],
                    } -> UploadResponse,
                ]
            }"#,
        )
        .expect("definition should parse");

        let Operation::Upload { config, .. } = &definition.endpoints[0].operation else {
            panic!("expected upload endpoint");
        };
        assert!(matches!(config.max_total_bytes, MaxBytes::Unlimited));
        assert!(config.reject_unknown_fields);
    }
}
