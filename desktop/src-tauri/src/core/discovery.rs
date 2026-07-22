use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{CodexInventory, ContentCounts, SourceOs},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryContext {
    pub codex_home_env: Option<PathBuf>,
    pub user_profile: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl DiscoveryContext {
    fn from_process_environment() -> Self {
        Self {
            codex_home_env: env::var_os("CODEX_HOME").map(PathBuf::from),
            user_profile: env::var_os("USERPROFILE").map(PathBuf::from),
            home: env::var_os("HOME").map(PathBuf::from),
        }
    }
}

pub fn resolve_codex_home(
    override_home: Option<PathBuf>,
    context: &DiscoveryContext,
) -> Result<PathBuf, RehomeError> {
    resolve_codex_home_for_os(override_home, context, current_source_os())
}

pub fn resolve_codex_home_for_os(
    override_home: Option<PathBuf>,
    context: &DiscoveryContext,
    source_os: SourceOs,
) -> Result<PathBuf, RehomeError> {
    let platform_default = match source_os {
        SourceOs::Windows => nonempty_path(context.user_profile.clone())
            .or_else(|| nonempty_path(context.home.clone())),
        SourceOs::Macos => nonempty_path(context.home.clone()),
    };

    override_home
        .or_else(|| nonempty_path(context.codex_home_env.clone()))
        .or_else(|| platform_default.map(|path| path.join(".codex")))
        .ok_or_else(|| {
            RehomeError::new(
                ErrorCode::CodexNotFound,
                "Codex home could not be resolved from the environment",
            )
        })
}

pub fn discover_codex(override_home: Option<PathBuf>) -> Result<CodexInventory, RehomeError> {
    discover_codex_with_context(override_home, &DiscoveryContext::from_process_environment())
}

pub fn discover_codex_with_context(
    override_home: Option<PathBuf>,
    context: &DiscoveryContext,
) -> Result<CodexInventory, RehomeError> {
    let codex_home = resolve_codex_home(override_home, context)?;
    let codex_home_is_real_directory = fs::symlink_metadata(&codex_home)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false);
    if !codex_home_is_real_directory {
        return Err(RehomeError::new(
            ErrorCode::CodexNotFound,
            "Codex home does not exist, is not a directory, or is a symbolic link",
        ));
    }

    let mut warnings = Vec::new();
    let mut conversation_paths = collect_files(
        &codex_home.join("sessions"),
        |path| extension_is(path, "jsonl"),
        "sessions",
        &mut warnings,
    );
    conversation_paths.extend(collect_files(
        &codex_home.join("archived_sessions"),
        |path| extension_is(path, "jsonl"),
        "archived sessions",
        &mut warnings,
    ));
    conversation_paths.sort();

    let skill_paths = collect_files(
        &codex_home.join("skills"),
        |path| file_name_is(path, "SKILL.md"),
        "skills",
        &mut warnings,
    );
    let plugin_paths = collect_files(
        &codex_home.join("plugins").join("cache"),
        |path| file_name_is(path, "plugin.json") || file_name_is(path, "manifest.json"),
        "plugins",
        &mut warnings,
    );
    let generated_image_paths = collect_files(
        &codex_home.join("generated_images"),
        |_| true,
        "generated images",
        &mut warnings,
    );

    let session_index = codex_home.join("session_index.jsonl");
    let session_index_path = discover_session_index(&session_index, &mut warnings);
    let state_db_path = newest_state_database(&codex_home, &mut warnings);
    if state_db_path.is_none() {
        warnings.push("Optional Codex state database was not found".to_owned());
    }

    let mut project_paths = Vec::new();
    let mut seen_projects = HashSet::new();
    read_global_project_roots(
        &codex_home.join(".codex-global-state.json"),
        &mut project_paths,
        &mut seen_projects,
        &mut warnings,
    );

    let sqlite_threads = state_db_path
        .as_deref()
        .map(|path| {
            read_state_database_roots(path, &mut project_paths, &mut seen_projects, &mut warnings)
        })
        .unwrap_or(0);

    Ok(CodexInventory {
        codex_home,
        source_os: current_source_os(),
        source_arch: env::consts::ARCH.to_owned(),
        source_device_id: Uuid::nil(),
        counts: ContentCounts {
            projects: project_paths.len() as u64,
            project_files: 0,
            conversations: conversation_paths.len() as u64,
            skills: skill_paths.len() as u64,
            plugins: plugin_paths.len() as u64,
            generated_images: generated_image_paths.len() as u64,
            sqlite_threads,
        },
        projects: Vec::new(),
        project_paths,
        conversations: Vec::new(),
        conversation_paths,
        session_index_path,
        state_db_path,
        skill_paths,
        plugin_paths,
        generated_image_paths,
        warnings,
    })
}

fn nonempty_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.filter(|value| !value.as_os_str().is_empty())
}

fn discover_session_index(path: &Path, warnings: &mut Vec<String>) -> Option<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            warnings
                .push("Optional Codex session index is a symbolic link and was ignored".to_owned());
            None
        }
        Ok(metadata) if metadata.is_file() => {
            validate_session_index(path, warnings);
            Some(path.to_path_buf())
        }
        Ok(_) => {
            warnings.push("Optional Codex session index is not a regular file".to_owned());
            None
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            warnings.push("Optional Codex session index was not found".to_owned());
            None
        }
        Err(_) => {
            warnings.push("Could not inspect optional Codex session index".to_owned());
            None
        }
    }
}

fn validate_session_index(path: &Path, warnings: &mut Vec<String>) {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => {
            warnings.push("Could not read optional Codex session index".to_owned());
            return;
        }
    };

    let malformed = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .fold(false, |malformed, line| {
            let invalid = serde_json::from_str::<Value>(line.trim())
                .map(|value| !value.is_object())
                .unwrap_or(true);
            malformed || invalid
        });
    if malformed {
        warnings.push("Optional Codex session index contains malformed JSONL".to_owned());
    }
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}

fn collect_files(
    root: &Path,
    matches: impl Fn(&Path) -> bool + Copy,
    label: &str,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_inner(root, matches, label, warnings, &mut files);
    files.sort();
    files
}

fn collect_files_inner(
    root: &Path,
    matches: impl Fn(&Path) -> bool + Copy,
    label: &str,
    warnings: &mut Vec<String>,
    files: &mut Vec<PathBuf>,
) {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            warnings.push(format!("Could not inspect optional {label} data"));
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        if matches(root) {
            files.push(root.to_path_buf());
        }
        return;
    }
    if !metadata.is_dir() {
        return;
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => {
            warnings.push(format!("Could not read optional {label} data"));
            return;
        }
    };
    let mut children = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    children.sort();
    for child in children {
        collect_files_inner(&child, matches, label, warnings, files);
    }
}

fn newest_state_database(codex_home: &Path, warnings: &mut Vec<String>) -> Option<PathBuf> {
    let entries = match fs::read_dir(codex_home) {
        Ok(entries) => entries,
        Err(_) => {
            warnings.push("Could not list Codex home for state databases".to_owned());
            return None;
        }
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("state_") || !name.ends_with(".sqlite") {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            warnings.push("Could not inspect an optional state database".to_owned());
            continue;
        };
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            candidates.push((metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), path));
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates.pop().map(|(_, path)| path)
}

fn read_global_project_roots(
    path: &Path,
    projects: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    warnings: &mut Vec<String>,
) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            warnings.push("Optional Codex global state metadata is not a regular file".to_owned());
            return;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            warnings.push("Optional Codex global state metadata was not found".to_owned());
            return;
        }
        Err(_) => {
            warnings.push("Could not inspect optional Codex global state metadata".to_owned());
            return;
        }
    }
    let value = match fs::read(path)
        .ok()
        .and_then(|contents| serde_json::from_slice::<Value>(&contents).ok())
    {
        Some(Value::Object(value)) => value,
        _ => {
            warnings.push("Could not parse optional Codex global state metadata".to_owned());
            return;
        }
    };

    for key in [
        "electron-saved-workspace-roots",
        "project-order",
        "active-workspace-roots",
    ] {
        let Some(raw) = value.get(key) else {
            continue;
        };
        let Some(items) = raw.as_array() else {
            warnings.push(format!("Ignored invalid {key} project metadata"));
            continue;
        };
        for item in items {
            if let Some(path) = item.as_str() {
                push_unique_path(path, projects, seen);
            } else {
                warnings.push(format!("Ignored a non-path entry in {key}"));
            }
        }
    }

    if let Some(raw_hints) = value.get("thread-workspace-root-hints") {
        if let Some(hints) = raw_hints.as_object() {
            for path in hints.values() {
                if let Some(path) = path.as_str() {
                    push_unique_path(path, projects, seen);
                } else {
                    warnings.push("Ignored a non-path thread workspace hint".to_owned());
                }
            }
        } else {
            warnings.push("Ignored invalid thread workspace root hints".to_owned());
        }
    }
}

fn read_state_database_roots(
    path: &Path,
    projects: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    warnings: &mut Vec<String>,
) -> u64 {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(_) => {
            warnings.push("Could not open the newest Codex state database read-only".to_owned());
            return 0;
        }
    };

    let count = match connection.query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0)) {
        Ok(count) => count,
        Err(_) => {
            warnings.push("Could not count threads in the newest Codex state database".to_owned());
            return 0;
        }
    };

    let mut statement = match connection.prepare("SELECT cwd FROM threads ORDER BY rowid") {
        Ok(statement) => statement,
        Err(_) => {
            warnings.push(
                "Could not read project roots from the newest Codex state database".to_owned(),
            );
            return count;
        }
    };
    let rows = match statement.query_map([], |row| row.get::<_, Option<String>>(0)) {
        Ok(rows) => rows,
        Err(_) => {
            warnings.push(
                "Could not read project roots from the newest Codex state database".to_owned(),
            );
            return count;
        }
    };
    for row in rows {
        match row {
            Ok(Some(path)) => push_unique_path(&path, projects, seen),
            Ok(None) => {}
            Err(_) => warnings.push("Ignored an unreadable thread project root".to_owned()),
        }
    }
    count
}

fn push_unique_path(raw: &str, projects: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if raw.is_empty() {
        return;
    }
    let path = PathBuf::from(raw);
    if seen.insert(path.clone()) {
        projects.push(path);
    }
}

fn extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn file_name_is(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}
