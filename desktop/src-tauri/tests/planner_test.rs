use rehome_desktop_lib::core::{
    bridge::apply_bridge_plan,
    error::ErrorCode,
    models::{
        ChangeKind, ContentCounts, ConversationEntry, ExclusionSummary, PackageManifest,
        PackageMode, PackagePreview, ProjectEntry, ReferenceRewriteKind, SessionAction, SourceOs,
        TargetInventory,
    },
    package::inspect_package,
    planner::build_restore_plan,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipWriter};

const PACKAGE_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PROJECT_ID: &str = "22222222-2222-4222-8222-222222222222";
const TASK_ID: &str = "11111111-1111-4111-8111-111111111111";
const SECOND_PROJECT_ID: &str = "33333333-3333-4333-8333-333333333333";
const SECOND_TASK_ID: &str = "44444444-4444-4444-8444-444444444444";
const PROJECT_SOURCE: &str = "projects/22222222-2222-4222-8222-222222222222/files/README.md";
const SESSION_SOURCE: &str = "codex/sessions/2026/07/22/thread.jsonl";
const SOURCE_ROLLOUT_PATH: &str = "C:/Users/OldUser/.codex/sessions/2026/07/22/thread.jsonl";
const INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREADS_SOURCE: &str = "codex/metadata/threads.json";
const PLUGIN_MARKER_SOURCE: &str =
    "codex/plugins/cache/openai-bundled/browser/1.2.3/.codex-plugin/plugin.json";
const PLUGIN_RUNTIME_SOURCE: &str =
    "codex/plugins/cache/openai-bundled/browser/1.2.3/assets/runtime.js";

struct PlannerFixture {
    _temp: TempDir,
    preview: PackagePreview,
    target: TargetInventory,
    projects_root: PathBuf,
    project_target: PathBuf,
}

#[test]
fn classifies_project_files_from_target_state() -> Result<(), Box<dyn Error>> {
    struct Case {
        name: &'static str,
        target_bytes: Option<&'static [u8]>,
        expected: ChangeKind,
    }

    for case in [
        Case {
            name: "target_missing",
            target_bytes: None,
            expected: ChangeKind::Add,
        },
        Case {
            name: "same_hash",
            target_bytes: Some(b"incoming project\n"),
            expected: ChangeKind::Unchanged,
        },
        Case {
            name: "target_present_without_baseline",
            target_bytes: Some(b"local project\n"),
            expected: ChangeKind::Conflict,
        },
    ] {
        let fixture = planner_fixture(None)?;
        if let Some(bytes) = case.target_bytes {
            fs::create_dir_all(fixture.project_target.parent().unwrap())?;
            fs::write(&fixture.project_target, bytes)?;
        }

        let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
        let operation = operation_for(&plan.operations, PROJECT_SOURCE);

        assert_eq!(operation.action, case.expected, "{}", case.name);
        assert_eq!(operation.package_source, PROJECT_SOURCE, "{}", case.name);
        assert_eq!(operation.target, fixture.project_target, "{}", case.name);
        assert_eq!(
            operation.expected_previous_hash,
            case.target_bytes.map(checksum),
            "{}",
            case.name
        );
        assert_eq!(
            operation.rollback_required,
            matches!(case.expected, ChangeKind::Add | ChangeKind::Update),
            "{}",
            case.name
        );
        assert_eq!(
            plan.conflict_count,
            u64::from(case.expected == ChangeKind::Conflict),
            "{}",
            case.name
        );
    }

    Ok(())
}

#[test]
fn existing_plugin_version_is_preserved_as_a_complete_unit() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    add_plugin_payloads(&mut fixture)?;
    let plugin_root = fixture
        .target
        .codex_home
        .join("plugins/cache/openai-bundled/browser/1.2.3");
    fs::create_dir_all(plugin_root.join(".codex-plugin"))?;
    fs::create_dir_all(plugin_root.join("assets"))?;
    fs::write(
        plugin_root.join(".codex-plugin/plugin.json"),
        b"mac marker\n",
    )?;
    fs::write(plugin_root.join("assets/runtime.js"), b"mac runtime\n")?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    for source in [PLUGIN_MARKER_SOURCE, PLUGIN_RUNTIME_SOURCE] {
        let operation = operation_for(&plan.operations, source);
        assert_eq!(operation.action, ChangeKind::Preserve);
        assert!(operation.expected_previous_hash.is_some());
        assert!(!operation.rollback_required);
    }
    assert_eq!(plan.conflict_count, 0);
    Ok(())
}

#[test]
fn missing_plugin_version_is_added_normally() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    add_plugin_payloads(&mut fixture)?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    for source in [PLUGIN_MARKER_SOURCE, PLUGIN_RUNTIME_SOURCE] {
        let operation = operation_for(&plan.operations, source);
        assert_eq!(operation.action, ChangeKind::Add);
        assert!(operation.rollback_required);
    }
    assert_eq!(plan.conflict_count, 0);
    Ok(())
}

#[test]
fn plugin_root_without_its_marker_remains_a_conflict() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    add_plugin_payloads(&mut fixture)?;
    fs::create_dir_all(
        fixture
            .target
            .codex_home
            .join("plugins/cache/openai-bundled/browser/1.2.3"),
    )?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(
        operation_for(&plan.operations, PLUGIN_MARKER_SOURCE).action,
        ChangeKind::Conflict
    );
    assert_eq!(
        operation_for(&plan.operations, PLUGIN_RUNTIME_SOURCE).action,
        ChangeKind::Conflict
    );
    assert_eq!(plan.conflict_count, 2);
    Ok(())
}

#[test]
fn existing_skill_with_different_content_remains_a_conflict() -> Result<(), Box<dyn Error>> {
    let fixture = planner_fixture(None)?;
    let target = fixture.target.codex_home.join("skills/example/SKILL.md");
    fs::create_dir_all(target.parent().unwrap())?;
    fs::write(&target, b"# Keep local skill\n")?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(
        operation_for(&plan.operations, "codex/skills/example/SKILL.md").action,
        ChangeKind::Conflict
    );
    assert_eq!(plan.conflict_count, 1);
    Ok(())
}

#[test]
fn classifies_sessions_by_id_and_content_hash() -> Result<(), Box<dyn Error>> {
    struct Case {
        name: &'static str,
        target_conversation: Option<ConversationEntry>,
        expected: SessionAction,
    }

    let incoming = incoming_session_bytes();
    let incoming_hash = checksum(&incoming);
    for case in [
        Case {
            name: "existing_session_same_id_same_hash",
            target_conversation: Some(conversation(incoming_hash.clone())),
            expected: SessionAction::Skip,
        },
        Case {
            name: "existing_session_same_id_different_hash",
            target_conversation: Some(conversation(checksum(b"target session\n"))),
            expected: SessionAction::ImportAsBranch,
        },
        Case {
            name: "new_session_id",
            target_conversation: None,
            expected: SessionAction::Import,
        },
    ] {
        let mut fixture = planner_fixture(None)?;
        fixture.target.conversations = case.target_conversation.into_iter().collect();
        if case.expected == SessionAction::Skip {
            write_target_session(
                &fixture,
                &rewritten_session_bytes(
                    Uuid::parse_str(TASK_ID)?,
                    "Synthetic migration thread",
                    &fixture.projects_root.join("visual"),
                ),
            )?;
        } else if case.expected == SessionAction::ImportAsBranch {
            write_target_session(&fixture, b"changed target session\n")?;
        }

        let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

        assert_eq!(plan.sessions.len(), 1, "{}", case.name);
        assert_eq!(plan.sessions[0].action, case.expected, "{}", case.name);
    }

    Ok(())
}

#[test]
fn branch_import_is_deterministic_and_exposes_every_package_reference_rewrite(
) -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.target.conversations = vec![conversation(checksum(b"different session\n"))];
    write_target_session(&fixture, b"different session\n")?;
    let first = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let second = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(first, second);
    let session = &first.sessions[0];
    let package_id = Uuid::parse_str(PACKAGE_ID)?;
    let source_task_id = Uuid::parse_str(TASK_ID)?;
    let expected_id = Uuid::new_v5(&package_id, source_task_id.as_bytes());
    assert_eq!(session.source_task_id, source_task_id);
    assert_eq!(session.target_task_id, expected_id);
    assert_eq!(session.title, "Synthetic migration thread · ReHome");
    assert_eq!(session.action, SessionAction::ImportAsBranch);
    assert_eq!(
        session.target,
        fixture
            .target
            .codex_home
            .join("sessions")
            .join("2026")
            .join("07")
            .join("22")
            .join(format!("{expected_id}.jsonl"))
    );

    let id_sources = first
        .reference_rewrites
        .iter()
        .filter(|rewrite| rewrite.kind == ReferenceRewriteKind::ConversationId)
        .map(|rewrite| rewrite.package_source.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        id_sources,
        vec![THREADS_SOURCE, INDEX_SOURCE, SESSION_SOURCE]
    );
    let session_path_rewrites = first
        .reference_rewrites
        .iter()
        .filter(|rewrite| rewrite.kind == ReferenceRewriteKind::SessionPath)
        .collect::<Vec<_>>();
    assert_eq!(session_path_rewrites.len(), 6);
    assert!(session_path_rewrites.iter().all(|rewrite| {
        rewrite.source_task_id == source_task_id && Path::new(&rewrite.to) == session.target
    }));
    let rollout_variants = session_path_rewrites
        .iter()
        .map(|rewrite| rewrite.from.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(rollout_variants.len(), 3);
    assert!(rollout_variants.contains(SOURCE_ROLLOUT_PATH));
    assert!(rollout_variants.contains(r"C:\Users\OldUser\.codex\sessions\2026\07\22\thread.jsonl"));
    assert!(
        rollout_variants.contains(r"\\?\C:\Users\OldUser\.codex\sessions\2026\07\22\thread.jsonl")
    );
    assert!(first.reference_rewrites.iter().all(|rewrite| {
        !rewrite.package_source.is_empty() && !rewrite.from.is_empty() && !rewrite.to.is_empty()
    }));

    Ok(())
}

#[test]
fn windows_project_paths_generate_slash_backslash_and_verbatim_rewrites(
) -> Result<(), Box<dyn Error>> {
    let fixture = planner_fixture(None)?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let variants = plan
        .reference_rewrites
        .iter()
        .filter(|rewrite| rewrite.kind == ReferenceRewriteKind::ProjectPath)
        .map(|rewrite| rewrite.from.as_str())
        .collect::<std::collections::HashSet<_>>();

    assert!(variants.contains(r"C:\Users\OldUser\Documents\visual"));
    assert!(variants.contains("C:/Users/OldUser/Documents/visual"));
    assert!(variants.contains(r"\\?\C:\Users\OldUser\Documents\visual"));
    Ok(())
}

#[test]
fn project_bound_conversation_rewrites_a_stale_cross_platform_cwd() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let stale_cwd = "/Users/caleb/Documents/visual";
    let mut manifest = fixture.preview.manifest.clone();
    manifest.projects[0].source_path = r"\\?\C:\Users\OldUser\Documents\visual".into();
    let session = format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{TASK_ID}\",\"title\":\"Synthetic migration thread\",\"cwd\":\"{stale_cwd}\"}}}}\n"
    )
    .into_bytes();
    manifest.conversations[0].content_hash = checksum(&session);
    let threads = serde_json::to_vec(&serde_json::json!([{
        "id": TASK_ID,
        "title": "Synthetic migration thread",
        "cwd": stale_cwd,
        "rollout_path": SOURCE_ROLLOUT_PATH,
    }]))?;
    let index = serde_json::to_vec(&serde_json::json!({
        "id": TASK_ID,
        "title": "Synthetic migration thread",
        "cwd": stale_cwd,
        "rollout_path": SOURCE_ROLLOUT_PATH,
    }))?;
    let mut index_line = index;
    index_line.push(b'\n');
    write_package(
        &fixture.preview.package_path,
        &manifest,
        &[
            (THREADS_SOURCE, threads.as_slice()),
            (INDEX_SOURCE, index_line.as_slice()),
            (SESSION_SOURCE, session.as_slice()),
            ("codex/skills/example/SKILL.md", b"# Example\n"),
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
        ],
    )?;
    fixture.preview = inspect_package(&fixture.preview.package_path)?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let target = fixture.projects_root.join("visual");
    for source in [THREADS_SOURCE, INDEX_SOURCE, SESSION_SOURCE] {
        assert!(plan.reference_rewrites.iter().any(|rewrite| {
            rewrite.kind == ReferenceRewriteKind::ProjectPath
                && rewrite.package_source == source
                && rewrite.from == stale_cwd
                && Path::new(&rewrite.to) == target
        }));
    }
    apply_bridge_plan(&plan)?;
    let restored_session = fs::read_to_string(&plan.sessions[0].target)?;
    let restored_session: serde_json::Value = serde_json::from_str(restored_session.trim())?;
    assert_eq!(
        restored_session["payload"]["cwd"],
        target.to_string_lossy().as_ref()
    );
    let index = fs::read_to_string(fixture.target.codex_home.join("session_index.jsonl"))?;
    let index: serde_json::Value = serde_json::from_str(index.trim())?;
    assert_eq!(index["cwd"], target.to_string_lossy().as_ref());
    let connection = Connection::open(fixture.target.codex_home.join("state_5.sqlite"))?;
    let imported_cwd: String =
        connection.query_row("SELECT cwd FROM threads WHERE id = ?1", [TASK_ID], |row| {
            row.get(0)
        })?;
    assert_eq!(Path::new(&imported_cwd), target);
    Ok(())
}

#[test]
fn unc_project_paths_generate_unc_slash_and_verbatim_unc_rewrites() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let mut manifest = fixture.preview.manifest.clone();
    manifest.projects[0].source_path = r"\\server\share\visual".into();
    let session = incoming_session_bytes();
    write_package(
        &fixture.preview.package_path,
        &manifest,
        &[
            (THREADS_SOURCE, thread_metadata_bytes().as_slice()),
            (INDEX_SOURCE, index_bytes().as_slice()),
            (SESSION_SOURCE, session.as_slice()),
            ("codex/skills/example/SKILL.md", b"# Example\n"),
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
        ],
    )?;
    fixture.preview = inspect_package(&fixture.preview.package_path)?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let variants = plan
        .reference_rewrites
        .iter()
        .filter(|rewrite| rewrite.kind == ReferenceRewriteKind::ProjectPath)
        .map(|rewrite| rewrite.from.as_str())
        .collect::<std::collections::HashSet<_>>();

    assert!(variants.contains(r"\\server\share\visual"));
    assert!(variants.contains("//server/share/visual"));
    assert!(variants.contains(r"\\?\UNC\server\share\visual"));
    Ok(())
}

#[test]
fn branch_import_never_classifies_required_bridge_merges_as_unchanged() -> Result<(), Box<dyn Error>>
{
    let mut fixture = planner_fixture(None)?;
    fixture.target.conversations = vec![conversation(checksum(b"different session\n"))];
    write_target_session(&fixture, b"different session\n")?;
    fs::write(
        fixture.target.codex_home.join("session_index.jsonl"),
        index_bytes(),
    )?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::ImportAsBranch);
    assert_eq!(
        operation_for(&plan.operations, INDEX_SOURCE).action,
        ChangeKind::Update
    );
    assert_eq!(
        operation_for(&plan.operations, THREADS_SOURCE).action,
        ChangeKind::Update
    );
    Ok(())
}

#[test]
fn existing_deterministic_branch_target_is_never_overwritten() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.target.conversations = vec![conversation(checksum(b"different session\n"))];
    write_target_session(&fixture, b"different session\n")?;
    let package_id = Uuid::parse_str(PACKAGE_ID)?;
    let source_task_id = Uuid::parse_str(TASK_ID)?;
    let derived_id = Uuid::new_v5(&package_id, source_task_id.as_bytes());
    let branch_target = fixture
        .target
        .codex_home
        .join("sessions")
        .join("2026")
        .join("07")
        .join("22")
        .join(format!("{derived_id}.jsonl"));
    fs::create_dir_all(branch_target.parent().unwrap())?;
    fs::write(&branch_target, b"unrelated branch bytes\n")?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let operation = operation_for(&plan.operations, SESSION_SOURCE);

    assert_eq!(plan.sessions[0].action, SessionAction::ImportAsBranch);
    assert_eq!(operation.action, ChangeKind::Conflict);
    assert_eq!(
        operation.expected_previous_hash,
        Some(checksum(b"unrelated branch bytes\n"))
    );
    assert!(!operation.rollback_required);
    assert_eq!(fs::read(branch_target)?, b"unrelated branch bytes\n");
    Ok(())
}

#[test]
fn rejects_a_valid_package_replacement_with_the_same_preview_shape() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.preview = inspect_package(&fixture.preview.package_path)?;
    let session = incoming_session_bytes();
    let manifest = fixture.preview.manifest.clone();
    write_package(
        &fixture.preview.package_path,
        &manifest,
        &[
            (THREADS_SOURCE, thread_metadata_bytes().as_slice()),
            (INDEX_SOURCE, index_bytes().as_slice()),
            (SESSION_SOURCE, session.as_slice()),
            ("codex/skills/example/SKILL.md", b"# Replaced\n"),
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
        ],
    )?;

    let error =
        build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root).unwrap_err();

    assert_eq!(error.code, ErrorCode::PackageInvalid);
    assert!(error.message.contains("changed"));
    Ok(())
}

#[test]
fn stale_inventory_cannot_skip_a_missing_or_changed_session() -> Result<(), Box<dyn Error>> {
    let incoming = incoming_session_bytes();

    let mut missing = planner_fixture(None)?;
    missing.target.conversations = vec![conversation(checksum(&incoming))];
    let missing_plan =
        build_restore_plan(&missing.preview, &missing.target, &missing.projects_root)?;
    let missing_operation = operation_for(&missing_plan.operations, SESSION_SOURCE);
    assert_eq!(missing_plan.sessions[0].action, SessionAction::Import);
    assert_eq!(missing_operation.action, ChangeKind::Add);
    assert_eq!(missing_operation.expected_previous_hash, None);

    let mut changed = planner_fixture(None)?;
    changed.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(&changed, b"changed after discovery\n")?;
    let changed_plan =
        build_restore_plan(&changed.preview, &changed.target, &changed.projects_root)?;
    assert_eq!(
        changed_plan.sessions[0].action,
        SessionAction::ImportAsBranch
    );
    assert_eq!(
        operation_for(&changed_plan.operations, SESSION_SOURCE).expected_previous_hash,
        None
    );
    Ok(())
}

#[test]
fn repeated_restore_skips_its_existing_rewritten_branch() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.target.conversations = vec![conversation(checksum(b"original target\n"))];
    write_target_session(&fixture, b"original target\n")?;
    let first = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let branch = first.sessions[0].clone();
    assert_eq!(branch.action, SessionAction::ImportAsBranch);
    assert_ne!(
        branch.source_content_hash,
        branch.expected_final_content_hash
    );

    let rewritten = rewritten_session_bytes(
        branch.target_task_id,
        &branch.title,
        &fixture.projects_root.join("visual"),
    );
    assert_eq!(checksum(&rewritten), branch.expected_final_content_hash);
    fs::create_dir_all(branch.target.parent().unwrap())?;
    fs::write(&branch.target, &rewritten)?;
    let branch_relative = branch
        .target
        .strip_prefix(&fixture.target.codex_home)?
        .to_string_lossy()
        .replace('\\', "/");
    fixture.target.conversations.push(ConversationEntry {
        task_id: branch.target_task_id,
        project_id: Some(Uuid::parse_str(PROJECT_ID)?),
        title: branch.title.clone(),
        updated_at: "2026-07-22T00:00:00Z".into(),
        content_hash: checksum(&rewritten),
        archive_path: format!("codex/{branch_relative}"),
        classification: None,
    });

    let second = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    assert_eq!(second.sessions[0].action, SessionAction::Skip);
    assert_eq!(second.sessions[0].target_task_id, branch.target_task_id);
    assert_eq!(
        operation_for(&second.operations, SESSION_SOURCE).expected_previous_hash,
        Some(checksum(&rewritten))
    );
    Ok(())
}

#[test]
fn all_skipped_fully_registered_sessions_do_not_emit_bridge_metadata_operations(
) -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let incoming = incoming_session_bytes();
    fixture.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(
        &fixture,
        &rewritten_session_bytes(
            Uuid::parse_str(TASK_ID)?,
            "Synthetic migration thread",
            &fixture.projects_root.join("visual"),
        ),
    )?;
    write_ready_bridge_metadata(
        &fixture,
        Uuid::parse_str(TASK_ID)?,
        "Synthetic migration thread",
        &target_session_path(&fixture),
    )?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::Skip);
    assert!(!plan.operations.iter().any(|operation| {
        matches!(
            operation.package_source.as_str(),
            INDEX_SOURCE | THREADS_SOURCE
        )
    }));
    Ok(())
}

#[test]
fn repeated_branch_uses_its_derived_session_path_in_ready_bridge_metadata(
) -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.target.conversations = vec![conversation(checksum(b"original target\n"))];
    write_target_session(&fixture, b"original target\n")?;
    let first = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let branch = first.sessions[0].clone();
    let rewritten = rewritten_session_bytes(
        branch.target_task_id,
        &branch.title,
        &fixture.projects_root.join("visual"),
    );
    fs::create_dir_all(branch.target.parent().unwrap())?;
    fs::write(&branch.target, &rewritten)?;
    let branch_relative = branch
        .target
        .strip_prefix(&fixture.target.codex_home)?
        .to_string_lossy()
        .replace('\\', "/");
    fixture.target.conversations.push(ConversationEntry {
        task_id: branch.target_task_id,
        project_id: Some(Uuid::parse_str(PROJECT_ID)?),
        title: branch.title.clone(),
        updated_at: "2026-07-22T00:00:00Z".into(),
        content_hash: checksum(&rewritten),
        archive_path: format!("codex/{branch_relative}"),
        classification: None,
    });
    write_ready_bridge_metadata(
        &fixture,
        branch.target_task_id,
        &branch.title,
        &branch.target,
    )?;

    let second = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(second.sessions[0].action, SessionAction::Skip);
    assert_eq!(second.sessions[0].target, branch.target);
    assert!(!second.operations.iter().any(|operation| {
        matches!(
            operation.package_source.as_str(),
            INDEX_SOURCE | THREADS_SOURCE
        )
    }));
    Ok(())
}

#[test]
fn stale_source_rollout_path_is_not_ready() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let incoming = incoming_session_bytes();
    fixture.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(
        &fixture,
        &rewritten_session_bytes(
            Uuid::parse_str(TASK_ID)?,
            "Synthetic migration thread",
            &fixture.projects_root.join("visual"),
        ),
    )?;
    write_ready_bridge_metadata(
        &fixture,
        Uuid::parse_str(TASK_ID)?,
        "Synthetic migration thread",
        Path::new(SOURCE_ROLLOUT_PATH),
    )?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::Skip);
    assert_eq!(
        operation_for(&plan.operations, INDEX_SOURCE).action,
        ChangeKind::Update
    );
    assert_eq!(
        operation_for(&plan.operations, THREADS_SOURCE).action,
        ChangeKind::Update
    );
    Ok(())
}

#[test]
fn missing_exported_rollout_path_requires_exact_planned_sqlite_path() -> Result<(), Box<dyn Error>>
{
    let mut fixture = planner_fixture(None)?;
    let incoming = incoming_session_bytes();
    let thread_metadata = format!(
        "[{{\"id\":\"{TASK_ID}\",\"title\":\"Synthetic migration thread\",\"cwd\":\"C:/Users/OldUser/Documents/visual\"}}]"
    )
    .into_bytes();
    let manifest = fixture.preview.manifest.clone();
    write_package(
        &fixture.preview.package_path,
        &manifest,
        &[
            (THREADS_SOURCE, thread_metadata.as_slice()),
            (INDEX_SOURCE, index_bytes().as_slice()),
            (SESSION_SOURCE, incoming.as_slice()),
            ("codex/skills/example/SKILL.md", b"# Example\n"),
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
        ],
    )?;
    fixture.preview = inspect_package(&fixture.preview.package_path)?;
    fixture.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(
        &fixture,
        &rewritten_session_bytes(
            Uuid::parse_str(TASK_ID)?,
            "Synthetic migration thread",
            &fixture.projects_root.join("visual"),
        ),
    )?;
    let planned_target = target_session_path(&fixture);
    write_ready_bridge_metadata(
        &fixture,
        Uuid::parse_str(TASK_ID)?,
        "Synthetic migration thread",
        &planned_target,
    )?;
    let connection = Connection::open(fixture.target.codex_home.join("state_5.sqlite"))?;

    connection.execute(
        "UPDATE threads SET rollout_path = NULL WHERE id = ?1",
        [TASK_ID],
    )?;
    let null_plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    assert_eq!(
        operation_for(&null_plan.operations, THREADS_SOURCE).action,
        ChangeKind::Update
    );

    connection.execute(
        "UPDATE threads SET rollout_path = 'C:/stale.jsonl' WHERE id = ?1",
        [TASK_ID],
    )?;
    let stale_plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    assert_eq!(
        operation_for(&stale_plan.operations, THREADS_SOURCE).action,
        ChangeKind::Update
    );

    connection.execute(
        "UPDATE threads SET rollout_path = ?1 WHERE id = ?2",
        params![planned_target.to_string_lossy().as_ref(), TASK_ID],
    )?;
    let ready_plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    assert!(!ready_plan
        .operations
        .iter()
        .any(|operation| operation.package_source == THREADS_SOURCE));
    Ok(())
}

#[test]
fn duplicate_target_index_rows_plan_a_repair() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let incoming = incoming_session_bytes();
    fixture.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(
        &fixture,
        &rewritten_session_bytes(
            Uuid::parse_str(TASK_ID)?,
            "Synthetic migration thread",
            &fixture.projects_root.join("visual"),
        ),
    )?;
    write_ready_bridge_metadata(
        &fixture,
        Uuid::parse_str(TASK_ID)?,
        "Synthetic migration thread",
        &target_session_path(&fixture),
    )?;
    let index_path = fixture.target.codex_home.join("session_index.jsonl");
    let row = fs::read(&index_path)?;
    fs::write(&index_path, [row.as_slice(), row.as_slice()].concat())?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::Skip);
    assert_eq!(
        operation_for(&plan.operations, INDEX_SOURCE).action,
        ChangeKind::Update
    );
    assert!(!plan
        .operations
        .iter()
        .any(|operation| operation.package_source == THREADS_SOURCE));
    Ok(())
}

#[test]
fn bridge_metadata_rewrites_are_scoped_before_shared_titles_and_paths() -> Result<(), Box<dyn Error>>
{
    let fixture = shared_metadata_fixture()?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert!(
        plan.sessions
            .iter()
            .all(|session| session.action == SessionAction::Skip),
        "{:?}",
        plan.sessions
    );
    assert!(!plan.operations.iter().any(|operation| {
        matches!(
            operation.package_source.as_str(),
            INDEX_SOURCE | THREADS_SOURCE
        )
    }));
    Ok(())
}

#[test]
fn skipped_session_repairs_missing_index_and_sqlite_registration() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let incoming = incoming_session_bytes();
    fixture.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(
        &fixture,
        &rewritten_session_bytes(
            Uuid::parse_str(TASK_ID)?,
            "Synthetic migration thread",
            &fixture.projects_root.join("visual"),
        ),
    )?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::Skip);
    assert_eq!(
        operation_for(&plan.operations, INDEX_SOURCE).action,
        ChangeKind::Add
    );
    assert_eq!(
        operation_for(&plan.operations, THREADS_SOURCE).action,
        ChangeKind::Update
    );
    Ok(())
}

#[test]
fn skipped_session_repairs_stale_index_and_sqlite_metadata() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let incoming = incoming_session_bytes();
    fixture.target.conversations = vec![conversation(checksum(&incoming))];
    write_target_session(
        &fixture,
        &rewritten_session_bytes(
            Uuid::parse_str(TASK_ID)?,
            "Synthetic migration thread",
            &fixture.projects_root.join("visual"),
        ),
    )?;
    fs::write(
        fixture.target.codex_home.join("session_index.jsonl"),
        format!("{{\"id\":\"{TASK_ID}\",\"title\":\"Stale\",\"cwd\":\"C:/stale\"}}\n"),
    )?;
    let connection = Connection::open(fixture.target.codex_home.join("state_5.sqlite"))?;
    connection.execute(
        "INSERT INTO threads (id, title, cwd) VALUES (?1, 'Stale', 'C:/stale')",
        params![TASK_ID],
    )?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::Skip);
    assert_eq!(
        operation_for(&plan.operations, INDEX_SOURCE).action,
        ChangeKind::Update
    );
    assert_eq!(
        operation_for(&plan.operations, THREADS_SOURCE).action,
        ChangeKind::Update
    );
    Ok(())
}

#[test]
fn derived_ids_avoid_target_and_planned_conversation_ids() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let package_id = Uuid::parse_str(PACKAGE_ID)?;
    let source_id = Uuid::parse_str(TASK_ID)?;
    let first_derived = Uuid::new_v5(&package_id, source_id.as_bytes());
    fixture.target.conversations = vec![
        conversation(checksum(b"original target\n")),
        ConversationEntry {
            task_id: first_derived,
            project_id: None,
            title: "Occupied ID".into(),
            updated_at: "2026-07-22T00:00:00Z".into(),
            content_hash: checksum(b"occupied\n"),
            archive_path: format!("codex/sessions/2026/07/22/{first_derived}.jsonl"),
            classification: None,
        },
    ];
    write_target_session(&fixture, b"original target\n")?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(plan.sessions[0].action, SessionAction::ImportAsBranch);
    assert_ne!(plan.sessions[0].target_task_id, first_derived);
    assert!(!fixture
        .target
        .conversations
        .iter()
        .any(|session| session.task_id == plan.sessions[0].target_task_id));
    Ok(())
}

#[test]
fn plans_project_session_index_metadata_and_codex_payload_operations() -> Result<(), Box<dyn Error>>
{
    let fixture = planner_fixture(None)?;
    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let sources = plan
        .operations
        .iter()
        .map(|operation| operation.package_source.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        sources,
        vec![
            THREADS_SOURCE,
            INDEX_SOURCE,
            SESSION_SOURCE,
            "codex/skills/example/SKILL.md",
            PROJECT_SOURCE,
        ]
    );
    assert!(plan.operations.iter().all(|operation| {
        !operation.package_source.is_empty()
            && operation.target.is_absolute()
            && (operation.expected_previous_hash.is_some()
                || matches!(operation.action, ChangeKind::Add | ChangeKind::Unchanged))
    }));
    assert_eq!(
        operation_for(&plan.operations, INDEX_SOURCE).target,
        fixture.target.codex_home.join("session_index.jsonl")
    );
    assert_eq!(
        operation_for(&plan.operations, THREADS_SOURCE).target,
        fixture.target.codex_home.join("state_5.sqlite")
    );
    assert_eq!(
        operation_for(&plan.operations, "codex/skills/example/SKILL.md").target,
        fixture
            .target
            .codex_home
            .join("skills")
            .join("example")
            .join("SKILL.md")
    );

    Ok(())
}

#[test]
fn project_conflicts_are_preserved_without_modifying_target() -> Result<(), Box<dyn Error>> {
    let fixture = planner_fixture(None)?;
    fs::create_dir_all(fixture.project_target.parent().unwrap())?;
    fs::write(&fixture.project_target, b"keep local bytes\n")?;
    let before = fs::read(&fixture.project_target)?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;

    assert_eq!(
        operation_for(&plan.operations, PROJECT_SOURCE).action,
        ChangeKind::Conflict
    );
    assert_eq!(plan.conflict_count, 1);
    assert_eq!(fs::read(&fixture.project_target)?, before);
    assert!(!fixture.projects_root.join("created-by-planner").exists());
    Ok(())
}

#[test]
fn rejects_unsafe_manifest_paths_even_for_a_prevalidated_preview() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.preview.manifest.projects[0].archive_path = "projects/../escape".into();

    let error =
        build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root).unwrap_err();

    assert_eq!(error.code, ErrorCode::PackageInvalid);
    assert!(!fixture.projects_root.join("escape").exists());
    Ok(())
}

#[test]
fn rejects_project_names_that_could_escape_the_projects_root() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.preview.manifest.projects[0].name = "../escape".into();

    let error =
        build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root).unwrap_err();

    assert_eq!(error.code, ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn maps_project_targets_with_the_target_operating_system_syntax() -> Result<(), Box<dyn Error>> {
    struct Case {
        target_os: SourceOs,
        codex_home: &'static str,
        projects_root: &'static str,
        expected_target: &'static str,
    }

    for case in [
        Case {
            target_os: SourceOs::Windows,
            codex_home: r"C:\Users\test\.codex",
            projects_root: r"D:\ReHome",
            expected_target: r"D:\ReHome\visual\README.md",
        },
        Case {
            target_os: SourceOs::Macos,
            codex_home: "/Users/test/.codex",
            projects_root: "/Users/test/Codex-Restored-Projects",
            expected_target: "/Users/test/Codex-Restored-Projects/visual/README.md",
        },
    ] {
        let temp = tempfile::tempdir()?;
        let preview = project_only_preview(temp.path())?;
        let target = TargetInventory {
            codex_home: PathBuf::from(case.codex_home),
            target_os: case.target_os,
            target_arch: "x86_64".into(),
            counts: ContentCounts::default(),
            projects: vec![],
            conversations: vec![],
        };

        let plan = build_restore_plan(&preview, &target, Path::new(case.projects_root))?;

        assert_eq!(
            operation_for(&plan.operations, PROJECT_SOURCE).target,
            PathBuf::from(case.expected_target)
        );
    }
    Ok(())
}

#[test]
fn package_projects_cannot_silently_share_a_target_directory() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let preview = project_preview(&temp_root, true)?;
    let target_root = temp_root.join("target");
    let target = TargetInventory {
        codex_home: target_root.join(".codex"),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };

    let error = build_restore_plan(&preview, &target, &target_root.join("projects")).unwrap_err();

    assert_eq!(error.code, ErrorCode::ProjectConflict);
    Ok(())
}

#[test]
fn rejects_existing_link_ancestors_for_both_restore_roots() -> Result<(), Box<dyn Error>> {
    for root in ["codex_home", "projects_root"] {
        let fixture = planner_fixture(None)?;
        let link = if root == "codex_home" {
            fixture.target.codex_home.clone()
        } else {
            fixture.projects_root.clone()
        };
        let real = fixture
            ._temp
            .path()
            .join(format!("real-{}", root.replace('_', "-")));
        if link.exists() {
            fs::rename(&link, &real)?;
        } else {
            fs::create_dir_all(&real)?;
        }
        create_directory_link(&real, &link)?;

        let error = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::RestoreFailed, "{root}");
        assert!(
            error.message.contains("link") || error.message.contains("reparse"),
            "{root}: {}",
            error.message
        );
    }
    Ok(())
}

#[test]
fn rejects_overlapping_codex_and_project_roots() -> Result<(), Box<dyn Error>> {
    let fixture = planner_fixture(None)?;

    let error = build_restore_plan(
        &fixture.preview,
        &fixture.target,
        &fixture.target.codex_home,
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("overlap"));
    Ok(())
}

#[test]
fn project_target_names_use_unicode_normalized_collision_rules() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let mut preview = project_preview(temp.path(), true)?;
    preview.manifest.projects[0].name = "Caf\u{00e9}".into();
    preview.manifest.projects[1].name = "Cafe\u{0301}".into();
    let manifest = preview.manifest.clone();
    write_package(
        &preview.package_path,
        &manifest,
        &[
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/files/README.md",
                b"second project\n",
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/project.json",
                b"{}",
            ),
        ],
    )?;
    preview = inspect_package(&preview.package_path)?;
    let target_root = temp.path().join("target");
    let codex_home = target_root.join(".codex");
    fs::create_dir_all(&codex_home)?;
    let target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };

    let error = build_restore_plan(&preview, &target, &target_root.join("projects")).unwrap_err();

    assert_eq!(error.code, ErrorCode::ProjectConflict);
    Ok(())
}

#[test]
fn macos_project_target_names_use_case_insensitive_unicode_collision_rules(
) -> Result<(), Box<dyn Error>> {
    for (first, second) in [("Visual", "visual"), ("Caf\u{00e9}", "Cafe\u{0301}")] {
        let temp = tempfile::tempdir()?;
        let mut preview = project_preview(temp.path(), true)?;
        preview.manifest.projects[0].name = first.into();
        preview.manifest.projects[1].name = second.into();
        let manifest = preview.manifest.clone();
        write_project_preview_payloads(&preview.package_path, &manifest)?;
        preview = inspect_package(&preview.package_path)?;
        let target = TargetInventory {
            codex_home: PathBuf::from("/Users/test/.codex"),
            target_os: SourceOs::Macos,
            target_arch: "aarch64".into(),
            counts: ContentCounts::default(),
            projects: vec![],
            conversations: vec![],
        };

        let error = build_restore_plan(
            &preview,
            &target,
            Path::new("/Users/test/Codex-Restored-Projects"),
        )
        .unwrap_err();

        assert_eq!(
            error.code,
            ErrorCode::ProjectConflict,
            "{first:?} / {second:?}"
        );
    }
    Ok(())
}

#[test]
fn derived_ids_are_reserved_against_future_planned_imports() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    let package_id = Uuid::parse_str(PACKAGE_ID)?;
    let future_task_id = Uuid::new_v5(&package_id, Uuid::parse_str(TASK_ID)?.as_bytes());
    let second_source = "codex/sessions/2026/07/22/second.jsonl";
    let second_bytes = format!("{{\"id\":\"{future_task_id}\"}}\n").into_bytes();
    fixture
        .preview
        .manifest
        .conversations
        .push(ConversationEntry {
            task_id: future_task_id,
            project_id: None,
            title: "Future import".into(),
            updated_at: "2026-07-22T00:00:00Z".into(),
            content_hash: checksum(&second_bytes),
            archive_path: second_source.into(),
            classification: None,
        });
    fixture.preview.manifest.counts.conversations = 2;
    let manifest = fixture.preview.manifest.clone();
    let incoming = incoming_session_bytes();
    write_package(
        &fixture.preview.package_path,
        &manifest,
        &[
            (THREADS_SOURCE, thread_metadata_bytes().as_slice()),
            (INDEX_SOURCE, index_bytes().as_slice()),
            (SESSION_SOURCE, incoming.as_slice()),
            (second_source, second_bytes.as_slice()),
            ("codex/skills/example/SKILL.md", b"# Example\n"),
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
        ],
    )?;
    fixture.preview = inspect_package(&fixture.preview.package_path)?;
    fixture.target.conversations = vec![conversation(checksum(b"original target\n"))];
    write_target_session(&fixture, b"original target\n")?;

    let plan = build_restore_plan(&fixture.preview, &fixture.target, &fixture.projects_root)?;
    let ids = plan
        .sessions
        .iter()
        .map(|session| session.target_task_id)
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(ids.len(), 2);
    assert_eq!(
        plan.sessions
            .iter()
            .find(|session| session.source_task_id == Uuid::parse_str(TASK_ID).unwrap())
            .unwrap()
            .action,
        SessionAction::ImportAsBranch
    );
    Ok(())
}

fn add_plugin_payloads(fixture: &mut PlannerFixture) -> Result<(), Box<dyn Error>> {
    let mut manifest = fixture.preview.manifest.clone();
    manifest.counts.plugins = 1;
    let session = incoming_session_bytes();
    let threads = thread_metadata_bytes();
    let index = index_bytes();
    write_package(
        &fixture.preview.package_path,
        &manifest,
        &[
            (THREADS_SOURCE, threads.as_slice()),
            (INDEX_SOURCE, index.as_slice()),
            (SESSION_SOURCE, session.as_slice()),
            ("codex/skills/example/SKILL.md", b"# Example\n"),
            (PLUGIN_MARKER_SOURCE, b"windows marker\n"),
            (PLUGIN_RUNTIME_SOURCE, b"windows runtime\n"),
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
        ],
    )?;
    fixture.preview = inspect_package(&fixture.preview.package_path)?;
    Ok(())
}

fn planner_fixture(target_os: Option<SourceOs>) -> Result<PlannerFixture, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let package_path = temp_root.join("handoff.rehome");
    let target_root = temp_root.join("target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    fs::create_dir_all(&codex_home)?;
    create_target_database(&codex_home.join("state_5.sqlite"))?;

    let project_id = Uuid::parse_str(PROJECT_ID)?;
    let session_bytes = incoming_session_bytes();
    let manifest = PackageManifest {
        format: "codex-rehome".into(),
        schema_version: 1,
        package_id: Uuid::parse_str(PACKAGE_ID)?,
        created_at: "2026-07-22T00:00:00Z".into(),
        source_os: SourceOs::Windows,
        source_arch: "x86_64".into(),
        source_device_id: Uuid::nil(),
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: ContentCounts {
            projects: 1,
            project_files: 1,
            conversations: 1,
            skills: 1,
            sqlite_threads: 1,
            ..ContentCounts::default()
        },
        projects: vec![ProjectEntry {
            project_id,
            name: "visual".into(),
            source_path: "C:/Users/OldUser/Documents/visual".into(),
            source_available: true,
            archive_path: format!("projects/{project_id}/files"),
            file_count: 1,
            content_bytes: b"incoming project\n".len() as u64,
            git_remote: None,
            git_branch: None,
            git_head: None,
        }],
        conversations: vec![conversation(checksum(&session_bytes))],
        exclusions: ExclusionSummary::default(),
    };
    let thread_metadata = thread_metadata_bytes();
    let index = index_bytes();
    let payloads = [
        (THREADS_SOURCE, thread_metadata.as_slice()),
        (INDEX_SOURCE, index.as_slice()),
        (SESSION_SOURCE, session_bytes.as_slice()),
        ("codex/skills/example/SKILL.md", b"# Example\n".as_slice()),
        (PROJECT_SOURCE, b"incoming project\n".as_slice()),
        (
            "projects/22222222-2222-4222-8222-222222222222/project.json",
            b"{}".as_slice(),
        ),
    ];
    write_package(&package_path, &manifest, &payloads)?;
    let preview = inspect_package(&package_path)?;

    let target_os = target_os.unwrap_or_else(current_source_os);
    let target = TargetInventory {
        codex_home,
        target_os,
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let project_target = projects_root.join("visual").join("README.md");
    Ok(PlannerFixture {
        _temp: temp,
        preview,
        target,
        projects_root,
        project_target,
    })
}

fn shared_metadata_fixture() -> Result<PlannerFixture, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let package_path = temp_root.join("shared-metadata.rehome");
    let codex_home = temp_root.join("target").join(".codex");
    let projects_root = temp_root.join("target").join("projects");
    fs::create_dir_all(&codex_home)?;
    create_target_database(&codex_home.join("state_5.sqlite"))?;

    let package_id = Uuid::parse_str(PACKAGE_ID)?;
    let first_id = Uuid::parse_str(TASK_ID)?;
    let second_id = Uuid::parse_str(SECOND_TASK_ID)?;
    let first_project_id = Uuid::parse_str(PROJECT_ID)?;
    let second_project_id = Uuid::parse_str(SECOND_PROJECT_ID)?;
    let derived_id = Uuid::new_v5(&package_id, first_id.as_bytes());
    let source_project = "C:/Users/OldUser/Documents/shared";
    let first_source = "codex/sessions/2026/07/22/first.jsonl";
    let second_source = "codex/sessions/2026/07/22/second.jsonl";
    let first_rollout = "C:/Users/OldUser/.codex/sessions/2026/07/22/first.jsonl";
    let second_rollout = "C:/Users/OldUser/.codex/sessions/2026/07/22/second.jsonl";
    let source_session = |id: Uuid| {
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "id": id.to_string(),
            "title": "Shared title",
            "cwd": source_project,
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes
    };
    let first_bytes = source_session(first_id);
    let second_bytes = source_session(second_id);
    let conversation = |task_id, project_id, content_hash, archive_path: &str| ConversationEntry {
        task_id,
        project_id: Some(project_id),
        title: "Shared title".into(),
        updated_at: "2026-07-22T00:00:00Z".into(),
        content_hash,
        archive_path: archive_path.into(),
        classification: None,
    };
    let projects = vec![
        ProjectEntry {
            project_id: first_project_id,
            name: "first-project".into(),
            source_path: source_project.into(),
            source_available: true,
            archive_path: format!("projects/{first_project_id}/files"),
            file_count: 1,
            content_bytes: 6,
            git_remote: None,
            git_branch: None,
            git_head: None,
        },
        ProjectEntry {
            project_id: second_project_id,
            name: "second-project".into(),
            source_path: source_project.into(),
            source_available: true,
            archive_path: format!("projects/{second_project_id}/files"),
            file_count: 1,
            content_bytes: 7,
            git_remote: None,
            git_branch: None,
            git_head: None,
        },
    ];
    let manifest = PackageManifest {
        format: "codex-rehome".into(),
        schema_version: 1,
        package_id,
        created_at: "2026-07-22T00:00:00Z".into(),
        source_os: SourceOs::Windows,
        source_arch: "x86_64".into(),
        source_device_id: Uuid::nil(),
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: ContentCounts {
            projects: 2,
            project_files: 2,
            conversations: 2,
            sqlite_threads: 2,
            ..ContentCounts::default()
        },
        projects,
        conversations: vec![
            conversation(
                first_id,
                first_project_id,
                checksum(&first_bytes),
                first_source,
            ),
            conversation(
                second_id,
                second_project_id,
                checksum(&second_bytes),
                second_source,
            ),
        ],
        exclusions: ExclusionSummary::default(),
    };
    let source_rows = [
        serde_json::json!({
            "id": first_id.to_string(),
            "title": "Shared title",
            "cwd": source_project,
            "rollout_path": first_rollout,
        }),
        serde_json::json!({
            "id": second_id.to_string(),
            "title": "Shared title",
            "cwd": source_project,
            "rollout_path": second_rollout,
        }),
    ];
    let mut index = Vec::new();
    for row in &source_rows {
        serde_json::to_writer(&mut index, row)?;
        index.push(b'\n');
    }
    let threads = serde_json::to_vec(&source_rows)?;
    write_package(
        &package_path,
        &manifest,
        &[
            (THREADS_SOURCE, threads.as_slice()),
            (INDEX_SOURCE, index.as_slice()),
            (first_source, first_bytes.as_slice()),
            (second_source, second_bytes.as_slice()),
            (
                "projects/22222222-2222-4222-8222-222222222222/files/a.txt",
                b"first\n",
            ),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/files/b.txt",
                b"second\n",
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/project.json",
                b"{}",
            ),
        ],
    )?;
    let first_target = codex_home
        .join("sessions")
        .join("2026")
        .join("07")
        .join("22")
        .join("first.jsonl");
    let branch_target = first_target
        .parent()
        .unwrap()
        .join(format!("{derived_id}.jsonl"));
    let second_target = first_target.parent().unwrap().join("second.jsonl");
    fs::create_dir_all(first_target.parent().unwrap())?;
    fs::write(&first_target, b"conflicting original\n")?;
    let first_project_target = projects_root.join("first-project");
    let second_project_target = projects_root.join("second-project");
    let branch_bytes =
        rewritten_session_bytes(derived_id, "Shared title · ReHome", &first_project_target);
    let second_target_bytes =
        rewritten_session_bytes(second_id, "Shared title", &second_project_target);
    fs::write(&branch_target, &branch_bytes)?;
    fs::write(&second_target, &second_target_bytes)?;

    let ready_rows = [
        (
            derived_id,
            "Shared title · ReHome",
            first_project_target.as_path(),
            branch_target.as_path(),
        ),
        (
            second_id,
            "Shared title",
            second_project_target.as_path(),
            second_target.as_path(),
        ),
    ];
    let mut ready_index = Vec::new();
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    for (id, title, cwd, rollout_path) in ready_rows {
        serde_json::to_writer(
            &mut ready_index,
            &serde_json::json!({
                "id": id.to_string(),
                "title": title,
                "cwd": cwd.to_string_lossy(),
                "rollout_path": rollout_path.to_string_lossy(),
            }),
        )?;
        ready_index.push(b'\n');
        connection.execute(
            "INSERT INTO threads (id, title, cwd, rollout_path) VALUES (?1, ?2, ?3, ?4)",
            params![
                id.to_string(),
                title,
                cwd.to_string_lossy().as_ref(),
                rollout_path.to_string_lossy().as_ref()
            ],
        )?;
    }
    fs::write(codex_home.join("session_index.jsonl"), ready_index)?;
    let target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![
            conversation(
                first_id,
                first_project_id,
                checksum(b"conflicting original\n"),
                first_source,
            ),
            conversation(
                derived_id,
                first_project_id,
                checksum(&branch_bytes),
                &format!("codex/sessions/2026/07/22/{derived_id}.jsonl"),
            ),
            conversation(
                second_id,
                second_project_id,
                checksum(&second_target_bytes),
                second_source,
            ),
        ],
    };
    Ok(PlannerFixture {
        _temp: temp,
        preview: inspect_package(&package_path)?,
        target,
        projects_root,
        project_target: PathBuf::new(),
    })
}

fn write_project_preview_payloads(
    path: &Path,
    manifest: &PackageManifest,
) -> Result<(), Box<dyn Error>> {
    write_package(
        path,
        manifest,
        &[
            (PROJECT_SOURCE, b"incoming project\n"),
            (
                "projects/22222222-2222-4222-8222-222222222222/project.json",
                b"{}",
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/files/README.md",
                b"second project\n",
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/project.json",
                b"{}",
            ),
        ],
    )
}

fn create_target_database(path: &Path) -> Result<(), Box<dyn Error>> {
    let connection = Connection::open(path)?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT,
            rollout_path TEXT,
            title TEXT,
            updated_at INTEGER,
            archived INTEGER,
            has_user_event INTEGER,
            preview TEXT
        );",
    )?;
    Ok(())
}

fn write_ready_bridge_metadata(
    fixture: &PlannerFixture,
    task_id: Uuid,
    title: &str,
    rollout_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let project_path = fixture.projects_root.join("visual");
    let ready_index = serde_json::to_vec(&serde_json::json!({
        "id": task_id.to_string(),
        "title": title,
        "cwd": project_path.to_string_lossy(),
        "rollout_path": rollout_path.to_string_lossy(),
        "target_only": "preserve me",
    }))?;
    let mut ready_index_line = ready_index;
    ready_index_line.push(b'\n');
    fs::write(
        fixture.target.codex_home.join("session_index.jsonl"),
        ready_index_line,
    )?;
    let connection = Connection::open(fixture.target.codex_home.join("state_5.sqlite"))?;
    connection.execute(
        "INSERT INTO threads (id, title, cwd, rollout_path) VALUES (?1, ?2, ?3, ?4)",
        params![
            task_id.to_string(),
            title,
            project_path.to_string_lossy().as_ref(),
            rollout_path.to_string_lossy().as_ref()
        ],
    )?;
    Ok(())
}

fn project_only_preview(root: &Path) -> Result<PackagePreview, Box<dyn Error>> {
    project_preview(root, false)
}

fn project_preview(root: &Path, duplicate_target: bool) -> Result<PackagePreview, Box<dyn Error>> {
    let package_path = root.join("project-only.rehome");
    let project_id = Uuid::parse_str(PROJECT_ID)?;
    let second_project_id = Uuid::parse_str("33333333-3333-4333-8333-333333333333")?;
    let mut projects = vec![ProjectEntry {
        project_id,
        name: "visual".into(),
        source_path: r"C:\Users\OldUser\Documents\visual".into(),
        source_available: true,
        archive_path: format!("projects/{project_id}/files"),
        file_count: 1,
        content_bytes: b"incoming project\n".len() as u64,
        git_remote: None,
        git_branch: None,
        git_head: None,
    }];
    if duplicate_target {
        projects.push(ProjectEntry {
            project_id: second_project_id,
            name: "visual".into(),
            source_path: r"C:\Users\OldUser\Documents\visual-copy".into(),
            source_available: true,
            archive_path: format!("projects/{second_project_id}/files"),
            file_count: 1,
            content_bytes: b"second project\n".len() as u64,
            git_remote: None,
            git_branch: None,
            git_head: None,
        });
    }
    let manifest = PackageManifest {
        format: "codex-rehome".into(),
        schema_version: 1,
        package_id: Uuid::parse_str(PACKAGE_ID)?,
        created_at: "2026-07-22T00:00:00Z".into(),
        source_os: SourceOs::Windows,
        source_arch: "x86_64".into(),
        source_device_id: Uuid::nil(),
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: ContentCounts {
            projects: projects.len() as u64,
            project_files: projects.len() as u64,
            ..ContentCounts::default()
        },
        projects,
        conversations: vec![],
        exclusions: ExclusionSummary::default(),
    };
    let mut payloads = vec![
        (PROJECT_SOURCE, b"incoming project\n".as_slice()),
        (
            "projects/22222222-2222-4222-8222-222222222222/project.json",
            b"{}".as_slice(),
        ),
    ];
    if duplicate_target {
        payloads.extend([
            (
                "projects/33333333-3333-4333-8333-333333333333/files/README.md",
                b"second project\n".as_slice(),
            ),
            (
                "projects/33333333-3333-4333-8333-333333333333/project.json",
                b"{}".as_slice(),
            ),
        ]);
    }
    write_package(&package_path, &manifest, &payloads)?;
    Ok(inspect_package(&package_path)?)
}

fn conversation(content_hash: String) -> ConversationEntry {
    ConversationEntry {
        task_id: Uuid::parse_str(TASK_ID).unwrap(),
        project_id: Some(Uuid::parse_str(PROJECT_ID).unwrap()),
        title: "Synthetic migration thread".into(),
        updated_at: "2026-07-22T00:00:00Z".into(),
        content_hash,
        archive_path: SESSION_SOURCE.into(),
        classification: None,
    }
}

fn operation_for<'a>(
    operations: &'a [rehome_desktop_lib::core::models::PlannedOperation],
    source: &str,
) -> &'a rehome_desktop_lib::core::models::PlannedOperation {
    operations
        .iter()
        .find(|operation| operation.package_source == source)
        .unwrap_or_else(|| panic!("missing operation for {source}"))
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn incoming_session_bytes() -> Vec<u8> {
    format!(
        "{{\"id\":\"{TASK_ID}\",\"title\":\"Synthetic migration thread\",\"cwd\":\"C:/Users/OldUser/Documents/visual\"}}\n"
    )
    .into_bytes()
}

fn thread_metadata_bytes() -> Vec<u8> {
    format!(
        "[{{\"id\":\"{TASK_ID}\",\"title\":\"Synthetic migration thread\",\"cwd\":\"C:/Users/OldUser/Documents/visual\",\"rollout_path\":\"{SOURCE_ROLLOUT_PATH}\"}}]"
    )
    .into_bytes()
}

fn index_bytes() -> Vec<u8> {
    format!(
        "{{\"id\":\"{TASK_ID}\",\"title\":\"Synthetic migration thread\",\"cwd\":\"C:/Users/OldUser/Documents/visual\",\"rollout_path\":\"{SOURCE_ROLLOUT_PATH}\"}}\n"
    )
    .into_bytes()
}

fn rewritten_session_bytes(task_id: Uuid, title: &str, project_path: &Path) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&serde_json::json!({
        "id": task_id.to_string(),
        "title": title,
        "cwd": project_path.to_string_lossy(),
    }))
    .unwrap();
    bytes.push(b'\n');
    bytes
}

fn target_session_path(fixture: &PlannerFixture) -> PathBuf {
    fixture
        .target
        .codex_home
        .join("sessions")
        .join("2026")
        .join("07")
        .join("22")
        .join("thread.jsonl")
}

fn write_target_session(fixture: &PlannerFixture, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = target_session_path(fixture);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, bytes)?;
    Ok(())
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn write_package(
    path: &Path,
    manifest: &PackageManifest,
    payloads: &[(&str, &[u8])],
) -> Result<(), Box<dyn Error>> {
    let file = fs::File::create(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for (name, bytes) in payloads {
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    let checksums = payloads
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", checksum(bytes)))
        .collect::<String>();
    writer.start_file("checksums.sha256", options)?;
    writer.write_all(checksums.as_bytes())?;
    writer.start_file("manifest.json", options)?;
    writer.write_all(&serde_json::to_vec(manifest)?)?;
    writer.finish()?;
    Ok(())
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}
