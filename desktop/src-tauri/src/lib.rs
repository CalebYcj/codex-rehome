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

#[cfg(test)]
mod brand_tests {
    #[test]
    fn compiled_default_window_icon_contains_the_current_blue_brand() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let icon = context
            .default_window_icon()
            .expect("application window icon");
        assert!(icon.width() >= 32 && icon.height() >= 32);
        assert!(
            icon.rgba().as_chunks::<4>().0.contains(&[47, 91, 211, 255]),
            "the runtime/taskbar default must embed the blue house/return mark"
        );
        assert!(
            icon.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 0),
            "platform icon corners must remain transparent"
        );
    }
}
