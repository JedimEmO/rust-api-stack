# ras-topology-macro

The `ras_topology!` macro: declare a RAS service graph and generate a
function returning a validated `ras_topology_core::Topology`.

Compile-time checks: duplicate service/gateway ids and routes or call edges
referencing undeclared services fail the build with spanned errors; manifest
functions and permission constants are referenced by path, so renames and
removals in service API crates break the topology build immediately.
Value-dependent validation (audience uniqueness, manifest membership of edge
permissions, exposure rules) runs deterministically in the generated
`build()` call.

See the crate docs for the full syntax.
