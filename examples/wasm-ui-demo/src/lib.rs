#[macro_use]
extern crate dominator;

use dominator::{Dom, class, clone, events};
use dwind::prelude::*;
use dwind_macros::dwclass;
use futures_signals::{
    signal::{Mutable, Signal, SignalExt},
    signal_vec::{MutableVec, SignalVecExt},
};
use once_cell::sync::Lazy;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use basic_jsonrpc_api::{
    CreateTaskRequest, DashboardStats, MyServiceClient, MyServiceClientBuilder, SignInRequest,
    SignInResponse, Task, TaskListResponse, TaskPriority, UpdateTaskRequest,
};

// Define styles using dominator's class! macro
static STYLES: Lazy<String> = Lazy::new(|| {
    class! {
        .raw("
            * {
                box-sizing: border-box;
            }
            
            body {
                margin: 0;
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
                background-color: #0a0a0a;
                color: #e5e5e5;
                line-height: 1.5;
            }
            
            /* Custom scrollbar for dark mode */
            ::-webkit-scrollbar {
                width: 8px;
                height: 8px;
            }
            
            ::-webkit-scrollbar-track {
                background: #1a1a1a;
            }
            
            ::-webkit-scrollbar-thumb {
                background: #404040;
                border-radius: 4px;
            }
            
            ::-webkit-scrollbar-thumb:hover {
                background: #555;
            }
            
            /* Glass morphism effect */
            .glass {
                background: rgba(255, 255, 255, 0.05);
                backdrop-filter: blur(10px);
                border: 1px solid rgba(255, 255, 255, 0.1);
            }
            
            /* Smooth transitions */
            * {
                transition: all 0.2s ease;
            }
            
            /* Animations */
            @keyframes fadeIn {
                from { opacity: 0; transform: translateY(10px); }
                to { opacity: 1; transform: translateY(0); }
            }
            
            @keyframes slideIn {
                from { transform: translateX(-100%); }
                to { transform: translateX(0); }
            }
            
            @keyframes pulse {
                0%, 100% { opacity: 1; }
                50% { opacity: 0.8; }
            }
            
            .animate-fade-in {
                animation: fadeIn 0.5s ease-out;
            }
            
            .animate-slide-in {
                animation: slideIn 0.3s ease-out;
            }
            
            .animate-pulse {
                animation: pulse 2s infinite;
            }
        ")
    }
});

#[derive(Clone)]
struct App {
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
    fn new() -> Arc<Self> {
        // Get the current window location to build the API URL dynamically
        let window = web_sys::window().unwrap();
        let location = window.location();
        let protocol = location.protocol().unwrap();
        let host = location.host().unwrap();
        let api_url = rpc_endpoint_url(&protocol, &host);

        // Initialize the RPC client
        let client = MyServiceClientBuilder::new()
            .server_url(&api_url)
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

fn render_login_form(app: Arc<App>) -> Dom {
    html!("div", {
        .class(&*STYLES)
        .apply(|b| dwclass!(b, "flex justify-center"))
        .style("background", "linear-gradient(to bottom right, #1a1a1a, #0f0f0f, #000000)")
        .style("min-height", "100vh")
        .style("position", "relative")
        .style("overflow", "hidden")
        .children(&mut [
            // Background decoration
            html!("div", {
                .style("position", "absolute")
                .style("top", "-50%")
                .style("right", "-50%")
                .style("width", "200%")
                .style("height", "200%")
                .style("background", "radial-gradient(circle at center, rgba(59, 130, 246, 0.1) 0%, transparent 70%)")
                .style("animation", "rotate 30s linear infinite")
            }),

            html!("div", {
                .apply(|b| dwclass!(b, "flex flex-col justify-center w-full max-w-md p-8"))
                .style("position", "relative")
                .style("z-index", "10")
                .child(html!("div", {
                    .class("glass")
                    .apply(|b| dwclass!(b, "rounded-2xl shadow-2xl p-10"))
                    .children(&mut [
                        html!("h2", {
                            .apply(|b| dwclass!(b, "text-3xl font-bold text-center"))
                            .style("background", "linear-gradient(to right, #60a5fa, #a78bfa)")
                            .style("background-clip", "text")
                            .style("-webkit-background-clip", "text")
                            .style("color", "transparent")
                            .style("margin-bottom", "0.5rem")
                            .text("Welcome Back")
                        }),

                        html!("p", {
                            .apply(|b| dwclass!(b, "text-bunker-400 text-center"))
                            .style("margin-bottom", "2rem")
                            .text("Sign in to manage your tasks")
                        }),

                        html!("div", {
                            .children(&mut [
                                html!("div", {
                                    .style("margin-bottom", "1.5rem")
                                    .children(&mut [
                                        html!("label", {
                                            .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                            .style("display", "block")
                                            .style("margin-bottom", "0.5rem")
                                            .text("Username")
                                        }),
                                        html!("input", {
                                            .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500"))
                                            .style("background-color", "rgba(24, 24, 27, 0.5)")
                                            .style("outline", "none")
                                            .attr("type", "text")
                                            .attr("placeholder", "Enter your username")
                                            .prop_signal("value", app.username.signal_cloned())
                                            .event(clone!(app => move |_: events::Input| {
                                                let elem = web_sys::window()
                                                    .unwrap()
                                                    .document()
                                                    .unwrap()
                                                    .active_element()
                                                    .unwrap()
                                                    .dyn_into::<web_sys::HtmlInputElement>()
                                                    .unwrap();
                                                app.username.set(elem.value());
                                            }))
                                        }),
                                    ])
                                }),

                                html!("div", {
                                    .style("margin-bottom", "2rem")
                                    .children(&mut [
                                        html!("label", {
                                            .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                            .style("display", "block")
                                            .style("margin-bottom", "0.5rem")
                                            .text("Password")
                                        }),
                                        html!("input", {
                                            .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500"))
                                            .style("background-color", "rgba(24, 24, 27, 0.5)")
                                            .style("outline", "none")
                                            .attr("type", "password")
                                            .attr("placeholder", "Enter your password")
                                            .prop_signal("value", app.password.signal_cloned())
                                            .event(clone!(app => move |_: events::Input| {
                                                let elem = web_sys::window()
                                                    .unwrap()
                                                    .document()
                                                    .unwrap()
                                                    .active_element()
                                                    .unwrap()
                                                    .dyn_into::<web_sys::HtmlInputElement>()
                                                    .unwrap();
                                                app.password.set(elem.value());
                                            }))
                                        }),
                                    ])
                                }),

                                html!("div", {
                                    .child_signal(app.login_error.signal_cloned().map(|error| {
                                        error.map(|msg| {
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-red-400 text-sm text-center border border-red-800 rounded-lg p-3"))
                                                .style("background-color", "rgba(127, 29, 29, 0.2)")
                                                .style("margin-bottom", "1.5rem")
                                                .text(&msg)
                                            })
                                        })
                                    }))
                                }),

                                html!("button", {
                                    .apply(|b| dwclass!(b, "w-full p-4 font-semibold rounded-lg transition-all"))
                                    .style("color", "white")
                                    .style_signal("background", app.is_loading.signal().map(|loading| {
                                        if !loading { "linear-gradient(135deg, #3b82f6 0%, #8b5cf6 100%)" } else { "#4b5563" }
                                    }))
                                    .style_signal("cursor", app.is_loading.signal().map(|loading| {
                                        if !loading { "pointer" } else { "not-allowed" }
                                    }))
                                    .style("box-shadow", "0 4px 15px rgba(59, 130, 246, 0.3)")
                                    .attr("type", "button")
                                    .prop_signal("disabled", app.is_loading.signal())
                                    .text_signal(app.is_loading.signal().map(|loading| {
                                        if loading { "Signing In..." } else { "Sign In" }
                                    }))
                                    .event(clone!(app => move |_: events::Click| {
                                        App::login(app.clone());
                                    }))
                                }),

                                html!("div", {
                                    .style("margin-top", "2rem")
                                    .apply(|b| dwclass!(b, "text-sm text-bunker-500 text-center"))
                                    .children(&mut [
                                        html!("p", {
                                            .text("Demo credentials:")
                                        }),
                                        html!("p", {
                                            .apply(|b| dwclass!(b, "text-bunker-400"))
                                            .style("margin-top", "0.25rem")
                                            .text("user/password • admin/secret")
                                        }),
                                    ])
                                }),
                            ])
                        }),
                    ])
                }))
            }),
        ])
    })
}

fn render_stats_card(stats: &DashboardStats) -> Dom {
    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "rounded-2xl p-8"))
        .children(&mut [
            html!("h3", {
                .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                .style("margin-bottom", "2rem")
                .text("Dashboard Overview")
            }),

            html!("div", {
                .apply(|b| dwclass!(b, "grid grid-cols-2 gap-6"))
                .children(&mut [
                    // Total Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #2563eb, #1e40af)")
                        .style("box-shadow", "0 8px 32px rgba(59, 130, 246, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.total_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-picton-blue-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("Total Tasks")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-picton-blue-300"))
                                        .text("All")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),

                    // Completed Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #16a34a, #15803d)")
                        .style("box-shadow", "0 8px 32px rgba(34, 197, 94, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.completed_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-apple-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("Completed")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-apple-300"))
                                        .text("Done")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),

                    // Pending Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #d97706, #b45309)")
                        .style("box-shadow", "0 8px 32px rgba(251, 191, 36, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.pending_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-candlelight-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("Pending")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-candlelight-300"))
                                        .text("⏳")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),

                    // High Priority Tasks
                    html!("div", {
                        .apply(|b| dwclass!(b, "p-6 rounded-xl"))
                        .style("background", "linear-gradient(to bottom right, #dc2626, #991b1b)")
                        .style("box-shadow", "0 8px 32px rgba(239, 68, 68, 0.2)")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex justify-between"))
                                .style("align-items", "flex-start")
                                .children(&mut [
                                    html!("div", {
                                        .children(&mut [
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-3xl font-bold"))
                                                .style("color", "white")
                                                .text(&stats.high_priority_tasks.to_string())
                                            }),
                                            html!("div", {
                                                .apply(|b| dwclass!(b, "text-sm text-red-200"))
                                                .style("margin-top", "0.25rem")
                                                .text("High Priority")
                                            }),
                                        ])
                                    }),
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "text-red-300"))
                                        .text("High")
                                        .style("font-size", "1.5rem")
                                    }),
                                ])
                            }),
                        ])
                    }),
                ])
            }),
        ])
    })
}

fn render_task_form(app: Arc<App>) -> Dom {
    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "rounded-2xl p-8"))
        .children(&mut [
            html!("h3", {
                .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                .style("margin-bottom", "2rem")
                .text("Create New Task")
            }),

            html!("div", {
                .children(&mut [
                    // Title field
                    html!("div", {
                        .style("margin-bottom", "1.5rem")
                        .children(&mut [
                            html!("label", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                .style("display", "block")
                                .style("margin-bottom", "0.5rem")
                                .text("Title")
                            }),
                            html!("input", {
                                .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500 transition-all"))
                                .style("background-color", "rgba(24, 24, 27, 0.5)")
                                .style("outline", "none")
                                .attr("type", "text")
                                .attr("placeholder", "What needs to be done?")
                                .prop_signal("value", app.new_task_title.signal_cloned())
                                .event(clone!(app => move |_: events::Input| {
                                    let elem = web_sys::window()
                                        .unwrap()
                                        .document()
                                        .unwrap()
                                        .active_element()
                                        .unwrap()
                                        .dyn_into::<web_sys::HtmlInputElement>()
                                        .unwrap();
                                    app.new_task_title.set(elem.value());
                                }))
                            }),
                        ])
                    }),

                    // Description field
                    html!("div", {
                        .style("margin-bottom", "1.5rem")
                        .children(&mut [
                            html!("label", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                .style("display", "block")
                                .style("margin-bottom", "0.5rem")
                                .text("Description")
                            }),
                            html!("textarea", {
                                .apply(|b| dwclass!(b, "w-full p-4 border border-bunker-700 rounded-lg text-bunker-100 focus:border-picton-blue-500 transition-all"))
                                .style("background-color", "rgba(24, 24, 27, 0.5)")
                                .style("outline", "none")
                                .style("resize", "vertical")
                                .style("min-height", "80px")
                                .attr("placeholder", "Add more details...")
                                .prop_signal("value", app.new_task_description.signal_cloned())
                                .event(clone!(app => move |_: events::Input| {
                                    let elem = web_sys::window()
                                        .unwrap()
                                        .document()
                                        .unwrap()
                                        .active_element()
                                        .unwrap()
                                        .dyn_into::<web_sys::HtmlTextAreaElement>()
                                        .unwrap();
                                    app.new_task_description.set(elem.value());
                                }))
                            }),
                        ])
                    }),

                    // Priority field
                    html!("div", {
                        .style("margin-bottom", "2rem")
                        .children(&mut [
                            html!("label", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300"))
                                .style("display", "block")
                                .style("margin-bottom", "0.5rem")
                                .text("Priority")
                            }),
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex gap-3"))
                                .children(&mut [
                                    html!("button", {
                                        .apply(|b| dwclass!(b, "flex-1 p-3 text-sm font-medium rounded-lg border transition-all"))
                                        .style_signal("background-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Low) { "#16a34a" } else { "#1f2937" }
                                        }))
                                        .style_signal("border-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Low) { "#16a34a" } else { "#374151" }
                                        }))
                                        .style_signal("color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Low) { "white" } else { "#9ca3af" }
                                        }))
                                        .attr("type", "button")
                                        .text("Low")
                                        .event(clone!(app => move |_: events::Click| {
                                            app.new_task_priority.set(TaskPriority::Low);
                                        }))
                                    }),

                                    html!("button", {
                                        .apply(|b| dwclass!(b, "flex-1 p-3 text-sm font-medium rounded-lg border transition-all"))
                                        .style_signal("background-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Medium) { "#d97706" } else { "#1f2937" }
                                        }))
                                        .style_signal("border-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Medium) { "#d97706" } else { "#374151" }
                                        }))
                                        .style_signal("color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::Medium) { "white" } else { "#9ca3af" }
                                        }))
                                        .attr("type", "button")
                                        .text("Medium")
                                        .event(clone!(app => move |_: events::Click| {
                                            app.new_task_priority.set(TaskPriority::Medium);
                                        }))
                                    }),

                                    html!("button", {
                                        .apply(|b| dwclass!(b, "flex-1 p-3 text-sm font-medium rounded-lg border transition-all"))
                                        .style_signal("background-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::High) { "#dc2626" } else { "#1f2937" }
                                        }))
                                        .style_signal("border-color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::High) { "#dc2626" } else { "#374151" }
                                        }))
                                        .style_signal("color", app.new_task_priority.signal_cloned().map(|p| {
                                            if matches!(p, TaskPriority::High) { "white" } else { "#9ca3af" }
                                        }))
                                        .attr("type", "button")
                                        .text("High")
                                        .event(clone!(app => move |_: events::Click| {
                                            app.new_task_priority.set(TaskPriority::High);
                                        }))
                                    }),
                                ])
                            }),
                        ])
                    }),

                    html!("button", {
                        .apply(|b| dwclass!(b, "w-full p-4 font-semibold rounded-lg transition-all"))
                        .style("color", "white")
                        .style_signal("background", app.new_task_title.signal_ref(|t| {
                            if !t.is_empty() { "linear-gradient(135deg, #3b82f6 0%, #8b5cf6 100%)" } else { "#374151" }
                        }))
                        .style_signal("cursor", app.new_task_title.signal_ref(|t| {
                            if !t.is_empty() { "pointer" } else { "not-allowed" }
                        }))
                        .style_signal("box-shadow", app.new_task_title.signal_ref(|t| {
                            if !t.is_empty() { "0 4px 15px rgba(59, 130, 246, 0.3)" } else { "none" }
                        }))
                        .attr("type", "button")
                        .prop_signal("disabled", app.new_task_title.signal_ref(|t| t.is_empty()))
                        .text("Create Task")
                        .event(clone!(app => move |_: events::Click| {
                            App::create_task(app.clone());
                        }))
                    }),
                ])
            }),
        ])
    })
}

fn render_task_item(app: Arc<App>, task: Task) -> Dom {
    let task_id = task.id.clone();
    let (_priority_color, _priority_bg, priority_mark) = match task.priority {
        TaskPriority::High => ("text-red-400", "bg-red-900 bg-opacity-20", "H"),
        TaskPriority::Medium => (
            "text-candlelight-400",
            "bg-candlelight-900 bg-opacity-20",
            "M",
        ),
        TaskPriority::Low => ("text-apple-400", "bg-apple-900 bg-opacity-20", "L"),
    };

    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "p-6 rounded-xl hover:shadow-2xl transition-all"))
        .style("cursor", "pointer")
        .style("border", "1px solid rgba(255, 255, 255, 0.1)")
        .event(clone!(app, task => move |_: events::Click| {
            app.selected_task.set(Some(task.clone()));
        }))
        .child(html!("div", {
            .apply(|b| dwclass!(b, "flex gap-4"))
            .children(&mut [
                html!("div", {
                    .apply(|b| dwclass!(b, "flex"))
                    .style("align-items", "center")
                    .child(html!("input" => web_sys::HtmlInputElement, {
                        .apply(|b| dwclass!(b, "w-5 h-5 rounded bg-bunker-800 border-bunker-600 text-picton-blue-500"))
                        .style("cursor", "pointer")
                        .attr("type", "checkbox")
                        .prop("checked", task.completed)
                        .event(clone!(app, task_id => move |e: events::Change| {
                            e.stop_propagation();
                            App::toggle_task_completion(app.clone(), task_id.clone());
                        }))
                    }))
                }),

                html!("div", {
                    .apply(|b| dwclass!(b, "flex-1"))
                    .style("min-width", "0")
                    .children(&mut [
                        html!("div", {
                            .apply(|b| dwclass!(b, "flex justify-between"))
                            .style("align-items", "flex-start")
                            .children(&mut [
                                html!("h4", {
                                    .apply(|b| dwclass!(b, "text-lg font-semibold text-bunker-100"))
                                    .style_signal("text-decoration", Mutable::new(task.completed).signal().map(|completed| {
                                        if completed { "line-through" } else { "none" }
                                    }))
                                    .style_signal("opacity", Mutable::new(task.completed).signal().map(|completed| {
                                        if completed { "0.5" } else { "1" }
                                    }))
                                    .text(&task.title)
                                }),

                                html!("span", {
                                    .class(match task.priority {
                                        TaskPriority::High => "text-red-400",
                                        TaskPriority::Medium => "text-candlelight-400",
                                        TaskPriority::Low => "text-apple-400",
                                    })
                                    .style("background-color", match task.priority {
                                        TaskPriority::High => "rgba(127, 29, 29, 0.2)",
                                        TaskPriority::Medium => "rgba(180, 83, 9, 0.2)",
                                        TaskPriority::Low => "rgba(21, 128, 61, 0.2)",
                                    })
                                    .apply(|b| dwclass!(b, "rounded-full text-xs font-medium flex gap-1"))
                                    .style("padding", "0.25rem 0.75rem")
                                    .style("align-items", "center")
                                    .children(&mut [
                                        html!("span", {
                                            .text(priority_mark)
                                        }),
                                        html!("span", {
                                            .class(match task.priority {
                                                TaskPriority::High => "text-red-400",
                                                TaskPriority::Medium => "text-candlelight-400",
                                                TaskPriority::Low => "text-apple-400",
                                            })
                                            .text(&format!("{:?}", task.priority))
                                        }),
                                    ])
                                }),
                            ])
                        }),

                        html!("p", {
                            .apply(|b| dwclass!(b, "text-sm text-bunker-400"))
                            .style("margin-top", "0.5rem")
                            .style_signal("opacity", Mutable::new(task.completed).signal().map(|completed| {
                                if completed { "0.5" } else { "1" }
                            }))
                            .text(&task.description)
                        }),

                        html!("div", {
                            .apply(|b| dwclass!(b, "flex gap-4 text-xs text-bunker-500"))
                            .style("margin-top", "0.75rem")
                            .children(&mut [
                                html!("span", {
                                    .apply(|b| dwclass!(b, "flex gap-1"))
                                    .style("align-items", "center")
                                    .children(&mut [
                                        html!("span", {
                                            .text("Created")
                                        }),
                                        html!("span", {
                                            .text(timestamp_date(&task.created_at))
                                        }),
                                    ])
                                }),
                            ])
                        }),
                    ])
                }),

                html!("button", {
                    .apply(|b| dwclass!(b, "text-red-400 hover:text-red-300 text-sm font-medium rounded-lg transition-all"))
                    .style("padding", "0.25rem 0.75rem")
                    .text("Delete")
                    .event(clone!(app, task_id => move |e: events::Click| {
                        e.stop_propagation();
                        App::delete_task(app.clone(), task_id.clone());
                    }))
                }),
            ])
        }))
    })
}

fn render_task_list(app: Arc<App>) -> Dom {
    html!("div", {
        .class("glass")
        .apply(|b| dwclass!(b, "rounded-2xl p-8"))
        .children(&mut [
            html!("div", {
                .apply(|b| dwclass!(b, "flex justify-between"))
                .style("align-items", "center")
                .style("margin-bottom", "2rem")
                .children(&mut [
                    html!("h3", {
                        .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                        .text("Your Tasks")
                    }),
                    html!("div", {
                        .apply(|b| dwclass!(b, "text-sm text-bunker-400"))
                        .text_signal(app.tasks.signal_vec_cloned().len().map(|len| {
                            format!("{} task{}", len, if len == 1 { "" } else { "s" })
                        }))
                    }),
                ])
            }),

            html!("div", {
                .style("display", "flex")
                .style("flex-direction", "column")
                .style("gap", "1rem")
                .children_signal_vec(app.tasks.signal_vec_cloned()
                    .map(clone!(app => move |task| {
                        render_task_item(app.clone(), task)
                    })))
            }),

            // Empty state
            html!("div", {
                .apply(|b| dwclass!(b, "text-center"))
                .style("padding", "3rem 0")
                .visible_signal(app.tasks.signal_vec_cloned().len().map(|len| len == 0))
                .children(&mut [
                    html!("div", {
                        .apply(|b| dwclass!(b, "text-2xl font-semibold text-bunker-300"))
                        .style("margin-bottom", "1rem")
                        .text("No Tasks")
                    }),
                    html!("p", {
                        .apply(|b| dwclass!(b, "text-bunker-400 text-lg"))
                        .text("No tasks yet. Create your first task!")
                    }),
                ])
            }),
        ])
    })
}

fn render_dashboard(app: Arc<App>) -> Dom {
    html!("div", {
        .class(&*STYLES)
        .style("min-height", "100vh")
        .style("background", "linear-gradient(to bottom, #0a0a0a, #000000)")
        .children(&mut [
            // Header
            html!("nav", {
                .class("glass")
                .apply(|b| dwclass!(b, "sticky top-0"))
                .style("z-index", "50")
                .child(html!("div", {
                    .apply(|b| dwclass!(b, "max-w-7xl p-4"))
                    .style("margin", "0 auto")
                    .child(html!("div", {
                        .apply(|b| dwclass!(b, "flex justify-between"))
                        .style("align-items", "center")
                        .children(&mut [
                            html!("div", {
                                .apply(|b| dwclass!(b, "flex gap-3"))
                                .style("align-items", "center")
                                .children(&mut [
                                    html!("div", {
                                        .apply(|b| dwclass!(b, "w-10 h-10 rounded-lg flex justify-center"))
                                        .style("background", "linear-gradient(to bottom right, #3b82f6, #8b5cf6)")
                                        .style("align-items", "center")
                                        .child(html!("span", {
                                            .apply(|b| dwclass!(b, "font-bold text-lg"))
                                            .style("color", "white")
                                            .text("T")
                                        }))
                                    }),
                                    html!("h1", {
                                        .apply(|b| dwclass!(b, "text-2xl font-bold"))
                                        .style("background", "linear-gradient(to right, #60a5fa, #a78bfa)")
                                        .style("background-clip", "text")
                                        .style("-webkit-background-clip", "text")
                                        .style("color", "transparent")
                                        .text("Task Manager")
                                    }),
                                ])
                            }),

                            html!("button", {
                                .apply(|b| dwclass!(b, "text-sm font-medium text-bunker-300 rounded-lg transition-all border border-bunker-700"))
                                .style("padding", "0.5rem 1rem")
                                .style("background-color", "rgba(31, 41, 55, 0.5)")
                                .text("Sign Out")
                                .event(clone!(app => move |_: events::Click| {
                                    App::logout(app.clone());
                                }))
                            }),
                        ])
                    }))
                }))
            }),

            // Main content
            html!("main", {
                .apply(|b| dwclass!(b, "max-w-7xl p-6"))
                .style("margin", "0 auto")
                .style("padding-top", "2rem")
                .style("padding-bottom", "2rem")
                .child(html!("div", {
                    .apply(|b| dwclass!(b, "grid gap-8"))
                    .style("grid-template-columns", "1fr")
                    .children(&mut [
                        // Left column - Stats and Tasks
                        html!("div", {
                            .style("display", "flex")
                            .style("flex-direction", "column")
                            .style("gap", "1.5rem")
                            .children(&mut [
                                // Stats
                                html!("div", {
                                    .child_signal(app.stats.signal_cloned().map(|stats| {
                                        stats.map(|s| render_stats_card(&s))
                                    }))
                                }),

                                // Task list
                                render_task_list(app.clone()),
                            ])
                        }),

                        // Right column - Create form and selected task
                        html!("div", {
                            .style("display", "flex")
                            .style("flex-direction", "column")
                            .style("gap", "1.5rem")
                            .children(&mut [
                                // Create task form
                                render_task_form(app.clone()),

                                // Selected task details
                                html!("div", {
                                    .child_signal(app.selected_task.signal_cloned().map(clone!(app => move |task| {
                                        task.map(|t| {
                                            html!("div", {
                                                .class("glass animate-fade-in")
                                                .apply(|b| dwclass!(b, "rounded-2xl p-8"))
                                                .children(&mut [
                                                    html!("div", {
                                                        .apply(|b| dwclass!(b, "flex justify-between"))
                                                        .style("align-items", "center")
                                                        .style("margin-bottom", "2rem")
                                                        .children(&mut [
                                                            html!("h3", {
                                                                .apply(|b| dwclass!(b, "text-2xl font-bold text-bunker-100"))
                                                                .text("Task Details")
                                                            }),
                                                            html!("button", {
                                                                .apply(|b| dwclass!(b, "text-bunker-400 hover:text-bunker-200 text-2xl"))
                                                                .text("×")
                                                                .event(clone!(app => move |_: events::Click| {
                                                                    app.selected_task.set(None);
                                                                }))
                                                            }),
                                                        ])
                                                    }),

                                                    html!("div", {
                                                        .style("display", "flex")
                                                        .style("flex-direction", "column")
                                                        .style("gap", "1.5rem")
                                                        .children(&mut [
                                                            // Title and status
                                                            html!("div", {
                                                                .children(&mut [
                                                                    html!("h4", {
                                                                        .apply(|b| dwclass!(b, "text-xl font-semibold text-bunker-100"))
                                                                        .style("margin-bottom", "0.5rem")
                                                                        .text(&t.title)
                                                                    }),
                                                                    html!("p", {
                                                                        .apply(|b| dwclass!(b, "text-bunker-400"))
                                                                        .text(&t.description)
                                                                    }),
                                                                ])
                                                            }),

                                                            // Meta info
                                                            html!("div", {
                                                                .apply(|b| dwclass!(b, "grid grid-cols-2 gap-4"))
                                                                .children(&mut [
                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Task ID")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm text-bunker-300 font-mono"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .text(task_id_preview(&t.id))
                                                                                .attr("title", &t.id)
                                                                            }),
                                                                        ])
                                                                    }),

                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Status")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm font-medium"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .apply(|b| if t.completed {
                                                                                    dwclass!(b, "text-apple-400")
                                                                                } else {
                                                                                    dwclass!(b, "text-candlelight-400")
                                                                                })
                                                                                .text(if t.completed { "Completed" } else { "In Progress" })
                                                                            }),
                                                                        ])
                                                                    }),

                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Created")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm text-bunker-300"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .text(timestamp_date(&t.created_at))
                                                                            }),
                                                                        ])
                                                                    }),

                                                                    html!("div", {
                                                                        .apply(|b| dwclass!(b, "rounded-lg p-4"))
                                                                        .style("background-color", "rgba(31, 41, 55, 0.5)")
                                                                        .children(&mut [
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-xs text-bunker-500"))
                                                                                .style("text-transform", "uppercase")
                                                                                .style("letter-spacing", "0.05em")
                                                                                .text("Updated")
                                                                            }),
                                                                            html!("div", {
                                                                                .apply(|b| dwclass!(b, "text-sm text-bunker-300"))
                                                                                .style("margin-top", "0.25rem")
                                                                                .text(timestamp_date(&t.updated_at))
                                                                            }),
                                                                        ])
                                                                    }),
                                                                ])
                                                            }),
                                                        ])
                                                    }),
                                                ])
                                            })
                                        })
                                    })))
                                }),
                            ])
                        }),
                    ])
                }))
            }),
        ])
    })
}

fn render(app: Arc<App>) -> Dom {
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
mod tests {
    use super::*;

    fn task(completed: bool) -> Task {
        Task {
            id: "task-1".to_string(),
            title: "Review generated client".to_string(),
            description: "Keep the browser example using typed requests".to_string(),
            completed,
            priority: TaskPriority::High,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn rpc_endpoint_url_uses_same_origin_rpc_path() {
        assert_eq!(
            rpc_endpoint_url("https:", "app.example.test"),
            "https://app.example.test/rpc"
        );
        assert_eq!(
            rpc_endpoint_url("http:", "localhost:8080"),
            "http://localhost:8080/rpc"
        );
    }

    #[test]
    fn create_task_request_preserves_typed_form_values() {
        let request = create_task_request(
            "Ship docs".to_string(),
            "Update the example README".to_string(),
            TaskPriority::High,
        )
        .expect("non-empty title should build request");

        assert_eq!(request.title, "Ship docs");
        assert_eq!(request.description, "Update the example README");
        assert!(matches!(request.priority, TaskPriority::High));
    }

    #[test]
    fn create_task_request_rejects_empty_title() {
        assert!(
            create_task_request(String::new(), "ignored".to_string(), TaskPriority::Low).is_none()
        );
    }

    #[test]
    fn task_completion_update_only_toggles_completion() {
        let update = task_completion_update(&task(false));

        assert_eq!(update.id, "task-1");
        assert_eq!(update.title, None);
        assert_eq!(update.description, None);
        assert_eq!(update.completed, Some(true));
        assert!(update.priority.is_none());

        assert_eq!(task_completion_update(&task(true)).completed, Some(false));
    }

    #[test]
    fn task_id_preview_uses_short_safe_display_id() {
        assert_eq!(task_id_preview("1234567890"), "12345678");
        assert_eq!(task_id_preview("short"), "short");
    }

    #[test]
    fn timestamp_date_uses_date_prefix_when_timestamp_is_long_enough() {
        assert_eq!(timestamp_date("2026-01-01T00:00:00Z"), "2026-01-01");
        assert_eq!(timestamp_date("bad"), "bad");
    }

    #[test]
    fn safe_prefix_returns_original_when_byte_boundary_would_split_character() {
        assert_eq!(safe_prefix("abcé", 4), "abcé");
    }
}

#[wasm_bindgen(start)]
pub fn main() {
    // Initialize panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize dwind styles
    dwind::stylesheet();

    // Create app and render
    let app = App::new();
    dominator::append_dom(&dominator::body(), render(app));
}
