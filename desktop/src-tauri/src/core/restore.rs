use crate::core::{
    backup::{
        prepare_transaction, record_applied_hashes, rollback_prepared, update_status,
        PreparedTransaction,
    },
    bridge::{apply_bridge_plan, apply_file_operation, register_project_with_detected_cli},
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, PendingRecovery, RecoveryStatus, ReferenceRewriteKind, RegistrationStatus,
        RestoreOptions, RestorePlan, RestoreReport, RollbackReport, SourceOs, VerificationReport,
    },
    package::{inspect_package_for_planning, VerifiedPackage},
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io, path::Path};
use uuid::Uuid;

const SESSION_INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREAD_METADATA_SOURCE: &str = "codex/metadata/threads.json";

pub fn apply_restore(
    plan: RestorePlan,
    options: RestoreOptions,
) -> Result<RestoreReport, RehomeError> {
    if !options.codex_closed_confirmed {
        return Err(RehomeError::new(
            ErrorCode::CodexRunning,
            "restore requires explicit confirmation that Codex is closed",
        ));
    }
    validate_plan(&plan)?;
    let verified = inspect_package_for_planning(&plan.package_path)?;
    validate_package_identity(&plan, &verified)?;
    let mut transaction = prepare_transaction(&plan, &options.backup_root)?;

    let result = apply_transaction(&plan, &options, &verified, &mut transaction);
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

pub fn recover_incomplete_transactions() -> Result<Vec<PendingRecovery>, RehomeError> {
    crate::core::backup::recover_incomplete_transactions()
}

fn apply_transaction(
    plan: &RestorePlan,
    options: &RestoreOptions,
    verified: &VerifiedPackage,
    transaction: &mut PreparedTransaction,
) -> Result<RestoreReport, RehomeError> {
    update_status(transaction, RecoveryStatus::Applying)?;
    let (mut restored_files, mut restored_bytes) = apply_regular_files(plan, verified)?;
    let bridge = apply_bridge_plan(plan).map_err(|error| {
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
    let verification = verify_restore(plan, options, verified)?;
    if !data_verification_passed(&verification) {
        return Err(restore_failed(format!(
            "restore verification did not pass: {verification:?}"
        )));
    }
    record_applied_hashes(transaction)?;
    update_status(transaction, RecoveryStatus::Committed)?;

    Ok(RestoreReport {
        transaction_id: transaction.journal.transaction_id,
        package_id: plan.package_id,
        completed_at: timestamp(),
        restored_files,
        restored_bytes,
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
) -> Result<(u64, u64), RehomeError> {
    let mut restored_files = 0_u64;
    let mut restored_bytes = 0_u64;
    for operation in &plan.operations {
        if !matches!(operation.action, ChangeKind::Add | ChangeKind::Update)
            || is_bridge_operation(plan, &operation.package_source)
        {
            continue;
        }
        let bytes = verified.authenticated_payload(&operation.package_source)?;
        let root = operation_root(plan, &operation.target)?;
        apply_file_operation(root, operation, &bytes)?;
        restored_files += 1;
        restored_bytes = restored_bytes
            .checked_add(bytes.len() as u64)
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
    options: &RestoreOptions,
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
    let app_registration_valid = verify_registration(plan, options, &current)?;
    let mut report = VerificationReport {
        package_checksum_valid,
        files_valid,
        sessions_valid,
        session_index_valid: bridge.session_index_valid,
        sqlite_threads_valid: bridge.sqlite_threads_valid,
        path_mapping_valid: bridge.path_mapping_valid,
        forbidden_files_absent,
        project_files_valid,
        app_registration_valid,
        app_visible_ready: false,
    };
    report.app_visible_ready = data_verification_passed(&report) && app_registration_valid;
    Ok(report)
}

fn verify_plain_files(plan: &RestorePlan, verified: &VerifiedPackage) -> Result<bool, RehomeError> {
    for operation in &plan.operations {
        if !matches!(operation.action, ChangeKind::Add | ChangeKind::Update)
            || is_bridge_operation(plan, &operation.package_source)
        {
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

fn verify_project_files(
    plan: &RestorePlan,
    verified: &VerifiedPackage,
) -> Result<bool, RehomeError> {
    for operation in plan
        .operations
        .iter()
        .filter(|operation| operation.target.starts_with(&plan.projects_root))
        .filter(|operation| matches!(operation.action, ChangeKind::Add | ChangeKind::Update))
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
    let has_index_operation = plan
        .operations
        .iter()
        .any(|operation| operation.package_source == SESSION_INDEX_SOURCE);
    let has_sqlite_operation = plan
        .operations
        .iter()
        .any(|operation| operation.package_source == THREAD_METADATA_SOURCE);
    let mut index_valid = !has_index_operation;
    let mut sqlite_valid = !has_sqlite_operation;
    let mut mapping_valid = true;

    if has_index_operation {
        index_valid = plan.sessions.iter().all(|session| {
            index_rows
                .get(&session.target_task_id.to_string())
                .is_some_and(|row| {
                    row.get("rollout_path").and_then(Value::as_str) == session.target.to_str()
                })
        });
    }
    if has_sqlite_operation {
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
                && (!has_index_operation || index_cwd == Some(*expected))
                && (!has_sqlite_operation || sqlite_cwd == Some(*expected))
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
    let Some(operation) = plan
        .operations
        .iter()
        .find(|operation| operation.package_source == SESSION_INDEX_SOURCE)
    else {
        return Ok(BTreeMap::new());
    };
    let bytes = fs::read(&operation.target).map_err(|error| {
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
    let Some(operation) = plan
        .operations
        .iter()
        .find(|operation| operation.package_source == THREAD_METADATA_SOURCE)
    else {
        return Ok(BTreeMap::new());
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(&operation.target, flags).map_err(|error| {
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

fn verify_registration(
    plan: &RestorePlan,
    options: &RestoreOptions,
    verified: &VerifiedPackage,
) -> Result<bool, RehomeError> {
    if !options.register_projects {
        return Ok(false);
    }
    let target_os = if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    };
    Ok(verified.preview.manifest.projects.iter().all(|project| {
        register_project_with_detected_cli(target_os, &plan.projects_root.join(&project.name))
            == RegistrationStatus::Registered
    }))
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

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn restore_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}
