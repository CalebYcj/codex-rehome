mod common;

use common::{synthetic_codex_fixture, THREAD_ID};
use rehome_desktop_lib::core::{
    error::ErrorCode,
    models::{
        ContentCounts, ConversationEntry, CreatePackageRequest, ExclusionSummary, PackageManifest,
        PackageMode, SourceOs,
    },
    package::{create_package, create_package_replacing, inspect_package},
};
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime},
};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipArchive, ZipWriter};

#[test]
fn packages_selected_fixture_content_without_mutating_sources() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(fixture.codex_home.join("auth.json"), b"fixture-token\n")?;
    fs::write(fixture.codex_home.join("config.toml"), b"model = 'local'\n")?;
    let skill_reference = fixture
        .skill_path
        .parent()
        .unwrap()
        .join("references")
        .join("guide.md");
    fs::create_dir_all(skill_reference.parent().unwrap())?;
    fs::write(&skill_reference, b"# Synthetic guide\n")?;
    let plugin_tool = fixture
        .plugin_manifest_path
        .parent()
        .unwrap()
        .join("bin")
        .join("tool.js");
    fs::create_dir_all(plugin_tool.parent().unwrap())?;
    fs::write(&plugin_tool, b"export const synthetic = true;\n")?;
    fs::write(
        fixture.plugin_manifest_path.parent().unwrap().join(".env"),
        b"PLUGIN_SECRET=excluded\n",
    )?;
    assert!([
        &fixture.session_path,
        &fixture.session_index_path,
        &fixture.state_db_path,
        &fixture.skill_path,
        &fixture.plugin_manifest_path,
        &fixture.generated_image_path,
        &fixture.readme_path,
        &fixture.env_path,
        &fixture.git_config_path,
        &fixture.node_modules_file_path,
    ]
    .iter()
    .all(|path| path.exists()));
    let source_before = snapshot_files(&fixture.root)?;
    let output_directory = tempfile::tempdir()?;
    let output = output_directory.path().join("handoff.rehome");

    let report = create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output.clone(),
        source_device_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333")?,
        skill_paths: vec![fixture.skill_path.clone()],
        plugin_paths: vec![fixture.plugin_manifest_path.clone()],
        generated_image_paths: vec![fixture.generated_image_path.clone()],
    })?;

    assert_eq!(report.package_path, output);
    assert_eq!(report.counts.projects, 1);
    assert_eq!(report.counts.project_files, 1);
    assert_eq!(report.counts.conversations, 1);
    assert_eq!(report.counts.skills, 1);
    assert_eq!(report.counts.plugins, 1);
    assert_eq!(report.counts.generated_images, 1);
    assert_eq!(report.counts.sqlite_threads, 1);
    assert_eq!(
        report.bytes_written,
        fs::metadata(&report.package_path)?.len()
    );

    let preview = inspect_package(&report.package_path)?;
    assert_eq!(preview.manifest.format, "codex-rehome");
    assert_eq!(preview.manifest.schema_version, 1);
    assert_eq!(preview.manifest.source_os, current_source_os());
    assert_eq!(
        preview.manifest.source_device_id,
        Uuid::parse_str("33333333-3333-4333-8333-333333333333")?
    );
    assert_eq!(preview.manifest.counts, report.counts);
    assert_eq!(preview.forbidden_files_total, 0);
    assert!(preview.checksum_valid);
    assert!(preview.entries.iter().all(|entry| !entry.contains('\\')));
    assert!(!preview.entries.iter().any(|entry| entry.contains("/.git/")));
    assert!(!preview.entries.iter().any(|entry| entry.ends_with("/.env")));
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("auth.json")));
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("config.toml")));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/session_index.jsonl"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/metadata/threads.json"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("/files/README.md")));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/skills/synthetic-skill/SKILL.md"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/skills/synthetic-skill/references/guide.md"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/plugins/cache/synthetic-plugin/manifest.json"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/plugins/cache/synthetic-plugin/bin/tool.js"));
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry == "codex/plugins/cache/synthetic-plugin/.env"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/generated_images/synthetic-image.png"));
    assert!(preview
        .entries
        .iter()
        .filter(|entry| entry.ends_with(".jsonl"))
        .any(|entry| entry.contains("codex/sessions/")));
    assert_eq!(preview.entries, sorted(preview.entries.clone()));
    let conversation = &preview.manifest.conversations[0];
    let mut archived = Vec::new();
    ZipArchive::new(fs::File::open(&report.package_path)?)?
        .by_name(&conversation.archive_path)?
        .read_to_end(&mut archived)?;
    assert_eq!(conversation.content_hash, checksum(&archived));
    assert_eq!(snapshot_files(&fixture.root)?, source_before);
    Ok(())
}

#[test]
fn package_collapses_overlapping_plugin_roots() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let codex_home = temp.path().join(".codex");
    let version_root = codex_home
        .join("plugins")
        .join("cache")
        .join("openai-primary-runtime")
        .join("presentations")
        .join("1.0.0");
    let plugin_marker = version_root.join(".codex-plugin").join("plugin.json");
    let nested_manifest = version_root
        .join("skills")
        .join("layout-library")
        .join("manifest.json");
    fs::create_dir_all(plugin_marker.parent().unwrap())?;
    fs::create_dir_all(nested_manifest.parent().unwrap())?;
    fs::write(&plugin_marker, b"{}")?;
    fs::write(&nested_manifest, b"asset manifest")?;
    let output = temp.path().join("overlapping-plugin-roots.rehome");

    let report = create_package(CreatePackageRequest {
        codex_home,
        project_paths: vec![],
        conversation_ids: vec![],
        output_path: output.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![plugin_marker, nested_manifest],
        generated_image_paths: vec![],
    })?;

    assert_eq!(report.counts.plugins, 1);
    let preview = inspect_package(&output)?;
    assert_eq!(
        preview
            .entries
            .iter()
            .filter(|entry| entry.ends_with("/skills/layout-library/manifest.json"))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn package_deduplicates_selected_index_rows_with_a_stable_last_row_winner(
) -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let first = serde_json::json!({
        "id": THREAD_ID,
        "title": "Older title",
        "updated_at": "2026-07-21T00:00:00Z",
        "rollout_path": "C:/older.jsonl",
    });
    let winner = serde_json::json!({
        "id": THREAD_ID,
        "title": "Stable winner",
        "updated_at": "2026-07-22T00:00:00Z",
        "rollout_path": "C:/winner.jsonl",
    });
    fs::write(
        &fixture.session_index_path,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&first)?,
            serde_json::to_string(&winner)?
        ),
    )?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("deduplicated-index.rehome");

    create_package(package_request(&fixture, package.clone()))?;

    let index = String::from_utf8(read_zip_entry(&package, "codex/session_index.jsonl")?)?;
    let rows = index
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    assert_eq!(rows, vec![winner]);
    let preview = inspect_package(&package)?;
    assert_eq!(preview.manifest.conversations[0].title, "Stable winner");
    assert_eq!(
        preview.manifest.conversations[0].updated_at,
        "2026-07-22T00:00:00Z"
    );
    Ok(())
}

#[test]
fn package_exports_threads_from_a_private_wal_snapshot() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let generator_directory = tempfile::tempdir()?;
    let generator = generator_directory.path().join("generator.sqlite");
    let writer = Connection::open(&generator)?;
    writer.pragma_update(None, "journal_mode", "WAL")?;
    writer.pragma_update(None, "wal_autocheckpoint", 0)?;
    writer.execute(
        "CREATE TABLE threads (\
            id TEXT PRIMARY KEY, cwd TEXT NOT NULL, rollout_path TEXT NOT NULL, \
            title TEXT NOT NULL, updated_at TEXT NOT NULL, archived INTEGER NOT NULL, \
            has_user_event INTEGER NOT NULL, preview TEXT NOT NULL\
        )",
        [],
    )?;
    writer.execute(
        "INSERT INTO threads VALUES (?1, ?2, ?3, ?4, ?5, 0, 1, ?6)",
        params![
            THREAD_ID,
            r"C:\Users\OldUser\Documents\visual",
            r"C:\Users\OldUser\.codex\sessions\thread.jsonl",
            "WAL package thread",
            "2026-07-22T00:00:00Z",
            "WAL package preview",
        ],
    )?;

    let state_database = fixture.codex_home.join("state_9.sqlite");
    fs::copy(&generator, &state_database)?;
    fs::copy(
        sqlite_sidecar(&generator, "-wal"),
        sqlite_sidecar(&state_database, "-wal"),
    )?;
    assert!(!sqlite_sidecar(&state_database, "-shm").exists());
    let source_before = snapshot_files(&fixture.root)?;
    let output_directory = tempfile::tempdir()?;
    let output = output_directory.path().join("wal-handoff.rehome");

    let report = create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: Vec::new(),
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output,
        source_device_id: Uuid::new_v4(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;

    assert_eq!(report.counts.sqlite_threads, 1);
    assert_eq!(snapshot_files(&fixture.root)?, source_before);
    assert!(!sqlite_sidecar(&state_database, "-shm").exists());
    drop(writer);
    Ok(())
}

#[test]
fn package_uses_a_transactional_sqlite_snapshot_during_concurrent_writes(
) -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let state_database = fixture.codex_home.join("state_9.sqlite");
    {
        let connection = Connection::open(&state_database)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "wal_autocheckpoint", 0)?;
        connection.execute(
            "CREATE TABLE threads (\
                id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT, updated_at TEXT, \
                archived INTEGER, has_user_event INTEGER, preview TEXT\
            )",
            [],
        )?;
        connection.execute(
            "INSERT INTO threads VALUES (?1, '', '', '0', '', 0, 1, '0')",
            [THREAD_ID],
        )?;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let stop_writer = Arc::clone(&stop);
    let started_writer = Arc::clone(&started);
    let database_for_writer = state_database.clone();
    let writer = thread::spawn(move || -> Result<(), String> {
        let mut connection = Connection::open(database_for_writer).map_err(|e| e.to_string())?;
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .map_err(|e| e.to_string())?;
        let mut version = 0_u64;
        while !stop_writer.load(Ordering::Acquire) {
            version += 1;
            let transaction = connection.transaction().map_err(|e| e.to_string())?;
            transaction
                .execute(
                    "UPDATE threads SET title = ?1, preview = ?1 WHERE id = ?2",
                    params![version.to_string(), THREAD_ID],
                )
                .map_err(|e| e.to_string())?;
            transaction.commit().map_err(|e| e.to_string())?;
            started_writer.store(true, Ordering::Release);
        }
        Ok(())
    });
    let deadline = SystemTime::now() + Duration::from_secs(10);
    while !started.load(Ordering::Acquire) {
        if SystemTime::now() >= deadline {
            stop.store(true, Ordering::Release);
            return Err("timed out waiting for SQLite writer".into());
        }
        thread::yield_now();
    }
    let source_names_before = directory_entry_names(&fixture.codex_home)?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("concurrent-sqlite.rehome");

    let result = create_package(package_request(&fixture, package.clone()));
    let source_names_after = directory_entry_names(&fixture.codex_home)?;
    stop.store(true, Ordering::Release);
    writer
        .join()
        .map_err(|_| "SQLite writer panicked")?
        .map_err(|error| format!("SQLite writer failed: {error}"))?;
    result?;

    assert_eq!(source_names_after, source_names_before);
    let rows: Value =
        serde_json::from_slice(&read_zip_entry(&package, "codex/metadata/threads.json")?)?;
    assert_eq!(rows[0]["title"], rows[0]["preview"]);
    Ok(())
}

#[test]
fn thread_export_uses_a_versioned_allowlist_and_tolerates_missing_optional_columns(
) -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::remove_file(&fixture.state_db_path)?;
    let connection = Connection::open(&fixture.state_db_path)?;
    connection.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, future_secret TEXT NOT NULL)",
        [],
    )?;
    connection.execute(
        "INSERT INTO threads VALUES (?1, 'must-not-leave-source')",
        [THREAD_ID],
    )?;
    drop(connection);
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("allowlisted-threads.rehome");

    let report = create_package(package_request(&fixture, package.clone()))?;

    assert_eq!(report.counts.sqlite_threads, 1);
    let rows: Value =
        serde_json::from_slice(&read_zip_entry(&package, "codex/metadata/threads.json")?)?;
    assert_eq!(rows[0]["id"], THREAD_ID);
    assert!(rows[0].get("future_secret").is_none());
    assert_eq!(rows[0].as_object().expect("thread object").len(), 1);
    Ok(())
}

#[test]
fn thread_export_includes_portable_fields_required_by_current_codex() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let connection = Connection::open(&fixture.state_db_path)?;
    connection.execute_batch(
        "ALTER TABLE threads ADD COLUMN created_at INTEGER;
         ALTER TABLE threads ADD COLUMN source TEXT;
         ALTER TABLE threads ADD COLUMN model_provider TEXT;
         ALTER TABLE threads ADD COLUMN sandbox_policy TEXT;
         ALTER TABLE threads ADD COLUMN approval_mode TEXT;",
    )?;
    connection.execute(
        "UPDATE threads
         SET created_at = ?1,
             source = ?2,
             model_provider = ?3,
             sandbox_policy = ?4,
             approval_mode = ?5
         WHERE id = ?6",
        params![
            1_780_000_000_i64,
            "vscode",
            "openai",
            r#"{"type":"disabled"}"#,
            "never",
            THREAD_ID,
        ],
    )?;
    drop(connection);
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("portable-thread-fields.rehome");

    create_package(package_request(&fixture, package.clone()))?;

    let rows: Value =
        serde_json::from_slice(&read_zip_entry(&package, "codex/metadata/threads.json")?)?;
    assert_eq!(rows[0]["created_at"], 1_780_000_000_i64);
    assert_eq!(rows[0]["source"], "vscode");
    assert_eq!(rows[0]["model_provider"], "openai");
    assert_eq!(rows[0]["sandbox_policy"], r#"{"type":"disabled"}"#);
    assert_eq!(rows[0]["approval_mode"], "never");
    Ok(())
}

#[test]
fn rejects_corrupt_zip_bytes_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("corrupt.rehome");
    fs::write(&package, b"this is not a zip archive")?;

    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_corrupted_payload_bytes_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("payload-corrupt.rehome");
    let manifest = serde_json::to_vec(&test_manifest(1))?;
    let payload = b"ORIGINAL-STORED-PAYLOAD";
    let checksums = format!("{}  codex/sessions/thread.jsonl\n", checksum(payload));
    write_test_zip(
        &package,
        &[
            ("checksums.sha256", checksums.as_bytes()),
            ("codex/sessions/thread.jsonl", payload),
            ("manifest.json", &manifest),
        ],
    )?;
    assert_eq!(
        replace_all(&package, payload, b"CORRUPT!-STORED-PAYLOAD")?,
        1
    );

    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_unsafe_and_duplicate_zip_entry_names() -> Result<(), Box<dyn Error>> {
    let cases: &[&[(&str, &[u8])]] = &[
        &[("../escape", b"payload")],
        &[("/absolute", b"payload")],
        &[("C:/absolute", b"payload")],
        &[("folder\\file", b"payload")],
    ];

    for (index, entries) in cases.iter().enumerate() {
        let directory = tempfile::tempdir()?;
        let package = directory.path().join(format!("unsafe-{index}.rehome"));
        write_test_zip(&package, entries)?;
        assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    }

    let directory = tempfile::tempdir()?;
    let package = directory.path().join("duplicate.rehome");
    write_test_zip(
        &package,
        &[("duplicate-a", b"one"), ("duplicate-b", b"two")],
    )?;
    assert!(replace_all(&package, b"duplicate-b", b"duplicate-a")? >= 2);
    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_unicode_and_file_descendant_portable_path_collisions() -> Result<(), Box<dyn Error>> {
    let collision_sets: &[&[(&str, &[u8])]] = &[
        &[
            ("codex/sessions/Thread.jsonl", b"upper"),
            ("codex/sessions/thread.jsonl", b"lower"),
        ],
        &[
            ("codex/sessions/caf\u{e9}.jsonl", b"composed"),
            ("codex/sessions/cafe\u{301}.jsonl", b"decomposed"),
        ],
        &[
            ("codex/sessions/thread", b"file"),
            ("codex/sessions/thread/child", b"descendant"),
        ],
    ];

    for (index, payloads) in collision_sets.iter().enumerate() {
        let directory = tempfile::tempdir()?;
        let package = directory.path().join(format!("collision-{index}.rehome"));
        write_valid_test_package(&package, &test_manifest(1), payloads)?;

        assert_error_message_contains(inspect_package(&package), "collision");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[test]
fn creation_rejects_unicode_portable_path_collisions() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(fixture.project_path.join("caf\u{e9}.txt"), b"composed")?;
    fs::write(fixture.project_path.join("cafe\u{301}.txt"), b"decomposed")?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("creation-collision.rehome");

    assert_error_message_contains(
        create_package(package_request(&fixture, package)),
        "collision",
    );
    Ok(())
}

#[test]
fn rejects_invalid_or_missing_manifest_payload_references() -> Result<(), Box<dyn Error>> {
    for (index, archive_path) in [
        "../escape.jsonl",
        "/absolute.jsonl",
        "codex\\sessions\\thread.jsonl",
        "projects/not-a-conversation.jsonl",
        "codex/sessions/missing.jsonl",
    ]
    .into_iter()
    .enumerate()
    {
        let directory = tempfile::tempdir()?;
        let package = directory
            .path()
            .join(format!("manifest-path-{index}.rehome"));
        let mut manifest = test_manifest(1);
        manifest.conversations.push(ConversationEntry {
            task_id: Uuid::new_v4(),
            project_id: None,
            title: "Synthetic".into(),
            updated_at: "2026-07-22T00:00:00Z".into(),
            content_hash: checksum(b"payload"),
            archive_path: archive_path.into(),
            classification: None,
        });
        write_valid_test_package(
            &package,
            &manifest,
            &[("codex/sessions/thread.jsonl", b"payload")],
        )?;

        assert_error_message_contains(inspect_package(&package), "manifest");
    }
    Ok(())
}

#[test]
fn inspection_rejects_oversized_controls_and_entry_counts() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;

    let oversized_control = directory.path().join("oversized-control.rehome");
    write_test_zip(
        &oversized_control,
        &[("manifest.json", &vec![b' '; 4 * 1024 * 1024 + 1])],
    )?;
    assert_error_message_contains(inspect_package(&oversized_control), "control file size");

    let excessive_count = directory.path().join("excessive-count.rehome");
    write_many_directories_zip(&excessive_count, 10_001)?;
    assert_error_message_contains(inspect_package(&excessive_count), "entry count");
    Ok(())
}

#[test]
fn rejects_missing_manifest_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("missing-manifest.rehome");
    write_test_zip(&package, &[("checksums.sha256", b"")])?;

    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_unsupported_schema_before_checksum_validation() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("future.rehome");
    let manifest = serde_json::to_vec(&test_manifest(99))?;
    write_test_zip(&package, &[("manifest.json", &manifest)])?;

    assert_error_code(inspect_package(&package), ErrorCode::UnsupportedSchema);
    Ok(())
}

#[test]
fn rejects_checksum_mismatch_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("mismatch.rehome");
    let manifest = serde_json::to_vec(&test_manifest(1))?;
    let checksums = format!("{}  codex/sessions/thread.jsonl\n", "0".repeat(64));
    write_test_zip(
        &package,
        &[
            ("checksums.sha256", checksums.as_bytes()),
            ("codex/sessions/thread.jsonl", b"selected payload"),
            ("manifest.json", &manifest),
        ],
    )?;

    assert_error_code(inspect_package(&package), ErrorCode::ChecksumMismatch);
    Ok(())
}

#[test]
fn writer_uses_portable_deterministic_zip_metadata_and_checksum_text() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("portable.rehome");
    create_package(package_request(&fixture, package.clone()))?;

    let file = fs::File::open(&package)?;
    let mut archive = ZipArchive::new(file)?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        names.push(entry.name().to_owned());
        assert_eq!(entry.last_modified(), Some(DateTime::default()));
        let mode = entry.unix_mode().expect("portable Unix mode") & 0o777;
        assert_eq!(mode, if entry.is_dir() { 0o755 } else { 0o644 });
    }
    assert_eq!(names, sorted(names.clone()));

    let mut checksums = String::new();
    archive
        .by_name("checksums.sha256")?
        .read_to_string(&mut checksums)?;
    assert!(!checksums.starts_with('\u{feff}'));
    assert!(!checksums.contains('\r'));
    assert!(checksums.ends_with('\n'));
    let checksum_paths: Vec<&str> = checksums
        .lines()
        .map(|line| line.split_once("  ").expect("checksum line").1)
        .collect();
    assert_eq!(checksum_paths, sorted_strs(checksum_paths.clone()));

    let payload_paths: Vec<&str> = names
        .iter()
        .filter(|name| !name.ends_with('/'))
        .map(String::as_str)
        .filter(|name| !matches!(*name, "checksums.sha256" | "manifest.json"))
        .collect();
    assert_eq!(checksum_paths, payload_paths);
    Ok(())
}

#[test]
fn create_package_never_clobbers_an_existing_output() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("existing.rehome");
    fs::write(&package, b"keep me")?;

    assert_error_code(
        create_package(package_request(&fixture, package.clone())),
        ErrorCode::PackageInvalid,
    );
    assert_eq!(fs::read(package)?, b"keep me");
    Ok(())
}

#[test]
fn desktop_confirmed_replace_publishes_a_complete_package() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("existing.rehome");
    fs::write(&package, b"old package")?;

    let report = create_package_replacing(package_request(&fixture, package.clone()))?;

    assert_eq!(report.package_path, package);
    assert_ne!(fs::read(&package)?, b"old package");
    assert!(inspect_package(&package)?.checksum_valid);
    Ok(())
}

#[test]
fn published_package_is_complete_when_the_target_first_appears() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let large_source = fixture.project_path.join("large.bin");
    fs::File::create(&large_source)?.set_len(64 * 1024 * 1024)?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("published.rehome");
    let package_for_observer = package.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_observer = Arc::clone(&stop);
    let observer = thread::spawn(move || -> Result<bool, String> {
        while !stop_observer.load(Ordering::Acquire) {
            if package_for_observer.exists() {
                inspect_package(&package_for_observer).map_err(|error| error.to_string())?;
                return Ok(true);
            }
            thread::yield_now();
        }
        Ok(false)
    });

    let result = create_package(package_request(&fixture, package.clone()));
    if result.is_err() {
        stop.store(true, Ordering::Release);
    }
    let observed = observer.join().map_err(|_| "observer panicked")??;
    result?;
    assert!(observed);
    assert_eq!(fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}

#[test]
fn sensitive_staging_never_appears_in_project_or_output_directories() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    fs::File::create(fixture.project_path.join("large.bin"))?.set_len(128 * 1024 * 1024)?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("private-stage.rehome");
    let watched_roots = [fixture.project_path.clone(), directory.path().to_path_buf()];
    let stop = Arc::new(AtomicBool::new(false));
    let stop_observer = Arc::clone(&stop);
    let observer = thread::spawn(move || {
        let mut found = false;
        while !stop_observer.load(Ordering::Acquire) {
            found |= watched_roots.iter().any(|root| {
                WalkDir::new(root)
                    .into_iter()
                    .filter_map(Result::ok)
                    .any(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".rehome-stage-")
                    })
            });
            if found {
                break;
            }
            thread::yield_now();
        }
        found
    });

    create_package(package_request(&fixture, package))?;
    stop.store(true, Ordering::Release);
    assert!(!observer.join().map_err(|_| "observer panicked")?);
    Ok(())
}

#[test]
fn package_synthesizes_a_missing_selected_session_index_row() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::remove_file(&fixture.session_index_path)?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("synthesized-index.rehome");

    create_package(package_request(&fixture, package.clone()))?;

    let index = String::from_utf8(read_zip_entry(&package, "codex/session_index.jsonl")?)?;
    let rows = index
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], THREAD_ID);
    assert_eq!(rows[0]["thread_name"], "Imported conversation");
    assert_eq!(rows[0]["title"], "Imported conversation");
    Ok(())
}

#[test]
fn symbolic_links_in_selected_projects_are_safely_excluded() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let outside = fixture.root.join("outside-secret.txt");
    fs::write(&outside, b"must not enter the package\n")?;
    let linked = fixture.project_path.join("linked-secret.txt");
    if let Err(error) = create_file_symlink(&outside, &linked) {
        if windows_symlink_privilege_is_unavailable(&error) {
            eprintln!("skipping project symlink test: Windows symlink privilege unavailable");
            return Ok(());
        }
        return Err(error.into());
    }
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("symlink-safe.rehome");

    create_package(package_request(&fixture, package.clone()))?;

    let preview = inspect_package(&package)?;
    assert_eq!(preview.manifest.counts.projects, 1);
    assert_eq!(preview.manifest.counts.project_files, 1);
    assert_eq!(preview.manifest.exclusions.excluded_files, 4);
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("/linked-secret.txt")));
    Ok(())
}

#[test]
fn aborts_if_a_source_changes_while_it_is_copied() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let large_source = fixture.project_path.join("large.bin");
    let large_file = fs::File::create(&large_source)?;
    large_file.set_len(64 * 1024 * 1024)?;
    drop(large_file);

    let directory = tempfile::tempdir()?;
    let package = directory.path().join("racing.rehome");
    let source_for_thread = large_source.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_mutator = Arc::clone(&stop);
    let mutator = thread::spawn(move || -> Result<(), String> {
        while !stop_mutator.load(Ordering::Acquire) {
            OpenOptions::new()
                .append(true)
                .open(&source_for_thread)
                .and_then(|mut file| file.write_all(b"changed"))
                .map_err(|error| error.to_string())?;
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    });

    let result = create_package(package_request(&fixture, package.clone()));
    stop.store(true, Ordering::Release);
    assert_error_code(result, ErrorCode::PackageInvalid);
    mutator
        .join()
        .map_err(|_| "source mutator panicked")?
        .map_err(|error| format!("source mutator failed: {error}"))?;
    assert!(!package.exists());
    assert!(fs::read_dir(directory.path())?.next().is_none());
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    bytes: Vec<u8>,
    length: u64,
    modified: Option<SystemTime>,
    readonly: bool,
}

fn snapshot_files(root: &Path) -> Result<BTreeMap<PathBuf, FileSnapshot>, Box<dyn Error>> {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            let relative = entry.path().strip_prefix(root)?.to_path_buf();
            let metadata = entry.metadata()?;
            Ok((
                relative,
                FileSnapshot {
                    bytes: fs::read(entry.path())?,
                    length: metadata.len(),
                    modified: metadata.modified().ok(),
                    readonly: metadata.permissions().readonly(),
                },
            ))
        })
        .collect()
}

fn directory_entry_names(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = fs::read_dir(root)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    names.sort();
    Ok(names)
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(not(windows))]
fn windows_symlink_privilege_is_unavailable(_error: &io::Error) -> bool {
    false
}

#[cfg(windows)]
fn windows_symlink_privilege_is_unavailable(error: &io::Error) -> bool {
    error.raw_os_error() == Some(1314)
}

fn read_zip_entry(path: &Path, name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut archive = ZipArchive::new(fs::File::open(path)?)?;
    let mut bytes = Vec::new();
    archive.by_name(name)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn sorted(mut entries: Vec<String>) -> Vec<String> {
    entries.sort();
    entries
}

fn sorted_strs(mut entries: Vec<&str>) -> Vec<&str> {
    entries.sort();
    entries
}

fn package_request(
    fixture: &common::SyntheticCodexFixture,
    output_path: PathBuf,
) -> CreatePackageRequest {
    CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID).unwrap()],
        output_path,
        source_device_id: Uuid::nil(),
        skill_paths: vec![fixture.skill_path.clone()],
        plugin_paths: vec![fixture.plugin_manifest_path.clone()],
        generated_image_paths: vec![fixture.generated_image_path.clone()],
    }
}

fn test_manifest(schema_version: u32) -> PackageManifest {
    PackageManifest {
        format: "codex-rehome".into(),
        schema_version,
        package_id: Uuid::nil(),
        created_at: "2026-07-22T00:00:00Z".into(),
        source_os: SourceOs::Windows,
        source_arch: "x86_64".into(),
        source_device_id: Uuid::nil(),
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
        exclusions: ExclusionSummary::default(),
    }
}

fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) -> Result<(), Box<dyn Error>> {
    let file = fs::File::create(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    writer.finish()?;
    Ok(())
}

fn write_valid_test_package(
    path: &Path,
    manifest: &PackageManifest,
    payloads: &[(&str, &[u8])],
) -> Result<(), Box<dyn Error>> {
    let manifest = serde_json::to_vec(manifest)?;
    let mut checksum_text = String::new();
    for (name, bytes) in payloads {
        checksum_text.push_str(&format!("{}  {name}\n", checksum(bytes)));
    }
    let mut entries: Vec<(&str, &[u8])> = payloads.to_vec();
    entries.push(("checksums.sha256", checksum_text.as_bytes()));
    entries.push(("manifest.json", &manifest));
    write_test_zip(path, &entries)
}

fn write_many_directories_zip(path: &Path, count: usize) -> Result<(), Box<dyn Error>> {
    let file = fs::File::create(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o755);
    for index in 0..count {
        writer.add_directory(format!("entries/{index:05}/"), options)?;
    }
    writer.finish()?;
    Ok(())
}

fn replace_all(path: &Path, from: &[u8], to: &[u8]) -> Result<usize, Box<dyn Error>> {
    assert_eq!(from.len(), to.len());
    let mut bytes = fs::read(path)?;
    let mut replacements = 0;
    for offset in 0..=bytes.len() - from.len() {
        if &bytes[offset..offset + from.len()] == from {
            bytes[offset..offset + to.len()].copy_from_slice(to);
            replacements += 1;
        }
    }
    fs::write(path, bytes)?;
    Ok(replacements)
}

fn assert_error_code<T: std::fmt::Debug>(
    result: Result<T, rehome_desktop_lib::core::error::RehomeError>,
    code: ErrorCode,
) {
    assert_eq!(result.expect_err("operation must fail").code, code);
}

fn assert_error_message_contains<T: std::fmt::Debug>(
    result: Result<T, rehome_desktop_lib::core::error::RehomeError>,
    expected: &str,
) {
    let error = result.expect_err("operation must fail");
    assert_eq!(error.code, ErrorCode::PackageInvalid);
    assert!(
        error.message.contains(expected),
        "expected error containing {expected:?}, got {:?}",
        error.message
    );
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}
