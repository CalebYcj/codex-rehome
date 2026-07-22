#[allow(dead_code)]
mod common;

use common::{synthetic_codex_fixture, SyntheticCodexFixture, THREAD_ID};
use rehome_desktop_lib::core::{
    error::ErrorCode,
    models::{
        ContentCounts, CreatePackageRequest, RecoveryStatus, RestoreOptions, RestorePlan, SourceOs,
        TargetInventory,
    },
    package::{create_package, inspect_package},
    planner::build_restore_plan,
    restore::{apply_restore, recover_incomplete_transactions, rollback},
};
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use tempfile::TempDir;
use uuid::Uuid;

static APP_DATA_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn successful_restore_commits_with_layered_verification() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;

    let report = apply_restore(harness.plan.clone(), harness.options())?;

    assert_eq!(report.package_id, harness.plan.package_id);
    assert!(report.restored_files > 0);
    assert!(report.restored_bytes > 0);
    assert!(report.verification.package_checksum_valid);
    assert!(report.verification.files_valid);
    assert!(report.verification.sessions_valid);
    assert!(report.verification.session_index_valid);
    assert!(report.verification.sqlite_threads_valid);
    assert!(report.verification.path_mapping_valid);
    assert!(report.verification.forbidden_files_absent);
    assert!(report.verification.project_files_valid);
    assert!(!report.verification.app_registration_valid);
    assert!(!report.verification.app_visible_ready);
    assert_ne!(snapshot_mutable_targets(&harness.plan)?, before);

    let journal = harness.read_journal(report.transaction_id)?;
    assert_eq!(journal["status"], "committed");
    assert_eq!(
        PathBuf::from(journal["backup_root"].as_str().unwrap()),
        fs::canonicalize(&harness.backup_root)?
    );
    let operations = journal["operations"].as_array().unwrap();
    assert!(operations.len() >= harness.plan.operations.len() + 3);
    assert!(operations.iter().any(|operation| {
        operation["target"] == harness.plan.sessions[0].target.to_string_lossy().as_ref()
            && operation["backup_kind"] == "absent"
    }));
    Ok(())
}

#[test]
fn failure_after_project_copy_rolls_every_target_back_exactly() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let session_target = &harness.plan.sessions[0].target;
    fs::create_dir_all(session_target.parent().unwrap())?;
    let lock_path = session_target.parent().unwrap().join(format!(
        ".{}.codex-rehome.lock",
        session_target.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&lock_path, b"test harness failure injection")?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert!(!harness
        .plan
        .projects_root
        .join("visual")
        .join("README.md")
        .exists());
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    Ok(())
}

#[test]
fn sqlite_update_failure_rolls_project_index_and_database_back_exactly(
) -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::RequiredColumnWithoutDefault)?;
    let before = snapshot_mutable_targets(&harness.plan)?;

    let error = apply_restore(harness.plan.clone(), harness.options()).unwrap_err();

    assert_eq!(error.code, ErrorCode::RestoreFailed);
    assert!(error.message.contains("SQLite") || error.message.contains("sqlite"));
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    Ok(())
}

#[test]
fn restart_discovers_an_incomplete_journal_from_app_data() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;
    let journal_path = harness.journal_path(report.transaction_id);
    let mut journal: Value = serde_json::from_slice(&fs::read(&journal_path)?)?;
    journal["status"] = Value::String("applying".into());
    for operation in journal["operations"].as_array_mut().unwrap() {
        operation["applied_hash"] = Value::Null;
    }
    fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;

    let pending = recover_incomplete_transactions()?;

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].transaction_id, report.transaction_id);
    assert_eq!(pending[0].status, RecoveryStatus::Applying);
    assert_eq!(
        pending[0].backup_root,
        fs::canonicalize(&harness.backup_root)?
    );
    assert!(rollback(report.transaction_id)?.success);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    Ok(())
}

#[test]
fn user_rollback_restores_exact_pre_restore_hashes_and_tombstones() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let before = snapshot_mutable_targets(&harness.plan)?;
    let report = apply_restore(harness.plan.clone(), harness.options())?;

    let rollback_report = rollback(report.transaction_id)?;

    assert!(rollback_report.success);
    assert!(rollback_report.restored_files > 0);
    assert_eq!(snapshot_mutable_targets(&harness.plan)?, before);
    assert_eq!(harness.single_journal_status()?, RecoveryStatus::RolledBack);
    Ok(())
}

#[test]
fn restore_requires_explicit_confirmation_that_codex_is_closed() -> Result<(), Box<dyn Error>> {
    let harness = RestoreHarness::new(DatabaseSchema::Compatible)?;
    let mut options = harness.options();
    options.codex_closed_confirmed = false;

    let error = apply_restore(harness.plan.clone(), options).unwrap_err();

    assert_eq!(error.code, ErrorCode::CodexRunning);
    assert!(!harness.transactions_dir().exists());
    Ok(())
}

#[derive(Clone, Copy)]
enum DatabaseSchema {
    Compatible,
    RequiredColumnWithoutDefault,
}

struct RestoreHarness {
    _env_lock: MutexGuard<'static, ()>,
    _previous_local_app_data: Option<OsString>,
    _app_data: TempDir,
    _fixture: SyntheticCodexFixture,
    plan: RestorePlan,
    backup_root: PathBuf,
}

impl RestoreHarness {
    fn new(schema: DatabaseSchema) -> Result<Self, Box<dyn Error>> {
        let env_lock = APP_DATA_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let app_data = tempfile::tempdir()?;
        let previous_local_app_data = env::var_os("LOCALAPPDATA");
        env::set_var("LOCALAPPDATA", app_data.path());

        let fixture = synthetic_codex_fixture()?;
        align_fixture_project_metadata(&fixture)?;
        let package_path = fixture.root.join("handoff.rehome");
        create_package(CreatePackageRequest {
            codex_home: fixture.codex_home.clone(),
            project_paths: vec![fixture.project_path.clone()],
            conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
            output_path: package_path.clone(),
            source_device_id: Uuid::nil(),
            include_skills: false,
            include_plugins: false,
            include_generated_images: false,
        })?;
        let preview = inspect_package(&package_path)?;
        let target_root = fixture.root.join("target");
        let codex_home = target_root.join(".codex");
        let projects_root = target_root.join("projects");
        fs::create_dir_all(&codex_home)?;
        fs::write(
            codex_home.join("session_index.jsonl"),
            b"{\"id\":\"99999999-9999-4999-8999-999999999999\",\"title\":\"Target\"}\n",
        )?;
        create_target_database(&codex_home.join("state_5.sqlite"), schema)?;
        let target = TargetInventory {
            codex_home,
            target_os: current_source_os(),
            target_arch: "x86_64".into(),
            counts: ContentCounts::default(),
            projects: vec![],
            conversations: vec![],
        };
        let plan = build_restore_plan(&preview, &target, &projects_root)?;
        let backup_root = app_data.path().join("com.rehome.desktop").join("backups");

        Ok(Self {
            _env_lock: env_lock,
            _previous_local_app_data: previous_local_app_data,
            _app_data: app_data,
            _fixture: fixture,
            plan,
            backup_root,
        })
    }

    fn options(&self) -> RestoreOptions {
        RestoreOptions {
            codex_closed_confirmed: true,
            backup_root: self.backup_root.clone(),
            register_projects: false,
        }
    }

    fn transactions_dir(&self) -> PathBuf {
        self._app_data
            .path()
            .join("com.rehome.desktop")
            .join("transactions")
    }

    fn journal_path(&self, transaction_id: Uuid) -> PathBuf {
        self.transactions_dir()
            .join(format!("{transaction_id}.json"))
    }

    fn read_journal(&self, transaction_id: Uuid) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&fs::read(
            self.journal_path(transaction_id),
        )?)?)
    }

    fn single_journal_status(&self) -> Result<RecoveryStatus, Box<dyn Error>> {
        let entries = fs::read_dir(self.transactions_dir())?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(entries.len(), 1);
        let journal: Value = serde_json::from_slice(&fs::read(entries[0].path())?)?;
        Ok(serde_json::from_value(journal["status"].clone())?)
    }
}

impl Drop for RestoreHarness {
    fn drop(&mut self) {
        if let Some(value) = self._previous_local_app_data.take() {
            env::set_var("LOCALAPPDATA", value);
        } else {
            env::remove_var("LOCALAPPDATA");
        }
    }
}

fn create_target_database(path: &Path, schema: DatabaseSchema) -> Result<(), Box<dyn Error>> {
    let connection = Connection::open(path)?;
    let extra = match schema {
        DatabaseSchema::Compatible => "target_only TEXT NOT NULL DEFAULT 'untouched'",
        DatabaseSchema::RequiredColumnWithoutDefault => "target_only TEXT NOT NULL",
    };
    connection.execute_batch(&format!(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT,
            rollout_path TEXT,
            title TEXT,
            updated_at TEXT,
            archived INTEGER,
            has_user_event INTEGER,
            preview TEXT,
            {extra}
        );"
    ))?;
    Ok(())
}

fn align_fixture_project_metadata(fixture: &SyntheticCodexFixture) -> Result<(), Box<dyn Error>> {
    let source_project = fs::canonicalize(&fixture.project_path)?
        .to_string_lossy()
        .into_owned();
    let project_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, source_project.as_bytes());
    for path in [&fixture.session_path, &fixture.session_index_path] {
        let mut output = Vec::new();
        for line in fs::read_to_string(path)?
            .lines()
            .filter(|line| !line.is_empty())
        {
            let mut value = serde_json::from_str::<Value>(line)?;
            value["project_id"] = Value::String(project_id.to_string());
            value["cwd"] = Value::String(source_project.clone());
            serde_json::to_writer(&mut output, &value)?;
            output.push(b'\n');
        }
        fs::write(path, output)?;
    }
    Connection::open(&fixture.state_db_path)?
        .execute("UPDATE threads SET cwd = ?1", [&source_project])?;
    Ok(())
}

fn snapshot_mutable_targets(
    plan: &RestorePlan,
) -> Result<BTreeMap<PathBuf, Option<String>>, Box<dyn Error>> {
    let mut paths = plan
        .operations
        .iter()
        .filter(|operation| operation.rollback_required)
        .map(|operation| operation.target.clone())
        .collect::<Vec<_>>();
    if let Some(database) = plan
        .operations
        .iter()
        .find(|operation| operation.package_source == "codex/metadata/threads.json")
        .map(|operation| &operation.target)
    {
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut sidecar = database.as_os_str().to_owned();
            sidecar.push(suffix);
            paths.push(PathBuf::from(sidecar));
        }
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let hash = match fs::read(&path) {
                Ok(bytes) => Some(format!("{:x}", Sha256::digest(bytes))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            Ok((path, hash))
        })
        .collect()
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}
