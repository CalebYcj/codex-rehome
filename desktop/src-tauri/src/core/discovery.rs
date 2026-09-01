use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{
        CodexInventory, ContentCounts, ConversationClassification, ConversationEntry,
        OptionalContentEntry, ProjectEntry, SourceOs,
    },
    paths::normalize_entry,
    session::{metadata_string, metadata_uuid, parse_session_metadata, SessionMetadata},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
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
    let platform_default = platform_user_home(context, source_os);

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

fn platform_user_home(context: &DiscoveryContext, source_os: SourceOs) -> Option<PathBuf> {
    match source_os {
        SourceOs::Windows => nonempty_path(context.user_profile.clone()),
        SourceOs::Macos => nonempty_path(context.home.clone()),
    }
}

pub fn discover_codex(override_home: Option<PathBuf>) -> Result<CodexInventory, RehomeError> {
    discover_codex_with_context(override_home, &DiscoveryContext::from_process_environment())
}

pub fn discover_codex_with_context(
    override_home: Option<PathBuf>,
    context: &DiscoveryContext,
) -> Result<CodexInventory, RehomeError> {
    let requested_codex_home = resolve_codex_home(override_home, context)?;
    let (codex_home, linked_home_resolved) = resolve_existing_codex_home(&requested_codex_home)?;
    let mut warnings = Vec::new();
    if linked_home_resolved {
        warnings.push(format!(
            "Codex home link was resolved from {} to {}",
            requested_codex_home.display(),
            codex_home.display()
        ));
    }
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

    let mut skill_paths = collect_files(
        &codex_home.join("skills"),
        |path| file_name_is(path, "SKILL.md"),
        "skills",
        &mut warnings,
    );
    let user_home = platform_user_home(context, current_source_os())
        .or_else(|| codex_home.parent().map(Path::to_path_buf));
    let agent_skills_root = user_home.map(|home| home.join(".agents").join("skills"));
    let agent_skill_paths = agent_skills_root
        .as_deref()
        .map(|root| {
            collect_files(
                root,
                |path| file_name_is(path, "SKILL.md"),
                "global agent skills",
                &mut warnings,
            )
        })
        .unwrap_or_default();
    skill_paths.extend(agent_skill_paths.iter().cloned());
    skill_paths.sort();
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
    let has_registered_projects = read_global_project_roots(
        &codex_home.join(".codex-global-state.json"),
        &mut project_paths,
        &mut seen_projects,
        &mut warnings,
    );

    let (sqlite_threads, active_rollout_paths) = state_db_path
        .as_deref()
        .map(|path| {
            read_state_database_roots(
                path,
                if has_registered_projects {
                    None
                } else {
                    Some((&mut project_paths, &mut seen_projects))
                },
                &mut warnings,
            )
        })
        .unwrap_or_else(|| (0, BTreeMap::new()));

    let projects = discovered_projects(&project_paths);
    let conversations = discovered_conversations(
        &codex_home,
        &conversation_paths,
        session_index_path.as_deref(),
        &projects,
        &active_rollout_paths,
        &mut warnings,
    );

    dedupe_warnings(&mut warnings);

    let codex_skill_paths = skill_paths
        .iter()
        .filter(|path| path.starts_with(codex_home.join("skills")))
        .cloned()
        .collect::<Vec<_>>();
    let mut skills = optional_tree_entries(
        &codex_skill_paths,
        &codex_home.join("skills"),
        "skill",
        false,
    );
    if let Some(agent_skills_root) = agent_skills_root.as_deref() {
        let mut agent_skills =
            optional_tree_entries(&agent_skill_paths, agent_skills_root, "agent-skill", false);
        for skill in &mut agent_skills {
            skill.relative_path = format!(".agents/skills/{}", skill.relative_path);
        }
        skills.extend(agent_skills);
        skills.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.relative_path.cmp(&right.relative_path))
        });
    }
    let plugins = optional_tree_entries(
        &plugin_paths,
        &codex_home.join("plugins").join("cache"),
        "plugin",
        true,
    );
    let generated_images = optional_file_entries(
        &generated_image_paths,
        &codex_home.join("generated_images"),
        "generated-image",
    );

    Ok(CodexInventory {
        codex_home,
        source_os: current_source_os(),
        source_arch: env::consts::ARCH.to_owned(),
        source_device_id: Uuid::nil(),
        counts: ContentCounts {
            projects: projects.len() as u64,
            project_files: 0,
            conversations: conversations.len() as u64,
            skills: skills.len() as u64,
            plugins: plugins.len() as u64,
            generated_images: generated_images.len() as u64,
            sqlite_threads,
        },
        projects,
        project_paths,
        conversations,
        conversation_paths,
        session_index_path,
        state_db_path,
        skill_paths,
        plugin_paths,
        generated_image_paths,
        skills,
        plugins,
        generated_images,
        warnings,
    })
}

fn resolve_existing_codex_home(path: &Path) -> Result<(PathBuf, bool), RehomeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| codex_home_not_found())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        return Ok((path.to_path_buf(), false));
    }

    #[cfg(not(windows))]
    {
        Err(codex_home_not_found())
    }

    #[cfg(windows)]
    {
        let canonical = fs::canonicalize(path).map_err(|_| codex_home_not_found())?;
        let canonical = local_canonical_codex_home(canonical)?;
        let target_metadata =
            fs::symlink_metadata(&canonical).map_err(|_| codex_home_not_found())?;
        if !target_metadata.is_dir() || target_metadata.file_type().is_symlink() {
            return Err(codex_home_not_found());
        }
        Ok((canonical, true))
    }
}

#[cfg(windows)]
fn local_canonical_codex_home(path: PathBuf) -> Result<PathBuf, RehomeError> {
    let raw = path.to_string_lossy();
    if raw.starts_with(r"\\?\UNC\") {
        return Err(RehomeError::new(
            ErrorCode::CodexNotFound,
            "Linked Codex home must resolve to a local disk",
        ));
    }
    let Some(local) = raw.strip_prefix(r"\\?\") else {
        if raw.starts_with(r"\\") {
            return Err(RehomeError::new(
                ErrorCode::CodexNotFound,
                "Linked Codex home must resolve to a local disk",
            ));
        }
        return Ok(path);
    };
    let bytes = local.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return Err(RehomeError::new(
            ErrorCode::CodexNotFound,
            "Linked Codex home must resolve to a local disk",
        ));
    }
    Ok(PathBuf::from(local))
}

fn codex_home_not_found() -> RehomeError {
    RehomeError::new(
        ErrorCode::CodexNotFound,
        "Codex home does not exist or does not resolve to a directory",
    )
}

fn discovered_projects(paths: &[PathBuf]) -> Vec<ProjectEntry> {
    paths
        .iter()
        .map(|path| {
            let source = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            let source_path = source.to_string_lossy().into_owned();
            let project_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, source_path.as_bytes());
            let name = portable_project_name(&source);
            ProjectEntry {
                project_id,
                name,
                source_path,
                source_available: fs::symlink_metadata(&source)
                    .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink()),
                archive_path: format!("projects/{project_id}/files"),
                file_count: 0,
                content_bytes: 0,
                git_remote: None,
                git_branch: None,
                git_head: None,
            }
        })
        .collect()
}

fn discovered_conversations(
    codex_home: &Path,
    paths: &[PathBuf],
    session_index_path: Option<&Path>,
    projects: &[ProjectEntry],
    active_rollout_paths: &BTreeMap<Uuid, PathBuf>,
    warnings: &mut Vec<String>,
) -> Vec<ConversationEntry> {
    let index = read_session_index_entries(session_index_path, warnings);
    let mut conversations = Vec::new();
    for path in paths {
        let session = match read_session_metadata_file(path) {
            Ok(Some(session)) => session,
            Ok(None) => {
                push_warning_unique(
                    warnings,
                    format!(
                        "Could not identify discovered conversation {}",
                        path.display()
                    ),
                );
                continue;
            }
            Err(error) => {
                push_warning_unique(
                    warnings,
                    format!(
                        "Could not read discovered conversation {}: {error}",
                        path.display()
                    ),
                );
                continue;
            }
        };
        let task_id = session.task_id;
        let metadata = index.get(&task_id).unwrap_or(&session.fields);
        let relative = match path.strip_prefix(codex_home).map(normalize_entry) {
            Ok(Ok(relative)) => relative,
            _ => {
                push_warning_unique(
                    warnings,
                    format!(
                        "Discovered conversation escapes Codex home: {}",
                        path.display()
                    ),
                );
                continue;
            }
        };
        let classification = conversation_classification(&session.fields);
        let indexed_title = metadata_string(metadata, &["title", "thread_name"])
            .or_else(|| metadata_string(&session.fields, &["title", "thread_name"]));
        let title = indexed_title
            .filter(|title| title != "Codex conversation")
            .or_else(|| {
                classification
                    .as_ref()
                    .and_then(|classification| classification.agent_path.as_deref())
                    .map(humanize_agent_path)
            })
            .unwrap_or_else(|| "Codex conversation".to_owned());
        conversations.push(ConversationEntry {
            task_id,
            project_id: associated_project_id(metadata, &session.fields, projects),
            title,
            updated_at: metadata_string(metadata, &["updated_at", "timestamp"])
                .or_else(|| metadata_string(&session.fields, &["updated_at", "timestamp"]))
                .unwrap_or_default(),
            // Discovery only needs identity and UI metadata. The authenticated
            // content hash is calculated later, when a conversation is selected.
            content_hash: String::new(),
            archive_path: format!("codex/{relative}"),
            classification,
        });
    }
    let mut grouped = BTreeMap::<Uuid, Vec<ConversationEntry>>::new();
    for conversation in conversations {
        grouped
            .entry(conversation.task_id)
            .or_default()
            .push(conversation);
    }
    let mut selected = Vec::new();
    for (task_id, mut candidates) in grouped {
        if candidates.len() == 1 {
            selected.push(candidates.pop().expect("single conversation candidate"));
            continue;
        }
        let matching = active_rollout_paths
            .get(&task_id)
            .map(|active| {
                candidates
                    .iter()
                    .enumerate()
                    .filter_map(|(index, candidate)| {
                        discovered_rollout_matches(codex_home, candidate, active).then_some(index)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if matching.len() == 1 {
            selected.push(candidates.swap_remove(matching[0]));
            continue;
        }
        push_warning_unique(
            warnings,
            format!(
                "Conversation {task_id} has multiple rollout files but no unique active SQLite rollout_path"
            ),
        );
        selected.extend(candidates);
    }
    selected.sort_by_key(|conversation| conversation.task_id);
    selected
}

fn discovered_rollout_matches(
    codex_home: &Path,
    conversation: &ConversationEntry,
    active: &Path,
) -> bool {
    let Some(relative) = conversation.archive_path.strip_prefix("codex/") else {
        return false;
    };
    let candidate = codex_home.join(Path::new(relative));
    let active = if active.is_absolute() {
        active.to_path_buf()
    } else {
        codex_home.join(active)
    };
    candidate == active
        || match (candidate.canonicalize(), active.canonicalize()) {
            (Ok(candidate), Ok(active)) => candidate == active,
            _ => false,
        }
}

fn read_session_metadata_file(path: &Path) -> io::Result<Option<SessionMetadata>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();

    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        if let Some(metadata) = parse_session_metadata(&line) {
            return Ok(Some(metadata));
        }
    }
}

pub(crate) fn conversation_classification(fields: &Value) -> Option<ConversationClassification> {
    if metadata_string(fields, &["thread_source"]).as_deref() != Some("subagent")
        && fields.pointer("/source/subagent/thread_spawn").is_none()
    {
        return None;
    }
    Some(ConversationClassification {
        parent_task_id: metadata_uuid(fields, &["parent_thread_id", "forked_from_id"]),
        agent_path: metadata_string(fields, &["agent_path"]).or_else(|| {
            fields
                .pointer("/source/subagent/thread_spawn/agent_path")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
        agent_nickname: metadata_string(fields, &["agent_nickname"]).or_else(|| {
            fields
                .pointer("/source/subagent/thread_spawn/agent_nickname")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
        depth: fields
            .pointer("/source/subagent/thread_spawn/depth")
            .and_then(Value::as_u64),
    })
}

fn humanize_agent_path(path: &str) -> String {
    path.trim_start_matches("/root/")
        .split('/')
        .map(|part| part.replace('_', " "))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn read_session_index_entries(
    path: Option<&Path>,
    warnings: &mut Vec<String>,
) -> BTreeMap<Uuid, Value> {
    let Some(path) = path else {
        return BTreeMap::new();
    };
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            push_warning_unique(
                warnings,
                format!("Could not read optional Codex session index entries: {error}"),
            );
            return BTreeMap::new();
        }
    };
    let mut entries = BTreeMap::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(id) = metadata_uuid(&value, &["id", "thread_id", "conversation_id"]) {
            entries.insert(id, value);
        }
    }
    entries
}

pub(crate) fn associated_project_id(
    metadata: &Value,
    session: &Value,
    projects: &[ProjectEntry],
) -> Option<Uuid> {
    let cwd = metadata_string(metadata, &["cwd", "workspace_root"])
        .or_else(|| metadata_string(session, &["cwd", "workspace_root"]));
    if let Some(cwd) = cwd {
        let path = PathBuf::from(&cwd);
        let canonical = fs::canonicalize(&path).unwrap_or(path);
        let key = ProjectPathKey::new(&canonical.to_string_lossy(), &canonical);
        if let Some(project) = projects.iter().find(|project| {
            let source = PathBuf::from(&project.source_path);
            let source_key = ProjectPathKey::new(&project.source_path, &source);
            source_key == key || project_path_contains(&source_key, &key)
        }) {
            return Some(project.project_id);
        }
    }

    metadata_uuid(metadata, &["project_id"])
        .or_else(|| metadata_uuid(session, &["project_id"]))
        .filter(|id| projects.iter().any(|project| project.project_id == *id))
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
        Err(error) => {
            push_warning_unique(
                warnings,
                format!(
                    "Could not inspect optional {label} data at {}: {error}",
                    root.display()
                ),
            );
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        push_warning_unique(
            warnings,
            format!(
                "Skipped symbolic link in optional {label} data: {}",
                root.display()
            ),
        );
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
        Err(error) => {
            push_warning_unique(
                warnings,
                format!(
                    "Could not read optional {label} data at {}: {error}",
                    root.display()
                ),
            );
            return;
        }
    };
    let mut children = collect_child_paths(
        entries.map(|entry| entry.map(|entry| entry.path())),
        root,
        label,
        warnings,
    );
    children.sort();
    for child in children {
        collect_files_inner(&child, matches, label, warnings, files);
    }
}

fn collect_child_paths(
    entries: impl Iterator<Item = io::Result<PathBuf>>,
    root: &Path,
    label: &str,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    entries
        .filter_map(|entry| match entry {
            Ok(path) => Some(path),
            Err(error) => {
                push_warning_unique(
                    warnings,
                    format!(
                        "Could not read a directory entry in optional {label} data at {}: {error}",
                        root.display(),
                    ),
                );
                None
            }
        })
        .collect()
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
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                push_warning_unique(
                    warnings,
                    format!(
                        "Could not read a Codex home entry while listing state databases at {}: {error}",
                        codex_home.display()
                    ),
                );
                continue;
            }
        };
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
    seen: &mut HashSet<ProjectPathKey>,
    warnings: &mut Vec<String>,
) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            warnings.push("Optional Codex global state metadata is not a regular file".to_owned());
            return false;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            warnings.push("Optional Codex global state metadata was not found".to_owned());
            return false;
        }
        Err(_) => {
            warnings.push("Could not inspect optional Codex global state metadata".to_owned());
            return false;
        }
    }
    let value = match fs::read(path)
        .ok()
        .and_then(|contents| serde_json::from_slice::<Value>(&contents).ok())
    {
        Some(Value::Object(value)) => value,
        _ => {
            warnings.push("Could not parse optional Codex global state metadata".to_owned());
            return false;
        }
    };

    if let Some(local_projects) = value.get("local-projects").and_then(Value::as_object) {
        for project in local_projects.values().filter_map(Value::as_object) {
            let Some(roots) = project.get("rootPaths").and_then(Value::as_array) else {
                continue;
            };
            for root in roots.iter().filter_map(Value::as_str) {
                push_unique_path(root, projects, seen);
            }
        }
        if !local_projects.is_empty() {
            return true;
        }
    }

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

    false
}

fn read_state_database_roots(
    path: &Path,
    mut fallback_projects: Option<(&mut Vec<PathBuf>, &mut HashSet<ProjectPathKey>)>,
    warnings: &mut Vec<String>,
) -> (u64, BTreeMap<Uuid, PathBuf>) {
    let snapshot = match StateDatabaseSnapshot::create(path) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            warnings.push(format!(
                "Could not snapshot the newest Codex state database: {error}"
            ));
            return (0, BTreeMap::new());
        }
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = match Connection::open_with_flags(&snapshot.database_path, flags) {
        Ok(connection) => connection,
        Err(_) => {
            warnings.push("Could not open the newest Codex state database read-only".to_owned());
            return (0, BTreeMap::new());
        }
    };

    let count = match connection.query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0)) {
        Ok(count) => count,
        Err(_) => {
            warnings.push("Could not count threads in the newest Codex state database".to_owned());
            return (0, BTreeMap::new());
        }
    };

    let active_rollout_paths = read_active_rollout_paths(&connection, warnings);

    let mut statement = match connection.prepare("SELECT cwd FROM threads ORDER BY rowid") {
        Ok(statement) => statement,
        Err(_) => {
            warnings.push(
                "Could not read project roots from the newest Codex state database".to_owned(),
            );
            return (count, active_rollout_paths);
        }
    };
    let rows = match statement.query_map([], |row| row.get::<_, Option<String>>(0)) {
        Ok(rows) => rows,
        Err(_) => {
            warnings.push(
                "Could not read project roots from the newest Codex state database".to_owned(),
            );
            return (count, active_rollout_paths);
        }
    };
    for row in rows {
        match row {
            Ok(Some(path)) => {
                if let Some((projects, seen)) = fallback_projects.as_mut() {
                    push_unique_path(&path, projects, seen);
                }
            }
            Ok(None) => {}
            Err(_) => warnings.push("Ignored an unreadable thread project root".to_owned()),
        }
    }
    (count, active_rollout_paths)
}

fn read_active_rollout_paths(
    connection: &Connection,
    warnings: &mut Vec<String>,
) -> BTreeMap<Uuid, PathBuf> {
    let mut statement =
        match connection.prepare("SELECT id, rollout_path FROM threads ORDER BY rowid") {
            Ok(statement) => statement,
            Err(_) => {
                warnings.push(
                    "Could not read active rollout paths from the newest Codex state database"
                        .to_owned(),
                );
                return BTreeMap::new();
            }
        };
    let rows = match statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => {
            warnings.push(
                "Could not read active rollout paths from the newest Codex state database"
                    .to_owned(),
            );
            return BTreeMap::new();
        }
    };
    let mut paths = BTreeMap::new();
    for row in rows {
        match row {
            Ok((Some(id), Some(path))) if !path.is_empty() => {
                if let Ok(id) = Uuid::parse_str(&id) {
                    paths.insert(id, PathBuf::from(path));
                }
            }
            Ok(_) => {}
            Err(_) => warnings.push("Ignored an unreadable active rollout path".to_owned()),
        }
    }
    paths
}

fn push_unique_path(raw: &str, projects: &mut Vec<PathBuf>, seen: &mut HashSet<ProjectPathKey>) {
    if raw.is_empty() {
        return;
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() && !looks_like_absolute_windows_path(raw) {
        return;
    }
    if seen.insert(ProjectPathKey::new(raw, &path)) {
        projects.push(path);
    }
}

fn looks_like_absolute_windows_path(raw: &str) -> bool {
    let normalized = raw.strip_prefix(r"\\?\").unwrap_or(raw);
    let bytes = normalized.as_bytes();
    normalized.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum ProjectPathKey {
    Windows(String),
    Native(PathBuf),
}

fn project_path_contains(project: &ProjectPathKey, candidate: &ProjectPathKey) -> bool {
    match (project, candidate) {
        (ProjectPathKey::Windows(project), ProjectPathKey::Windows(candidate)) => {
            candidate == project
                || candidate
                    .strip_prefix(project)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }
        (ProjectPathKey::Native(project), ProjectPathKey::Native(candidate)) => {
            candidate.starts_with(project)
        }
        _ => false,
    }
}

impl ProjectPathKey {
    fn new(raw: &str, path: &Path) -> Self {
        if looks_like_windows_path(raw) {
            let normalized = raw.strip_prefix(r"\\?\").unwrap_or(raw).replace('\\', "/");
            let prefix = if normalized.starts_with("//") {
                "//"
            } else {
                ""
            };
            let components = normalized
                .split('/')
                .filter(|component| !component.is_empty())
                .collect::<Vec<_>>()
                .join("/");
            Self::Windows(format!("{prefix}{components}").to_lowercase())
        } else {
            Self::Native(path.to_path_buf())
        }
    }
}

fn optional_tree_entries(
    markers: &[PathBuf],
    root: &Path,
    kind: &str,
    expand_plugin_root: bool,
) -> Vec<OptionalContentEntry> {
    let mut bundles = markers
        .iter()
        .filter_map(|marker| {
            let marker_parent = marker.parent()?;
            let is_modern_plugin = expand_plugin_root
                && marker_parent.file_name().and_then(|name| name.to_str())
                    == Some(".codex-plugin");
            let bundle = if is_modern_plugin {
                marker_parent.parent()?
            } else if !expand_plugin_root {
                outermost_skill_bundle(marker, root)?
            } else {
                marker_parent
            };
            Some((bundle.to_path_buf(), marker.clone(), is_modern_plugin))
        })
        .collect::<Vec<_>>();
    bundles.sort_by(|left, right| {
        left.0
            .components()
            .count()
            .cmp(&right.0.components().count())
            .then(left.0.cmp(&right.0))
    });

    let mut selected_bundles = Vec::new();
    for candidate in bundles {
        if selected_bundles
            .iter()
            .any(|(selected, _, _): &(PathBuf, PathBuf, bool)| candidate.0.starts_with(selected))
        {
            continue;
        }
        selected_bundles.push(candidate);
    }

    let mut entries = selected_bundles
        .into_iter()
        .filter_map(|(bundle, marker, is_modern_plugin)| {
            let relative = bundle.strip_prefix(root).ok()?;
            let relative_path = normalize_entry(relative).ok()?;
            Some(OptionalContentEntry {
                content_id: Uuid::new_v5(
                    &Uuid::NAMESPACE_URL,
                    format!("{kind}:{relative_path}").as_bytes(),
                ),
                name: if is_modern_plugin {
                    bundle.parent().and_then(Path::file_name)
                } else {
                    bundle.file_name()
                }
                .and_then(|name| name.to_str())
                .unwrap_or(kind)
                .to_owned(),
                source_path: if expand_plugin_root {
                    marker
                } else {
                    bundle.join("SKILL.md")
                },
                relative_path,
                size_bytes: directory_size(&bundle),
                thumbnail_data_url: None,
                reveal_id: None,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.relative_path.cmp(&right.relative_path))
    });
    entries
}

fn outermost_skill_bundle<'a>(marker: &'a Path, root: &Path) -> Option<&'a Path> {
    let mut bundle = marker.parent()?;
    let mut ancestor = bundle.parent();
    while let Some(candidate) = ancestor {
        if candidate == root {
            break;
        }
        let candidate_marker = candidate.join("SKILL.md");
        if fs::symlink_metadata(candidate_marker)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            bundle = candidate;
        }
        ancestor = candidate.parent();
    }
    Some(bundle)
}

fn optional_file_entries(paths: &[PathBuf], root: &Path, kind: &str) -> Vec<OptionalContentEntry> {
    let mut entries = paths
        .iter()
        .filter_map(|path| {
            let relative_path = normalize_entry(path.strip_prefix(root).ok()?).ok()?;
            Some(OptionalContentEntry {
                content_id: Uuid::new_v5(
                    &Uuid::NAMESPACE_URL,
                    format!("{kind}:{relative_path}").as_bytes(),
                ),
                name: path.file_name()?.to_string_lossy().into_owned(),
                source_path: path.clone(),
                relative_path,
                size_bytes: path.metadata().map(|metadata| metadata.len()).unwrap_or(0),
                thumbnail_data_url: image_thumbnail(path),
                reveal_id: None,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

fn image_thumbnail(path: &Path) -> Option<String> {
    const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
    let metadata = path.metadata().ok()?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let (width, height) = image::image_dimensions(path).ok()?;
    if width > 16_384 || height > 16_384 {
        return None;
    }
    let thumbnail = image::open(path).ok()?.thumbnail(160, 100);
    let mut bytes = std::io::Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut bytes, image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        BASE64.encode(bytes.into_inner())
    ))
}

fn directory_size(root: &Path) -> u64 {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

fn looks_like_windows_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    raw.contains('\\')
        || raw.starts_with("//")
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

pub(crate) struct StateDatabaseSnapshot {
    _directory: tempfile::TempDir,
    database_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFileFingerprint {
    suffix: &'static str,
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

impl StateDatabaseSnapshot {
    pub(crate) fn create(source_database: &Path) -> io::Result<Self> {
        const MAX_ATTEMPTS: usize = 3;

        let directory = tempfile::Builder::new()
            .prefix("rehome-state-snapshot-")
            .tempdir()?;
        let file_name = source_database.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "state database has no file name",
            )
        })?;
        let database_path = directory.path().join(file_name);
        let source_files = source_database_files(source_database)?;
        let has_wal = source_files.iter().any(|source| source.suffix == "-wal");
        let has_shm = source_files.iter().any(|source| source.suffix == "-shm");

        if !has_wal || has_shm {
            backup_live_database(source_database, &database_path)?;
            return Ok(Self {
                _directory: directory,
                database_path,
            });
        }

        let mut last_error = None;
        for _ in 0..MAX_ATTEMPTS {
            match copy_state_database_once(source_database, &database_path, directory.path()) {
                Ok(true) => {
                    return Ok(Self {
                        _directory: directory,
                        database_path,
                    });
                }
                Ok(false) => {
                    last_error = Some(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "state database changed while it was being snapshotted",
                    ));
                }
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| io::Error::other("state database snapshot did not run")))
    }

    pub(crate) fn database_path(&self) -> &Path {
        &self.database_path
    }
}

fn backup_live_database(source_database: &Path, database_path: &Path) -> io::Result<()> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let source = Connection::open_with_flags(source_database, flags).map_err(sqlite_io_error)?;
    let mut destination = Connection::open(database_path).map_err(sqlite_io_error)?;
    let backup = Backup::new(&source, &mut destination).map_err(sqlite_io_error)?;
    backup
        .run_to_completion(128, Duration::from_millis(1), None)
        .map_err(sqlite_io_error)
}

fn sqlite_io_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}

fn copy_state_database_once(
    source_database: &Path,
    database_path: &Path,
    snapshot_directory: &Path,
) -> io::Result<bool> {
    let before = source_database_files(source_database)?;
    clear_snapshot_files(snapshot_directory)?;
    for source in &before {
        let destination = sqlite_sidecar_path(database_path, source.suffix);
        fs::copy(&source.path, destination)?;
    }
    Ok(before == source_database_files(source_database)?)
}

fn source_database_files(database: &Path) -> io::Result<Vec<SourceFileFingerprint>> {
    let mut files = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let path = sqlite_sidecar_path(database, suffix);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                files.push(SourceFileFingerprint {
                    suffix,
                    path,
                    len: metadata.len(),
                    modified: metadata.modified()?,
                });
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "state database snapshot source is not a regular file",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && !suffix.is_empty() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}

fn clear_snapshot_files(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        fs::remove_file(path)?;
    }
    Ok(())
}

fn sqlite_sidecar_path(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn push_warning_unique(warnings: &mut Vec<String>, warning: impl Into<String>) {
    let warning = warning.into();
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

fn dedupe_warnings(warnings: &mut Vec<String>) {
    let mut seen = HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));
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

fn portable_project_name(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|name| !name.is_empty())
        .unwrap_or("project")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{collect_child_paths, portable_project_name, read_session_metadata_file};
    use crate::core::session::{metadata_string, parse_session_metadata};
    use std::{
        fs, io,
        path::{Path, PathBuf},
    };
    use uuid::Uuid;

    #[test]
    fn project_name_accepts_windows_paths_on_macos_and_unix_paths_on_windows() {
        assert_eq!(
            portable_project_name(Path::new(r"C:\Users\Example\Documents\visual")),
            "visual"
        );
        assert_eq!(
            portable_project_name(Path::new("/Users/example/Documents/visual")),
            "visual"
        );
    }

    #[test]
    fn session_parser_accepts_current_nested_metadata_and_safe_legacy_metadata() {
        let current_id = Uuid::new_v4();
        let current = format!(
            "{{\"type\":\"session_meta\",\"timestamp\":\"outer\",\"payload\":{{\"id\":\"{current_id}\",\"cwd\":\"C:/work/current\",\"title\":\"Current\"}}}}\n"
        );
        let parsed = parse_session_metadata(current.as_bytes()).expect("current metadata");
        assert_eq!(parsed.task_id, current_id);
        assert_eq!(
            metadata_string(&parsed.fields, &["cwd"]).as_deref(),
            Some("C:/work/current")
        );
        assert_eq!(
            metadata_string(&parsed.fields, &["title"]).as_deref(),
            Some("Current")
        );
        assert_eq!(
            metadata_string(&parsed.fields, &["timestamp"]).as_deref(),
            Some("outer")
        );

        let legacy_id = Uuid::new_v4();
        let legacy = format!(
            "{{\"thread_id\":\"{legacy_id}\",\"cwd\":\"C:/work/legacy\",\"timestamp\":\"legacy\"}}\n"
        );
        assert_eq!(
            parse_session_metadata(legacy.as_bytes())
                .expect("legacy metadata")
                .task_id,
            legacy_id
        );
    }

    #[test]
    fn session_parser_never_infers_identity_from_arbitrary_message_payloads() {
        let message = format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"id\":\"{}\",\"cwd\":\"C:/private\"}}}}\n",
            Uuid::new_v4()
        );
        assert!(parse_session_metadata(message.as_bytes()).is_none());
    }

    #[test]
    fn session_discovery_reads_metadata_without_loading_the_rollout_tail() {
        let root = tempfile::tempdir().expect("temporary directory");
        let path = root.path().join("session.jsonl");
        let id = Uuid::new_v4();
        fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"title\":\"Fast\"}}}}\n{}",
                "x".repeat(2 * 1024 * 1024)
            ),
        )
        .expect("session fixture");

        let metadata = read_session_metadata_file(&path)
            .expect("read metadata")
            .expect("metadata present");
        assert_eq!(metadata.task_id, id);
    }

    #[test]
    fn individual_directory_entry_errors_warn_once_and_keep_readable_children() {
        let readable = PathBuf::from("sessions/readable.jsonl");
        let entries = vec![
            Ok(readable.clone()),
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        ];
        let mut warnings = Vec::new();

        let children = collect_child_paths(
            entries.into_iter(),
            Path::new("sessions"),
            "sessions",
            &mut warnings,
        );

        assert_eq!(children, vec![readable]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("directory entry"));
        assert!(warnings[0].contains("sessions"));
    }
}
