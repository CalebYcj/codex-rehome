use crate::core::{
    bridge::register_project_with_detected_cli,
    discovery::discover_codex as core_discover_codex,
    error::{ErrorCode, RehomeError},
    models::{
        CodexInventory, CreatePackageReport, CreatePackageRequest, PackagePreview, RecoveryStatus,
        RegistrationStatus, RestoreOptions, RestorePlan, RestoreReport, RollbackReport, SourceOs,
        TargetInventory, TransactionSummary,
    },
    package::{create_package as core_create_package, inspect_package as core_inspect_package},
    planner::build_restore_plan as core_build_restore_plan,
    restore::{
        apply_restore_by_id, list_transactions as core_list_transactions, rollback as core_rollback,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

#[tauri::command]
pub fn discover_codex(override_home: Option<PathBuf>) -> Result<CodexInventory, RehomeError> {
    core_discover_codex(override_home)
}

#[tauri::command]
pub fn create_package(request: CreatePackageRequest) -> Result<CreatePackageReport, RehomeError> {
    core_create_package(request)
}

#[tauri::command]
pub fn inspect_package(path: PathBuf) -> Result<PackagePreview, RehomeError> {
    core_inspect_package(&path)
}

#[tauri::command]
pub fn build_restore_plan(
    package_path: PathBuf,
    target_codex_home: PathBuf,
    projects_root: PathBuf,
) -> Result<RestorePlan, RehomeError> {
    let package = core_inspect_package(&package_path)?;
    let inventory = core_discover_codex(Some(target_codex_home))?;
    let target = TargetInventory {
        codex_home: inventory.codex_home,
        target_os: inventory.source_os,
        target_arch: inventory.source_arch,
        counts: inventory.counts,
        projects: inventory.projects,
        conversations: inventory.conversations,
    };
    core_build_restore_plan(&package, &target, &projects_root)
}

#[tauri::command]
pub fn apply_restore(plan_id: Uuid, options: RestoreOptions) -> Result<RestoreReport, RehomeError> {
    apply_restore_by_id(plan_id, options)
}

#[tauri::command]
pub fn list_transactions() -> Result<Vec<TransactionSummary>, RehomeError> {
    core_list_transactions()
}

#[tauri::command]
pub fn rollback_transaction(transaction_id: Uuid) -> Result<RollbackReport, RehomeError> {
    let transaction = transaction(transaction_id)?;
    if transaction.status != RecoveryStatus::Committed {
        return Err(RehomeError::new(
            ErrorCode::RollbackFailed,
            "only committed transactions can be rolled back from history",
        ));
    }
    core_rollback(transaction_id)
}

#[tauri::command]
pub fn open_path(
    app: AppHandle,
    path: PathBuf,
    transaction_id: Option<Uuid>,
) -> Result<(), RehomeError> {
    let canonical = authorize_open_path(&path, transaction_id, false)?;
    app.opener()
        .reveal_item_in_dir(canonical)
        .map_err(|error| open_failed(format!("could not reveal path: {error}")))
}

#[tauri::command]
pub fn open_restored_thread(
    path: PathBuf,
    transaction_id: Uuid,
) -> Result<RegistrationStatus, RehomeError> {
    let canonical = authorize_open_path(&path, Some(transaction_id), true)?;
    Ok(register_project_with_detected_cli(
        current_source_os(),
        &canonical,
    ))
}

fn transaction(transaction_id: Uuid) -> Result<TransactionSummary, RehomeError> {
    core_list_transactions()?
        .into_iter()
        .find(|transaction| transaction.transaction_id == transaction_id)
        .ok_or_else(|| open_failed("transaction was not found"))
}

fn authorize_open_path(
    path: &Path,
    transaction_id: Option<Uuid>,
    restored_only: bool,
) -> Result<PathBuf, RehomeError> {
    let canonical = canonical_existing(path)?;
    let Some(transaction_id) = transaction_id else {
        if !restored_only
            && canonical
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rehome"))
        {
            core_inspect_package(&canonical)?;
            return Ok(canonical);
        }
        return Err(open_failed("path is not an application-owned package"));
    };

    let transaction = transaction(transaction_id)?;
    authorize_transaction_path(&canonical, &transaction, restored_only)?;
    Ok(canonical)
}

fn authorize_transaction_path(
    canonical: &Path,
    transaction: &TransactionSummary,
    restored_only: bool,
) -> Result<(), RehomeError> {
    let exact_restored_project = transaction.restored_project_paths.iter().any(|path| {
        fs::canonicalize(path).is_ok_and(|canonical_project| canonical_project == canonical)
    });
    let exact_transaction_backup = !restored_only
        && fs::canonicalize(&transaction.transaction_backup_path)
            .is_ok_and(|canonical_backup| canonical_backup == canonical);

    if exact_restored_project || exact_transaction_backup {
        Ok(())
    } else {
        Err(open_failed(
            "path is not an exact object owned by the selected transaction",
        ))
    }
}

fn canonical_existing(path: &Path) -> Result<PathBuf, RehomeError> {
    if !path.is_absolute() {
        return Err(open_failed("path must be absolute"));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| open_failed(format!("could not inspect path: {error}")))?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(open_failed("path is not a regular file or directory"));
    }
    fs::canonicalize(path)
        .map_err(|error| open_failed(format!("could not canonicalize path: {error}")))
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}

fn open_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}

#[cfg(test)]
mod tests {
    use super::authorize_transaction_path;
    use crate::core::models::{RecoveryStatus, TransactionSummary};
    use std::fs;
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
