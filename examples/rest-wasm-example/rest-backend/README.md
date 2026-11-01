# REST backend

The Axum server for the [REST example](../README.md).
It implements the `UserService` contract from [`rest-api`](../rest-api/README.md) with in-memory data.

## Run

From the workspace root:

```bash
cargo run -p rest-backend --locked
```

The server listens on `127.0.0.1:3000`:

- API base URL: `http://127.0.0.1:3000/api/v1`
- API explorer: `http://127.0.0.1:3000/api/v1/docs`
- OpenAPI document: `http://127.0.0.1:3000/api/v1/docs/openapi.json`

Building the crate also writes the OpenAPI document to `target/openapi/userservice.json`
and the permission manifest to `target/ras-permissions/rest-wasm-example.json`, both under this directory.
CORS allows any origin, so a frontend on another port can call it.

## Demo data

The backend starts with two users:

- `1`: John Doe, role `user`
- `2`: Admin User, role `admin`

Users and tasks live in memory and reset when the process exits.
The task endpoints check the `user` permission but not who owns the `user_id` in the path,
so any user can read and change any task list.

## Authentication

The backend accepts two fixed bearer tokens:

- `validtoken` has the `user` permission.
- `admintoken` has the `admin` and `user` permissions.

```bash
curl -fsS http://127.0.0.1:3000/api/v1/users/1
curl -fsS 'http://127.0.0.1:3000/api/v1/search/users?q=john'
curl -fsS http://127.0.0.1:3000/api/v1/users/1/tasks \
  -H 'authorization: Bearer validtoken'
curl -fsS -X POST http://127.0.0.1:3000/api/v1/users \
  -H 'authorization: Bearer admintoken' \
  -H 'content-type: application/json' \
  --data '{"name":"Ada","email":"ada@example.com"}'
```

The [example README](../README.md#endpoints) lists every endpoint.

## Checks

The unit tests cover the auth provider, user create, read, update, delete and search,
the same for tasks, and missing-task errors.

```bash
cargo check -p rest-backend --locked
cargo test -p rest-backend --locked
cargo clippy -p rest-backend --all-targets --all-features --locked -- -D warnings
```
