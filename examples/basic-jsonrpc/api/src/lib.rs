use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub enum SignInRequest {
    WithCredentials { username: String, password: String },
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub enum SignInResponse {
    Success { jwt: String },
    Failure { msg: String },
}

impl Default for SignInResponse {
    fn default() -> Self {
        Self::Success { jwt: String::new() }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub priority: TaskPriority,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct UpdateTaskRequest {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub completed: Option<bool>,
    pub priority: Option<TaskPriority>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct TaskListResponse {
    pub tasks: Vec<Task>,
    pub total: usize,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct UserProfile {
    pub username: String,
    pub email: String,
    pub permissions: Vec<String>,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct UpdateProfileRequest {
    pub email: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
pub struct DashboardStats {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub pending_tasks: usize,
    pub high_priority_tasks: usize,
}

jsonrpc_service!({
    service_name: MyService,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS([]) sign_out(()) -> (),
        WITH_PERMISSIONS(["admin"]) delete_everything(()) -> (),

        // Task management
        WITH_PERMISSIONS([]) list_tasks(()) -> TaskListResponse,
        WITH_PERMISSIONS([]) create_task(CreateTaskRequest) -> Task,
        WITH_PERMISSIONS([]) update_task(UpdateTaskRequest) -> Task,
        WITH_PERMISSIONS([]) delete_task(String) -> bool,
        WITH_PERMISSIONS([]) get_task(String) -> Option<Task>,

        // User profile
        WITH_PERMISSIONS([]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS([]) update_profile(UpdateProfileRequest) -> UserProfile,

        // Dashboard
        WITH_PERMISSIONS([]) get_dashboard_stats(()) -> DashboardStats,
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn sign_in_request_serializes_with_stable_variant_name() {
        let request = SignInRequest::WithCredentials {
            username: "alice".to_string(),
            password: "secret".to_string(),
        };

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "WithCredentials": {
                    "username": "alice",
                    "password": "secret"
                }
            })
        );
    }

    #[test]
    fn default_sign_in_response_is_empty_success_token() {
        assert_eq!(
            serde_json::to_value(SignInResponse::default()).unwrap(),
            json!({ "Success": { "jwt": "" } })
        );
    }

    #[test]
    fn create_task_request_serializes_priority_wire_value() {
        let request = CreateTaskRequest {
            title: "Ship the example".to_string(),
            description: "Exercise the generated client".to_string(),
            priority: TaskPriority::High,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "title": "Ship the example",
                "description": "Exercise the generated client",
                "priority": "High"
            })
        );
    }

    #[test]
    fn update_task_request_preserves_partial_update_nulls() {
        let request = UpdateTaskRequest {
            id: "task-1".to_string(),
            title: None,
            description: Some("Updated through JSON-RPC".to_string()),
            completed: Some(true),
            priority: None,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "id": "task-1",
                "title": null,
                "description": "Updated through JSON-RPC",
                "completed": true,
                "priority": null
            })
        );
    }

    #[test]
    fn task_list_response_preserves_total_and_task_wire_shape() {
        let response = TaskListResponse {
            total: 1,
            tasks: vec![Task {
                id: "task-1".to_string(),
                title: "Document generated client".to_string(),
                description: "Keep the example minimal".to_string(),
                completed: false,
                priority: TaskPriority::Medium,
                created_at: "2026-05-23T12:00:00Z".to_string(),
                updated_at: "2026-05-23T12:30:00Z".to_string(),
            }],
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "tasks": [{
                    "id": "task-1",
                    "title": "Document generated client",
                    "description": "Keep the example minimal",
                    "completed": false,
                    "priority": "Medium",
                    "created_at": "2026-05-23T12:00:00Z",
                    "updated_at": "2026-05-23T12:30:00Z"
                }],
                "total": 1
            })
        );
    }

    #[test]
    fn dashboard_stats_serializes_counter_fields() {
        let stats = DashboardStats {
            total_tasks: 4,
            completed_tasks: 1,
            pending_tasks: 3,
            high_priority_tasks: 2,
        };

        assert_eq!(
            serde_json::to_value(stats).unwrap(),
            json!({
                "total_tasks": 4,
                "completed_tasks": 1,
                "pending_tasks": 3,
                "high_priority_tasks": 2
            })
        );
    }

    #[test]
    fn generated_openrpc_documents_example_methods_and_permissions() {
        let doc = generate_myservice_openrpc();

        assert_eq!(doc["openrpc"], "1.3.2");
        assert_eq!(doc["info"]["title"], "MyService JSON-RPC API");

        let methods = doc["methods"].as_array().expect("methods array");
        let method_names = methods
            .iter()
            .map(|method| method["name"].as_str().expect("method name"))
            .collect::<BTreeSet<_>>();

        assert_eq!(
            method_names,
            BTreeSet::from([
                "create_task",
                "delete_everything",
                "delete_task",
                "get_dashboard_stats",
                "get_profile",
                "get_task",
                "list_tasks",
                "sign_in",
                "sign_out",
                "update_profile",
                "update_task",
            ])
        );

        let sign_in = methods
            .iter()
            .find(|method| method["name"] == "sign_in")
            .expect("sign_in method");
        assert!(sign_in.get("x-authentication").is_none());

        let delete_everything = methods
            .iter()
            .find(|method| method["name"] == "delete_everything")
            .expect("delete_everything method");
        assert_eq!(
            delete_everything["x-authentication"]["required"].as_bool(),
            Some(true)
        );
        assert_eq!(delete_everything["x-permissions"], json!(["admin"]));
    }
}
