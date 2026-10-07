#[allow(dead_code)]
mod common;

use rehome_desktop_lib::core::{
    models::{ContentCounts, CreatePackageRequest, RestoreOptions, SourceOs, TargetInventory},
    package::{create_package, inspect_package},
    planner::build_restore_plan,
    restore::apply_restore,
};
use std::{error::Error, fs, path::PathBuf};
use uuid::Uuid;

// Explicit local acceptance helper. Inputs must be a synthetic Codex rollout;
// ordinary CI uses the deterministic unit/round-trip tests instead.
#[test]
#[ignore = "requires an isolated synthetic source and a fresh absolute output directory"]
fn prepare_workspace_resume_fixture() -> Result<(), Box<dyn Error>> {
    let source = PathBuf::from(std::env::var("REHOME_WORKSPACE_PROBE_SOURCE")?);
    let output = PathBuf::from(std::env::var("REHOME_WORKSPACE_PROBE_OUTPUT")?);
    assert!(output.is_absolute() && !output.exists());
    let fixture = common::synthetic_codex_fixture()?;
    let source_db = rusqlite::Connection::open(&fixture.state_db_path)?;
    source_db.execute_batch(
        "ALTER TABLE threads ADD COLUMN created_at INTEGER NOT NULL DEFAULT 1791417600;
        ALTER TABLE threads ADD COLUMN source TEXT NOT NULL DEFAULT 'appServer';
        ALTER TABLE threads ADD COLUMN model_provider TEXT NOT NULL DEFAULT 'openai';
        ALTER TABLE threads ADD COLUMN approval_mode TEXT NOT NULL DEFAULT 'never';
        ALTER TABLE threads ADD COLUMN sandbox_policy TEXT NOT NULL DEFAULT 'read-only';
        ALTER TABLE threads ADD COLUMN history_mode TEXT NOT NULL DEFAULT 'legacy';",
    )?;
    let native_project =
        rehome_desktop_lib::core::paths::codex_project_path(&fixture.project_path)?;
    source_db.execute("UPDATE threads SET cwd = ?1", [&native_project])?;
    drop(source_db);
    let mut index: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.session_index_path)?)?;
    index["cwd"] = native_project.clone().into();
    fs::write(&fixture.session_index_path, format!("{index}\n"))?;
    // Keep the native project association for export; the deliberately foreign
    // workspace roots reproduce cross-platform resume independently of cwd.
    let mut records = fs::read_to_string(&source)?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let old = records[0]["payload"]["cwd"].as_str().unwrap().to_owned();
    records[0]["payload"]["id"] = common::THREAD_ID.into();
    records[0]["payload"]["cwd"] = native_project.into();
    records.push(serde_json::json!({"type":"turn_context","payload":{"cwd":old}}));
    fs::write(
        &fixture.session_path,
        records.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )?;
    // Project ID from the fixture's authoritative index associates the selected
    // conversation with these harmless project files.
    fs::create_dir(&output)?;
    let package = output.join("workspace.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(common::THREAD_ID)?],
        output_path: package.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let home = output.join(".codex");
    fs::create_dir(&home)?;
    let database = rusqlite::Connection::open_with_flags(
        std::env::var("REHOME_WORKSPACE_PROBE_DATABASE")?,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut destination = rusqlite::Connection::open(home.join("state_5.sqlite"))?;
    rusqlite::backup::Backup::new(&database, &mut destination)?.run_to_completion(
        64,
        std::time::Duration::from_millis(10),
        None,
    )?;
    drop(destination);
    drop(database);
    let target = TargetInventory {
        codex_home: home.clone(),
        target_os: if cfg!(windows) {
            SourceOs::Windows
        } else {
            SourceOs::Macos
        },
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(
        &inspect_package(&package)?,
        &target,
        &output.join("projects"),
    )?;
    assert!(plan
        .reference_rewrites
        .iter()
        .any(|r| r.kind == rehome_desktop_lib::core::models::ReferenceRewriteKind::ProjectPath));
    let session = plan.sessions[0].target.clone();
    let report = apply_restore(
        plan,
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: output.join("backups"),
            register_projects: false,
        },
    )?;
    assert!(report.verification.sessions_valid && report.verification.path_mapping_valid);
    let receipt = serde_json::json!({"session":session,"home":home,"threadId":common::THREAD_ID,"verification":report.verification});
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "PASS isolated workspace restore fixture: {}",
        output.display()
    );
    Ok(())
}
