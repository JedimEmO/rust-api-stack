//! The `ras_topology!` macro.
//!
//! Declares a logical RAS service graph and generates a function returning a
//! validated [`Topology`](https://docs.rs/ras-topology-core). Compile-time
//! guarantees come from two places:
//!
//! - The macro itself rejects duplicate service/gateway ids and routes or
//!   call edges that reference undeclared services.
//! - The generated code references manifest functions and permission
//!   constants *by path*, so renamed or removed services and permissions
//!   fail the build of the topology crate.
//!
//! Everything value-dependent (audience uniqueness, manifest membership of
//! edge permissions, exposure rules) is validated deterministically by
//! `TopologyBuilder::build` inside the generated function.
//!
//! # Syntax
//!
//! ```ignore
//! ras_topology!({
//!     topology_name: InternalTools,
//!
//!     services: [
//!         invoice: {
//!             audience: "invoice-service",
//!             manifest: invoice_api::generate_invoiceservice_permission_manifest,
//!             exposure: private,
//!         },
//!         billing: {
//!             audience: "billing-service",
//!             manifest: billing_api::generate_billingservice_permission_manifest,
//!             exposure: private,
//!         },
//!     ],
//!
//!     gateways: [
//!         public_web: {
//!             exposure: public,
//!             routes: [
//!                 "/invoices" => invoice { expose_private },
//!                 "/billing" => billing { expose_private, authenticated_only },
//!             ],
//!         },
//!     ],
//!
//!     calls: [
//!         billing -> invoice {
//!             permissions: [
//!                 invoice_api::invoiceservice_permissions::INVOICE_READ,
//!             ],
//!         },
//!     ],
//! });
//! ```
//!
//! This generates `pub fn internal_tools_topology() -> Result<Topology,
//! TopologyError>`. Permission entries must be generated
//! `PermissionRef` constants; raw strings go through the explicit
//! `custom_permissions: ["..."]` list instead.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Path, Token, braced, bracketed};

struct TopologyInput {
    name: Ident,
    services: Vec<ServiceDecl>,
    gateways: Vec<GatewayDecl>,
    calls: Vec<CallDecl>,
}

struct ServiceDecl {
    id: Ident,
    audience: LitStr,
    manifest: Path,
    exposure: Exposure,
}

struct GatewayDecl {
    id: Ident,
    exposure: Exposure,
    routes: Vec<RouteDecl>,
}

struct RouteDecl {
    prefix: LitStr,
    target: Ident,
    expose_private: bool,
    authenticated_only: bool,
}

struct CallDecl {
    caller: Ident,
    target: Ident,
    permissions: Vec<Path>,
    custom_permissions: Vec<LitStr>,
}

enum Exposure {
    Public,
    Private,
}

fn parse_exposure(input: ParseStream) -> syn::Result<Exposure> {
    let ident: Ident = input.parse()?;
    match ident.to_string().as_str() {
        "public" => Ok(Exposure::Public),
        "private" => Ok(Exposure::Private),
        other => Err(syn::Error::new_spanned(
            &ident,
            format!("exposure must be `public` or `private`, got `{other}`"),
        )),
    }
}

impl Parse for ServiceDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let id: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let body;
        braced!(body in input);

        let mut audience = None;
        let mut manifest = None;
        let mut exposure = None;
        while !body.is_empty() {
            let key: Ident = body.parse()?;
            body.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "audience" => audience = Some(body.parse::<LitStr>()?),
                "manifest" => manifest = Some(body.parse::<Path>()?),
                "exposure" => exposure = Some(parse_exposure(&body)?),
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!("unknown service field `{other}`"),
                    ));
                }
            }
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        let missing = |field: &str| {
            syn::Error::new_spanned(&id, format!("service `{id}` is missing `{field}`"))
        };
        Ok(ServiceDecl {
            audience: audience.ok_or_else(|| missing("audience"))?,
            manifest: manifest.ok_or_else(|| missing("manifest"))?,
            exposure: exposure.ok_or_else(|| missing("exposure"))?,
            id,
        })
    }
}

impl Parse for RouteDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let prefix: LitStr = input.parse()?;
        input.parse::<Token![=>]>()?;
        let target: Ident = input.parse()?;
        let mut expose_private = false;
        let mut authenticated_only = false;
        if input.peek(syn::token::Brace) {
            let flags;
            braced!(flags in input);
            let flags: Punctuated<Ident, Token![,]> =
                flags.parse_terminated(Ident::parse, Token![,])?;
            for flag in flags {
                match flag.to_string().as_str() {
                    "expose_private" => expose_private = true,
                    "authenticated_only" => authenticated_only = true,
                    other => {
                        return Err(syn::Error::new_spanned(
                            &flag,
                            format!("unknown route flag `{other}`"),
                        ));
                    }
                }
            }
        }
        Ok(RouteDecl {
            prefix,
            target,
            expose_private,
            authenticated_only,
        })
    }
}

impl Parse for GatewayDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let id: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let body;
        braced!(body in input);

        let mut exposure = None;
        let mut routes = Vec::new();
        while !body.is_empty() {
            let key: Ident = body.parse()?;
            body.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "exposure" => exposure = Some(parse_exposure(&body)?),
                "routes" => {
                    let list;
                    bracketed!(list in body);
                    let parsed: Punctuated<RouteDecl, Token![,]> =
                        list.parse_terminated(RouteDecl::parse, Token![,])?;
                    routes = parsed.into_iter().collect();
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!("unknown gateway field `{other}`"),
                    ));
                }
            }
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        Ok(GatewayDecl {
            exposure: exposure.ok_or_else(|| {
                syn::Error::new_spanned(&id, format!("gateway `{id}` is missing `exposure`"))
            })?,
            id,
            routes,
        })
    }
}

impl Parse for CallDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let caller: Ident = input.parse()?;
        input.parse::<Token![->]>()?;
        let target: Ident = input.parse()?;
        let body;
        braced!(body in input);

        let mut permissions = Vec::new();
        let mut custom_permissions = Vec::new();
        while !body.is_empty() {
            let key: Ident = body.parse()?;
            body.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "permissions" => {
                    let list;
                    bracketed!(list in body);
                    let parsed: Punctuated<Path, Token![,]> =
                        list.parse_terminated(Path::parse, Token![,])?;
                    permissions = parsed.into_iter().collect();
                }
                "custom_permissions" => {
                    let list;
                    bracketed!(list in body);
                    let parsed: Punctuated<LitStr, Token![,]> =
                        list.parse_terminated(|p| p.parse::<LitStr>(), Token![,])?;
                    custom_permissions = parsed.into_iter().collect();
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!("unknown call field `{other}`"),
                    ));
                }
            }
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        Ok(CallDecl {
            caller,
            target,
            permissions,
            custom_permissions,
        })
    }
}

impl Parse for TopologyInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        braced!(content in input);

        let mut name = None;
        let mut services = Vec::new();
        let mut gateways = Vec::new();
        let mut calls = Vec::new();

        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "topology_name" => name = Some(content.parse::<Ident>()?),
                "services" => {
                    let list;
                    bracketed!(list in content);
                    let parsed: Punctuated<ServiceDecl, Token![,]> =
                        list.parse_terminated(ServiceDecl::parse, Token![,])?;
                    services = parsed.into_iter().collect();
                }
                "gateways" => {
                    let list;
                    bracketed!(list in content);
                    let parsed: Punctuated<GatewayDecl, Token![,]> =
                        list.parse_terminated(GatewayDecl::parse, Token![,])?;
                    gateways = parsed.into_iter().collect();
                }
                "calls" => {
                    let list;
                    bracketed!(list in content);
                    let parsed: Punctuated<CallDecl, Token![,]> =
                        list.parse_terminated(CallDecl::parse, Token![,])?;
                    calls = parsed.into_iter().collect();
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!("unknown topology field `{other}`"),
                    ));
                }
            }
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        let name =
            name.ok_or_else(|| syn::Error::new(input.span(), "topology requires `topology_name`"))?;
        if services.is_empty() {
            return Err(syn::Error::new_spanned(
                &name,
                "topology requires at least one service",
            ));
        }

        Ok(TopologyInput {
            name,
            services,
            gateways,
            calls,
        })
    }
}

/// Compile-time structural checks: duplicate ids and references to
/// undeclared services fail the build with spanned errors.
fn check_structure(input: &TopologyInput) -> syn::Result<()> {
    let mut service_ids = std::collections::BTreeSet::new();
    for service in &input.services {
        if !service_ids.insert(service.id.to_string()) {
            return Err(syn::Error::new_spanned(
                &service.id,
                format!("duplicate service id `{}`", service.id),
            ));
        }
    }
    let mut gateway_ids = std::collections::BTreeSet::new();
    for gateway in &input.gateways {
        if !gateway_ids.insert(gateway.id.to_string()) {
            return Err(syn::Error::new_spanned(
                &gateway.id,
                format!("duplicate gateway id `{}`", gateway.id),
            ));
        }
        for route in &gateway.routes {
            if !service_ids.contains(&route.target.to_string()) {
                return Err(syn::Error::new_spanned(
                    &route.target,
                    format!("route targets undeclared service `{}`", route.target),
                ));
            }
        }
    }
    for call in &input.calls {
        for endpoint in [&call.caller, &call.target] {
            if !service_ids.contains(&endpoint.to_string()) {
                return Err(syn::Error::new_spanned(
                    endpoint,
                    format!("call references undeclared service `{endpoint}`"),
                ));
            }
        }
    }
    Ok(())
}

fn snake_case(ident: &Ident) -> String {
    let mut out = String::new();
    for (index, ch) in ident.to_string().chars().enumerate() {
        if ch.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Declare a RAS service topology. See the crate docs for syntax.
#[proc_macro]
pub fn ras_topology(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as TopologyInput);
    if let Err(err) = check_structure(&input) {
        return err.to_compile_error().into();
    }

    let fn_name = format_ident!("{}_topology", snake_case(&input.name));
    let topology_name = input.name.to_string();

    let services = input.services.iter().map(|service| {
        let id = service.id.to_string();
        let audience = &service.audience;
        let manifest = &service.manifest;
        let exposure = match service.exposure {
            Exposure::Public => quote!(::ras_topology_core::Exposure::Public),
            Exposure::Private => quote!(::ras_topology_core::Exposure::Private),
        };
        quote! {
            .service(#id, #audience, #exposure, #manifest())
        }
    });

    let gateways = input.gateways.iter().map(|gateway| {
        let id = gateway.id.to_string();
        let exposure = match gateway.exposure {
            Exposure::Public => quote!(::ras_topology_core::Exposure::Public),
            Exposure::Private => quote!(::ras_topology_core::Exposure::Private),
        };
        let routes = gateway.routes.iter().map(|route| {
            let prefix = &route.prefix;
            let target = route.target.to_string();
            let mut decl = quote! {
                ::ras_topology_core::RouteDecl::new(#prefix, #target)
            };
            if route.expose_private {
                decl = quote! { #decl.expose_private() };
            }
            if route.authenticated_only {
                decl = quote! { #decl.authenticated_only() };
            }
            decl
        });
        quote! {
            .gateway(#id, #exposure, vec![#(#routes),*])
        }
    });

    let calls = input.calls.iter().map(|call| {
        let caller = call.caller.to_string();
        let target = call.target.to_string();
        let typed = if call.permissions.is_empty() && call.custom_permissions.is_empty() {
            // An edge with no permissions is valid (authenticated-only call).
            Some(quote! { .call(#caller, #target, ::std::iter::empty::<&str>()) })
        } else if call.permissions.is_empty() {
            None
        } else {
            let permissions = call.permissions.iter().map(|path| quote!((#path).as_str()));
            Some(quote! { .call(#caller, #target, [#(#permissions),*]) })
        };
        let custom = if call.custom_permissions.is_empty() {
            None
        } else {
            let permissions = call.custom_permissions.iter();
            Some(quote! {
                .call_with_custom_permissions(#caller, #target, [#(#permissions),*])
            })
        };
        quote! { #typed #custom }
    });

    let expanded = quote! {
        /// Generated by `ras_topology!`: build and validate the declared
        /// topology.
        pub fn #fn_name() -> ::std::result::Result<
            ::ras_topology_core::Topology,
            ::ras_topology_core::TopologyError,
        > {
            ::ras_topology_core::Topology::builder(#topology_name)
                #(#services)*
                #(#gateways)*
                #(#calls)*
                .build()
        }
    };
    expanded.into()
}
