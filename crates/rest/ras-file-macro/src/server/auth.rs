use crate::parser::AuthRequirement;
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn generate_auth_check(auth: &AuthRequirement) -> TokenStream {
    match auth {
        AuthRequirement::Unauthorized => quote! {
            let user: Option<::ras_auth_core::AuthenticatedUser> = None;
        },
        AuthRequirement::OptionalAuth => quote! {
            // Best-effort authentication for an OPTIONAL_AUTH file route — never
            // rejected. Resolves to None for a missing/invalid credential (or a
            // cookie that fails CSRF on an unsafe method), Some(user) otherwise.
            // The caller is surfaced through FileRequestContext::new below.
            let user: Option<::ras_auth_core::AuthenticatedUser> =
                ::ras_auth_core::resolve_caller(method, &parts.headers, &state.4, state.1.as_deref())
                    .await
                    .into_authenticated();
        },
        AuthRequirement::WithPermissions(_) => quote! {
            let auth_provider = match state.1.as_ref() {
                Some(provider) => provider,
                None => return __ras_file_error_response(::ras_file_core::FileError::Internal),
            };

            let auth_credential = match ::ras_auth_core::extract_auth_credential(&parts.headers, &state.4) {
                Ok(credential) => credential,
                Err(_) => return __ras_file_error_response(::ras_file_core::FileError::Unauthorized),
            };

            if ::ras_auth_core::validate_csrf_for_credential(method, &parts.headers, &auth_credential, &state.4).is_err() {
                return __ras_file_error_response(::ras_file_core::FileError::Forbidden);
            }

            let user = match auth_provider.authenticate(auth_credential.token().to_string()).await {
                Ok(user) => Some(user),
                Err(_) => return __ras_file_error_response(::ras_file_core::FileError::Unauthorized),
            };
        },
    }
}

pub(super) fn generate_permission_check(auth: &AuthRequirement) -> TokenStream {
    match auth {
        // Public routes (Unauthorized / OptionalAuth) have no permission gate.
        AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => quote! {},
        AuthRequirement::WithPermissions(permission_groups) => {
            let groups = permission_groups.iter().map(|group| {
                let perms = group.iter();
                quote! { vec![#(#perms.to_string()),*] }
            });

            quote! {
                // OR-of-AND permission check (shared ras-auth-core implementation).
                // A group list with no non-empty groups means "any authenticated
                // user", consistent with the REST and JSON-RPC macros.
                let required_permission_groups: Vec<Vec<String>> = vec![#(#groups),*];
                let authenticated_user = user.as_ref().expect("authenticated user exists after auth check");
                if ::ras_auth_core::check_permission_groups(auth_provider.as_ref(), authenticated_user, &required_permission_groups).is_err() {
                    return __ras_file_error_response(::ras_file_core::FileError::Forbidden);
                }
            }
        }
    }
}
