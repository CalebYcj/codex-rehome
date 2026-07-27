#[allow(dead_code)]
mod common;

use common::{synthetic_codex_fixture, SyntheticCodexFixture, THREAD_ID};
use rehome_desktop_lib::core::{
    backup::claim_transaction_rollback,
    error::ErrorCode,
    models::{
        ChangeKind, ContentCounts, ConversationEntry, CreatePackageRequest, RecoveryStatus,
        RegistrationStatus, RestoreOptions, RestorePlan, SourceOs, TargetInventory,
    },
    package::{create_package, inspect_package},
    planner::build_restore_plan,
    restore::{
        apply_restore, apply_restore_by_id, apply_restore_with_registrar, list_transaction_history,
        list_transactions, recover_incomplete_transactions, rollback, transaction_summary,
    },
};
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use tempfile::TempDir;
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipArchive, ZipWriter};

static APP_DATA_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn successful_restore_commits_with_layered_verification() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;

    let report = apply_restore(harness.plan.clone(), harness.options())?;

    assert_eq!(report.package_id, harness.plan.package_id);
    assert!(report.restored_files > 0);
    assert!(report.restored_bytes > 0);
    assert!(report.verification.package_checksum_valid);
    assert!(report.verification.files_valid);
    assert!(report.verification.sessions_valid);
    assert!(report.verification.session_index_valid);
    assert!(report.verification.sqlite_threads_valid);
    assert!(report.verification.path_mapping_valid);
    assert!(report.verification.forbidden_files_absent);
    assert!(report.verification.project_files_valid);
    assert!(!report.verification.app_registration_valid);
    assert!(!report.verification.app_visible_ready);
    assert_ne!(snapshot_mutable_targets(&harness.plan)?, before);

    let journal = harness.read_journal(report.transaction_id)?;
    assert_eq!(journal["status"], "committed");
    assert_eq!(
        PathBuf::from(journal["backup_root"].as_str().unwrap()),
        fs::canonicalize(&harness.backup_root)?
    );
    let operations = journal["operations"].as_array().unwrap();
    assert!(operations.len() >= harness.plan.operations.len() + 3);
    assert!(operations.iter().any(|operation| {
        operation["target"] == harness.plan.sessions[0].target.to_string_lossy().as_ref()
            && operation["backup_kind"] == "absent"
    }));
    Ok(())
}

#[test]
fn transaction_history_lists_committed_and_rolled_back_journals_without_mutation(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore_by_id(harness.plan.plan_id, harness.options())?;

    let committed = list_transactions()?;

    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].transaction_id, report.transaction_id);
    assert_eq!(committed[0].status, RecoveryStatus::Committed);
    assert_eq!(
        committed[0].restored_project_paths,
        vec![fs::canonicalize(harness.plan.projects_root.join("visual"))?]
    );
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::Committed);

    rollback(report.transaction_id)?;
    let rolled_back = list_transactions()?;

    assert_eq!(rolled_back.len(), 1);
    assert_eq!(rolled_back[0].status, RecoveryStatus::RolledBack);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    Ok(())
}

#[test]
fn malformed_unrelated_journal_is_isolated_from_history_and_direct_rollback(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore_by_id(harness.plan.plan_id, harness.options())?;
    fs::write(harness.transactions_dir().join("notes.json"), b"not-json")?;

    let history = list_transaction_history()?;

    assert_eq!(history.transactions.len(), 1);
    assert_eq!(
        history.transactions[0].transaction_id,
        report.transaction_id
    );
    assert_eq!(history.warnings.len(), 1);
    assert!(history.warnings[0].contains("notes.json"));
    assert_eq!(
        transaction_summary(report.transaction_id)?
            .expect("requested transaction")
            .status,
        RecoveryStatus::Committed
    );

    let rollback_report = rollback(report.transaction_id)?;
    assert!(rollback_report.success);
    Ok(())
}

#[test]
fn os_rollback_claim_excludes_independent_owners_without_changing_the_journal(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore_by_id(harness.plan.plan_id, harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let before = fs::read(&journal_path)?;
    let first_owner = claim_transaction_rollback(report.transaction_id)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(error.message.contains("already in progress"));
    assert_eq!(fs::read(&journal_path)?, before);

    drop(first_owner);
    assert!(rollback(report.transaction_id)?.success);
    Ok(())
}

#[test]
fn empty_transaction_history_does_not_create_app_data_directories() -> Result<(), Box<dyn Error>> {
    let _env_lock = APP_DATA_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let local_app_data = tempfile::tempdir()?;
    let previous = env::var_os("LOCALAPPDATA");
    env::set_var("LOCALAPPDATA", local_app_data.path());
    let application_root = local_app_data.path().join("com.rehome.desktop");

    let history = list_transactions();
    let application_root_created = application_root.exists();

    if let Some(value) = previous {
        env::set_var("LOCALAPPDATA", value);
    } else {
        env::remove_var("LOCALAPPDATA");
    }
    assert!(history?.is_empty());
    assert!(!application_root_created);
    Ok(())
}

#[test]
fn failure_after_project_copy_rolls_every_target_back_exactly() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let session_target = &harness.plan.sessions[0].target;
    fs::create_dir_all(session_target.parent().unwrap())?;
    let lock_path = session_target.parent().unwrap().join(format!(
        ".{}.codex-rehome.lock",
        session_target.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&lock_path, b"test harness failure injection")?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert!(!harness
        .plan
        .projects_root
        .join("visual")
        .join("README.md")
        .exists());
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    let journal = fs::read_dir(harness.transactions_dir())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "json"))
        .expect("rolled-back restore should retain a JSON transaction journal");
    let journal: Value = serde_json::from_slice(&fs::read(journal)?)?;
    let copied_project = journal["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| {
            operation["package_source"]
                .as_str()
                .unwrap()
                .ends_with("/README.md")
        })
        .unwrap();
    assert_ne!(copied_project["applied_state"], Value::Null);
    Ok(())
}

#[test]
fn sqlite_update_failure_rolls_project_index_and_database_back_exactly(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::RequiredColumnWithoutDefault)?;
    let before = snapshot_mutable_targets(&harness.plan)?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(
        error.message.contains("SQLite") || error.message.contains("sqlite"),
        "{error:?}"
    );
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    Ok(())
}

#[test]
fn sqlite_wal_update_failure_rolls_back_without_leaving_sidecars() -> Result<(), Box<dyn Error>> {
    let mut harness = RestoreHarness::new(DatabaseSchema::RequiredColumnWithoutDefault)?;
    let database = harness.plan.target_codex_home.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    drop(connection);
    let preview = inspect_package(&harness.plan.package_path)?;
    let target = TargetInventory {
        codex_home: harness.plan.target_codex_home.clone(),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    harness.plan = build_restore_plan(&preview, &target, &harness.plan.projects_root)?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed, "{error:?}");
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!sqlite_sidecar(&database, suffix).exists());
    }
    let restored = Connection::open(&database)?;
    let thread_count: i64 =
        restored.query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))?;
    assert_eq!(thread_count, 0);
    Ok(())
}

#[test]
fn sqlite_wal_verification_failure_refreshes_sidecars_before_rollback() -> Result<(), Box<dyn Error>>
{
    let mut harness = RestoreHarness::new_with_setup(DatabaseSchema::Compatible, |package, _| {
        replace_selected_session_payload(
            package,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{THREAD_ID}\",\"title\":\"Synthetic migration thread\"}}}}\n"
            )
            .as_bytes(),
        )
    })?;
    let database = harness.plan.target_codex_home.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    drop(connection);
    let preview = inspect_package(&harness.plan.package_path)?;
    let target = TargetInventory {
        codex_home: harness.plan.target_codex_home.clone(),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    harness.plan = build_restore_plan(&preview, &target, &harness.plan.projects_root)?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed, "{error:?}");
    assert!(error.message.contains("path_mapping_valid: false"));
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!sqlite_sidecar(&database, suffix).exists());
    }
    Ok(())
}

#[test]
fn backup_root_must_not_overlap_projects_root() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let error = apply_restore(
        harness.plan.clone(),
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: harness.plan.projects_root.clone(),
            register_projects: false,
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("must not overlap"));
    assert!(!harness.transactions_dir().exists());
    Ok(())
}

#[test]
fn restart_discovers_an_incomplete_journal_from_app_data() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal: Value = serde_json::from_slice(&fs::read(&journal_path)?)?;
    journal["status"] = Value::String("applying".into());
    for operation in journal["operations"].as_array_mut().unwrap() {
        operation["applied_hash"] = Value::Null;
    }
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    let pending = recover_incomplete_transactions()?;

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].transaction_id, report.transaction_id);
    assert_eq!(pending[0].status, RecoveryStatus::Applying);
    assert_eq!(
        pending[0].backup_root,
        fs::canonicalize(&harness.backup_root)?
    );
    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn user_rollback_restores_exact_pre_restore_hashes_and_tombstones() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;

    let rollback_report = rollback(report.transaction_id)?;

    assert!(rollback_report.success);
    assert!(rollback_report.restored_files > 0);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    Ok(())
}

#[test]
fn restore_requires_explicit_confirmation_that_codex_is_closed() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let mut options = harness.options();
    options.codex_closed_confirmed = false;

    let error = apply_restore(harness.plan.clone(), options).unwrap_err();

    assert_eq!(error.code, ErrorCode::CodexRunning);
    assert!(!harness.transactions_dir().exists());
    Ok(())
}

#[test]
fn apply_rejects_caller_forged_roots_before_any_write() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let outside = tempfile::tempdir()?;
    let forged_target = outside.path().join("README.md");
    let mut forged = harness.plan.clone();
    let operation = forged
        .operations
        .iter_mut()
        .find(|operation| operation.package_source.ends_with("/README.md"))
        .unwrap();
    operation.target = forged_target.clone();
    forged.projects_root = outside.path().to_path_buf();

    let error = apply_restore(forged, harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("plan") && error.message.contains("server"));
    assert!(!forged_target.exists());
    assert!(!harness.transactions_dir().exists());
    Ok(())
}

#[test]
fn opaque_plan_id_applies_the_server_held_plan() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;

    let report = apply_restore_by_id(harness.plan.plan_id, harness.options())?;

    assert_eq!(report.package_id, harness.plan.package_id);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::Committed);
    Ok(())
}

#[test]
fn forbidden_package_is_rejected_before_journal_or_backup_creation() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new_with_setup(DatabaseSchema::Compatible, |package_path, _| {
        let preview = inspect_package(package_path)?;
        let name = format!("{}/.env", preview.manifest.projects[0].archive_path);
        add_forbidden_payload(package_path, &name, b"SECRET=1\n")
    })?;
    assert!(inspect_package(&harness.plan.package_path)?.forbidden_files_total > 0);

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::PackageInvalid);
    assert!(error.message.contains("forbidden"));
    assert!(!harness.transactions_dir().exists());
    assert!(!harness.backup_root.exists());
    Ok(())
}

#[test]
fn changed_noop_target_prevents_commit() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new_with_setup(DatabaseSchema::Compatible, |_, target_root| {
        let target = target_root
            .join("projects")
            .join("visual")
            .join("README.md");
        fs::create_dir_all(target.parent().unwrap())?;
        fs::write(target, b"# Visual project\n")?;
        Ok(())
    })?;
    let operation = harness
        .plan
        .operations
        .iter()
        .find(|operation| operation.package_source.ends_with("/README.md"))
        .unwrap();
    assert_eq!(operation.action, ChangeKind::Unchanged);
    fs::write(&operation.target, b"changed after planning\n")?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("changed") || error.message.contains("verification"));
    assert_ne!(harness.single_journal_status()?, RecoveryStatus::Committed);
    Ok(())
}

#[test]
fn corrupted_ready_session_index_prevents_commit_when_bridge_write_was_omitted(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let plan = ready_restore_plan(&harness)?;
    let index_path = plan.target_codex_home.join("session_index.jsonl");
    fs::write(&index_path, b"{\"id\":\"unrelated\"}\n")?;

    let error = apply_restore(plan, harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("verification"));
    assert_eq!(fs::read(&index_path)?, b"{\"id\":\"unrelated\"}\n");
    Ok(())
}

#[test]
fn corrupted_ready_sqlite_row_prevents_commit_when_bridge_write_was_omitted(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let plan = ready_restore_plan(&harness)?;
    let database = plan.target_codex_home.join("state_5.sqlite");
    Connection::open(&database)?.execute("DELETE FROM threads", [])?;

    let error = apply_restore(plan, harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("verification"));
    let remaining: i64 =
        Connection::open(database)?
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))?;
    assert_eq!(remaining, 0);
    Ok(())
}

#[test]
fn recovery_reports_corrupt_uuid_named_journal() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    fs::create_dir_all(harness.transactions_dir())?;
    fs::write(
        harness
            .transactions_dir()
            .join(format!("{}.json", Uuid::new_v4())),
        b"{not-json",
    )?;

    let first = recover_incomplete_transactions().unwrap_err();
    let second = recover_incomplete_transactions().unwrap_err();

    assert_eq!(first.code, ErrorCode::RollbackFailed);
    assert_eq!(first.message, second.message);
    assert!(first.message.contains("journal") && first.message.contains("invalid"));
    Ok(())
}

#[test]
fn recovery_reports_non_uuid_json_entry_as_an_invalid_journal() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    fs::create_dir_all(harness.transactions_dir())?;
    fs::write(harness.transactions_dir().join("notes.json"), b"{}")?;

    let first = recover_incomplete_transactions().unwrap_err();
    let second = recover_incomplete_transactions().unwrap_err();

    assert_eq!(first.code, ErrorCode::RollbackFailed);
    assert_eq!(first.message, second.message);
    assert!(first.message.contains("journal") && first.message.contains("UUID"));
    Ok(())
}

#[test]
fn recovery_skips_unrelated_non_json_file() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    fs::create_dir_all(harness.transactions_dir())?;
    fs::write(
        harness.transactions_dir().join("notes.txt"),
        b"not a journal",
    )?;

    assert!(recover_incomplete_transactions()?.is_empty());
    Ok(())
}

#[test]
fn recovery_reports_uuid_named_directory_as_an_invalid_journal() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let path = harness
        .transactions_dir()
        .join(format!("{}.json", Uuid::new_v4()));
    fs::create_dir_all(&path)?;

    let first = recover_incomplete_transactions().unwrap_err();
    let second = recover_incomplete_transactions().unwrap_err();

    assert_eq!(first.code, ErrorCode::RollbackFailed);
    assert_eq!(first.message, second.message);
    assert!(first.message.contains("journal") && first.message.contains("regular file"));
    Ok(())
}

#[test]
fn recovery_reports_uuid_named_symlink_as_an_invalid_journal() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    fs::create_dir_all(harness.transactions_dir())?;
    let real = harness.transactions_dir().join("real-journal.json");
    let linked = harness
        .transactions_dir()
        .join(format!("{}.json", Uuid::new_v4()));
    fs::write(&real, b"{}")?;
    if let Err(error) = create_file_symlink(&real, &linked) {
        if windows_symlink_privilege_is_unavailable(&error) {
            eprintln!("skipping journal symlink test: Windows symlink privilege unavailable");
            return Ok(());
        }
        return Err(error.into());
    }

    let first = recover_incomplete_transactions().unwrap_err();
    let second = recover_incomplete_transactions().unwrap_err();

    assert_eq!(first.code, ErrorCode::RollbackFailed);
    assert_eq!(first.message, second.message);
    assert!(first.message.contains("journal") && first.message.contains("regular file"));
    Ok(())
}

#[test]
fn rollback_rejects_same_content_replacement_with_a_new_identity() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let target = harness.plan.sessions[0].target.clone();
    let replacement = target.with_extension("replacement");
    let applied = fs::read(&target)?;
    fs::write(&replacement, &applied)?;
    fs::remove_file(&target)?;
    fs::rename(&replacement, &target)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(
        error.message.contains("identity") || error.message.contains("conflict"),
        "{error:?}"
    );
    assert_eq!(fs::read(&target)?, applied);
    Ok(())
}

#[test]
fn rollback_preserves_replacement_created_after_target_was_quarantined(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let (index, operation) = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .find(|(_, operation)| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let quarantine_name = rollback_quarantine_name(report.transaction_id, index);
    let quarantine = target.parent().unwrap().join(&quarantine_name);
    let applied = fs::read(&target)?;
    fs::rename(&target, &quarantine)?;
    let replacement = b"replacement created while rollback was verifying quarantine\n";
    fs::write(&target, replacement)?;
    operation["rollback_quarantine"] = Value::String(quarantine_name);
    operation["rollback_progress"] = Value::String("target_quarantined".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(
        error.message.contains("conflict") || error.message.contains("present"),
        "{error:?}"
    );
    assert_eq!(fs::read(&target)?, replacement);
    assert_eq!(fs::read(&quarantine)?, applied);
    Ok(())
}

#[test]
fn rollback_restores_an_unrecognized_directory_from_quarantine() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let (index, operation) = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .find(|(_, operation)| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let quarantine_name = rollback_quarantine_name(report.transaction_id, index);
    let quarantine = target.parent().unwrap().join(&quarantine_name);
    fs::remove_file(&target)?;
    fs::create_dir(&target)?;
    fs::write(target.join("unknown"), b"do not delete")?;
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(
        error.message.contains("conflict") || error.message.contains("regular file"),
        "{error:?}"
    );
    assert_eq!(fs::read(target.join("unknown"))?, b"do not delete");
    assert!(!quarantine.exists());
    Ok(())
}

#[test]
fn rollback_resumes_after_quarantine_before_phase_persisted() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let (index, operation) = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .find(|(_, operation)| {
            operation["backup_kind"] == "file"
                && operation["package_source"] != "codex/metadata/threads.json"
        })
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let quarantine_name = rollback_quarantine_name(report.transaction_id, index);
    fs::rename(&target, target.parent().unwrap().join(&quarantine_name))?;
    operation["rollback_quarantine"] = Value::String(quarantine_name);
    operation["rollback_progress"] = Value::String("pending".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn rollback_resumes_after_quarantine_was_recorded() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let (index, operation) = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .find(|(_, operation)| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let quarantine_name = rollback_quarantine_name(report.transaction_id, index);
    fs::rename(&target, target.parent().unwrap().join(&quarantine_name))?;
    operation["rollback_quarantine"] = Value::String(quarantine_name);
    operation["rollback_progress"] = Value::String("target_quarantined".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn rollback_resumes_after_quarantine_verification_was_recorded() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let (index, operation) = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .find(|(_, operation)| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let quarantine_name = rollback_quarantine_name(report.transaction_id, index);
    let quarantine = target.parent().unwrap().join(&quarantine_name);
    let applied = fs::read(&target)?;
    fs::rename(&target, &quarantine)?;
    operation["rollback_quarantine"] = Value::String(quarantine_name);
    operation["rollback_progress"] = Value::String("quarantine_verified".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert_eq!(fs::read(&quarantine)?, applied);
    Ok(())
}

#[test]
fn rollback_resumes_after_verified_quarantine_was_consumed() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let (index, operation) = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
        .find(|(_, operation)| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let quarantine_name = rollback_quarantine_name(report.transaction_id, index);
    fs::remove_file(&target)?;
    operation["rollback_quarantine"] = Value::String(quarantine_name);
    operation["rollback_progress"] = Value::String("quarantine_verified".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn incomplete_rollback_refuses_to_overwrite_a_newer_edit() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("applying".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;
    let target = harness.plan.sessions[0].target.clone();
    let newer = b"newer edit after interrupted restore\n";
    fs::write(&target, newer)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(
        error.message.contains("changed") || error.message.contains("conflict"),
        "{error:?}"
    );
    assert_eq!(fs::read(&target)?, newer);
    assert_eq!(
        harness.single_journal_status()?,
        RecoveryStatus::RollbackFailed
    );
    Ok(())
}

#[test]
fn rollback_rejects_remove_before_progress_was_persisted() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let operation = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|operation| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    fs::remove_file(&target)?;
    operation["rollback_progress"] = Value::String("pending".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(
        error.message.contains("conflict") || error.message.contains("missing"),
        "{error:?}"
    );
    assert!(!target.exists());
    Ok(())
}

#[test]
fn rollback_retries_after_recorded_target_removal() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let operation = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|operation| operation["backup_kind"] == "absent")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    fs::remove_file(target)?;
    operation["rollback_progress"] = Value::String("target_removed".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn rollback_rejects_restore_before_progress_was_persisted() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let backup_root = PathBuf::from(journal["backup_root"].as_str().unwrap());
    let operation = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|operation| operation["backup_kind"] == "file")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let backup = backup_root
        .join(report.transaction_id.to_string())
        .join(operation["backup_path"].as_str().unwrap());
    fs::copy(&backup, &target)?;
    operation["rollback_progress"] = Value::String("target_removed".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    let error = rollback(report.transaction_id).unwrap_err();

    assert_eq!(error.code, ErrorCode::RollbackFailed);
    assert!(error.message.contains("conflict") || error.message.contains("present"));
    assert_eq!(fs::read(&target)?, fs::read(&backup)?);
    Ok(())
}

#[test]
fn rollback_retries_after_recorded_original_restoration() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("rolling_back".into());
    let backup_root = PathBuf::from(journal["backup_root"].as_str().unwrap());
    let operation = journal["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|operation| operation["backup_kind"] == "file")
        .unwrap();
    let target = PathBuf::from(operation["target"].as_str().unwrap());
    let backup = backup_root
        .join(report.transaction_id.to_string())
        .join(operation["backup_path"].as_str().unwrap());
    fs::copy(backup, target)?;
    operation["rollback_progress"] = Value::String("original_restored".into());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn recovery_removes_only_a_journal_owned_crash_lock() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("applying".into());
    let target = &harness.plan.sessions[0].target;
    let owned_lock = journal["locks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lock| lock["target"] == target.to_string_lossy().as_ref())
        .unwrap();
    let lock_path = PathBuf::from(owned_lock["path"].as_str().unwrap());
    let token = owned_lock["token"].as_str().unwrap().to_owned();
    assert_eq!(token, report.transaction_id.to_string());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;
    fs::write(&lock_path, token)?;

    let pending = recover_incomplete_transactions()?;

    assert_eq!(pending.len(), 1);
    assert!(!lock_path.exists());
    Ok(())
}

#[test]
fn recovery_preserves_a_foreign_lock_at_an_owned_path() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal = harness.read_journal(report.transaction_id)?;
    journal["status"] = Value::String("applying".into());
    let lock_path = PathBuf::from(journal["locks"][0]["path"].as_str().unwrap());
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;
    fs::write(&lock_path, b"another-transaction")?;

    let pending = recover_incomplete_transactions()?;

    assert_eq!(pending.len(), 1);
    assert_eq!(fs::read_to_string(lock_path)?, "another-transaction");
    Ok(())
}

#[test]
fn apply_rejects_a_hardlink_added_after_planning() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let index = harness
        .plan
        .operations
        .iter()
        .find(|operation| operation.package_source == "codex/session_index.jsonl")
        .unwrap();
    let outside = harness._fixture.root.join("outside-index.jsonl");
    fs::hard_link(&index.target, &outside)?;
    let outside_before = fs::read(&outside)?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("hard link"));
    assert_eq!(fs::read(outside)?, outside_before);
    Ok(())
}

#[test]
fn registration_runs_after_commit_and_sets_app_visible_ready() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let mut options = harness.options();
    options.register_projects = true;
    let transactions = harness.transactions_dir();
    let mut observed_committed = false;

    let report = apply_restore_with_registrar(harness.plan.clone(), options, |_, _| {
        let entry = fs::read_dir(&transactions)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let journal: Value = serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
        observed_committed = journal["status"] == "committed";
        RegistrationStatus::Registered
    })?;

    assert!(observed_committed);
    assert_eq!(report.registrations.len(), 1);
    assert_eq!(
        report.registrations[0].status,
        RegistrationStatus::Registered
    );
    assert!(report.verification.app_registration_valid);
    assert!(report.verification.app_visible_ready);
    Ok(())
}

#[test]
fn registration_attempts_every_project_and_reports_partial_failure() -> Result<(), Box<dyn Error>> {
    let harness =
        RestoreHarness::new_with_projects(DatabaseSchema::Compatible, true, |_, _| Ok(()))?;
    let mut options = harness.options();
    options.register_projects = true;
    let mut attempts = 0;

    let report = apply_restore_with_registrar(harness.plan.clone(), options, |_, _| {
        attempts += 1;
        if attempts == 1 {
            RegistrationStatus::InvocationFailed {
                message: "first project failed".into(),
            }
        } else {
            RegistrationStatus::Registered
        }
    })?;

    assert_eq!(attempts, 2);
    assert_eq!(report.registrations.len(), 2);
    assert!(report
        .registrations
        .iter()
        .any(|result| result.status == RegistrationStatus::Registered));
    assert!(report
        .registrations
        .iter()
        .any(|result| matches!(result.status, RegistrationStatus::InvocationFailed { .. })));
    assert!(!report.verification.app_registration_valid);
    assert!(!report.verification.app_visible_ready);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::Committed);
    Ok(())
}

#[test]
fn sqlite_wal_backup_is_a_coherent_self_contained_database() -> Result<(), Box<dyn Error>> {
    let mut harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let database = harness.plan.target_codex_home.join("state_5.sqlite");
    let keeper = Connection::open(&database)?;
    keeper.pragma_update(None, "journal_mode", "WAL")?;
    keeper.pragma_update(None, "wal_autocheckpoint", 0)?;
    keeper.execute_batch(
        "CREATE TABLE rollback_marker(value TEXT NOT NULL);\
         INSERT INTO rollback_marker VALUES ('from-wal');",
    )?;
    assert!(sqlite_sidecar(&database, "-wal").exists());
    let preview = inspect_package(&harness.plan.package_path)?;
    let target = TargetInventory {
        codex_home: harness.plan.target_codex_home.clone(),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    harness.plan = build_restore_plan(&preview, &target, &harness.plan.projects_root)?;

    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal = harness.read_journal(report.transaction_id)?;
    let operation = journal["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["package_source"] == "codex/metadata/threads.json")
        .unwrap();
    let backup = PathBuf::from(journal["backup_root"].as_str().unwrap())
        .join(report.transaction_id.to_string())
        .join(operation["backup_path"].as_str().unwrap());
    let snapshot = Connection::open(backup)?;
    let marker: String =
        snapshot.query_row("SELECT value FROM rollback_marker", [], |row| row.get(0))?;
    assert_eq!(marker, "from-wal");
    Ok(())
}

#[derive(Clone, Copy)]
enum DatabaseSchema {
    Compatible,
    RequiredColumnWithoutDefault,
}

struct RestoreHarness {
    _env_lock: MutexGuard<'static, ()>,
    _previous_local_app_data: Option<OsString>,
    _app_data: TempDir,
    _fixture: SyntheticCodexFixture,
    plan: RestorePlan,
    backup_root: PathBuf,
}

impl RestoreHarness {
    fn new(schema: DatabaseSchema) -> Result<Self, Box<dyn Error>> {
        Self::new_with_setup(schema, |_, _| Ok(()))
    }

    fn new_with_setup(
        schema: DatabaseSchema,
        setup: impl FnOnce(&Path, &Path) -> Result<(), Box<dyn Error>>,
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_with_projects(schema, false, setup)
    }

    fn new_with_projects(
        schema: DatabaseSchema,
        include_second_project: bool,
        setup: impl FnOnce(&Path, &Path) -> Result<(), Box<dyn Error>>,
    ) -> Result<Self, Box<dyn Error>> {
        let env_lock = APP_DATA_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let app_data = tempfile::tempdir()?;
        let previous_local_app_data = env::var_os("LOCALAPPDATA");
        env::set_var("LOCALAPPDATA", app_data.path());

        let fixture = synthetic_codex_fixture()?;
        align_fixture_project_metadata(&fixture)?;
        let package_path = fixture.root.join("handoff.rehome");
        let mut project_paths = vec![fixture.project_path.clone()];
        if include_second_project {
            let second = fixture.root.join("projects").join("second");
            fs::create_dir_all(&second)?;
            fs::write(second.join("README.md"), b"# Second project\n")?;
            project_paths.push(second);
        }
        create_package(CreatePackageRequest {
            codex_home: fixture.codex_home.clone(),
            project_paths,
            conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
            output_path: package_path.clone(),
            source_device_id: Uuid::nil(),
            skill_paths: vec![],
            plugin_paths: vec![],
            generated_image_paths: vec![],
        })?;
        let target_root = fixture.root.join("target");
        let codex_home = target_root.join(".codex");
        let projects_root = target_root.join("projects");
        fs::create_dir_all(&codex_home)?;
        fs::write(
            codex_home.join("session_index.jsonl"),
            b"{\"id\":\"99999999-9999-4999-8999-999999999999\",\"title\":\"Target\"}\n",
        )?;
        create_target_database(&codex_home.join("state_5.sqlite"), schema)?;
        setup(&package_path, &target_root)?;
        let preview = inspect_package(&package_path)?;
        let target = TargetInventory {
            codex_home,
            target_os: current_source_os(),
            target_arch: "x86_64".into(),
            counts: ContentCounts::default(),
            projects: vec![],
            conversations: vec![],
        };
        let plan = build_restore_plan(&preview, &target, &projects_root)?;
        let backup_root = app_data.path().join("com.rehome.desktop").join("backups");

        Ok(Self {
            _env_lock: env_lock,
            _previous_local_app_data: previous_local_app_data,
            _app_data: app_data,
            _fixture: fixture,
            plan,
            backup_root,
        })
    }

    fn options(&self) -> RestoreOptions {
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: self.backup_root.clone(),
            register_projects: false,
        }
    }

    fn transactions_dir(&self) -> PathBuf {
        self._app_data
            .path()
            .join("com.rehome.desktop")
            .join("transactions")
    }

    fn journal_path(&self, transaction_id: Uuid) -> PathBuf {
        self.transactions_dir()
            .join(format!("{transaction_id}.json"))
    }

    fn read_journal(&self, transaction_id: Uuid) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&fs::read(
            self.journal_path(transaction_id),
        )?)?)
    }

    fn single_journal_status(&self) -> Result<RecoveryStatus, Box<dyn Error>> {
        let entries = fs::read_dir(self.transactions_dir())?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json")
            })
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let journal: Value = serde_json::from_slice(&fs::read(entries[0].path())?)?;
        Ok(serde_json::from_value(journal["status"].clone())?)
    }
}

fn add_forbidden_payload(
    package_path: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<(), Box<dyn Error>> {
    let source = fs::File::open(package_path)?;
    let mut archive = ZipArchive::new(source)?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() || entry.name() == "checksums.sha256" {
            continue;
        }
        let mut contents = Vec::new();
        std::io::copy(&mut entry, &mut contents)?;
        entries.push((entry.name().to_owned(), contents));
    }
    entries.push((name.to_owned(), bytes.to_vec()));
    let temporary = package_path.with_extension("rehome.tmp");
    let mut writer = ZipWriter::new(fs::File::create(&temporary)?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for (entry_name, contents) in &entries {
        writer.start_file(entry_name, options)?;
        writer.write_all(contents)?;
    }
    let checksums = entries
        .iter()
        .filter(|(entry_name, _)| entry_name != "manifest.json")
        .map(|(entry_name, contents)| format!("{:x}  {entry_name}\n", Sha256::digest(contents)))
        .collect::<String>();
    writer.start_file("checksums.sha256", options)?;
    writer.write_all(checksums.as_bytes())?;
    writer.finish()?;
    fs::rename(temporary, package_path)?;
    Ok(())
}

fn replace_selected_session_payload(
    package_path: &Path,
    replacement: &[u8],
) -> Result<(), Box<dyn Error>> {
    let source = fs::File::open(package_path)?;
    let mut archive = ZipArchive::new(source)?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() || entry.name() == "checksums.sha256" {
            continue;
        }
        let mut contents = Vec::new();
        std::io::copy(&mut entry, &mut contents)?;
        entries.push((entry.name().to_owned(), contents));
    }
    let manifest_index = entries
        .iter()
        .position(|(name, _)| name == "manifest.json")
        .ok_or("package manifest is missing")?;
    let mut manifest: Value = serde_json::from_slice(&entries[manifest_index].1)?;
    let session_source = manifest["conversations"][0]["archive_path"]
        .as_str()
        .ok_or("package conversation source is missing")?
        .to_owned();
    let session_index = entries
        .iter()
        .position(|(name, _)| name == &session_source)
        .ok_or("package conversation payload is missing")?;
    entries[session_index].1 = replacement.to_vec();
    manifest["conversations"][0]["content_hash"] =
        Value::String(format!("{:x}", Sha256::digest(replacement)));
    entries[manifest_index].1 = serde_json::to_vec(&manifest)?;

    let temporary = package_path.with_extension("rehome.tmp");
    let mut writer = ZipWriter::new(fs::File::create(&temporary)?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for (entry_name, contents) in &entries {
        writer.start_file(entry_name, options)?;
        writer.write_all(contents)?;
    }
    let checksums = entries
        .iter()
        .filter(|(entry_name, _)| entry_name != "manifest.json")
        .map(|(entry_name, contents)| format!("{:x}  {entry_name}\n", Sha256::digest(contents)))
        .collect::<String>();
    writer.start_file("checksums.sha256", options)?;
    writer.write_all(checksums.as_bytes())?;
    writer.finish()?;
    fs::rename(temporary, package_path)?;
    Ok(())
}

impl Drop for RestoreHarness {
    fn drop(&mut self) {
        if let Some(value) = self._previous_local_app_data.take() {
            env::set_var("LOCALAPPDATA", value);
        } else {
            env::remove_var("LOCALAPPDATA");
        }
    }
}

fn create_target_database(path: &Path, schema: DatabaseSchema) -> Result<(), Box<dyn Error>> {
    let connection = Connection::open(path)?;
    let extra = match schema {
        DatabaseSchema::Compatible => "target_only TEXT NOT NULL DEFAULT 'untouched'",
        DatabaseSchema::RequiredColumnWithoutDefault => "target_only TEXT NOT NULL",
    };
    connection.execute_batch(&format!(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT,
            rollout_path TEXT,
            title TEXT,
            updated_at TEXT,
            archived INTEGER,
            has_user_event INTEGER,
            preview TEXT,
            {extra}
        );"
    ))?;
    Ok(())
}

fn align_fixture_project_metadata(fixture: &SyntheticCodexFixture) -> Result<(), Box<dyn Error>> {
    let source_project = fs::canonicalize(&fixture.project_path)?
        .to_string_lossy()
        .into_owned();
    let project_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, source_project.as_bytes());
    for path in [&fixture.session_path, &fixture.session_index_path] {
        let mut output = Vec::new();
        for line in fs::read_to_string(path)?
            .lines()
            .filter(|line| !line.is_empty())
        {
            let mut value = serde_json::from_str::<Value>(line)?;
            if value["type"] == "session_meta" {
                value["payload"]["project_id"] = Value::String(project_id.to_string());
                value["payload"]["cwd"] = Value::String(source_project.clone());
            } else {
                value["project_id"] = Value::String(project_id.to_string());
                value["cwd"] = Value::String(source_project.clone());
            }
            serde_json::to_writer(&mut output, &value)?;
            output.push(b'\n');
        }
        fs::write(path, output)?;
    }
    Connection::open(&fixture.state_db_path)?
        .execute("UPDATE threads SET cwd = ?1", [&source_project])?;
    Ok(())
}

fn snapshot_mutable_targets(
    plan: &RestorePlan,
) -> Result<BTreeMap<PathBuf, Option<String>>, Box<dyn Error>> {
    let mut paths = plan
        .operations
        .iter()
        .filter(|operation| operation.rollback_required)
        .map(|operation| operation.target.clone())
        .collect::<Vec<_>>();
    if let Some(database) = plan
        .operations
        .iter()
        .find(|operation| operation.package_source == "codex/metadata/threads.json")
        .map(|operation| &operation.target)
    {
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut sidecar = database.as_os_str().to_owned();
            sidecar.push(suffix);
            paths.push(PathBuf::from(sidecar));
        }
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let hash = match fs::read(&path) {
                Ok(bytes) => Some(format!("{:x}", Sha256::digest(bytes))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            Ok((path, hash))
        })
        .collect()
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

fn rollback_quarantine_name(transaction_id: Uuid, operation_index: usize) -> String {
    format!(".codex-rehome-{transaction_id}-{operation_index:08}.rollback")
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(not(windows))]
fn windows_symlink_privilege_is_unavailable(_error: &std::io::Error) -> bool {
    false
}

#[cfg(windows)]
fn windows_symlink_privilege_is_unavailable(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 1314))
}

fn ready_restore_plan(harness: &RestoreHarness) -> Result<RestorePlan, Box<dyn Error>> {
    apply_restore(harness.plan.clone(), harness.options())?;
    let preview = inspect_package(&harness.plan.package_path)?;
    let planned = &harness.plan.sessions[0];
    let relative = planned
        .target
        .strip_prefix(&harness.plan.target_codex_home)?
        .to_string_lossy()
        .replace('\\', "/");
    let source = &preview.manifest.conversations[0];
    let target = TargetInventory {
        codex_home: harness.plan.target_codex_home.clone(),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![ConversationEntry {
            task_id: planned.target_task_id,
            project_id: source.project_id,
            title: planned.title.clone(),
            updated_at: source.updated_at.clone(),
            content_hash: planned.expected_final_content_hash.clone(),
            archive_path: format!("codex/{relative}"),
            classification: None,
        }],
    };
    let plan = build_restore_plan(&preview, &target, &harness.plan.projects_root)?;
    assert!(plan.sessions.iter().all(|session| matches!(
        session.action,
        rehome_desktop_lib::core::models::SessionAction::Skip
    )));
    assert!(!plan.operations.iter().any(|operation| matches!(
        operation.package_source.as_str(),
        "codex/session_index.jsonl" | "codex/metadata/threads.json"
    )));
    Ok(plan)
}
