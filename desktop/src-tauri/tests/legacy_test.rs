use rehome_desktop_lib::core::{
    models::{ContentCounts, SourceOs, TargetInventory},
    package::inspect_package,
    planner::build_restore_plan,
};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, fs::File, io::Write, path::Path};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const THREAD_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn imports_schema_v3_as_the_normal_package_preview() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let package = temp.path().join("legacy.zip");
    write_legacy_package(&package)?;

    let preview = inspect_package(&package)?;

    assert_eq!(preview.package_path, package);
    assert_eq!(preview.manifest.format, "codex-rehome");
    assert_eq!(preview.manifest.schema_version, 1);
    assert_eq!(preview.manifest.source_os, SourceOs::Windows);
    assert_eq!(preview.manifest.counts.projects, 1);
    assert_eq!(preview.manifest.counts.conversations, 1);
    assert_eq!(preview.manifest.counts.skills, 1);
    assert_eq!(preview.manifest.counts.plugins, 1);
    assert_eq!(preview.manifest.counts.generated_images, 1);
    assert_eq!(preview.manifest.counts.sqlite_threads, 1);
    assert_eq!(preview.manifest.projects[0].name, "legacy-project");
    assert_eq!(
        preview.manifest.conversations[0].task_id,
        Uuid::parse_str(THREAD_ID)?
    );
    assert!(preview.checksum_valid);
    assert_eq!(preview.forbidden_files_total, 0);
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/session_index.jsonl"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("/files/README.md")));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/metadata/threads.json"));
    Ok(())
}

#[test]
fn schema_v3_payloads_enter_the_normal_restore_plan() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let package = temp.path().join("legacy.zip");
    write_legacy_package(&package)?;
    let preview = inspect_package(&package)?;
    let codex_home = temp.path().join("target").join(".codex");
    let projects_root = temp.path().join("restored-projects");
    std::fs::create_dir_all(&codex_home)?;
    std::fs::create_dir_all(&projects_root)?;
    let database = Connection::open(codex_home.join("state_5.sqlite"))?;
    database.execute_batch(
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
    drop(database);
    let target = TargetInventory {
        codex_home,
        target_os: if cfg!(target_os = "macos") {
            SourceOs::Macos
        } else {
            SourceOs::Windows
        },
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: Vec::new(),
        conversations: Vec::new(),
    };

    let plan = build_restore_plan(&preview, &target, &projects_root)?;

    assert_eq!(plan.sessions.len(), 1);
    assert_eq!(plan.sessions[0].source_task_id, Uuid::parse_str(THREAD_ID)?);
    assert!(plan
        .operations
        .iter()
        .any(|operation| operation.package_source == "codex/session_index.jsonl"));
    assert!(plan
        .operations
        .iter()
        .any(|operation| operation.package_source == "codex/metadata/threads.json"));
    assert!(plan
        .operations
        .iter()
        .any(|operation| operation.package_source.ends_with("/files/README.md")));
    Ok(())
}

fn write_legacy_package(path: &Path) -> Result<(), Box<dyn Error>> {
    let root = "Codex-Migration-Windows-Source-20260726-120000";
    let session_path = format!("home/.codex/sessions/2026/07/26/rollout-{THREAD_ID}.jsonl");
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    files.insert(
        "MANIFEST.txt".into(),
        b"source_os=Windows\npackage_schema_version=3\nmode=standard\n".to_vec(),
    );
    files.insert(
        "MANIFEST.json".into(),
        serde_json::to_vec_pretty(&json!({
            "created_at": "20260726-120000",
            "source_os": "Windows",
            "package_schema_version": 3,
            "source_home": "C:\\Users\\Legacy",
            "mode": "standard",
            "projects": ["C:\\Users\\Legacy\\Documents\\legacy-project"],
            "selected_chats": [format!("C:\\Users\\Legacy\\.codex\\sessions\\rollout-{THREAD_ID}.jsonl")],
            "counts": {
                "sessions": 1,
                "skills": 1,
                "plugin_manifests": 1,
                "generated_images": 1,
                "projects": 1,
                "selected_chats": 1,
                "thread_index_export": 1
            },
            "exclude_strategy": "credentials, development folders, and runtime files excluded"
        }))?,
    );
    let session = format!(
        "{}\n{}\n",
        json!({
            "timestamp": "2026-07-26T12:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": THREAD_ID,
                "cwd": "C:\\Users\\Legacy\\Documents\\legacy-project",
                "thread_name": "Legacy conversation"
            }
        }),
        json!({"type": "response_item", "payload": {"role": "user", "content": "hello"}})
    );
    files.insert(session_path, session.as_bytes().to_vec());
    files.insert(
        "home/.codex/session_index.jsonl".into(),
        format!(
            "{}\n",
            json!({"id": THREAD_ID, "thread_name": "Legacy conversation", "updated_at": "2026-07-26T12:00:00Z"})
        )
        .into_bytes(),
    );
    files.insert(
        "home/.codex/skills/example/SKILL.md".into(),
        b"# Example skill\n".to_vec(),
    );
    files.insert(
        "home/.codex/plugins/cache/example/plugin.json".into(),
        b"{\"name\":\"example\"}\n".to_vec(),
    );
    files.insert(
        "home/.codex/generated_images/example.png".into(),
        b"synthetic-image".to_vec(),
    );
    files.insert(
        "projects/legacy-project/README.md".into(),
        b"# Legacy project\n".to_vec(),
    );
    files.insert("selected_chats/selected.jsonl".into(), session.into_bytes());
    files.insert(
        "metadata/path_map.json".into(),
        serde_json::to_vec_pretty(&json!({
            "schema": 3,
            "source_os": "Windows",
            "projects": [{
                "source_path": "C:\\Users\\Legacy\\Documents\\legacy-project",
                "package_project_name": "legacy-project",
                "package_project_path": "projects/legacy-project"
            }]
        }))?,
    );
    files.insert(
        "metadata/thread_index_export.json".into(),
        serde_json::to_vec_pretty(&json!({
            "schema": 3,
            "source_os": "Windows",
            "threads": [{
                "id": THREAD_ID,
                "cwd": "C:\\Users\\Legacy\\Documents\\legacy-project",
                "rollout_path": format!("C:\\Users\\Legacy\\.codex\\sessions\\rollout-{THREAD_ID}.jsonl"),
                "title": "Legacy conversation",
                "updated_at": 1785067200,
                "archived": 0,
                "has_user_event": 1
            }]
        }))?,
    );

    let checksum = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect::<String>();
    files.insert("SHA256SUMS.txt".into(), checksum.into_bytes());

    let file = File::create(path)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in files {
        archive.start_file(format!("{root}/{name}"), options)?;
        archive.write_all(&bytes)?;
    }
    archive.finish()?;
    Ok(())
}
