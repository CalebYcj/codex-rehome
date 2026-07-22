use crate::core::{
    bridge::{validate_restore_target, validate_restore_target_ancestry},
    error::{ErrorCode, RehomeError},
    models::{
        PendingRecovery, RecoveryStatus, RestorePlan, RollbackReport, TransactionHistory,
        TransactionSummary,
    },
    stable_fs::PinnedParent,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    env, fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tempfile::NamedTempFile;
use uuid::Uuid;

const APP_IDENTIFIER: &str = "com.rehome.desktop";
const TRANSACTIONS_DIRECTORY: &str = "transactions";
const SQLITE_SIDECARS: &[&str] = &["-wal", "-shm", "-journal"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BackupKind {
    File,
    Absent,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RollbackProgress {
    #[default]
    Pending,
    TargetQuarantined,
    QuarantineVerified,
    TargetRemoved,
    OriginalRestored,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum AppliedState {
    Absent,
    File { hash: String, identity: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct JournalLock {
    pub target: PathBuf,
    pub path: PathBuf,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BackupOperation {
    pub package_source: String,
    pub target: PathBuf,
    pub backup_kind: BackupKind,
    pub backup_path: Option<PathBuf>,
    pub original_hash: Option<String>,
    #[serde(default)]
    pub applied_hash: Option<String>,
    #[serde(default)]
    pub applied_state: Option<AppliedState>,
    #[serde(default)]
    pub applied_database_hash: Option<String>,
    pub readonly: Option<bool>,
    pub unix_mode: Option<u32>,
    #[serde(default)]
    pub rollback_progress: RollbackProgress,
    #[serde(default)]
    pub rollback_quarantine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TransactionJournal {
    pub transaction_id: Uuid,
    pub package_id: Uuid,
    pub status: RecoveryStatus,
    pub created_at: String,
    pub operations: Vec<BackupOperation>,
    pub backup_root: PathBuf,
    pub target_codex_home: PathBuf,
    pub projects_root: PathBuf,
    #[serde(default)]
    pub locks: Vec<JournalLock>,
}

pub(crate) struct PreparedTransaction {
    pub journal: TransactionJournal,
    pub journal_path: PathBuf,
}

pub(crate) fn prepare_transaction(
    plan: &RestorePlan,
    requested_backup_root: &Path,
) -> Result<PreparedTransaction, RehomeError> {
    if !requested_backup_root.is_absolute() {
        return Err(restore_failed("backup root must be an absolute local path"));
    }
    let backup_root = create_and_canonicalize_directory(requested_backup_root, "backup root")?;
    let app_data = app_data_root()?;
    let transactions = create_and_canonicalize_directory(
        &app_data.join(TRANSACTIONS_DIRECTORY),
        "transaction journal directory",
    )?;
    validate_directory_entry(&transactions)?;

    let transaction_id = Uuid::new_v4();
    let transaction_backup = backup_root.join(transaction_id.to_string());
    fs::create_dir(&transaction_backup)
        .map_err(|error| restore_failed(format!("could not create transaction backup: {error}")))?;
    sync_directory(&backup_root).map_err(|error| {
        restore_failed(format!("could not sync transaction backup parent: {error}"))
    })?;
    let objects = transaction_backup.join("objects");
    fs::create_dir(&objects)
        .map_err(|error| restore_failed(format!("could not create backup objects: {error}")))?;
    sync_directory(&transaction_backup)
        .map_err(|error| restore_failed(format!("could not sync backup directory: {error}")))?;

    let targets = mutable_targets(plan)?;
    let mut operations = Vec::with_capacity(targets.len());
    for (index, (package_source, target, expected_hash)) in targets.into_iter().enumerate() {
        let root = operation_root(plan, &target)?;
        validate_restore_target(root, &target)?;
        let operation = if package_source == "codex/metadata/threads.json" {
            backup_sqlite_database(
                &objects,
                index,
                package_source,
                target,
                expected_hash.as_deref(),
            )?
        } else if package_source.starts_with("codex/metadata/sqlite-sidecar") {
            backup_sqlite_sidecar(package_source, target)?
        } else {
            backup_target(
                &objects,
                index,
                package_source,
                target,
                expected_hash.as_deref(),
            )?
        };
        operations.push(operation);
    }

    let locks = operations
        .iter()
        .map(|operation| {
            Ok(JournalLock {
                target: operation.target.clone(),
                path: target_lock_path(&operation.target)?,
                token: transaction_id.to_string(),
            })
        })
        .collect::<Result<Vec<_>, RehomeError>>()?;
    let journal = TransactionJournal {
        transaction_id,
        package_id: plan.package_id,
        status: RecoveryStatus::Prepared,
        created_at: timestamp(),
        operations,
        backup_root,
        target_codex_home: plan.target_codex_home.clone(),
        projects_root: plan.projects_root.clone(),
        locks,
    };
    let journal_path = transactions.join(format!("{transaction_id}.json"));
    write_journal(&journal_path, &journal)?;
    Ok(PreparedTransaction {
        journal,
        journal_path,
    })
}

pub(crate) fn update_status(
    prepared: &mut PreparedTransaction,
    status: RecoveryStatus,
) -> Result<(), RehomeError> {
    prepared.journal.status = status;
    write_journal(&prepared.journal_path, &prepared.journal)
}

pub(crate) fn record_applied_mutation(
    prepared: &mut PreparedTransaction,
    target: &Path,
) -> Result<(), RehomeError> {
    let index = prepared
        .journal
        .operations
        .iter()
        .position(|operation| operation.target == target)
        .ok_or_else(|| restore_failed("mutated target is missing from the transaction journal"))?;
    let include_sidecars =
        prepared.journal.operations[index].package_source == "codex/metadata/threads.json";
    let mut indices = vec![index];
    if include_sidecars {
        indices.extend(
            prepared
                .journal
                .operations
                .iter()
                .enumerate()
                .filter(|(_, operation)| {
                    operation
                        .package_source
                        .starts_with("codex/metadata/sqlite-sidecar")
                })
                .map(|(index, _)| index),
        );
    }
    for index in indices {
        let operation = &prepared.journal.operations[index];
        let applied_state = inspect_applied_state(operation)?;
        prepared.journal.operations[index].applied_hash = match &applied_state {
            AppliedState::File { hash, .. } => Some(hash.clone()),
            AppliedState::Absent => None,
        };
        prepared.journal.operations[index].applied_state = Some(applied_state);
    }
    if include_sidecars {
        let database_hash = prepared.journal.operations[index]
            .applied_hash
            .clone()
            .ok_or_else(|| restore_failed("applied SQLite database has no logical hash"))?;
        for operation in &mut prepared.journal.operations {
            if operation
                .package_source
                .starts_with("codex/metadata/sqlite-sidecar")
            {
                operation.applied_database_hash = Some(database_hash.clone());
            }
        }
    }
    write_journal(&prepared.journal_path, &prepared.journal)
}

pub(crate) fn ensure_applied_states(prepared: &PreparedTransaction) -> Result<(), RehomeError> {
    if prepared
        .journal
        .operations
        .iter()
        .any(|operation| operation.applied_state.is_none())
    {
        return Err(restore_failed(
            "transaction journal is missing applied operation state",
        ));
    }
    Ok(())
}

fn inspect_applied_state(operation: &BackupOperation) -> Result<AppliedState, RehomeError> {
    match fs::symlink_metadata(&operation.target) {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(restore_failed(format!(
                "restored target is not a regular file: {}",
                operation.target.display()
            )))
        }
        Ok(_) => {
            let hash = if operation.package_source == "codex/metadata/threads.json" {
                hash_sqlite_database(&operation.target)?
            } else {
                hash_file(&operation.target)?
            };
            Ok(AppliedState::File {
                hash,
                identity: file_identity(&operation.target)?,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppliedState::Absent),
        Err(error) => Err(restore_failed(format!(
            "could not inspect restored target {}: {error}",
            operation.target.display()
        ))),
    }
}

pub(crate) fn rollback_prepared(
    prepared: &mut PreparedTransaction,
) -> Result<RollbackReport, RehomeError> {
    rollback_loaded(&prepared.journal_path, &mut prepared.journal)
}

pub fn rollback(transaction_id: Uuid) -> Result<RollbackReport, RehomeError> {
    let journal_path = journal_path(transaction_id)?;
    let mut journal = load_validated_journal(&journal_path, Some(transaction_id))?;
    if journal.status == RecoveryStatus::RolledBack {
        return Ok(RollbackReport {
            transaction_id,
            completed_at: timestamp(),
            restored_files: 0,
            success: true,
        });
    }
    rollback_loaded(&journal_path, &mut journal)
}

pub fn list_transactions() -> Result<Vec<TransactionSummary>, RehomeError> {
    Ok(list_transaction_history()?.transactions)
}

pub fn list_transaction_history() -> Result<TransactionHistory, RehomeError> {
    let Some(app_data) = existing_app_data_root()? else {
        return Ok(TransactionHistory {
            transactions: Vec::new(),
            warnings: Vec::new(),
        });
    };
    let transactions = app_data.join(TRANSACTIONS_DIRECTORY);
    let entries = match fs::read_dir(&transactions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TransactionHistory {
                transactions: Vec::new(),
                warnings: Vec::new(),
            })
        }
        Err(error) => {
            return Err(rollback_failed(format!(
                "could not enumerate transaction journals: {error}"
            )))
        }
    };
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|error| {
        rollback_failed(format!("could not read transaction journal entry: {error}"))
    })?;
    entries.sort_by_key(|entry| entry.file_name());

    let mut summaries = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let summary = journal_id_from_path(&path)
            .and_then(|transaction_id| load_validated_journal(&path, Some(transaction_id)))
            .map(transaction_summary_from_journal);
        match summary {
            Ok(summary) => summaries.push(summary),
            Err(error) => warnings.push(format!(
                "skipped transaction journal {}: {}",
                entry.file_name().to_string_lossy(),
                error.message
            )),
        }
    }
    summaries.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.transaction_id.cmp(&left.transaction_id))
    });
    Ok(TransactionHistory {
        transactions: summaries,
        warnings,
    })
}

pub fn transaction_summary(
    transaction_id: Uuid,
) -> Result<Option<TransactionSummary>, RehomeError> {
    let Some(app_data) = existing_app_data_root()? else {
        return Ok(None);
    };
    let path = app_data
        .join(TRANSACTIONS_DIRECTORY)
        .join(format!("{transaction_id}.json"));
    match fs::symlink_metadata(&path) {
        Ok(_) => load_validated_journal(&path, Some(transaction_id))
            .map(transaction_summary_from_journal)
            .map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(rollback_failed(format!(
            "could not inspect requested transaction journal: {error}"
        ))),
    }
}

fn transaction_summary_from_journal(journal: TransactionJournal) -> TransactionSummary {
    let restored_project_paths = restored_project_paths(&journal);
    TransactionSummary {
        transaction_id: journal.transaction_id,
        package_id: journal.package_id,
        created_at: journal.created_at,
        status: journal.status,
        transaction_backup_path: journal.backup_root.join(journal.transaction_id.to_string()),
        backup_root: journal.backup_root,
        target_codex_home: journal.target_codex_home,
        projects_root: journal.projects_root,
        restored_project_paths,
        changed_files: journal.operations.len() as u64,
    }
}

fn restored_project_paths(journal: &TransactionJournal) -> Vec<PathBuf> {
    let Ok(canonical_projects_root) = fs::canonicalize(&journal.projects_root) else {
        return Vec::new();
    };
    let mut restored = Vec::new();

    for operation in &journal.operations {
        let mut source_components = Path::new(&operation.package_source).components();
        if source_components.next() != Some(Component::Normal("projects".as_ref())) {
            continue;
        }
        let Some(Component::Normal(project_id)) = source_components.next() else {
            continue;
        };
        if project_id
            .to_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            .is_none()
            || source_components.next() != Some(Component::Normal("files".as_ref()))
            || source_components.next().is_none()
        {
            continue;
        }
        let Ok(relative_target) = operation.target.strip_prefix(&journal.projects_root) else {
            continue;
        };
        let mut target_components = relative_target.components();
        let Some(Component::Normal(project_name)) = target_components.next() else {
            continue;
        };
        if target_components.next().is_none() {
            continue;
        }

        let candidate = journal.projects_root.join(project_name);
        let Ok(metadata) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            continue;
        }
        let Ok(canonical) = fs::canonicalize(candidate) else {
            continue;
        };
        if canonical.parent() == Some(canonical_projects_root.as_path()) {
            restored.push(canonical);
        }
    }

    restored.sort();
    restored.dedup();
    restored
}

pub fn recover_incomplete_transactions() -> Result<Vec<PendingRecovery>, RehomeError> {
    let transactions = app_data_root()?.join(TRANSACTIONS_DIRECTORY);
    let entries = match fs::read_dir(&transactions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(restore_failed(format!(
                "could not enumerate transaction journals: {error}"
            )))
        }
    };
    let mut pending = Vec::new();
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|error| {
        rollback_failed(format!("could not read transaction journal entry: {error}"))
    })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let transaction_id = journal_id_from_path(&path)?;
        let journal = load_validated_journal(&path, Some(transaction_id))?;
        remove_owned_stale_locks(&journal)?;
        if matches!(
            journal.status,
            RecoveryStatus::Committed | RecoveryStatus::RolledBack
        ) {
            continue;
        }
        pending.push(PendingRecovery {
            transaction_id: journal.transaction_id,
            package_id: journal.package_id,
            created_at: journal.created_at,
            status: journal.status,
            backup_root: journal.backup_root,
        });
    }
    pending.sort_by_key(|entry| (entry.created_at.clone(), entry.transaction_id));
    Ok(pending)
}

fn journal_id_from_path(path: &Path) -> Result<Uuid, RehomeError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        rollback_failed(format!("could not inspect transaction journal: {error}"))
    })?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(rollback_failed("transaction journal is not a regular file"));
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| rollback_failed("transaction journal file name is not a UUID"))?;
    Uuid::parse_str(stem)
        .map_err(|_| rollback_failed("transaction journal file name is not a UUID"))
}

fn mutable_targets(
    plan: &RestorePlan,
) -> Result<Vec<(String, PathBuf, Option<String>)>, RehomeError> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let mut sqlite_database = None;
    for operation in &plan.operations {
        let writable = matches!(
            operation.action,
            crate::core::models::ChangeKind::Add | crate::core::models::ChangeKind::Update
        );
        if writable != operation.rollback_required {
            return Err(restore_failed(
                "every writable restore operation must require rollback",
            ));
        }
        if !writable {
            continue;
        }
        if !seen.insert(operation.target.clone()) {
            return Err(restore_failed("restore plan contains duplicate targets"));
        }
        targets.push((
            operation.package_source.clone(),
            operation.target.clone(),
            operation.expected_previous_hash.clone(),
        ));
        if operation.package_source == "codex/metadata/threads.json" {
            sqlite_database = Some(operation.target.clone());
        }
    }
    if let Some(database) = sqlite_database {
        for suffix in SQLITE_SIDECARS {
            let sidecar = sqlite_sidecar(&database, suffix);
            if seen.insert(sidecar.clone()) {
                targets.push((
                    format!("codex/metadata/sqlite-sidecar{suffix}"),
                    sidecar,
                    None,
                ));
            }
        }
    }
    Ok(targets)
}

fn backup_target(
    objects: &Path,
    index: usize,
    package_source: String,
    target: PathBuf,
    expected_hash: Option<&str>,
) -> Result<BackupOperation, RehomeError> {
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(restore_failed(format!(
                "restore target is not a regular file: {}",
                target.display()
            )))
        }
        Ok(metadata) => {
            let before_hash = hash_file(&target)?;
            if expected_hash.is_some_and(|expected| !before_hash.eq_ignore_ascii_case(expected)) {
                return Err(restore_failed(format!(
                    "restore target changed after planning: {}",
                    target.display()
                )));
            }
            let relative = PathBuf::from("objects").join(format!("{index:08}.bin"));
            let destination = objects.join(format!("{index:08}.bin"));
            copy_file_atomically(&target, &destination)?;
            let backup_hash = hash_file(&destination)?;
            let after_hash = hash_file(&target)?;
            if backup_hash != before_hash || after_hash != before_hash {
                return Err(restore_failed(format!(
                    "restore target changed while it was backed up: {}",
                    target.display()
                )));
            }
            Ok(BackupOperation {
                package_source,
                target,
                backup_kind: BackupKind::File,
                backup_path: Some(relative),
                original_hash: Some(before_hash),
                applied_hash: None,
                applied_state: None,
                applied_database_hash: None,
                readonly: Some(metadata.permissions().readonly()),
                unix_mode: unix_mode(&metadata),
                rollback_progress: RollbackProgress::Pending,
                rollback_quarantine: None,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if expected_hash.is_some() {
                return Err(restore_failed(format!(
                    "restore target disappeared after planning: {}",
                    target.display()
                )));
            }
            Ok(BackupOperation {
                package_source,
                target,
                backup_kind: BackupKind::Absent,
                backup_path: None,
                original_hash: None,
                applied_hash: None,
                applied_state: None,
                applied_database_hash: None,
                readonly: None,
                unix_mode: None,
                rollback_progress: RollbackProgress::Pending,
                rollback_quarantine: None,
            })
        }
        Err(error) => Err(restore_failed(format!(
            "could not inspect restore target {}: {error}",
            target.display()
        ))),
    }
}

fn backup_sqlite_database(
    objects: &Path,
    index: usize,
    package_source: String,
    target: PathBuf,
    expected_hash: Option<&str>,
) -> Result<BackupOperation, RehomeError> {
    let metadata = fs::symlink_metadata(&target).map_err(|error| {
        restore_failed(format!("could not inspect target SQLite database: {error}"))
    })?;
    if metadata_is_link_or_reparse(&metadata)
        || !metadata.is_file()
        || raw_file_link_count(&target)
            .map_err(|error| restore_failed(format!("could not inspect SQLite links: {error}")))?
            > 1
    {
        return Err(restore_failed(
            "target SQLite database is not a regular unlinked file",
        ));
    }
    let before_hash = hash_file(&target)?;
    if expected_hash.is_some_and(|expected| !before_hash.eq_ignore_ascii_case(expected)) {
        return Err(restore_failed(
            "target SQLite database changed after planning",
        ));
    }

    let relative = PathBuf::from("objects").join(format!("{index:08}.bin"));
    let destination = objects.join(format!("{index:08}.bin"));
    let temporary = NamedTempFile::new_in(objects)
        .map_err(|error| restore_failed(format!("could not create SQLite backup: {error}")))?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let source = Connection::open_with_flags(&target, flags).map_err(|error| {
        restore_failed(format!("could not open target SQLite database: {error}"))
    })?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| {
            restore_failed(format!("could not configure SQLite backup lock: {error}"))
        })?;
    let snapshot_result = (|| {
        let mut snapshot = Connection::open(temporary.path()).map_err(|error| {
            restore_failed(format!("could not open SQLite backup destination: {error}"))
        })?;
        let backup = Backup::new(&source, &mut snapshot)
            .map_err(|error| restore_failed(format!("could not start SQLite backup: {error}")))?;
        backup
            .run_to_completion(128, Duration::from_millis(1), None)
            .map_err(|error| {
                restore_failed(format!("could not complete SQLite backup: {error}"))
            })?;
        drop(backup);
        snapshot
            .close()
            .map_err(|(_, error)| restore_failed(format!("could not close SQLite backup: {error}")))
    })();
    snapshot_result?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| restore_failed(format!("could not flush SQLite backup: {error}")))?;
    let destination_parent = PinnedParent::open(objects)
        .map_err(|error| restore_failed(format!("could not pin SQLite backup parent: {error}")))?;
    destination_parent
        .replace_file(temporary.path(), destination.file_name().unwrap())
        .map_err(|error| restore_failed(format!("could not publish SQLite backup: {error}")))?;
    sync_directory(objects).map_err(|error| {
        restore_failed(format!("could not sync SQLite backup directory: {error}"))
    })?;
    let original_hash = hash_file(&destination)?;
    Ok(BackupOperation {
        package_source,
        target,
        backup_kind: BackupKind::File,
        backup_path: Some(relative),
        original_hash: Some(original_hash),
        applied_hash: None,
        applied_state: None,
        applied_database_hash: None,
        readonly: Some(metadata.permissions().readonly()),
        unix_mode: unix_mode(&metadata),
        rollback_progress: RollbackProgress::Pending,
        rollback_quarantine: None,
    })
}

fn backup_sqlite_sidecar(
    package_source: String,
    target: PathBuf,
) -> Result<BackupOperation, RehomeError> {
    let original_hash = match fs::symlink_metadata(&target) {
        Ok(metadata)
            if metadata_is_link_or_reparse(&metadata)
                || !metadata.is_file()
                || raw_file_link_count(&target).map_err(|error| {
                    restore_failed(format!("could not inspect SQLite sidecar links: {error}"))
                })? > 1 =>
        {
            return Err(restore_failed(
                "SQLite sidecar is not a regular unlinked file",
            ));
        }
        Ok(_) => Some(hash_file(&target)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(restore_failed(format!(
                "could not inspect SQLite sidecar: {error}"
            )))
        }
    };
    Ok(BackupOperation {
        package_source,
        target,
        backup_kind: BackupKind::Absent,
        backup_path: None,
        original_hash,
        applied_hash: None,
        applied_state: None,
        applied_database_hash: None,
        readonly: None,
        unix_mode: None,
        rollback_progress: RollbackProgress::Pending,
        rollback_quarantine: None,
    })
}

fn rollback_loaded(
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Result<RollbackReport, RehomeError> {
    if let Err(error) = validate_rollback_inputs(journal) {
        journal.status = RecoveryStatus::RollbackFailed;
        let _ = write_journal(journal_path, journal);
        return Err(error);
    }
    journal.status = RecoveryStatus::RollingBack;
    write_journal(journal_path, journal)?;

    let result = (|| {
        let indices = rollback_order(journal);
        for index in indices {
            rollback_operation(journal_path, journal, index)?;
        }
        verify_original_state(journal)?;
        Ok(journal
            .operations
            .iter()
            .filter(|operation| operation.backup_kind == BackupKind::File)
            .count() as u64)
    })();

    match result {
        Ok(restored_files) => {
            journal.status = RecoveryStatus::RolledBack;
            write_journal(journal_path, journal)?;
            Ok(RollbackReport {
                transaction_id: journal.transaction_id,
                completed_at: timestamp(),
                restored_files,
                success: true,
            })
        }
        Err(error) => {
            journal.status = RecoveryStatus::RollbackFailed;
            let _ = write_journal(journal_path, journal);
            Err(error)
        }
    }
}

fn rollback_operation(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    index: usize,
) -> Result<(), RehomeError> {
    let operation = journal.operations[index].clone();
    if operation.rollback_progress == RollbackProgress::OriginalRestored {
        return verify_original_operation_state(&operation);
    }
    if operation.rollback_progress == RollbackProgress::Pending && operation.applied_state.is_none()
    {
        verify_original_operation_state(&operation)?;
        return record_rollback_progress(
            journal_path,
            journal,
            index,
            RollbackProgress::OriginalRestored,
        );
    }

    if operation.rollback_progress == RollbackProgress::Pending {
        match operation.applied_state {
            Some(AppliedState::Absent) => {
                verify_target_absent(
                    &operation,
                    "rollback conflict: applied target was expected to be absent",
                )?;
                record_rollback_progress(
                    journal_path,
                    journal,
                    index,
                    RollbackProgress::TargetRemoved,
                )?;
            }
            Some(AppliedState::File { .. }) => {
                let quarantine = ensure_rollback_quarantine(journal_path, journal, index)?;
                quarantine_target(journal, &journal.operations[index], &quarantine)?;
                record_rollback_progress(
                    journal_path,
                    journal,
                    index,
                    RollbackProgress::TargetQuarantined,
                )?;
            }
            None => unreachable!(),
        }
    }

    if journal.operations[index].rollback_progress == RollbackProgress::TargetQuarantined {
        let operation = journal.operations[index].clone();
        let quarantine = operation_quarantine(journal, &operation, index)?;
        if quarantine_matches_applied(&operation, &quarantine)? {
            record_rollback_progress(
                journal_path,
                journal,
                index,
                RollbackProgress::QuarantineVerified,
            )?;
        } else {
            restore_unrecognized_quarantine(journal_path, journal, index, &quarantine)?;
            return Err(rollback_failed(format!(
                "rollback conflict: quarantined target hash, identity, or type changed: {}",
                operation.target.display()
            )));
        }
    }

    if journal.operations[index].rollback_progress == RollbackProgress::QuarantineVerified {
        let operation = journal.operations[index].clone();
        let quarantine = operation_quarantine(journal, &operation, index)?;
        if quarantine_exists(&operation, &quarantine)?
            && !quarantine_matches_applied(&operation, &quarantine)?
        {
            record_rollback_progress(
                journal_path,
                journal,
                index,
                RollbackProgress::TargetQuarantined,
            )?;
            restore_unrecognized_quarantine(journal_path, journal, index, &quarantine)?;
            return Err(rollback_failed(format!(
                "rollback conflict: verified quarantine was replaced: {}",
                operation.target.display()
            )));
        }
        // Keep the verified applied state as recovery evidence. Deleting it by
        // pathname would reopen a TOCTOU window where an unrelated concurrent
        // replacement could be removed after the identity check.
        record_rollback_progress(
            journal_path,
            journal,
            index,
            RollbackProgress::TargetRemoved,
        )?;
    }

    let operation = journal.operations[index].clone();
    verify_target_absent(
        &operation,
        "rollback conflict: target is present after its recorded removal",
    )?;
    if operation.backup_kind == BackupKind::File {
        restore_backup_file(journal, &operation)?;
    }
    record_rollback_progress(
        journal_path,
        journal,
        index,
        RollbackProgress::OriginalRestored,
    )
}

fn record_rollback_progress(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    index: usize,
    progress: RollbackProgress,
) -> Result<(), RehomeError> {
    journal.operations[index].rollback_progress = progress;
    write_journal(journal_path, journal)
}

fn ensure_rollback_quarantine(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    index: usize,
) -> Result<String, RehomeError> {
    let expected = rollback_quarantine_name(journal.transaction_id, index);
    match journal.operations[index].rollback_quarantine.as_deref() {
        Some(recorded) if recorded == expected => Ok(expected),
        Some(_) => Err(rollback_failed(
            "transaction journal contains invalid rollback quarantine ownership",
        )),
        None => {
            journal.operations[index].rollback_quarantine = Some(expected.clone());
            write_journal(journal_path, journal)?;
            Ok(expected)
        }
    }
}

fn rollback_quarantine_name(transaction_id: Uuid, index: usize) -> String {
    format!(".codex-rehome-{transaction_id}-{index:08}.rollback")
}

fn operation_quarantine(
    journal: &TransactionJournal,
    operation: &BackupOperation,
    index: usize,
) -> Result<String, RehomeError> {
    let expected = rollback_quarantine_name(journal.transaction_id, index);
    match operation.rollback_quarantine.as_deref() {
        Some(recorded) if recorded == expected => Ok(expected),
        _ => Err(rollback_failed(
            "transaction journal is missing rollback quarantine ownership",
        )),
    }
}

fn quarantine_target(
    journal: &TransactionJournal,
    operation: &BackupOperation,
    quarantine: &str,
) -> Result<(), RehomeError> {
    let root = operation_root_from_journal(journal, &operation.target)?;
    validate_rollback_target_ancestry(root, &operation.target)?;
    let parent = operation
        .target
        .parent()
        .ok_or_else(|| rollback_failed("rollback target has no parent"))?;
    let name = operation
        .target
        .file_name()
        .ok_or_else(|| rollback_failed("rollback target has no file name"))?;
    let quarantine = std::ffi::OsStr::new(quarantine);
    let pinned = PinnedParent::open(parent).map_err(|error| {
        rollback_failed(format!("could not pin rollback target parent: {error}"))
    })?;
    match pinned.rename_child_if_absent(name, quarantine) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound
            ) && pinned.child_exists(quarantine).map_err(|inspect_error| {
                rollback_failed(format!(
                    "could not inspect rollback quarantine: {inspect_error}"
                ))
            })? =>
        {
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(rollback_failed(format!(
            "rollback conflict: expected target and quarantine are missing: {}",
            operation.target.display()
        ))),
        Err(error) => Err(rollback_failed(format!(
            "could not quarantine rollback target {}: {error}",
            operation.target.display()
        ))),
    }
}

fn rollback_order(journal: &TransactionJournal) -> Vec<usize> {
    let mut indices = (0..journal.operations.len()).collect::<Vec<_>>();
    indices.sort_by_key(|index| {
        !journal.operations[*index]
            .package_source
            .starts_with("codex/metadata/sqlite-sidecar")
    });
    indices
}

fn verify_original_operation_state(operation: &BackupOperation) -> Result<(), RehomeError> {
    if operation.backup_kind == BackupKind::Absent {
        return verify_target_absent(
            operation,
            "rollback conflict: target is present after its recorded restoration",
        );
    }
    let expected = operation
        .original_hash
        .as_deref()
        .ok_or_else(|| rollback_failed("file backup has no original hash"))?;
    let (actual, _) = inspect_current_file(operation)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(rollback_failed(format!(
            "rollback conflict: restored original hash changed: {}",
            operation.target.display()
        )))
    }
}

fn verify_target_absent(
    operation: &BackupOperation,
    present_message: &str,
) -> Result<(), RehomeError> {
    match fs::symlink_metadata(&operation.target) {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(rollback_failed(format!(
                "rollback target is not a regular file: {}",
                operation.target.display()
            )))
        }
        Ok(_) => Err(rollback_failed(format!(
            "{present_message}: {}",
            operation.target.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(rollback_failed(format!(
            "could not inspect rollback target {}: {error}",
            operation.target.display()
        ))),
    }
}

fn inspect_current_file(operation: &BackupOperation) -> Result<(String, String), RehomeError> {
    let metadata = fs::symlink_metadata(&operation.target).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            rollback_failed(format!(
                "rollback conflict: expected target is missing: {}",
                operation.target.display()
            ))
        } else {
            rollback_failed(format!(
                "could not inspect rollback target {}: {error}",
                operation.target.display()
            ))
        }
    })?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(rollback_failed(format!(
            "rollback target is not a regular file: {}",
            operation.target.display()
        )));
    }
    let hash = if operation.package_source == "codex/metadata/threads.json" {
        hash_sqlite_database(&operation.target)
    } else {
        hash_file(&operation.target)
    }
    .map_err(|error| rollback_failed(error.message))?;
    let identity =
        file_identity(&operation.target).map_err(|error| rollback_failed(error.message))?;
    Ok((hash, identity))
}

fn validate_rollback_inputs(journal: &TransactionJournal) -> Result<(), RehomeError> {
    validate_journal(journal)?;
    for (index, operation) in journal.operations.iter().enumerate() {
        let root = operation_root_from_journal(journal, &operation.target)?;
        validate_rollback_target_ancestry(root, &operation.target)?;
        if operation.backup_kind == BackupKind::File {
            let backup = backup_file_path(journal, operation)?;
            let expected = operation
                .original_hash
                .as_deref()
                .ok_or_else(|| rollback_failed("file backup has no original hash"))?;
            if !hash_file(&backup)?.eq_ignore_ascii_case(expected) {
                return Err(rollback_failed(
                    "backup object hash does not match its journal",
                ));
            }
        }
        if let Some(quarantine) = operation.rollback_quarantine.as_deref() {
            if quarantine != rollback_quarantine_name(journal.transaction_id, index) {
                return Err(rollback_failed(
                    "transaction journal contains invalid rollback quarantine ownership",
                ));
            }
        }
        if matches!(
            operation.rollback_progress,
            RollbackProgress::TargetQuarantined | RollbackProgress::QuarantineVerified
        ) && operation.rollback_quarantine.is_none()
        {
            return Err(rollback_failed(
                "transaction journal is missing rollback quarantine ownership",
            ));
        }
    }
    Ok(())
}

fn quarantine_exists(operation: &BackupOperation, quarantine: &str) -> Result<bool, RehomeError> {
    let parent = operation
        .target
        .parent()
        .ok_or_else(|| rollback_failed("rollback target has no parent"))?;
    let pinned = PinnedParent::open(parent).map_err(|error| {
        rollback_failed(format!("could not pin rollback target parent: {error}"))
    })?;
    pinned
        .child_exists(std::ffi::OsStr::new(quarantine))
        .map_err(|error| rollback_failed(format!("could not inspect rollback quarantine: {error}")))
}

fn quarantine_matches_applied(
    operation: &BackupOperation,
    quarantine: &str,
) -> Result<bool, RehomeError> {
    let Some(AppliedState::File { hash, identity }) = operation.applied_state.as_ref() else {
        return Ok(false);
    };
    let parent = operation
        .target
        .parent()
        .ok_or_else(|| rollback_failed("rollback target has no parent"))?;
    let pinned = PinnedParent::open(parent).map_err(|error| {
        rollback_failed(format!("could not pin rollback target parent: {error}"))
    })?;
    let quarantine_name = std::ffi::OsStr::new(quarantine);
    let mut file = match pinned.open_file(quarantine_name) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return pinned
                .child_exists(quarantine_name)
                .map(|_| false)
                .map_err(|inspect_error| {
                    rollback_failed(format!(
                        "could not inspect rollback quarantine after open failed ({error}): {inspect_error}"
                    ))
                })
        }
    };
    let metadata = file.metadata().map_err(|error| {
        rollback_failed(format!("could not inspect rollback quarantine: {error}"))
    })?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Ok(false);
    }
    let actual_identity =
        file_identity_from_file(&file).map_err(|error| rollback_failed(error.message))?;
    if actual_identity != *identity {
        return Ok(false);
    }
    let actual_hash = if operation.package_source == "codex/metadata/threads.json" {
        let path = parent.join(quarantine);
        let hash = hash_sqlite_database(&path).map_err(|error| rollback_failed(error.message))?;
        let reopened = pinned.open_file(quarantine_name).map_err(|error| {
            rollback_failed(format!("could not reopen rollback quarantine: {error}"))
        })?;
        if file_identity_from_file(&reopened).map_err(|error| rollback_failed(error.message))?
            != actual_identity
        {
            return Ok(false);
        }
        hash
    } else {
        hash_open_file(&mut file).map_err(|error| rollback_failed(error.message))?
    };
    Ok(actual_hash.eq_ignore_ascii_case(hash))
}

fn restore_unrecognized_quarantine(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    index: usize,
    quarantine: &str,
) -> Result<(), RehomeError> {
    record_rollback_progress(journal_path, journal, index, RollbackProgress::Pending)?;
    let operation = &journal.operations[index];
    let parent = operation
        .target
        .parent()
        .ok_or_else(|| rollback_failed("rollback target has no parent"))?;
    let target_name = operation
        .target
        .file_name()
        .ok_or_else(|| rollback_failed("rollback target has no file name"))?;
    let pinned = PinnedParent::open(parent).map_err(|error| {
        rollback_failed(format!("could not pin rollback target parent: {error}"))
    })?;
    match pinned.rename_child_if_absent(std::ffi::OsStr::new(quarantine), target_name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(rollback_failed(format!(
            "could not restore unrecognized rollback quarantine: {error}"
        ))),
    }
}

fn restore_backup_file(
    journal: &TransactionJournal,
    operation: &BackupOperation,
) -> Result<(), RehomeError> {
    let root = operation_root_from_journal(journal, &operation.target)?;
    validate_rollback_target_ancestry(root, &operation.target)?;
    let parent = operation
        .target
        .parent()
        .ok_or_else(|| rollback_failed("rollback target has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        rollback_failed(format!("could not create rollback directory: {error}"))
    })?;
    validate_rollback_target_ancestry(root, &operation.target)?;
    let pinned = PinnedParent::open(parent).map_err(|error| {
        rollback_failed(format!("could not pin rollback target parent: {error}"))
    })?;
    validate_rollback_target_ancestry(root, &operation.target)?;
    let backup = backup_file_path(journal, operation)?;
    let name = operation
        .target
        .file_name()
        .ok_or_else(|| rollback_failed("rollback target has no file name"))?;
    pinned
        .install_file_if_absent(&backup, name)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                rollback_failed(format!(
                    "rollback conflict: target appeared before original restoration: {}",
                    operation.target.display()
                ))
            } else {
                rollback_failed(format!("could not restore backup atomically: {error}"))
            }
        })?;
    let restored = pinned
        .open_file_for_write(name)
        .map_err(|error| rollback_failed(format!("could not open restored backup: {error}")))?;
    let mut permissions = restored
        .metadata()
        .map_err(|error| rollback_failed(format!("could not inspect restored backup: {error}")))?
        .permissions();
    if let Some(readonly) = operation.readonly {
        permissions.set_readonly(readonly);
    }
    set_unix_mode(&mut permissions, operation.unix_mode);
    restored
        .set_permissions(permissions)
        .map_err(|error| rollback_failed(format!("could not restore file permissions: {error}")))?;
    restored
        .sync_all()
        .map_err(|error| rollback_failed(format!("could not flush restored backup: {error}")))?;
    pinned
        .sync()
        .map_err(|error| rollback_failed(format!("could not flush restored backup: {error}")))
}

fn validate_rollback_target_ancestry(root: &Path, target: &Path) -> Result<(), RehomeError> {
    validate_restore_target_ancestry(root, target).map_err(|error| rollback_failed(error.message))
}

fn verify_original_state(journal: &TransactionJournal) -> Result<(), RehomeError> {
    for operation in &journal.operations {
        verify_original_operation_state(operation)?;
    }
    Ok(())
}

fn load_validated_journal(
    path: &Path,
    expected_id: Option<Uuid>,
) -> Result<TransactionJournal, RehomeError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        rollback_failed(format!("could not inspect transaction journal: {error}"))
    })?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(rollback_failed("transaction journal is not a regular file"));
    }
    let bytes = fs::read(path)
        .map_err(|error| rollback_failed(format!("could not read transaction journal: {error}")))?;
    let journal: TransactionJournal = serde_json::from_slice(&bytes)
        .map_err(|error| rollback_failed(format!("transaction journal is invalid: {error}")))?;
    if expected_id.is_some_and(|expected| journal.transaction_id != expected) {
        return Err(rollback_failed(
            "transaction journal ID does not match its file name",
        ));
    }
    let expected_path = journal_path(journal.transaction_id)?;
    if expected_path != path {
        return Err(rollback_failed(
            "transaction journal is outside application data",
        ));
    }
    validate_journal(&journal)?;
    Ok(journal)
}

fn validate_journal(journal: &TransactionJournal) -> Result<(), RehomeError> {
    if !journal.backup_root.is_absolute()
        || !journal.target_codex_home.is_absolute()
        || !journal.projects_root.is_absolute()
    {
        return Err(rollback_failed(
            "transaction journal contains a relative root",
        ));
    }
    let canonical_backup = fs::canonicalize(&journal.backup_root)
        .map_err(|error| rollback_failed(format!("backup root cannot be resolved: {error}")))?;
    if canonical_backup != journal.backup_root {
        return Err(rollback_failed(
            "backup root changed after the transaction was created",
        ));
    }
    let transaction_backup = canonical_backup.join(journal.transaction_id.to_string());
    let canonical_transaction = fs::canonicalize(&transaction_backup).map_err(|error| {
        rollback_failed(format!("transaction backup cannot be resolved: {error}"))
    })?;
    if !canonical_transaction.starts_with(&canonical_backup) {
        return Err(rollback_failed(
            "transaction backup escapes the backup root",
        ));
    }
    for operation in &journal.operations {
        operation_root_from_journal(journal, &operation.target)?;
        match operation.backup_kind {
            BackupKind::File => {
                let _ = backup_file_path(journal, operation)?;
                if operation.original_hash.is_none() {
                    return Err(rollback_failed("file backup is missing its original hash"));
                }
            }
            BackupKind::Absent
                if operation.backup_path.is_some()
                    || operation.original_hash.is_some()
                        && !operation
                            .package_source
                            .starts_with("codex/metadata/sqlite-sidecar") =>
            {
                return Err(rollback_failed(
                    "absent backup has unexpected file metadata",
                ));
            }
            BackupKind::Absent => {}
        }
    }
    for lock in &journal.locks {
        operation_root_from_journal(journal, &lock.target)?;
        if lock.path != target_lock_path(&lock.target)?
            || lock.token != journal.transaction_id.to_string()
        {
            return Err(rollback_failed(
                "transaction journal contains invalid lock ownership",
            ));
        }
    }
    Ok(())
}

fn target_lock_path(target: &Path) -> Result<PathBuf, RehomeError> {
    let parent = target
        .parent()
        .ok_or_else(|| rollback_failed("restore target has no parent directory"))?;
    let file_name = target
        .file_name()
        .ok_or_else(|| rollback_failed("restore target has no file name"))?
        .to_string_lossy();
    Ok(parent.join(format!(".{file_name}.codex-rehome.lock")))
}

fn remove_owned_stale_locks(journal: &TransactionJournal) -> Result<(), RehomeError> {
    for lock in &journal.locks {
        operation_root_from_journal(journal, &lock.target)?;
        if lock.path != target_lock_path(&lock.target)? {
            return Err(rollback_failed("transaction lock path is unsafe"));
        }
        let parent = lock
            .path
            .parent()
            .ok_or_else(|| rollback_failed("transaction lock has no parent"))?;
        let name = lock
            .path
            .file_name()
            .ok_or_else(|| rollback_failed("transaction lock has no file name"))?;
        let pinned = PinnedParent::open(parent).map_err(|error| {
            rollback_failed(format!("could not pin transaction lock parent: {error}"))
        })?;
        let mut file = match pinned.open_file(name) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(rollback_failed(format!(
                    "could not inspect transaction lock: {error}"
                )))
            }
        };
        let metadata = file.metadata().map_err(|error| {
            rollback_failed(format!("could not inspect transaction lock: {error}"))
        })?;
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
            continue;
        }
        if raw_file_link_count(&lock.path).map_err(|error| {
            rollback_failed(format!("could not inspect transaction lock links: {error}"))
        })? != 1
        {
            continue;
        }
        let mut token = String::new();
        file.read_to_string(&mut token).map_err(|error| {
            rollback_failed(format!("could not read transaction lock: {error}"))
        })?;
        drop(file);
        if token == lock.token {
            pinned.remove_file(name).map_err(|error| {
                rollback_failed(format!("could not remove transaction lock: {error}"))
            })?;
        }
    }
    Ok(())
}

fn backup_file_path(
    journal: &TransactionJournal,
    operation: &BackupOperation,
) -> Result<PathBuf, RehomeError> {
    let relative = operation
        .backup_path
        .as_deref()
        .ok_or_else(|| rollback_failed("file backup has no object path"))?;
    validate_relative_path(relative)?;
    let transaction_root = journal.backup_root.join(journal.transaction_id.to_string());
    let path = transaction_root.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| rollback_failed(format!("could not inspect backup object: {error}")))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(rollback_failed("backup object is not a regular file"));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| rollback_failed(format!("could not resolve backup object: {error}")))?;
    let canonical_root = fs::canonicalize(&transaction_root).map_err(|error| {
        rollback_failed(format!("could not resolve transaction backup: {error}"))
    })?;
    if !canonical.starts_with(&canonical_root) {
        return Err(rollback_failed(
            "backup object escapes the transaction backup",
        ));
    }
    Ok(canonical)
}

fn operation_root<'a>(plan: &'a RestorePlan, target: &Path) -> Result<&'a Path, RehomeError> {
    choose_root(&plan.target_codex_home, &plan.projects_root, target)
}

fn operation_root_from_journal<'a>(
    journal: &'a TransactionJournal,
    target: &Path,
) -> Result<&'a Path, RehomeError> {
    choose_root(&journal.target_codex_home, &journal.projects_root, target)
        .map_err(|error| rollback_failed(error.message))
}

fn choose_root<'a>(
    codex_home: &'a Path,
    projects_root: &'a Path,
    target: &Path,
) -> Result<&'a Path, RehomeError> {
    if target.starts_with(codex_home) {
        Ok(codex_home)
    } else if target.starts_with(projects_root) {
        Ok(projects_root)
    } else {
        Err(restore_failed(format!(
            "restore target escapes the planned roots: {}",
            target.display()
        )))
    }
}

fn validate_relative_path(path: &Path) -> Result<(), RehomeError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(rollback_failed("backup object path is unsafe"));
    }
    Ok(())
}

fn app_data_root() -> Result<PathBuf, RehomeError> {
    create_and_canonicalize_directory(&app_data_root_path()?, "application data directory")
}

fn app_data_root_path() -> Result<PathBuf, RehomeError> {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| {
                let home = PathBuf::from(home);
                if cfg!(target_os = "macos") {
                    home.join("Library").join("Application Support")
                } else {
                    env::var_os("XDG_DATA_HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home.join(".local").join("share"))
                }
            })
        })
        .ok_or_else(|| restore_failed("could not resolve the ReHome application data directory"))?;
    Ok(base.join(APP_IDENTIFIER))
}

fn existing_app_data_root() -> Result<Option<PathBuf>, RehomeError> {
    let path = app_data_root_path()?;
    match fs::canonicalize(path) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(rollback_failed(format!(
            "could not resolve the ReHome application data directory: {error}"
        ))),
    }
}

fn journal_path(transaction_id: Uuid) -> Result<PathBuf, RehomeError> {
    let app_data = existing_app_data_root()?.unwrap_or(app_data_root_path()?);
    Ok(app_data
        .join(TRANSACTIONS_DIRECTORY)
        .join(format!("{transaction_id}.json")))
}

fn create_and_canonicalize_directory(path: &Path, label: &str) -> Result<PathBuf, RehomeError> {
    fs::create_dir_all(path)
        .map_err(|error| restore_failed(format!("could not create {label}: {error}")))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| restore_failed(format!("could not inspect {label}: {error}")))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(restore_failed(format!(
            "{label} is not a regular directory"
        )));
    }
    sync_directory(path)
        .map_err(|error| restore_failed(format!("could not sync {label}: {error}")))?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)
            .map_err(|error| restore_failed(format!("could not sync {label} parent: {error}")))?;
    }
    fs::canonicalize(path)
        .map_err(|error| restore_failed(format!("could not resolve {label}: {error}")))
}

fn validate_directory_entry(path: &Path) -> Result<(), RehomeError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| restore_failed(format!("could not inspect journal directory: {error}")))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        Err(restore_failed("transaction journal directory is unsafe"))
    } else {
        Ok(())
    }
}

fn write_journal(path: &Path, journal: &TransactionJournal) -> Result<(), RehomeError> {
    let parent = path
        .parent()
        .ok_or_else(|| restore_failed("transaction journal has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| restore_failed(format!("could not create journal directory: {error}")))?;
    validate_directory_entry(parent)?;
    let mut bytes = Vec::new();
    serde_json::to_writer_pretty(&mut bytes, journal).map_err(|error| {
        restore_failed(format!("could not encode transaction journal: {error}"))
    })?;
    bytes.push(b'\n');
    let pinned = PinnedParent::open(parent)
        .map_err(|error| restore_failed(format!("could not pin journal directory: {error}")))?;
    pinned
        .replace_bytes(path.file_name().unwrap(), &bytes)
        .map_err(|error| restore_failed(format!("could not atomically write journal: {error}")))?;
    pinned
        .sync()
        .map_err(|error| restore_failed(format!("could not sync journal directory: {error}")))
}

fn copy_file_atomically(source: &Path, destination: &Path) -> Result<(), RehomeError> {
    let parent = destination
        .parent()
        .ok_or_else(|| restore_failed("file destination has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| restore_failed(format!("could not create file directory: {error}")))?;
    let pinned = PinnedParent::open(parent)
        .map_err(|error| restore_failed(format!("could not pin file directory: {error}")))?;
    pinned
        .replace_file(source, destination.file_name().unwrap())
        .map_err(|error| restore_failed(format!("could not publish copied file: {error}")))?;
    pinned
        .sync()
        .map_err(|error| restore_failed(format!("could not sync file directory: {error}")))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, RehomeError> {
    let mut file = fs::File::open(path)
        .map_err(|error| restore_failed(format!("could not open file for hashing: {error}")))?;
    hash_open_file(&mut file)
}

fn hash_open_file(file: &mut fs::File) -> Result<String, RehomeError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| restore_failed(format!("could not hash file: {error}")))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_sqlite_database(path: &Path) -> Result<String, RehomeError> {
    let directory = tempfile::tempdir().map_err(|error| {
        restore_failed(format!("could not create SQLite hash directory: {error}"))
    })?;
    let snapshot_path = directory.path().join("snapshot.sqlite");
    let source = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| {
        restore_failed(format!(
            "could not open SQLite database for hashing: {error}"
        ))
    })?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| {
            restore_failed(format!("could not configure SQLite hash lock: {error}"))
        })?;
    let mut destination = Connection::open(&snapshot_path).map_err(|error| {
        restore_failed(format!("could not open SQLite hash destination: {error}"))
    })?;
    let backup = Backup::new(&source, &mut destination)
        .map_err(|error| restore_failed(format!("could not start SQLite hash backup: {error}")))?;
    backup
        .run_to_completion(128, Duration::from_millis(1), None)
        .map_err(|error| {
            restore_failed(format!("could not complete SQLite hash backup: {error}"))
        })?;
    drop(backup);
    drop(destination);
    hash_file(&snapshot_path)
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

#[cfg(unix)]
fn unix_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.mode())
}

#[cfg(not(unix))]
fn unix_mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_unix_mode(permissions: &mut fs::Permissions, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        permissions.set_mode(mode);
    }
}

#[cfg(not(unix))]
fn set_unix_mode(_permissions: &mut fs::Permissions, _mode: Option<u32>) {}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink()
        || metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
}

#[cfg(not(windows))]
fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn raw_file_link_count(path: &Path) -> io::Result<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(u64::from(information.nNumberOfLinks))
    }
}

#[cfg(windows)]
fn file_identity(path: &Path) -> Result<String, RehomeError> {
    let file = fs::File::open(path)
        .map_err(|error| restore_failed(format!("could not open file identity: {error}")))?;
    file_identity_from_file(&file)
}

#[cfg(windows)]
fn file_identity_from_file(file: &fs::File) -> Result<String, RehomeError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if result == 0 {
        return Err(restore_failed(format!(
            "could not inspect file identity: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(format!(
        "{}:{:08x}{:08x}",
        information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow
    ))
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Result<String, RehomeError> {
    let file = fs::File::open(path)
        .map_err(|error| restore_failed(format!("could not open file identity: {error}")))?;
    file_identity_from_file(&file)
}

#[cfg(unix)]
fn file_identity_from_file(file: &fs::File) -> Result<String, RehomeError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file
        .metadata()
        .map_err(|error| restore_failed(format!("could not inspect file identity: {error}")))?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(unix)]
fn raw_file_link_count(path: &Path) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).map(|metadata| metadata.nlink())
}

#[cfg(not(any(windows, unix)))]
fn raw_file_link_count(path: &Path) -> io::Result<u64> {
    fs::metadata(path).map(|_| 1)
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn restore_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}

fn rollback_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RollbackFailed, message)
}
