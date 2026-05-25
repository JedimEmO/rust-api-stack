use axum::Router;
use basic_jsonrpc_api::{
    CreateTaskRequest, DashboardStats, MyServiceBuilder, MyServiceTrait, SignInRequest,
    SignInResponse, Task, TaskListResponse, TaskPriority, UpdateProfileRequest, UpdateTaskRequest,
    UserProfile,
};
use chrono::Utc;
use ras_jsonrpc_core::{AuthFuture, AuthProvider, AuthenticatedUser};
use ras_observability_core::{MethodDurationTracker, RequestContext, UsageTracker};
use ras_observability_otel::OtelSetupBuilder;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tracing::info;
use uuid::Uuid;

// Example auth provider
pub struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            // Simple example - in real implementation, validate JWT
            if token == "valid_token" {
                let mut permissions = HashSet::new();
                permissions.insert("user".to_string());

                Ok(AuthenticatedUser {
                    user_id: "user123".to_string(),
                    permissions,
                    metadata: None,
                })
            } else if token == "admin_token" {
                let mut permissions = HashSet::new();
                permissions.insert("user".to_string());
                permissions.insert("admin".to_string());

                Ok(AuthenticatedUser {
                    user_id: "admin123".to_string(),
                    permissions,
                    metadata: None,
                })
            } else {
                Err(ras_jsonrpc_core::AuthError::InvalidToken)
            }
        })
    }
}

// Simple in-memory task storage
#[derive(Clone)]
struct TaskStorage {
    tasks: Arc<Mutex<HashMap<String, Task>>>,
}

impl TaskStorage {
    fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn create_task(&self, req: CreateTaskRequest) -> Task {
        let task = Task {
            id: Uuid::new_v4().to_string(),
            title: req.title,
            description: req.description,
            completed: false,
            priority: req.priority,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };

        self.tasks
            .lock()
            .unwrap()
            .insert(task.id.clone(), task.clone());
        task
    }

    fn update_task(&self, req: UpdateTaskRequest) -> Option<Task> {
        let mut tasks = self.tasks.lock().unwrap();

        tasks.get_mut(&req.id).map(|task| {
            if let Some(title) = req.title {
                task.title = title;
            }
            if let Some(description) = req.description {
                task.description = description;
            }
            if let Some(completed) = req.completed {
                task.completed = completed;
            }
            if let Some(priority) = req.priority {
                task.priority = priority;
            }
            task.updated_at = Utc::now().to_rfc3339();
            task.clone()
        })
    }

    fn delete_task(&self, id: String) -> bool {
        self.tasks.lock().unwrap().remove(&id).is_some()
    }

    fn get_task(&self, id: String) -> Option<Task> {
        self.tasks.lock().unwrap().get(&id).cloned()
    }

    fn list_tasks(&self) -> TaskListResponse {
        let tasks = self.tasks.lock().unwrap();
        let task_vec: Vec<Task> = tasks.values().cloned().collect();
        let total = task_vec.len();

        TaskListResponse {
            tasks: task_vec,
            total,
        }
    }

    fn get_stats(&self) -> DashboardStats {
        let tasks = self.tasks.lock().unwrap();
        let total_tasks = tasks.len();
        let completed_tasks = tasks.values().filter(|t| t.completed).count();
        let pending_tasks = total_tasks - completed_tasks;
        let high_priority_tasks = tasks
            .values()
            .filter(|t| matches!(t.priority, TaskPriority::High))
            .count();

        DashboardStats {
            total_tasks,
            completed_tasks,
            pending_tasks,
            high_priority_tasks,
        }
    }
}

struct MyServiceImpl {
    storage: Arc<TaskStorage>,
}

impl MyServiceTrait for MyServiceImpl {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        println!("{request:?}");
        match request {
            SignInRequest::WithCredentials { username, password } => {
                if username == "admin" && password == "secret" {
                    Ok(SignInResponse::Success {
                        jwt: "admin_token".to_string(),
                    })
                } else if username == "user" && password == "password" {
                    Ok(SignInResponse::Success {
                        jwt: "valid_token".to_string(),
                    })
                } else {
                    Ok(SignInResponse::Failure {
                        msg: "Invalid credentials".to_string(),
                    })
                }
            }
        }
    }

    async fn sign_out(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("User {} signed out", user.user_id);
        Ok(())
    }

    async fn delete_everything(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::warn!("Admin {} is deleting everything!", user.user_id);
        Ok(())
    }

    async fn list_tasks(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> Result<TaskListResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.storage.list_tasks())
    }

    async fn create_task(
        &self,
        _user: &AuthenticatedUser,
        request: CreateTaskRequest,
    ) -> Result<Task, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.storage.create_task(request))
    }

    async fn update_task(
        &self,
        _user: &AuthenticatedUser,
        request: UpdateTaskRequest,
    ) -> Result<Task, Box<dyn std::error::Error + Send + Sync>> {
        self.storage.update_task(request).ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Task not found",
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn delete_task(
        &self,
        _user: &AuthenticatedUser,
        id: String,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.storage.delete_task(id))
    }

    async fn get_task(
        &self,
        _user: &AuthenticatedUser,
        id: String,
    ) -> Result<Option<Task>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.storage.get_task(id))
    }

    async fn get_profile(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserProfile {
            username: if user.user_id == "admin123" {
                "admin"
            } else {
                "user"
            }
            .to_string(),
            email: format!("{}@example.com", user.user_id),
            permissions: user.permissions.iter().cloned().collect(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
    }

    async fn update_profile(
        &self,
        user: &AuthenticatedUser,
        request: UpdateProfileRequest,
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserProfile {
            username: if user.user_id == "admin123" {
                "admin"
            } else {
                "user"
            }
            .to_string(),
            email: request
                .email
                .unwrap_or_else(|| format!("{}@example.com", user.user_id)),
            permissions: user.permissions.iter().cloned().collect(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
    }

    async fn get_dashboard_stats(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> Result<DashboardStats, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.storage.get_stats())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Initialize observability with the new crates
    info!("Initializing OpenTelemetry with unified observability...");
    let otel = OtelSetupBuilder::new("basic-jsonrpc-service")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to set up OpenTelemetry: {e}"))?;

    // Note about OTLP: For OTLP export, you would typically run this service
    // alongside an OpenTelemetry Collector that scrapes the /metrics endpoint
    // and forwards to your OTLP backend
    let otlp_note = std::env::var("OTLP_ENDPOINT")
        .map(|endpoint| format!("Configure your OpenTelemetry Collector to scrape metrics from http://localhost:3000/metrics and forward to {}", endpoint))
        .unwrap_or_else(|_| "To use OTLP, run an OpenTelemetry Collector that scrapes http://localhost:3000/metrics".to_string());

    // Initialize task storage
    let task_storage = Arc::new(TaskStorage::new());

    let rpc_router = MyServiceBuilder::new(MyServiceImpl {
        storage: task_storage.clone(),
    })
    .base_url("/rpc")
    .with_usage_tracker({
        let usage_tracker = otel.usage_tracker();
        move |headers, user, payload| {
            let method = payload.method.clone();
            let context = RequestContext::jsonrpc(method);
            let usage_tracker = usage_tracker.clone();
            let headers_clone = headers.clone();
            let user_clone = user.cloned();

            async move {
                // Log the request
                match &user_clone {
                    Some(u) => {
                        info!(
                            "RPC call: method={}, user={}, permissions={:?}",
                            context.method, u.user_id, u.permissions,
                        );
                    }
                    None => {
                        info!("RPC call: method={}, user=anonymous", context.method,);
                    }
                }

                // Track the request
                usage_tracker
                    .track_request(&headers_clone, user_clone.as_ref(), &context)
                    .await;
            }
        }
    })
    .with_method_duration_tracker({
        let duration_tracker = otel.method_duration_tracker();
        move |method: &str,
              user: Option<&ras_jsonrpc_core::AuthenticatedUser>,
              duration: std::time::Duration| {
            let context = RequestContext::jsonrpc(method.to_string());
            let duration_tracker = duration_tracker.clone();
            let user_clone = user.cloned();

            async move {
                duration_tracker
                    .track_duration(&context, user_clone.as_ref(), duration)
                    .await;
            }
        }
    })
    .auth_provider(MyAuthProvider)
    .build()
    .map_err(|e| anyhow::anyhow!("Failed to build JSON-RPC router: {e}"))?;

    // Create the main app with metrics endpoint
    let app = Router::new().merge(rpc_router).merge(otel.metrics_router());

    println!("Basic JSON-RPC Service");
    println!("===================");
    println!();
    println!("JSON-RPC endpoint: http://localhost:3000/rpc");
    println!("API Explorer: http://localhost:3000/rpc/explorer");
    println!("OpenRPC spec: http://localhost:3000/rpc/explorer/openrpc.json");
    println!();
    println!("Test credentials:");
    println!("  Admin: username='admin', password='secret'");
    println!("  User:  username='user', password='password'");
    println!();
    println!("Metrics available at: http://localhost:3000/metrics");
    println!();
    println!("{}", otlp_note);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> MyServiceImpl {
        MyServiceImpl {
            storage: Arc::new(TaskStorage::new()),
        }
    }

    fn auth_user(user_id: &str, permissions: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_string(),
            permissions: permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect(),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn auth_provider_maps_user_and_admin_tokens() {
        let provider = MyAuthProvider;

        let user = provider
            .authenticate("valid_token".to_string())
            .await
            .expect("valid user token");
        assert_eq!(user.user_id, "user123");
        assert!(user.permissions.contains("user"));
        assert!(!user.permissions.contains("admin"));

        let admin = provider
            .authenticate("admin_token".to_string())
            .await
            .expect("valid admin token");
        assert_eq!(admin.user_id, "admin123");
        assert!(admin.permissions.contains("user"));
        assert!(admin.permissions.contains("admin"));
    }

    #[tokio::test]
    async fn auth_provider_rejects_unknown_tokens() {
        let provider = MyAuthProvider;

        let error = provider
            .authenticate("bad_token".to_string())
            .await
            .expect_err("unknown token should be rejected");

        assert!(matches!(error, ras_jsonrpc_core::AuthError::InvalidToken));
    }

    #[tokio::test]
    async fn sign_in_returns_documented_demo_tokens() {
        let service = service();

        let admin = service
            .sign_in(SignInRequest::WithCredentials {
                username: "admin".to_string(),
                password: "secret".to_string(),
            })
            .await
            .expect("admin sign in");
        assert!(matches!(
            admin,
            SignInResponse::Success { ref jwt } if jwt == "admin_token"
        ));

        let user = service
            .sign_in(SignInRequest::WithCredentials {
                username: "user".to_string(),
                password: "password".to_string(),
            })
            .await
            .expect("user sign in");
        assert!(matches!(
            user,
            SignInResponse::Success { ref jwt } if jwt == "valid_token"
        ));

        let failure = service
            .sign_in(SignInRequest::WithCredentials {
                username: "user".to_string(),
                password: "wrong".to_string(),
            })
            .await
            .expect("failed sign in response");
        assert!(matches!(
            failure,
            SignInResponse::Failure { ref msg } if msg == "Invalid credentials"
        ));
    }

    #[tokio::test]
    async fn task_lifecycle_updates_dashboard_stats() {
        let service = service();
        let user = auth_user("user123", &["user"]);

        let high = service
            .create_task(
                &user,
                CreateTaskRequest {
                    title: "Write docs".to_string(),
                    description: "Document the JSON-RPC example".to_string(),
                    priority: TaskPriority::High,
                },
            )
            .await
            .expect("create high priority task");
        let low = service
            .create_task(
                &user,
                CreateTaskRequest {
                    title: "Tidy examples".to_string(),
                    description: "Remove misleading snippets".to_string(),
                    priority: TaskPriority::Low,
                },
            )
            .await
            .expect("create low priority task");

        let updated = service
            .update_task(
                &user,
                UpdateTaskRequest {
                    id: high.id.clone(),
                    title: Some("Write verified docs".to_string()),
                    description: None,
                    completed: Some(true),
                    priority: Some(TaskPriority::Medium),
                },
            )
            .await
            .expect("update task");
        assert_eq!(updated.title, "Write verified docs");
        assert!(updated.completed);
        assert!(matches!(updated.priority, TaskPriority::Medium));

        let stats = service
            .get_dashboard_stats(&user, ())
            .await
            .expect("dashboard stats");
        assert_eq!(stats.total_tasks, 2);
        assert_eq!(stats.completed_tasks, 1);
        assert_eq!(stats.pending_tasks, 1);
        assert_eq!(stats.high_priority_tasks, 0);

        let list = service.list_tasks(&user, ()).await.expect("list tasks");
        assert_eq!(list.total, 2);

        assert!(
            service
                .delete_task(&user, low.id)
                .await
                .expect("delete task")
        );
        let after_delete = service
            .get_dashboard_stats(&user, ())
            .await
            .expect("stats after delete");
        assert_eq!(after_delete.total_tasks, 1);
    }

    #[tokio::test]
    async fn updating_missing_task_returns_not_found_error() {
        let service = service();
        let user = auth_user("user123", &["user"]);

        let error = service
            .update_task(
                &user,
                UpdateTaskRequest {
                    id: "missing".to_string(),
                    title: Some("Missing".to_string()),
                    description: None,
                    completed: None,
                    priority: None,
                },
            )
            .await
            .expect_err("missing task should be rejected");

        assert_eq!(error.to_string(), "Task not found");
    }

    #[tokio::test]
    async fn missing_task_lookup_and_delete_are_non_error_absences() {
        let service = service();
        let user = auth_user("user123", &["user"]);

        let task = service
            .get_task(&user, "missing".to_string())
            .await
            .expect("missing get should be a successful absence");
        assert!(task.is_none());

        let deleted = service
            .delete_task(&user, "missing".to_string())
            .await
            .expect("missing delete should report false");
        assert!(!deleted);
    }

    #[tokio::test]
    async fn profile_methods_use_authenticated_user_and_requested_email() {
        let service = service();
        let admin = auth_user("admin123", &["admin", "user"]);

        let profile = service
            .get_profile(&admin, ())
            .await
            .expect("profile response");
        assert_eq!(profile.username, "admin");
        assert_eq!(profile.email, "admin123@example.com");
        assert!(profile.permissions.contains(&"admin".to_string()));

        let updated = service
            .update_profile(
                &admin,
                UpdateProfileRequest {
                    email: Some("admin@example.test".to_string()),
                },
            )
            .await
            .expect("profile update");
        assert_eq!(updated.email, "admin@example.test");
    }
}
