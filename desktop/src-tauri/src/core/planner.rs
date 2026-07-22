use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, PackagePreview, PlannedOperation, PlannedSession, ReferenceRewrite,
        ReferenceRewriteKind, RestorePlan, SessionAction, SourceOs, TargetInventory,
    },
    package::{inspect_package_for_planning, VerifiedPayload},
    paths::normalize_entry,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

const SESSION_INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREAD_METADATA_SOURCE: &str = "codex/metadata/threads.json";

#[derive(Debug)]
enum TargetState {
    Missing,
    File(String),
    Other,
}

type SessionDecision = (
    SessionAction,
    Uuid,
    String,
    PathBuf,
    Option<String>,
    ChangeKind,
    String,
    Vec<ReferenceRewrite>,
);

pub fn build_restore_plan(
    package: &PackagePreview,
    target: &TargetInventory,
    projects_root: &Path,
) -> Result<RestorePlan, RehomeError> {
    validate_plan_inputs(package, target, projects_root)?;
    validate_root_ancestry(&target.codex_home, target.target_os)?;
    validate_root_ancestry(projects_root, target.target_os)?;
    validate_root_separation(&target.codex_home, projects_root, target.target_os)?;
    let verified = inspect_package_for_planning(&package.package_path)?;
    if verified.preview != *package {
        return Err(package_invalid(
            "package changed after it was inspected or the preview was altered",
        ));
    }
    let payloads = &verified.payloads;

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
            operations.push(classify_file(
                source,
                target_path,
                payload,
                target.target_os,
            )?);
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
    let source_ids = package
        .manifest
        .conversations
        .iter()
        .map(|conversation| conversation.task_id)
        .collect::<HashSet<_>>();
    let mut planned_ids = HashSet::new();

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

        let source_bytes = verified
            .session_payloads
            .get(&conversation.archive_path)
            .ok_or_else(|| package_invalid("verified session payload bytes are missing"))?;
        let source_task_id = conversation.task_id;
        let original_target = codex_target_path(
            &target.codex_home,
            &conversation.archive_path,
            target.target_os,
        )?;
        let existing_target = target_conversations
            .get(&source_task_id)
            .map(|existing| {
                codex_target_path(&target.codex_home, &existing.archive_path, target.target_os)
            })
            .transpose()?
            .unwrap_or_else(|| original_target.clone());
        let base_rewrites = conversation_rewrites(
            payloads,
            conversation,
            &package.manifest.projects,
            &project_targets,
            None,
        )?;
        let base_hash =
            rewritten_content_hash(source_bytes, &base_rewrites, &conversation.archive_path)?;
        let existing_state = target_state(&existing_target, target.target_os)?;
        let (
            action,
            target_task_id,
            title,
            target_path,
            expected_previous_hash,
            change,
            expected_final_content_hash,
            selected_rewrites,
        ) = match existing_state {
            TargetState::File(hash) if hash.eq_ignore_ascii_case(&base_hash) => (
                SessionAction::Skip,
                source_task_id,
                conversation.title.clone(),
                existing_target,
                Some(hash),
                ChangeKind::Unchanged,
                base_hash,
                Vec::new(),
            ),
            TargetState::Missing => (
                SessionAction::Import,
                source_task_id,
                conversation.title.clone(),
                existing_target,
                None,
                ChangeKind::Add,
                base_hash,
                base_rewrites,
            ),
            TargetState::File(_) | TargetState::Other => plan_branch_session(
                package.manifest.package_id,
                conversation,
                source_bytes,
                &original_target,
                payloads,
                &package.manifest.projects,
                &project_targets,
                &target_conversations,
                &source_ids,
                &planned_ids,
                &target.codex_home,
                target.target_os,
            )?,
        };
        if !planned_ids.insert(target_task_id) {
            return Err(restore_failed(
                "restore plan contains duplicate conversation IDs",
            ));
        }
        for rewrite in selected_rewrites {
            insert_rewrite(
                &mut rewrites,
                rewrite.package_source,
                rewrite.kind,
                rewrite.from,
                rewrite.to,
            );
        }

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
            source_content_hash: conversation.content_hash.clone(),
            expected_final_content_hash,
            action,
        });
        consumed.insert(conversation.archive_path.clone());
    }

    for source in [SESSION_INDEX_SOURCE, THREAD_METADATA_SOURCE] {
        if payloads.contains_key(source) {
            consumed.insert(source.to_owned());
        }
    }
    if sessions
        .iter()
        .any(|session| session.action != SessionAction::Skip)
    {
        if let Some(payload) = payloads.get(SESSION_INDEX_SOURCE) {
            let target_path =
                join_target(&target.codex_home, "session_index.jsonl", target.target_os)?;
            operations.push(classify_merge(
                SESSION_INDEX_SOURCE,
                target_path,
                payload,
                target.target_os,
            )?);
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
                target.target_os,
            )?);
        }
    }

    for (source, payload) in payloads {
        if consumed.contains(source) || is_package_only_metadata(source) {
            continue;
        }
        let target_path = codex_target_path(&target.codex_home, source, target.target_os)?;
        operations.push(classify_file(
            source,
            target_path,
            payload,
            target.target_os,
        )?);
    }

    operations.sort_by(|left, right| left.package_source.cmp(&right.package_source));
    sessions.sort_by_key(|session| session.source_task_id);
    validate_final_targets(
        &operations,
        &target.codex_home,
        projects_root,
        target.target_os,
    )?;
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
        if !project_target_names.insert(normalize_target_component(
            &normalized_name,
            target.target_os,
        )) {
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

fn classify_file(
    source: &str,
    target: PathBuf,
    payload: &VerifiedPayload,
    target_os: SourceOs,
) -> Result<PlannedOperation, RehomeError> {
    let state = target_state(&target, target_os)?;
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
    payload: &VerifiedPayload,
    target_os: SourceOs,
) -> Result<PlannedOperation, RehomeError> {
    let state = target_state(&target, target_os)?;
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

fn target_state(path: &Path, target_os: SourceOs) -> Result<TargetState, RehomeError> {
    if let Some(parent) = path.parent() {
        validate_root_ancestry(parent, target_os)?;
    }
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

#[allow(clippy::too_many_arguments)]
fn plan_branch_session(
    package_id: Uuid,
    conversation: &crate::core::models::ConversationEntry,
    source_bytes: &[u8],
    original_target: &Path,
    payloads: &BTreeMap<String, VerifiedPayload>,
    projects: &[crate::core::models::ProjectEntry],
    project_targets: &HashMap<Uuid, PathBuf>,
    target_conversations: &HashMap<Uuid, &crate::core::models::ConversationEntry>,
    source_ids: &HashSet<Uuid>,
    planned_ids: &HashSet<Uuid>,
    codex_home: &Path,
    target_os: SourceOs,
) -> Result<SessionDecision, RehomeError> {
    let title = format!("{} · ReHome", conversation.title);
    for attempt in 0_u32.. {
        let candidate = derive_branch_id(package_id, conversation.task_id, attempt);
        if source_ids.contains(&candidate) || planned_ids.contains(&candidate) {
            continue;
        }
        let candidate_rewrites = conversation_rewrites(
            payloads,
            conversation,
            projects,
            project_targets,
            Some((candidate, &title)),
        )?;
        let expected_hash = rewritten_content_hash(
            source_bytes,
            &candidate_rewrites,
            &conversation.archive_path,
        )?;

        if let Some(existing) = target_conversations.get(&candidate) {
            let existing_target = codex_target_path(codex_home, &existing.archive_path, target_os)?;
            if let TargetState::File(hash) = target_state(&existing_target, target_os)? {
                if hash.eq_ignore_ascii_case(&expected_hash) {
                    return Ok((
                        SessionAction::Skip,
                        candidate,
                        title,
                        existing_target,
                        Some(hash),
                        ChangeKind::Unchanged,
                        expected_hash,
                        Vec::new(),
                    ));
                }
            }
            continue;
        }

        let candidate_target = branch_session_target(original_target, candidate, target_os)?;
        let state = target_state(&candidate_target, target_os)?;
        if let TargetState::File(hash) = &state {
            if hash.eq_ignore_ascii_case(&expected_hash) {
                return Ok((
                    SessionAction::Skip,
                    candidate,
                    title,
                    candidate_target,
                    Some(hash.clone()),
                    ChangeKind::Unchanged,
                    expected_hash,
                    Vec::new(),
                ));
            }
        }
        let (change, previous) = classify_target_state(&state, &expected_hash);
        return Ok((
            SessionAction::ImportAsBranch,
            candidate,
            title,
            candidate_target,
            previous,
            change,
            expected_hash,
            candidate_rewrites,
        ));
    }
    Err(restore_failed(
        "could not derive a collision-free conversation ID",
    ))
}

fn derive_branch_id(package_id: Uuid, source_task_id: Uuid, attempt: u32) -> Uuid {
    if attempt == 0 {
        Uuid::new_v5(&package_id, source_task_id.as_bytes())
    } else {
        Uuid::new_v5(
            &package_id,
            format!("{source_task_id}:{attempt}").as_bytes(),
        )
    }
}

fn conversation_rewrites(
    payloads: &BTreeMap<String, VerifiedPayload>,
    conversation: &crate::core::models::ConversationEntry,
    projects: &[crate::core::models::ProjectEntry],
    project_targets: &HashMap<Uuid, PathBuf>,
    branch: Option<(Uuid, &str)>,
) -> Result<Vec<ReferenceRewrite>, RehomeError> {
    let mut rewrites = BTreeMap::new();
    if let Some((target_task_id, target_title)) = branch {
        add_branch_rewrites(
            &mut rewrites,
            payloads,
            conversation,
            target_task_id,
            target_title,
        );
    }
    add_project_path_rewrites(
        &mut rewrites,
        payloads,
        conversation,
        projects,
        project_targets,
    )?;
    Ok(rewrites.into_values().collect())
}

fn rewritten_content_hash(
    bytes: &[u8],
    rewrites: &[ReferenceRewrite],
    source: &str,
) -> Result<String, RehomeError> {
    let rewritten = rewrite_jsonl_payload(bytes, rewrites, source)?;
    Ok(format!("{:x}", Sha256::digest(&rewritten)))
}

pub(crate) fn rewrite_jsonl_payload(
    bytes: &[u8],
    rewrites: &[ReferenceRewrite],
    source: &str,
) -> Result<Vec<u8>, RehomeError> {
    let selected = rewrites
        .iter()
        .filter(|rewrite| rewrite.package_source == source)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Ok(bytes.to_vec());
    }

    let text =
        std::str::from_utf8(bytes).map_err(|_| package_invalid("session payload is not UTF-8"))?;
    let mut output = Vec::with_capacity(bytes.len());
    for line in text.lines() {
        if line.is_empty() {
            output.push(b'\n');
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| package_invalid(format!("session JSONL is invalid: {error}")))?;
        rewrite_json_value(&mut value, &selected);
        serde_json::to_writer(&mut output, &value)
            .map_err(|error| package_invalid(format!("could not encode session JSONL: {error}")))?;
        output.push(b'\n');
    }
    Ok(output)
}

fn rewrite_json_value(value: &mut serde_json::Value, rewrites: &[&ReferenceRewrite]) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(rewrite) = rewrites.iter().find(|rewrite| text == &rewrite.from) {
                *text = rewrite.to.clone();
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                rewrite_json_value(value, rewrites);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                rewrite_json_value(value, rewrites);
            }
        }
        _ => {}
    }
}

fn normalize_target_component(component: &str, target_os: SourceOs) -> String {
    let normalized = component.nfc().collect::<String>();
    if target_os == SourceOs::Windows {
        normalized
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<String>()
            .nfc()
            .collect()
    } else {
        normalized
    }
}

fn target_path_key(path: &Path, target_os: SourceOs) -> Result<Vec<String>, RehomeError> {
    Ok(target_path_text(path)?
        .replace('\\', "/")
        .split('/')
        .filter(|component| !component.is_empty())
        .map(|component| normalize_target_component(component, target_os))
        .collect())
}

fn path_keys_overlap(left: &[String], right: &[String]) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn validate_root_separation(
    codex_home: &Path,
    projects_root: &Path,
    target_os: SourceOs,
) -> Result<(), RehomeError> {
    let codex_key = target_path_key(codex_home, target_os)?;
    let projects_key = target_path_key(projects_root, target_os)?;
    if path_keys_overlap(&codex_key, &projects_key) {
        return Err(restore_failed(
            "projects root and Codex home overlap on the target platform",
        ));
    }
    Ok(())
}

fn validate_final_targets(
    operations: &[PlannedOperation],
    codex_home: &Path,
    projects_root: &Path,
    target_os: SourceOs,
) -> Result<(), RehomeError> {
    validate_root_separation(codex_home, projects_root, target_os)?;
    let mut targets: Vec<(Vec<String>, &str)> = Vec::new();
    for operation in operations {
        let key = target_path_key(&operation.target, target_os)?;
        if let Some((_, source)) = targets
            .iter()
            .find(|(existing, _)| path_keys_overlap(existing, &key))
        {
            return Err(restore_failed(format!(
                "restore targets overlap after target-platform normalization: {source} and {}",
                operation.package_source
            )));
        }
        targets.push((key, operation.package_source.as_str()));
    }
    Ok(())
}

fn validate_root_ancestry(path: &Path, target_os: SourceOs) -> Result<(), RehomeError> {
    if target_os != current_source_os() {
        return Ok(());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata_is_link_or_reparse(&metadata) => {
                return Err(restore_failed(format!(
                    "restore target ancestry contains a symbolic link or reparse point: {}",
                    ancestor.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(restore_failed(format!(
                    "restore target ancestor is not a directory: {}",
                    ancestor.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(restore_failed(format!(
                    "could not inspect restore target ancestor {}: {error}",
                    ancestor.display()
                )));
            }
        }
    }
    Ok(())
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
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

fn add_branch_rewrites(
    rewrites: &mut BTreeMap<(String, ReferenceRewriteKind, String, String), ReferenceRewrite>,
    payloads: &BTreeMap<String, VerifiedPayload>,
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
    payloads: &BTreeMap<String, VerifiedPayload>,
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
    payloads: &'a BTreeMap<String, VerifiedPayload>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_target_registry_rejects_file_descendant_conflicts() {
        let operations = vec![
            PlannedOperation {
                package_source: "first".into(),
                target: PathBuf::from(r"C:\restore\item"),
                expected_previous_hash: None,
                action: ChangeKind::Add,
                rollback_required: true,
            },
            PlannedOperation {
                package_source: "second".into(),
                target: PathBuf::from(r"C:\restore\item\child"),
                expected_previous_hash: None,
                action: ChangeKind::Add,
                rollback_required: true,
            },
        ];

        let error = validate_final_targets(
            &operations,
            Path::new(r"C:\codex"),
            Path::new(r"D:\projects"),
            SourceOs::Windows,
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::RestoreFailed);
        assert!(error.message.contains("overlap"));
    }

    #[test]
    fn session_rewrites_are_recursive_exact_and_deterministic() {
        let source = "codex/sessions/thread.jsonl";
        let rewrites = vec![ReferenceRewrite {
            package_source: source.into(),
            kind: ReferenceRewriteKind::ProjectPath,
            from: "C:/old".into(),
            to: "/Users/new".into(),
        }];
        let bytes = br#"{"nested":{"cwd":"C:/old"},"note":"prefix C:/old suffix"}
"#;

        let rewritten = rewrite_jsonl_payload(bytes, &rewrites, source).unwrap();

        assert_eq!(
            rewritten,
            br#"{"nested":{"cwd":"/Users/new"},"note":"prefix C:/old suffix"}
"#
        );
    }
}
