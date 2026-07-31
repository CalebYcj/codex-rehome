use rehome_desktop_lib::core::{
    discovery::discover_codex,
    models::{ContentCounts, CreatePackageRequest, RestoreOptions, SourceOs, TargetInventory},
    package::{create_package, inspect_package},
    planner::build_restore_plan,
    restore::apply_restore,
};
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::tempdir;

#[test]
#[ignore = "reads the local Codex profile and packages all optional content into a temporary file"]
fn local_all_optional_content_package() -> Result<(), Box<dyn Error>> {
    let inventory = discover_codex(None)?;
    let skill_paths = inventory
        .skills
        .iter()
        .map(|entry| entry.source_path.clone())
        .collect();
    let plugin_paths = inventory
        .plugins
        .iter()
        .map(|entry| entry.source_path.clone())
        .collect();
    let generated_image_paths = inventory
        .generated_images
        .iter()
        .map(|entry| entry.source_path.clone())
        .collect();
    let sandbox = tempdir()?;
    let started = Instant::now();
    let report = create_package(CreatePackageRequest {
        codex_home: inventory.codex_home,
        project_paths: vec![],
        conversation_ids: vec![],
        output_path: sandbox.path().join("all-optional-content.rehome"),
        source_device_id: inventory.source_device_id,
        skill_paths,
        plugin_paths,
        generated_image_paths,
    })?;
    let package = inspect_package(&report.package_path)?;
    assert!(package.checksum_valid);
    assert_eq!(package.forbidden_files_total, 0);
    println!(
        "Optional-content package passed in {:.2?}: {} skill(s), {} plugin(s), {} image(s), {} bytes.",
        started.elapsed(),
        report.counts.skills,
        report.counts.plugins,
        report.counts.generated_images,
        report.bytes_written,
    );
    Ok(())
}
use uuid::Uuid;

#[test]
#[ignore = "set REHOME_LOCAL_SESSION_ID and REHOME_LOCAL_PROJECT_PATH for a local profile"]
fn local_windows_to_windows_acceptance() -> Result<(), Box<dyn Error>> {
    let source_codex_home = env::var_os("REHOME_LOCAL_CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|path| PathBuf::from(path).join(".codex")))
        .ok_or("REHOME_LOCAL_CODEX_HOME or USERPROFILE is required")?;
    let session_id = Uuid::parse_str(
        &env::var("REHOME_LOCAL_SESSION_ID").map_err(|_| "REHOME_LOCAL_SESSION_ID is required")?,
    )?;
    let project_path = PathBuf::from(
        env::var_os("REHOME_LOCAL_PROJECT_PATH").ok_or("REHOME_LOCAL_PROJECT_PATH is required")?,
    );
    if !project_path.is_dir() {
        return Err("REHOME_LOCAL_PROJECT_PATH is not a readable project directory".into());
    }

    let sandbox = tempdir()?;
    let package_path = sandbox.path().join("windows-to-windows.rehome");
    let package_report = create_package(CreatePackageRequest {
        codex_home: source_codex_home.clone(),
        project_paths: vec![project_path],
        conversation_ids: vec![session_id],
        output_path: package_path.clone(),
        source_device_id: Uuid::new_v4(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_report.package_path)?;
    assert!(preview.checksum_valid);
    assert_eq!(preview.forbidden_files_total, 0);
    assert_eq!(preview.manifest.source_os, SourceOs::Windows);
    assert_eq!(preview.manifest.projects.len(), 1);
    assert_eq!(preview.manifest.conversations.len(), 1);

    let target_root = sandbox.path().join("virtual-windows-machine");
    let target_codex_home = target_root.join(".codex");
    let projects_root = target_root.join("Codex-Restored-Projects");
    fs::create_dir_all(&target_codex_home)?;
    fs::write(target_codex_home.join("session_index.jsonl"), b"")?;
    snapshot_database(
        &source_codex_home.join("state_5.sqlite"),
        &target_codex_home.join("state_5.sqlite"),
    )?;

    let target = TargetInventory {
        codex_home: target_codex_home.clone(),
        target_os: SourceOs::Windows,
        target_arch: env::consts::ARCH.into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(&preview, &target, &projects_root)?;
    let report = apply_restore(
        plan,
        RestoreOptions {
            // The target is an isolated test directory, never the live Codex profile.
            codex_closed_confirmed: true,
            backup_root: sandbox.path().join("transaction-backups"),
            register_projects: false,
        },
    )?;

    assert!(report.verification.package_checksum_valid);
    assert!(report.verification.files_valid);
    assert!(report.verification.sessions_valid);
    assert!(report.verification.session_index_valid);
    assert!(report.verification.sqlite_threads_valid);
    assert!(report.verification.path_mapping_valid);
    assert!(report.verification.forbidden_files_absent);
    assert!(report.verification.project_files_valid);
    assert!(!report.verification.app_registration_valid);
    assert!(!report.verification.app_visible_ready);

    println!(
        "Windows-to-Windows local acceptance passed: {} project file(s), {} conversation(s), {} restored file(s).",
        preview.manifest.counts.project_files,
        preview.manifest.counts.conversations,
        report.restored_files
    );
    Ok(())
}

fn snapshot_database(source: &Path, target: &Path) -> Result<(), Box<dyn Error>> {
    let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut target = Connection::open(target)?;
    let backup = Backup::new(&source, &mut target)?;
    for _ in 0..200 {
        match backup.step(128)? {
            StepResult::Done => return Ok(()),
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(10)),
            _ => return Err("unsupported SQLite backup status".into()),
        }
    }
    Err("timed out while taking a read-only snapshot of the live Codex state database".into())
}
