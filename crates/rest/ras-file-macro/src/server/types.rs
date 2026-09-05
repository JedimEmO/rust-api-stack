use crate::parser::{Endpoint, FileServiceDefinition, Operation, UploadPart, UploadPartKind};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) fn generate_support_types(definition: &FileServiceDefinition) -> TokenStream {
    let support = definition.endpoints.iter().flat_map(|endpoint| {
        let path_struct = path_struct_name(&definition.service_name, endpoint);
        let path_fields = endpoint.path_params.iter().map(|param| {
            let name = &param.name;
            let ty = &param.ty;
            quote! { pub #name: #ty }
        });

        let mut tokens = vec![quote! {
            #[derive(Debug, Clone)]
            pub struct #path_struct {
                #(#path_fields),*
            }
        }];

        if let Operation::Upload { config, .. } = &endpoint.operation {
            let part_enum = part_enum_name(&definition.service_name, endpoint);
            let has_file_part = config
                .parts
                .iter()
                .any(|part| part.kind == UploadPartKind::File);
            let variants = config.parts.iter().map(|part| {
                let variant = part_variant_name(part);
                match part.kind {
                    UploadPartKind::File => quote! { #variant(::ras_file_core::IncomingFile<'a>) },
                    UploadPartKind::Json => {
                        let ty = part.ty.as_ref().expect("json part type");
                        quote! { #variant(#ty) }
                    }
                    UploadPartKind::Text => quote! { #variant(String) },
                }
            });
            let lifetime_variant = if has_file_part {
                quote! {}
            } else {
                quote! { #[doc(hidden)] __Lifetime(std::marker::PhantomData<&'a ()>), }
            };

            let consumed_arms = config.parts.iter().map(|part| {
                let variant = part_variant_name(part);
                match part.kind {
                    UploadPartKind::File => quote! { Self::#variant(file) => file.is_finished() },
                    UploadPartKind::Json | UploadPartKind::Text => {
                        quote! { Self::#variant(_) => true }
                    }
                }
            });
            let lifetime_consumed_arm = if has_file_part {
                quote! {}
            } else {
                quote! { Self::__Lifetime(_) => true, }
            };

            let bytes_arms = config.parts.iter().map(|part| {
                let variant = part_variant_name(part);
                match part.kind {
                    UploadPartKind::File => quote! { Self::#variant(file) => file.bytes_read() },
                    UploadPartKind::Json | UploadPartKind::Text => {
                        quote! { Self::#variant(_) => 0 }
                    }
                }
            });
            let lifetime_bytes_arm = if has_file_part {
                quote! {}
            } else {
                quote! { Self::__Lifetime(_) => 0, }
            };

            tokens.push(quote! {
                pub enum #part_enum<'a> {
                    #lifetime_variant
                    #(#variants),*
                }

                impl #part_enum<'_> {
                    pub fn is_consumed(&self) -> bool {
                        match self {
                            #lifetime_consumed_arm
                            #(#consumed_arms),*
                        }
                    }

                    pub fn bytes_read(&self) -> u64 {
                        match self {
                            #lifetime_bytes_arm
                            #(#bytes_arms),*
                        }
                    }
                }
            });
        }

        tokens
    });

    quote! { #(#support)* }
}

pub(super) fn generate_trait_methods(
    definition: &FileServiceDefinition,
    _trait_name: &Ident,
) -> TokenStream {
    let methods = definition.endpoints.iter().map(|endpoint| {
        let path_struct = path_struct_name(&definition.service_name, endpoint);
        let handler_name = &endpoint.name;

        match &endpoint.operation {
            Operation::Upload { response_type, .. } => {
                let state_type = upload_state_type_name(endpoint);
                let begin = format_ident!("{}_begin", handler_name);
                let part = format_ident!("{}_part", handler_name);
                let finish = format_ident!("{}_finish", handler_name);
                let abort = format_ident!("{}_abort", handler_name);
                let part_enum = part_enum_name(&definition.service_name, endpoint);

                quote! {
                    type #state_type: Send;

                    async fn #begin(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: &#path_struct,
                    ) -> ::ras_file_core::FileResult<Self::#state_type>;

                    async fn #part(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: &#path_struct,
                        state: &mut Self::#state_type,
                        part: &mut #part_enum<'_>,
                    ) -> ::ras_file_core::FileResult<()>;

                    async fn #finish(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: &#path_struct,
                        state: Self::#state_type,
                        summary: ::ras_file_core::UploadSummary,
                    ) -> ::ras_file_core::FileResult<::ras_file_core::JsonResponse<#response_type>>;

                    async fn #abort(
                        &self,
                        _ctx: &::ras_file_core::FileRequestContext<'_>,
                        _path: &#path_struct,
                        _state: Self::#state_type,
                        _error: &::ras_file_core::FileError,
                    ) {
                    }
                }
            }
            Operation::Download { .. } => {
                quote! {
                    async fn #handler_name(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: #path_struct,
                    ) -> ::ras_file_core::FileResult<::ras_file_core::DownloadResponse>;
                }
            }
        }
    });

    quote! { #(#methods)* }
}

pub(super) fn path_struct_name(service_name: &Ident, endpoint: &Endpoint) -> Ident {
    format_ident!(
        "{}{}Path",
        service_name,
        pascal_ident_segment(&endpoint.name.to_string())
    )
}

pub(super) fn part_enum_name(service_name: &Ident, endpoint: &Endpoint) -> Ident {
    format_ident!(
        "{}{}Part",
        service_name,
        pascal_ident_segment(&endpoint.name.to_string())
    )
}

fn upload_state_type_name(endpoint: &Endpoint) -> Ident {
    format_ident!("{}State", pascal_ident_segment(&endpoint.name.to_string()))
}

pub(super) fn part_variant_name(part: &UploadPart) -> Ident {
    format_ident!("{}", pascal_ident_segment(&part.name.to_string()))
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
        "Generated".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("V{out}")
    } else {
        out
    }
}
