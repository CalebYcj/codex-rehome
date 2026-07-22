mod fixtures;

use fixtures::{synthetic_codex_fixture, FIXED_TIMESTAMP, PROJECT_ID, THREAD_ID, WINDOWS_CWD};
use rusqlite::Connection;
use serde_json::Value;
use std::{error::Error, fs};

#[test]
fn synthetic_fixture_contains_every_codex_and_project_element() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;

    assert_eq!(fixture.temp_dir.path(), fixture.root);
    assert_eq!(fixture.codex_home, fixture.root.join(".codex"));

    let session: Value = serde_json::from_str(&fs::read_to_string(&fixture.session_path)?)?;
    assert_eq!(session["thread_id"], THREAD_ID);
    assert_eq!(session["timestamp"], FIXED_TIMESTAMP);
    assert_eq!(session["cwd"], WINDOWS_CWD);

    let index: Value = serde_json::from_str(&fs::read_to_string(&fixture.session_index_path)?)?;
    assert_eq!(index["id"], THREAD_ID);
    assert_eq!(index["updated_at"], FIXED_TIMESTAMP);
    assert_eq!(index["cwd"], WINDOWS_CWD);
    assert_eq!(
        index["rollout_path"],
        fixture.session_path.to_string_lossy().as_ref()
    );

    assert!(fs::read_to_string(&fixture.skill_path)?.contains("Synthetic Skill"));
    let plugin: Value = serde_json::from_str(&fs::read_to_string(&fixture.plugin_manifest_path)?)?;
    assert_eq!(plugin["id"], "synthetic-plugin");
    assert_eq!(
        fs::read(&fixture.generated_image_path)?,
        b"synthetic image placeholder\n"
    );

    assert_eq!(
        fs::read_to_string(&fixture.readme_path)?,
        "# Visual project\n"
    );
    assert_eq!(
        fs::read_to_string(&fixture.env_path)?,
        "SECRET=fixture-only\n"
    );
    assert!(fs::read_to_string(&fixture.git_config_path)?.contains("example.invalid"));
    assert_eq!(
        fs::read_to_string(&fixture.node_modules_file_path)?,
        "module.exports = 'excluded';\n"
    );
    assert_eq!(
        fixture.project_path,
        fixture.root.join("projects").join("visual")
    );

    Ok(())
}

#[test]
fn synthetic_fixture_sqlite_thread_matches_session() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let connection = Connection::open(&fixture.state_db_path)?;
    let row = connection.query_row(
        "SELECT id, project_id, updated_at, cwd, rollout_path FROM threads WHERE id = ?1",
        [THREAD_ID],
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

    assert_eq!(row.0, THREAD_ID);
    assert_eq!(row.1, PROJECT_ID);
    assert_eq!(row.2, FIXED_TIMESTAMP);
    assert_eq!(row.3, WINDOWS_CWD);
    assert_eq!(row.4, fixture.session_path.to_string_lossy());

    Ok(())
}
