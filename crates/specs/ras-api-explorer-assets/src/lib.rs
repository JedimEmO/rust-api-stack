//! Embedded API explorer shared by REST and JSON-RPC service macros.

/// Self-contained explorer HTML. Replace `{EXPLORER_CONFIG_JSON}` with JSON
/// whose `<` characters are escaped to keep it inside the configuration script.
/// Source order preserves one script scope; event binding runs after all helpers.
pub const TEMPLATE: &str = concat!(
    include_str!("assets/head.html"),
    include_str!("assets/explorer.css"),
    include_str!("assets/body.html"),
    include_str!("assets/bootstrap.js"),
    include_str!("assets/storage.js"),
    include_str!("assets/schema-model.js"),
    include_str!("assets/markdown.js"),
    include_str!("assets/schema-render.js"),
    include_str!("assets/specs.js"),
    include_str!("assets/state.js"),
    include_str!("assets/navigation.js"),
    include_str!("assets/forms.js"),
    include_str!("assets/requests.js"),
    include_str!("assets/events.js"),
    include_str!("assets/tail.html"),
);
