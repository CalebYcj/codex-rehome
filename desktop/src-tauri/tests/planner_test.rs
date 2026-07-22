use rehome_desktop_lib::core::{
    error::ErrorCode,
    models::{
        ChangeKind, ContentCounts, ConversationEntry, ExclusionSummary, PackageManifest,
        PackageMode, PackagePreview, ProjectEntry, ReferenceRewriteKind, SessionAction, SourceOs,
        TargetInventory,
    },
    planner::build_restore_plan,
};
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
const PROJECT_SOURCE: &str = "projects/22222222-2222-4222-8222-222222222222/files/README.md";
const SESSION_SOURCE: &str = "codex/sessions/2026/07/22/thread.jsonl";
const INDEX_SOURCE: &str = "codex/session_index.jsonl";
const THREADS_SOURCE: &str = "codex/metadata/threads.json";

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
fn classifies_sessions_by_id_and_content_hash() -> Result<(), Box<dyn Error>> {
    struct Case {
        name: &'static str,
        target_conversation: Option<ConversationEntry>,
        expected: SessionAction,
    }

    let incoming_hash = checksum(b"incoming session\n");
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
    assert!(first.reference_rewrites.iter().all(|rewrite| {
        !rewrite.package_source.is_empty() && !rewrite.from.is_empty() && !rewrite.to.is_empty()
    }));

    Ok(())
}

#[test]
fn existing_deterministic_branch_target_is_never_overwritten() -> Result<(), Box<dyn Error>> {
    let mut fixture = planner_fixture(None)?;
    fixture.target.conversations = vec![conversation(checksum(b"different session\n"))];
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
    let preview = project_preview(temp.path(), true)?;
    let target_root = temp.path().join("target");
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

fn planner_fixture(target_os: Option<SourceOs>) -> Result<PlannerFixture, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let package_path = temp.path().join("handoff.rehome");
    let target_root = temp.path().join("target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    fs::create_dir_all(&codex_home)?;
    fs::write(codex_home.join("state_5.sqlite"), b"target database")?;

    let project_id = Uuid::parse_str(PROJECT_ID)?;
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
            source_path: r"C:\Users\OldUser\Documents\visual".into(),
            archive_path: format!("projects/{project_id}/files"),
            file_count: 1,
            content_bytes: b"incoming project\n".len() as u64,
            git_remote: None,
            git_branch: None,
            git_head: None,
        }],
        conversations: vec![conversation(checksum(b"incoming session\n"))],
        exclusions: ExclusionSummary::default(),
    };
    let payloads = [
        (
            THREADS_SOURCE,
            br#"[{"id":"11111111-1111-4111-8111-111111111111"}]"#.as_slice(),
        ),
        (
            INDEX_SOURCE,
            br#"{"id":"11111111-1111-4111-8111-111111111111"}\n"#.as_slice(),
        ),
        (SESSION_SOURCE, b"incoming session\n".as_slice()),
        ("codex/skills/example/SKILL.md", b"# Example\n".as_slice()),
        (PROJECT_SOURCE, b"incoming project\n".as_slice()),
        (
            "projects/22222222-2222-4222-8222-222222222222/project.json",
            b"{}".as_slice(),
        ),
    ];
    write_package(&package_path, &manifest, &payloads)?;
    let mut entries = payloads
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect::<Vec<_>>();
    entries.extend(["checksums.sha256".into(), "manifest.json".into()]);
    entries.sort();

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
        preview: PackagePreview {
            package_path,
            manifest,
            checksum_valid: true,
            entries,
            forbidden_files_total: 0,
        },
        target,
        projects_root,
        project_target,
    })
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
    let mut entries = payloads
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect::<Vec<_>>();
    entries.extend(["checksums.sha256".into(), "manifest.json".into()]);
    entries.sort();
    Ok(PackagePreview {
        package_path,
        manifest,
        checksum_valid: true,
        entries,
        forbidden_files_total: 0,
    })
}

fn conversation(content_hash: String) -> ConversationEntry {
    ConversationEntry {
        task_id: Uuid::parse_str(TASK_ID).unwrap(),
        project_id: Some(Uuid::parse_str(PROJECT_ID).unwrap()),
        title: "Synthetic migration thread".into(),
        updated_at: "2026-07-22T00:00:00Z".into(),
        content_hash,
        archive_path: SESSION_SOURCE.into(),
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
