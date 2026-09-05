Embedded API explorer assets for the REST and JSON-RPC service macros.

`TEMPLATE` is a self-contained HTML page assembled at compile time from the
standalone markup, stylesheet, and script files under `src/assets`. The macros
embed it and replace `CONFIG_PLACEHOLDER` with the service configuration as
JSON whose `<` characters are escaped. This crate has no runtime dependencies.
