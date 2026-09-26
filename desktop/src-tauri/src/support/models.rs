use crate::core::{
    error::{ErrorCode, RehomeError},
    models::{ContentCounts, RecoveryStatus, SourceOs, VerificationReport},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub const KNOWLEDGE_REVISION: &str = "support-v1";
pub const MAX_SESSIONS: usize = 100;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Export,
    Inspect,
    Destinations,
    Plan,
    Apply,
    UserReported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}
impl Locale {
    pub fn is_zh(self) -> bool {
        matches!(self, Self::ZhCn)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTarget {
    pub id: Uuid,
    pub path: PathBuf,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportSnapshot {
    pub schema_version: u32,
    pub knowledge_revision: String,
    pub support_id: Uuid,
    pub created_at: String,
    pub app_version: String,
    pub stage: Stage,
    pub source_os: Option<SourceOs>,
    pub package_schema: Option<u32>,
    pub counts: Option<ContentCounts>,
    pub error_code: Option<ErrorCode>,
    pub local_error_excerpt: Option<String>,
    pub user_note: Option<String>,
    pub transaction_id: Option<Uuid>,
    pub transaction_status: Option<RecoveryStatus>,
    pub backup_path: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub package_path: Option<PathBuf>,
    pub project_paths: Vec<PathBuf>,
    #[serde(default)]
    pub projects_truncated: bool,
    pub sessions: Vec<SessionTarget>,
    pub omitted_sessions: usize,
    pub index_path: Option<PathBuf>,
    pub verification_at_import: Option<VerificationReport>,
}
impl SupportSnapshot {
    pub fn new(stage: Stage) -> Self {
        Self {
            schema_version: 1,
            knowledge_revision: KNOWLEDGE_REVISION.into(),
            support_id: Uuid::new_v4(),
            created_at: Utc::now().to_rfc3339(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            stage,
            source_os: None,
            package_schema: None,
            counts: None,
            error_code: None,
            local_error_excerpt: None,
            user_note: None,
            transaction_id: None,
            transaction_status: None,
            backup_path: None,
            codex_home: None,
            package_path: None,
            project_paths: vec![],
            projects_truncated: false,
            sessions: vec![],
            omitted_sessions: 0,
            index_path: None,
            verification_at_import: None,
        }
    }
    pub fn record_error(&mut self, error: &RehomeError) {
        self.error_code = Some(error.code);
        self.local_error_excerpt = Some(super::render::private_excerpt(&error.message));
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportFailure {
    #[serde(flatten)]
    pub error: RehomeError,
    pub support_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportPreview {
    pub support_id: Uuid,
    pub codex_text: String,
    pub github_text: String,
    pub reveal_id: Option<Uuid>,
    pub saved: bool,
    pub can_recheck: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SupportSource {
    Incident { support_id: Uuid },
    Transaction { transaction_id: Uuid },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareSelection {
    pub source: SupportSource,
    pub locale: Locale,
    pub user_note: Option<String>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    Codex,
    Github,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopySelection {
    pub support_id: Uuid,
    pub kind: TextKind,
    pub locale: Locale,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueSelection {
    pub support_id: Uuid,
    pub locale: Locale,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecheckSelection {
    pub support_id: Uuid,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueOpenResult {
    Opened,
    CopyRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Unknown,
    NotApplicable,
}
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub subject: String,
    pub code: String,
    pub status: CheckStatus,
}
#[derive(Debug, Clone, Serialize)]
pub struct RecheckReport {
    pub checked_at: String,
    pub checks: Vec<Check>,
    pub omitted_sessions: usize,
}
