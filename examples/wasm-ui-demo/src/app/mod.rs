use dominator::{Dom, clone, events};
use dwind::prelude::*;
use dwind_macros::dwclass;
use futures_signals::{
    signal::{Mutable, Signal, SignalExt},
    signal_vec::{MutableVec, SignalVecExt},
};
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use basic_jsonrpc_api::{
    CreateTaskRequest, DashboardStats, MyServiceClient, MyServiceClientBuilder, SignInRequest,
    SignInResponse, Task, TaskListResponse, TaskPriority, UpdateTaskRequest,
};

mod styles;
use styles::STYLES;
mod login;
use login::render_login_form;
mod statistics;
use statistics::render_stats_card;
mod task_form;
use task_form::render_task_form;
mod task_item;
use task_item::render_task_item;
mod task_list;
use task_list::render_task_list;
mod dashboard;
use dashboard::render_dashboard;

#[derive(Clone)]
pub(super) struct App {
    // Authentication state
    auth_token: Mutable<Option<String>>,
    username: Mutable<String>,
    password: Mutable<String>,
    login_error: Mutable<Option<String>>,
    is_loading: Mutable<bool>,

    // Tasks state
    tasks: MutableVec<Task>,
    selected_task: Mutable<Option<Task>>,

    // Task form state
    new_task_title: Mutable<String>,
    new_task_description: Mutable<String>,
    new_task_priority: Mutable<TaskPriority>,

    // Dashboard stats
    stats: Mutable<Option<DashboardStats>>,

    // RPC client
    client: MyServiceClient,
}

impl App {
    pub(super) fn new() -> Arc<Self> {
        // Get the current window location to build the API URL dynamically
        let window = web_sys::window().unwrap();
        let location = window.location();
        let protocol = location.protocol().unwrap();
        let host = location.host().unwrap();
        let api_url = rpc_endpoint_url(&protocol, &host);

        // Initialize the RPC client
        let client = MyServiceClientBuilder::new(&api_url)
            .build()
            .expect("Failed to build client");

        Arc::new(Self {
            auth_token: Mutable::new(None),
            username: Mutable::new(String::new()),
            password: Mutable::new(String::new()),
            login_error: Mutable::new(None),
            is_loading: Mutable::new(false),

            tasks: MutableVec::new(),
            selected_task: Mutable::new(None),

            new_task_title: Mutable::new(String::new()),
            new_task_description: Mutable::new(String::new()),
            new_task_priority: Mutable::new(TaskPriority::Medium),

            stats: Mutable::new(None),

            client,
        })
    }

    fn is_authenticated(&self) -> impl Signal<Item = bool> + 'static {
        self.auth_token.signal_ref(|token| token.is_some())
    }

    fn login(app: Arc<Self>) {
        let username = app.username.get_cloned();
        let password = app.password.get_cloned();

        app.is_loading.set(true);
        app.login_error.set(None);

        spawn_local(clone!(app => async move {
            let result = app.client.sign_in(SignInRequest::WithCredentials {
                username,
                password,
            }).await;

            app.is_loading.set(false);

            match result {
                Ok(SignInResponse::Success { jwt }) => {
                    app.auth_token.set(Some(jwt));
                    app.password.set(String::new());

                    // Load initial data after login
                    Self::load_tasks(app.clone());
                    Self::load_stats(app.clone());
                }
                Ok(SignInResponse::Failure { msg }) => {
                    app.login_error.set(Some(msg));
                }
                Err(e) => {
                    app.login_error.set(Some(format!("Connection error: {}", e)));
                }
            }
        }));
    }

    fn logout(app: Arc<Self>) {
        spawn_local(clone!(app => async move {
            if let Some(token) = app.auth_token.get_cloned() {
                let mut client = app.client.clone();
                client.set_bearer_token(Some(token));

                let _ = client.sign_out(()).await;
            }

            app.auth_token.set(None);
            app.tasks.lock_mut().clear();
            app.stats.set(None);
            app.selected_task.set(None);
        }));
    }

    fn load_tasks(app: Arc<Self>) {
        spawn_local(clone!(app => async move {
            if let Some(token) = app.auth_token.get_cloned() {
                let mut client = app.client.clone();
                client.set_bearer_token(Some(token));

                if let Ok(TaskListResponse { tasks, .. }) = client.list_tasks(()).await {
                    app.tasks.lock_mut().replace_cloned(tasks);
                }
            }
        }));
    }

    fn load_stats(app: Arc<Self>) {
        spawn_local(clone!(app => async move {
            if let Some(token) = app.auth_token.get_cloned() {
                let mut client = app.client.clone();
                client.set_bearer_token(Some(token));

                if let Ok(stats) = client.get_dashboard_stats(()).await {
                    app.stats.set(Some(stats));
                }
            }
        }));
    }

    fn create_task(app: Arc<Self>) {
        let title = app.new_task_title.get_cloned();
        let description = app.new_task_description.get_cloned();
        let priority = app.new_task_priority.get_cloned();

        let Some(request) = create_task_request(title, description, priority) else {
            return;
        };

        spawn_local(clone!(app => async move {
            if let Some(token) = app.auth_token.get_cloned() {
                let mut client = app.client.clone();
                client.set_bearer_token(Some(token));

                if let Ok(task) = client.create_task(request).await {
                    app.tasks.lock_mut().push_cloned(task);
                    app.new_task_title.set(String::new());
                    app.new_task_description.set(String::new());
                    app.new_task_priority.set(TaskPriority::Medium);

                    // Reload stats
                    Self::load_stats(app.clone());
                }
            }
        }));
    }

    fn toggle_task_completion(app: Arc<Self>, task_id: String) {
        spawn_local(clone!(app => async move {
            if let Some(token) = app.auth_token.get_cloned() {
                let mut client = app.client.clone();
                client.set_bearer_token(Some(token));

                // Find the task to toggle
                let task_index = app.tasks.lock_ref().iter()
                    .position(|t| t.id == task_id);

                if let Some(index) = task_index {
                    let request = task_completion_update(&app.tasks.lock_ref()[index]);

                    if let Ok(updated_task) = client.update_task(request).await {
                        app.tasks.lock_mut().set_cloned(index, updated_task);

                        // Reload stats
                        Self::load_stats(app.clone());
                    }
                }
            }
        }));
    }

    fn delete_task(app: Arc<Self>, task_id: String) {
        spawn_local(clone!(app => async move {
            if let Some(token) = app.auth_token.get_cloned() {
                let mut client = app.client.clone();
                client.set_bearer_token(Some(token));

                if client.delete_task(task_id.clone()).await.is_ok() {
                    app.tasks.lock_mut().retain(|t| t.id != task_id);

                    // Clear selection if the deleted task was selected
                    if let Some(selected) = app.selected_task.get_cloned()
                        && selected.id == task_id
                    {
                        app.selected_task.set(None);
                    }

                    // Reload stats
                    Self::load_stats(app.clone());
                }
            }
        }));
    }
}

fn rpc_endpoint_url(protocol: &str, host: &str) -> String {
    format!("{}//{}/rpc", protocol, host)
}

fn create_task_request(
    title: String,
    description: String,
    priority: TaskPriority,
) -> Option<CreateTaskRequest> {
    if title.is_empty() {
        return None;
    }

    Some(CreateTaskRequest {
        title,
        description,
        priority,
    })
}

fn task_completion_update(task: &Task) -> UpdateTaskRequest {
    UpdateTaskRequest {
        id: task.id.clone(),
        title: None,
        description: None,
        completed: Some(!task.completed),
        priority: None,
    }
}

fn task_id_preview(id: &str) -> &str {
    safe_prefix(id, 8)
}

fn timestamp_date(timestamp: &str) -> &str {
    safe_prefix(timestamp, 10)
}

fn safe_prefix(value: &str, max_bytes: usize) -> &str {
    value.get(..max_bytes).unwrap_or(value)
}

pub(super) fn render(app: Arc<App>) -> Dom {
    html!("div", {
        .child_signal(app.is_authenticated().map(clone!(app => move |authenticated| {
            if authenticated {
                Some(render_dashboard(app.clone()))
            } else {
                Some(render_login_form(app.clone()))
            }
        })))
    })
}

#[cfg(test)]
mod tests;
