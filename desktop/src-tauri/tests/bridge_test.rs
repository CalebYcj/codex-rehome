#[allow(dead_code)]
mod common;

use common::{synthetic_codex_fixture, THREAD_ID};
use rehome_desktop_lib::core::{
    bridge::{
        apply_bridge_plan, import_sqlite_threads, merge_session_index, register_project,
        rewrite_session_jsonl, CommandRunError, CommandRunner,
    },
    models::{
        ContentCounts, CreatePackageRequest, PlannedSession, ReferenceRewrite,
        ReferenceRewriteKind, RegistrationStatus, SessionAction, SourceOs, TargetInventory,
    },
    package::{create_package, inspect_package},
    planner::build_restore_plan,
};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    cell::RefCell,
    error::Error,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};
use uuid::Uuid;

const SOURCE_ID: &str = "11111111-1111-4111-8111-111111111111";
const TARGET_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const SOURCE: &str = "codex/sessions/2026/07/22/thread.jsonl";
const INDEX_SOURCE: &str = "codex/session_index.jsonl";
const WINDOWS_PROJECT: &str = r"C:\Users\OldUser\Documents\visual";
const WINDOWS_PROJECT_SLASHED: &str = "C:/Users/OldUser/Documents/visual";
const MAC_PROJECT: &str = "/Users/test/Documents/Codex-Restored-Projects/visual";
const WINDOWS_SESSION: &str = r"C:\Users\OldUser\.codex\sessions\2026\07\22\thread.jsonl";
const MAC_SESSION: &str = "/Users/test/.codex/sessions/2026/07/22/branch.jsonl";

#[test]
fn rewrites_only_approved_recursive_session_metadata_and_never_messages(
) -> Result<(), Box<dyn Error>> {
    let input = jsonl(&[
        serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": SOURCE_ID,
                "title": "Original",
                "cwd": WINDOWS_PROJECT,
                "metadata": {
                    "project_path": WINDOWS_PROJECT_SLASHED,
                    "rollout_path": WINDOWS_SESSION,
                    "contexts": [{"cwd": WINDOWS_PROJECT}],
                },
                "message": format!("Keep {WINDOWS_PROJECT}"),
            }
        }),
        serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": format!("Keep {WINDOWS_PROJECT} and {SOURCE_ID}"),
                }]
            }
        }),
    ]);

    let rewritten = rewrite_session_jsonl(input.as_bytes(), &rewrites(SOURCE), SOURCE)?;
    let text = String::from_utf8(rewritten)?;
    let rows = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(rows[0]["payload"]["id"], TARGET_ID);
    assert_eq!(rows[0]["payload"]["title"], "Original 路 ReHome");
    assert_eq!(rows[0]["payload"]["cwd"], MAC_PROJECT);
    assert_eq!(rows[0]["payload"]["metadata"]["project_path"], MAC_PROJECT);
    assert_eq!(rows[0]["payload"]["metadata"]["rollout_path"], MAC_SESSION);
    assert_eq!(
        rows[0]["payload"]["metadata"]["contexts"][0]["cwd"],
        MAC_PROJECT
    );
    assert_eq!(
        rows[0]["payload"]["message"],
        format!("Keep {WINDOWS_PROJECT}")
    );
    assert_eq!(
        rows[1]["payload"]["content"][0]["text"],
        format!("Keep {WINDOWS_PROJECT} and {SOURCE_ID}")
    );
    assert!(!text.contains(WINDOWS_PROJECT_SLASHED));
    assert!(text.contains(MAC_PROJECT));
    Ok(())
}

#[test]
fn session_index_merge_preserves_target_rows_and_repairs_planned_metadata(
) -> Result<(), Box<dyn Error>> {
    let preserved_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    let existing = jsonl(&[
        serde_json::json!({
            "id": preserved_id,
            "title": "Target only",
            "favorite": true,
        }),
        serde_json::json!({
            "id": TARGET_ID,
            "title": "Stale",
            "target_only": "keep",
        }),
        serde_json::json!({
            "id": TARGET_ID,
            "title": "Duplicate",
            "target_only": "keep",
        }),
    ]);
    let package = jsonl(&[serde_json::json!({
        "id": SOURCE_ID,
        "title": "Original",
        "cwd": WINDOWS_PROJECT,
        "rollout_path": WINDOWS_SESSION,
        "updated_at": "2026-07-22T00:00:00Z",
    })]);

    let merged = merge_session_index(
        existing.as_bytes(),
        package.as_bytes(),
        &[planned_session()],
        &rewrites(INDEX_SOURCE),
    )?;
    let merged_again = merge_session_index(
        &merged,
        package.as_bytes(),
        &[planned_session()],
        &rewrites(INDEX_SOURCE),
    )?;
    let rows = merged
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(merged_again, merged);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().filter(|row| row["id"] == TARGET_ID).count(), 1);
    assert!(rows
        .iter()
        .any(|row| row["id"] == preserved_id && row["favorite"] == true));
    let imported = rows.iter().find(|row| row["id"] == TARGET_ID).unwrap();
    assert_eq!(imported["title"], "Original 路 ReHome");
    assert_eq!(imported["cwd"], MAC_PROJECT);
    assert_eq!(imported["rollout_path"], MAC_SESSION);
    assert_eq!(imported["target_only"], "keep");
    Ok(())
}

#[test]
fn session_index_merge_repairs_an_old_package_with_a_missing_planned_row(
) -> Result<(), Box<dyn Error>> {
    let unrelated = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    let package = jsonl(&[serde_json::json!({
        "id": unrelated,
        "title": "Unrelated",
    })]);

    let merged = merge_session_index(
        b"",
        package.as_bytes(),
        &[planned_session()],
        &rewrites(INDEX_SOURCE),
    )?;
    let rows = merged
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], TARGET_ID);
    assert_eq!(rows[0]["title"], planned_session().title);
    assert_eq!(rows[0]["rollout_path"], MAC_SESSION);
    Ok(())
}

#[test]
fn session_index_merge_preserves_newer_target_metadata_and_unrelated_duplicate_rows_exactly(
) -> Result<(), Box<dyn Error>> {
    let unrelated = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    let first_unrelated = format!("{{ \"id\": \"{unrelated}\", \"title\": \"first\" }}\r\n");
    let second_unrelated = format!("{{\"id\":\"{unrelated}\",\"title\":\"second\"}}\n");
    let target = format!(
        "{first_unrelated}{}\n{second_unrelated}",
        serde_json::json!({
            "id": TARGET_ID,
            "title": "New target title",
            "preview": "new target preview",
            "cwd": r"C:\stale\target",
            "rollout_path": r"C:\stale\thread.jsonl",
            "updated_at": "2026-07-23T00:00:00Z",
            "target_only": true,
        })
    );
    let package = jsonl(&[serde_json::json!({
        "id": SOURCE_ID,
        "title": "Older incoming title",
        "preview": "older incoming preview",
        "cwd": WINDOWS_PROJECT,
        "rollout_path": WINDOWS_SESSION,
        "updated_at": "2026-07-22T00:00:00Z",
    })]);

    let merged = merge_session_index(
        target.as_bytes(),
        package.as_bytes(),
        &[planned_session()],
        &rewrites(INDEX_SOURCE),
    )?;
    let merged_text = String::from_utf8(merged.clone())?;
    let imported = merged
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .map(serde_json::from_slice::<Value>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|row| row["id"] == TARGET_ID)
        .unwrap();

    assert!(merged_text.contains(&first_unrelated));
    assert!(merged_text.contains(&second_unrelated));
    assert_eq!(imported["title"], "New target title");
    assert_eq!(imported["preview"], "new target preview");
    assert_eq!(imported["updated_at"], "2026-07-23T00:00:00Z");
    assert_eq!(imported["cwd"], MAC_PROJECT);
    assert_eq!(imported["rollout_path"], MAC_SESSION);
    assert_eq!(imported["target_only"], true);
    Ok(())
}

#[test]
fn sqlite_import_uses_existing_allowlisted_columns_and_preserves_target_only_values(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let memory = temp_root.join("memory.sqlite");
    let goals = temp_root.join("goals.sqlite");
    std::fs::write(&memory, b"memory untouched")?;
    std::fs::write(&goals, b"goals untouched")?;
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT,
            rollout_path TEXT,
            title TEXT,
            updated_at TEXT,
            target_only TEXT NOT NULL DEFAULT 'default'
        );",
    )?;
    connection.execute(
        "INSERT INTO threads (id, cwd, rollout_path, title, target_only)
         VALUES (?1, 'stale', 'stale', 'stale', 'preserve me')",
        [TARGET_ID],
    )?;
    drop(connection);
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "cwd": WINDOWS_PROJECT,
        "updated_at": "2026-07-22T00:00:00Z",
        "archived": 0,
    }]))?;

    let imported = import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )?;

    assert_eq!(imported, 1);
    let connection = Connection::open(&database)?;
    let row = connection.query_row(
        "SELECT cwd, rollout_path, title, updated_at, target_only FROM threads WHERE id = ?1",
        [TARGET_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    )?;
    assert_eq!(
        row,
        (
            MAC_PROJECT.into(),
            MAC_SESSION.into(),
            "Original 路 ReHome".into(),
            "2026-07-22T00:00:00Z".into(),
            "preserve me".into(),
        )
    );
    assert_eq!(std::fs::read(&memory)?, b"memory untouched");
    assert_eq!(std::fs::read(&goals)?, b"goals untouched");
    Ok(())
}

#[test]
fn sqlite_existing_row_update_preserves_required_target_only_column_without_default(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            cwd TEXT,
            rollout_path TEXT,
            target_only TEXT NOT NULL
        );",
    )?;
    connection.execute(
        "INSERT INTO threads (id, title, target_only) VALUES (?1, 'stale', 'preserve me')",
        [TARGET_ID],
    )?;
    drop(connection);
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "title": "incoming",
        "cwd": WINDOWS_PROJECT,
    }]))?;

    import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )?;

    let connection = Connection::open(&database)?;
    let row = connection.query_row(
        "SELECT title, cwd, rollout_path, target_only FROM threads WHERE id = ?1",
        [TARGET_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;
    assert_eq!(
        row,
        (
            "Original 路 ReHome".into(),
            MAC_PROJECT.into(),
            MAC_SESSION.into(),
            "preserve me".into(),
        )
    );
    Ok(())
}

#[test]
fn sqlite_missing_row_imports_portable_fields_required_by_current_codex(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT NOT NULL,
            rollout_path TEXT NOT NULL,
            title TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL
        );",
    )?;
    drop(connection);
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "cwd": WINDOWS_PROJECT,
        "rollout_path": WINDOWS_SESSION,
        "title": "incoming",
        "created_at": 1_780_000_000_i64,
        "updated_at": 1_780_000_100_i64,
        "source": "vscode",
        "model_provider": "openai",
        "sandbox_policy": r#"{"type":"disabled"}"#,
        "approval_mode": "never",
    }]))?;

    let imported = import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )?;

    assert_eq!(imported, 1);
    let connection = Connection::open(&database)?;
    let row = connection.query_row(
        "SELECT cwd, rollout_path, title, created_at, updated_at, source,
                model_provider, sandbox_policy, approval_mode
         FROM threads WHERE id = ?1",
        [TARGET_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        },
    )?;
    assert_eq!(
        row,
        (
            MAC_PROJECT.into(),
            MAC_SESSION.into(),
            "Original 路 ReHome".into(),
            1_780_000_000_i64,
            1_780_000_100_i64,
            "vscode".into(),
            "openai".into(),
            r#"{"type":"disabled"}"#.into(),
            "never".into(),
        )
    );
    Ok(())
}

#[test]
fn sqlite_missing_row_required_target_only_column_fails_without_changing_database(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            cwd TEXT,
            rollout_path TEXT,
            target_only TEXT NOT NULL
        );",
    )?;
    drop(connection);
    let before = std::fs::read(&database)?;
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "title": "incoming",
    }]))?;

    let error = import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )
    .unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert!(error.message.contains("target_only"));
    assert_eq!(std::fs::read(&database)?, before);
    Ok(())
}

#[test]
fn sqlite_import_rejects_hard_linked_database_without_touching_either_name(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let alias = temp_root.join("state_alias.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, rollout_path TEXT);",
    )?;
    drop(connection);
    std::fs::hard_link(&database, &alias)?;
    let lock = temp_root.join(".state_5.sqlite.codex-rehome.lock");
    std::fs::write(&lock, b"another restore")?;
    let before = std::fs::read(&database)?;
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "title": "incoming",
    }]))?;

    let error = import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )
    .unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert!(error.message.contains("hard link"));
    assert_eq!(std::fs::read(&database)?, before);
    assert_eq!(std::fs::read(&alias)?, before);
    assert_eq!(std::fs::read(&lock)?, b"another restore");
    Ok(())
}

#[test]
fn sqlite_import_honors_the_database_cas_lock_without_changing_rows() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, rollout_path TEXT);",
    )?;
    drop(connection);
    let lock = temp_root.join(".state_5.sqlite.codex-rehome.lock");
    std::fs::write(&lock, b"another restore")?;
    let before = std::fs::read(&database)?;
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "title": "incoming",
    }]))?;

    let error = import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )
    .unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert!(error.message.contains("locked"));
    assert_eq!(std::fs::read(&database)?, before);
    assert_eq!(std::fs::read(&lock)?, b"another restore");
    Ok(())
}

#[test]
fn sqlite_import_updates_a_live_wal_database_in_place_and_survives_reopen(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA wal_autocheckpoint = 0;
         CREATE TABLE threads (
             id TEXT PRIMARY KEY,
             title TEXT,
             cwd TEXT,
             rollout_path TEXT,
             target_only TEXT NOT NULL DEFAULT 'default'
         );
         PRAGMA wal_checkpoint(TRUNCATE);",
    )?;
    let identity = file_identity(&database)?;
    connection.execute(
        "INSERT INTO threads (id, title, target_only) VALUES (?1, 'wal title', 'keep me')",
        [TARGET_ID],
    )?;
    assert!(sqlite_sidecar(&database, "-wal").exists());
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "title": "incoming",
        "cwd": WINDOWS_PROJECT,
    }]))?;

    import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )?;

    assert_eq!(file_identity(&database)?, identity);
    let live_title: String = connection.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [TARGET_ID],
        |row| row.get(0),
    )?;
    assert_eq!(live_title, planned_session().title);
    drop(connection);

    let reopened = Connection::open(&database)?;
    let row = reopened.query_row(
        "SELECT title, cwd, rollout_path, target_only FROM threads WHERE id = ?1",
        [TARGET_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;
    assert_eq!(
        row,
        (
            planned_session().title,
            MAC_PROJECT.into(),
            MAC_SESSION.into(),
            "keep me".into(),
        )
    );
    assert_eq!(file_identity(&database)?, identity);
    Ok(())
}

#[test]
fn sqlite_import_merges_with_a_pinned_stale_wal_and_survives_reopen() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let writer = Connection::open(&database)?;
    writer.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA wal_autocheckpoint = 0;
         CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, rollout_path TEXT);
         PRAGMA wal_checkpoint(TRUNCATE);",
    )?;
    let identity = file_identity(&database)?;
    let reader = Connection::open(&database)?;
    reader.execute_batch("BEGIN")?;
    let _: i64 = reader.query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))?;
    let wal_only_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    writer.execute(
        "INSERT INTO threads (id, title) VALUES (?1, 'stale wal row')",
        [wal_only_id],
    )?;
    drop(writer);
    assert!(sqlite_sidecar(&database, "-wal").exists());
    let metadata = serde_json::to_vec(&serde_json::json!([{
        "id": SOURCE_ID,
        "title": "incoming",
    }]))?;

    import_sqlite_threads(
        &database,
        &metadata,
        &[planned_session()],
        &rewrites("codex/metadata/threads.json"),
    )?;

    assert_eq!(file_identity(&database)?, identity);
    reader.execute_batch("ROLLBACK")?;
    drop(reader);
    let reopened = Connection::open(&database)?;
    let imported_title: String = reopened.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [TARGET_ID],
        |row| row.get(0),
    )?;
    let stale_wal_title: String = reopened.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [wal_only_id],
        |row| row.get(0),
    )?;
    assert_eq!(imported_title, planned_session().title);
    assert_eq!(stale_wal_title, "stale wal row");
    assert_eq!(file_identity(&database)?, identity);
    Ok(())
}

#[test]
fn sqlite_import_rolls_back_every_row_when_a_later_row_fails() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let temp_root = fs::canonicalize(temp.path())?;
    let database = temp_root.join("state_5.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL CHECK (title <> 'reject'),
            cwd TEXT,
            rollout_path TEXT
        );",
    )?;
    let first_id = Uuid::parse_str("33333333-3333-4333-8333-333333333333")?;
    connection.execute(
        "INSERT INTO threads (id, title) VALUES (?1, 'original')",
        [first_id.to_string()],
    )?;
    drop(connection);
    let second_id = Uuid::parse_str("44444444-4444-4444-8444-444444444444")?;
    let sessions = vec![
        simple_session(first_id, "accepted"),
        simple_session(second_id, "reject"),
    ];
    let metadata = serde_json::to_vec(&serde_json::json!([
        {"id": first_id.to_string(), "title": "accepted"},
        {"id": second_id.to_string(), "title": "reject"},
    ]))?;

    let error = import_sqlite_threads(&database, &metadata, &sessions, &[]).unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    let connection = Connection::open(&database)?;
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))?;
    let title: String = connection.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [first_id.to_string()],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    assert_eq!(title, "original");
    Ok(())
}

#[test]
fn project_registration_reports_every_outcome_without_launching_codex() -> Result<(), Box<dyn Error>>
{
    let cli = Path::new("/Applications/Codex.app/Contents/Resources/codex");
    let project = Path::new(MAC_PROJECT);
    let success = FakeRunner::succeeds();

    assert_eq!(
        register_project(SourceOs::Macos, Some(cli), project, &success),
        RegistrationStatus::Registered
    );
    assert_eq!(
        success.calls.borrow().as_slice(),
        &[(
            cli.to_path_buf(),
            vec![OsString::from("app"), project.as_os_str().to_owned()]
        )]
    );
    assert_eq!(
        register_project(SourceOs::Macos, None, project, &success),
        RegistrationStatus::CommandUnavailable
    );
    assert_eq!(
        register_project(SourceOs::Windows, None, project, &success),
        RegistrationStatus::ManualOpenRequired
    );

    let unavailable = FakeRunner::fails(CommandRunError::Unavailable);
    assert_eq!(
        register_project(
            SourceOs::Windows,
            Some(Path::new("codex.exe")),
            project,
            &unavailable
        ),
        RegistrationStatus::CommandUnavailable
    );
    let failed = FakeRunner::fails(CommandRunError::InvocationFailed {
        message: "exit code 7".into(),
    });
    assert_eq!(
        register_project(SourceOs::Macos, Some(cli), project, &failed),
        RegistrationStatus::InvocationFailed {
            message: "exit code 7".into()
        }
    );
    Ok(())
}

#[test]
fn bridge_applies_task_six_session_index_and_sqlite_plan() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let source_project = align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("handoff.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("restored-projects");
    std::fs::create_dir_all(&codex_home)?;
    std::fs::write(
        codex_home.join("session_index.jsonl"),
        b"{\"id\":\"99999999-9999-4999-8999-999999999999\",\"title\":\"Target\"}\n",
    )?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT,
            rollout_path TEXT,
            title TEXT,
            updated_at TEXT,
            archived INTEGER,
            has_user_event INTEGER,
            preview TEXT,
            target_only TEXT NOT NULL DEFAULT 'untouched'
        );",
    )?;
    drop(connection);
    let target = TargetInventory {
        codex_home: codex_home.clone(),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(&preview, &target, &projects_root)?;

    let report = apply_bridge_plan(&plan)?;

    assert_eq!(report.sessions_written, 1);
    assert_eq!(report.index_entries_merged, 1);
    assert_eq!(report.sqlite_threads_imported, 1);
    let planned = &plan.sessions[0];
    let restored_jsonl = std::fs::read_to_string(&planned.target)?;
    let target_project = projects_root.join("visual").to_string_lossy().into_owned();
    let restored_row = serde_json::from_str::<Value>(restored_jsonl.trim())?;
    assert_eq!(restored_row["payload"]["cwd"], target_project);
    assert_ne!(restored_row["payload"]["cwd"], source_project);
    let index = std::fs::read_to_string(codex_home.join("session_index.jsonl"))?;
    assert_eq!(
        index
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .filter(|row| row["id"] == THREAD_ID)
            .count(),
        1
    );
    assert!(index.contains("99999999-9999-4999-8999-999999999999"));
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    let row = connection.query_row(
        "SELECT cwd, rollout_path, target_only FROM threads WHERE id = ?1",
        [THREAD_ID],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    assert_eq!(row.0, target_project);
    assert_eq!(row.1, planned.target.to_string_lossy());
    assert_eq!(row.2, "untouched");
    Ok(())
}

#[test]
fn bridge_revalidates_planned_hash_before_replacing_an_index() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("changed-target.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("hash-target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    std::fs::create_dir_all(&codex_home)?;
    let index_path = codex_home.join("session_index.jsonl");
    std::fs::write(&index_path, b"{\"id\":\"target-before-plan\"}\n")?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT);",
    )?;
    drop(connection);
    let target = TargetInventory {
        codex_home: codex_home.clone(),
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(&preview, &target, &projects_root)?;
    std::fs::write(&index_path, b"changed after planning\n")?;

    let error = apply_bridge_plan(&plan).unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert_eq!(std::fs::read(&index_path)?, b"changed after planning\n");
    Ok(())
}

#[test]
fn bridge_revalidates_a_skipped_session_before_accepting_the_plan() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("changed-skipped-session.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("changed-skipped-session-target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    std::fs::create_dir_all(&codex_home)?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT);",
    )?;
    drop(connection);
    let mut target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let initial_plan = build_restore_plan(&preview, &target, &projects_root)?;
    apply_bridge_plan(&initial_plan)?;
    target.conversations = preview.manifest.conversations.clone();
    let skip_plan = build_restore_plan(&preview, &target, &projects_root)?;
    assert_eq!(skip_plan.sessions[0].action, SessionAction::Skip);
    std::fs::write(
        &skip_plan.sessions[0].target,
        b"changed after skip planning\n",
    )?;

    let error = apply_bridge_plan(&skip_plan).unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert!(error.message.contains("changed after planning"));
    Ok(())
}

#[test]
fn bridge_rejects_changed_archive_with_same_package_id() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("archive-hash.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("archive-hash-target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    std::fs::create_dir_all(&codex_home)?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT);",
    )?;
    drop(connection);
    let target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(&preview, &target, &projects_root)?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&package_path)?
        .write_all(b"changed archive bytes")?;

    let error = apply_bridge_plan(&plan).unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::PackageInvalid
    );
    assert!(error.message.contains("archive hash"));
    Ok(())
}

#[test]
fn concurrent_bridge_applies_use_compare_and_swap_so_only_one_plan_commits(
) -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("concurrent.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("concurrent-target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    std::fs::create_dir_all(&codex_home)?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT);",
    )?;
    drop(connection);
    let target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = Arc::new(build_restore_plan(&preview, &target, &projects_root)?);
    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let handles = (0..workers)
        .map(|_| {
            let plan = Arc::clone(&plan);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                apply_bridge_plan(&plan)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("bridge apply thread panicked"))
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .all(|error| error.code == rehome_desktop_lib::core::error::ErrorCode::RestoreFailed));
    Ok(())
}

#[test]
fn bridge_refuses_to_replace_a_target_with_an_active_cas_lock() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("locked-target.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("locked-target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    std::fs::create_dir_all(&codex_home)?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT);",
    )?;
    drop(connection);
    let target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(&preview, &target, &projects_root)?;
    let session_target = &plan.sessions[0].target;
    std::fs::create_dir_all(session_target.parent().unwrap())?;
    let lock_name = format!(
        ".{}.codex-rehome.lock",
        session_target.file_name().unwrap().to_string_lossy()
    );
    let lock_path = session_target.parent().unwrap().join(lock_name);
    std::fs::write(&lock_path, b"another restore")?;

    let error = apply_bridge_plan(&plan).unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert!(error.message.contains("locked"));
    assert!(!session_target.exists());
    assert_eq!(std::fs::read(lock_path)?, b"another restore");
    Ok(())
}

#[test]
fn bridge_revalidates_ancestry_after_a_planned_directory_is_swapped_for_a_link(
) -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    align_fixture_project_metadata(&fixture)?;
    let package_path = fixture.root.join("ancestor-swap.rehome");
    create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: package_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&package_path)?;
    let target_root = fixture.root.join("ancestor-swap-target");
    let codex_home = target_root.join(".codex");
    let projects_root = target_root.join("projects");
    std::fs::create_dir_all(&codex_home)?;
    let connection = Connection::open(codex_home.join("state_5.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT, title TEXT);",
    )?;
    drop(connection);
    let target = TargetInventory {
        codex_home,
        target_os: current_source_os(),
        target_arch: "x86_64".into(),
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
    };
    let plan = build_restore_plan(&preview, &target, &projects_root)?;
    let planned_parent = plan.sessions[0].target.parent().unwrap();
    std::fs::create_dir_all(planned_parent)?;
    let original_parent = target_root.join("original-session-parent");
    std::fs::rename(planned_parent, &original_parent)?;
    let outside = fixture.root.join("outside-restore-root");
    std::fs::create_dir_all(&outside)?;
    if let Err(error) = create_directory_link(&outside, planned_parent) {
        if windows_symlink_privilege_is_unavailable(&error) {
            eprintln!("skipping apply ancestry swap test: symlink privilege unavailable");
            return Ok(());
        }
        return Err(error.into());
    }

    let error = apply_bridge_plan(&plan).unwrap_err();

    assert_eq!(
        error.code,
        rehome_desktop_lib::core::error::ErrorCode::RestoreFailed
    );
    assert!(error.message.contains("ancestor") || error.message.contains("ancestry"));
    assert_eq!(std::fs::read_dir(outside)?.count(), 0);
    Ok(())
}

fn planned_session() -> PlannedSession {
    PlannedSession {
        package_source: SOURCE.into(),
        target: PathBuf::from(MAC_SESSION),
        source_task_id: Uuid::parse_str(SOURCE_ID).unwrap(),
        target_task_id: Uuid::parse_str(TARGET_ID).unwrap(),
        title: "Original 路 ReHome".into(),
        source_content_hash: "source".into(),
        expected_final_content_hash: "target".into(),
        action: SessionAction::ImportAsBranch,
    }
}

fn simple_session(id: Uuid, title: &str) -> PlannedSession {
    PlannedSession {
        package_source: format!("codex/sessions/{id}.jsonl"),
        target: PathBuf::from(format!("/Users/test/.codex/sessions/{id}.jsonl")),
        source_task_id: id,
        target_task_id: id,
        title: title.into(),
        source_content_hash: "source".into(),
        expected_final_content_hash: "target".into(),
        action: SessionAction::Import,
    }
}

fn rewrites(source: &str) -> Vec<ReferenceRewrite> {
    let task_id = Uuid::parse_str(SOURCE_ID).unwrap();
    [
        (ReferenceRewriteKind::ConversationId, SOURCE_ID, TARGET_ID),
        (
            ReferenceRewriteKind::ConversationTitle,
            "Original",
            "Original 路 ReHome",
        ),
        (
            ReferenceRewriteKind::ProjectPath,
            WINDOWS_PROJECT,
            MAC_PROJECT,
        ),
        (
            ReferenceRewriteKind::ProjectPath,
            WINDOWS_PROJECT_SLASHED,
            MAC_PROJECT,
        ),
        (
            ReferenceRewriteKind::SessionPath,
            WINDOWS_SESSION,
            MAC_SESSION,
        ),
    ]
    .into_iter()
    .map(|(kind, from, to)| ReferenceRewrite {
        source_task_id: task_id,
        package_source: source.into(),
        kind,
        from: from.into(),
        to: to.into(),
    })
    .collect()
}

fn jsonl(values: &[Value]) -> String {
    let mut result = values
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    result.push('\n');
    result
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

#[cfg(windows)]
fn file_identity(path: &Path) -> std::io::Result<(u32, u32, u32)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = std::fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok((
            information.dwVolumeSerialNumber,
            information.nFileIndexHigh,
            information.nFileIndexLow,
        ))
    }
}

#[cfg(unix)]
fn file_identity(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(any(windows, unix)))]
fn file_identity(path: &Path) -> std::io::Result<std::time::SystemTime> {
    std::fs::metadata(path)?.created()
}

struct FakeRunner {
    result: Result<(), CommandRunError>,
    calls: RefCell<Vec<(PathBuf, Vec<OsString>)>>,
}

impl FakeRunner {
    fn succeeds() -> Self {
        Self {
            result: Ok(()),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn fails(error: CommandRunError) -> Self {
        Self {
            result: Err(error),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, command: &Path, arguments: &[OsString]) -> Result<(), CommandRunError> {
        self.calls
            .borrow_mut()
            .push((command.to_path_buf(), arguments.to_vec()));
        self.result.clone()
    }
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(not(windows))]
fn windows_symlink_privilege_is_unavailable(_error: &std::io::Error) -> bool {
    false
}

#[cfg(windows)]
fn windows_symlink_privilege_is_unavailable(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1314)
}

fn align_fixture_project_metadata(
    fixture: &common::SyntheticCodexFixture,
) -> Result<String, Box<dyn Error>> {
    let canonical = std::fs::canonicalize(&fixture.project_path)?;
    let source_project = canonical.to_string_lossy().into_owned();
    let project_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, source_project.as_bytes());
    for path in [&fixture.session_path, &fixture.session_index_path] {
        let text = std::fs::read_to_string(path)?;
        let mut output = Vec::new();
        for line in text.lines().filter(|line| !line.is_empty()) {
            let mut value = serde_json::from_str::<Value>(line)?;
            if value["type"] == "session_meta" {
                value["payload"]["project_id"] = Value::String(project_id.to_string());
                value["payload"]["cwd"] = Value::String(source_project.clone());
            } else {
                value["project_id"] = Value::String(project_id.to_string());
                value["cwd"] = Value::String(source_project.clone());
            }
            serde_json::to_writer(&mut output, &value)?;
            output.push(b'\n');
        }
        std::fs::write(path, output)?;
    }
    let connection = Connection::open(&fixture.state_db_path)?;
    connection.execute("UPDATE threads SET cwd = ?1", [&source_project])?;
    Ok(source_project)
}
