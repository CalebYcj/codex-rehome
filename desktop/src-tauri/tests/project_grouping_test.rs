#[allow(dead_code)]
mod common;
use common::synthetic_codex_fixture;
use rehome_desktop_lib::core::{
    discovery::discover_codex,
    models::*,
    package::{create_package, inspect_package},
    planner::build_restore_plan,
    restore::apply_restore,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{error::Error, fs, path::Path};
use uuid::Uuid;

static APPDATA_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct AppData(std::ffi::OsString);
impl AppData {
    fn isolate(path: &Path) -> Self {
        let saved = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
        std::env::set_var("LOCALAPPDATA", path);
        Self(saved)
    }
}
impl Drop for AppData {
    fn drop(&mut self) {
        if self.0.is_empty() {
            std::env::remove_var("LOCALAPPDATA")
        } else {
            std::env::set_var("LOCALAPPDATA", &self.0)
        }
    }
}

fn seed(home: &Path, roots: &[String], cwds: &[String]) -> Result<Vec<Uuid>, Box<dyn Error>> {
    fs::write(
        home.join(".codex-global-state.json"),
        serde_json::to_vec(&json!({"electron-saved-workspace-roots":roots}))?,
    )?;
    let db = Connection::open(home.join("state_5.sqlite"))?;
    db.execute("DELETE FROM threads", [])?;
    let sessions = home.join("sessions");
    fs::create_dir_all(&sessions)?;
    let mut index = String::new();
    let mut ids = vec![];
    for (i, cwd) in cwds.iter().enumerate() {
        let id = Uuid::from_u128(0x11111111111141118111111111111111 + i as u128);
        let path = sessions.join(format!("rollout-{id}.jsonl"));
        let data = format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"history_mode":"legacy"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("grouping-audit-message-{i}")}]}})
        );
        fs::write(&path, data)?;
        index.push_str(&format!(
            "{}\n",
            json!({"id":id,"thread_name":format!("chat-{i}"),"updated_at":"2026-10-19T00:00:00Z"})
        ));
        db.execute("INSERT INTO threads(id,cwd,rollout_path,title,updated_at,archived,has_user_event,preview) VALUES(?1,?2,?3,?4,'2026-10-19T00:00:00Z',0,1,'audit')",params![id.to_string(),cwd,path.to_string_lossy(),format!("chat-{i}")])?;
        ids.push(id);
    }
    fs::write(home.join("session_index.jsonl"), index)?;
    Ok(ids)
}

#[test]
fn independent_projects_four_chats_roundtrip_and_repeat() -> Result<(), Box<dyn Error>> {
    roundtrip(false)
}

#[test]
fn logical_projects_four_chats_roundtrip_and_repeat() -> Result<(), Box<dyn Error>> {
    roundtrip(true)
}

fn roundtrip(modern: bool) -> Result<(), Box<dyn Error>> {
    let _lock = APPDATA_LOCK.lock().unwrap();
    let f = synthetic_codex_fixture()?;
    fs::remove_file(&f.session_path)?;
    let p2 = f.root.join("other/论文");
    fs::create_dir_all(&p2)?;
    fs::write(p2.join("notes.txt"), "second project")?;
    let roots = vec![
        fs::canonicalize(&f.project_path)?
            .to_string_lossy()
            .to_string(),
        fs::canonicalize(&p2)?.to_string_lossy().to_string(),
    ];
    let cwds = vec![
        roots[0].clone(),
        roots[0].clone(),
        roots[1].clone(),
        roots[1].clone(),
    ];
    let ids = seed(&f.codex_home, &roots, &cwds)?;
    let project_schema = "CREATE TABLE projects(id TEXT PRIMARY KEY,name TEXT NOT NULL,metadata TEXT NOT NULL DEFAULT '{}',position INTEGER NOT NULL,created_at_ms INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL);CREATE TABLE project_roots(project_id TEXT NOT NULL,position INTEGER NOT NULL,path TEXT NOT NULL,PRIMARY KEY(project_id,position));";
    if modern {
        let db = Connection::open(&f.state_db_path)?;
        db.execute_batch(project_schema)?;
        db.execute_batch("ALTER TABLE threads ADD COLUMN project_id TEXT")?;
        for (i, root) in roots.iter().enumerate() {
            let id = format!("logical-{i}");
            db.execute(
                "INSERT INTO projects VALUES(?1,'同名项目','{}',?2,0,0)",
                params![id, i],
            )?;
            db.execute(
                "INSERT INTO project_roots VALUES(?1,0,?2)",
                params![id, root],
            )?;
            db.execute(
                "UPDATE threads SET project_id=?1 WHERE cwd=?2",
                params![id, root],
            )?;
        }
        // The first header predates a move; current SQLite remains authoritative.
        let rollout = f
            .codex_home
            .join("sessions")
            .join(format!("rollout-{}.jsonl", ids[0]));
        let data = fs::read_to_string(&rollout)?.replace(
            &serde_json::to_string(&roots[0])?,
            &serde_json::to_string(&f.root.join("old-workspace").to_string_lossy())?,
        );
        fs::write(rollout, data)?;
    }
    let inventory = discover_codex(Some(f.codex_home.clone()))?;
    assert_eq!(inventory.conversations.len(), 4);
    for (id, cwd) in ids.iter().zip(&cwds) {
        let project = inventory
            .projects
            .iter()
            .find(|p| p.source_path == *cwd)
            .unwrap();
        assert_eq!(
            inventory
                .conversations
                .iter()
                .find(|c| c.task_id == *id)
                .unwrap()
                .project_id,
            Some(project.project_id)
        );
    }
    let path = f.root.join("four-chats.rehome");
    create_package(CreatePackageRequest {
        codex_home: f.codex_home.clone(),
        project_paths: vec![f.project_path.clone(), p2],
        conversation_ids: ids.clone(),
        output_path: path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&path)?;
    assert_eq!(preview.manifest.projects.len(), 2);
    assert_eq!(preview.manifest.conversations.len(), 4);
    let target_home = f.root.join("target/.codex");
    fs::create_dir_all(&target_home)?;
    let schema: String = Connection::open(&f.state_db_path)?.query_row(
        "SELECT sql FROM sqlite_master WHERE name='threads'",
        [],
        |r| r.get(0),
    )?;
    Connection::open(target_home.join("state_5.sqlite"))?.execute_batch(&schema)?;
    if modern {
        Connection::open(target_home.join("state_5.sqlite"))?.execute_batch(project_schema)?;
    }
    fs::write(target_home.join("session_index.jsonl"), "")?;
    let target = TargetInventory {
        codex_home: target_home.clone(),
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
    let root = f.root.join("target/projects");
    let plan = build_restore_plan(&preview, &target, &root)?;
    let appdata = f.root.join("isolated-appdata");
    fs::create_dir_all(&appdata)?;
    let _appdata = AppData::isolate(&appdata);
    let report = apply_restore(
        plan.clone(),
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: f.root.join("backups"),
            register_projects: false,
        },
    );
    let report = report?;
    assert!(
        report.verification.sessions_valid
            && report.verification.project_files_valid
            && report.verification.path_mapping_valid
            && report.verification.sqlite_threads_valid
            && report.verification.session_index_valid
    );
    let db = Connection::open(target_home.join("state_5.sqlite"))?;
    if modern {
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))?,
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM threads WHERE project_id IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            4
        );
        for (id, cwd) in ids.iter().zip(&cwds) {
            let source_group = if cwd == &roots[0] {
                "logical-0"
            } else {
                "logical-1"
            };
            let expected = Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                format!("rehome:logical-project:{source_group}").as_bytes(),
            )
            .to_string();
            assert_eq!(
                db.query_row(
                    "SELECT project_id FROM threads WHERE id=?1",
                    [id.to_string()],
                    |r| r.get::<_, String>(0)
                )?,
                expected
            );
        }
    }
    for chat in &preview.manifest.conversations {
        let project = preview
            .manifest
            .projects
            .iter()
            .find(|p| Some(p.project_id) == chat.project_id)
            .unwrap();
        let session = plan
            .sessions
            .iter()
            .find(|s| s.target_task_id == chat.task_id)
            .unwrap();
        let line = fs::read_to_string(&session.target)?;
        let head: Value = serde_json::from_str(line.lines().next().unwrap())?;
        let cwd: String = db.query_row(
            "SELECT cwd FROM threads WHERE id=?1",
            [chat.task_id.to_string()],
            |r| r.get(0),
        )?;
        assert_eq!(head["payload"]["cwd"], cwd);
        assert!(Path::new(&cwd).is_dir());
        assert!(cwd.ends_with(&project.name));
        assert!(line.contains("grouping-audit-message-"));
    }
    let repeat = build_restore_plan(&preview, &target, &root)?;
    assert!(repeat
        .operations
        .iter()
        .all(|o| o.action == ChangeKind::Unchanged));
    println!("PASS four chats retain two distinct project roots, files, IDs, messages and repeated-import identity; GUI registration intentionally not invoked");
    Ok(())
}

#[test]
fn nested_registered_projects_use_exact_child() -> Result<(), Box<dyn Error>> {
    let f = synthetic_codex_fixture()?;
    fs::remove_file(&f.session_path)?;
    let child = f.project_path.join("child");
    fs::create_dir_all(&child)?;
    let roots = vec![
        fs::canonicalize(&f.project_path)?
            .to_string_lossy()
            .to_string(),
        fs::canonicalize(&child)?.to_string_lossy().to_string(),
    ];
    let ids = seed(&f.codex_home, &roots, &[roots[1].clone()])?;
    let inventory = discover_codex(Some(f.codex_home.clone()))?;
    let expected = inventory
        .projects
        .iter()
        .find(|p| p.source_path == roots[1])
        .unwrap()
        .project_id;
    let observed = inventory
        .conversations
        .iter()
        .find(|c| c.task_id == ids[0])
        .unwrap()
        .project_id;
    println!("NESTED expected_child={expected} observed={observed:?}");
    assert_eq!(
        observed,
        Some(expected),
        "exact child workspace must take precedence over containing parent"
    );
    Ok(())
}

#[test]
fn moved_thread_uses_current_sqlite_project() -> Result<(), Box<dyn Error>> {
    let f = synthetic_codex_fixture()?;
    fs::remove_file(&f.session_path)?;
    let root = fs::canonicalize(&f.project_path)?
        .to_string_lossy()
        .to_string();
    let ids = seed(
        &f.codex_home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&root),
    )?;
    let path = f
        .codex_home
        .join("sessions")
        .join(format!("rollout-{}.jsonl", ids[0]));
    let old = f.root.join("old-workspace");
    fs::create_dir_all(&old)?;
    let old = fs::canonicalize(old)?.to_string_lossy().to_string();
    fs::write(
        path,
        format!(
            "{}\n",
            json!({"type":"session_meta","payload":{"id":ids[0],"cwd":old}})
        ),
    )?;
    let inventory = discover_codex(Some(f.codex_home.clone()))?;
    let expected = inventory.projects[0].project_id;
    let observed = inventory
        .conversations
        .iter()
        .find(|c| c.task_id == ids[0])
        .unwrap()
        .project_id;
    println!("MOVED database_cwd_is_registered=true stale_header=true expected={expected} observed={observed:?}");
    assert_eq!(observed,Some(expected),"current SQLite project should not become ungrouped because initial session header is stale");
    Ok(())
}

#[test]
#[ignore = "isolated native fixture prepared by private audit script"]
fn native_project_source_roundtrip() -> Result<(), Box<dyn Error>> {
    let _lock = APPDATA_LOCK.lock().unwrap();
    let base = std::env::var("REHOME_GROUPING_AUDIT_ROOT")?;
    let base = Path::new(&base);
    let receipt: Value = serde_json::from_slice(&fs::read(base.join("source-receipt.json"))?)?;
    let home = Path::new(receipt["home"].as_str().unwrap());
    let projects = receipt["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::path::PathBuf::from(p["roots"][0]["path"].as_str().unwrap()))
        .collect::<Vec<_>>();
    let ids = receipt["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| Uuid::parse_str(t["id"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let source_db = Connection::open(home.join("state_5.sqlite"))?;
    let source_count: i64 = source_db.query_row(
        "SELECT COUNT(*) FROM threads WHERE project_id IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(source_count, 4);
    drop(source_db);
    let package = base.join("native.rehome");
    create_package(CreatePackageRequest {
        codex_home: home.into(),
        project_paths: projects,
        conversation_ids: ids,
        output_path: package.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package)?;
    let target_home = base.join("target/.codex");
    let target = TargetInventory {
        codex_home: target_home.clone(),
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
    let plan = build_restore_plan(&preview, &target, &base.join("target/projects"))?;
    let appdata = base.join("appdata");
    fs::create_dir_all(&appdata)?;
    let _appdata = AppData::isolate(&appdata);
    let report = apply_restore(
        plan,
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: base.join("backups"),
            register_projects: false,
        },
    );
    let report = report?;
    fs::write(
        base.join("rehome-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    assert!(
        report.verification.files_valid
            && report.verification.sessions_valid
            && report.verification.sqlite_threads_valid
            && report.verification.project_files_valid
    );
    // Roll back before external inspection/planning opens the WAL database.
    std::env::set_var("LOCALAPPDATA", &appdata);
    let reverted = rehome_desktop_lib::core::restore::rollback(report.transaction_id);
    // This dedicated test runs alone in a child process with isolated app data.
    std::env::remove_var("LOCALAPPDATA");
    assert!(reverted?.success);
    let db = Connection::open(target_home.join("state_5.sqlite"))?;
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads", [], |r| r.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM project_roots", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    drop(db);
    let plan = build_restore_plan(&preview, &target, &base.join("target/projects"))?;
    std::env::set_var("LOCALAPPDATA", &appdata);
    let restored_again = apply_restore(
        plan,
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: base.join("backups"),
            register_projects: false,
        },
    );
    std::env::remove_var("LOCALAPPDATA");
    assert!(restored_again?.verification.path_mapping_valid);
    let repeat = build_restore_plan(&preview, &target, &base.join("target/projects"))?;
    std::env::set_var("LOCALAPPDATA", &appdata);
    let repeated = apply_restore(
        repeat,
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: base.join("backups"),
            register_projects: false,
        },
    );
    std::env::remove_var("LOCALAPPDATA");
    assert!(repeated?.verification.path_mapping_valid);
    let db = Connection::open(target_home.join("state_5.sqlite"))?;
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM threads WHERE project_id IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        4
    );
    println!("NATIVE repeated import without duplicate projects, immediate rollback to empty native tables, and reapply PASS");
    println!("NATIVE ReHome internal checks passed; native target projects=2, assigned_threads=4; source projects=2 assigned_threads=4");
    // Native protocol assertions are deliberately in the separate fresh-server readback.
    Ok(())
}
