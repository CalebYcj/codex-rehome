export type SourceOs = "windows" | "macos";
export type RecoveryStatus =
  | "prepared"
  | "applying"
  | "verifying"
  | "committed"
  | "rolling_back"
  | "rolled_back"
  | "rollback_failed";
export type ChangeKind = "add" | "update" | "unchanged" | "conflict";
export type RegistrationStatus =
  | "registered"
  | "command_unavailable"
  | "manual_open_required"
  | { invocation_failed: { message: string } };

export interface RehomeError {
  code: string;
  message: string;
}

export interface ContentCounts {
  projects: number;
  project_files: number;
  conversations: number;
  skills: number;
  plugins: number;
  generated_images: number;
  sqlite_threads: number;
}

export interface ProjectEntry {
  project_id: string;
  name: string;
  source_path: string;
  archive_path: string;
  file_count: number;
  content_bytes: number;
  git_remote: string | null;
  git_branch: string | null;
  git_head: string | null;
}

export interface ConversationEntry {
  task_id: string;
  project_id: string | null;
  title: string;
  updated_at: string;
  content_hash: string;
  archive_path: string;
}

export interface OptionalContentEntry {
  content_id: string;
  name: string;
  source_path: string;
  relative_path: string;
  size_bytes: number;
  thumbnail_data_url: string | null;
  reveal_id: string | null;
}

export interface CodexInventory {
  codex_home: string;
  source_os: SourceOs;
  source_arch: string;
  source_device_id: string;
  counts: ContentCounts;
  projects: ProjectEntry[];
  project_paths: string[];
  conversations: ConversationEntry[];
  conversation_paths: string[];
  session_index_path: string | null;
  state_db_path: string | null;
  skill_paths: string[];
  plugin_paths: string[];
  generated_image_paths: string[];
  skills: OptionalContentEntry[];
  plugins: OptionalContentEntry[];
  generated_images: OptionalContentEntry[];
  warnings: string[];
}

export interface CreatePackageRequest {
  project_ids: string[];
  conversation_ids: string[];
  skill_ids: string[];
  plugin_ids: string[];
  generated_image_ids: string[];
}

export interface CreatePackageReport {
  package_path: string;
  package_id: string;
  bytes_written: number;
  counts: ContentCounts;
  archive_hash: string;
  reveal_id: string;
}

export interface PackageManifest {
  format: string;
  schema_version: number;
  package_id: string;
  created_at: string;
  source_os: SourceOs;
  source_arch: string;
  source_device_id: string;
  mode: "full";
  parent_checkpoint: string | null;
  counts: ContentCounts;
  projects: ProjectEntry[];
  conversations: ConversationEntry[];
  exclusions: {
    excluded_files: number;
    excluded_bytes: number;
    rules: string[];
  };
}

export interface PackagePreview {
  selection_id: string;
  package_path: string;
  archive_hash: string;
  manifest: PackageManifest;
  checksum_valid: boolean;
  entries: string[];
  forbidden_files_total: number;
}

export interface PlannedOperation {
  package_source: string;
  target: string;
  expected_previous_hash: string | null;
  action: ChangeKind;
  rollback_required: boolean;
}

export interface RestorePlan {
  plan_id: string;
  package_path: string;
  package_id: string;
  archive_hash: string;
  target_codex_home: string;
  projects_root: string;
  operations: PlannedOperation[];
  sessions: unknown[];
  reference_rewrites: unknown[];
  bridge_verification: {
    session_index: string | null;
    sqlite_database: string | null;
  };
  conflict_count: number;
  required_bytes: number;
}

export interface RestoreOptions {
  codex_closed_confirmed: boolean;
  register_projects: boolean;
}

export interface RestoreLocationSelection {
  selection_id: string;
  target_codex_home: string;
  projects_root: string;
  backup_root: string;
}

export interface VerificationReport {
  package_checksum_valid: boolean;
  files_valid: boolean;
  sessions_valid: boolean;
  session_index_valid: boolean;
  sqlite_threads_valid: boolean;
  path_mapping_valid: boolean;
  forbidden_files_absent: boolean;
  project_files_valid: boolean;
  app_registration_valid: boolean;
  app_visible_ready: boolean;
}

export interface ProjectRegistration {
  project_id: string;
  project_path: string;
  status: RegistrationStatus;
}

export interface RestoreReport {
  transaction_id: string;
  package_id: string;
  completed_at: string;
  restored_files: number;
  restored_bytes: number;
  registrations: ProjectRegistration[];
  verification: VerificationReport;
}

export interface RollbackReport {
  transaction_id: string;
  completed_at: string;
  restored_files: number;
  success: boolean;
}

export interface TransactionSummary {
  transaction_id: string;
  package_id: string;
  created_at: string;
  status: RecoveryStatus;
  backup_root: string;
  transaction_backup_path: string;
  target_codex_home: string;
  projects_root: string;
  restored_project_paths: string[];
  changed_files: number;
}

export interface TransactionHistory {
  transactions: TransactionSummary[];
  warnings: string[];
}

export type RollbackAction = "rollback" | "resume";

export function errorMessage(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

export function registrationIsComplete(status: RegistrationStatus): boolean {
  return status === "registered";
}
