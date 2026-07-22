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
    pub source_path: PathBuf,
    pub archive_path: String,
    pub file_count: u64,
    pub content_bytes: u64,
    pub git_remote: Option<String>,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationEntry {
    pub task_id: Uuid,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub updated_at: String,
    pub content_hash: String,
    pub archive_path: String,
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
    pub conversations: Vec<ConversationEntry>,
    pub session_index_path: Option<PathBuf>,
    pub state_db_path: Option<PathBuf>,
    pub skill_paths: Vec<PathBuf>,
    pub plugin_paths: Vec<PathBuf>,
    pub generated_image_paths: Vec<PathBuf>,
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
    pub include_skills: bool,
    pub include_plugins: bool,
    pub include_generated_images: bool,
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
    pub manifest: PackageManifest,
    pub checksum_valid: bool,
    pub entries: Vec<String>,
    pub forbidden_files_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestorePlan {
    pub package_path: PathBuf,
    pub package_id: Uuid,
    pub target_codex_home: PathBuf,
    pub projects_root: PathBuf,
    pub project_targets: Vec<PathBuf>,
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
pub struct RestoreReport {
    pub transaction_id: Uuid,
    pub package_id: Uuid,
    pub completed_at: String,
    pub restored_files: u64,
    pub restored_bytes: u64,
    pub verification: VerificationReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackReport {
    pub transaction_id: Uuid,
    pub completed_at: String,
    pub restored_files: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingRecovery {
    pub transaction_id: Uuid,
    pub package_id: Uuid,
    pub created_at: String,
    pub status: String,
    pub backup_root: PathBuf,
}
