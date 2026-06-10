# authorization-demo

End-to-end demo of the RAS authorization extension (#12–#15): two generated
RAS REST services, an embedded authority, the auth gateway, and a topology
declaration wiring them together.

```text
browser ── ras_web_session ──> gateway ──/api/invoice──> invoice-service
                                  │
                                  └──────/api/billing──> billing-service
                                                             │ RAS internal token
                                                             ▼ (embedded authority)
                                                         invoice-service
```

What it demonstrates:

- `ras_topology!` declares the service graph; the generated policy artifact
  constrains the authority's token issuance and the generated gateway
  profile configures the gateway's routes (upstream bindings provided by the
  deployment).
- The embedded authority registers services from the topology, imports their
  generated permission manifests, and grants `billing -> invoice-service:
  invoice:read`.
- Billing serves `/api/billing/summary` by acquiring a RAS internal token
  through `TokenManager` + `RasInternalTokenSource` (embedded mode) and
  calling the generated `InvoiceServiceClient` with it.
- The gateway validates web sessions locally and narrows them to
  single-audience `ras_gateway_access` tokens; the invoice service accepts
  both internal and gateway tokens through `RasTokenAuthProvider`s composed
  with a small `MultiTokenAuthProvider`.
- Generated `WITH_PERMISSIONS` enforcement still applies per operation: a
  read-only session passes `GET /invoices` and is rejected on
  `POST /invoices`.

Run the binary (`cargo run -p authorization-demo`) to serve the stack on
localhost with a printed demo session token, or see `tests/e2e.rs` for the
full in-process flow, including the fail-closed paths (missing audience
permissions, direct backend access with a session token, undeclared
topology edges).
