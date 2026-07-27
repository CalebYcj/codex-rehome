const WORKFLOW_SOURCE: &str = include_str!("../src/workflow.rs");
const APP_SOURCE: &str = include_str!("../src/lib.rs");

const COMMANDS: &[&str] = &[
    "discover_codex",
    "create_package",
    "inspect_package",
    "build_restore_plan",
    "apply_restore",
    "list_transactions",
    "rollback_transaction",
    "open_path",
    "open_restored_thread",
];

#[test]
fn desktop_registers_exactly_the_nine_reviewed_commands() {
    assert_eq!(WORKFLOW_SOURCE.matches("#[tauri::command]").count(), 9);
    for command in COMMANDS {
        assert!(
            APP_SOURCE.contains(&format!("workflow::{command}")),
            "missing command registration for {command}"
        );
    }
}

#[test]
fn every_custom_command_is_async() {
    for command in COMMANDS {
        assert!(
            WORKFLOW_SOURCE.contains(&format!("pub async fn {command}")),
            "{command} must dispatch blocking work asynchronously"
        );
    }
    assert!(WORKFLOW_SOURCE.contains("tauri::async_runtime::spawn_blocking"));
}

#[test]
fn restore_application_uses_only_the_opaque_core_plan_id() {
    assert!(WORKFLOW_SOURCE.contains("apply_restore_by_id("));
    assert!(!WORKFLOW_SOURCE.contains("pub async fn apply_restore(\n    plan:"));
}
