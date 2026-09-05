//! Embedded API explorer shared by REST and JSON-RPC service macros.

/// Placeholder in [`TEMPLATE`] that consumers replace with the explorer
/// configuration JSON. The JSON must have its `<` characters escaped
/// (for example as `\u003c`) so it cannot terminate the configuration script.
pub const CONFIG_PLACEHOLDER: &str = "{EXPLORER_CONFIG_JSON}";

/// Self-contained explorer HTML with a single [`CONFIG_PLACEHOLDER`].
///
/// Each asset file is a standalone document fragment: `head.html`, `body.html`
/// and `tail.html` are markup, `explorer.css` is a stylesheet, and the `*.js`
/// files are scripts. The wrapper tags live here so the fragments stay valid on
/// their own and editors can normalize their whitespace freely. Script order
/// matters: the files share one scope, and `events.js` binds handlers to
/// functions defined by the earlier files.
pub const TEMPLATE: &str = concat!(
    include_str!("assets/head.html"),
    "    <style>\n",
    include_str!("assets/explorer.css"),
    "    </style>\n",
    include_str!("assets/body.html"),
    "    <script>\n",
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
    "    </script>\n",
    include_str!("assets/tail.html"),
);

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPTS: [&str; 11] = [
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
    ];

    #[test]
    fn placeholder_appears_exactly_once_inside_the_config_script() {
        assert_eq!(TEMPLATE.matches(CONFIG_PLACEHOLDER).count(), 1);
        let expected = format!(
            "<script id=\"ras-explorer-config\" type=\"application/json\">{CONFIG_PLACEHOLDER}</script>"
        );
        assert!(TEMPLATE.contains(&expected));
    }

    #[test]
    fn template_is_a_complete_html_document() {
        assert!(TEMPLATE.starts_with("<!DOCTYPE html>\n"));
        assert!(TEMPLATE.ends_with("</html>\n"));
        for (open, close) in [
            ("<html", "</html>"),
            ("<head>", "</head>"),
            ("<body>", "</body>"),
        ] {
            assert_eq!(TEMPLATE.matches(open).count(), 1, "{open}");
            assert_eq!(TEMPLATE.matches(close).count(), 1, "{close}");
        }
        assert_eq!(TEMPLATE.matches("<style>").count(), 1);
        assert_eq!(TEMPLATE.matches("</style>").count(), 1);
        // The configuration script plus the single explorer script.
        assert_eq!(TEMPLATE.matches("<script").count(), 2);
        assert_eq!(TEMPLATE.matches("</script>").count(), 2);
    }

    #[test]
    fn stylesheet_and_scripts_cannot_break_out_of_their_wrapper_tags() {
        let css = include_str!("assets/explorer.css");
        assert!(
            !css.contains("</style"),
            "explorer.css must not close its wrapper"
        );
        assert!(
            !css.contains("<script"),
            "explorer.css must not open a script"
        );
        for (index, script) in SCRIPTS.iter().enumerate() {
            assert!(
                !script.contains("</script"),
                "script {index} must not close its wrapper"
            );
            assert!(
                script.ends_with('\n'),
                "script {index} must end with a newline"
            );
        }
    }

    #[test]
    fn event_binding_runs_after_the_helpers_it_uses() {
        // Scripts share one scope; the last file wires events to functions
        // defined earlier. Keep the ordering visible to reviewers.
        let joined = TEMPLATE;
        let defines = joined
            .find("function renderOperations(")
            .expect("navigation helper");
        let binds = joined
            .find("addEventListener(\"DOMContentLoaded\"")
            .expect("bootstrap binding");
        assert!(defines < binds);
    }
}
