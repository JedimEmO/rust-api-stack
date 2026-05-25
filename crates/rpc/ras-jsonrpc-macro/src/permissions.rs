use crate::{AuthRequirement, MethodDefinition, ServiceDefinition};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet};

pub fn generate_permissions_code(service_def: &ServiceDefinition) -> TokenStream {
    let service_name = service_def.service_name.to_string();
    let service_lower = service_name.to_lowercase();
    let manifest_fn_name = format_ident!("generate_{}_permission_manifest", service_lower);
    let permissions_mod_name = format_ident!("{}_permissions", service_lower);

    let permission_names = collect_permissions(service_def);
    let permission_idents = unique_const_idents(permission_names.iter().map(String::as_str));
    let permission_consts = permission_names.iter().map(|permission| {
        let ident = permission_idents
            .get(permission)
            .expect("permission ident must exist");
        quote! {
            pub const #ident: ras_permission_manifest::PermissionRef =
                ras_permission_manifest::PermissionRef::new(#permission);
        }
    });

    let operations = operation_entries(service_def);
    let operation_const_idents = unique_const_idents(operations.iter().filter_map(|operation| {
        if operation.is_protected {
            Some(operation.const_base.as_str())
        } else {
            None
        }
    }));
    let operation_consts = operations.iter().filter_map(|operation| {
        if !operation.is_protected {
            return None;
        }
        let ident = operation_const_idents
            .get(&operation.const_base)
            .expect("operation ident must exist");
        let requirement = static_requirement_tokens(operation.auth);
        Some(quote! {
            pub const #ident: ras_permission_manifest::StaticPermissionRequirement = #requirement;
        })
    });
    let manifest_operations = operations
        .iter()
        .map(|operation| operation.manifest_tokens());

    quote! {
        pub fn #manifest_fn_name() -> ras_permission_manifest::ServicePermissions {
            ras_permission_manifest::ServicePermissions {
                service_name: #service_name.to_string(),
                transport: ras_permission_manifest::TransportKind::JsonRpc,
                operations: vec![#(#manifest_operations),*],
            }
        }

        pub mod #permissions_mod_name {
            #(#permission_consts)*

            pub mod operations {
                #(#operation_consts)*
            }
        }
    }
}

fn collect_permissions(service_def: &ServiceDefinition) -> Vec<String> {
    let mut permissions = BTreeSet::new();
    for method in &service_def.methods {
        if let AuthRequirement::WithPermissions(groups) = &method.auth {
            for group in groups {
                permissions.extend(group.iter().cloned());
            }
        }
    }
    permissions.into_iter().collect()
}

struct OperationEntry<'a> {
    operation_id: String,
    operation_name: String,
    const_base: String,
    wire_method: String,
    auth: &'a AuthRequirement,
    version: Option<String>,
    canonical_operation_id: Option<String>,
    is_protected: bool,
}

impl OperationEntry<'_> {
    fn manifest_tokens(&self) -> TokenStream {
        let operation_id = &self.operation_id;
        let operation_name = &self.operation_name;
        let wire_method = &self.wire_method;
        let auth = auth_tokens(self.auth);
        let version = option_string_tokens(self.version.as_deref());
        let canonical_operation_id = option_string_tokens(self.canonical_operation_id.as_deref());

        quote! {
            ras_permission_manifest::OperationPermissions {
                operation_id: #operation_id.to_string(),
                operation_name: #operation_name.to_string(),
                kind: ras_permission_manifest::OperationKind::JsonRpcMethod,
                wire: ras_permission_manifest::WireTarget::JsonRpc {
                    method: #wire_method.to_string(),
                },
                auth: #auth,
                version: #version,
                canonical_operation_id: #canonical_operation_id,
            }
        }
    }
}

fn operation_entries(service_def: &ServiceDefinition) -> Vec<OperationEntry<'_>> {
    let mut entries = Vec::new();
    for method in &service_def.methods {
        let canonical_operation_id = operation_id(&service_def.service_name.to_string(), method);
        let canonical_wire = method
            .wire_name
            .clone()
            .unwrap_or_else(|| method.name.to_string());
        let is_protected = !matches!(method.auth, AuthRequirement::Unauthorized);

        entries.push(OperationEntry {
            operation_id: canonical_operation_id.clone(),
            operation_name: method.name.to_string(),
            const_base: method.name.to_string(),
            wire_method: canonical_wire,
            auth: &method.auth,
            version: method.version.clone(),
            canonical_operation_id: None,
            is_protected,
        });

        for version in &method.versions {
            entries.push(OperationEntry {
                operation_id: format!("{}@{}", canonical_operation_id, version.version),
                operation_name: method.name.to_string(),
                const_base: format!("{}_{}", method.name, version.version),
                wire_method: version.wire_name.clone(),
                auth: &method.auth,
                version: Some(version.version.clone()),
                canonical_operation_id: Some(canonical_operation_id.clone()),
                is_protected,
            });
        }
    }
    entries
}

fn operation_id(service_name: &str, method: &MethodDefinition) -> String {
    format!("{}.{}", service_name, method.name)
}

fn auth_tokens(auth: &AuthRequirement) -> TokenStream {
    match auth {
        AuthRequirement::Unauthorized => {
            quote! { ras_permission_manifest::AuthRequirementInfo::Public }
        }
        AuthRequirement::WithPermissions(groups) => {
            if groups.is_empty() || groups.iter().any(Vec::is_empty) {
                quote! { ras_permission_manifest::AuthRequirementInfo::Authenticated }
            } else {
                let group_tokens = groups.iter().map(|group| {
                    quote! {
                        ras_permission_manifest::PermissionGroupInfo {
                            all_of: vec![#(#group.to_string()),*],
                        }
                    }
                });
                quote! {
                    ras_permission_manifest::AuthRequirementInfo::Permissions {
                        any_of: vec![#(#group_tokens),*],
                    }
                }
            }
        }
    }
}

fn static_requirement_tokens(auth: &AuthRequirement) -> TokenStream {
    match auth {
        AuthRequirement::Unauthorized => {
            quote! { ras_permission_manifest::StaticPermissionRequirement::authenticated_only() }
        }
        AuthRequirement::WithPermissions(groups) => {
            if groups.is_empty() || groups.iter().any(Vec::is_empty) {
                quote! { ras_permission_manifest::StaticPermissionRequirement::authenticated_only() }
            } else {
                let group_tokens = groups.iter().map(|group| quote! { &[#(#group),*] });
                quote! { ras_permission_manifest::StaticPermissionRequirement::new(&[#(#group_tokens),*]) }
            }
        }
    }
}

fn option_string_tokens(value: Option<&str>) -> TokenStream {
    match value {
        Some(value) => quote! { Some(#value.to_string()) },
        None => quote! { None },
    }
}

fn unique_const_idents<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, proc_macro2::Ident> {
    let mut by_base: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in names {
        by_base
            .entry(sanitize_const_base(name))
            .or_default()
            .push(name.to_string());
    }

    let mut idents = BTreeMap::new();
    for (base, mut names) in by_base {
        names.sort();
        names.dedup();
        let has_collision = names.len() > 1;
        for name in names {
            let ident = if has_collision {
                format!("{}_{}", base, stable_hash_hex(&name))
            } else {
                base.clone()
            };
            idents.insert(name, format_ident!("{}", ident));
        }
    }
    idents
}

fn sanitize_const_base(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_underscore = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.is_empty() {
        "PERMISSION".to_string()
    } else {
        out
    };
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("PERMISSION_{}", out)
    } else {
        out
    }
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:08X}", hash as u32)
}
