use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, PackagePreview, PlannedOperation, PlannedSession, ReferenceRewrite,
        ReferenceRewriteKind, RestorePlan, SessionAction, SourceOs, TargetInventory,
    },
    package::inspect_package,
    paths::normalize_entry,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};
use uuid::Uuid;
use zip::ZipArchive;

const SESSION_INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREAD_METADATA_SOURCE: &str = "codex/metadata/threads.json";

#[derive(Debug)]
struct PayloadInfo {
    content_hash: String,
    size_bytes: u64,
}

#[derive(Debug)]
enum TargetState {
    Missing,
    File(String),
    Other,
}

pub fn build_restore_plan(
    package: &PackagePreview,
    target: &TargetInventory,
    projects_root: &Path,
) -> Result<RestorePlan, RehomeError> {
    validate_plan_inputs(package, target, projects_root)?;
    let current_preview = inspect_package(&package.package_path)?;
    if current_preview != *package {
        return Err(package_invalid(
            "package changed after it was inspected or the preview was altered",
        ));
    }
    let payloads = read_payloads(&package.package_path)?;

    let mut operations = Vec::new();
    let mut sessions = Vec::new();
    let mut rewrites = BTreeMap::new();
    let mut consumed = HashSet::new();
    let mut project_targets = HashMap::new();

    for project in &package.manifest.projects {
        let target_root = join_target(projects_root, &project.name, target.target_os)?;
        project_targets.insert(project.project_id, target_root.clone());
        let prefix = format!("{}/", project.archive_path);
        for (source, payload) in payloads
            .iter()
            .filter(|(source, _)| source.starts_with(&prefix))
        {
            let relative = source
                .strip_prefix(&prefix)
                .ok_or_else(|| package_invalid("project payload path is malformed"))?;
            let target_path = join_target(&target_root, relative, target.target_os)?;
            operations.push(classify_file(source, target_path, payload)?);
            consumed.insert(source.clone());
        }
    }

    let target_conversations = target
        .conversations
        .iter()
        .map(|conversation| (conversation.task_id, conversation))
        .collect::<HashMap<_, _>>();
    if target_conversations.len() != target.conversations.len() {
        return Err(restore_failed(
            "target inventory contains duplicate conversation IDs",
        ));
    }

    for conversation in &package.manifest.conversations {
        let payload = payloads.get(&conversation.archive_path).ok_or_else(|| {
            package_invalid("manifest conversation references a missing package payload")
        })?;
        if !payload
            .content_hash
            .eq_ignore_ascii_case(&conversation.content_hash)
        {
            return Err(package_invalid(
                "manifest conversation content hash does not match its package payload",
            ));
        }

        let source_task_id = conversation.task_id;
        let original_target = codex_target_path(
            &target.codex_home,
            &conversation.archive_path,
            target.target_os,
        )?;
        let (action, target_task_id, title, target_path, expected_previous_hash, change) =
            match target_conversations.get(&source_task_id) {
                Some(existing)
                    if existing
                        .content_hash
                        .eq_ignore_ascii_case(&conversation.content_hash) =>
                {
                    let existing_target = codex_target_path(
                        &target.codex_home,
                        &existing.archive_path,
                        target.target_os,
                    )?;
                    (
                        SessionAction::Skip,
                        source_task_id,
                        existing.title.clone(),
                        existing_target.clone(),
                        existing_hash(&existing_target, &existing.content_hash)?,
                        ChangeKind::Unchanged,
                    )
                }
                Some(_) => {
                    let derived_id =
                        Uuid::new_v5(&package.manifest.package_id, source_task_id.as_bytes());
                    let branch_target =
                        branch_session_target(&original_target, derived_id, target.target_os)?;
                    let branch_state = target_state(&branch_target)?;
                    let (branch_change, previous) =
                        classify_target_state(&branch_state, &payload.content_hash);
                    add_branch_rewrites(
                        &mut rewrites,
                        &payloads,
                        conversation,
                        derived_id,
                        &format!("{} · ReHome", conversation.title),
                    );
                    (
                        SessionAction::ImportAsBranch,
                        derived_id,
                        format!("{} · ReHome", conversation.title),
                        branch_target,
                        previous,
                        branch_change,
                    )
                }
                None => {
                    let state = target_state(&original_target)?;
                    let (change, previous) = classify_target_state(&state, &payload.content_hash);
                    (
                        SessionAction::Import,
                        source_task_id,
                        conversation.title.clone(),
                        original_target,
                        previous,
                        change,
                    )
                }
            };

        let rollback_required = matches!(change, ChangeKind::Add | ChangeKind::Update);
        operations.push(PlannedOperation {
            package_source: conversation.archive_path.clone(),
            target: target_path.clone(),
            expected_previous_hash,
            action: change,
            rollback_required,
        });
        sessions.push(PlannedSession {
            package_source: conversation.archive_path.clone(),
            target: target_path,
            source_task_id,
            target_task_id,
            title,
            content_hash: conversation.content_hash.clone(),
            action,
        });
        consumed.insert(conversation.archive_path.clone());

        if action != SessionAction::Skip {
            add_project_path_rewrites(
                &mut rewrites,
                &payloads,
                conversation,
                &package.manifest.projects,
                &project_targets,
            )?;
        }
    }

    if sessions
        .iter()
        .any(|session| session.action != SessionAction::Skip)
    {
        if let Some(payload) = payloads.get(SESSION_INDEX_SOURCE) {
            let target_path =
                join_target(&target.codex_home, "session_index.jsonl", target.target_os)?;
            operations.push(classify_merge(SESSION_INDEX_SOURCE, target_path, payload)?);
            consumed.insert(SESSION_INDEX_SOURCE.to_owned());
        }
        if let Some(payload) = payloads.get(THREAD_METADATA_SOURCE) {
            let target_path = find_state_database(&target.codex_home)?.ok_or_else(|| {
                RehomeError::new(
                    ErrorCode::CodexNotFound,
                    "target Codex state database was not found",
                )
            })?;
            operations.push(classify_merge(
                THREAD_METADATA_SOURCE,
                target_path,
                payload,
            )?);
            consumed.insert(THREAD_METADATA_SOURCE.to_owned());
        }
    }

    for (source, payload) in &payloads {
        if consumed.contains(source) || is_package_only_metadata(source) {
            continue;
        }
        let target_path = codex_target_path(&target.codex_home, source, target.target_os)?;
        operations.push(classify_file(source, target_path, payload)?);
    }

    operations.sort_by(|left, right| left.package_source.cmp(&right.package_source));
    sessions.sort_by_key(|session| session.source_task_id);
    let reference_rewrites = rewrites.into_values().collect::<Vec<_>>();
    let conflict_count = operations
        .iter()
        .filter(|operation| operation.action == ChangeKind::Conflict)
        .count() as u64;
    let required_bytes = operations
        .iter()
        .filter(|operation| matches!(operation.action, ChangeKind::Add | ChangeKind::Update))
        .filter_map(|operation| payloads.get(&operation.package_source))
        .try_fold(0_u64, |total, payload| {
            total
                .checked_add(payload.size_bytes)
                .ok_or_else(|| restore_failed("restore plan size exceeds the supported range"))
        })?;

    Ok(RestorePlan {
        package_path: package.package_path.clone(),
        package_id: package.manifest.package_id,
        target_codex_home: target.codex_home.clone(),
        projects_root: projects_root.to_path_buf(),
        operations,
        sessions,
        reference_rewrites,
        conflict_count,
        required_bytes,
    })
}

fn validate_plan_inputs(
    package: &PackagePreview,
    target: &TargetInventory,
    projects_root: &Path,
) -> Result<(), RehomeError> {
    if !package.checksum_valid {
        return Err(RehomeError::new(
            ErrorCode::ChecksumMismatch,
            "package checksum validation did not pass",
        ));
    }
    if !is_target_absolute(&target.codex_home, target.target_os)?
        || !is_target_absolute(projects_root, target.target_os)?
    {
        return Err(restore_failed("restore target paths must be absolute"));
    }

    let mut project_ids = HashSet::new();
    let mut project_target_names = HashSet::new();
    for project in &package.manifest.projects {
        if !project_ids.insert(project.project_id) {
            return Err(package_invalid("manifest contains duplicate project IDs"));
        }
        validate_manifest_path(&project.archive_path)?;
        let expected = format!("projects/{}/files", project.project_id);
        if project.archive_path != expected {
            return Err(package_invalid(
                "manifest project path does not match its expected package prefix",
            ));
        }
        let normalized_name = normalize_entry(Path::new(&project.name))?;
        if normalized_name != project.name || normalized_name.contains('/') {
            return Err(package_invalid(
                "manifest project name is not a portable path component",
            ));
        }
        if !project_target_names.insert(normalized_name.to_lowercase()) {
            return Err(RehomeError::new(
                ErrorCode::ProjectConflict,
                "multiple package projects map to the same target directory",
            ));
        }
    }

    let mut conversation_ids = HashSet::new();
    for conversation in &package.manifest.conversations {
        validate_manifest_path(&conversation.archive_path)?;
        if !conversation.archive_path.starts_with("codex/sessions/")
            && !conversation
                .archive_path
                .starts_with("codex/archived_sessions/")
        {
            return Err(package_invalid(
                "manifest conversation path is outside the expected Codex session prefixes",
            ));
        }
        if !conversation_ids.insert(conversation.task_id) {
            return Err(package_invalid(
                "manifest contains duplicate conversation IDs",
            ));
        }
    }
    Ok(())
}

fn validate_manifest_path(path: &str) -> Result<(), RehomeError> {
    let normalized = normalize_entry(Path::new(path))?;
    if normalized != path {
        return Err(package_invalid("manifest archive path is not normalized"));
    }
    Ok(())
}

fn read_payloads(path: &Path) -> Result<BTreeMap<String, PayloadInfo>, RehomeError> {
    let file = fs::File::open(path)
        .map_err(|error| package_invalid(format!("could not open package: {error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| package_invalid(format!("invalid ZIP container: {error}")))?;
    let mut payloads = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| package_invalid(format!("could not read ZIP entry: {error}")))?;
        if entry.is_dir() {
            continue;
        }
        let source = entry.name().to_owned();
        validate_manifest_path(&source)?;
        if matches!(source.as_str(), "manifest.json" | "checksums.sha256") {
            continue;
        }
        let size_bytes = entry.size();
        let content_hash = hash_reader(&mut entry)?;
        if payloads
            .insert(
                source,
                PayloadInfo {
                    content_hash,
                    size_bytes,
                },
            )
            .is_some()
        {
            return Err(package_invalid("package contains duplicate payload paths"));
        }
    }
    Ok(payloads)
}

fn hash_reader(reader: &mut impl Read) -> Result<String, RehomeError> {
    let mut hasher = Sha256::new();
    io::copy(reader, &mut hasher)
        .map_err(|error| package_invalid(format!("could not hash package payload: {error}")))?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn classify_file(
    source: &str,
    target: PathBuf,
    payload: &PayloadInfo,
) -> Result<PlannedOperation, RehomeError> {
    let state = target_state(&target)?;
    let (action, expected_previous_hash) = classify_target_state(&state, &payload.content_hash);
    Ok(PlannedOperation {
        package_source: source.to_owned(),
        target,
        expected_previous_hash,
        action,
        rollback_required: matches!(action, ChangeKind::Add | ChangeKind::Update),
    })
}

fn classify_merge(
    source: &str,
    target: PathBuf,
    payload: &PayloadInfo,
) -> Result<PlannedOperation, RehomeError> {
    let state = target_state(&target)?;
    let (action, expected_previous_hash) = match state {
        TargetState::Missing => (ChangeKind::Add, None),
        TargetState::File(hash) if hash == payload.content_hash => {
            (ChangeKind::Unchanged, Some(hash))
        }
        TargetState::File(hash) => (ChangeKind::Update, Some(hash)),
        TargetState::Other => (ChangeKind::Conflict, None),
    };
    Ok(PlannedOperation {
        package_source: source.to_owned(),
        target,
        expected_previous_hash,
        action,
        rollback_required: matches!(action, ChangeKind::Add | ChangeKind::Update),
    })
}

fn classify_target_state(state: &TargetState, incoming_hash: &str) -> (ChangeKind, Option<String>) {
    match state {
        TargetState::Missing => (ChangeKind::Add, None),
        TargetState::File(hash) if hash.eq_ignore_ascii_case(incoming_hash) => {
            (ChangeKind::Unchanged, Some(hash.clone()))
        }
        TargetState::File(hash) => (ChangeKind::Conflict, Some(hash.clone())),
        TargetState::Other => (ChangeKind::Conflict, None),
    }
}

fn target_state(path: &Path) -> Result<TargetState, RehomeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Ok(TargetState::Other)
        }
        Ok(_) => Ok(TargetState::File(hash_file(path)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(TargetState::Missing),
        Err(error) => Err(restore_failed(format!(
            "could not inspect restore target {}: {error}",
            path.display()
        ))),
    }
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

fn existing_hash(path: &Path, inventory_hash: &str) -> Result<Option<String>, RehomeError> {
    match target_state(path)? {
        TargetState::Missing => Ok(Some(inventory_hash.to_owned())),
        TargetState::File(hash) => Ok(Some(hash)),
        TargetState::Other => Ok(None),
    }
}

fn codex_target_path(
    codex_home: &Path,
    source: &str,
    target_os: SourceOs,
) -> Result<PathBuf, RehomeError> {
    let relative = source
        .strip_prefix("codex/")
        .ok_or_else(|| package_invalid("Codex payload is outside the codex package prefix"))?;
    join_target(codex_home, relative, target_os)
}

fn join_target(root: &Path, relative: &str, target_os: SourceOs) -> Result<PathBuf, RehomeError> {
    validate_manifest_path(relative)?;
    let root = target_path_text(root)?;
    let separator = match target_os {
        SourceOs::Windows => '\\',
        SourceOs::Macos => '/',
    };
    let root = root.trim_end_matches(['/', '\\']);
    let relative = relative.replace(['/', '\\'], &separator.to_string());
    Ok(PathBuf::from(format!("{root}{separator}{relative}")))
}

fn branch_session_target(
    original: &Path,
    derived_id: Uuid,
    target_os: SourceOs,
) -> Result<PathBuf, RehomeError> {
    let original = target_path_text(original)?;
    let separator = match target_os {
        SourceOs::Windows => '\\',
        SourceOs::Macos => '/',
    };
    let parent = original
        .rfind(separator)
        .map(|index| &original[..index])
        .ok_or_else(|| package_invalid("conversation target has no parent directory"))?;
    Ok(PathBuf::from(format!(
        "{parent}{separator}{derived_id}.jsonl"
    )))
}

fn is_target_absolute(path: &Path, target_os: SourceOs) -> Result<bool, RehomeError> {
    let value = target_path_text(path)?;
    Ok(match target_os {
        SourceOs::Macos => value.starts_with('/'),
        SourceOs::Windows => {
            let bytes = value.as_bytes();
            let drive_absolute = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            let unc_absolute = (value.starts_with(r"\\") || value.starts_with("//"))
                && value
                    .trim_start_matches(['/', '\\'])
                    .split(['/', '\\'])
                    .filter(|component| !component.is_empty())
                    .take(2)
                    .count()
                    == 2;
            drive_absolute || unc_absolute
        }
    })
}

fn target_path_text(path: &Path) -> Result<&str, RehomeError> {
    path.to_str().ok_or_else(|| {
        restore_failed("target path cannot be represented in Codex JSON metadata without loss")
    })
}

fn add_branch_rewrites(
    rewrites: &mut BTreeMap<(String, ReferenceRewriteKind, String, String), ReferenceRewrite>,
    payloads: &BTreeMap<String, PayloadInfo>,
    conversation: &crate::core::models::ConversationEntry,
    target_task_id: Uuid,
    target_title: &str,
) {
    for source in reference_sources(payloads, &conversation.archive_path) {
        insert_rewrite(
            rewrites,
            source.clone(),
            ReferenceRewriteKind::ConversationId,
            conversation.task_id.to_string(),
            target_task_id.to_string(),
        );
        insert_rewrite(
            rewrites,
            source,
            ReferenceRewriteKind::ConversationTitle,
            conversation.title.clone(),
            target_title.to_owned(),
        );
    }
}

fn add_project_path_rewrites(
    rewrites: &mut BTreeMap<(String, ReferenceRewriteKind, String, String), ReferenceRewrite>,
    payloads: &BTreeMap<String, PayloadInfo>,
    conversation: &crate::core::models::ConversationEntry,
    projects: &[crate::core::models::ProjectEntry],
    project_targets: &HashMap<Uuid, PathBuf>,
) -> Result<(), RehomeError> {
    let Some(project_id) = conversation.project_id else {
        return Ok(());
    };
    let project = projects
        .iter()
        .find(|project| project.project_id == project_id)
        .ok_or_else(|| package_invalid("conversation references an unknown package project"))?;
    let target = project_targets
        .get(&project_id)
        .ok_or_else(|| package_invalid("conversation project target is missing"))?;
    let target = target.to_str().ok_or_else(|| {
        restore_failed("target project path cannot be represented in Codex JSON metadata")
    })?;
    for source in reference_sources(payloads, &conversation.archive_path) {
        insert_rewrite(
            rewrites,
            source,
            ReferenceRewriteKind::ProjectPath,
            project.source_path.clone(),
            target.to_owned(),
        );
    }
    Ok(())
}

fn reference_sources<'a>(
    payloads: &'a BTreeMap<String, PayloadInfo>,
    conversation_source: &'a str,
) -> impl Iterator<Item = String> + 'a {
    payloads
        .keys()
        .filter(move |source| {
            *source == conversation_source
                || source.as_str() == SESSION_INDEX_SOURCE
                || source.as_str() == THREAD_METADATA_SOURCE
        })
        .cloned()
}

fn insert_rewrite(
    rewrites: &mut BTreeMap<(String, ReferenceRewriteKind, String, String), ReferenceRewrite>,
    package_source: String,
    kind: ReferenceRewriteKind,
    from: String,
    to: String,
) {
    let rewrite = ReferenceRewrite {
        package_source: package_source.clone(),
        kind,
        from: from.clone(),
        to: to.clone(),
    };
    rewrites.insert((package_source, kind, from, to), rewrite);
}

fn is_package_only_metadata(source: &str) -> bool {
    source.starts_with("projects/") && source.ends_with("/project.json")
}

fn find_state_database(codex_home: &Path) -> Result<Option<PathBuf>, RehomeError> {
    let entries = fs::read_dir(codex_home).map_err(|error| {
        restore_failed(format!(
            "could not list target Codex home {}: {error}",
            codex_home.display()
        ))
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            restore_failed(format!(
                "could not inspect a target Codex home entry: {error}"
            ))
        })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("state_") || !name.ends_with(".sqlite") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            restore_failed(format!(
                "could not inspect target state database {}: {error}",
                path.display()
            ))
        })?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            candidates.push((metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), path));
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(candidates.pop().map(|(_, path)| path))
}

fn package_invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}

fn restore_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}
