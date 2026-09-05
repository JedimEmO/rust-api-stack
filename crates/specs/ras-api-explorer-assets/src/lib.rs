//! Embedded API explorer shared by REST and JSON-RPC service macros.

/// Self-contained explorer HTML. Replace `{EXPLORER_CONFIG_JSON}` with JSON
/// whose `<` characters are escaped to keep it inside the configuration script.
pub const TEMPLATE: &str = include_str!("template.html");
