use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceOs {
    Windows,
    Macos,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PackageMode {
    Full,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentCounts {
    pub projects: u64,
    pub project_files: u64,
    pub conversations: u64,
    pub skills: u64,
    pub plugins: u64,
    pub generated_images: u64,
    pub sqlite_threads: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectEntry {
    pub project_id: Uuid,
    pub name: String,
    pub source_path: String,
    #[serde(default = "project_source_available_by_default")]
    pub source_available: bool,
    pub archive_path: String,
    pub file_count: u64,
    pub content_bytes: u64,
    pub git_remote: Option<String>,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

fn project_source_available_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationEntry {
    pub task_id: Uuid,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub updated_at: String,
    pub content_hash: String,
    pub archive_path: String,
    pub classification: Option<ConversationClassification>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationClassification {
    pub parent_task_id: Option<Uuid>,
    pub agent_path: Option<String>,
    pub agent_nickname: Option<String>,
    pub depth: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptionalContentEntry {
    pub content_id: Uuid,
    pub name: String,
    pub source_path: PathBuf,
    pub relative_path: String,
    pub size_bytes: u64,
    pub thumbnail_data_url: Option<String>,
    pub reveal_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExclusionSummary {
    pub excluded_files: u64,
    pub excluded_bytes: u64,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub format: String,
    pub schema_version: u32,
    pub package_id: Uuid,
    pub created_at: String,
    pub source_os: SourceOs,
    pub source_arch: String,
    pub source_device_id: Uuid,
    pub mode: PackageMode,
    pub parent_checkpoint: Option<Uuid>,
    pub counts: ContentCounts,
    pub projects: Vec<ProjectEntry>,
    pub conversations: Vec<ConversationEntry>,
    pub exclusions: ExclusionSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexInventory {
    pub codex_home: PathBuf,
    pub source_os: SourceOs,
    pub source_arch: String,
    pub source_device_id: Uuid,
    pub counts: ContentCounts,
    pub projects: Vec<ProjectEntry>,
    pub project_paths: Vec<PathBuf>,
    pub conversations: Vec<ConversationEntry>,
    pub conversation_paths: Vec<PathBuf>,
    pub session_index_path: Option<PathBuf>,
    pub state_db_path: Option<PathBuf>,
    pub skill_paths: Vec<PathBuf>,
    pub plugin_paths: Vec<PathBuf>,
    pub generated_image_paths: Vec<PathBuf>,
    pub skills: Vec<OptionalContentEntry>,
    pub plugins: Vec<OptionalContentEntry>,
    pub generated_images: Vec<OptionalContentEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetInventory {
    pub codex_home: PathBuf,
    pub target_os: SourceOs,
    pub target_arch: String,
    pub counts: ContentCounts,
    pub projects: Vec<ProjectEntry>,
    pub conversations: Vec<ConversationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatePackageRequest {
    pub codex_home: PathBuf,
    pub project_paths: Vec<PathBuf>,
    pub conversation_ids: Vec<Uuid>,
    pub output_path: PathBuf,
    pub source_device_id: Uuid,
    pub skill_paths: Vec<PathBuf>,
    pub plugin_paths: Vec<PathBuf>,
    pub generated_image_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatePackageReport {
    pub package_path: PathBuf,
    pub package_id: Uuid,
    pub bytes_written: u64,
    pub counts: ContentCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackagePreview {
    pub package_path: PathBuf,
    pub archive_hash: String,
    pub manifest: PackageManifest,
    pub checksum_valid: bool,
    pub entries: Vec<String>,
    pub forbidden_files_total: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Add,
    Update,
    Unchanged,
    Preserve,
    Conflict,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    Skip,
    Import,
    ImportAsBranch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceRewriteKind {
    ConversationId,
    ConversationTitle,
    ProjectPath,
    SessionPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationStatus {
    Registered,
    CommandUnavailable,
    InvocationFailed { message: String },
    ManualOpenRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceRewrite {
    pub source_task_id: Uuid,
    pub package_source: String,
    pub kind: ReferenceRewriteKind,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedSession {
    pub package_source: String,
    pub target: PathBuf,
    pub source_task_id: Uuid,
    pub target_task_id: Uuid,
    pub title: String,
    pub source_content_hash: String,
    pub expected_final_content_hash: String,
    pub action: SessionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedOperation {
    pub package_source: String,
    pub target: PathBuf,
    pub expected_previous_hash: Option<String>,
    pub action: ChangeKind,
    pub rollback_required: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeVerificationRequirements {
    pub session_index: Option<PathBuf>,
    pub sqlite_database: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestorePlan {
    pub plan_id: Uuid,
    pub package_path: PathBuf,
    pub package_id: Uuid,
    pub archive_hash: String,
    pub target_codex_home: PathBuf,
    pub projects_root: PathBuf,
    pub operations: Vec<PlannedOperation>,
    pub sessions: Vec<PlannedSession>,
    pub reference_rewrites: Vec<ReferenceRewrite>,
    #[serde(default)]
    pub bridge_verification: BridgeVerificationRequirements,
    pub conflict_count: u64,
    pub required_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreOptions {
    pub codex_closed_confirmed: bool,
    pub backup_root: PathBuf,
    pub register_projects: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReport {
    pub package_checksum_valid: bool,
    pub files_valid: bool,
    pub sessions_valid: bool,
    pub session_index_valid: bool,
    pub sqlite_threads_valid: bool,
    pub path_mapping_valid: bool,
    pub forbidden_files_absent: bool,
    pub project_files_valid: bool,
    pub app_registration_valid: bool,
    pub app_visible_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRegistration {
    pub project_id: Uuid,
    pub project_path: PathBuf,
    pub status: RegistrationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreReport {
    pub transaction_id: Uuid,
    pub package_id: Uuid,
    pub completed_at: String,
    pub restored_files: u64,
    pub restored_bytes: u64,
    pub registrations: Vec<ProjectRegistration>,
    pub verification: VerificationReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackReport {
    pub transaction_id: Uuid,
    pub completed_at: String,
    pub restored_files: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    Prepared,
    Applying,
    Verifying,
    Committed,
    RollingBack,
    RolledBack,
    RollbackFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingRecovery {
    pub transaction_id: Uuid,
    pub package_id: Uuid,
    pub created_at: String,
    pub status: RecoveryStatus,
    pub backup_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionSummary {
    pub transaction_id: Uuid,
    pub package_id: Uuid,
    pub created_at: String,
    pub status: RecoveryStatus,
    pub backup_root: PathBuf,
    pub transaction_backup_path: PathBuf,
    pub target_codex_home: PathBuf,
    pub projects_root: PathBuf,
    pub restored_project_paths: Vec<PathBuf>,
    pub changed_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionHistory {
    pub transactions: Vec<TransactionSummary>,
    pub warnings: Vec<String>,
}
