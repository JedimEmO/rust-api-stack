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
    assert!(create_task_request(String::new(), "ignored".to_string(), TaskPriority::Low).is_none());
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
