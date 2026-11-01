# compile-fixture-edition2018

A consumer crate on Rust edition 2018 that uses `jsonrpc_service!` and `rest_service!`
with OpenRPC, OpenAPI and authored examples.
It only has to compile, in each feature set CI checks:
none, `client`, `server`, and `client` for `wasm32-unknown-unknown`.

Two things go wrong silently otherwise.
The generated document code embeds `ras-spec-components` as source,
so syntax or prelude items newer than 2018 break consumers on older editions.
And in a client-only build the JSON-RPC document functions still compile,
so `OnlySent`, a request type without `Deserialize` in that build,
proves that a method without examples adds no `Deserialize` bound.
