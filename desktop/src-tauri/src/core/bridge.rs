use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, PlannedOperation, PlannedSession, ReferenceRewrite, RegistrationStatus,
        RestorePlan, SessionAction, SourceOs,
    },
    package::inspect_package_for_planning,
    planner::rewrite_jsonl_payload,
};
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::Path,
    path::PathBuf,
    process::Command,
};
use tempfile::NamedTempFile;

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
    "title",
    "updated_at",
];
const THREAD_IMPORT_FIELDS: &[&str] = &[
    "id",
    "cwd",
    "rollout_path",
    "title",
    "updated_at",
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
    let verified = inspect_package_for_planning(&plan.package_path)?;
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
        ensure_safe_codex_target(&plan.target_codex_home, &session.target)?;
        validate_operation_state(operation)?;
        if session.action == SessionAction::Skip {
            continue;
        }
        ensure_writable_change(operation)?;
        let bytes = verified
            .planning_payloads
            .get(&session.package_source)
            .ok_or_else(|| {
                package_invalid("planned session payload is missing from the package")
            })?;
        let rewritten =
            rewrite_session_jsonl(bytes, &plan.reference_rewrites, &session.package_source)?;
        let final_hash = sha256_hex(&rewritten);
        if !final_hash.eq_ignore_ascii_case(&session.expected_final_content_hash) {
            return Err(restore_failed(format!(
                "planned session transformation hash changed for {}",
                session.target.display()
            )));
        }
        validate_operation_state(operation)?;
        atomic_write(&session.target, &rewritten)?;
        sessions_written += 1;
    }

    let mut index_entries_merged = 0;
    if let Some(operation) = operation_for(plan, "codex/session_index.jsonl") {
        ensure_safe_codex_target(&plan.target_codex_home, &operation.target)?;
        validate_operation_state(operation)?;
        ensure_writable_change(operation)?;
        let package_bytes = verified
            .planning_payloads
            .get("codex/session_index.jsonl")
            .ok_or_else(|| package_invalid("planned session index payload is missing"))?;
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
        validate_operation_state(operation)?;
        atomic_write(&operation.target, &merged)?;
        index_entries_merged = plan.sessions.len();
    }

    let mut sqlite_threads_imported = 0;
    if let Some(operation) = operation_for(plan, "codex/metadata/threads.json") {
        ensure_safe_codex_target(&plan.target_codex_home, &operation.target)?;
        validate_operation_state(operation)?;
        ensure_writable_change(operation)?;
        let package_bytes = verified
            .planning_payloads
            .get("codex/metadata/threads.json")
            .ok_or_else(|| package_invalid("planned thread metadata payload is missing"))?;
        sqlite_threads_imported = import_sqlite_threads(
            &operation.target,
            package_bytes,
            &plan.sessions,
            &plan.reference_rewrites,
        )?;
    }

    Ok(BridgeApplyReport {
        sessions_written,
        index_entries_merged,
        sqlite_threads_imported,
    })
}

pub fn merge_session_index(
    target_bytes: &[u8],
    package_bytes: &[u8],
    sessions: &[PlannedSession],
    rewrites: &[ReferenceRewrite],
) -> Result<Vec<u8>, RehomeError> {
    let rewritten_package =
        rewrite_jsonl_payload(package_bytes, rewrites, "codex/session_index.jsonl")?;
    let mut target = parse_index(target_bytes, false)?;
    let imported = parse_index(&rewritten_package, true)?;
    let planned = sessions
        .iter()
        .map(|session| (session.target_task_id.to_string(), session))
        .collect::<HashMap<_, _>>();

    for (id, session) in planned {
        let incoming = imported.get(&id).ok_or_else(|| {
            package_invalid(format!(
                "session index is missing planned conversation {id}"
            ))
        })?;
        let row = target
            .entry(id.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        let object = row.as_object_mut().ok_or_else(|| {
            restore_failed(format!("target session index row {id} is not an object"))
        })?;
        let incoming = incoming.as_object().ok_or_else(|| {
            package_invalid(format!("package session index row {id} is not an object"))
        })?;
        for field in INDEX_IMPORT_FIELDS {
            if let Some(value) = incoming.get(*field) {
                object.insert((*field).to_owned(), value.clone());
            }
        }
        object.remove("thread_id");
        object.remove("conversation_id");
        object.insert("id".into(), Value::String(id));
        object.insert("title".into(), Value::String(session.title.clone()));
        object.insert(
            "rollout_path".into(),
            Value::String(path_text(&session.target)?.to_owned()),
        );
    }

    let mut output = Vec::new();
    for row in target.into_values() {
        serde_json::to_writer(&mut output, &row).map_err(|error| {
            restore_failed(format!("could not encode target session index: {error}"))
        })?;
        output.push(b'\n');
    }
    Ok(output)
}

pub fn import_sqlite_threads(
    database: &Path,
    package_bytes: &[u8],
    sessions: &[PlannedSession],
    rewrites: &[ReferenceRewrite],
) -> Result<usize, RehomeError> {
    let rows = package_thread_rows(package_bytes, sessions, rewrites)?;
    let mut connection = Connection::open(database).map_err(|error| {
        restore_failed(format!(
            "could not open target Codex state database {}: {error}",
            database.display()
        ))
    })?;
    let available = thread_columns(&connection)?;
    if !available.contains("id") {
        return Err(restore_failed(
            "target Codex threads table has no id column",
        ));
    }
    let transaction = connection
        .transaction()
        .map_err(|error| restore_failed(format!("could not start Codex thread import: {error}")))?;
    for row in &rows {
        upsert_thread(&transaction, row, &available)?;
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

fn thread_columns(connection: &Connection) -> Result<HashSet<String>, RehomeError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| restore_failed(format!("could not inspect Codex threads: {error}")))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| restore_failed(format!("could not inspect Codex threads: {error}")))?;
    let mut result = HashSet::new();
    for column in columns {
        result.insert(
            column
                .map_err(|error| {
                    restore_failed(format!("could not inspect Codex thread column: {error}"))
                })?
                .to_ascii_lowercase(),
        );
    }
    Ok(result)
}

fn upsert_thread(
    connection: &Connection,
    row: &Map<String, Value>,
    available: &HashSet<String>,
) -> Result<(), RehomeError> {
    let columns = THREAD_IMPORT_FIELDS
        .iter()
        .copied()
        .filter(|column| available.contains(*column) && row.contains_key(*column))
        .collect::<Vec<_>>();
    if !columns.contains(&"id") {
        return Err(package_invalid("thread metadata row is missing its id"));
    }
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let updates = columns
        .iter()
        .copied()
        .filter(|column| *column != "id")
        .map(|column| format!("{column} = excluded.{column}"))
        .collect::<Vec<_>>();
    let conflict = if updates.is_empty() {
        "DO NOTHING".to_owned()
    } else {
        format!("DO UPDATE SET {}", updates.join(", "))
    };
    let sql = format!(
        "INSERT INTO threads ({}) VALUES ({placeholders}) ON CONFLICT(id) {conflict}",
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
            if metadata.file_type().is_symlink() || !metadata.is_file() {
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
    let mut current = root.to_path_buf();
    if fs::symlink_metadata(&current)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(restore_failed(
            "planned Codex home cannot be a symbolic link",
        ));
    }
    for component in relative.parent().into_iter().flat_map(Path::components) {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
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

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RehomeError> {
    let parent = path
        .parent()
        .ok_or_else(|| restore_failed("bridge target has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| {
        restore_failed(format!(
            "could not create bridge target directory {}: {error}",
            parent.display()
        ))
    })?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| {
        restore_failed(format!("could not create bridge temporary file: {error}"))
    })?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| {
            restore_failed(format!("could not write bridge temporary file: {error}"))
        })?;
    replace_file(temporary.path(), path).map_err(|error| {
        restore_failed(format!(
            "could not atomically replace bridge target {}: {error}",
            path.display()
        ))
    })?;
    drop(temporary);
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
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

fn parse_index(bytes: &[u8], package: bool) -> Result<BTreeMap<String, Value>, RehomeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        if package {
            package_invalid("session index is not UTF-8")
        } else {
            restore_failed("target session index is not UTF-8")
        }
    })?;
    let mut rows = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|error| {
            if package {
                package_invalid(format!("session index JSONL is invalid: {error}"))
            } else {
                restore_failed(format!("target session index JSONL is invalid: {error}"))
            }
        })?;
        let id = metadata_id(&value).ok_or_else(|| {
            if package {
                package_invalid("session index entry is missing its conversation ID")
            } else {
                restore_failed("target session index entry is missing its conversation ID")
            }
        })?;
        if package && rows.contains_key(id) {
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
