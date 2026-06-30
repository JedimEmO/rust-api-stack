use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, LitStr, Token, Type, parse::Parse, parse_macro_input};

mod client;
mod permissions;
mod server;

#[cfg(test)]
mod tests;

/// Macro to generate a bidirectional JSON-RPC service with client and server code
///
/// This macro generates:
/// - Server trait with methods for client_to_server handlers
/// - Server builder with WebSocket integration  
/// - Client struct with type-safe method calls and notification handlers
/// - Type-safe message enums for both directions
///
/// `client_to_server` methods declare one of three auth levels — `UNAUTHORIZED`,
/// `OPTIONAL_AUTH`, or `WITH_PERMISSIONS([...])`. An `OPTIONAL_AUTH` handler
/// receives a `ras_auth_core::Caller` built from the connection's optional user
/// (the server must allow anonymous connections for anonymous callers to reach
/// it). `OPTIONAL_AUTH` is rejected on `server_to_client` calls (outbound — no
/// inbound caller to identify).
///
/// See the tests for usage examples.
#[proc_macro]
pub fn jsonrpc_bidirectional_service(input: TokenStream) -> TokenStream {
    let service_definition = parse_macro_input!(input as BidirectionalServiceDefinition);

    match generate_service_code(service_definition) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[derive(Debug)]
struct BidirectionalServiceDefinition {
    service_name: Ident,
    feature_gated: bool,
    client_to_server: Vec<MethodDefinition>,
    server_to_client: Vec<NotificationDefinition>,
    server_to_client_calls: Vec<MethodDefinition>,
}

#[derive(Debug)]
struct MethodDefinition {
    auth: AuthRequirement,
    name: Ident,
    request_type: Type,
    response_type: Type,
}

#[derive(Debug)]
struct NotificationDefinition {
    name: Ident,
    params_type: Type,
}

#[derive(Debug)]
enum AuthRequirement {
    Unauthorized,
    /// Public method that opportunistically identifies its caller. Never rejected
    /// for auth reasons; the handler receives a `ras_auth_core::Caller` built from
    /// the connection's (optional) authenticated user.
    OptionalAuth,
    WithPermissions(Vec<Vec<String>>), // Vec of permission groups - OR between groups, AND within groups
}

impl Parse for BidirectionalServiceDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse the opening brace
        let content;
        syn::braced!(content in input);

        // Parse service_name: Ident
        let _ = content.parse::<Ident>()?; // "service_name"
        let _ = content.parse::<Token![:]>()?;
        let service_name = content.parse::<Ident>()?;
        let _ = content.parse::<Token![,]>()?;

        // Optional: feature_gated: <bool>,
        let mut feature_gated = false;
        if content
            .fork()
            .parse::<Ident>()
            .map(|ident| ident == "feature_gated")
            .unwrap_or(false)
        {
            let _ = content.parse::<Ident>()?; // "feature_gated"
            let _ = content.parse::<Token![:]>()?;
            feature_gated = content.parse::<syn::LitBool>()?.value();
            let _ = content.parse::<Token![,]>()?;
        }

        // Parse client_to_server: [...]
        let _ = content.parse::<Ident>()?; // "client_to_server"
        let _ = content.parse::<Token![:]>()?;

        let client_to_server_content;
        syn::bracketed!(client_to_server_content in content);

        let mut client_to_server = Vec::new();
        while !client_to_server_content.is_empty() {
            let method = client_to_server_content.parse::<MethodDefinition>()?;
            client_to_server.push(method);

            // Handle optional trailing comma
            if client_to_server_content.peek(Token![,]) {
                let _ = client_to_server_content.parse::<Token![,]>()?;
            }
        }

        let _ = content.parse::<Token![,]>()?;

        // Parse server_to_client: [...]
        let _ = content.parse::<Ident>()?; // "server_to_client"
        let _ = content.parse::<Token![:]>()?;

        let server_to_client_content;
        syn::bracketed!(server_to_client_content in content);

        let mut server_to_client = Vec::new();
        while !server_to_client_content.is_empty() {
            let notification = server_to_client_content.parse::<NotificationDefinition>()?;
            server_to_client.push(notification);

            // Handle optional trailing comma
            if server_to_client_content.peek(Token![,]) {
                let _ = server_to_client_content.parse::<Token![,]>()?;
            }
        }

        let _ = content.parse::<Token![,]>()?;

        // Parse server_to_client_calls: [...]
        let _ = content.parse::<Ident>()?; // "server_to_client_calls"
        let _ = content.parse::<Token![:]>()?;

        let server_to_client_calls_content;
        syn::bracketed!(server_to_client_calls_content in content);

        let mut server_to_client_calls = Vec::new();
        while !server_to_client_calls_content.is_empty() {
            let method = parse_server_to_client_call(&server_to_client_calls_content)?;
            server_to_client_calls.push(method);

            // Handle optional trailing comma
            if server_to_client_calls_content.peek(Token![,]) {
                let _ = server_to_client_calls_content.parse::<Token![,]>()?;
            }
        }

        Ok(BidirectionalServiceDefinition {
            service_name,
            feature_gated,
            client_to_server,
            server_to_client,
            server_to_client_calls,
        })
    }
}

fn parse_server_to_client_call(input: syn::parse::ParseStream) -> syn::Result<MethodDefinition> {
    let fork = input.fork();
    if fork.peek(syn::Ident) {
        let ident = fork.parse::<Ident>()?;
        match ident.to_string().as_str() {
            // A server_to_client call is outbound — there is no inbound caller to
            // identify, and the generated call/handler signatures carry no `Caller`.
            // Reject OPTIONAL_AUTH here rather than silently treating it as public.
            "OPTIONAL_AUTH" => {
                return Err(syn::Error::new(
                    ident.span(),
                    "OPTIONAL_AUTH is not supported on server_to_client calls: there is \
                     no inbound caller to identify. Use UNAUTHORIZED (it is only meaningful \
                     on client_to_server methods).",
                ));
            }
            "UNAUTHORIZED" | "WITH_PERMISSIONS" => {
                return input.parse::<MethodDefinition>();
            }
            _ => {}
        }
    }

    let name = input.parse::<Ident>()?;

    let request_content;
    syn::parenthesized!(request_content in input);
    let request_type = request_content.parse::<Type>()?;

    let _ = input.parse::<Token![->]>()?;
    let response_type = input.parse::<Type>()?;

    Ok(MethodDefinition {
        auth: AuthRequirement::Unauthorized,
        name,
        request_type,
        response_type,
    })
}

impl Parse for MethodDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
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

        // Parse method name
        let name = input.parse::<Ident>()?;

        // Parse (RequestType)
        let request_content;
        syn::parenthesized!(request_content in input);
        let request_type = request_content.parse::<Type>()?;

        // Parse -> ResponseType
        let _ = input.parse::<Token![->]>()?;
        let response_type = input.parse::<Type>()?;

        Ok(MethodDefinition {
            auth,
            name,
            request_type,
            response_type,
        })
    }
}

impl Parse for NotificationDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse notification name
        let name = input.parse::<Ident>()?;

        // Parse (ParamsType)
        let params_content;
        syn::parenthesized!(params_content in input);
        let params_type = params_content.parse::<Type>()?;

        Ok(NotificationDefinition { name, params_type })
    }
}

fn generate_service_code(
    service_def: BidirectionalServiceDefinition,
) -> syn::Result<proc_macro2::TokenStream> {
    let service_name_lower = service_def.service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_jsonrpc_bidirectional_{}_server", service_name_lower);
    let client_mod = format_ident!("__ras_jsonrpc_bidirectional_{}_client", service_name_lower);

    // Generate server code only when the macro crate's server feature is enabled.
    let server_code = server::generate_server_code(&service_def);

    // Generate client code only when the macro crate's client feature is enabled.
    let client_code = client::generate_client_code(&service_def);
    let permissions_code = if cfg!(feature = "permissions") {
        permissions::generate_permissions_code(&service_def)
    } else {
        quote! {}
    };

    // With `feature_gated: true` the generated code is wrapped in
    // `#[cfg(feature = ...)]` attributes resolved against the CONSUMER
    // crate's features, immune to workspace feature unification of the
    // macro crate's own features (which `cfg!` evaluates).
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

    let server_output = if feature_gated || cfg!(feature = "server") {
        quote! {
        #cfg_server
        mod #server_mod {
            use super::*;

            #server_code
        }

        #cfg_server
        pub use #server_mod::*;
        }
    } else {
        quote! {}
    };

    let client_output = if feature_gated || cfg!(feature = "client") {
        quote! {
        #cfg_client
        mod #client_mod {
            use super::*;

            #client_code
        }

        #cfg_client
        pub use #client_mod::*;
        }
    } else {
        quote! {}
    };

    let output = quote! {
        #permissions_code
        #server_output
        #client_output
    };

    Ok(output)
}
