use crate::core::{
    backup::managed_backup_root,
    bridge::register_project_with_detected_cli,
    discovery::discover_codex as core_discover_codex,
    error::{ErrorCode, RehomeError},
    models::{
        CodexInventory, ContentCounts, CreatePackageReport, CreatePackageRequest,
        FileConflictResolution, PackagePreview, RecoveryStatus, RegistrationStatus, RestoreOptions,
        RestorePlan, RestoreReport, RollbackReport, SourceOs, TargetInventory, TransactionHistory,
        TransactionSummary,
    },
    package::{
        create_package_replacing as core_create_package_replacing,
        inspect_package as core_inspect_package,
    },
    planner::build_restore_plan_with_conflict_resolution as core_build_restore_plan,
    restore::{
        apply_restore_by_id_observed, list_transaction_history as core_list_transaction_history,
        rollback as core_rollback, transaction_summary as core_transaction_summary,
    },
};
use crate::support::{self, models::*, SupportService};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf, Prefix},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

const GRANT_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePackageSelection {
    pub project_ids: Vec<Uuid>,
    pub conversation_ids: Vec<Uuid>,
    pub skill_ids: Vec<Uuid>,
    pub plugin_ids: Vec<Uuid>,
    pub generated_image_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedPackage {
    #[serde(flatten)]
    pub report: CreatePackageReport,
    pub archive_hash: String,
    pub reveal_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectedPackage {
    pub selection_id: Uuid,
    #[serde(flatten)]
    pub preview: PackagePreview,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum BuildRestorePlanRequest {
    SelectDestinations {
        package_selection_id: Uuid,
    },
    Build {
        package_selection_id: Uuid,
        destination_selection_id: Uuid,
        conflict_resolution: Option<FileConflictResolution>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BuildRestorePlanResponse {
    Destinations {
        selection_id: Uuid,
        target_codex_home: PathBuf,
        projects_root: PathBuf,
        backup_root: PathBuf,
    },
    Plan {
        plan: RestorePlan,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyRestoreSelection {
    pub plan_id: Uuid,
    pub codex_closed_confirmed: bool,
    pub register_projects: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollbackAction {
    Rollback,
    Resume,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackSelection {
    pub transaction_id: Uuid,
    pub action: RollbackAction,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpenPathSelection {
    Granted { object_id: Uuid },
    Transaction { path: PathBuf, transaction_id: Uuid },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRestoredThreadSelection {
    pub path: PathBuf,
    pub transaction_id: Uuid,
}

#[derive(Clone, Default)]
pub struct WorkflowState {
    inner: Arc<Mutex<WorkflowGrants>>,
    support: SupportService,
}

#[derive(Default)]
struct WorkflowGrants {
    packages: HashMap<Uuid, Timed<PackageGrant>>,
    reveal_paths: HashMap<Uuid, Timed<PathBuf>>,
    restore_locations: HashMap<Uuid, Timed<RestoreLocationGrant>>,
    plans: HashMap<Uuid, Timed<RestorePlanGrant>>,
    rollbacks_in_flight: HashSet<Uuid>,
}

struct Timed<T> {
    value: T,
    expires_at: Instant,
}

struct RestoreLocationGrant {
    package_selection_id: Uuid,
    projects_root: PathBuf,
    backup_root: PathBuf,
}

#[derive(Clone)]
struct PackageGrant {
    path: PathBuf,
    archive_hash: String,
    file_identity: String,
}

struct RestorePlanGrant {
    backup_root: PathBuf,
    state: GrantState,
    package_facts: Option<(SourceOs, u32, ContentCounts)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GrantState {
    Available,
    InFlight,
}

pub(crate) struct PlanClaim {
    workflow: WorkflowState,
    plan_id: Uuid,
    pub(crate) backup_root: PathBuf,
    finished: bool,
    package_facts: Option<(SourceOs, u32, ContentCounts)>,
}

impl PlanClaim {
    pub(crate) fn restore_available(mut self) {
        self.workflow.finish_plan(self.plan_id, true);
        self.finished = true;
    }
}

impl Drop for PlanClaim {
    fn drop(&mut self) {
        if !self.finished {
            self.workflow.finish_plan(self.plan_id, false);
        }
    }
}

pub(crate) struct RollbackClaim {
    workflow: WorkflowState,
    transaction_id: Uuid,
}

impl Drop for RollbackClaim {
    fn drop(&mut self) {
        self.workflow
            .grants()
            .rollbacks_in_flight
            .remove(&self.transaction_id);
    }
}

impl WorkflowState {
    fn grant_reveal_path(&self, path: PathBuf) -> Uuid {
        let id = Uuid::new_v4();
        self.grants().reveal_paths.insert(id, timed(path));
        id
    }

    fn resolve_granted_path(&self, id: Uuid) -> Result<PathBuf, RehomeError> {
        let mut grants = self.grants();
        grants.prune();
        if let Some(grant) = grants.packages.get(&id) {
            validate_package_file_identity(&grant.value)?;
            return Ok(grant.value.path.clone());
        }
        let granted = grants.reveal_paths.get(&id).ok_or_else(|| {
            selection_failed(
                ErrorCode::RestoreFailed,
                "file location permission expired or was not found",
            )
        })?;
        let canonical = canonical_existing_file(&granted.value)?;
        if canonical != granted.value {
            return Err(open_failed("granted file path changed"));
        }
        Ok(canonical)
    }

    pub(crate) fn grant_inspected_package(
        &self,
        path: PathBuf,
        archive_hash: String,
    ) -> Result<Uuid, RehomeError> {
        let file_identity = package_file_identity(&path)?;
        let id = Uuid::new_v4();
        self.grants().packages.insert(
            id,
            timed(PackageGrant {
                path,
                archive_hash,
                file_identity,
            }),
        );
        Ok(id)
    }

    pub(crate) fn resolve_package(&self, id: Uuid) -> Result<PathBuf, RehomeError> {
        let mut grants = self.grants();
        grants.prune();
        let grant = grants.packages.get(&id).ok_or_else(|| {
            selection_failed(
                ErrorCode::PackageInvalid,
                "package selection expired or was not found",
            )
        })?;
        validate_package_file_identity(&grant.value)?;
        Ok(grant.value.path.clone())
    }

    pub(crate) fn validate_package_grant(
        &self,
        id: Uuid,
        archive_hash: &str,
    ) -> Result<PathBuf, RehomeError> {
        let mut grants = self.grants();
        grants.prune();
        let grant = grants.packages.get(&id).ok_or_else(|| {
            selection_failed(
                ErrorCode::PackageInvalid,
                "package selection expired or was not found",
            )
        })?;
        validate_package_file_identity(&grant.value)?;
        if !grant.value.archive_hash.eq_ignore_ascii_case(archive_hash) {
            return Err(selection_failed(
                ErrorCode::PackageInvalid,
                "selected package archive hash changed after inspection",
            ));
        }
        Ok(grant.value.path.clone())
    }

    pub(crate) fn grant_restore_locations(
        &self,
        package_selection_id: Uuid,
        projects_root: PathBuf,
        backup_root: PathBuf,
    ) -> Uuid {
        let id = Uuid::new_v4();
        self.grants().restore_locations.insert(
            id,
            timed(RestoreLocationGrant {
                package_selection_id,
                projects_root,
                backup_root,
            }),
        );
        id
    }

    pub(crate) fn resolve_restore_locations(
        &self,
        package_selection_id: Uuid,
        id: Uuid,
    ) -> Result<(PathBuf, PathBuf), RehomeError> {
        let mut grants = self.grants();
        grants.prune();
        let grant = grants.restore_locations.get(&id).ok_or_else(|| {
            selection_failed(
                ErrorCode::RestoreFailed,
                "restore location selection expired or was not found",
            )
        })?;
        if grant.value.package_selection_id != package_selection_id {
            return Err(selection_failed(
                ErrorCode::RestoreFailed,
                "restore locations do not belong to the selected package",
            ));
        }
        Ok((
            grant.value.projects_root.clone(),
            grant.value.backup_root.clone(),
        ))
    }

    pub(crate) fn grant_plan(
        &self,
        plan_id: Uuid,
        backup_root: PathBuf,
    ) -> Result<(), RehomeError> {
        let mut grants = self.grants();
        grants.prune();
        if grants
            .plans
            .get(&plan_id)
            .is_some_and(|grant| grant.value.state == GrantState::InFlight)
        {
            return Err(selection_failed(
                ErrorCode::RestoreFailed,
                "restore plan is already being applied",
            ));
        }
        grants.plans.insert(
            plan_id,
            timed(RestorePlanGrant {
                backup_root,
                state: GrantState::Available,
                package_facts: None,
            }),
        );
        Ok(())
    }

    pub(crate) fn claim_plan(&self, plan_id: Uuid) -> Result<PlanClaim, RehomeError> {
        let mut grants = self.grants();
        grants.prune();
        let grant = grants.plans.get_mut(&plan_id).ok_or_else(|| {
            selection_failed(
                ErrorCode::RestoreFailed,
                "restore plan capability expired or was not found",
            )
        })?;
        if grant.value.state != GrantState::Available {
            return Err(selection_failed(
                ErrorCode::RestoreFailed,
                "restore plan is already being applied",
            ));
        }
        grant.value.state = GrantState::InFlight;
        Ok(PlanClaim {
            workflow: self.clone(),
            plan_id,
            backup_root: grant.value.backup_root.clone(),
            finished: false,
            package_facts: grant.value.package_facts.clone(),
        })
    }

    fn finish_plan(&self, plan_id: Uuid, restore_available: bool) {
        let mut grants = self.grants();
        if restore_available {
            if let Some(grant) = grants.plans.get_mut(&plan_id) {
                if grant.value.state == GrantState::InFlight {
                    grant.value.state = GrantState::Available;
                    grant.expires_at = Instant::now() + GRANT_TTL;
                }
            }
        } else {
            grants.plans.remove(&plan_id);
        }
    }

    pub(crate) fn claim_rollback(
        &self,
        transaction_id: Uuid,
    ) -> Result<RollbackClaim, RehomeError> {
        let mut grants = self.grants();
        if !grants.rollbacks_in_flight.insert(transaction_id) {
            return Err(selection_failed(
                ErrorCode::RollbackFailed,
                "transaction rollback is already in progress",
            ));
        }
        Ok(RollbackClaim {
            workflow: self.clone(),
            transaction_id,
        })
    }

    fn grants(&self) -> std::sync::MutexGuard<'_, WorkflowGrants> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl WorkflowGrants {
    fn prune(&mut self) {
        let now = Instant::now();
        self.packages.retain(|_, grant| grant.expires_at > now);
        self.reveal_paths.retain(|_, grant| grant.expires_at > now);
        self.restore_locations
            .retain(|_, grant| grant.expires_at > now);
        self.plans
            .retain(|_, grant| grant.value.state == GrantState::InFlight || grant.expires_at > now);
    }
}

fn timed<T>(value: T) -> Timed<T> {
    Timed {
        value,
        expires_at: Instant::now() + GRANT_TTL,
    }
}

#[tauri::command]
pub async fn discover_codex(
    state: State<'_, WorkflowState>,
) -> Result<CodexInventory, RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::CodexNotFound, move || {
        let mut inventory = core_discover_codex(None)?;
        for image in &mut inventory.generated_images {
            let canonical = canonical_existing_file(&image.source_path)?;
            image.reveal_id = Some(state.grant_reveal_path(canonical));
        }
        Ok(inventory)
    })
    .await
}

#[tauri::command]
pub async fn create_package(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, WorkflowState>,
    selection: CreatePackageSelection,
) -> Result<Option<CreatedPackage>, SupportFailure> {
    let state = state.inner().clone();
    run_supported(Stage::Export, state.support.clone(), move |snapshot| {
        let Some(selected) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("保存 ReHome 包")
            .set_file_name("handoff.rehome")
            .add_filter("ReHome 包", &["rehome"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let output_path = canonical_save_path(selected)?;
        snapshot.package_path = Some(output_path.clone());
        let inventory = core_discover_codex(None)?;
        snapshot.codex_home = Some(inventory.codex_home.clone());
        snapshot.source_os = Some(inventory.source_os);
        let request = resolve_create_package_request(&inventory, selection, output_path)?;
        // The native save dialog only returns an existing filename after the user confirms replace.
        let report = core_create_package_replacing(request)?;
        let preview = core_inspect_package(&report.package_path)?;
        let canonical = canonical_existing_file(&report.package_path)?;
        let reveal_id = state.grant_inspected_package(canonical, preview.archive_hash.clone())?;
        Ok(Some(CreatedPackage {
            report,
            archive_hash: preview.archive_hash,
            reveal_id,
        }))
    })
    .await
}

#[tauri::command]
pub async fn inspect_package(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, WorkflowState>,
) -> Result<Option<InspectedPackage>, SupportFailure> {
    let state = state.inner().clone();
    run_supported(Stage::Inspect, state.support.clone(), move |snapshot| {
        let Some(selected) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择 ReHome 包")
            .add_filter("ReHome 包", &["rehome"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = canonical_existing_file(&selected_path(selected)?)?;
        snapshot.package_path = Some(path.clone());
        if !has_rehome_extension(&path) {
            return Err(selection_failed(
                ErrorCode::PackageInvalid,
                "selected package must use the .rehome extension",
            ));
        }
        let preview = core_inspect_package(&path)?;
        support::capture_package(snapshot, &preview);
        let selection_id = state.grant_inspected_package(path, preview.archive_hash.clone())?;
        Ok(Some(InspectedPackage {
            selection_id,
            preview,
        }))
    })
    .await
}

#[tauri::command]
pub async fn build_restore_plan(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, WorkflowState>,
    request: BuildRestorePlanRequest,
) -> Result<Option<BuildRestorePlanResponse>, SupportFailure> {
    let state = state.inner().clone();
    run_supported(
        Stage::Plan,
        state.support.clone(),
        move |snapshot| match request {
            BuildRestorePlanRequest::SelectDestinations {
                package_selection_id,
            } => {
                snapshot.stage = Stage::Destinations;
                let package_path = state.resolve_package(package_selection_id)?;
                let package = core_inspect_package(&package_path)?;
                support::capture_package(snapshot, &package);
                state.validate_package_grant(package_selection_id, &package.archive_hash)?;
                let inventory = core_discover_codex(None)?;
                snapshot.codex_home = Some(inventory.codex_home.clone());
                let projects_root = if package.manifest.projects.is_empty() {
                    default_unused_projects_root(&inventory.codex_home)?
                } else {
                    let Some(projects) = app
                        .dialog()
                        .file()
                        .set_parent(&window)
                        .set_title("选择项目目录")
                        .blocking_pick_folder()
                    else {
                        return Ok(None);
                    };
                    canonical_existing_directory(&selected_path(projects)?)?
                };
                let backup_root = managed_backup_root()?;
                validate_restore_location_separation(&projects_root, &backup_root)?;
                let selection_id = state.grant_restore_locations(
                    package_selection_id,
                    projects_root.clone(),
                    backup_root.clone(),
                );
                Ok(Some(BuildRestorePlanResponse::Destinations {
                    selection_id,
                    target_codex_home: inventory.codex_home,
                    projects_root,
                    backup_root,
                }))
            }
            BuildRestorePlanRequest::Build {
                package_selection_id,
                destination_selection_id,
                conflict_resolution,
            } => {
                let package_path = state.resolve_package(package_selection_id)?;
                let (projects_root, backup_root) = state
                    .resolve_restore_locations(package_selection_id, destination_selection_id)?;
                let package = core_inspect_package(&package_path)?;
                support::capture_package(snapshot, &package);
                state.validate_package_grant(package_selection_id, &package.archive_hash)?;
                let inventory = core_discover_codex(None)?;
                snapshot.codex_home = Some(inventory.codex_home.clone());
                let target = TargetInventory {
                    codex_home: inventory.codex_home,
                    target_os: inventory.source_os,
                    target_arch: inventory.source_arch,
                    counts: inventory.counts,
                    projects: inventory.projects,
                    conversations: inventory.conversations,
                };
                let plan = core_build_restore_plan(
                    &package,
                    &target,
                    &projects_root,
                    conflict_resolution,
                )?;
                state.grant_plan(plan.plan_id, backup_root)?;
                if let Some(grant) = state.grants().plans.get_mut(&plan.plan_id) {
                    grant.value.package_facts = Some((
                        package.manifest.source_os,
                        package.manifest.schema_version,
                        package.manifest.counts.clone(),
                    ));
                }
                Ok(Some(BuildRestorePlanResponse::Plan { plan }))
            }
        },
    )
    .await
}

#[tauri::command]
pub async fn apply_restore(
    state: State<'_, WorkflowState>,
    selection: ApplyRestoreSelection,
) -> Result<RestoreReport, SupportFailure> {
    let state = state.inner().clone();
    run_supported(Stage::Apply, state.support.clone(), move |snapshot| {
        let claim = state.claim_plan(selection.plan_id)?;
        if let Some((source_os, schema, counts)) = &claim.package_facts {
            snapshot.source_os = Some(*source_os);
            snapshot.package_schema = Some(*schema);
            snapshot.counts = Some(counts.clone());
        }
        if let Ok(plan) = crate::core::plan_store::load(selection.plan_id) {
            support::capture_plan(snapshot, &plan);
        }
        let result = apply_restore_by_id_observed(
            selection.plan_id,
            RestoreOptions {
                codex_closed_confirmed: selection.codex_closed_confirmed,
                backup_root: claim.backup_root.clone(),
                register_projects: selection.register_projects,
            },
            |id, status| {
                snapshot.transaction_id = Some(id);
                snapshot.transaction_status = Some(status);
            },
        );
        if let Ok(report) = &result {
            snapshot.verification_at_import = Some(report.verification.clone());
            if !report.verification.app_registration_valid && !report.registrations.is_empty() {
                snapshot.error_code = Some(ErrorCode::RegistrationIncomplete);
            }
        }
        match result {
            Err(error) if error.code == ErrorCode::CodexRunning => {
                claim.restore_available();
                Err(error)
            }
            result => result,
        }
    })
    .await
}

async fn run_supported<T, F>(
    stage: Stage,
    service: SupportService,
    operation: F,
) -> Result<T, SupportFailure>
where
    T: Send + 'static,
    F: FnOnce(&mut SupportSnapshot) -> Result<T, RehomeError> + Send + 'static,
{
    let fallback = SupportSnapshot::new(stage);
    let result = run_blocking(ErrorCode::RestoreFailed, move || {
        let mut snapshot = SupportSnapshot::new(stage);
        let result = operation(&mut snapshot);
        Ok((result, snapshot))
    })
    .await;
    let (result, mut snapshot) = match result {
        Ok(pair) => pair,
        Err(error) => (Err(error), fallback),
    };
    if let Err(error) = &result {
        snapshot.record_error(error);
    }
    let snapshot = service.refresh(snapshot);
    let id = snapshot.support_id;
    if result.is_err() || snapshot.transaction_id.is_some() {
        service.record(snapshot);
    }
    result.map_err(|error| SupportFailure {
        error,
        support_id: id,
    })
}

#[tauri::command]
pub async fn prepare_support(
    state: State<'_, WorkflowState>,
    selection: PrepareSelection,
) -> Result<SupportPreview, RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::RestoreFailed, move || {
        let mut snapshot = match selection.source {
            SupportSource::Incident { support_id } => state.support.incident(support_id)?,
            SupportSource::Transaction { transaction_id } => state
                .support
                .transaction(&open_transaction_by_id(transaction_id)?),
        };
        snapshot = state.support.refresh(snapshot);
        if let Some(note) = selection.user_note {
            snapshot.user_note = Some(support::render::private_excerpt(
                &support::render::truncate(&note, 2048),
            ));
        }
        state.support.record(snapshot.clone());
        let (mut preview, path) = state.support.preview(&snapshot, selection.locale);
        preview.reveal_id = path
            .and_then(|p| fs::canonicalize(p).ok())
            .map(|p| state.grant_reveal_path(p));
        Ok(preview)
    })
    .await
}

#[tauri::command]
pub async fn copy_support_text(
    app: AppHandle,
    state: State<'_, WorkflowState>,
    selection: CopySelection,
) -> Result<(), RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::RestoreFailed, move || {
        let snapshot = state.support.incident(selection.support_id)?;
        let (preview, _) = state.support.preview(&snapshot, selection.locale);
        let text = match selection.kind {
            TextKind::Codex => preview.codex_text,
            TextKind::Github => preview.github_text,
        };
        app.clipboard()
            .write_text(text)
            .map_err(|_| open_failed("clipboard unavailable; select and copy the preview text"))
    })
    .await
}

#[tauri::command]
pub async fn open_support_issue(
    app: AppHandle,
    state: State<'_, WorkflowState>,
    selection: IssueSelection,
) -> Result<IssueOpenResult, RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::RestoreFailed, move || {
        let snapshot = state.support.incident(selection.support_id)?;
        let url = support::render::issue_url(&snapshot, selection.locale);
        app.opener()
            .open_url(
                url.as_deref().unwrap_or(support::render::ISSUE_URL),
                None::<&str>,
            )
            .map_err(|_| {
                open_failed("browser unavailable; copy the public summary and open GitHub manually")
            })?;
        Ok(if url.is_some() {
            IssueOpenResult::Opened
        } else {
            IssueOpenResult::CopyRequired
        })
    })
    .await
}

#[tauri::command]
pub async fn recheck_support(
    state: State<'_, WorkflowState>,
    selection: RecheckSelection,
) -> Result<RecheckReport, RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::RestoreFailed, move || {
        let snapshot = state
            .support
            .refresh(state.support.incident(selection.support_id)?);
        Ok(support::recheck::run(&snapshot))
    })
    .await
}

#[tauri::command]
pub async fn list_transactions() -> Result<TransactionHistory, RehomeError> {
    run_blocking(ErrorCode::RollbackFailed, core_list_transaction_history).await
}

#[tauri::command]
pub async fn rollback_transaction(
    state: State<'_, WorkflowState>,
    selection: RollbackSelection,
) -> Result<RollbackReport, RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::RollbackFailed, move || {
        ensure_codex_desktop_is_closed()?;
        let _claim = state.claim_rollback(selection.transaction_id)?;
        let transaction = rollback_transaction_by_id(selection.transaction_id)?;
        validate_rollback_action(transaction.status, selection.action)?;
        core_rollback(selection.transaction_id)
    })
    .await
}

#[tauri::command]
pub async fn open_path(
    app: AppHandle,
    state: State<'_, WorkflowState>,
    selection: OpenPathSelection,
) -> Result<(), RehomeError> {
    let state = state.inner().clone();
    run_blocking(ErrorCode::RestoreFailed, move || {
        let canonical = match selection {
            OpenPathSelection::Granted { object_id } => {
                let granted = state.resolve_granted_path(object_id)?;
                let canonical = canonical_existing(&granted)?;
                if canonical != granted {
                    return Err(open_failed("granted package path changed"));
                }
                canonical
            }
            OpenPathSelection::Transaction {
                path,
                transaction_id,
            } => authorize_open_path(&path, transaction_id, false)?,
        };
        app.opener()
            .reveal_item_in_dir(canonical)
            .map_err(|error| open_failed(format!("could not reveal path: {error}")))
    })
    .await
}

#[tauri::command]
pub async fn open_restored_thread(
    selection: OpenRestoredThreadSelection,
) -> Result<RegistrationStatus, RehomeError> {
    run_blocking(ErrorCode::RegistrationIncomplete, move || {
        let canonical = authorize_open_path(&selection.path, selection.transaction_id, true)?;
        Ok(register_project_with_detected_cli(
            current_source_os(),
            &canonical,
        ))
    })
    .await
}

pub(crate) fn resolve_create_package_request(
    inventory: &CodexInventory,
    selection: CreatePackageSelection,
    output_path: PathBuf,
) -> Result<CreatePackageRequest, RehomeError> {
    let selected_projects = selection
        .project_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    if selected_projects.len() != selection.project_ids.len() {
        return Err(selection_failed(
            ErrorCode::ProjectConflict,
            "project selection contains duplicates",
        ));
    }
    let projects_by_id = inventory
        .projects
        .iter()
        .map(|project| (project.project_id, project))
        .collect::<HashMap<_, _>>();
    let project_paths = selection
        .project_ids
        .iter()
        .map(|project_id| {
            projects_by_id
                .get(project_id)
                .and_then(|project| {
                    project
                        .source_available
                        .then(|| PathBuf::from(&project.source_path))
                })
                .ok_or_else(|| {
                    selection_failed(
                        ErrorCode::ProjectConflict,
                        format!(
                            "selected project {project_id} is missing or is not available in fresh discovery; rescan and select its conversations instead"
                        ),
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let conversations_by_id = inventory
        .conversations
        .iter()
        .map(|conversation| (conversation.task_id, conversation))
        .collect::<HashMap<_, _>>();
    let mut seen_conversations = HashSet::new();
    for conversation_id in &selection.conversation_ids {
        if !seen_conversations.insert(*conversation_id) {
            return Err(selection_failed(
                ErrorCode::ProjectConflict,
                "conversation selection contains duplicates",
            ));
        }
        conversations_by_id.get(conversation_id).ok_or_else(|| {
            selection_failed(
                ErrorCode::ProjectConflict,
                format!("selected conversation {conversation_id} is not in fresh discovery"),
            )
        })?;
    }

    Ok(CreatePackageRequest {
        codex_home: inventory.codex_home.clone(),
        project_paths,
        conversation_ids: selection.conversation_ids,
        output_path,
        source_device_id: inventory.source_device_id,
        skill_paths: resolve_optional_paths(&selection.skill_ids, &inventory.skills, "skill")?,
        plugin_paths: resolve_optional_paths(&selection.plugin_ids, &inventory.plugins, "plugin")?,
        generated_image_paths: resolve_optional_paths(
            &selection.generated_image_ids,
            &inventory.generated_images,
            "generated image",
        )?,
    })
}

fn resolve_optional_paths(
    selected_ids: &[Uuid],
    entries: &[crate::core::models::OptionalContentEntry],
    kind: &str,
) -> Result<Vec<PathBuf>, RehomeError> {
    let available = entries
        .iter()
        .map(|entry| (entry.content_id, &entry.source_path))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    selected_ids
        .iter()
        .map(|id| {
            if !seen.insert(*id) {
                return Err(selection_failed(
                    ErrorCode::ProjectConflict,
                    format!("{kind} selection contains duplicates"),
                ));
            }
            available
                .get(id)
                .map(|path| (*path).clone())
                .ok_or_else(|| {
                    selection_failed(
                        ErrorCode::ProjectConflict,
                        format!("selected {kind} {id} is not in fresh discovery"),
                    )
                })
        })
        .collect()
}

pub(crate) fn validate_rollback_action(
    status: RecoveryStatus,
    action: RollbackAction,
) -> Result<(), RehomeError> {
    let valid = match action {
        RollbackAction::Rollback => status == RecoveryStatus::Committed,
        RollbackAction::Resume => matches!(
            status,
            RecoveryStatus::Prepared
                | RecoveryStatus::Applying
                | RecoveryStatus::Verifying
                | RecoveryStatus::RollingBack
                | RecoveryStatus::RollbackFailed
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(selection_failed(
            ErrorCode::RollbackFailed,
            "rollback action does not match the transaction status",
        ))
    }
}

pub(crate) fn rollback_transaction_by_id(
    transaction_id: Uuid,
) -> Result<TransactionSummary, RehomeError> {
    core_transaction_summary(transaction_id)?.ok_or_else(|| {
        selection_failed(
            ErrorCode::RollbackFailed,
            "transaction was not found for rollback",
        )
    })
}

pub(crate) fn open_transaction_by_id(
    transaction_id: Uuid,
) -> Result<TransactionSummary, RehomeError> {
    core_transaction_summary(transaction_id)
        .map_err(|error| open_failed(error.message))?
        .ok_or_else(|| open_failed("transaction was not found for open operation"))
}

fn authorize_open_path(
    path: &Path,
    transaction_id: Uuid,
    restored_only: bool,
) -> Result<PathBuf, RehomeError> {
    let canonical = canonical_existing(path)?;
    let transaction = open_transaction_by_id(transaction_id)?;
    authorize_transaction_path(&canonical, &transaction, restored_only)?;
    Ok(canonical)
}

pub(crate) fn authorize_transaction_path(
    canonical: &Path,
    transaction: &TransactionSummary,
    restored_only: bool,
) -> Result<(), RehomeError> {
    let exact_restored_project = transaction.restored_project_paths.iter().any(|path| {
        fs::canonicalize(path).is_ok_and(|canonical_project| canonical_project == canonical)
    });
    let exact_transaction_backup = !restored_only
        && fs::canonicalize(&transaction.transaction_backup_path)
            .is_ok_and(|canonical_backup| canonical_backup == canonical);

    if exact_restored_project || exact_transaction_backup {
        Ok(())
    } else {
        Err(open_failed(
            "path is not an exact object owned by the selected transaction",
        ))
    }
}

fn canonical_save_path(selected: FilePath) -> Result<PathBuf, RehomeError> {
    let mut path = selected_path(selected)?;
    if !has_rehome_extension(&path) {
        path.set_extension("rehome");
    }
    validate_local_dialog_path(&path)?;
    let parent = path
        .parent()
        .ok_or_else(|| selection_failed(ErrorCode::PackageInvalid, "save path has no parent"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| selection_failed(ErrorCode::PackageInvalid, "save path has no file name"))?;
    let parent = canonical_existing_directory(parent)?;
    let output = parent.join(file_name);
    validate_local_dialog_path(&output)?;
    Ok(output)
}

fn selected_path(selected: FilePath) -> Result<PathBuf, RehomeError> {
    selected.into_path().map_err(|error| {
        selection_failed(
            ErrorCode::RestoreFailed,
            format!("native selection is not a local filesystem path: {error}"),
        )
    })
}

pub(crate) fn validate_local_dialog_path(path: &Path) -> Result<(), RehomeError> {
    let supported = if !path.is_absolute() {
        false
    } else {
        match path.components().next() {
            Some(Component::Prefix(prefix)) => {
                matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
            }
            Some(Component::RootDir) => true,
            _ => false,
        }
    };
    if !supported {
        return Err(selection_failed(
            ErrorCode::RestoreFailed,
            "native selection must be an absolute local path",
        ));
    }
    Ok(())
}

fn canonical_existing_file(path: &Path) -> Result<PathBuf, RehomeError> {
    let canonical = canonical_existing(path)?;
    if !canonical.is_file() {
        return Err(selection_failed(
            ErrorCode::PackageInvalid,
            "selected path is not a regular file",
        ));
    }
    Ok(canonical)
}

fn canonical_existing_directory(path: &Path) -> Result<PathBuf, RehomeError> {
    validate_local_dialog_path(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        selection_failed(
            ErrorCode::RestoreFailed,
            format!("could not inspect selected directory: {error}"),
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(selection_failed(
            ErrorCode::RestoreFailed,
            "selected path is not a regular directory",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|error| {
        selection_failed(
            ErrorCode::RestoreFailed,
            format!("could not canonicalize selected directory: {error}"),
        )
    })?;
    validate_local_dialog_path(&canonical)?;
    Ok(canonical)
}

fn validate_restore_location_separation(
    projects_root: &Path,
    backup_root: &Path,
) -> Result<(), RehomeError> {
    if projects_root.starts_with(backup_root) || backup_root.starts_with(projects_root) {
        return Err(selection_failed(
            ErrorCode::RestoreFailed,
            "项目目录和事务备份目录必须是两个互不包含的目录",
        ));
    }
    Ok(())
}

fn default_unused_projects_root(codex_home: &Path) -> Result<PathBuf, RehomeError> {
    let parent = codex_home.parent().ok_or_else(|| {
        selection_failed(
            ErrorCode::RestoreFailed,
            "could not derive an unused project location from the Codex data path",
        )
    })?;
    Ok(parent.join("ReHome Projects"))
}

fn canonical_existing(path: &Path) -> Result<PathBuf, RehomeError> {
    validate_local_dialog_path(path)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| open_failed(format!("could not inspect path: {error}")))?;
    if (!metadata.is_file() && !metadata.is_dir()) || metadata.file_type().is_symlink() {
        return Err(open_failed("path is not a regular file or directory"));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| open_failed(format!("could not canonicalize path: {error}")))?;
    validate_local_dialog_path(&canonical)?;
    Ok(canonical)
}

fn validate_package_file_identity(grant: &PackageGrant) -> Result<(), RehomeError> {
    let current = package_file_identity(&grant.path)?;
    if current != grant.file_identity {
        return Err(selection_failed(
            ErrorCode::PackageInvalid,
            "selected package file identity changed after inspection",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn package_file_identity(path: &Path) -> Result<String, RehomeError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = fs::File::open(path).map_err(|error| {
        selection_failed(
            ErrorCode::PackageInvalid,
            format!("could not open selected package identity: {error}"),
        )
    })?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if result == 0 {
        return Err(selection_failed(
            ErrorCode::PackageInvalid,
            format!(
                "could not inspect selected package identity: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    Ok(format!(
        "{}:{:08x}{:08x}",
        information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow
    ))
}

#[cfg(unix)]
fn package_file_identity(path: &Path) -> Result<String, RehomeError> {
    use std::os::unix::fs::MetadataExt;

    let file = fs::File::open(path).map_err(|error| {
        selection_failed(
            ErrorCode::PackageInvalid,
            format!("could not open selected package identity: {error}"),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        selection_failed(
            ErrorCode::PackageInvalid,
            format!("could not inspect selected package identity: {error}"),
        )
    })?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(not(any(windows, unix)))]
fn package_file_identity(path: &Path) -> Result<String, RehomeError> {
    let metadata = fs::metadata(path).map_err(|error| {
        selection_failed(
            ErrorCode::PackageInvalid,
            format!("could not inspect selected package identity: {error}"),
        )
    })?;
    Ok(format!("{}:{:?}", metadata.len(), metadata.modified().ok()))
}

fn has_rehome_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rehome"))
}

async fn run_blocking<T, F>(code: ErrorCode, operation: F) -> Result<T, RehomeError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, RehomeError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| selection_failed(code, format!("background operation failed: {error}")))?
}

fn current_source_os() -> SourceOs {
    if cfg!(target_os = "macos") {
        SourceOs::Macos
    } else {
        SourceOs::Windows
    }
}

fn selection_failed(code: ErrorCode, message: impl Into<String>) -> RehomeError {
    RehomeError::new(code, message)
}

fn open_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}

#[cfg(test)]
mod support_workflow_tests {
    use super::*;
    #[test]
    fn help_storage_failure_preserves_original_error_and_memory_context() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("not-a-directory");
        fs::write(&blocked, "keep").unwrap();
        let service = SupportService::with_root(blocked);
        let result =
            tauri::async_runtime::block_on(run_supported(Stage::Inspect, service.clone(), |_| {
                Err::<(), _>(RehomeError::new(
                    ErrorCode::ChecksumMismatch,
                    "original checksum error",
                ))
            }))
            .unwrap_err();
        assert_eq!(result.error.code, ErrorCode::ChecksumMismatch);
        assert_eq!(result.error.message, "original checksum error");
        let snapshot = service.incident(result.support_id).unwrap();
        let (preview, path) = service.preview(&snapshot, Locale::En);
        assert!(!preview.saved);
        assert!(path.is_none());
        assert!(preview.codex_text.contains("checksum_mismatch"));
        assert!(!preview.codex_text.contains("not-a-directory"));
        assert!(service.incident(Uuid::new_v4()).is_err());
    }
    #[test]
    fn cancelled_and_successful_non_import_operations_do_not_create_incidents() {
        let root = tempfile::tempdir().unwrap();
        let support_root = root.path().join("support");
        let service = SupportService::with_root(support_root.clone());
        let result = tauri::async_runtime::block_on(run_supported(Stage::Inspect, service, |_| {
            Ok(None::<()>)
        }))
        .unwrap();
        assert!(result.is_none());
        assert!(!support_root.exists());
    }
}

fn ensure_codex_desktop_is_closed() -> Result<(), RehomeError> {
    if codex_desktop_is_running()? {
        return Err(RehomeError::new(
            ErrorCode::CodexRunning,
            "Codex 或相关后台进程仍在运行。请完全退出 Codex Desktop、ChatGPT 和相关扩展进程后，再开始恢复或回滚。",
        ));
    }
    Ok(())
}

#[cfg(windows)]
const WINDOWS_CODEX_PROCESS_NAMES: &[&str] = &[
    "codex.exe",
    "codex-code-mode-host.exe",
    "ChatGPT.exe",
    "extension-host.exe",
];

#[cfg(windows)]
fn codex_desktop_is_running() -> Result<bool, RehomeError> {
    for process_name in WINDOWS_CODEX_PROCESS_NAMES {
        if tasklist_reports_process(process_name)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn tasklist_reports_process(process_name: &str) -> Result<bool, RehomeError> {
    let filter = format!("IMAGENAME eq {process_name}");
    let output = Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .map_err(|error| {
            RehomeError::new(
                ErrorCode::CodexRunning,
                format!("could not check whether Codex is running: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(RehomeError::new(
            ErrorCode::CodexRunning,
            "could not check whether Codex is running",
        ));
    }
    Ok(tasklist_csv_has_process(
        &String::from_utf8_lossy(&output.stdout),
        process_name,
    ))
}

#[cfg(windows)]
fn tasklist_csv_has_process(output: &str, process_name: &str) -> bool {
    let expected = format!("\"{}\"", process_name.to_ascii_lowercase());
    output
        .to_ascii_lowercase()
        .lines()
        .any(|line| line.trim_start().starts_with(&expected))
}

#[cfg(target_os = "macos")]
fn codex_desktop_is_running() -> Result<bool, RehomeError> {
    let output = Command::new("pgrep")
        .args(["-f", "/Codex.app/"])
        .output()
        .map_err(|error| {
            RehomeError::new(
                ErrorCode::CodexRunning,
                format!("could not check whether Codex is running: {error}"),
            )
        })?;
    Ok(output.status.success())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn codex_desktop_is_running() -> Result<bool, RehomeError> {
    Ok(false)
}

#[cfg(test)]
mod grant_tests {
    use super::*;

    #[test]
    fn pruning_expired_capabilities_keeps_in_flight_restore_plans() {
        let plan_id = Uuid::new_v4();
        let mut grants = WorkflowGrants::default();
        grants.plans.insert(
            plan_id,
            Timed {
                value: RestorePlanGrant {
                    backup_root: PathBuf::from("C:\\backups"),
                    state: GrantState::InFlight,
                    package_facts: None,
                },
                expires_at: Instant::now() - Duration::from_secs(1),
            },
        );

        grants.prune();

        assert!(grants.plans.contains_key(&plan_id));
    }

    #[test]
    fn restore_locations_must_not_overlap() {
        let root = PathBuf::from("/restore");
        assert!(validate_restore_location_separation(&root, &root).is_err());
        assert!(validate_restore_location_separation(&root, &root.join("backups")).is_err());
        assert!(validate_restore_location_separation(
            &root.join("projects"),
            &root.join("backups")
        )
        .is_ok());
    }

    #[test]
    fn conversation_only_restore_uses_a_non_overlapping_placeholder_project_root() {
        let parent = PathBuf::from("root").join("user");
        let codex_home = parent.join(".codex");
        let projects_root = default_unused_projects_root(&codex_home).unwrap();

        assert_eq!(projects_root, parent.join("ReHome Projects"));
        assert!(!projects_root.starts_with(&codex_home));
        assert!(!codex_home.starts_with(&projects_root));
    }

    #[cfg(windows)]
    #[test]
    fn tasklist_process_detection_uses_the_csv_image_name() {
        assert!(tasklist_csv_has_process(
            "\"codex.exe\",\"123\",\"Console\",\"1\",\"100 K\"\r\n",
            "codex.exe"
        ));
        assert!(!tasklist_csv_has_process(
            "INFO: No tasks are running which match the specified criteria.\r\n",
            "codex.exe"
        ));
        assert!(WINDOWS_CODEX_PROCESS_NAMES.contains(&"ChatGPT.exe"));
        assert!(WINDOWS_CODEX_PROCESS_NAMES.contains(&"extension-host.exe"));
    }
}
