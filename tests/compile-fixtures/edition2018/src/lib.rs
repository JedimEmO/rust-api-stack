//! Compile fixture: both service macros, with examples, in an edition-2018 crate.

use ras_jsonrpc_macro::jsonrpc_service;
use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    pub text: String,
}

/// Only ever sent by clients: a client-only build gives it no `Deserialize`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[cfg_attr(feature = "server", derive(Deserialize))]
pub struct OnlySent {
    pub text: String,
}

fn note(text: &str) -> Note {
    Note {
        text: text.to_string(),
    }
}

jsonrpc_service!({
    service_name: Notes,
    openrpc: true,
    explorer: true,
    feature_gated: true,
    methods: [
        /// Save a note.
        UNAUTHORIZED save(Note) -> Note {
            examples: [
                /// A short note.
                short { params: note("a"), result: note("a") },
                empty { params: note(""), error: { code: 1001, message: "A note needs text" } },
            ],
        },
        UNAUTHORIZED send(OnlySent) -> (),
    ]
});

rest_service!({
    service_name: NotesRest,
    base_path: "/api",
    openapi: true,
    serve_docs: true,
    feature_gated: true,
    endpoints: [
        /// Replace a note.
        PUT UNAUTHORIZED notes/{id: u32} ? tag: Option<String> (Note) -> Note {
            examples: [
                tagged { path: { id: 1 }, query: { tag: Some("work".to_string()) }, body: note("a"), response: note("a") },
                missing { path: { id: 404 }, body: note("a"), error: { status: 404, message: "Note not found" } },
            ],
        },
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrpc_document_builds() {
        let doc = try_generate_notes_openrpc().expect("examples serialize");
        assert_eq!(doc["methods"][0]["examples"][0]["name"], "short");
    }

    #[cfg(feature = "server")]
    #[test]
    fn openapi_document_builds() {
        let doc = try_generate_notesrest_openapi().expect("examples serialize");
        assert!(doc["paths"]["/notes/{id}"]["put"].is_object());
    }
}
