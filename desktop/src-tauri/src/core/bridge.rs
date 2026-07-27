use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, PlannedOperation, PlannedSession, ReferenceRewrite, RegistrationStatus,
        RestorePlan, SessionAction, SourceOs,
    },
    package::inspect_package_for_planning,
    planner::rewrite_jsonl_payload,
    stable_fs::PinnedParent,
};
use chrono::DateTime;
use rusqlite::{
    params_from_iter, types::Value as SqlValue, Connection, OpenFlags, OptionalExtension,
    TransactionBehavior,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::Path,
    path::PathBuf,
    process::Command,
    time::Duration,
};
use uuid::Uuid;

const INDEX_IMPORT_FIELDS: &[&str] = &[
    "archived",
    "cwd",
    "has_user_event",
    "preview",
    "project",
    "project_id",
    "project_path",
    "rollout",
    "rollout_path",
    "thread_name",
    "title",
    "updated_at",
];
const INDEX_REPAIR_FIELDS: &[&str] = &[
    "cwd",
    "project",
    "project_id",
    "project_path",
    "rollout",
    "rollout_path",
];
const THREAD_IMPORT_FIELDS: &[&str] = &[
    "id",
    "cwd",
    "rollout_path",
    "title",
    "created_at",
    "updated_at",
    "source",
    "model_provider",
    "sandbox_policy",
    "approval_mode",
    "archived",
    "has_user_event",
    "preview",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRunError {
    Unavailable,
    InvocationFailed { message: String },
}

pub trait CommandRunner {
    fn run(&self, command: &Path, arguments: &[OsString]) -> Result<(), CommandRunError>;
}

pub struct SystemCommandRunner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeApplyReport {
    pub sessions_written: usize,
    pub index_entries_merged: usize,
    pub sqlite_threads_imported: usize,
}

impl CommandRunner for SystemCommandRunner {
    fn run(&self, command: &Path, arguments: &[OsString]) -> Result<(), CommandRunError> {
        let output = Command::new(command)
            .args(arguments)
            .output()
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    CommandRunError::Unavailable
                } else {
                    CommandRunError::InvocationFailed {
                        message: error.to_string(),
                    }
                }
            })?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            format!("Codex app command exited with {}", output.status)
        } else {
            stderr
        };
        Err(CommandRunError::InvocationFailed { message })
    }
}

pub fn register_project(
    target_os: SourceOs,
    detected_cli: Option<&Path>,
    project: &Path,
    runner: &impl CommandRunner,
) -> RegistrationStatus {
    let Some(command) = detected_cli else {
        return match target_os {
            SourceOs::Macos => RegistrationStatus::CommandUnavailable,
            SourceOs::Windows => RegistrationStatus::ManualOpenRequired,
        };
    };
    let arguments = [OsString::from("app"), project.as_os_str().to_owned()];
    match runner.run(command, &arguments) {
        Ok(()) => RegistrationStatus::Registered,
        Err(CommandRunError::Unavailable) => RegistrationStatus::CommandUnavailable,
        Err(CommandRunError::InvocationFailed { message }) => {
            RegistrationStatus::InvocationFailed { message }
        }
    }
}

pub fn register_project_with_detected_cli(
    target_os: SourceOs,
    project: &Path,
) -> RegistrationStatus {
    let command = detect_registration_cli(target_os);
    register_project(target_os, command.as_deref(), project, &SystemCommandRunner)
}

pub fn detect_registration_cli(target_os: SourceOs) -> Option<PathBuf> {
    let candidates = match target_os {
        SourceOs::Macos => vec![PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex",
        )],
        SourceOs::Windows => windows_cli_candidates(),
    };
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn windows_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let root = PathBuf::from(local_app_data);
        candidates.push(root.join("Programs").join("Codex").join("codex.exe"));
        candidates.push(root.join("Codex").join("codex.exe"));
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            candidates.push(directory.join("codex.exe"));
            candidates.push(directory.join("codex.cmd"));
        }
    }
    candidates
}

pub fn rewrite_session_jsonl(
    bytes: &[u8],
    rewrites: &[ReferenceRewrite],
    package_source: &str,
) -> Result<Vec<u8>, RehomeError> {
    rewrite_jsonl_payload(bytes, rewrites, package_source)
}

pub fn apply_bridge_plan(plan: &RestorePlan) -> Result<BridgeApplyReport, RehomeError> {
    apply_bridge_plan_with_lock_token(plan, None, |_| Ok(()))
}

pub(crate) fn apply_bridge_plan_for_transaction(
    plan: &RestorePlan,
    transaction_id: Uuid,
    on_applied: impl FnMut(&Path) -> Result<(), RehomeError>,
) -> Result<BridgeApplyReport, RehomeError> {
    let token = transaction_id.to_string();
    apply_bridge_plan_with_lock_token(plan, Some(&token), on_applied)
}

fn apply_bridge_plan_with_lock_token(
    plan: &RestorePlan,
    lock_token: Option<&str>,
    mut on_applied: impl FnMut(&Path) -> Result<(), RehomeError>,
) -> Result<BridgeApplyReport, RehomeError> {
    let verified = inspect_package_for_planning(&plan.package_path)?;
    if verified.preview.archive_hash != plan.archive_hash {
        return Err(package_invalid(
            "restore plan archive hash does not match the package on disk",
        ));
    }
    if verified.preview.manifest.package_id != plan.package_id {
        return Err(package_invalid(
            "restore plan package ID does not match the package on disk",
        ));
    }
    let mut sessions_written = 0;
    for session in &plan.sessions {
        let operation = required_operation(plan, &session.package_source)?;
        if operation.target != session.target {
            return Err(restore_failed(
                "planned session operation target does not match the planned session",
            ));
        }
        if session.action == SessionAction::Skip {
            ensure_safe_codex_target(&plan.target_codex_home, &session.target)?;
            validate_operation_state(operation)?;
            continue;
        }
        ensure_writable_change(operation)?;
        let bytes = verified.authenticated_planning_payload(&session.package_source)?;
        let rewritten =
            rewrite_session_jsonl(bytes, &plan.reference_rewrites, &session.package_source)?;
        let final_hash = sha256_hex(&rewritten);
        if !final_hash.eq_ignore_ascii_case(&session.expected_final_content_hash) {
            return Err(restore_failed(format!(
                "planned session transformation hash changed for {}",
                session.target.display()
            )));
        }
        let guard =
            TargetReplacementGuard::acquire(&plan.target_codex_home, operation, lock_token)?;
        guard.commit_bytes(operation, &rewritten)?;
        on_applied(&operation.target)?;
        sessions_written += 1;
    }

    let mut index_entries_merged = 0;
    if let Some(operation) = operation_for(plan, "codex/session_index.jsonl") {
        ensure_writable_change(operation)?;
        let guard =
            TargetReplacementGuard::acquire(&plan.target_codex_home, operation, lock_token)?;
        let package_bytes = verified.authenticated_planning_payload("codex/session_index.jsonl")?;
        let target_bytes = match fs::read(&operation.target) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(restore_failed(format!(
                    "could not read target session index {}: {error}",
                    operation.target.display()
                )))
            }
        };
        let merged = merge_session_index(
            &target_bytes,
            package_bytes,
            &plan.sessions,
            &plan.reference_rewrites,
        )?;
        guard.commit_bytes(operation, &merged)?;
        on_applied(&operation.target)?;
        index_entries_merged = plan.sessions.len();
    }

    let mut sqlite_threads_imported = 0;
    if let Some(operation) = operation_for(plan, "codex/metadata/threads.json") {
        ensure_writable_change(operation)?;
        let package_bytes =
            verified.authenticated_planning_payload("codex/metadata/threads.json")?;
        let import_result = import_sqlite_threads_for_operation(
            &plan.target_codex_home,
            operation,
            package_bytes,
            &plan.sessions,
            &plan.reference_rewrites,
            lock_token,
        );
        on_applied(&operation.target)?;
        sqlite_threads_imported = import_result?;
    }

    Ok(BridgeApplyReport {
        sessions_written,
        index_entries_merged,
        sqlite_threads_imported,
    })
}

pub(crate) fn apply_file_operation_for_transaction(
    root: &Path,
    operation: &PlannedOperation,
    bytes: &[u8],
    transaction_id: Uuid,
) -> Result<(), RehomeError> {
    let token = transaction_id.to_string();
    apply_file_operation_with_lock_token(root, operation, bytes, Some(&token))
}

fn apply_file_operation_with_lock_token(
    root: &Path,
    operation: &PlannedOperation,
    bytes: &[u8],
    lock_token: Option<&str>,
) -> Result<(), RehomeError> {
    ensure_writable_change(operation)?;
    let guard = TargetReplacementGuard::acquire(root, operation, lock_token)?;
    guard.commit_bytes(operation, bytes)
}

pub(crate) fn validate_restore_target(root: &Path, target: &Path) -> Result<(), RehomeError> {
    validate_restore_target_ancestry(root, target)?;
    reject_hard_linked_target(target)
}

pub(crate) fn validate_restore_target_ancestry(
    root: &Path,
    target: &Path,
) -> Result<(), RehomeError> {
    ensure_safe_codex_target(root, target)
}

fn reject_hard_linked_target(path: &Path) -> Result<(), RehomeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(restore_failed(format!(
                "restore target is not a regular file: {}",
                path.display()
            )))
        }
        Ok(_) => {
            let links = file_link_count(path).map_err(|error| {
                restore_failed(format!("could not inspect restore target links: {error}"))
            })?;
            if links > 1 {
                Err(restore_failed(format!(
                    "restore target has more than one hard link: {}",
                    path.display()
                )))
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(restore_failed(format!(
            "could not inspect restore target {}: {error}",
            path.display()
        ))),
    }
}

pub fn merge_session_index(
    target_bytes: &[u8],
    package_bytes: &[u8],
    sessions: &[PlannedSession],
    rewrites: &[ReferenceRewrite],
) -> Result<Vec<u8>, RehomeError> {
    let rewritten_package =
        rewrite_jsonl_payload(package_bytes, rewrites, "codex/session_index.jsonl")?;
    let target = parse_target_index(target_bytes)?;
    let imported = parse_package_index(&rewritten_package)?;
    let planned = sessions
        .iter()
        .map(|session| (session.target_task_id.to_string(), session))
        .collect::<BTreeMap<_, _>>();
    let mut target_bases = BTreeMap::<String, Value>::new();
    for row in &target {
        let Some(id) = &row.id else {
            continue;
        };
        if !planned.contains_key(id) {
            continue;
        }
        let Some(candidate) = row.value.as_ref() else {
            continue;
        };
        match target_bases.entry(id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidate.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if compare_metadata_freshness(candidate, entry.get()).is_gt() {
                    entry.insert(candidate.clone());
                }
            }
        }
    }

    let mut merged_rows = BTreeMap::new();
    for (id, session) in &planned {
        let incoming = imported.get(id).ok_or_else(|| {
            package_invalid(format!(
                "session index is missing planned conversation {id}"
            ))
        })?;
        let mut row = target_bases
            .remove(id)
            .unwrap_or_else(|| Value::Object(Map::new()));
        let object = row.as_object_mut().ok_or_else(|| {
            restore_failed(format!("target session index row {id} is not an object"))
        })?;
        let incoming = incoming.as_object().ok_or_else(|| {
            package_invalid(format!("package session index row {id} is not an object"))
        })?;
        let incoming_is_older = incoming_metadata_is_older(object, incoming);
        for field in INDEX_IMPORT_FIELDS {
            if !incoming_is_older || INDEX_REPAIR_FIELDS.contains(field) {
                if let Some(value) = incoming.get(*field) {
                    object.insert((*field).to_owned(), value.clone());
                }
            }
        }
        object.remove("thread_id");
        object.remove("conversation_id");
        object.insert("id".into(), Value::String(id.clone()));
        if !incoming_is_older && incoming.contains_key("title") {
            object.insert("title".into(), Value::String(session.title.clone()));
        }
        if !incoming_is_older && incoming.contains_key("thread_name") {
            object.insert("thread_name".into(), Value::String(session.title.clone()));
        }
        object.insert(
            "rollout_path".into(),
            Value::String(path_text(&session.target)?.to_owned()),
        );
        merged_rows.insert(id.clone(), row);
    }

    let mut output = Vec::new();
    let mut emitted = HashSet::new();
    for row in target {
        let Some(id) = row.id else {
            output.extend_from_slice(&row.raw);
            continue;
        };
        let Some(merged) = merged_rows.get(&id) else {
            output.extend_from_slice(&row.raw);
            continue;
        };
        if emitted.insert(id) {
            write_index_row(&mut output, merged)?;
        }
    }
    for (id, row) in merged_rows {
        if emitted.insert(id) {
            if output.last().is_some_and(|byte| *byte != b'\n') {
                output.push(b'\n');
            }
            write_index_row(&mut output, &row)?;
        }
    }
    Ok(output)
}

fn incoming_metadata_is_older(target: &Map<String, Value>, incoming: &Map<String, Value>) -> bool {
    compare_updated_at(target.get("updated_at"), incoming.get("updated_at")).is_gt()
}

fn compare_metadata_freshness(left: &Value, right: &Value) -> std::cmp::Ordering {
    let left = left.as_object().and_then(|row| row.get("updated_at"));
    let right = right.as_object().and_then(|row| row.get("updated_at"));
    compare_updated_at(left, right)
}

fn compare_updated_at(left: Option<&Value>, right: Option<&Value>) -> std::cmp::Ordering {
    let left = left.and_then(Value::as_str);
    let right = right.and_then(Value::as_str);
    match (left, right) {
        (Some(left), Some(right)) => {
            match (
                DateTime::parse_from_rfc3339(left),
                DateTime::parse_from_rfc3339(right),
            ) {
                (Ok(left), Ok(right)) => left.cmp(&right),
                _ => left.cmp(right),
            }
        }
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn write_index_row(output: &mut Vec<u8>, row: &Value) -> Result<(), RehomeError> {
    serde_json::to_writer(&mut *output, row).map_err(|error| {
        restore_failed(format!("could not encode target session index: {error}"))
    })?;
    output.push(b'\n');
    Ok(())
}

pub fn import_sqlite_threads(
    database: &Path,
    package_bytes: &[u8],
    sessions: &[PlannedSession],
    rewrites: &[ReferenceRewrite],
) -> Result<usize, RehomeError> {
    let parent = database
        .parent()
        .ok_or_else(|| restore_failed("target Codex state database has no parent directory"))?;
    ensure_safe_codex_target(parent, database)?;
    reject_hard_linked_sqlite(database)?;
    let operation = PlannedOperation {
        package_source: "codex/metadata/threads.json".into(),
        target: database.to_path_buf(),
        expected_previous_hash: Some(hash_file(database)?),
        action: ChangeKind::Update,
        rollback_required: true,
    };
    import_sqlite_threads_for_operation(parent, &operation, package_bytes, sessions, rewrites, None)
}

fn import_sqlite_threads_for_operation(
    root: &Path,
    operation: &PlannedOperation,
    package_bytes: &[u8],
    sessions: &[PlannedSession],
    rewrites: &[ReferenceRewrite],
    lock_token: Option<&str>,
) -> Result<usize, RehomeError> {
    ensure_safe_codex_target(root, &operation.target)?;
    reject_hard_linked_sqlite(&operation.target)?;
    let identity = sqlite_file_identity(&operation.target)?;
    let rows = package_thread_rows(package_bytes, sessions, rewrites)?;
    let _guard = TargetReplacementGuard::acquire(root, operation, lock_token)?;
    reject_hard_linked_sqlite(&operation.target)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut connection =
        Connection::open_with_flags(&operation.target, flags).map_err(|error| {
            restore_failed(format!(
                "could not open target Codex state database {}: {error}",
                operation.target.display()
            ))
        })?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| restore_failed(format!("could not configure SQLite locking: {error}")))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            restore_failed(format!(
                "could not lock Codex state database for import: {error}"
            ))
        })?;
    ensure_safe_codex_target(root, &operation.target)?;
    reject_hard_linked_sqlite(&operation.target)?;
    if sqlite_file_identity(&operation.target)? != identity {
        return Err(restore_failed(format!(
            "target Codex state database changed identity after planning: {}",
            operation.target.display()
        )));
    }
    validate_operation_state(operation)?;
    let schema = thread_columns(&transaction)?;
    if !schema.contains_key("id") {
        return Err(restore_failed(
            "target Codex threads table has no id column",
        ));
    }
    for row in &rows {
        import_thread_row(&transaction, row, &schema)?;
    }
    transaction.commit().map_err(|error| {
        restore_failed(format!("could not commit Codex thread import: {error}"))
    })?;
    Ok(rows.len())
}

fn package_thread_rows(
    package_bytes: &[u8],
    sessions: &[PlannedSession],
    rewrites: &[ReferenceRewrite],
) -> Result<Vec<Map<String, Value>>, RehomeError> {
    let values = serde_json::from_slice::<Value>(package_bytes)
        .map_err(|error| package_invalid(format!("bridge metadata JSON is invalid: {error}")))?
        .as_array()
        .cloned()
        .ok_or_else(|| package_invalid("thread metadata must be a JSON array"))?;
    let planned = sessions
        .iter()
        .map(|session| (session.source_task_id.to_string(), session))
        .collect::<HashMap<_, _>>();
    if planned.len() != sessions.len() {
        return Err(restore_failed(
            "restore plan contains duplicate source conversation IDs",
        ));
    }
    let mut target_ids = HashSet::new();
    let mut result = Vec::with_capacity(values.len());
    let mut source_ids = HashSet::new();
    for value in values {
        let source_id = metadata_id(&value)
            .ok_or_else(|| package_invalid("thread metadata row is missing its conversation ID"))?
            .to_owned();
        if !source_ids.insert(source_id.clone()) {
            return Err(package_invalid(
                "thread metadata contains duplicate conversation IDs",
            ));
        }
        let session = planned.get(&source_id).ok_or_else(|| {
            package_invalid(format!(
                "thread metadata references unplanned conversation {source_id}"
            ))
        })?;
        if !target_ids.insert(session.target_task_id) {
            return Err(restore_failed(
                "restore plan contains duplicate target conversation IDs",
            ));
        }
        let selected = rewrites
            .iter()
            .filter(|rewrite| {
                rewrite.package_source == "codex/metadata/threads.json"
                    && rewrite.source_task_id == session.source_task_id
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut line = serde_json::to_vec(&value).map_err(|error| {
            package_invalid(format!("could not encode bridge metadata row: {error}"))
        })?;
        line.push(b'\n');
        let rewritten = rewrite_jsonl_payload(&line, &selected, "codex/metadata/threads.json")?;
        let mut object = serde_json::from_slice::<Value>(&rewritten)
            .map_err(|error| package_invalid(format!("could not decode bridge metadata: {error}")))?
            .as_object()
            .cloned()
            .ok_or_else(|| package_invalid("thread metadata row must be a JSON object"))?;
        object.remove("thread_id");
        object.remove("conversation_id");
        object.insert(
            "id".into(),
            Value::String(session.target_task_id.to_string()),
        );
        object.insert("title".into(), Value::String(session.title.clone()));
        object.insert(
            "rollout_path".into(),
            Value::String(path_text(&session.target)?.to_owned()),
        );
        result.push(object);
    }
    Ok(result)
}

#[derive(Debug)]
struct ThreadColumn {
    name: String,
    not_null: bool,
    has_default: bool,
}

fn thread_columns(connection: &Connection) -> Result<HashMap<String, ThreadColumn>, RehomeError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| restore_failed(format!("could not inspect Codex threads: {error}")))?;
    let columns = statement
        .query_map([], |row| {
            Ok(ThreadColumn {
                name: row.get(1)?,
                not_null: row.get::<_, i64>(3)? != 0,
                has_default: row.get::<_, Option<String>>(4)?.is_some(),
            })
        })
        .map_err(|error| restore_failed(format!("could not inspect Codex threads: {error}")))?;
    let mut result = HashMap::new();
    for column in columns {
        let column = column.map_err(|error| {
            restore_failed(format!("could not inspect Codex thread column: {error}"))
        })?;
        result.insert(column.name.to_ascii_lowercase(), column);
    }
    Ok(result)
}

fn import_thread_row(
    connection: &Connection,
    row: &Map<String, Value>,
    schema: &HashMap<String, ThreadColumn>,
) -> Result<(), RehomeError> {
    let columns = THREAD_IMPORT_FIELDS
        .iter()
        .copied()
        .filter(|column| schema.contains_key(*column) && row.contains_key(*column))
        .collect::<Vec<_>>();
    if !columns.contains(&"id") {
        return Err(package_invalid("thread metadata row is missing its id"));
    }
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| package_invalid("thread metadata row id is not a string"))?;
    let exists = connection
        .query_row("SELECT 1 FROM threads WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map_err(|error| restore_failed(format!("could not inspect Codex thread row: {error}")))?
        .is_some();
    if exists {
        return update_thread(connection, row, &columns, id);
    }

    let missing_required = schema
        .iter()
        .filter(|(name, column)| {
            column.not_null && !column.has_default && !columns.contains(&name.as_str())
        })
        .map(|(_, column)| column.name.as_str())
        .collect::<Vec<_>>();
    if !missing_required.is_empty() {
        return Err(restore_failed(format!(
            "cannot insert Codex thread because required target-only columns have no defaults: {}",
            missing_required.join(", ")
        )));
    }

    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO threads ({}) VALUES ({placeholders})",
        columns.join(", ")
    );
    let values = columns
        .iter()
        .map(|column| json_sql_value(&row[*column]))
        .collect::<Result<Vec<_>, _>>()?;
    connection
        .execute(&sql, params_from_iter(values))
        .map_err(|error| restore_failed(format!("could not import Codex thread row: {error}")))?;
    Ok(())
}

fn update_thread(
    connection: &Connection,
    row: &Map<String, Value>,
    columns: &[&str],
    id: &str,
) -> Result<(), RehomeError> {
    let updates = columns
        .iter()
        .copied()
        .filter(|column| *column != "id")
        .collect::<Vec<_>>();
    if updates.is_empty() {
        return Ok(());
    }
    let assignments = updates
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let id_parameter = updates.len() + 1;
    let sql = format!("UPDATE threads SET {assignments} WHERE id = ?{id_parameter}");
    let mut values = updates
        .iter()
        .map(|column| json_sql_value(&row[*column]))
        .collect::<Result<Vec<_>, _>>()?;
    values.push(SqlValue::Text(id.to_owned()));
    connection
        .execute(&sql, params_from_iter(values))
        .map_err(|error| restore_failed(format!("could not update Codex thread row: {error}")))?;
    Ok(())
}

fn json_sql_value(value: &Value) -> Result<SqlValue, RehomeError> {
    match value {
        Value::Null => Ok(SqlValue::Null),
        Value::Bool(value) => Ok(SqlValue::Integer(i64::from(*value))),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(SqlValue::Integer(value))
            } else if let Some(value) = value.as_u64() {
                i64::try_from(value)
                    .map(SqlValue::Integer)
                    .map_err(|_| package_invalid("thread integer exceeds SQLite range"))
            } else if let Some(value) = value.as_f64() {
                Ok(SqlValue::Real(value))
            } else {
                Err(package_invalid("thread number is not supported"))
            }
        }
        Value::String(value) => Ok(SqlValue::Text(value.clone())),
        Value::Array(_) | Value::Object(_) => Err(package_invalid(
            "thread metadata contains a non-scalar field",
        )),
    }
}

fn operation_for<'a>(plan: &'a RestorePlan, source: &str) -> Option<&'a PlannedOperation> {
    plan.operations
        .iter()
        .find(|operation| operation.package_source == source)
}

fn required_operation<'a>(
    plan: &'a RestorePlan,
    source: &str,
) -> Result<&'a PlannedOperation, RehomeError> {
    operation_for(plan, source)
        .ok_or_else(|| restore_failed(format!("restore plan is missing operation {source}")))
}

fn ensure_writable_change(operation: &PlannedOperation) -> Result<(), RehomeError> {
    if matches!(operation.action, ChangeKind::Add | ChangeKind::Update) {
        Ok(())
    } else {
        Err(restore_failed(format!(
            "bridge operation is not writable: {}",
            operation.target.display()
        )))
    }
}

fn validate_operation_state(operation: &PlannedOperation) -> Result<(), RehomeError> {
    match (
        &operation.expected_previous_hash,
        fs::symlink_metadata(&operation.target),
    ) {
        (None, Err(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        (None, Ok(_)) => Err(restore_failed(format!(
            "restore target appeared after planning: {}",
            operation.target.display()
        ))),
        (None, Err(error)) => Err(restore_failed(format!(
            "could not inspect restore target {}: {error}",
            operation.target.display()
        ))),
        (Some(_), Err(error)) if error.kind() == io::ErrorKind::NotFound => {
            Err(restore_failed(format!(
                "restore target disappeared after planning: {}",
                operation.target.display()
            )))
        }
        (Some(_), Err(error)) => Err(restore_failed(format!(
            "could not inspect restore target {}: {error}",
            operation.target.display()
        ))),
        (Some(expected), Ok(metadata)) => {
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
                return Err(restore_failed(format!(
                    "restore target is no longer a regular file: {}",
                    operation.target.display()
                )));
            }
            let actual = hash_file(&operation.target)?;
            if actual.eq_ignore_ascii_case(expected) {
                Ok(())
            } else {
                Err(restore_failed(format!(
                    "restore target changed after planning: {}",
                    operation.target.display()
                )))
            }
        }
    }
}

fn reject_hard_linked_sqlite(path: &Path) -> Result<(), RehomeError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        restore_failed(format!(
            "could not inspect target Codex state database {}: {error}",
            path.display()
        ))
    })?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(restore_failed(format!(
            "target Codex state database is not a regular unlinked file: {}",
            path.display()
        )));
    }
    let links = file_link_count(path).map_err(|error| {
        restore_failed(format!(
            "could not inspect target Codex state database links {}: {error}",
            path.display()
        ))
    })?;
    if links > 1 {
        return Err(restore_failed(format!(
            "target Codex state database has more than one hard link: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
struct SqliteFileIdentity {
    volume: u32,
    index_high: u32,
    index_low: u32,
}

#[cfg(windows)]
fn sqlite_file_identity(path: &Path) -> Result<SqliteFileIdentity, RehomeError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path).map_err(|error| {
        restore_failed(format!(
            "could not open target Codex state database identity {}: {error}",
            path.display()
        ))
    })?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if result == 0 {
        return Err(restore_failed(format!(
            "could not inspect target Codex state database identity {}: {}",
            path.display(),
            io::Error::last_os_error()
        )));
    }
    Ok(SqliteFileIdentity {
        volume: information.dwVolumeSerialNumber,
        index_high: information.nFileIndexHigh,
        index_low: information.nFileIndexLow,
    })
}

#[cfg(unix)]
fn sqlite_file_identity(path: &Path) -> Result<(u64, u64), RehomeError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).map_err(|error| {
        restore_failed(format!(
            "could not inspect target Codex state database identity {}: {error}",
            path.display()
        ))
    })?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(any(windows, unix)))]
fn sqlite_file_identity(path: &Path) -> Result<std::time::SystemTime, RehomeError> {
    fs::metadata(path)
        .and_then(|metadata| metadata.created())
        .map_err(|error| {
            restore_failed(format!(
                "could not inspect target Codex state database identity {}: {error}",
                path.display()
            ))
        })
}

#[cfg(windows)]
fn file_link_count(path: &Path) -> io::Result<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(u64::from(information.nNumberOfLinks))
    }
}

#[cfg(unix)]
fn file_link_count(path: &Path) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(path)?.nlink())
}

#[cfg(not(any(windows, unix)))]
fn file_link_count(path: &Path) -> io::Result<u64> {
    fs::metadata(path).map(|_| 1)
}

fn ensure_safe_codex_target(root: &Path, target: &Path) -> Result<(), RehomeError> {
    let relative = target.strip_prefix(root).map_err(|_| {
        restore_failed(format!(
            "bridge target escapes the planned Codex home: {}",
            target.display()
        ))
    })?;
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(restore_failed("bridge target path is unsafe"));
    }
    for ancestor in root.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() => {
                return Err(restore_failed(format!(
                    "planned Codex home ancestry is unsafe: {}",
                    ancestor.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(restore_failed(format!(
                    "could not inspect planned Codex home ancestry {}: {error}",
                    ancestor.display()
                )))
            }
        }
    }
    let mut current = root.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() => {
                return Err(restore_failed(format!(
                    "bridge target ancestor is unsafe: {}",
                    current.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(restore_failed(format!(
                    "could not inspect bridge target ancestor {}: {error}",
                    current.display()
                )))
            }
        }
    }
    Ok(())
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

struct TargetReplacementGuard<'a> {
    root: &'a Path,
    target: &'a Path,
    parent: PinnedParent,
    lock_path: PathBuf,
    lock_token: String,
    lock_file: Option<fs::File>,
}

impl<'a> TargetReplacementGuard<'a> {
    fn acquire(
        root: &'a Path,
        operation: &'a PlannedOperation,
        transaction_token: Option<&str>,
    ) -> Result<Self, RehomeError> {
        ensure_safe_codex_target(root, &operation.target)?;
        reject_hard_linked_target(&operation.target)?;
        let parent = operation
            .target
            .parent()
            .ok_or_else(|| restore_failed("bridge target has no parent directory"))?;
        fs::create_dir_all(parent).map_err(|error| {
            restore_failed(format!(
                "could not create bridge target directory {}: {error}",
                parent.display()
            ))
        })?;
        sync_directory(parent).map_err(|error| {
            restore_failed(format!("could not sync bridge target directory: {error}"))
        })?;
        ensure_safe_codex_target(root, &operation.target)?;
        reject_hard_linked_target(&operation.target)?;
        let file_name = operation
            .target
            .file_name()
            .ok_or_else(|| restore_failed("bridge target has no file name"))?
            .to_string_lossy();
        let lock_path = parent.join(format!(".{file_name}.codex-rehome.lock"));
        let lock_name = lock_path
            .file_name()
            .ok_or_else(|| restore_failed("target lock has no file name"))?;
        let lock_token = transaction_token
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let pinned_parent = PinnedParent::open(parent).map_err(|error| {
            restore_failed(format!("could not pin bridge target directory: {error}"))
        })?;
        let lock_file = pinned_parent
            .create_new_file(lock_name)
            .or_else(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    let mut existing = pinned_parent.open_file(lock_name)?;
                    let metadata = existing.metadata()?;
                    let mut existing_token = String::new();
                    existing.read_to_string(&mut existing_token)?;
                    if metadata_is_link_or_reparse(&metadata)
                        || !metadata.is_file()
                        || file_link_count(&lock_path)? != 1
                        || existing_token != lock_token
                    {
                        return Err(error);
                    }
                    pinned_parent.open_file_for_write(lock_name)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    restore_failed(format!(
                        "restore target is locked by another apply: {}",
                        operation.target.display()
                    ))
                } else {
                    restore_failed(format!(
                        "could not lock restore target {}: {error}",
                        operation.target.display()
                    ))
                }
            })?;
        let mut lock_file = lock_file;
        lock_file
            .write_all(lock_token.as_bytes())
            .and_then(|()| lock_file.sync_all())
            .map_err(|error| {
                restore_failed(format!("could not initialize target lock: {error}"))
            })?;
        sync_directory(parent)
            .map_err(|error| restore_failed(format!("could not sync target lock: {error}")))?;
        let guard = Self {
            root,
            target: &operation.target,
            parent: pinned_parent,
            lock_path,
            lock_token,
            lock_file: Some(lock_file),
        };
        ensure_safe_codex_target(root, &operation.target)?;
        validate_operation_state(operation)?;
        Ok(guard)
    }

    fn commit_bytes(&self, operation: &PlannedOperation, bytes: &[u8]) -> Result<(), RehomeError> {
        validate_operation_state(operation)?;
        ensure_safe_codex_target(self.root, self.target)?;
        reject_hard_linked_target(self.target)?;
        let name = self
            .target
            .file_name()
            .ok_or_else(|| restore_failed("bridge target has no file name"))?;
        self.parent.replace_bytes(name, bytes).map_err(|error| {
            restore_failed(format!(
                "could not atomically replace bridge target {}: {error}",
                self.target.display()
            ))
        })?;
        self.parent.sync().map_err(|error| {
            restore_failed(format!("could not sync bridge target directory: {error}"))
        })
    }
}

impl Drop for TargetReplacementGuard<'_> {
    fn drop(&mut self) {
        let Some(lock_name) = self.lock_path.file_name() else {
            return;
        };
        let owns_lock = self
            .parent
            .open_file(lock_name)
            .and_then(|mut file| {
                let mut token = String::new();
                file.read_to_string(&mut token)?;
                Ok(token == self.lock_token)
            })
            .unwrap_or(false);
        drop(self.lock_file.take());
        if owns_lock {
            let _ = self.parent.remove_file(lock_name);
        }
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, RehomeError> {
    let mut file = fs::File::open(path).map_err(|error| {
        restore_failed(format!(
            "could not read restore target {}: {error}",
            path.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(|error| {
        restore_failed(format!(
            "could not hash restore target {}: {error}",
            path.display()
        ))
    })?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
struct TargetIndexRow {
    raw: Vec<u8>,
    id: Option<String>,
    value: Option<Value>,
}

fn parse_target_index(bytes: &[u8]) -> Result<Vec<TargetIndexRow>, RehomeError> {
    std::str::from_utf8(bytes).map_err(|_| restore_failed("target session index is not UTF-8"))?;
    let mut rows = Vec::new();
    for raw in bytes.split_inclusive(|byte| *byte == b'\n') {
        let without_newline = raw.strip_suffix(b"\n").unwrap_or(raw);
        let json = without_newline
            .strip_suffix(b"\r")
            .unwrap_or(without_newline);
        if json.is_empty() {
            rows.push(TargetIndexRow {
                raw: raw.to_vec(),
                id: None,
                value: None,
            });
            continue;
        }
        let value: Value = serde_json::from_slice(json).map_err(|error| {
            restore_failed(format!("target session index JSONL is invalid: {error}"))
        })?;
        let id = metadata_id(&value)
            .ok_or_else(|| {
                restore_failed("target session index entry is missing its conversation ID")
            })?
            .to_owned();
        rows.push(TargetIndexRow {
            raw: raw.to_vec(),
            id: Some(id),
            value: Some(value),
        });
    }
    Ok(rows)
}

fn parse_package_index(bytes: &[u8]) -> Result<BTreeMap<String, Value>, RehomeError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| package_invalid("session index is not UTF-8"))?;
    let mut rows = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| package_invalid(format!("session index JSONL is invalid: {error}")))?;
        let id = metadata_id(&value)
            .ok_or_else(|| package_invalid("session index entry is missing its conversation ID"))?;
        if rows.contains_key(id) {
            return Err(package_invalid(
                "session index contains duplicate conversation IDs",
            ));
        }
        rows.insert(id.to_owned(), value);
    }
    Ok(rows)
}

fn metadata_id(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    ["id", "thread_id", "conversation_id"]
        .iter()
        .find_map(|field| object.get(*field).and_then(Value::as_str))
}

fn path_text(path: &std::path::Path) -> Result<&str, RehomeError> {
    path.to_str()
        .ok_or_else(|| restore_failed("planned session path is not valid UTF-8"))
}

fn package_invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}

fn restore_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}
