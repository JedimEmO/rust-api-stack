# REST Backend Example

Axum backend for the REST/OpenAPI TypeScript usage sample.

## Run

From the workspace root:

```bash
cargo run -p rest-backend --locked
```

The server listens on `http://127.0.0.1:3000`.

- API base path: `http://127.0.0.1:3000/api/v1`
- API explorer: `http://127.0.0.1:3000/api/v1/docs`
- OpenAPI JSON: `http://127.0.0.1:3000/api/v1/docs/openapi.json`

The build script also writes the generated OpenAPI document to
`target/openapi/userservice.json`.

## Demo Data

The backend starts with two in-memory users:

- `1`: John Doe, role `user`
- `2`: Admin User, role `admin`

All users and tasks are kept in memory and reset when the process exits.

## Authentication

The example uses fixed demo bearer tokens:

- `validtoken`: user permission
- `admintoken`: admin and user permissions

Example:

```bash
curl -fsS http://127.0.0.1:3000/api/v1/users/1
curl -fsS http://127.0.0.1:3000/api/v1/users/1/tasks \
  -H 'authorization: Bearer validtoken'
```

## Endpoints

Public endpoints:

- `GET /api/v1/users`
- `GET /api/v1/users/{id}`
- `GET /api/v1/search/users?q={query}&limit={limit}&offset={offset}`

Admin endpoints:

- `POST /api/v1/users`
- `PUT /api/v1/users/{id}`
- `DELETE /api/v1/users/{id}`

User endpoints:

- `GET /api/v1/users/{user_id}/tasks`
- `POST /api/v1/users/{user_id}/tasks`
- `PUT /api/v1/users/{user_id}/tasks/{task_id}`
- `DELETE /api/v1/users/{user_id}/tasks/{task_id}`
- `GET /api/v1/users/{user_id}/tasks/search?completed={bool}&page={page}&per_page={per_page}`

## Checks

```bash
cargo check -p rest-backend --locked
cargo test -p rest-backend --locked
cargo clippy -p rest-backend --all-targets --all-features --locked -- -D warnings
```

The unit tests cover the demo auth provider, user CRUD/search behavior, task
CRUD/search behavior, and missing-task errors.
