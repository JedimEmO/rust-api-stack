# ras-api-explorer-assets

The API explorer page that `ras-rest-macro` and `ras-jsonrpc-macro` serve next to a generated service.
The page reads the service's OpenAPI or OpenRPC document, lists its operations,
and lets you send requests from the browser.
You do not add this crate yourself; the macros depend on it,
and you turn the explorer on in the service definition
(`serve_docs: true` for `rest_service!`, `explorer: true` for `jsonrpc_service!`).

The crate holds the page's markup, stylesheet and scripts as files under `src/assets`,
and exposes two constants:

- `TEMPLATE` is the whole page as one HTML string, assembled at compile time with `include_str!`.
  It loads nothing from other servers.
- `CONFIG_PLACEHOLDER` marks the spot in `TEMPLATE` where the macros put the page's configuration:
  the service name, the protocol, the path of the spec document and the API base path, as JSON.
  The JSON must have every `<` escaped (as `<`) so it cannot close the `<script>` element it sits in.

The macros embed `TEMPLATE` in the generated code as a string literal,
so the page ships inside your binary and needs no files at run time.
The crate has no dependencies and no Cargo features.
