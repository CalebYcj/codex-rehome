//! Logical Codex projects are independent of filesystem workspace identities.
//! Keep their selected-thread metadata inside the existing backed-up DB transaction.
use super::{
    discovery::StateDatabaseSnapshot,
    error::{ErrorCode, RehomeError},
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;

pub(crate) const DESCRIPTOR: &str = "_rehome_project";

pub(crate) fn target_id(source: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("rehome:logical-project:{source}").as_bytes(),
    )
    .to_string()
}

fn invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}
fn failure(error: impl std::fmt::Display) -> RehomeError {
    RehomeError::new(
        ErrorCode::RestoreFailed,
        format!("Codex project metadata: {error}"),
    )
}
fn has_column(db: &Connection, table: &str, column: &str) -> Result<bool, RehomeError> {
    // Callers only supply static table names.
    let mut stmt = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(failure)?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(failure)?;
    for name in names {
        if name.map_err(failure)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn descriptor(db: &Connection, id: &str) -> Result<Option<Value>, RehomeError> {
    if !has_column(db, "projects", "id")? || !has_column(db, "project_roots", "path")? {
        return Ok(None);
    }
    let name: Option<String> = db
        .query_row("SELECT name FROM projects WHERE id=?1", [id], |r| r.get(0))
        .optional()
        .map_err(failure)?;
    let Some(name) = name else { return Ok(None) };
    let mut stmt = db
        .prepare("SELECT path FROM project_roots WHERE project_id=?1 ORDER BY position")
        .map_err(failure)?;
    let roots = stmt
        .query_map([id], |r| r.get::<_, String>(0))
        .map_err(failure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(failure)?;
    if roots.len() > 1000 || name.len() > 16 * 1024 || id.len() > 1024 {
        return Err(invalid("project metadata exceeds its inspection limit"));
    }
    Ok(Some(
        json!({"source_id":id,"name":name,"workspace_roots":roots}),
    ))
}

pub(crate) fn current_threads(database: &Path) -> Result<BTreeMap<Uuid, Value>, RehomeError> {
    let snapshot = StateDatabaseSnapshot::create(database).map_err(failure)?;
    let db = Connection::open_with_flags(
        snapshot.database_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(failure)?;
    let modern = has_column(&db, "threads", "project_id")?;
    let sql = if modern {
        "SELECT id,cwd,project_id FROM threads"
    } else {
        "SELECT id,cwd,NULL FROM threads"
    };
    let mut stmt = db.prepare(sql).map_err(failure)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(failure)?;
    let mut result = BTreeMap::new();
    for row in rows {
        let (id, cwd, project) = row.map_err(failure)?;
        let Ok(id) = Uuid::parse_str(&id) else {
            continue;
        };
        let mut value = json!({"cwd":cwd});
        if let Some(project) = project {
            if let Some(d) = descriptor(&db, &project)? {
                value[DESCRIPTOR] = d
            }
        }
        result.insert(id, value);
    }
    Ok(result)
}

/// Apply only when the target natively supports logical projects. Older Codex
/// keeps its existing path/open-workspace flow and receives no new DB schema.
pub(crate) fn prepare_import(
    db: &Connection,
    row: &mut Map<String, Value>,
) -> Result<(), RehomeError> {
    let Some(d) = row.get(DESCRIPTOR) else {
        row.remove("project_id");
        return Ok(());
    };
    if !has_column(db, "threads", "project_id")? {
        row.remove("project_id");
        return Ok(());
    }
    if !has_column(db, "projects", "id")? || !has_column(db, "project_roots", "path")? {
        return Err(failure("target project tables are unavailable"));
    }
    let d = d
        .as_object()
        .ok_or_else(|| invalid("invalid logical project descriptor"))?;
    let source = d
        .get("source_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 1024)
        .ok_or_else(|| invalid("logical project identity is missing"))?;
    let name = d
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 16 * 1024)
        .ok_or_else(|| invalid("logical project name is missing"))?;
    let roots = d
        .get("workspace_roots")
        .and_then(Value::as_array)
        .filter(|r| r.len() <= 1000)
        .ok_or_else(|| invalid("invalid logical project roots"))?;
    let roots = roots
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| Path::new(s).is_absolute())
                .map(str::to_owned)
                .ok_or_else(|| invalid("logical project root is not a valid target path"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let id = target_id(source);
    let thread = row
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("project member has no thread identity"))?;
    let existing: Option<Option<String>> = db
        .query_row(
            "SELECT project_id FROM threads WHERE id=?1",
            [thread],
            |r| r.get(0),
        )
        .optional()
        .map_err(failure)?;
    if existing.flatten().is_some_and(|old| old != id) {
        return Err(failure("existing conversation belongs to a different target project; its assignment was preserved"));
    }
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )
        .map_err(failure)?;
    if exists {
        let metadata: String = db
            .query_row("SELECT metadata FROM projects WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .map_err(failure)?;
        let metadata: Value = serde_json::from_str(&metadata).map_err(failure)?;
        if metadata
            .get("rehome_source_project_id")
            .and_then(Value::as_str)
            != Some(source)
        {
            return Err(failure("target project identity is owned by different metadata; existing project was preserved"));
        }
        let mut stmt = db
            .prepare("SELECT path FROM project_roots WHERE project_id=?1 ORDER BY position")
            .map_err(failure)?;
        let saved = stmt
            .query_map([&id], |r| r.get::<_, String>(0))
            .map_err(failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failure)?;
        let mut position: i64 = db
            .query_row(
                "SELECT COALESCE(MAX(position),-1)+1 FROM project_roots WHERE project_id=?1",
                [&id],
                |r| r.get(0),
            )
            .map_err(failure)?;
        for root in &roots {
            if !saved.contains(root) {
                db.execute(
                    "INSERT INTO project_roots(project_id,position,path) VALUES(?1,?2,?3)",
                    rusqlite::params![id, position, root],
                )
                .map_err(failure)?;
                position += 1;
            }
        }
    } else {
        let now = chrono::Utc::now().timestamp_millis();
        let metadata =
            serde_json::to_string(&json!({"rehome_source_project_id":source})).map_err(failure)?;
        db.execute("INSERT INTO projects(id,name,metadata,position,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,(SELECT COALESCE(MAX(position),-1)+1 FROM projects),?4,?4)",rusqlite::params![id,name,metadata,now]).map_err(failure)?;
        for (position, root) in roots.iter().enumerate() {
            db.execute(
                "INSERT INTO project_roots(project_id,position,path) VALUES(?1,?2,?3)",
                rusqlite::params![id, position as i64, root],
            )
            .map_err(failure)?;
        }
    }
    row.insert("project_id".into(), Value::String(id));
    Ok(())
}

pub(crate) fn verify_member(db: &Connection, row: &Map<String, Value>) -> Result<(), RehomeError> {
    let Some(expected) = row.get("project_id").and_then(Value::as_str) else {
        return Ok(());
    };
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing project member"))?;
    let actual: Option<String> = db
        .query_row("SELECT project_id FROM threads WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(failure)?;
    if actual.as_deref() != Some(expected) {
        return Err(failure(
            "restored conversation project membership did not verify",
        ));
    }
    Ok(())
}

/// Independent post-commit check; never repairs or writes during verification.
pub(crate) fn verify_restored(
    db: &Connection,
    row: &Map<String, Value>,
) -> Result<bool, RehomeError> {
    let Some(d) = row.get(DESCRIPTOR) else {
        return Ok(true);
    };
    if !has_column(db, "threads", "project_id")? {
        return Ok(true);
    }
    let source = d
        .get("source_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing source project"))?;
    let expected = target_id(source);
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing project member"))?;
    let actual: Option<String> = db
        .query_row("SELECT project_id FROM threads WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(failure)?
        .flatten();
    if actual.as_deref() != Some(&expected) {
        return Ok(false);
    }
    let Some(saved) = descriptor(db, &expected)? else {
        return Ok(false);
    };
    let roots = d
        .get("workspace_roots")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("invalid project roots"))?;
    let saved_roots = saved["workspace_roots"]
        .as_array()
        .ok_or_else(|| failure("invalid saved project roots"))?;
    Ok(roots.iter().all(|root| saved_roots.contains(root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,name TEXT NOT NULL,metadata TEXT NOT NULL DEFAULT '{}',position INTEGER NOT NULL,created_at_ms INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL);CREATE TABLE project_roots(project_id TEXT NOT NULL,position INTEGER NOT NULL,path TEXT NOT NULL,PRIMARY KEY(project_id,position));CREATE TABLE threads(id TEXT PRIMARY KEY,project_id TEXT);").unwrap();
        db
    }
    fn row(id: &str, source: &str, roots: &[String]) -> Map<String, Value> {
        json!({"id":id,DESCRIPTOR:{"source_id":source,"name":"论文分类","workspace_roots":roots}})
            .as_object()
            .unwrap()
            .clone()
    }
    #[test]
    fn repeated_and_partial_project_imports_reuse_identity_and_preserve_target_name() {
        let db = database();
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first").to_string_lossy().to_string();
        let second = temp.path().join("second").to_string_lossy().to_string();
        let mut a = row("a", "source", std::slice::from_ref(&first));
        prepare_import(&db, &mut a).unwrap();
        let id = a["project_id"].as_str().unwrap();
        db.execute(
            "UPDATE projects SET name='Renamed by user' WHERE id=?1",
            [id],
        )
        .unwrap();
        let mut b = row("b", "source", std::slice::from_ref(&second));
        prepare_import(&db, &mut b).unwrap();
        assert_eq!(a["project_id"], b["project_id"]);
        prepare_import(&db, &mut a).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM project_roots", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row("SELECT name FROM projects", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "Renamed by user"
        );
    }
    #[test]
    fn same_named_source_projects_stay_separate() {
        let db = database();
        let mut a = row("a", "first", &[]);
        let mut b = row("b", "second", &[]);
        prepare_import(&db, &mut a).unwrap();
        prepare_import(&db, &mut b).unwrap();
        assert_ne!(a["project_id"], b["project_id"]);
    }
    #[test]
    fn projectless_or_legacy_rows_do_not_import_unverified_source_ids() {
        let db = database();
        let mut r = json!({"id":"a","project_id":"unmapped-source-id"})
            .as_object()
            .unwrap()
            .clone();
        prepare_import(&db, &mut r).unwrap();
        assert!(!r.contains_key("project_id"));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn legacy_target_does_not_receive_new_project_schema() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY)")
            .unwrap();
        let mut r = row("a", "source", &[]);
        prepare_import(&db, &mut r).unwrap();
        assert!(!r.contains_key("project_id"));
        assert!(!has_column(&db, "projects", "id").unwrap());
    }
    #[test]
    fn changed_target_membership_is_preserved() {
        let db = database();
        db.execute("INSERT INTO threads VALUES('a','user-project')", [])
            .unwrap();
        let mut r = row("a", "source", &[]);
        assert!(prepare_import(&db, &mut r).is_err());
        assert_eq!(
            db.query_row("SELECT project_id FROM threads", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "user-project"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn foreign_project_identity_is_not_overwritten() {
        let db = database();
        let mut r = row("a", "source", &[]);
        prepare_import(&db, &mut r).unwrap();
        db.execute(
            "UPDATE projects SET metadata='{}',name='Original user project'",
            [],
        )
        .unwrap();
        assert!(prepare_import(&db, &mut r).is_err());
        assert_eq!(
            db.query_row("SELECT name FROM projects", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "Original user project"
        );
    }
    #[test]
    fn project_mutations_roll_back_inside_the_existing_transaction() {
        let mut db = database();
        {
            let tx = db.transaction().unwrap();
            let mut r = row("a", "source", &[]);
            prepare_import(&tx, &mut r).unwrap();
            assert_eq!(
                tx.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn relative_project_root_is_rejected_before_any_write() {
        let db = database();
        let mut r = row("a", "source", &["../outside".into()]);
        assert!(prepare_import(&db, &mut r).is_err());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn post_commit_verification_detects_missing_membership_and_roots() {
        let db = database();
        let temp = tempfile::tempdir().unwrap();
        let mut r = row("a", "source", &[temp.path().to_string_lossy().into_owned()]);
        prepare_import(&db, &mut r).unwrap();
        db.execute(
            "INSERT INTO threads VALUES('a',?1)",
            [r["project_id"].as_str().unwrap()],
        )
        .unwrap();
        assert!(verify_restored(&db, &r).unwrap());
        db.execute("UPDATE threads SET project_id=NULL", [])
            .unwrap();
        assert!(!verify_restored(&db, &r).unwrap());
        db.execute(
            "UPDATE threads SET project_id=?1",
            [r["project_id"].as_str().unwrap()],
        )
        .unwrap();
        db.execute("DELETE FROM project_roots", []).unwrap();
        assert!(!verify_restored(&db, &r).unwrap());
    }
}
