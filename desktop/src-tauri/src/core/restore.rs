use crate::core::{
    backup::{
        ensure_applied_states, prepare_transaction, record_applied_mutation, rollback_prepared,
        update_status, PreparedTransaction,
    },
    bridge::{
        apply_bridge_plan_for_transaction, apply_file_source_for_transaction,
        register_project_with_detected_cli,
    },
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, PendingRecovery, ProjectRegistration, RecoveryStatus, ReferenceRewriteKind,
        RegistrationStatus, RestoreOptions, RestorePlan, RestoreReport, RollbackReport, SourceOs,
        TransactionHistory, TransactionSummary, VerificationReport,
    },
    package::{inspect_package_for_planning, VerifiedPackage},
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io, path::Path};
use tempfile::NamedTempFile;
use uuid::Uuid;

const SESSION_INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREAD_METADATA_SOURCE: &str = "codex/metadata/threads.json";

pub fn apply_restore(
    plan: RestorePlan,
    options: RestoreOptions,
) -> Result<RestoreReport, RehomeError> {
    let plan = crate::core::plan_store::load_exact(&plan)?;
    apply_server_plan(plan, options, |target_os, project| {
        register_project_with_detected_cli(target_os, project)
    })
}

pub fn apply_restore_by_id(
    plan_id: Uuid,
    options: RestoreOptions,
) -> Result<RestoreReport, RehomeError> {
    let plan = crate::core::plan_store::load(plan_id)?;
    apply_server_plan(plan, options, |target_os, project| {
        register_project_with_detected_cli(target_os, project)
    })
}

pub fn apply_restore_with_registrar(
    plan: RestorePlan,
    options: RestoreOptions,
    registrar: impl FnMut(SourceOs, &Path) -> RegistrationStatus,
) -> Result<RestoreReport, RehomeError> {
    let plan = crate::core::plan_store::load_exact(&plan)?;
    apply_server_plan(plan, options, registrar)
}

fn apply_server_plan(
    plan: RestorePlan,
    options: RestoreOptions,
    mut registrar: impl FnMut(SourceOs, &Path) -> RegistrationStatus,
) -> Result<RestoreReport, RehomeError> {
    if !options.codex_closed_confirmed {
        return Err(RehomeError::new(
            ErrorCode::CodexRunning,
            "restore requires confirmation that current Codex work is saved",
        ));
    }
    validate_plan(&plan)?;
    let verified = inspect_package_for_planning(&plan.package_path)?;
    validate_package_identity(&plan, &verified)?;
    if verified.preview.forbidden_files_total > 0 {
        return Err(RehomeError::new(
            ErrorCode::PackageInvalid,
            "restore package contains forbidden files",
        ));
    }
    validate_preserved_targets(&plan)?;
    let mut transaction = prepare_transaction(&plan, &options.backup_root)?;

    let result = apply_transaction(&plan, &options, &verified, &mut transaction, &mut registrar);
    match result {
        Ok(report) => Ok(report),
        Err(error) => match rollback_prepared(&mut transaction) {
            Ok(_) => Err(error),
            Err(rollback_error) => Err(RehomeError::new(
                ErrorCode::RollbackFailed,
                format!(
                    "restore failed: {}; automatic rollback failed: {}",
                    error.message, rollback_error.message
                ),
            )),
        },
    }
}

pub fn rollback(transaction_id: Uuid) -> Result<RollbackReport, RehomeError> {
    crate::core::backup::rollback(transaction_id)
}

pub fn list_transactions() -> Result<Vec<TransactionSummary>, RehomeError> {
    crate::core::backup::list_transactions()
}

pub fn list_transaction_history() -> Result<TransactionHistory, RehomeError> {
    crate::core::backup::list_transaction_history()
}

pub fn transaction_summary(
    transaction_id: Uuid,
) -> Result<Option<TransactionSummary>, RehomeError> {
    crate::core::backup::transaction_summary(transaction_id)
}

pub fn recover_incomplete_transactions() -> Result<Vec<PendingRecovery>, RehomeError> {
    crate::core::backup::recover_incomplete_transactions()
}

fn apply_transaction(
    plan: &RestorePlan,
    options: &RestoreOptions,
    verified: &VerifiedPackage,
    transaction: &mut PreparedTransaction,
    registrar: &mut impl FnMut(SourceOs, &Path) -> RegistrationStatus,
) -> Result<RestoreReport, RehomeError> {
    update_status(transaction, RecoveryStatus::Applying)?;
    let transaction_id = transaction.journal.transaction_id;
    let (mut restored_files, mut restored_bytes) =
        apply_regular_files(plan, verified, transaction)?;
    let bridge = apply_bridge_plan_for_transaction(plan, transaction_id, |target| {
        record_applied_mutation(transaction, target)
    })
    .map_err(|error| {
        if plan
            .operations
            .iter()
            .any(|operation| operation.package_source == THREAD_METADATA_SOURCE)
        {
            RehomeError::new(
                error.code,
                format!("SQLite/index bridge update failed: {}", error.message),
            )
        } else {
            error
        }
    })?;
    restored_files += (bridge.sessions_written
        + bridge.index_entries_merged
        + bridge.sqlite_threads_imported) as u64;
    restored_bytes += changed_target_bytes(plan)?;

    update_status(transaction, RecoveryStatus::Verifying)?;
    let mut verification = verify_restore(plan, verified)?;
    if !data_verification_passed(&verification) {
        // Verification opens the restored SQLite database after the bridge has
        // recorded its first applied state. In WAL mode that read can create
        // fresh sidecars, so refresh the journal before rollback decides
        // whether a sidecar belongs to this transaction.
        if let Some(operation) = plan
            .operations
            .iter()
            .find(|operation| operation.package_source == THREAD_METADATA_SOURCE)
        {
            record_applied_mutation(transaction, &operation.target)?;
        }
        return Err(restore_failed(format!(
            "restore verification did not pass: {verification:?}"
        )));
    }
    ensure_applied_states(transaction)?;
    update_status(transaction, RecoveryStatus::Committed)?;
    let registrations = register_projects(plan, options, verified, registrar);
    verification.app_registration_valid = options.register_projects
        && registrations
            .iter()
            .all(|result| result.status == RegistrationStatus::Registered);
    verification.app_visible_ready =
        data_verification_passed(&verification) && verification.app_registration_valid;

    Ok(RestoreReport {
        transaction_id: transaction.journal.transaction_id,
        package_id: plan.package_id,
        completed_at: timestamp(),
        restored_files,
        restored_bytes,
        registrations,
        verification,
    })
}

fn validate_plan(plan: &RestorePlan) -> Result<(), RehomeError> {
    if !plan.package_path.is_absolute()
        || !plan.target_codex_home.is_absolute()
        || !plan.projects_root.is_absolute()
    {
        return Err(restore_failed("restore plan paths must be absolute"));
    }
    if plan.conflict_count > 0
        || plan
            .operations
            .iter()
            .any(|operation| operation.action == ChangeKind::Conflict)
    {
        return Err(RehomeError::new(
            ErrorCode::ProjectConflict,
            "restore plan contains unresolved conflicts",
        ));
    }
    if plan.target_codex_home.starts_with(&plan.projects_root)
        || plan.projects_root.starts_with(&plan.target_codex_home)
    {
        return Err(restore_failed("restore roots must not overlap"));
    }
    Ok(())
}

fn validate_package_identity(
    plan: &RestorePlan,
    verified: &VerifiedPackage,
) -> Result<(), RehomeError> {
    if verified.preview.manifest.package_id != plan.package_id {
        return Err(RehomeError::new(
            ErrorCode::PackageInvalid,
            "restore plan package ID does not match the package",
        ));
    }
    if !verified
        .preview
        .archive_hash
        .eq_ignore_ascii_case(&plan.archive_hash)
    {
        return Err(RehomeError::new(
            ErrorCode::PackageInvalid,
            "restore plan archive hash does not match the package",
        ));
    }
    for operation in &plan.operations {
        if !verified.payloads.contains_key(&operation.package_source) {
            return Err(RehomeError::new(
                ErrorCode::PackageInvalid,
                format!(
                    "restore operation references a missing package payload: {}",
                    operation.package_source
                ),
            ));
        }
    }
    Ok(())
}

fn apply_regular_files(
    plan: &RestorePlan,
    verified: &VerifiedPackage,
    transaction: &mut PreparedTransaction,
) -> Result<(u64, u64), RehomeError> {
    let mut restored_files = 0_u64;
    let mut restored_bytes = 0_u64;
    for operation in &plan.operations {
        if !matches!(operation.action, ChangeKind::Add | ChangeKind::Update)
            || is_bridge_operation(plan, &operation.package_source)
        {
            continue;
        }
        let mut staged = NamedTempFile::new().map_err(|error| {
            restore_failed(format!("could not stage restored payload: {error}"))
        })?;
        let bytes = verified
            .write_authenticated_payload(&operation.package_source, staged.as_file_mut())?;
        staged.as_file().sync_all().map_err(|error| {
            restore_failed(format!("could not flush restored payload: {error}"))
        })?;
        let root = operation_root(plan, &operation.target)?;
        apply_file_source_for_transaction(
            root,
            operation,
            staged.path(),
            transaction.journal.transaction_id,
        )?;
        record_applied_mutation(transaction, &operation.target)?;
        restored_files += 1;
        restored_bytes = restored_bytes
            .checked_add(bytes)
            .ok_or_else(|| restore_failed("restored byte count overflowed"))?;
    }
    Ok((restored_files, restored_bytes))
}

fn is_bridge_operation(plan: &RestorePlan, source: &str) -> bool {
    source == SESSION_INDEX_SOURCE
        || source == THREAD_METADATA_SOURCE
        || plan
            .sessions
            .iter()
            .any(|session| session.package_source == source)
}

fn changed_target_bytes(plan: &RestorePlan) -> Result<u64, RehomeError> {
    plan.operations
        .iter()
        .filter(|operation| {
            matches!(operation.action, ChangeKind::Add | ChangeKind::Update)
                && is_bridge_operation(plan, &operation.package_source)
        })
        .try_fold(0_u64, |total, operation| {
            match fs::metadata(&operation.target) {
                Ok(metadata) if metadata.is_file() => total
                    .checked_add(metadata.len())
                    .ok_or_else(|| restore_failed("restored byte count overflowed")),
                Ok(_) => Err(restore_failed("restored target is not a regular file")),
                Err(error) => Err(restore_failed(format!(
                    "could not inspect restored target {}: {error}",
                    operation.target.display()
                ))),
            }
        })
}

fn verify_restore(
    plan: &RestorePlan,
    verified: &VerifiedPackage,
) -> Result<VerificationReport, RehomeError> {
    let current = inspect_package_for_planning(&plan.package_path)?;
    let package_checksum_valid = current.preview.checksum_valid
        && current.preview.manifest.package_id == plan.package_id
        && current
            .preview
            .archive_hash
            .eq_ignore_ascii_case(&plan.archive_hash);
    let files_valid = verify_plain_files(plan, verified)?;
    let sessions_valid = plan.sessions.iter().try_fold(true, |valid, session| {
        Ok::<_, RehomeError>(
            valid
                && hash_optional_file(&session.target)?.is_some_and(|hash| {
                    hash.eq_ignore_ascii_case(&session.expected_final_content_hash)
                }),
        )
    })?;
    let bridge = verify_bridge_metadata(plan)?;
    let forbidden_files_absent = current.preview.forbidden_files_total == 0;
    let project_files_valid = verify_project_files(plan, verified)?;
    Ok(VerificationReport {
        package_checksum_valid,
        files_valid,
        sessions_valid,
        session_index_valid: bridge.session_index_valid,
        sqlite_threads_valid: bridge.sqlite_threads_valid,
        path_mapping_valid: bridge.path_mapping_valid,
        forbidden_files_absent,
        project_files_valid,
        app_registration_valid: false,
        app_visible_ready: false,
    })
}

fn verify_plain_files(plan: &RestorePlan, verified: &VerifiedPackage) -> Result<bool, RehomeError> {
    for operation in &plan.operations {
        if operation.action == ChangeKind::Conflict
            || is_bridge_operation(plan, &operation.package_source)
        {
            continue;
        }
        if operation.action == ChangeKind::Preserve {
            if !preserved_target_matches(operation)? {
                return Ok(false);
            }
            continue;
        }
        let expected = &verified
            .payloads
            .get(&operation.package_source)
            .ok_or_else(|| restore_failed("verified payload metadata is missing"))?
            .content_hash;
        if !hash_optional_file(&operation.target)?
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_preserved_targets(plan: &RestorePlan) -> Result<(), RehomeError> {
    for operation in plan
        .operations
        .iter()
        .filter(|operation| operation.action == ChangeKind::Preserve)
    {
        if !preserved_target_matches(operation)? {
            return Err(restore_failed(format!(
                "preserved target changed after planning: {}",
                operation.target.display()
            )));
        }
    }
    Ok(())
}

fn preserved_target_matches(
    operation: &crate::core::models::PlannedOperation,
) -> Result<bool, RehomeError> {
    let metadata = match fs::symlink_metadata(&operation.target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(operation.expected_previous_hash.is_none())
        }
        Err(error) => {
            return Err(restore_failed(format!(
                "could not inspect preserved target {}: {error}",
                operation.target.display()
            )))
        }
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Ok(false);
    }
    let Some(expected) = operation.expected_previous_hash.as_deref() else {
        return Ok(false);
    };
    Ok(hash_optional_file(&operation.target)?
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected)))
}

fn verify_project_files(
    plan: &RestorePlan,
    verified: &VerifiedPackage,
) -> Result<bool, RehomeError> {
    for operation in plan
        .operations
        .iter()
        .filter(|operation| operation.target.starts_with(&plan.projects_root))
        .filter(|operation| operation.action != ChangeKind::Conflict)
    {
        let expected = &verified
            .payloads
            .get(&operation.package_source)
            .ok_or_else(|| restore_failed("project payload metadata is missing"))?
            .content_hash;
        if !hash_optional_file(&operation.target)?
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

struct BridgeVerification {
    session_index_valid: bool,
    sqlite_threads_valid: bool,
    path_mapping_valid: bool,
}

fn verify_bridge_metadata(plan: &RestorePlan) -> Result<BridgeVerification, RehomeError> {
    let index_rows = read_index_rows(plan)?;
    let sqlite_rows = read_sqlite_rows(plan)?;
    let requires_index = plan.bridge_verification.session_index.is_some();
    let requires_sqlite = plan.bridge_verification.sqlite_database.is_some();
    let mut index_valid = !requires_index;
    let mut sqlite_valid = !requires_sqlite;
    let mut mapping_valid = true;

    if requires_index {
        index_valid = plan.sessions.iter().all(|session| {
            index_rows
                .get(&session.target_task_id.to_string())
                .is_some_and(|row| {
                    row.get("rollout_path").and_then(Value::as_str) == session.target.to_str()
                })
        });
    }
    if requires_sqlite {
        sqlite_valid = plan.sessions.iter().all(|session| {
            sqlite_rows
                .get(&session.target_task_id.to_string())
                .is_some_and(|(_, rollout)| rollout.as_deref() == session.target.to_str())
        });
    }

    for session in &plan.sessions {
        let expected_project_paths = plan
            .reference_rewrites
            .iter()
            .filter(|rewrite| {
                rewrite.source_task_id == session.source_task_id
                    && rewrite.kind == ReferenceRewriteKind::ProjectPath
                    && rewrite.package_source == session.package_source
            })
            .map(|rewrite| rewrite.to.as_str())
            .collect::<Vec<_>>();
        if expected_project_paths.is_empty() {
            continue;
        }
        let session_bytes = fs::read(&session.target).map_err(|error| {
            restore_failed(format!(
                "could not read restored session for verification: {error}"
            ))
        })?;
        let session_values = parse_jsonl_values(&session_bytes)?;
        let index_cwd = index_rows
            .get(&session.target_task_id.to_string())
            .and_then(|row| row.get("cwd"))
            .and_then(Value::as_str);
        let sqlite_cwd = sqlite_rows
            .get(&session.target_task_id.to_string())
            .and_then(|(cwd, _)| cwd.as_deref());
        let mapped = expected_project_paths.iter().any(|expected| {
            session_values
                .iter()
                .any(|value| json_contains_string(value, expected))
                // Some Codex versions use a minimal session_index row containing only
                // id/title/timestamps/rollout_path. In that schema the authoritative
                // project binding is the session metadata plus the SQLite thread row;
                // do not require a cwd field that the index does not expose.
                && (!requires_index || index_cwd.is_none_or(|cwd| cwd == *expected))
                && (!requires_sqlite || sqlite_cwd == Some(*expected))
        });
        mapping_valid &= mapped;
    }

    Ok(BridgeVerification {
        session_index_valid: index_valid,
        sqlite_threads_valid: sqlite_valid,
        path_mapping_valid: mapping_valid,
    })
}

fn parse_jsonl_values(bytes: &[u8]) -> Result<Vec<Value>, RehomeError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| restore_failed("restored session JSONL is not UTF-8"))?;
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| {
                restore_failed(format!("restored session JSONL is invalid: {error}"))
            })
        })
        .collect()
}

fn json_contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(value) => value == expected,
        Value::Array(values) => values
            .iter()
            .any(|value| json_contains_string(value, expected)),
        Value::Object(values) => values
            .values()
            .any(|value| json_contains_string(value, expected)),
        _ => false,
    }
}

fn read_index_rows(plan: &RestorePlan) -> Result<BTreeMap<String, Value>, RehomeError> {
    let Some(target) = plan.bridge_verification.session_index.as_deref() else {
        return Ok(BTreeMap::new());
    };
    let bytes = fs::read(target).map_err(|error| {
        restore_failed(format!("could not read restored session index: {error}"))
    })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| restore_failed("restored session index is not UTF-8"))?;
    let mut rows = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|error| {
            restore_failed(format!("restored session index is invalid: {error}"))
        })?;
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            rows.insert(id.to_owned(), value);
        }
    }
    Ok(rows)
}

type SqliteRows = BTreeMap<String, (Option<String>, Option<String>)>;

fn read_sqlite_rows(plan: &RestorePlan) -> Result<SqliteRows, RehomeError> {
    let Some(target) = plan.bridge_verification.sqlite_database.as_deref() else {
        return Ok(BTreeMap::new());
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(target, flags).map_err(|error| {
        restore_failed(format!("could not open restored SQLite database: {error}"))
    })?;
    let mut statement = connection
        .prepare("SELECT id, cwd, rollout_path FROM threads")
        .map_err(|error| {
            restore_failed(format!("could not inspect restored SQLite rows: {error}"))
        })?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|error| {
            restore_failed(format!("could not query restored SQLite rows: {error}"))
        })?;
    let mut result = BTreeMap::new();
    for row in rows {
        let (id, cwd, rollout) = row.map_err(|error| {
            restore_failed(format!("could not read restored SQLite row: {error}"))
        })?;
        result.insert(id, (cwd, rollout));
    }
    Ok(result)
}

fn register_projects(
    plan: &RestorePlan,
    options: &RestoreOptions,
    verified: &VerifiedPackage,
    registrar: &mut impl FnMut(SourceOs, &Path) -> RegistrationStatus,
) -> Vec<ProjectRegistration> {
    if !options.register_projects {
        return Vec::new();
    }
    let target_os = if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    };
    verified
        .preview
        .manifest
        .projects
        .iter()
        .map(|project| {
            let project_path = plan.projects_root.join(&project.name);
            let status = registrar(target_os, &project_path);
            ProjectRegistration {
                project_id: project.project_id,
                project_path,
                status,
            }
        })
        .collect()
}

fn data_verification_passed(report: &VerificationReport) -> bool {
    report.package_checksum_valid
        && report.files_valid
        && report.sessions_valid
        && report.session_index_valid
        && report.sqlite_threads_valid
        && report.path_mapping_valid
        && report.forbidden_files_absent
        && report.project_files_valid
}

fn operation_root<'a>(plan: &'a RestorePlan, target: &Path) -> Result<&'a Path, RehomeError> {
    if target.starts_with(&plan.target_codex_home) {
        Ok(&plan.target_codex_home)
    } else if target.starts_with(&plan.projects_root) {
        Ok(&plan.projects_root)
    } else {
        Err(restore_failed(format!(
            "restore target escapes the planned roots: {}",
            target.display()
        )))
    }
}

fn hash_optional_file(path: &Path) -> Result<Option<String>, RehomeError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(restore_failed(format!(
                "could not read restored file {}: {error}",
                path.display()
            )))
        }
    };
    Ok(Some(format!("{:x}", Sha256::digest(bytes))))
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink()
        || metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
}

#[cfg(not(windows))]
fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn restore_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}
