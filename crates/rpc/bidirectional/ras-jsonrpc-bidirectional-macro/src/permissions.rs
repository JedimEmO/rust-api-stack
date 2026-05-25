use crate::{AuthRequirement, BidirectionalServiceDefinition};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet};

pub fn generate_permissions_code(service_def: &BidirectionalServiceDefinition) -> TokenStream {
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
                transport: ras_permission_manifest::TransportKind::JsonRpcBidirectional,
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

fn collect_permissions(service_def: &BidirectionalServiceDefinition) -> Vec<String> {
    let mut permissions = BTreeSet::new();
    for method in service_def
        .client_to_server
        .iter()
        .chain(service_def.server_to_client_calls.iter())
    {
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
    direction: &'static str,
    method: String,
    kind: TokenStream,
    auth: &'a AuthRequirement,
    is_protected: bool,
}

impl OperationEntry<'_> {
    fn manifest_tokens(&self) -> TokenStream {
        let operation_id = &self.operation_id;
        let operation_name = &self.operation_name;
        let direction = self.direction;
        let method = &self.method;
        let kind = &self.kind;
        let auth = auth_tokens(self.auth);

        quote! {
            ras_permission_manifest::OperationPermissions {
                operation_id: #operation_id.to_string(),
                operation_name: #operation_name.to_string(),
                kind: #kind,
                wire: ras_permission_manifest::WireTarget::BidirectionalJsonRpc {
                    direction: #direction.to_string(),
                    method: #method.to_string(),
                },
                auth: #auth,
                version: None,
                canonical_operation_id: None,
            }
        }
    }
}

fn operation_entries(service_def: &BidirectionalServiceDefinition) -> Vec<OperationEntry<'_>> {
    let mut entries = Vec::new();
    for method in &service_def.client_to_server {
        let is_protected = !matches!(method.auth, AuthRequirement::Unauthorized);
        entries.push(OperationEntry {
            operation_id: format!(
                "{}.client_to_server.{}",
                service_def.service_name, method.name
            ),
            operation_name: method.name.to_string(),
            const_base: format!("client_to_server_{}", method.name),
            direction: "client_to_server",
            method: method.name.to_string(),
            kind: quote! { ras_permission_manifest::OperationKind::BidirectionalClientToServer },
            auth: &method.auth,
            is_protected,
        });
    }
    for method in &service_def.server_to_client_calls {
        let is_protected = !matches!(method.auth, AuthRequirement::Unauthorized);
        entries.push(OperationEntry {
            operation_id: format!(
                "{}.server_to_client_call.{}",
                service_def.service_name, method.name
            ),
            operation_name: method.name.to_string(),
            const_base: format!("server_to_client_call_{}", method.name),
            direction: "server_to_client_call",
            method: method.name.to_string(),
            kind: quote! { ras_permission_manifest::OperationKind::BidirectionalServerToClientCall },
            auth: &method.auth,
            is_protected,
        });
    }
    entries
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
