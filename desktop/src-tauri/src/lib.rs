pub mod commands;
pub mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::discover_codex,
            commands::create_package,
            commands::inspect_package,
            commands::build_restore_plan,
            commands::apply_restore,
            commands::list_transactions,
            commands::rollback_transaction,
            commands::open_path,
            commands::open_restored_thread,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReHome Desktop");
}
