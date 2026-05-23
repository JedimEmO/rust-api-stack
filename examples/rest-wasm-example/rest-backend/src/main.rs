mod simple_auth;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestError, RestResponse, RestResult};

use rest_api::*;
use simple_auth::SimpleAuthProvider;

// Simple in-memory storage
#[derive(Clone)]
struct AppState {
    users: Arc<Mutex<HashMap<String, User>>>,
    tasks: Arc<Mutex<HashMap<String, Vec<Task>>>>,
}

struct UserServiceImpl {
    state: AppState,
}

#[async_trait::async_trait]
impl UserServiceTrait for UserServiceImpl {
    async fn get_users(&self) -> RestResult<UsersResponse> {
        let users = self.state.users.lock().unwrap();
        let users_vec: Vec<User> = users.values().cloned().collect();
        let total = users_vec.len();

        Ok(RestResponse::ok(UsersResponse {
            users: users_vec,
            total,
        }))
    }

    async fn get_users_by_id(&self, id: String) -> RestResult<User> {
        let users = self.state.users.lock().unwrap();

        users
            .get(&id)
            .cloned()
            .map(RestResponse::ok)
            .ok_or_else(|| RestError::not_found("User not found"))
    }

    async fn post_users(
        &self,
        _user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> RestResult<User> {
        let mut users = self.state.users.lock().unwrap();

        let user = User {
            id: Uuid::new_v4().to_string(),
            name: request.name,
            email: request.email,
            role: "user".to_string(),
        };

        users.insert(user.id.clone(), user.clone());

        Ok(RestResponse::created(user))
    }

    async fn put_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        id: String,
        request: UpdateUserRequest,
    ) -> RestResult<User> {
        let mut users = self.state.users.lock().unwrap();

        let user = users
            .get_mut(&id)
            .ok_or_else(|| RestError::not_found("User not found"))?;

        if let Some(name) = request.name {
            user.name = name;
        }
        if let Some(email) = request.email {
            user.email = email;
        }

        Ok(RestResponse::ok(user.clone()))
    }

    async fn delete_users_by_id(&self, _user: &AuthenticatedUser, id: String) -> RestResult<()> {
        let mut users = self.state.users.lock().unwrap();

        users
            .remove(&id)
            .map(|_| RestResponse::ok(()))
            .ok_or_else(|| RestError::not_found("User not found"))
    }

    async fn get_users_by_user_id_tasks(
        &self,
        _user: &AuthenticatedUser,
        user_id: String,
    ) -> RestResult<TasksResponse> {
        let tasks = self.state.tasks.lock().unwrap();

        let user_tasks = tasks.get(&user_id).cloned().unwrap_or_default();

        Ok(RestResponse::ok(TasksResponse { tasks: user_tasks }))
    }

    async fn post_users_by_user_id_tasks(
        &self,
        _user: &AuthenticatedUser,
        user_id: String,
        request: CreateTaskRequest,
    ) -> RestResult<Task> {
        let mut tasks = self.state.tasks.lock().unwrap();

        let task = Task {
            id: Uuid::new_v4().to_string(),
            title: request.title,
            description: request.description,
            completed: false,
            user_id: user_id.clone(),
        };

        tasks.entry(user_id).or_default().push(task.clone());

        Ok(RestResponse::created(task))
    }

    async fn put_users_by_user_id_tasks_by_task_id(
        &self,
        _user: &AuthenticatedUser,
        user_id: String,
        task_id: String,
        request: UpdateTaskRequest,
    ) -> RestResult<Task> {
        let mut tasks = self.state.tasks.lock().unwrap();

        let user_tasks = tasks
            .get_mut(&user_id)
            .ok_or_else(|| RestError::not_found("User tasks not found"))?;

        let task = user_tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| RestError::not_found("Task not found"))?;

        if let Some(title) = request.title {
            task.title = title;
        }
        if let Some(description) = request.description {
            task.description = description;
        }
        if let Some(completed) = request.completed {
            task.completed = completed;
        }

        Ok(RestResponse::ok(task.clone()))
    }

    async fn delete_users_by_user_id_tasks_by_task_id(
        &self,
        _user: &AuthenticatedUser,
        user_id: String,
        task_id: String,
    ) -> RestResult<()> {
        let mut tasks = self.state.tasks.lock().unwrap();

        let user_tasks = tasks
            .get_mut(&user_id)
            .ok_or_else(|| RestError::not_found("User tasks not found"))?;

        let pos = user_tasks
            .iter()
            .position(|t| t.id == task_id)
            .ok_or_else(|| RestError::not_found("Task not found"))?;

        user_tasks.remove(pos);

        Ok(RestResponse::ok(()))
    }

    async fn get_search_users(
        &self,
        q: String,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> RestResult<UsersResponse> {
        let users = self.state.users.lock().unwrap();
        let limit = limit.unwrap_or(10) as usize;
        let offset = offset.unwrap_or(0) as usize;

        // Filter users by search query
        let filtered: Vec<User> = users
            .values()
            .filter(|u| {
                u.name.to_lowercase().contains(&q.to_lowercase())
                    || u.email.to_lowercase().contains(&q.to_lowercase())
            })
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        Ok(RestResponse::ok(UsersResponse {
            users: filtered,
            total: users.len(),
        }))
    }

    async fn get_users_by_user_id_tasks_search(
        &self,
        _user: &AuthenticatedUser,
        user_id: String,
        completed: Option<bool>,
        page: Option<u32>,
        per_page: Option<u32>,
    ) -> RestResult<TasksResponse> {
        let tasks = self.state.tasks.lock().unwrap();
        let page = page.unwrap_or(1).max(1);
        let per_page = per_page.unwrap_or(10) as usize;
        let skip = ((page - 1) * per_page as u32) as usize;

        let user_tasks = tasks.get(&user_id).cloned().unwrap_or_default();

        // Filter by completed status if provided
        let filtered: Vec<Task> = user_tasks
            .into_iter()
            .filter(|task| completed.is_none_or(|c| task.completed == c))
            .skip(skip)
            .take(per_page)
            .collect();

        Ok(RestResponse::ok(TasksResponse { tasks: filtered }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rest_backend=debug,rest_api=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Initialize state
    let state = AppState {
        users: Arc::new(Mutex::new(HashMap::new())),
        tasks: Arc::new(Mutex::new(HashMap::new())),
    };

    // Add demo users
    {
        let mut users = state.users.lock().unwrap();
        users.insert(
            "1".to_string(),
            User {
                id: "1".to_string(),
                name: "John Doe".to_string(),
                email: "john@example.com".to_string(),
                role: "user".to_string(),
            },
        );
        users.insert(
            "2".to_string(),
            User {
                id: "2".to_string(),
                name: "Admin User".to_string(),
                email: "admin@example.com".to_string(),
                role: "admin".to_string(),
            },
        );
    }

    // Setup simple authentication
    let auth_provider = SimpleAuthProvider;

    // Create service
    let service = UserServiceImpl { state };

    // Build router
    let app = UserServiceBuilder::new(service)
        .auth_provider(auth_provider)
        .build();

    // Allow local generated-client examples and browser tooling to call the API.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = app.layer(cors);

    let addr = "127.0.0.1:3000";
    tracing::info!("Server running at http://{}", addr);
    tracing::info!("API docs at http://{}/api/v1/docs", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn empty_service() -> UserServiceImpl {
        UserServiceImpl {
            state: AppState {
                users: Arc::new(Mutex::new(HashMap::new())),
                tasks: Arc::new(Mutex::new(HashMap::new())),
            },
        }
    }

    fn auth_user(user_id: &str, permissions: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_string(),
            permissions: permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect::<HashSet<_>>(),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn user_crud_and_search_round_trip_through_service() {
        let service = empty_service();
        let admin = auth_user("admin", &["admin", "user"]);

        let created = service
            .post_users(
                &admin,
                CreateUserRequest {
                    name: "Alice Example".to_string(),
                    email: "alice@example.com".to_string(),
                },
            )
            .await
            .expect("create user");

        assert_eq!(created.status, 201);
        assert_eq!(created.body.role, "user");

        let user_id = created.body.id.clone();
        let fetched = service
            .get_users_by_id(user_id.clone())
            .await
            .expect("fetch created user");
        assert_eq!(fetched.body.email, "alice@example.com");

        let updated = service
            .put_users_by_id(
                &admin,
                user_id.clone(),
                UpdateUserRequest {
                    name: Some("Alice Updated".to_string()),
                    email: None,
                },
            )
            .await
            .expect("update user");
        assert_eq!(updated.body.name, "Alice Updated");
        assert_eq!(updated.body.email, "alice@example.com");

        let search = service
            .get_search_users("updated".to_string(), Some(10), Some(0))
            .await
            .expect("search user");
        assert_eq!(search.body.total, 1);
        assert_eq!(search.body.users.len(), 1);
        assert_eq!(search.body.users[0].id, user_id);

        let deleted = service
            .delete_users_by_id(&admin, user_id.clone())
            .await
            .expect("delete user");
        assert_eq!(deleted.status, 200);

        let error = service
            .get_users_by_id(user_id)
            .await
            .expect_err("deleted user should not be found");
        assert_eq!(error.status, 404);
        assert_eq!(error.message, "User not found");
    }

    #[tokio::test]
    async fn task_crud_and_filtered_search_round_trip_through_service() {
        let service = empty_service();
        let user = auth_user("testuser", &["user"]);
        let user_id = "user-1".to_string();

        let first = service
            .post_users_by_user_id_tasks(
                &user,
                user_id.clone(),
                CreateTaskRequest {
                    title: "Draft docs".to_string(),
                    description: "Write API notes".to_string(),
                },
            )
            .await
            .expect("create first task");
        let second = service
            .post_users_by_user_id_tasks(
                &user,
                user_id.clone(),
                CreateTaskRequest {
                    title: "Review examples".to_string(),
                    description: "Check example flows".to_string(),
                },
            )
            .await
            .expect("create second task");

        assert_eq!(first.status, 201);
        assert_eq!(second.status, 201);

        let updated = service
            .put_users_by_user_id_tasks_by_task_id(
                &user,
                user_id.clone(),
                first.body.id.clone(),
                UpdateTaskRequest {
                    title: None,
                    description: Some("Write and verify API notes".to_string()),
                    completed: Some(true),
                },
            )
            .await
            .expect("update task");
        assert!(updated.body.completed);
        assert_eq!(updated.body.description, "Write and verify API notes");

        let completed = service
            .get_users_by_user_id_tasks_search(
                &user,
                user_id.clone(),
                Some(true),
                Some(1),
                Some(10),
            )
            .await
            .expect("search completed tasks");
        assert_eq!(completed.body.tasks.len(), 1);
        assert_eq!(completed.body.tasks[0].id, first.body.id);

        let all_tasks = service
            .get_users_by_user_id_tasks(&user, user_id.clone())
            .await
            .expect("list tasks");
        assert_eq!(all_tasks.body.tasks.len(), 2);

        service
            .delete_users_by_user_id_tasks_by_task_id(&user, user_id.clone(), first.body.id.clone())
            .await
            .expect("delete task");

        let remaining = service
            .get_users_by_user_id_tasks(&user, user_id)
            .await
            .expect("list remaining tasks");
        assert_eq!(remaining.body.tasks.len(), 1);
        assert_eq!(remaining.body.tasks[0].id, second.body.id);
    }

    #[tokio::test]
    async fn updating_missing_task_returns_not_found() {
        let service = empty_service();
        let user = auth_user("testuser", &["user"]);

        let error = service
            .put_users_by_user_id_tasks_by_task_id(
                &user,
                "missing-user".to_string(),
                "missing-task".to_string(),
                UpdateTaskRequest {
                    title: Some("Nope".to_string()),
                    description: None,
                    completed: None,
                },
            )
            .await
            .expect_err("missing task collection should be rejected");

        assert_eq!(error.status, 404);
        assert_eq!(error.message, "User tasks not found");
    }

    #[tokio::test]
    async fn updating_missing_task_in_existing_collection_returns_task_not_found() {
        let service = empty_service();
        let user = auth_user("testuser", &["user"]);
        service
            .post_users_by_user_id_tasks(
                &user,
                "user-1".to_string(),
                CreateTaskRequest {
                    title: "Existing task".to_string(),
                    description: "Creates the collection".to_string(),
                },
            )
            .await
            .expect("create task");

        let error = service
            .put_users_by_user_id_tasks_by_task_id(
                &user,
                "user-1".to_string(),
                "missing-task".to_string(),
                UpdateTaskRequest {
                    title: Some("Nope".to_string()),
                    description: None,
                    completed: None,
                },
            )
            .await
            .expect_err("missing task should be rejected");

        assert_eq!(error.status, 404);
        assert_eq!(error.message, "Task not found");
    }

    #[tokio::test]
    async fn task_search_treats_zero_page_as_first_page() {
        let service = empty_service();
        let user = auth_user("testuser", &["user"]);
        let user_id = "user-1".to_string();

        let task = service
            .post_users_by_user_id_tasks(
                &user,
                user_id.clone(),
                CreateTaskRequest {
                    title: "First task".to_string(),
                    description: "Should appear on page zero".to_string(),
                },
            )
            .await
            .expect("create task")
            .body;

        let result = service
            .get_users_by_user_id_tasks_search(&user, user_id, None, Some(0), Some(10))
            .await
            .expect("search tasks");

        assert_eq!(result.body.tasks.len(), 1);
        assert_eq!(result.body.tasks[0].id, task.id);
    }
}
