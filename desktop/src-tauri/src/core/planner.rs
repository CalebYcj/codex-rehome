use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{
        BridgeVerificationRequirements, ChangeKind, PackagePreview, PlannedOperation,
        PlannedSession, ReferenceRewrite, ReferenceRewriteKind, RestorePlan, SessionAction,
        SourceOs, TargetInventory,
    },
    package::{inspect_package_for_planning, VerifiedPayload},
    paths::normalize_entry,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

const SESSION_INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREAD_METADATA_SOURCE: &str = "codex/metadata/threads.json";
const PLUGIN_CACHE_PREFIX: &str = "codex/plugins/cache/";
const MODERN_PLUGIN_MARKER_SUFFIX: &str = "/.codex-plugin/plugin.json";

#[derive(Debug)]
enum TargetState {
    Missing,
    File(String),
    Other,
}

#[derive(Debug, Clone, Copy)]
enum PluginRootDisposition {
    Preserve,
    Conflict,
}

#[derive(Debug)]
struct PluginRootDecision {
    archive_root: String,
    disposition: PluginRootDisposition,
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

type RewriteMap = BTreeMap<(Uuid, String, ReferenceRewriteKind, String, String), ReferenceRewrite>;

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
    let plugin_root_decisions =
        plugin_root_decisions(payloads, &target.codex_home, target.target_os)?;

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
            .planning_payloads
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
            &verified.planning_payloads,
            conversation,
            &package.manifest.projects,
            &project_targets,
            None,
            &existing_target,
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
                base_rewrites,
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
                &verified.planning_payloads,
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
                rewrite.source_task_id,
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
    let mut bridge_verification = BridgeVerificationRequirements::default();
    if !sessions.is_empty() {
        let planned_rewrites = rewrites.values().cloned().collect::<Vec<_>>();
        let desired_thread_rollout_paths = sessions
            .iter()
            .map(|session| {
                Ok((
                    session.target_task_id.to_string(),
                    target_path_text(&session.target)?.to_owned(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, RehomeError>>()?;
        let all_sessions_skipped = sessions
            .iter()
            .all(|session| session.action == SessionAction::Skip);
        if payloads.contains_key(SESSION_INDEX_SOURCE) {
            let target_path =
                join_target(&target.codex_home, "session_index.jsonl", target.target_os)?;
            bridge_verification.session_index = Some(target_path.clone());
            let bytes = verified
                .planning_payloads
                .get(SESSION_INDEX_SOURCE)
                .ok_or_else(|| package_invalid("verified session index bytes are missing"))?;
            if let Some(operation) = plan_index_merge(
                SESSION_INDEX_SOURCE,
                target_path,
                bytes,
                &planned_rewrites,
                all_sessions_skipped,
                target.target_os,
            )? {
                operations.push(operation);
            }
        }
        if payloads.contains_key(THREAD_METADATA_SOURCE) {
            let target_path = find_state_database(&target.codex_home)?.ok_or_else(|| {
                RehomeError::new(
                    ErrorCode::CodexNotFound,
                    "target Codex state database was not found",
                )
            })?;
            bridge_verification.sqlite_database = Some(target_path.clone());
            let bytes = verified
                .planning_payloads
                .get(THREAD_METADATA_SOURCE)
                .ok_or_else(|| package_invalid("verified thread metadata bytes are missing"))?;
            if let Some(operation) = plan_thread_metadata_merge(
                THREAD_METADATA_SOURCE,
                target_path,
                bytes,
                &planned_rewrites,
                &desired_thread_rollout_paths,
                all_sessions_skipped,
                target.target_os,
            )? {
                operations.push(operation);
            }
        }
    }

    for (source, payload) in payloads {
        if consumed.contains(source) || is_package_only_metadata(source) {
            continue;
        }
        let target_path = codex_target_path(&target.codex_home, source, target.target_os)?;
        let operation = match plugin_decision_for_source(source, &plugin_root_decisions) {
            Some(disposition) => {
                classify_plugin_file(source, target_path, disposition, target.target_os)?
            }
            None => classify_file(source, target_path, payload, target.target_os)?,
        };
        operations.push(operation);
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

    let mut plan = RestorePlan {
        plan_id: Uuid::nil(),
        package_path: package.package_path.clone(),
        package_id: package.manifest.package_id,
        archive_hash: package.archive_hash.clone(),
        target_codex_home: target.codex_home.clone(),
        projects_root: projects_root.to_path_buf(),
        operations,
        sessions,
        reference_rewrites,
        bridge_verification,
        conflict_count,
        required_bytes,
    };
    let canonical = serde_json::to_vec(&plan)
        .map_err(|error| restore_failed(format!("could not seal restore plan: {error}")))?;
    plan.plan_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, &canonical);
    crate::core::plan_store::store(&plan)?;
    Ok(plan)
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

fn plugin_root_decisions(
    payloads: &BTreeMap<String, VerifiedPayload>,
    codex_home: &Path,
    target_os: SourceOs,
) -> Result<Vec<PluginRootDecision>, RehomeError> {
    let mut candidates = BTreeMap::new();
    for source in payloads.keys() {
        let root = if let Some(root) = source.strip_suffix(MODERN_PLUGIN_MARKER_SUFFIX) {
            Some(root)
        } else if source.starts_with(PLUGIN_CACHE_PREFIX) && source.ends_with("/manifest.json") {
            source.rsplit_once('/').map(|(parent, _)| parent)
        } else {
            None
        };
        let Some(root) = root.filter(|root| {
            root.strip_prefix(PLUGIN_CACHE_PREFIX)
                .is_some_and(|relative| !relative.is_empty())
        }) else {
            continue;
        };
        candidates
            .entry(root.to_owned())
            .or_insert_with(|| source.clone());
    }

    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.0
            .split('/')
            .count()
            .cmp(&right.0.split('/').count())
            .then(left.0.cmp(&right.0))
    });
    let mut selected: Vec<(String, String)> = Vec::new();
    for candidate in candidates {
        if selected.iter().any(|(root, _)| {
            candidate.0 == *root
                || candidate
                    .0
                    .strip_prefix(root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) {
            continue;
        }
        selected.push(candidate);
    }

    let mut decisions = Vec::new();
    for (archive_root, marker_source) in selected {
        let target_marker = codex_target_path(codex_home, &marker_source, target_os)?;
        let disposition = match target_state(&target_marker, target_os)? {
            TargetState::File(_) => PluginRootDisposition::Preserve,
            TargetState::Other => PluginRootDisposition::Conflict,
            TargetState::Missing => {
                let target_root = codex_target_path(codex_home, &archive_root, target_os)?;
                match fs::symlink_metadata(&target_root) {
                    Ok(_) => PluginRootDisposition::Conflict,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(restore_failed(format!(
                            "could not inspect target plugin root {}: {error}",
                            target_root.display()
                        )))
                    }
                }
            }
        };
        decisions.push(PluginRootDecision {
            archive_root,
            disposition,
        });
    }
    Ok(decisions)
}

fn plugin_decision_for_source(
    source: &str,
    decisions: &[PluginRootDecision],
) -> Option<PluginRootDisposition> {
    decisions
        .iter()
        .find(|decision| {
            source
                .strip_prefix(&decision.archive_root)
                .is_some_and(|suffix| suffix.starts_with('/'))
        })
        .map(|decision| decision.disposition)
}

fn classify_plugin_file(
    source: &str,
    target: PathBuf,
    disposition: PluginRootDisposition,
    target_os: SourceOs,
) -> Result<PlannedOperation, RehomeError> {
    let state = target_state(&target, target_os)?;
    let (action, expected_previous_hash) = match (disposition, state) {
        (PluginRootDisposition::Preserve, TargetState::File(hash)) => {
            (ChangeKind::Preserve, Some(hash))
        }
        (PluginRootDisposition::Preserve, TargetState::Missing) => (ChangeKind::Preserve, None),
        (PluginRootDisposition::Preserve, TargetState::Other)
        | (PluginRootDisposition::Conflict, TargetState::Other)
        | (PluginRootDisposition::Conflict, TargetState::Missing) => (ChangeKind::Conflict, None),
        (PluginRootDisposition::Conflict, TargetState::File(hash)) => {
            (ChangeKind::Conflict, Some(hash))
        }
    };
    Ok(PlannedOperation {
        package_source: source.to_owned(),
        target,
        expected_previous_hash,
        action,
        rollback_required: false,
    })
}

fn plan_index_merge(
    source: &str,
    target: PathBuf,
    bytes: &[u8],
    rewrites: &[ReferenceRewrite],
    all_sessions_skipped: bool,
    target_os: SourceOs,
) -> Result<Option<PlannedOperation>, RehomeError> {
    let state = target_state(&target, target_os)?;
    if let TargetState::File(hash) = &state {
        let desired = rewrite_jsonl_payload(bytes, rewrites, source)?;
        let current = read_hashed_target(&target, hash)?;
        if all_sessions_skipped && jsonl_metadata_contains(&current, &desired)? {
            return Ok(None);
        }
    }
    Ok(Some(classify_bridge_change(source, target, state)))
}

fn plan_thread_metadata_merge(
    source: &str,
    target: PathBuf,
    bytes: &[u8],
    rewrites: &[ReferenceRewrite],
    desired_rollout_paths: &BTreeMap<String, String>,
    all_sessions_skipped: bool,
    target_os: SourceOs,
) -> Result<Option<PlannedOperation>, RehomeError> {
    let state = target_state(&target, target_os)?;
    if let TargetState::File(hash) = &state {
        let desired = rewrite_metadata_document(bytes, rewrites, source, desired_rollout_paths)?;
        let metadata_ready = sqlite_threads_contain(&target, &desired)?;
        ensure_target_hash(&target, hash)?;
        if all_sessions_skipped && metadata_ready {
            return Ok(None);
        }
    }
    Ok(Some(classify_bridge_change(source, target, state)))
}

fn classify_bridge_change(source: &str, target: PathBuf, state: TargetState) -> PlannedOperation {
    let (action, expected_previous_hash) = match state {
        TargetState::Missing => (ChangeKind::Add, None),
        TargetState::File(hash) => (ChangeKind::Update, Some(hash)),
        TargetState::Other => (ChangeKind::Conflict, None),
    };
    PlannedOperation {
        package_source: source.to_owned(),
        target,
        expected_previous_hash,
        action,
        rollback_required: matches!(action, ChangeKind::Add | ChangeKind::Update),
    }
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
    planning_payloads: &BTreeMap<String, Vec<u8>>,
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
        if let Some(existing) = target_conversations.get(&candidate) {
            let existing_target = codex_target_path(codex_home, &existing.archive_path, target_os)?;
            let candidate_rewrites = conversation_rewrites(
                payloads,
                planning_payloads,
                conversation,
                projects,
                project_targets,
                Some((candidate, &title)),
                &existing_target,
            )?;
            let expected_hash = rewritten_content_hash(
                source_bytes,
                &candidate_rewrites,
                &conversation.archive_path,
            )?;
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
                        candidate_rewrites,
                    ));
                }
            }
            continue;
        }

        let candidate_target = branch_session_target(original_target, candidate, target_os)?;
        let candidate_rewrites = conversation_rewrites(
            payloads,
            planning_payloads,
            conversation,
            projects,
            project_targets,
            Some((candidate, &title)),
            &candidate_target,
        )?;
        let expected_hash = rewritten_content_hash(
            source_bytes,
            &candidate_rewrites,
            &conversation.archive_path,
        )?;
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
                    candidate_rewrites,
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
    planning_payloads: &BTreeMap<String, Vec<u8>>,
    conversation: &crate::core::models::ConversationEntry,
    projects: &[crate::core::models::ProjectEntry],
    project_targets: &HashMap<Uuid, PathBuf>,
    branch: Option<(Uuid, &str)>,
    target_session: &Path,
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
        planning_payloads,
        conversation,
        projects,
        project_targets,
    )?;
    add_session_path_rewrites(
        &mut rewrites,
        planning_payloads,
        conversation.task_id,
        target_session,
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
        if source == SESSION_INDEX_SOURCE {
            rewrite_scoped_metadata_row(&mut value, &selected);
        } else {
            rewrite_json_value(&mut value, &selected);
        }
        serde_json::to_writer(&mut output, &value)
            .map_err(|error| package_invalid(format!("could not encode session JSONL: {error}")))?;
        output.push(b'\n');
    }
    Ok(output)
}

fn rewrite_scoped_metadata_row(value: &mut serde_json::Value, rewrites: &[&ReferenceRewrite]) {
    let Some(source_id) = metadata_id(value) else {
        return;
    };
    let scoped = rewrites
        .iter()
        .copied()
        .filter(|rewrite| rewrite.source_task_id.to_string() == source_id)
        .collect::<Vec<_>>();
    rewrite_json_value(value, &scoped);
}

fn rewrite_json_value(value: &mut serde_json::Value, rewrites: &[&ReferenceRewrite]) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    match object.get("type").and_then(serde_json::Value::as_str) {
        Some("session_meta") => {
            if let Some(payload) = object.get_mut("payload") {
                rewrite_known_metadata_fields(payload, rewrites, true);
            }
        }
        Some("turn_context") => {
            if let Some(payload) = object.get_mut("payload") {
                rewrite_known_metadata_fields(payload, rewrites, false);
            }
        }
        Some(_) => {}
        None => rewrite_known_metadata_fields(value, rewrites, true),
    }
}

fn rewrite_known_metadata_fields(
    value: &mut serde_json::Value,
    rewrites: &[&ReferenceRewrite],
    include_identity: bool,
) {
    if let Some(values) = value.as_array_mut() {
        for value in values {
            rewrite_known_metadata_fields(value, rewrites, include_identity);
        }
        return;
    }
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (field, value) in object {
        if !matches!(
            field.as_str(),
            "message" | "messages" | "content" | "text" | "input" | "output" | "instructions"
        ) {
            rewrite_known_metadata_fields(value, rewrites, include_identity);
        }
        let Some(text) = value.as_str() else {
            continue;
        };
        if let Some(rewrite) = rewrites.iter().find(|rewrite| {
            text == rewrite.from
                && match rewrite.kind {
                    ReferenceRewriteKind::ConversationId => {
                        include_identity
                            && matches!(field.as_str(), "id" | "thread_id" | "conversation_id")
                    }
                    ReferenceRewriteKind::ConversationTitle => include_identity && field == "title",
                    ReferenceRewriteKind::ProjectPath => {
                        matches!(field.as_str(), "project" | "project_path" | "cwd" | "path")
                    }
                    ReferenceRewriteKind::SessionPath => {
                        matches!(field.as_str(), "rollout" | "rollout_path")
                    }
                }
        }) {
            *value = serde_json::Value::String(rewrite.to.clone());
        }
    }
}

fn rewrite_metadata_document(
    bytes: &[u8],
    rewrites: &[ReferenceRewrite],
    source: &str,
    desired_rollout_paths: &BTreeMap<String, String>,
) -> Result<serde_json::Value, RehomeError> {
    let selected = rewrites
        .iter()
        .filter(|rewrite| rewrite.package_source == source)
        .collect::<Vec<_>>();
    let mut value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| package_invalid(format!("bridge metadata JSON is invalid: {error}")))?;
    match &mut value {
        serde_json::Value::Array(values) => {
            for value in values {
                rewrite_scoped_metadata_row(value, &selected);
                set_desired_rollout_path(value, desired_rollout_paths);
            }
        }
        serde_json::Value::Object(_) => {
            rewrite_scoped_metadata_row(&mut value, &selected);
            set_desired_rollout_path(&mut value, desired_rollout_paths);
        }
        _ => return Err(package_invalid("bridge metadata JSON has an invalid shape")),
    }
    Ok(value)
}

fn set_desired_rollout_path(
    value: &mut serde_json::Value,
    desired_rollout_paths: &BTreeMap<String, String>,
) {
    let desired = metadata_id(value)
        .and_then(|id| desired_rollout_paths.get(id))
        .cloned();
    if let (Some(object), Some(desired)) = (value.as_object_mut(), desired) {
        object.insert(
            "rollout_path".to_owned(),
            serde_json::Value::String(desired),
        );
    }
}

fn read_hashed_target(path: &Path, expected_hash: &str) -> Result<Vec<u8>, RehomeError> {
    let bytes = fs::read(path).map_err(|error| {
        restore_failed(format!(
            "could not read restore target {}: {error}",
            path.display()
        ))
    })?;
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
        return Err(restore_failed(format!(
            "restore target changed while it was being planned: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn ensure_target_hash(path: &Path, expected_hash: &str) -> Result<(), RehomeError> {
    let actual_hash = hash_file(path)?;
    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
        return Err(restore_failed(format!(
            "restore target changed while it was being planned: {}",
            path.display()
        )));
    }
    Ok(())
}

fn jsonl_metadata_contains(current: &[u8], desired: &[u8]) -> Result<bool, RehomeError> {
    let (desired, _) = jsonl_metadata_by_id(desired, true)?;
    let (current, current_has_duplicates) = jsonl_metadata_by_id(current, false)?;
    if current_has_duplicates {
        return Ok(false);
    }
    Ok(desired.iter().all(|(id, desired_value)| {
        current
            .get(id)
            .is_some_and(|current_value| metadata_fields_match(current_value, desired_value))
    }))
}

fn metadata_fields_match(current: &serde_json::Value, desired: &serde_json::Value) -> bool {
    match (current.as_object(), desired.as_object()) {
        (Some(current), Some(desired)) => desired
            .iter()
            .all(|(field, value)| current.get(field) == Some(value)),
        _ => current == desired,
    }
}

fn jsonl_metadata_by_id(
    bytes: &[u8],
    package_metadata: bool,
) -> Result<(BTreeMap<String, serde_json::Value>, bool), RehomeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        if package_metadata {
            package_invalid("session index is not UTF-8")
        } else {
            restore_failed("target session index is not UTF-8")
        }
    })?;
    let mut records = BTreeMap::new();
    let mut has_duplicates = false;
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
            if package_metadata {
                package_invalid(format!("session index JSONL is invalid: {error}"))
            } else {
                restore_failed(format!("target session index JSONL is invalid: {error}"))
            }
        })?;
        let id = metadata_id(&value).ok_or_else(|| {
            if package_metadata {
                package_invalid("session index entry is missing its conversation ID")
            } else {
                restore_failed("target session index entry is missing its conversation ID")
            }
        })?;
        if records.insert(id.to_owned(), value).is_some() {
            if package_metadata {
                return Err(package_invalid(
                    "session index contains duplicate conversation IDs",
                ));
            }
            has_duplicates = true;
        }
    }
    Ok((records, has_duplicates))
}

fn metadata_id(value: &serde_json::Value) -> Option<&str> {
    let object = value.as_object()?;
    ["id", "thread_id", "conversation_id"]
        .iter()
        .find_map(|field| object.get(*field).and_then(serde_json::Value::as_str))
}

fn sqlite_threads_contain(
    database: &Path,
    desired: &serde_json::Value,
) -> Result<bool, RehomeError> {
    let rows = desired
        .as_array()
        .ok_or_else(|| package_invalid("thread metadata must be a JSON array"))?;
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            restore_failed(format!(
                "could not open target Codex state database {}: {error}",
                database.display()
            ))
        })?;
    let columns = sqlite_thread_columns(&connection)?;
    if !columns.contains("id") {
        return Ok(false);
    }

    for desired_row in rows {
        let object = desired_row
            .as_object()
            .ok_or_else(|| package_invalid("thread metadata row must be a JSON object"))?;
        let id = metadata_id(desired_row)
            .ok_or_else(|| package_invalid("thread metadata row is missing its conversation ID"))?;
        if object.contains_key("rollout_path") && !columns.contains("rollout_path") {
            return Ok(false);
        }
        let compared_columns = object
            .keys()
            .filter(|column| columns.contains(column.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let projection = compared_columns
            .iter()
            .map(|column| quote_sql_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!("SELECT {projection} FROM threads WHERE id = ?1");
        let mut statement = connection.prepare(&query).map_err(|error| {
            restore_failed(format!(
                "could not inspect target Codex thread {id}: {error}"
            ))
        })?;
        let mut query_rows = statement.query([id]).map_err(|error| {
            restore_failed(format!("could not query target Codex thread {id}: {error}"))
        })?;
        let Some(current_row) = query_rows.next().map_err(|error| {
            restore_failed(format!("could not read target Codex thread {id}: {error}"))
        })?
        else {
            return Ok(false);
        };
        for (index, column) in compared_columns.iter().enumerate() {
            let current = sqlite_value_as_json(current_row.get_ref(index).map_err(|error| {
                restore_failed(format!("could not read target Codex thread field: {error}"))
            })?);
            if object.get(column) != Some(&current) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn sqlite_thread_columns(connection: &Connection) -> Result<HashSet<String>, RehomeError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| restore_failed(format!("could not inspect target threads: {error}")))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| restore_failed(format!("could not inspect target threads: {error}")))?;
    let mut columns = HashSet::new();
    for name in names {
        columns.insert(name.map_err(|error| {
            restore_failed(format!("could not read target thread schema: {error}"))
        })?);
    }
    Ok(columns)
}

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn sqlite_value_as_json(value: ValueRef<'_>) -> serde_json::Value {
    match value {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(value) => serde_json::Value::from(value),
        ValueRef::Real(value) => serde_json::Value::from(value),
        ValueRef::Text(value) => {
            serde_json::Value::String(String::from_utf8_lossy(value).into_owned())
        }
        ValueRef::Blob(value) => serde_json::Value::String(
            value
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        ),
    }
}

fn normalize_target_component(component: &str, target_os: SourceOs) -> String {
    let normalized = component.nfc().collect::<String>();
    if matches!(target_os, SourceOs::Windows | SourceOs::Macos) {
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
    rewrites: &mut RewriteMap,
    payloads: &BTreeMap<String, VerifiedPayload>,
    conversation: &crate::core::models::ConversationEntry,
    target_task_id: Uuid,
    target_title: &str,
) {
    for source in reference_sources(payloads, &conversation.archive_path) {
        insert_rewrite(
            rewrites,
            conversation.task_id,
            source.clone(),
            ReferenceRewriteKind::ConversationId,
            conversation.task_id.to_string(),
            target_task_id.to_string(),
        );
        insert_rewrite(
            rewrites,
            conversation.task_id,
            source,
            ReferenceRewriteKind::ConversationTitle,
            conversation.title.clone(),
            target_title.to_owned(),
        );
    }
}

fn add_project_path_rewrites(
    rewrites: &mut RewriteMap,
    payloads: &BTreeMap<String, VerifiedPayload>,
    planning_payloads: &BTreeMap<String, Vec<u8>>,
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
        let mut source_paths = BTreeSet::new();
        for source_path in windows_source_path_variants(&project.source_path) {
            source_paths.insert(source_path);
        }
        let bytes = planning_payloads.get(&source).ok_or_else(|| {
            package_invalid(format!("verified payload bytes are missing for {source}"))
        })?;
        let metadata_paths = if source == conversation.archive_path {
            session_project_paths(bytes, &source)?
        } else {
            metadata_project_paths(bytes, &source, conversation.task_id)?
        };
        for source_path in metadata_paths {
            for source_path in windows_source_path_variants(&source_path) {
                source_paths.insert(source_path);
            }
        }
        for source_path in source_paths {
            insert_rewrite(
                rewrites,
                conversation.task_id,
                source.clone(),
                ReferenceRewriteKind::ProjectPath,
                source_path,
                target.to_owned(),
            );
        }
    }
    Ok(())
}

fn add_session_path_rewrites(
    rewrites: &mut RewriteMap,
    planning_payloads: &BTreeMap<String, Vec<u8>>,
    source_task_id: Uuid,
    target_session: &Path,
) -> Result<(), RehomeError> {
    let target = target_path_text(target_session)?.to_owned();
    for source in [SESSION_INDEX_SOURCE, THREAD_METADATA_SOURCE] {
        let Some(bytes) = planning_payloads.get(source) else {
            continue;
        };
        for source_rollout in metadata_rollout_paths(bytes, source, source_task_id)? {
            for source_path in windows_source_path_variants(&source_rollout) {
                insert_rewrite(
                    rewrites,
                    source_task_id,
                    source.to_owned(),
                    ReferenceRewriteKind::SessionPath,
                    source_path,
                    target.clone(),
                );
            }
        }
    }
    Ok(())
}

fn windows_source_path_variants(path: &str) -> Vec<String> {
    let backslash = path.replace('/', "\\");
    let (body, is_unc) = if let Some(body) = backslash.strip_prefix(r"\\?\UNC\") {
        (body, true)
    } else if let Some(body) = backslash.strip_prefix(r"\\?\") {
        (body, false)
    } else if let Some(body) = backslash.strip_prefix(r"\\") {
        (body, true)
    } else if is_windows_drive_path(&backslash) {
        (backslash.as_str(), false)
    } else {
        return vec![path.to_owned()];
    };

    let mut variants = std::collections::BTreeSet::new();
    if is_unc {
        let body = body.trim_start_matches('\\');
        variants.insert(format!(r"\\{body}"));
        variants.insert(format!("//{}", body.replace('\\', "/")));
        variants.insert(format!(r"\\?\UNC\{body}"));
    } else if is_windows_drive_path(body) {
        variants.insert(body.to_owned());
        variants.insert(body.replace('\\', "/"));
        variants.insert(format!(r"\\?\{body}"));
    } else {
        variants.insert(path.to_owned());
    }
    variants.into_iter().collect()
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

fn metadata_rollout_paths(
    bytes: &[u8],
    source: &str,
    source_task_id: Uuid,
) -> Result<Vec<String>, RehomeError> {
    let values = metadata_rows(bytes, source)?;
    let source_task_id = source_task_id.to_string();
    Ok(values
        .iter()
        .filter(|value| metadata_id(value) == Some(source_task_id.as_str()))
        .filter_map(|value| {
            value
                .as_object()
                .and_then(|object| object.get("rollout_path"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect())
}

fn metadata_project_paths(
    bytes: &[u8],
    source: &str,
    source_task_id: Uuid,
) -> Result<Vec<String>, RehomeError> {
    let source_task_id = source_task_id.to_string();
    let mut paths = BTreeSet::new();
    for value in metadata_rows(bytes, source)?
        .iter()
        .filter(|value| metadata_id(value) == Some(source_task_id.as_str()))
    {
        collect_metadata_project_paths(value, &mut paths);
    }
    Ok(paths.into_iter().collect())
}

fn session_project_paths(bytes: &[u8], source: &str) -> Result<Vec<String>, RehomeError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| package_invalid("session payload is not UTF-8"))?;
    let mut paths = BTreeSet::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
            package_invalid(format!("session JSONL is invalid for {source}: {error}"))
        })?;
        collect_metadata_project_paths(&value, &mut paths);
    }
    Ok(paths.into_iter().collect())
}

fn metadata_rows(bytes: &[u8], source: &str) -> Result<Vec<serde_json::Value>, RehomeError> {
    if source == SESSION_INDEX_SOURCE {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| package_invalid("session index is not UTF-8"))?;
        Ok(text
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                serde_json::from_str(line).map_err(|error| {
                    package_invalid(format!("session index JSONL is invalid: {error}"))
                })
            })
            .collect::<Result<Vec<serde_json::Value>, RehomeError>>()?)
    } else {
        Ok(serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|error| package_invalid(format!("bridge metadata JSON is invalid: {error}")))?
            .as_array()
            .cloned()
            .ok_or_else(|| package_invalid("thread metadata must be a JSON array"))?)
    }
}

fn collect_metadata_project_paths(value: &serde_json::Value, paths: &mut BTreeSet<String>) {
    let Some(object) = value.as_object() else {
        if let Some(values) = value.as_array() {
            for value in values {
                collect_metadata_project_paths(value, paths);
            }
        }
        return;
    };

    for (field, value) in object {
        if matches!(field.as_str(), "cwd" | "project" | "project_path") {
            if let Some(value) = value
                .as_str()
                .filter(|value| looks_like_absolute_path(value))
            {
                paths.insert(value.to_owned());
            }
        }
        if !matches!(
            field.as_str(),
            "message" | "messages" | "content" | "text" | "input" | "output" | "instructions"
        ) {
            collect_metadata_project_paths(value, paths);
        }
    }
}

fn looks_like_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with(r"\\")
        || is_windows_drive_path(&value.replace('/', "\\"))
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
    rewrites: &mut RewriteMap,
    source_task_id: Uuid,
    package_source: String,
    kind: ReferenceRewriteKind,
    from: String,
    to: String,
) {
    let rewrite = ReferenceRewrite {
        source_task_id,
        package_source: package_source.clone(),
        kind,
        from: from.clone(),
        to: to.clone(),
    };
    rewrites.insert((source_task_id, package_source, kind, from, to), rewrite);
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
    fn final_target_registry_uses_default_macos_collision_keys() {
        for (first, second) in [
            ("/restore/Visual/item", "/restore/visual/item"),
            ("/restore/Caf\u{00e9}/item", "/restore/Cafe\u{0301}/item"),
        ] {
            let operations = [first, second]
                .into_iter()
                .enumerate()
                .map(|(index, target)| PlannedOperation {
                    package_source: format!("source-{index}"),
                    target: PathBuf::from(target),
                    expected_previous_hash: None,
                    action: ChangeKind::Add,
                    rollback_required: true,
                })
                .collect::<Vec<_>>();

            let error = validate_final_targets(
                &operations,
                Path::new("/codex"),
                Path::new("/projects"),
                SourceOs::Macos,
            )
            .unwrap_err();

            assert_eq!(error.code, ErrorCode::RestoreFailed);
            assert!(error.message.contains("overlap"));
        }
    }

    #[test]
    fn session_rewrites_are_schema_aware_exact_and_deterministic() {
        let source = "codex/sessions/thread.jsonl";
        let old_id = "11111111-1111-4111-8111-111111111111";
        let new_id = "22222222-2222-4222-8222-222222222222";
        let source_task_id = Uuid::parse_str(old_id).unwrap();
        let rewrites = vec![
            ReferenceRewrite {
                source_task_id,
                package_source: source.into(),
                kind: ReferenceRewriteKind::ProjectPath,
                from: "C:/old".into(),
                to: "/Users/new".into(),
            },
            ReferenceRewrite {
                source_task_id,
                package_source: source.into(),
                kind: ReferenceRewriteKind::ConversationId,
                from: old_id.into(),
                to: new_id.into(),
            },
            ReferenceRewrite {
                source_task_id,
                package_source: source.into(),
                kind: ReferenceRewriteKind::ConversationTitle,
                from: "Old title".into(),
                to: "New title".into(),
            },
            ReferenceRewrite {
                source_task_id,
                package_source: source.into(),
                kind: ReferenceRewriteKind::SessionPath,
                from: "C:/old/session.jsonl".into(),
                to: "/Users/new/session.jsonl".into(),
            },
        ];
        let bytes = format!(
            concat!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{old_id}\",\"title\":\"Old title\",\"cwd\":\"C:/old\",\"rollout_path\":\"C:/old/session.jsonl\"}}}}\n",
                "{{\"type\":\"turn_context\",\"payload\":{{\"cwd\":\"C:/old\"}}}}\n",
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Old title\"}},{{\"type\":\"input_text\",\"text\":\"{old_id}\"}},{{\"type\":\"input_text\",\"text\":\"C:/old\"}},{{\"type\":\"input_text\",\"text\":\"C:/old/session.jsonl\"}}]}}}}\n",
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"Old title\"}},{{\"type\":\"output_text\",\"text\":\"{old_id}\"}},{{\"type\":\"output_text\",\"text\":\"C:/old\"}}]}}}}\n",
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call_output\",\"output\":\"C:/old\",\"id\":\"{old_id}\",\"title\":\"Old title\"}}}}\n"
            ),
            old_id = old_id,
        );

        let rewritten = rewrite_jsonl_payload(bytes.as_bytes(), &rewrites, source).unwrap();
        let rewritten = String::from_utf8(rewritten).unwrap();
        let lines = rewritten
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(lines[0]["payload"]["id"], new_id);
        assert_eq!(lines[0]["payload"]["title"], "New title");
        assert_eq!(lines[0]["payload"]["cwd"], "/Users/new");
        assert_eq!(
            lines[0]["payload"]["rollout_path"],
            "/Users/new/session.jsonl"
        );
        assert_eq!(lines[1]["payload"]["cwd"], "/Users/new");
        assert_eq!(lines[2]["payload"]["content"][0]["text"], "Old title");
        assert_eq!(lines[2]["payload"]["content"][1]["text"], old_id);
        assert_eq!(lines[2]["payload"]["content"][2]["text"], "C:/old");
        assert_eq!(
            lines[2]["payload"]["content"][3]["text"],
            "C:/old/session.jsonl"
        );
        assert_eq!(lines[3]["payload"]["content"][0]["text"], "Old title");
        assert_eq!(lines[3]["payload"]["content"][1]["text"], old_id);
        assert_eq!(lines[3]["payload"]["content"][2]["text"], "C:/old");
        assert_eq!(lines[4]["payload"]["output"], "C:/old");
        assert_eq!(lines[4]["payload"]["id"], old_id);
        assert_eq!(lines[4]["payload"]["title"], "Old title");
    }
}
