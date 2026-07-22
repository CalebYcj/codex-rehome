pub use crate::workflow::*;

#[cfg(test)]
mod tests {
    use crate::core::models::{
        CodexInventory, ContentCounts, ConversationEntry, ProjectEntry, RecoveryStatus, SourceOs,
        TransactionSummary,
    };
    use crate::workflow::{
        authorize_transaction_path, open_transaction_by_id, resolve_create_package_request,
        rollback_transaction_by_id, validate_local_dialog_path, validate_rollback_action,
        ApplyRestoreSelection, BuildRestorePlanRequest, CreatePackageSelection, RollbackAction,
        WorkflowState,
    };
    use serde_json::json;
    use std::{fs, path::PathBuf};
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn transaction_open_authorization_accepts_exact_owned_objects() {
        let fixture = open_fixture();

        assert!(authorize_transaction_path(
            &fixture.transaction_backup_path,
            &fixture.summary,
            false,
        )
        .is_ok());
        assert!(authorize_transaction_path(
            &fixture.restored_project_path,
            &fixture.summary,
            false,
        )
        .is_ok());
        assert!(
            authorize_transaction_path(&fixture.restored_project_path, &fixture.summary, true,)
                .is_ok()
        );
    }

    #[test]
    fn transaction_open_authorization_rejects_unrelated_descendants() {
        let fixture = open_fixture();

        assert!(authorize_transaction_path(
            &fixture.unrelated_project_path,
            &fixture.summary,
            false,
        )
        .is_err());
        assert!(authorize_transaction_path(
            &fixture.unrelated_backup_path,
            &fixture.summary,
            false,
        )
        .is_err());
        assert!(
            authorize_transaction_path(&fixture.unrelated_codex_path, &fixture.summary, false,)
                .is_err()
        );
        assert!(authorize_transaction_path(
            &fixture.unrelated_project_path,
            &fixture.summary,
            true,
        )
        .is_err());
        assert!(authorize_transaction_path(
            &fixture.restored_project_child,
            &fixture.summary,
            true,
        )
        .is_err());
        assert!(authorize_transaction_path(
            &fixture.transaction_backup_path,
            &fixture.summary,
            true,
        )
        .is_err());
    }

    #[test]
    fn renderer_requests_reject_forged_roots_paths_and_unc_outputs() {
        let project_id = Uuid::new_v4();
        let selection = json!({
            "project_ids": [project_id],
            "conversation_ids": [],
            "include_skills": true,
            "include_plugins": false,
            "include_generated_images": false,
            "codex_home": "C:\\forged\\.codex",
            "project_paths": ["C:\\private"],
            "output_path": "\\\\server\\share\\stolen.rehome"
        });
        assert!(serde_json::from_value::<CreatePackageSelection>(selection).is_err());

        let build = json!({
            "action": "build",
            "package_selection_id": Uuid::new_v4(),
            "destination_selection_id": Uuid::new_v4(),
            "target_codex_home": "C:\\forged\\.codex",
            "projects_root": "C:\\forged\\projects"
        });
        assert!(serde_json::from_value::<BuildRestorePlanRequest>(build).is_err());

        let apply = json!({
            "plan_id": Uuid::new_v4(),
            "codex_closed_confirmed": true,
            "register_projects": true,
            "backup_root": "\\\\server\\share\\backup"
        });
        assert!(serde_json::from_value::<ApplyRestoreSelection>(apply).is_err());
        assert!(
            validate_local_dialog_path(&PathBuf::from("\\\\server\\share\\handoff.rehome"))
                .is_err()
        );
    }

    #[test]
    fn package_selection_uses_fresh_inventory_paths_and_rejects_project_chat_mismatch() {
        let (inventory, selected_project, matching_chat, mismatched_chat, unassociated_chat) =
            inventory_fixture();
        let output = PathBuf::from("C:\\selected-by-native-dialog\\handoff.rehome");
        let selection = CreatePackageSelection {
            project_ids: vec![selected_project],
            conversation_ids: vec![matching_chat, unassociated_chat],
            include_skills: false,
            include_plugins: false,
            include_generated_images: false,
        };

        let resolved =
            resolve_create_package_request(&inventory, selection.clone(), output.clone()).unwrap();
        assert_eq!(resolved.codex_home, inventory.codex_home);
        assert_eq!(
            resolved.project_paths,
            vec![PathBuf::from("C:\\Work\\alpha")]
        );
        assert_eq!(resolved.output_path, output);
        assert_eq!(
            resolved.conversation_ids,
            vec![matching_chat, unassociated_chat]
        );

        let error = resolve_create_package_request(
            &inventory,
            CreatePackageSelection {
                conversation_ids: vec![mismatched_chat],
                ..selection
            },
            PathBuf::from("C:\\selected-by-native-dialog\\other.rehome"),
        )
        .unwrap_err();
        assert_eq!(error.code, crate::core::error::ErrorCode::ProjectConflict);

        let unknown_project_error = resolve_create_package_request(
            &inventory,
            CreatePackageSelection {
                project_ids: vec![Uuid::new_v4()],
                conversation_ids: vec![unassociated_chat],
                include_skills: false,
                include_plugins: false,
                include_generated_images: false,
            },
            PathBuf::from("C:\\selected-by-native-dialog\\unknown.rehome"),
        )
        .unwrap_err();
        assert_eq!(
            unknown_project_error.code,
            crate::core::error::ErrorCode::ProjectConflict
        );
    }

    #[test]
    fn unknown_or_cross_bound_capabilities_are_rejected() {
        let state = WorkflowState::default();
        assert!(state.resolve_package(Uuid::new_v4()).is_err());

        let first_package = state.grant_package(PathBuf::from("C:\\packages\\first.rehome"));
        let second_package = state.grant_package(PathBuf::from("C:\\packages\\second.rehome"));
        let destinations = state.grant_restore_locations(
            first_package,
            PathBuf::from("C:\\projects"),
            PathBuf::from("C:\\backups"),
        );
        assert!(state
            .resolve_restore_locations(second_package, destinations)
            .is_err());
    }

    #[test]
    fn rollback_actions_distinguish_normal_and_recovery_paths() {
        assert!(
            validate_rollback_action(RecoveryStatus::Committed, RollbackAction::Rollback).is_ok()
        );
        for status in [
            RecoveryStatus::Prepared,
            RecoveryStatus::Applying,
            RecoveryStatus::Verifying,
            RecoveryStatus::RollingBack,
            RecoveryStatus::RollbackFailed,
        ] {
            assert!(validate_rollback_action(status, RollbackAction::Resume).is_ok());
            assert!(validate_rollback_action(status, RollbackAction::Rollback).is_err());
        }
        assert!(
            validate_rollback_action(RecoveryStatus::Committed, RollbackAction::Resume).is_err()
        );
        assert!(
            validate_rollback_action(RecoveryStatus::RolledBack, RollbackAction::Resume).is_err()
        );
    }

    #[test]
    fn missing_transactions_use_command_appropriate_error_codes() {
        let transaction_id = Uuid::new_v4();
        assert_eq!(
            rollback_transaction_by_id(transaction_id).unwrap_err().code,
            crate::core::error::ErrorCode::RollbackFailed
        );
        assert_eq!(
            open_transaction_by_id(transaction_id).unwrap_err().code,
            crate::core::error::ErrorCode::RestoreFailed
        );
    }

    fn inventory_fixture() -> (CodexInventory, Uuid, Uuid, Uuid, Uuid) {
        let alpha = Uuid::new_v4();
        let beta = Uuid::new_v4();
        let matching = Uuid::new_v4();
        let mismatched = Uuid::new_v4();
        let unassociated = Uuid::new_v4();
        let project = |project_id, name: &str, path: &str| ProjectEntry {
            project_id,
            name: name.into(),
            source_path: path.into(),
            archive_path: format!("projects/{project_id}/files"),
            file_count: 1,
            content_bytes: 1,
            git_remote: None,
            git_branch: None,
            git_head: None,
        };
        let conversation = |task_id, project_id| ConversationEntry {
            task_id,
            project_id,
            title: task_id.to_string(),
            updated_at: "2026-07-23T09:00:00Z".into(),
            content_hash: "hash".into(),
            archive_path: format!("codex/sessions/{task_id}.jsonl"),
        };
        (
            CodexInventory {
                codex_home: PathBuf::from("C:\\Users\\Me\\.codex"),
                source_os: SourceOs::Windows,
                source_arch: "x86_64".into(),
                source_device_id: Uuid::new_v4(),
                counts: ContentCounts::default(),
                projects: vec![
                    project(alpha, "alpha", "C:\\Work\\alpha"),
                    project(beta, "beta", "C:\\Work\\beta"),
                ],
                project_paths: vec![
                    PathBuf::from("C:\\Work\\alpha"),
                    PathBuf::from("C:\\Work\\beta"),
                ],
                conversations: vec![
                    conversation(matching, Some(alpha)),
                    conversation(mismatched, Some(beta)),
                    conversation(unassociated, None),
                ],
                conversation_paths: vec![],
                session_index_path: None,
                state_db_path: None,
                skill_paths: vec![],
                plugin_paths: vec![],
                generated_image_paths: vec![],
                warnings: vec![],
            },
            alpha,
            matching,
            mismatched,
            unassociated,
        )
    }

    struct OpenFixture {
        _root: tempfile::TempDir,
        summary: TransactionSummary,
        transaction_backup_path: std::path::PathBuf,
        restored_project_path: std::path::PathBuf,
        restored_project_child: std::path::PathBuf,
        unrelated_project_path: std::path::PathBuf,
        unrelated_backup_path: std::path::PathBuf,
        unrelated_codex_path: std::path::PathBuf,
    }

    fn open_fixture() -> OpenFixture {
        let root = tempdir().expect("temporary root");
        let backup_root = root.path().join("backups");
        let projects_root = root.path().join("projects");
        let target_codex_home = root.path().join("codex-home");
        let transaction_backup_path = backup_root.join("transaction");
        let unrelated_backup_path = backup_root.join("unrelated");
        let restored_project_path = projects_root.join("restored-project");
        let restored_project_child = restored_project_path.join("src");
        let unrelated_project_path = projects_root.join("unrelated-project");
        let unrelated_codex_path = target_codex_home.join("sessions");
        for path in [
            &transaction_backup_path,
            &unrelated_backup_path,
            &restored_project_child,
            &unrelated_project_path,
            &unrelated_codex_path,
        ] {
            fs::create_dir_all(path).expect("fixture directory");
        }

        let transaction_backup_path = fs::canonicalize(transaction_backup_path).unwrap();
        let unrelated_backup_path = fs::canonicalize(unrelated_backup_path).unwrap();
        let restored_project_path = fs::canonicalize(restored_project_path).unwrap();
        let restored_project_child = fs::canonicalize(restored_project_child).unwrap();
        let unrelated_project_path = fs::canonicalize(unrelated_project_path).unwrap();
        let unrelated_codex_path = fs::canonicalize(unrelated_codex_path).unwrap();

        let summary = TransactionSummary {
            transaction_id: Uuid::new_v4(),
            package_id: Uuid::new_v4(),
            created_at: "2026-07-23T09:00:00Z".into(),
            status: RecoveryStatus::Committed,
            backup_root,
            transaction_backup_path: transaction_backup_path.clone(),
            target_codex_home,
            projects_root,
            restored_project_paths: vec![restored_project_path.clone()],
            changed_files: 1,
        };

        OpenFixture {
            _root: root,
            summary,
            transaction_backup_path,
            restored_project_path,
            restored_project_child,
            unrelated_project_path,
            unrelated_backup_path,
            unrelated_codex_path,
        }
    }
}
