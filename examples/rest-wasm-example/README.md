# REST API TypeScript Usage Sample

This example demonstrates the `ras-rest-macro` OpenAPI output and a minimal
TypeScript usage sample that calls a generated fetch client directly.

## Project Structure

- `rest-api/` - Shared API definitions using the `rest_service!` macro
- `rest-backend/` - Axum backend implementation with OpenAPI generation
- `typescript-example/` - Minimal TypeScript usage sample for a generated client

## Features

- Type-safe API definitions with Rust as the source of truth
- OpenAPI 3.0 document generation from the REST macro
- TypeScript usage sample for a generated fetch client with typed request bodies and responses
- Bearer-token headers for protected endpoints

## Quick Start

From the repository root:

```bash
cargo check -p rest-backend --locked
```

This writes the OpenAPI document consumed by TypeScript client generators.
`typescript-example/src/example.ts` shows how the generated fetch client is used.

To exercise the calls manually, run the backend:

```bash
cargo run -p rest-backend --locked
```

The server listens at `http://localhost:3000`, with API endpoints under
`/api/v1/*` and OpenAPI docs at `/api/v1/docs`.

## API Endpoints

### Public Endpoints

- `GET /api/v1/users` - Get all users
- `GET /api/v1/users/{id}` - Get user by ID

### Protected Endpoints

- `POST /api/v1/users` - Create user (admin only)
- `PUT /api/v1/users/{id}` - Update user (admin only)
- `DELETE /api/v1/users/{id}` - Delete user (admin only)
- `GET /api/v1/users/{user_id}/tasks` - Get user tasks
- `POST /api/v1/users/{user_id}/tasks` - Create task
- `PUT /api/v1/users/{user_id}/tasks/{task_id}` - Update task
- `DELETE /api/v1/users/{user_id}/tasks/{task_id}` - Delete task

## Authentication

The example backend uses simple mock tokens:

- `validtoken` for user permissions
- `admintoken` for admin permissions

The generated client accepts ordinary request headers:

```typescript
const response = await getUsersUserIdTasks({
  baseUrl: 'http://localhost:3000/api/v1',
  headers: {
    Authorization: 'Bearer validtoken',
  },
  path: { user_id: '123' },
});
```

See `typescript-example/src/example.ts` for public, admin, and
user-protected calls.
