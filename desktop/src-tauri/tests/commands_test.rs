use rehome_desktop_lib::{
    commands::apply_restore,
    core::{error::ErrorCode, models::RestoreOptions},
};
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn apply_restore_command_accepts_only_a_trusted_plan_id() {
    let backup = tempdir().unwrap();

    let error = apply_restore(
        Uuid::nil(),
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: backup.path().to_path_buf(),
            register_projects: true,
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("plan"));
}
