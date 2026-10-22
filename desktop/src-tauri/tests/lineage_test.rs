#[allow(dead_code)]
mod common;

use common::{synthetic_codex_fixture, THREAD_ID};
use rehome_desktop_lib::core::{models::CreatePackageRequest, package::create_package};
use serde_json::json;
use std::{error::Error, fs};
use uuid::Uuid;

const CHILD: &str = "33333333-3333-4333-8333-333333333333";
const GRANDCHILD: &str = "44444444-4444-4444-8444-444444444444";

#[test]
fn byte_offsets_and_fork_ids_follow_renamed_ancestors_and_repeat_import(
) -> Result<(), Box<dyn Error>> {
    use rehome_desktop_lib::core::{
        discovery::discover_codex,
        models::*,
        package::inspect_package,
        planner::build_restore_plan,
        restore::{apply_restore, rollback},
    };
    let f = synthetic_codex_fixture()?;
    let cwd = f.project_path.to_string_lossy();
    let parent = format!(
        "{}\n{}\n",
        json!({"type":"session_meta","ordinal":0,"payload":{
        "id":THREAD_ID,"session_id":THREAD_ID,"history_mode":"paginated","cwd":cwd}}),
        json!({"type":"response_item","ordinal":1,"payload":{"type":"message","role":"user",
            "content":[{"type":"input_text","text":format!("unchanged body {THREAD_ID} {cwd}")}]}})
    );
    fs::write(&f.session_path, &parent)?;
    let fork = |id: &str, source: &str, ordinal: u64, offset: u64| {
        format!(
            "{}\n",
            json!({
                "type":"session_meta","ordinal":ordinal,"payload":{"id":id,"session_id":THREAD_ID,
                    "history_mode":"paginated","cwd":cwd,"forked_from_id":source,
                    "forked_from_ordinal_exclusive":ordinal,"history_base":{"thread_id":source,
                        "end_ordinal_exclusive":ordinal,"end_byte_offset":offset}}
            })
        )
    };
    let child = fork(CHILD, THREAD_ID, 2, parent.len() as u64);
    let child_path = f
        .session_path
        .parent()
        .unwrap()
        .join(format!("rollout-{CHILD}.jsonl"));
    fs::write(&child_path, &child)?;
    fs::write(
        f.session_path
            .parent()
            .unwrap()
            .join(format!("rollout-{GRANDCHILD}.jsonl")),
        fork(GRANDCHILD, CHILD, 3, child.len() as u64),
    )?;
    let source_db = rusqlite::Connection::open(&f.state_db_path)?;
    source_db.execute_batch(
        "ALTER TABLE threads ADD COLUMN history_mode TEXT NOT NULL DEFAULT 'legacy'",
    )?;
    source_db.execute(
        "UPDATE threads SET cwd=?1, rollout_path=?2, history_mode='paginated'",
        rusqlite::params![cwd, f.session_path.to_string_lossy()],
    )?;
    for id in [CHILD, GRANDCHILD] {
        let path = f
            .session_path
            .parent()
            .unwrap()
            .join(format!("rollout-{id}.jsonl"));
        source_db.execute("INSERT INTO threads (id,cwd,rollout_path,title,updated_at,archived,has_user_event,preview,history_mode) VALUES (?1,?2,?3,'fork','2026-10-22',0,1,'fork','paginated')",
            rusqlite::params![id,cwd,path.to_string_lossy()])?;
    }
    let package = f.root.join("nested.rehome");
    create_package(CreatePackageRequest {
        codex_home: f.codex_home.clone(),
        project_paths: vec![f.project_path.clone()],
        conversation_ids: vec![GRANDCHILD.parse()?, CHILD.parse()?, THREAD_ID.parse()?],
        output_path: package.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let package_bytes = fs::read(&package)?;
    let preview = inspect_package(&package)?;
    let home = f.root.join("target/.codex");
    fs::create_dir_all(&home)?;
    let schema: String = source_db.query_row(
        "SELECT sql FROM sqlite_master WHERE name='threads'",
        [],
        |r| r.get(0),
    )?;
    rusqlite::Connection::open(home.join("state_5.sqlite"))?.execute_batch(&schema)?;
    let parent_entry = preview
        .manifest
        .conversations
        .iter()
        .find(|c| c.task_id.to_string() == THREAD_ID)
        .unwrap();
    let old_path = home.join(parent_entry.archive_path.strip_prefix("codex/").unwrap());
    fs::create_dir_all(old_path.parent().unwrap())?;
    fs::write(&old_path, format!("{parent}\n"))?;
    let mut target = TargetInventory {
        codex_home: home.clone(),
        target_os: if cfg!(windows) {
            SourceOs::Windows
        } else {
            SourceOs::Macos
        },
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![parent_entry.clone()],
    };
    let projects = f.root.join("much-longer-mapped-project-destination");
    let plan = build_restore_plan(&preview, &target, &projects)?;
    let parent_session = plan
        .sessions
        .iter()
        .find(|s| s.source_task_id.to_string() == THREAD_ID)
        .unwrap();
    assert_ne!(parent_session.target_task_id, parent_session.source_task_id);
    let options = || RestoreOptions {
        codex_closed_confirmed: true,
        backup_root: f.root.join("backups"),
        register_projects: false,
    };
    let report = apply_restore(plan.clone(), options())?;
    assert!(report.verification.sessions_valid && report.verification.sqlite_threads_valid);
    for source in [CHILD, GRANDCHILD] {
        let session = plan
            .sessions
            .iter()
            .find(|s| s.source_task_id.to_string() == source)
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&session.target)?.lines().next().unwrap())?;
        let parent_id = if source == CHILD { THREAD_ID } else { CHILD };
        let parent_session = plan
            .sessions
            .iter()
            .find(|s| s.source_task_id.to_string() == parent_id)
            .unwrap();
        assert_eq!(
            value["payload"]["history_base"]["thread_id"],
            parent_session.target_task_id.to_string()
        );
        assert_eq!(
            value["payload"]["forked_from_id"],
            parent_session.target_task_id.to_string()
        );
        assert_eq!(
            value["payload"]["session_id"],
            plan.sessions
                .iter()
                .find(|s| s.source_task_id.to_string() == THREAD_ID)
                .unwrap()
                .target_task_id
                .to_string()
        );
        assert_eq!(
            value["payload"]["history_base"]["end_byte_offset"],
            fs::metadata(&parent_session.target)?.len()
        );
    }
    let parent_body: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&parent_session.target)?
            .lines()
            .nth(1)
            .unwrap(),
    )?;
    assert_eq!(
        parent_body["payload"]["content"][0]["text"],
        format!("unchanged body {THREAD_ID} {cwd}")
    );
    let discovered = discover_codex(Some(home.clone()))?;
    target.conversations = discovered.conversations;
    target.projects = discovered.projects;
    let repeated = build_restore_plan(&preview, &target, &projects)?;
    assert!(repeated
        .sessions
        .iter()
        .all(|s| s.action == SessionAction::Skip));
    rollback(report.transaction_id)?;
    assert_eq!(fs::read_to_string(&old_path)?, format!("{parent}\n"));
    assert!(!parent_session.target.exists());
    assert_eq!(fs::read(&package)?, package_bytes);
    assert_eq!(fs::read_to_string(&f.session_path)?, parent);
    Ok(())
}

#[test]
fn fork_export_requires_explicit_ancestor_selection() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let parent = json!({"type":"session_meta","ordinal":0,"payload":{
        "id":THREAD_ID,"history_mode":"paginated","cwd":common::WINDOWS_CWD
    }});
    fs::write(&fixture.session_path, format!("{parent}\n"))?;
    let child = json!({"type":"session_meta","ordinal":1,"payload":{
        "id":CHILD,"history_mode":"paginated","cwd":common::WINDOWS_CWD,
        "forked_from_id":THREAD_ID,"forked_from_ordinal_exclusive":1
    }});
    fs::write(
        fixture
            .session_path
            .parent()
            .unwrap()
            .join(format!("rollout-{CHILD}.jsonl")),
        format!("{child}\n"),
    )?;
    let output = fixture.root.join("lineage.rehome");
    let request = |ids| CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![],
        conversation_ids: ids,
        output_path: output.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    };
    let error = create_package(request(vec![CHILD.parse()?])).unwrap_err();
    assert!(
        error.message.contains("required source rollout"),
        "{}",
        error.message
    );
    assert!(error.message.contains(THREAD_ID));
    assert!(!output.exists());
    let report = create_package(request(vec![THREAD_ID.parse()?, CHILD.parse()?]))?;
    assert_eq!(report.counts.conversations, 2);
    assert_eq!(
        fs::read_to_string(&fixture.session_path)?,
        format!("{parent}\n")
    );
    Ok(())
}

#[test]
fn old_partial_package_is_rejected_before_planning_target_writes() -> Result<(), Box<dyn Error>> {
    use rehome_desktop_lib::core::{
        models::*, package::inspect_package, planner::build_restore_plan,
    };
    use std::io::{Read, Write};
    let f = synthetic_codex_fixture()?;
    let output = f.root.join("source.rehome");
    create_package(CreatePackageRequest {
        codex_home: f.codex_home.clone(),
        project_paths: vec![],
        conversation_ids: vec![THREAD_ID.parse()?],
        output_path: output.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let mut zip = zip::ZipArchive::new(fs::File::open(&output)?)?;
    let preview = inspect_package(&output)?;
    let source = preview.manifest.conversations[0].archive_path.clone();
    let missing = "55555555-5555-4555-8555-555555555555";
    let body = format!(
        "{}\n",
        json!({"type":"session_meta","ordinal":1,"payload":{
            "id":THREAD_ID,"history_mode":"paginated","forked_from_id":missing,"forked_from_ordinal_exclusive":1
        }})
    );
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(body.as_bytes()));
    let mut manifest = preview.manifest;
    let previous = manifest.conversations[0].content_hash.clone();
    manifest.conversations[0].content_hash = hash.clone();
    let partial = f.root.join("old-partial.rehome");
    let mut writer = zip::ZipWriter::new(fs::File::create(&partial)?);
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if name == source {
            bytes = body.as_bytes().to_vec();
        }
        if name == "manifest.json" {
            bytes = serde_json::to_vec(&manifest)?;
        }
        if name == "checksums.sha256" {
            bytes = String::from_utf8(bytes)?
                .replace(&previous, &hash)
                .into_bytes();
        }
        writer.start_file(name, zip::write::SimpleFileOptions::default())?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    let preview = inspect_package(&partial)?;
    let home = f.root.join("empty-target/.codex");
    fs::create_dir_all(&home)?;
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
    let error = build_restore_plan(&preview, &target, &f.root.join("target-projects")).unwrap_err();
    assert!(error.message.contains(missing));
    assert_eq!(fs::read_dir(home)?.count(), 0);
    Ok(())
}
