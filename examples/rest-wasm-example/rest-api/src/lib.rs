use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateUserRequest {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsersResponse {
    pub users: Vec<User>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TasksResponse {
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub completed: Option<bool>,
}

rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    endpoints: [
        // Public endpoints
        GET UNAUTHORIZED users() -> UsersResponse,
        GET UNAUTHORIZED users/{id: String}() -> User,

        // Admin endpoints
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
        PUT WITH_PERMISSIONS(["admin"]) users/{id: String}(UpdateUserRequest) -> User,
        DELETE WITH_PERMISSIONS(["admin"]) users/{id: String}() -> (),

        // User endpoints for tasks
        GET WITH_PERMISSIONS(["user"]) users/{user_id: String}/tasks() -> TasksResponse,
        POST WITH_PERMISSIONS(["user"]) users/{user_id: String}/tasks(CreateTaskRequest) -> Task,
        PUT WITH_PERMISSIONS(["user"]) users/{user_id: String}/tasks/{task_id: String}(UpdateTaskRequest) -> Task,
        DELETE WITH_PERMISSIONS(["user"]) users/{user_id: String}/tasks/{task_id: String}() -> (),

        // New endpoints with query parameters for pagination and search
        GET UNAUTHORIZED search/users ? q: String & limit: Option<u32> & offset: Option<u32> () -> UsersResponse,
        GET WITH_PERMISSIONS(["user"]) users/{user_id: String}/tasks/search ? completed: Option<bool> & page: Option<u32> & per_page: Option<u32> () -> TasksResponse,
    ]
});

// The TypeScript usage sample generates its fetch client from this OpenAPI document.

#[cfg(test)]
mod dto_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_task_request_serializes_stable_json_shape() {
        let request = CreateTaskRequest {
            title: "Review OpenAPI example".to_string(),
            description: "Keep the generated client sample minimal".to_string(),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "title": "Review OpenAPI example",
                "description": "Keep the generated client sample minimal"
            })
        );
    }

    #[test]
    fn update_task_request_preserves_partial_update_nulls() {
        let request = UpdateTaskRequest {
            title: None,
            description: Some("Updated from generated client".to_string()),
            completed: Some(true),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "title": null,
                "description": "Updated from generated client",
                "completed": true
            })
        );
    }

    #[test]
    fn update_user_request_preserves_partial_update_nulls() {
        let request = UpdateUserRequest {
            name: Some("Alice Updated".to_string()),
            email: None,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "name": "Alice Updated",
                "email": null
            })
        );
    }

    #[test]
    fn tasks_response_serializes_nested_tasks() {
        let response = TasksResponse {
            tasks: vec![Task {
                id: "task-1".to_string(),
                title: "Generate client".to_string(),
                description: "Use the OpenAPI TypeScript sample".to_string(),
                completed: true,
                user_id: "user-1".to_string(),
            }],
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "tasks": [{
                    "id": "task-1",
                    "title": "Generate client",
                    "description": "Use the OpenAPI TypeScript sample",
                    "completed": true,
                    "user_id": "user-1"
                }]
            })
        );
    }

    #[test]
    fn users_response_serializes_total_alongside_items() {
        let response = UsersResponse {
            users: vec![User {
                id: "user-1".to_string(),
                name: "Alice".to_string(),
                email: "alice@example.test".to_string(),
                role: "admin".to_string(),
            }],
            total: 1,
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "users": [{
                    "id": "user-1",
                    "name": "Alice",
                    "email": "alice@example.test",
                    "role": "admin"
                }],
                "total": 1
            })
        );
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use serde_json::Value;

    fn parameter<'a>(operation: &'a Value, name: &str) -> &'a Value {
        operation["parameters"]
            .as_array()
            .expect("parameters array")
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap_or_else(|| panic!("missing parameter {name}"))
    }

    #[test]
    fn generated_openapi_documents_public_and_protected_routes() {
        let doc = generate_userservice_openapi();

        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "UserService REST API");

        assert!(doc["paths"]["/users"]["get"].is_object());
        assert!(doc["paths"]["/users/{id}"]["get"].is_object());

        let create_user = &doc["paths"]["/users"]["post"];
        assert_eq!(
            create_user["security"][0]["bearerAuth"],
            serde_json::json!([])
        );
        assert_eq!(create_user["x-permissions"], serde_json::json!(["admin"]));

        let create_task = &doc["paths"]["/users/{user_id}/tasks"]["post"];
        assert_eq!(
            create_task["security"][0]["bearerAuth"],
            serde_json::json!([])
        );
        assert_eq!(create_task["x-permissions"], serde_json::json!(["user"]));
        assert_eq!(
            parameter(create_task, "user_id")["in"],
            serde_json::json!("path")
        );
        assert_eq!(
            parameter(create_task, "user_id")["required"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn generated_openapi_marks_query_parameter_requiredness() {
        let doc = generate_userservice_openapi();

        let user_search = &doc["paths"]["/search/users"]["get"];
        assert_eq!(
            parameter(user_search, "q")["in"],
            serde_json::json!("query")
        );
        assert_eq!(
            parameter(user_search, "q")["required"],
            serde_json::json!(true)
        );
        assert_eq!(
            parameter(user_search, "limit")["required"],
            serde_json::json!(false)
        );
        assert_eq!(
            parameter(user_search, "offset")["required"],
            serde_json::json!(false)
        );

        let task_search = &doc["paths"]["/users/{user_id}/tasks/search"]["get"];
        assert_eq!(
            parameter(task_search, "user_id")["in"],
            serde_json::json!("path")
        );
        assert_eq!(
            parameter(task_search, "completed")["required"],
            serde_json::json!(false)
        );
        assert_eq!(
            parameter(task_search, "page")["required"],
            serde_json::json!(false)
        );
        assert_eq!(
            parameter(task_search, "per_page")["required"],
            serde_json::json!(false)
        );
    }
}
