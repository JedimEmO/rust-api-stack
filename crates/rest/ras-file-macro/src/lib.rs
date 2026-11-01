//! The `file_service!` procedural macro for typed file upload and download
//! APIs.
//!
//! One definition generates a handler trait, an Axum router builder, a typed
//! Rust client with multipart builders, and an optional OpenAPI document. The
//! generated server checks auth before it reads the body, enforces a byte
//! limit on the request and on every multipart part, and streams file parts
//! to the handler instead of buffering them.
//!
//! The generated code uses the runtime types in `ras-file-core`
//! (`FileRequestContext`, `IncomingFile`, `DownloadResponse`, `FileError`) and
//! the client types in `ras-transport-core`. A proc-macro crate cannot
//! re-export them, so the crate that invokes the macro depends on them
//! directly.
//!
//! # Cargo features
//!
//! - `server` (default): handler trait, router builder and OpenAPI functions.
//! - `client`: client, `build_with_transport(...)` and multipart builders.
//! - `reqwest` (default): implies `client` and adds a `build()` that uses
//!   `ras_transport_core::ReqwestTransport`.
//! - `fs`: implies `client` and enables streaming file parts from disk. The
//!   generated helper is also gated on an `fs` feature of the invoking crate.
//! - `permissions`: emits a permission manifest built on
//!   `ras-permission-manifest`.
//!
//! See the [`file_service!` guide](https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/macros/file-service.md)
//! for the full grammar and a walkthrough.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

mod client;
mod openapi;
mod parser;
mod permissions;
mod server;

use parser::FileServiceDefinition;

/// Generates a multipart file upload and download service: an Axum server,
/// a typed client, and optionally an OpenAPI document.
///
/// ```text
/// file_service!({
///     service_name: DocumentService,
///     base_path: "/api/documents",
///     openapi: true,                      // or false, or { output: "path.json" }
///     feature_gated: false,               // optional
///     endpoints: [
///         UPLOAD AUTH upload multipart {
///             max_total_bytes: 52428800,  // or `unlimited`
///             reject_unknown_fields: true, // optional, default true
///             parts: [
///                 file file { max_bytes: 52428800, required: true, max_count: 1,
///                             content_types: ["application/pdf"], filename: optional },
///                 json metadata: UploadMetadata { max_bytes: 4096 },
///                 text note { max_bytes: 1024 },
///             ],
///         } -> UploadResponse,
///         DOWNLOAD AUTH download/{file_id: String} { content_types: [...], ranges: true },
///     ]
/// });
/// ```
///
/// `AUTH` is one of `UNAUTHORIZED`, `OPTIONAL_AUTH`, or
/// `WITH_PERMISSIONS([...])`, where `[a, b] | [c]` means "a and b, or c" and
/// `[]` means any authenticated user. For `OPTIONAL_AUTH` the route is never
/// rejected for auth reasons; the caller arrives as `ctx.user` on
/// `FileRequestContext`, `Some(user)` for a valid credential and `None`
/// otherwise.
///
/// Every upload needs `max_total_bytes`, and every part needs `max_bytes`.
/// `filename` (`optional`, `required` or `forbidden`) is valid on file parts
/// only. A download's options block is optional and only affects the OpenAPI
/// document; the handler returns `ras_file_core::DownloadResponse`.
///
/// For `service_name: DocumentService` the macro emits `DocumentServiceTrait`,
/// `DocumentServiceBuilder`, `DocumentServiceClient`, and per-endpoint path
/// structs, part enums and multipart builders. Method names join the path
/// segments with `_` and turn a path parameter into `by_<name>`, so
/// `download/{file_id: String}` becomes `download_by_file_id`.
#[proc_macro]
pub fn file_service(input: TokenStream) -> TokenStream {
    let definition = parse_macro_input!(input as FileServiceDefinition);

    let server_code = server::generate_server(&definition);
    let client_code = client::generate_client(&definition);

    // Generate OpenAPI code if enabled
    let (openapi_code, schema_checks) = if let Some(openapi_config) = &definition.openapi {
        (
            openapi::generate_openapi_code(&definition, openapi_config),
            openapi::generate_schema_impl_checks(&definition),
        )
    } else {
        (quote! {}, quote! {})
    };

    let service_name_lower = definition.service_name.to_string().to_lowercase();
    let server_mod = format_ident!("__ras_file_{}_server", service_name_lower);
    let openapi_mod = format_ident!("__ras_file_{}_openapi", service_name_lower);
    let client_mod = format_ident!("__ras_file_{}_client", service_name_lower);
    let permissions_code = if cfg!(feature = "permissions") {
        permissions::generate_permissions_code(&definition)
    } else {
        quote! {}
    };

    // With `feature_gated: true` the generated code is wrapped in
    // `#[cfg(feature = ...)]` attributes resolved against the CONSUMER
    // crate's features, immune to workspace feature unification of the
    // macro crate's own features (which `cfg!` evaluates).
    let feature_gated = definition.feature_gated;
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

        #cfg_server
        const _: () = {
            #schema_checks
        };

        #cfg_server
        mod #openapi_mod {
            use super::*;
            #openapi_code
        }

        #cfg_server
        pub use #openapi_mod::*;
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

    let expanded = quote! {
        #permissions_code
        #server_output
        #client_output
    };

    TokenStream::from(expanded)
}
