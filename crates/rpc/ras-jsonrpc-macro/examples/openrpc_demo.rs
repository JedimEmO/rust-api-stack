//! Example demonstrating OpenRPC document generation
//!
//! Run with: cargo run --locked --example openrpc_demo

use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Define request/response types with JsonSchema
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateTaskRequest {
    /// The title of the task
    pub title: String,
    /// Optional description
    pub description: Option<String>,
    /// Priority level (1-5)
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateTaskResponse {
    /// Unique task identifier
    pub id: String,
    /// Creation timestamp
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetTaskRequest {
    /// Task ID to retrieve
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetTaskResponse {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub priority: u8,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompleteTaskRequest {
    /// Task ID to mark as complete
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompleteTaskResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdminStatsRequest {
    /// Include detailed statistics
    pub detailed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdminStatsResponse {
    pub total_tasks: u64,
    pub completed_tasks: u64,
    pub active_users: u64,
}

// Generate the service with various authentication requirements and OpenRPC enabled
jsonrpc_service!({
    service_name: TaskService,
    openrpc: true,
    methods: [
        // Public method - no authentication required
        UNAUTHORIZED create_task(CreateTaskRequest) -> CreateTaskResponse,

        // Requires authentication and read permission
        WITH_PERMISSIONS(["task.read"]) get_task(GetTaskRequest) -> GetTaskResponse,

        // Requires authentication and write permission
        WITH_PERMISSIONS(["task.write"]) complete_task(CompleteTaskRequest) -> CompleteTaskResponse,

        // Requires admin permissions
        WITH_PERMISSIONS(["admin.read", "admin.stats"]) admin_stats(AdminStatsRequest) -> AdminStatsResponse,
    ]
});

fn main() {
    // Generate and display the OpenRPC document
    let openrpc_doc = generate_taskservice_openrpc();

    println!("Generated OpenRPC Document:");
    println!("{}", serde_json::to_string_pretty(&openrpc_doc).unwrap());

    // Also write to file
    match generate_taskservice_openrpc_to_file() {
        Ok(()) => println!("\nOpenRPC document written to target directory"),
        Err(e) => eprintln!("\nError writing OpenRPC document: {}", e),
    }
}
