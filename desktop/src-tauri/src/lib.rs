pub mod commands;
pub mod core;
pub mod support;
pub mod workflow;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(workflow::WorkflowState::default())
        .setup(|app| {
            #[cfg(any(target_os = "macos", windows))]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            workflow::discover_codex,
            workflow::create_package,
            workflow::inspect_package,
            workflow::build_restore_plan,
            workflow::apply_restore,
            workflow::list_transactions,
            workflow::rollback_transaction,
            workflow::open_path,
            workflow::open_restored_thread,
            workflow::prepare_support,
            workflow::copy_support_text,
            workflow::open_support_issue,
            workflow::recheck_support,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReHome Desktop");
}
