# REST service with a generated TypeScript client

A `rest_service!` contract shared by an Axum backend and a TypeScript sample
that calls a client generated from the OpenAPI document.
It shows path and query parameters, per-endpoint permissions,
authored OpenAPI examples and the built-in API explorer.

- [`rest-api/`](rest-api/README.md) declares the endpoints and types.
- [`rest-backend/`](rest-backend/README.md) implements them with in-memory data.
- [`typescript-example/`](typescript-example/README.md) shows the calls a browser frontend makes.

## Run

```bash
cargo run -p rest-backend --locked
```

The server listens on `127.0.0.1:3000`:

- API base URL: `http://127.0.0.1:3000/api/v1`
- API explorer: `http://127.0.0.1:3000/api/v1/docs`
- OpenAPI document: `http://127.0.0.1:3000/api/v1/docs/openapi.json`

Building the backend also writes the OpenAPI document to
`examples/rest-wasm-example/rest-backend/target/openapi/userservice.json`.

## Endpoints

| Endpoint | Access |
|---|---|
| `GET /users`, `GET /users/{id}` | public |
| `GET /search/users?q=&limit=&offset=` | public |
| `POST /users`, `PUT /users/{id}`, `DELETE /users/{id}` | `admin` permission |
| `GET /users/{user_id}/tasks`, `POST /users/{user_id}/tasks` | `user` permission |
| `PUT /users/{user_id}/tasks/{task_id}`, `DELETE /users/{user_id}/tasks/{task_id}` | `user` permission |
| `GET /users/{user_id}/tasks/search?completed=&page=&per_page=` | `user` permission |

All paths are under `/api/v1`.
`GET /users/{id}` and `GET /search/users` carry authored examples,
which appear in the OpenAPI document and the explorer.

## Authentication

The backend accepts two fixed bearer tokens:

- `validtoken` has the `user` permission.
- `admintoken` has the `admin` and `user` permissions.

A generated TypeScript client passes the token as an ordinary header:

```typescript
const response = await getUsersUserIdTasks({
  baseUrl: 'http://localhost:3000/api/v1',
  headers: {
    Authorization: 'Bearer validtoken',
  },
  path: { user_id: '1' },
});
```
