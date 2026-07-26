mod common;

use common::{synthetic_codex_fixture, FIXED_TIMESTAMP, THREAD_ID, WINDOWS_CWD};
use rehome_desktop_lib::core::{
    discovery::{
        discover_codex_with_context, resolve_codex_home, resolve_codex_home_for_os,
        DiscoveryContext,
    },
    error::ErrorCode,
    exclusions::is_forbidden,
    models::SourceOs,
    paths::{normalize_entry, validate_source_containment},
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use uuid::Uuid;

#[test]
fn mandatory_exclusions_are_component_aware() {
    let forbidden = [
        "project/.env",
        "project/.env.local",
        "project/.git/config",
        "project/node_modules/a.js",
        "home/.codex/auth.json",
        "profile/Cookies",
        "profile/Cookies-journal",
        "profile/Login Data",
        "profile/Login Data-journal",
        "profile/Login Data For Account",
        "profile/Login Data For Account-journal",
        "profile/Local Storage/data",
        "profile/Session Storage/data",
        "project/id_rsa",
        "project/id_dsa",
        "project/id_ecdsa",
        "project/id_ed25519",
        "project/client.pem",
        "project/client.key",
        "project/.venv/bin/python",
        "project/venv/bin/python",
        "runtime/server.sock",
        "runtime/server.ipc",
        "runtime/server.socket",
        "profile/SingletonLock",
        "profile/SingletonCookie",
        "profile/SingletonSocket",
        "profile/RunningChromeVersion",
        "project/__pycache__/module.pyc",
        "profile/Cache/data",
        "profile/Caches/data",
        "profile/GPUCache/data",
        "profile/Code Cache/data",
        "profile/CacheStorage/data",
        "project/build/output",
        "project/dist/output",
        "project/target/debug/app",
        "project/logs/app.log",
        "home/.codex/logs_1.sqlite",
        "home/.codex/logs_1.sqlite-wal",
        "project/.tmp/work",
        "project/tmp/work",
        "project/process_manager/state",
        "project/vendor_imports/source",
        "project/.DS_Store",
    ];

    for path in forbidden {
        assert!(is_forbidden(Path::new(path)), "expected forbidden: {path}");
    }

    for path in [
        "project/src/main.ts",
        "project/src/environment.ts",
        "project/git-notes/README.md",
        "project/cache-control.ts",
        "project/builder/main.rs",
    ] {
        assert!(!is_forbidden(Path::new(path)), "expected allowed: {path}");
    }
}

#[test]
fn portable_archive_entries_normalize_both_separator_styles() {
    assert_eq!(
        normalize_entry(Path::new(r"projects\visual\README.md")).unwrap(),
        "projects/visual/README.md"
    );
}

#[test]
fn portable_archive_entries_reject_unsafe_or_ambiguous_names() {
    let invalid = [
        "",
        ".",
        "./file",
        "projects//file",
        "projects/./file",
        "projects/",
        "../file",
        "projects/../file",
        "/etc/passwd",
        r"\Windows\System32",
        r"C:\Windows\file",
        "C:/Windows/file",
        r"C:file",
        r"\\server\share\file",
        "//server/share/file",
        r"\\?\C:\Windows\file",
        r"\\.\pipe\name",
        "project/file.txt:secret",
        "project/file\0name",
    ];

    for entry in invalid {
        assert!(
            normalize_entry(Path::new(entry)).is_err(),
            "expected rejection: {entry:?}"
        );
    }
}

#[test]
fn portable_archive_entries_reject_windows_forbidden_characters_and_controls() {
    for character in ['<', '>', '"', '|', '?', '*'] {
        let entry = format!("project/file{character}name");
        assert!(
            normalize_entry(Path::new(&entry)).is_err(),
            "expected rejection: {entry:?}"
        );
    }

    for control in '\u{1}'..='\u{1f}' {
        let entry = format!("project/file{control}name");
        assert!(
            normalize_entry(Path::new(&entry)).is_err(),
            "expected rejection of U+{:04X}",
            control as u32
        );
    }
}

#[test]
fn codex_home_resolution_has_explicit_precedence() {
    let context = DiscoveryContext {
        codex_home_env: Some(Path::new("env-home").to_path_buf()),
        user_profile: Some(Path::new("windows-user").to_path_buf()),
        home: Some(Path::new("unix-user").to_path_buf()),
    };

    assert_eq!(
        resolve_codex_home(Some(Path::new("override-home").to_path_buf()), &context).unwrap(),
        Path::new("override-home")
    );
    assert_eq!(
        resolve_codex_home(None, &context).unwrap(),
        Path::new("env-home")
    );

    let windows_default = DiscoveryContext {
        codex_home_env: None,
        ..context.clone()
    };
    assert_eq!(
        resolve_codex_home_for_os(None, &windows_default, SourceOs::Windows).unwrap(),
        Path::new("windows-user").join(".codex")
    );

    let mac_default = DiscoveryContext {
        codex_home_env: None,
        user_profile: context.user_profile,
        home: context.home,
    };
    assert_eq!(
        resolve_codex_home_for_os(None, &mac_default, SourceOs::Macos).unwrap(),
        Path::new("unix-user").join(".codex")
    );

    let windows_with_only_home = DiscoveryContext {
        codex_home_env: None,
        user_profile: None,
        home: Some(Path::new("unix-user").to_path_buf()),
    };
    let error =
        resolve_codex_home_for_os(None, &windows_with_only_home, SourceOs::Windows).unwrap_err();
    assert_eq!(error.code, ErrorCode::CodexNotFound);
}

#[test]
fn discovery_reports_fixture_without_modifying_it() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let before_session = fs::read(&fixture.session_path)?;
    let before_db = fs::read(&fixture.state_db_path)?;
    let context = DiscoveryContext::default();

    let inventory = discover_codex_with_context(Some(fixture.codex_home.clone()), &context)?;

    assert_eq!(inventory.codex_home, fixture.codex_home);
    assert_eq!(inventory.counts.conversations, 1);
    assert_eq!(inventory.counts.skills, 1);
    assert_eq!(inventory.counts.plugins, 1);
    assert_eq!(inventory.counts.generated_images, 1);
    assert_eq!(inventory.counts.sqlite_threads, 1);
    assert_eq!(
        inventory.session_index_path,
        Some(fixture.session_index_path)
    );
    assert_eq!(inventory.state_db_path, Some(fixture.state_db_path.clone()));
    assert_eq!(inventory.skill_paths, vec![fixture.skill_path]);
    assert_eq!(inventory.plugin_paths, vec![fixture.plugin_manifest_path]);
    assert_eq!(
        inventory.generated_image_paths,
        vec![fixture.generated_image_path]
    );
    assert_eq!(inventory.project_paths, vec![Path::new(WINDOWS_CWD)]);
    let project_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, WINDOWS_CWD.as_bytes());
    assert_eq!(inventory.projects.len(), 1);
    assert_eq!(inventory.projects[0].project_id, project_id);
    assert_eq!(inventory.projects[0].name, "visual");
    assert_eq!(inventory.projects[0].source_path, WINDOWS_CWD);
    assert_eq!(
        inventory.projects[0].archive_path,
        format!("projects/{project_id}/files")
    );
    assert_eq!(inventory.conversations.len(), 1);
    assert_eq!(
        inventory.conversations[0].task_id,
        Uuid::parse_str(THREAD_ID)?
    );
    assert_eq!(inventory.conversations[0].project_id, Some(project_id));
    assert_eq!(
        inventory.conversations[0].title,
        "Synthetic migration thread"
    );
    assert_eq!(inventory.conversations[0].updated_at, FIXED_TIMESTAMP);
    assert!(!inventory.conversations[0].content_hash.is_empty());
    assert!(inventory.conversations[0]
        .archive_path
        .starts_with("codex/sessions/"));
    assert!(!is_forbidden(&fixture.project_path));
    assert!(!is_forbidden(&fixture.readme_path));
    assert!(is_forbidden(&fixture.env_path));
    assert!(is_forbidden(&fixture.git_config_path));
    assert!(is_forbidden(&fixture.node_modules_file_path));
    assert_eq!(fs::read(&fixture.session_path)?, before_session);
    assert_eq!(fs::read(&fixture.state_db_path)?, before_db);

    Ok(())
}

#[test]
fn discovery_combines_global_state_and_sqlite_roots_in_stable_order() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let first = fixture.root.join("projects").join("first");
    let second = fixture.root.join("projects").join("second");
    fs::write(
        fixture.codex_home.join(".codex-global-state.json"),
        serde_json::to_vec(&json!({
            "electron-saved-workspace-roots": [first, second, "local-ae302ef6a5cf5ad3c6c80bf9dc388bb4", "058fe80e-b919-46c0-b2c0-c5e9a9fbe20f"],
            "project-order": [second],
            "active-workspace-roots": [first],
            "thread-workspace-root-hints": {
                "thread-a": second,
                "thread-b": WINDOWS_CWD
            }
        }))?,
    )?;

    let inventory =
        discover_codex_with_context(Some(fixture.codex_home), &DiscoveryContext::default())?;

    assert_eq!(
        inventory.project_paths,
        vec![first, second, Path::new(WINDOWS_CWD).to_path_buf()]
    );
    Ok(())
}

#[test]
fn discovery_dedupes_windows_paths_without_rewriting_the_first_path() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let first = r"C:\Users\OldUser\Documents\Visual\";
    fs::write(
        fixture.codex_home.join(".codex-global-state.json"),
        serde_json::to_vec(&json!({
            "electron-saved-workspace-roots": [first]
        }))?,
    )?;
    let connection = Connection::open(&fixture.state_db_path)?;
    connection.execute(
        "UPDATE threads SET cwd = ?1",
        [r"c:/users/olduser/documents/visual"],
    )?;
    drop(connection);

    let inventory =
        discover_codex_with_context(Some(fixture.codex_home), &DiscoveryContext::default())?;

    assert_eq!(inventory.project_paths, vec![PathBuf::from(first)]);
    Ok(())
}

#[test]
fn discovery_reads_a_private_wal_snapshot_without_touching_source_sidecars(
) -> Result<(), Box<dyn Error>> {
    let fixture = wal_codex_fixture()?;
    let before = snapshot_directory(&fixture.codex_home)?;
    assert!(before.contains_key(&OsString::from("state_7.sqlite-wal")));
    assert!(!before.contains_key(&OsString::from("state_7.sqlite-shm")));

    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;

    assert_eq!(inventory.counts.sqlite_threads, 1);
    assert_eq!(inventory.project_paths, vec![PathBuf::from(WINDOWS_CWD)]);
    assert_eq!(snapshot_directory(&fixture.codex_home)?, before);
    Ok(())
}

#[test]
fn traversal_warns_when_a_linked_directory_is_skipped() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let outside = fixture.root.join("outside-sessions");
    let linked = fixture.codex_home.join("sessions").join("linked");
    fs::create_dir(&outside)?;
    fs::write(outside.join("outside.jsonl"), b"{}\n")?;
    create_directory_link(&outside, &linked)?;

    let inventory =
        discover_codex_with_context(Some(fixture.codex_home), &DiscoveryContext::default())?;

    assert_eq!(inventory.counts.conversations, 1);
    assert!(inventory.warnings.iter().any(|warning| {
        warning.contains("sessions")
            && warning.contains("symbolic link")
            && warning.contains("linked")
    }));
    Ok(())
}

#[test]
fn invalid_optional_metadata_warns_but_does_not_fail() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(
        fixture.codex_home.join(".codex-global-state.json"),
        b"not json",
    )?;
    fs::write(fixture.codex_home.join("state_9.sqlite"), b"not sqlite")?;

    let inventory =
        discover_codex_with_context(Some(fixture.codex_home), &DiscoveryContext::default())?;

    assert_eq!(
        inventory.state_db_path,
        Some(fixture.root.join(".codex/state_9.sqlite"))
    );
    assert!(inventory.warnings.len() >= 2);
    Ok(())
}

#[test]
fn missing_optional_metadata_warns_but_does_not_fail() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let codex_home = temp.path().join(".codex");
    fs::create_dir(&codex_home)?;

    let inventory = discover_codex_with_context(Some(codex_home), &DiscoveryContext::default())?;

    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.contains("session index")));
    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.contains("global state")));
    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.contains("state database")));
    Ok(())
}

#[test]
fn missing_or_non_directory_codex_home_has_stable_error_code() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let missing = temp.path().join("missing");
    let error =
        discover_codex_with_context(Some(missing), &DiscoveryContext::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::CodexNotFound);

    let file = temp.path().join("file");
    fs::write(&file, b"not a directory")?;
    let error = discover_codex_with_context(Some(file), &DiscoveryContext::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::CodexNotFound);
    Ok(())
}

#[test]
fn symlinked_codex_home_is_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let real_home = temp.path().join("real-codex-home");
    let linked_home = temp.path().join("linked-codex-home");
    fs::create_dir(&real_home)?;
    if let Err(error) = create_dir_symlink(&real_home, &linked_home) {
        if windows_symlink_privilege_is_unavailable(&error) {
            eprintln!("skipping linked Codex home test: Windows symlink privilege unavailable");
            return Ok(());
        }
        return Err(error.into());
    }

    let error =
        discover_codex_with_context(Some(linked_home), &DiscoveryContext::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::CodexNotFound);
    Ok(())
}

#[test]
fn symlinked_session_index_is_ignored_with_a_warning() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let linked_index = fixture.session_index_path.clone();
    let real_index = fixture.root.join("external-session-index.jsonl");
    fs::remove_file(&linked_index)?;
    fs::write(&real_index, b"{}\n")?;
    if let Err(error) = create_file_symlink(&real_index, &linked_index) {
        if windows_symlink_privilege_is_unavailable(&error) {
            eprintln!("skipping linked session index test: Windows symlink privilege unavailable");
            return Ok(());
        }
        return Err(error.into());
    }

    let inventory =
        discover_codex_with_context(Some(fixture.codex_home), &DiscoveryContext::default())?;

    assert_eq!(inventory.session_index_path, None);
    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.contains("session index") && warning.contains("symbolic link")));
    Ok(())
}

#[test]
fn malformed_session_index_warns_and_keeps_its_path() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(&fixture.session_index_path, b"{}\n[]\nnot-json\n")?;

    let inventory =
        discover_codex_with_context(Some(fixture.codex_home), &DiscoveryContext::default())?;

    assert_eq!(
        inventory.session_index_path,
        Some(fixture.session_index_path)
    );
    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.contains("session index") && warning.contains("malformed")));
    Ok(())
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn windows_symlink_privilege_is_unavailable(_error: &std::io::Error) -> bool {
    false
}

#[cfg(windows)]
fn windows_symlink_privilege_is_unavailable(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(1314)
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    create_junction(target, link)
}

#[cfg(windows)]
fn create_junction(target: &Path, link: &Path) -> std::io::Result<()> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

#[cfg(windows)]
#[test]
fn junctioned_codex_home_is_rejected_without_symlink_privileges() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let real_home = temp.path().join("real-codex-home");
    let junction_home = temp.path().join("junction-codex-home");
    fs::create_dir(&real_home)?;
    create_junction(&real_home, &junction_home)?;

    let error =
        discover_codex_with_context(Some(junction_home), &DiscoveryContext::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::CodexNotFound);
    Ok(())
}

#[cfg(windows)]
#[test]
fn source_containment_rejects_junctions_escaping_the_selected_root() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("selected");
    let outside = temp.path().join("outside");
    let junction = root.join("junction");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    create_junction(&outside, &junction)?;

    assert!(validate_source_containment(&root, &junction).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_containment_rejects_symlinks_escaping_the_selected_root() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let root = temp.path().join("selected");
    let outside = temp.path().join("outside.txt");
    fs::create_dir(&root)?;
    fs::write(&outside, b"secret")?;
    symlink(&outside, root.join("link"))?;

    assert!(validate_source_containment(&root, &root.join("link")).is_err());
    Ok(())
}

struct WalCodexFixture {
    _temp_dir: tempfile::TempDir,
    _writer: Connection,
    codex_home: PathBuf,
}

fn wal_codex_fixture() -> Result<WalCodexFixture, Box<dyn Error>> {
    let temp_dir = tempfile::tempdir()?;
    let generator = temp_dir.path().join("generator.sqlite");
    let codex_home = temp_dir.path().join(".codex");
    fs::create_dir(&codex_home)?;

    let writer = Connection::open(&generator)?;
    writer.pragma_update(None, "journal_mode", "WAL")?;
    writer.pragma_update(None, "wal_autocheckpoint", 0)?;
    writer.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL)",
        [],
    )?;
    writer.execute(
        "INSERT INTO threads (id, cwd) VALUES (?1, ?2)",
        params!["wal-thread", WINDOWS_CWD],
    )?;

    let source_wal = sqlite_sidecar(&generator, "-wal");
    let state_db = codex_home.join("state_7.sqlite");
    fs::copy(&generator, &state_db)?;
    fs::copy(source_wal, sqlite_sidecar(&state_db, "-wal"))?;

    Ok(WalCodexFixture {
        _temp_dir: temp_dir,
        _writer: writer,
        codex_home,
    })
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

#[derive(Debug, PartialEq, Eq)]
struct DirectoryEntrySnapshot {
    bytes: Option<Vec<u8>>,
    is_file: bool,
    is_dir: bool,
    len: u64,
    readonly: bool,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

fn snapshot_directory(
    directory: &Path,
) -> Result<BTreeMap<OsString, DirectoryEntrySnapshot>, Box<dyn Error>> {
    let mut snapshot = BTreeMap::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        snapshot.insert(
            entry.file_name(),
            DirectoryEntrySnapshot {
                bytes: metadata.is_file().then(|| fs::read(&path)).transpose()?,
                is_file: metadata.is_file(),
                is_dir: metadata.is_dir(),
                len: metadata.len(),
                readonly: metadata.permissions().readonly(),
                modified: metadata.modified().ok(),
                created: metadata.created().ok(),
            },
        );
    }
    Ok(snapshot)
}

#[cfg(windows)]
#[test]
fn source_containment_rejects_symlinks_escaping_the_selected_root() -> Result<(), Box<dyn Error>> {
    use std::os::windows::fs::symlink_file;

    let temp = tempfile::tempdir()?;
    let root = temp.path().join("selected");
    let outside = temp.path().join("outside.txt");
    fs::create_dir(&root)?;
    fs::write(&outside, b"secret")?;
    if let Err(error) = symlink_file(&outside, root.join("link")) {
        if error.raw_os_error() == Some(1314) {
            eprintln!("skipping symlink containment test: Windows symlink privilege unavailable");
            return Ok(());
        }
        return Err(error.into());
    }

    assert!(validate_source_containment(&root, &root.join("link")).is_err());
    Ok(())
}
