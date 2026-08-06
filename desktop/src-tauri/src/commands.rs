pub use crate::workflow::*;

#[cfg(test)]
mod tests {
    use crate::core::discovery::{discover_codex_with_context, DiscoveryContext};
    use crate::core::models::{
        CodexInventory, ContentCounts, ConversationEntry, ProjectEntry, RecoveryStatus, SourceOs,
        TransactionSummary,
    };
    use crate::core::package::{
        create_package as core_create_package, inspect_package as core_inspect_package,
    };
    use crate::workflow::{
        authorize_transaction_path, open_transaction_by_id, resolve_create_package_request,
        rollback_transaction_by_id, validate_local_dialog_path, validate_rollback_action,
        ApplyRestoreSelection, BuildRestorePlanRequest, CreatePackageSelection, RollbackAction,
        WorkflowState,
    };
    use serde_json::json;
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Barrier},
        thread,
    };
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
    fn package_selection_uses_fresh_inventory_paths_and_allows_independent_chats() {
        let (inventory, selected_project, matching_chat, mismatched_chat, unassociated_chat) =
            inventory_fixture();
        let output = PathBuf::from("C:\\selected-by-native-dialog\\handoff.rehome");
        let selection = CreatePackageSelection {
            project_ids: vec![selected_project],
            conversation_ids: vec![matching_chat, unassociated_chat],
            skill_ids: vec![],
            plugin_ids: vec![],
            generated_image_ids: vec![],
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

        let conversation_only = resolve_create_package_request(
            &inventory,
            CreatePackageSelection {
                project_ids: vec![],
                conversation_ids: vec![mismatched_chat],
                ..selection
            },
            PathBuf::from("C:\\selected-by-native-dialog\\other.rehome"),
        )
        .unwrap();
        assert!(conversation_only.project_paths.is_empty());
        assert_eq!(conversation_only.conversation_ids, vec![mismatched_chat]);

        let unknown_project_error = resolve_create_package_request(
            &inventory,
            CreatePackageSelection {
                project_ids: vec![Uuid::new_v4()],
                conversation_ids: vec![unassociated_chat],
                skill_ids: vec![],
                plugin_ids: vec![],
                generated_image_ids: vec![],
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
    fn package_selection_rejects_a_deleted_registered_project_but_not_its_chat() {
        let (mut inventory, selected_project, matching_chat, _, _) = inventory_fixture();
        inventory.projects[0].source_available = false;

        let error = resolve_create_package_request(
            &inventory,
            CreatePackageSelection {
                project_ids: vec![selected_project],
                conversation_ids: vec![matching_chat],
                skill_ids: vec![],
                plugin_ids: vec![],
                generated_image_ids: vec![],
            },
            PathBuf::from("C:\\selected-by-native-dialog\\missing.rehome"),
        )
        .unwrap_err();

        assert_eq!(error.code, crate::core::error::ErrorCode::ProjectConflict);
        assert!(error.message.contains("select its conversations instead"));
    }

    #[test]
    fn production_discovery_ids_resolve_to_server_owned_package_paths() {
        let root = tempdir().expect("temporary root");
        let codex_home = root.path().join(".codex");
        let project = root.path().join("projects").join("desktop");
        let task_id = Uuid::new_v4();
        let session = codex_home
            .join("sessions")
            .join(format!("rollout-{task_id}.jsonl"));
        fs::create_dir_all(&project).expect("project directory");
        fs::create_dir_all(session.parent().unwrap()).expect("session directory");
        fs::write(project.join("README.md"), "# Desktop\n").expect("project file");
        fs::write(
            codex_home.join(".codex-global-state.json"),
            serde_json::to_vec(&json!({
                "electron-saved-workspace-roots": [project]
            }))
            .unwrap(),
        )
        .expect("global state");
        fs::write(
            &session,
            format!(
                "{}\n",
                json!({
                    "type": "session_meta",
                    "timestamp": "2026-07-23T10:00:00Z",
                    "payload": {
                        "id": task_id,
                        "cwd": project
                    }
                })
            ),
        )
        .expect("session");
        fs::write(
            codex_home.join("session_index.jsonl"),
            format!(
                "{}\n",
                json!({
                    "id": task_id,
                    "cwd": project,
                    "thread_name": "Production workflow",
                    "updated_at": "2026-07-23T10:00:00Z"
                })
            ),
        )
        .expect("session index");

        let inventory = discover_codex_with_context(Some(codex_home), &DiscoveryContext::default())
            .expect("discovery");
        let discovered_project = inventory.projects.first().expect("project entry");
        let discovered_conversation = inventory.conversations.first().expect("conversation entry");
        assert_eq!(
            discovered_conversation.project_id,
            Some(discovered_project.project_id)
        );

        let request = resolve_create_package_request(
            &inventory,
            CreatePackageSelection {
                project_ids: vec![discovered_project.project_id],
                conversation_ids: vec![discovered_conversation.task_id],
                skill_ids: vec![],
                plugin_ids: vec![],
                generated_image_ids: vec![],
            },
            root.path().join("handoff.rehome"),
        )
        .expect("server-owned request");
        assert_eq!(
            request.project_paths,
            vec![fs::canonicalize(project).unwrap()]
        );
        assert_eq!(request.conversation_ids, vec![task_id]);

        let report = core_create_package(request).expect("package creation");
        let package = core_inspect_package(&report.package_path).expect("package inspection");
        assert_eq!(package.manifest.projects.len(), 1);
        assert_eq!(package.manifest.conversations.len(), 1);
        assert_eq!(
            package.manifest.conversations[0].project_id,
            Some(package.manifest.projects[0].project_id)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_local_verbatim_disk_paths_are_allowed_but_unc_prefixes_are_rejected() {
        let root = tempdir().expect("temporary root");
        let canonical = fs::canonicalize(root.path()).expect("canonical local path");
        assert!(canonical.to_string_lossy().starts_with(r"\\?\"));
        assert!(validate_local_dialog_path(&canonical).is_ok());

        for rejected in [
            PathBuf::from(r"\\server\share\handoff.rehome"),
            PathBuf::from(r"\\?\UNC\server\share\handoff.rehome"),
        ] {
            assert!(
                validate_local_dialog_path(&rejected).is_err(),
                "expected UNC rejection for {}",
                rejected.display()
            );
        }
    }

    #[test]
    fn unknown_or_cross_bound_capabilities_are_rejected() {
        let state = WorkflowState::default();
        assert!(state.resolve_package(Uuid::new_v4()).is_err());

        let root = tempdir().expect("temporary root");
        let first_path = root.path().join("first.rehome");
        let second_path = root.path().join("second.rehome");
        fs::write(&first_path, b"first").expect("first package");
        fs::write(&second_path, b"second").expect("second package");
        let first_package = state
            .grant_inspected_package(fs::canonicalize(first_path).unwrap(), "first".into())
            .unwrap();
        let second_package = state
            .grant_inspected_package(fs::canonicalize(second_path).unwrap(), "second".into())
            .unwrap();
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
    fn inspected_package_grant_rejects_hash_changes_and_same_path_replacement() {
        let root = tempdir().expect("temporary root");
        let package = root.path().join("selected.rehome");
        fs::write(&package, b"first package").expect("first package");
        let package = fs::canonicalize(package).expect("canonical package");
        let state = WorkflowState::default();
        let grant = state
            .grant_inspected_package(package.clone(), "first-hash".into())
            .expect("package grant");

        let hash_error = state
            .validate_package_grant(grant, "different-hash")
            .unwrap_err();
        assert_eq!(
            hash_error.code,
            crate::core::error::ErrorCode::PackageInvalid
        );

        let displaced = root.path().join("displaced.rehome");
        fs::rename(&package, displaced).expect("move original package");
        fs::write(&package, b"first package").expect("replacement package");
        let identity_error = state
            .validate_package_grant(grant, "first-hash")
            .unwrap_err();
        assert_eq!(
            identity_error.code,
            crate::core::error::ErrorCode::PackageInvalid
        );
        assert!(identity_error.message.contains("identity"));
    }

    #[test]
    fn concurrent_restore_plan_claims_allow_exactly_one_caller() {
        let state = WorkflowState::default();
        let plan_id = Uuid::new_v4();
        state
            .grant_plan(plan_id, PathBuf::from("C:\\backups"))
            .unwrap();

        let claimed = coordinated_claims({
            let state = state.clone();
            move || state.claim_plan(plan_id)
        });
        assert_eq!(claimed, 1);
    }

    #[test]
    fn an_in_flight_restore_plan_cannot_be_regranted() {
        let state = WorkflowState::default();
        let plan_id = Uuid::new_v4();
        state
            .grant_plan(plan_id, PathBuf::from("C:\\backups"))
            .unwrap();
        let claim = state.claim_plan(plan_id).expect("in-flight claim");

        assert!(state
            .grant_plan(plan_id, PathBuf::from("C:\\different-backups"))
            .is_err());
        assert!(state.claim_plan(plan_id).is_err());
        drop(claim);
    }

    #[test]
    fn restore_plan_claim_can_only_be_reopened_by_explicit_safe_retry() {
        let state = WorkflowState::default();
        let retryable_id = Uuid::new_v4();
        state
            .grant_plan(retryable_id, PathBuf::from("C:\\backups"))
            .unwrap();
        state
            .claim_plan(retryable_id)
            .expect("first claim")
            .restore_available();
        assert!(state.claim_plan(retryable_id).is_ok());

        let consumed_id = Uuid::new_v4();
        state
            .grant_plan(consumed_id, PathBuf::from("C:\\backups"))
            .unwrap();
        drop(state.claim_plan(consumed_id).expect("consumed claim"));
        assert!(state.claim_plan(consumed_id).is_err());
    }

    #[test]
    fn concurrent_rollback_claims_allow_exactly_one_caller() {
        let state = WorkflowState::default();
        let transaction_id = Uuid::new_v4();

        let claimed = coordinated_claims({
            let state = state.clone();
            move || state.claim_rollback(transaction_id)
        });
        assert_eq!(claimed, 1);
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
            source_available: true,
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
            classification: None,
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
                skills: vec![],
                plugins: vec![],
                generated_images: vec![],
                warnings: vec![],
            },
            alpha,
            matching,
            mismatched,
            unassociated,
        )
    }

    fn coordinated_claims<T: Send + 'static, E: Send + 'static>(
        claim: impl Fn() -> Result<T, E> + Send + Sync + 'static,
    ) -> usize {
        let start = Arc::new(Barrier::new(3));
        let hold = Arc::new(Barrier::new(3));
        let claim = Arc::new(claim);
        let threads = (0..2)
            .map(|_| {
                let start = start.clone();
                let hold = hold.clone();
                let claim = claim.clone();
                thread::spawn(move || {
                    start.wait();
                    let claimed = claim();
                    hold.wait();
                    claimed.is_ok()
                })
            })
            .collect::<Vec<_>>();
        start.wait();
        hold.wait();
        threads
            .into_iter()
            .map(|thread| thread.join().expect("claim thread"))
            .filter(|claimed| *claimed)
            .count()
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
