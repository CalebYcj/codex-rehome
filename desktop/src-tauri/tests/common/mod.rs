use rusqlite::{params, Connection};
use serde_json::json;
use std::{error::Error, fs, path::PathBuf};
use tempfile::TempDir;

pub const THREAD_ID: &str = "11111111-1111-4111-8111-111111111111";
pub const PROJECT_ID: &str = "22222222-2222-4222-8222-222222222222";
pub const FIXED_TIMESTAMP: &str = "2026-07-22T00:00:00Z";
pub const WINDOWS_CWD: &str = r"C:\Users\OldUser\Documents\visual";
const THREAD_TITLE: &str = "Synthetic migration thread";
const THREAD_PREVIEW: &str = "Synthetic migration thread preview";
const SOURCE_ROLLOUT_PATH: &str = r"C:\Users\OldUser\.codex\sessions\2026\07\22\rollout-2026-07-22T00-00-00-11111111-1111-4111-8111-111111111111.jsonl";

pub struct SyntheticCodexFixture {
    _temp_dir: TempDir,
    pub root: PathBuf,
    pub codex_home: PathBuf,
    pub session_path: PathBuf,
    pub session_index_path: PathBuf,
    pub state_db_path: PathBuf,
    pub skill_path: PathBuf,
    pub plugin_manifest_path: PathBuf,
    pub generated_image_path: PathBuf,
    pub project_path: PathBuf,
    pub readme_path: PathBuf,
    pub env_path: PathBuf,
    pub git_config_path: PathBuf,
    pub node_modules_file_path: PathBuf,
}

pub fn synthetic_codex_fixture() -> Result<SyntheticCodexFixture, Box<dyn Error>> {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path().to_path_buf();
    let codex_home = root.join(".codex");
    let session_path = codex_home
        .join("sessions")
        .join("2026")
        .join("07")
        .join("22")
        .join(format!("rollout-2026-07-22T00-00-00-{THREAD_ID}.jsonl"));
    let session_index_path = codex_home.join("session_index.jsonl");
    let state_db_path = codex_home.join("state_5.sqlite");
    let skill_path = codex_home
        .join("skills")
        .join("synthetic-skill")
        .join("SKILL.md");
    let plugin_manifest_path = codex_home
        .join("plugins")
        .join("cache")
        .join("synthetic-plugin")
        .join("manifest.json");
    let generated_image_path = codex_home
        .join("generated_images")
        .join("synthetic-image.png");
    let project_path = root.join("projects").join("visual");
    let readme_path = project_path.join("README.md");
    let env_path = project_path.join(".env");
    let git_config_path = project_path.join(".git").join("config");
    let node_modules_file_path = project_path.join("node_modules").join("file.js");

    write_file(
        &session_path,
        format!(
            "{}\n",
            serde_json::to_string(&json!({
                "type": "session_meta",
                "timestamp": FIXED_TIMESTAMP,
                "payload": {
                    "id": THREAD_ID,
                    "project_id": PROJECT_ID,
                    "cwd": WINDOWS_CWD,
                }
            }))?
        ),
    )?;
    write_file(
        &session_index_path,
        format!(
            "{}\n",
            serde_json::to_string(&json!({
                "id": THREAD_ID,
                "project_id": PROJECT_ID,
                "thread_name": THREAD_TITLE,
                "updated_at": FIXED_TIMESTAMP,
                "cwd": WINDOWS_CWD,
                "rollout_path": SOURCE_ROLLOUT_PATH,
            }))?
        ),
    )?;

    fs::create_dir_all(&codex_home)?;
    {
        let connection = Connection::open(&state_db_path)?;
        connection.execute(
            "CREATE TABLE threads (\
                id TEXT PRIMARY KEY, \
                cwd TEXT NOT NULL, \
                rollout_path TEXT NOT NULL, \
                title TEXT NOT NULL, \
                updated_at TEXT NOT NULL, \
                archived INTEGER NOT NULL, \
                has_user_event INTEGER NOT NULL, \
                preview TEXT NOT NULL\
            )",
            [],
        )?;
        connection.execute(
            "INSERT INTO threads (\
                id, cwd, rollout_path, title, updated_at, archived, has_user_event, preview\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                THREAD_ID,
                WINDOWS_CWD,
                SOURCE_ROLLOUT_PATH,
                THREAD_TITLE,
                FIXED_TIMESTAMP,
                0,
                1,
                THREAD_PREVIEW,
            ],
        )?;
    }

    write_file(
        &skill_path,
        "---\nname: synthetic-skill\ndescription: Synthetic Skill fixture\n---\n",
    )?;
    write_file(
        &plugin_manifest_path,
        serde_json::to_string_pretty(&json!({
            "id": "synthetic-plugin",
            "version": "1.0.0"
        }))?,
    )?;
    write_file(&generated_image_path, b"synthetic image placeholder\n")?;
    write_file(&readme_path, "# Visual project\n")?;
    write_file(&env_path, "SECRET=fixture-only\n")?;
    write_file(
        &git_config_path,
        "[remote \"origin\"]\n\turl = https://example.invalid/visual.git\n",
    )?;
    write_file(&node_modules_file_path, "module.exports = 'excluded';\n")?;

    Ok(SyntheticCodexFixture {
        _temp_dir: temp_dir,
        root,
        codex_home,
        session_path,
        session_index_path,
        state_db_path,
        skill_path,
        plugin_manifest_path,
        generated_image_path,
        project_path,
        readme_path,
        env_path,
        git_config_path,
        node_modules_file_path,
    })
}

fn write_file(path: &PathBuf, contents: impl AsRef<[u8]>) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}
