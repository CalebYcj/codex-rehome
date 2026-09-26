pub mod models;
pub mod recheck;
pub mod render;
pub mod storage;

use crate::core::{
    backup,
    error::{ErrorCode, RehomeError},
    models::{PackagePreview, ReferenceRewriteKind, RestorePlan, TransactionSummary},
};
use models::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct SupportService {
    entries: Arc<Mutex<HashMap<Uuid, SupportSnapshot>>>,
    root_override: Option<PathBuf>,
}
pub(crate) fn failure() -> RehomeError {
    RehomeError::new(
        ErrorCode::RestoreFailed,
        "support evidence is unavailable; reopen help from migration history",
    )
}
impl SupportService {
    fn root(&self) -> Result<PathBuf, RehomeError> {
        self.root_override
            .clone()
            .map(Ok)
            .unwrap_or_else(|| Ok(backup::app_data_root_path()?.join("support")))
    }
    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self {
            root_override: Some(root),
            ..Self::default()
        }
    }
    pub fn record(&self, snapshot: SupportSnapshot) {
        if let Ok(root) = self.root() {
            let _ = storage::save(&root, &snapshot);
        }
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= 50 && !entries.contains_key(&snapshot.support_id) {
                if let Some(id) = entries
                    .values()
                    .min_by_key(|s| &s.created_at)
                    .map(|s| s.support_id)
                {
                    entries.remove(&id);
                }
            }
            entries.insert(snapshot.support_id, snapshot);
        }
    }
    pub fn incident(&self, id: Uuid) -> Result<SupportSnapshot, RehomeError> {
        self.entries
            .lock()
            .map_err(|_| failure())?
            .get(&id)
            .cloned()
            .ok_or_else(failure)
    }
    pub fn transaction(&self, summary: &TransactionSummary) -> SupportSnapshot {
        let memory = self.entries.lock().ok().and_then(|entries| {
            entries
                .values()
                .find(|s| s.transaction_id == Some(summary.transaction_id))
                .cloned()
        });
        let saved = memory.or_else(|| {
            let root = self.root().ok()?;
            storage::check_ancestry(&root).ok()?;
            std::fs::read_dir(&root)
                .ok()?
                .take(52)
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let id = entry.path().file_stem()?.to_str()?.parse().ok()?;
                    let snapshot = storage::load(&root, id).ok()?;
                    (snapshot.transaction_id == Some(summary.transaction_id)).then_some(snapshot)
                })
                .next()
        });
        let mut snapshot = saved.unwrap_or_else(|| SupportSnapshot::new(Stage::UserReported));
        bind_transaction(&mut snapshot, summary);
        self.record(snapshot.clone());
        snapshot
    }
    pub fn refresh(&self, mut snapshot: SupportSnapshot) -> SupportSnapshot {
        if let Some(id) = snapshot.transaction_id {
            match backup::transaction_summary(id) {
                Ok(Some(summary)) => bind_transaction(&mut snapshot, &summary),
                _ => {
                    snapshot.transaction_status = None;
                    snapshot.sessions.clear();
                    snapshot.project_paths.clear();
                    snapshot.index_path = None;
                }
            }
        }
        snapshot
    }
    pub fn preview(
        &self,
        snapshot: &SupportSnapshot,
        locale: Locale,
    ) -> (SupportPreview, Option<PathBuf>) {
        let path = self
            .root()
            .ok()
            .and_then(|root| storage::save(&root, snapshot).ok());
        let guide_path = self
            .root()
            .ok()
            .and_then(|root| storage::save_guide(&root.join("guides"), locale).ok());
        let preview = SupportPreview {
            support_id: snapshot.support_id,
            codex_text: render::codex_text(
                snapshot,
                locale,
                path.as_deref(),
                guide_path.as_deref(),
            ),
            github_text: render::public_text(snapshot, locale),
            reveal_id: None,
            saved: path.is_some(),
            can_recheck: snapshot.transaction_id.is_some(),
        };
        (preview, path)
    }
}

pub(crate) fn capture_package(snapshot: &mut SupportSnapshot, package: &PackagePreview) {
    snapshot.source_os = Some(package.manifest.source_os);
    snapshot.package_schema = Some(package.manifest.schema_version);
    snapshot.counts = Some(package.manifest.counts.clone());
    snapshot.package_path = Some(package.package_path.clone());
}
pub fn capture_plan(snapshot: &mut SupportSnapshot, plan: &RestorePlan) {
    snapshot.codex_home = Some(plan.target_codex_home.clone());
    snapshot.package_path = Some(plan.package_path.clone());
    snapshot.index_path = plan.bridge_verification.session_index.clone();
    snapshot.omitted_sessions = plan.sessions.len().saturating_sub(MAX_SESSIONS);
    snapshot.sessions = plan
        .sessions
        .iter()
        .take(MAX_SESSIONS)
        .map(|s| SessionTarget {
            id: s.target_task_id,
            path: s.target.clone(),
            cwd: plan
                .reference_rewrites
                .iter()
                .find(|r| {
                    r.source_task_id == s.source_task_id
                        && r.package_source == s.package_source
                        && r.kind == ReferenceRewriteKind::ProjectPath
                })
                .map(|r| r.to.clone()),
        })
        .collect();
    for operation in &plan.operations {
        if let Ok(relative) = operation.target.strip_prefix(&plan.projects_root) {
            if let Some(component) = relative.components().next() {
                let project = plan.projects_root.join(component);
                if !snapshot.project_paths.contains(&project) {
                    if snapshot.project_paths.len() < MAX_SESSIONS {
                        snapshot.project_paths.push(project);
                    } else {
                        snapshot.projects_truncated = true;
                    }
                }
            }
        }
    }
}

fn bind_transaction(snapshot: &mut SupportSnapshot, summary: &TransactionSummary) {
    snapshot.transaction_id = Some(summary.transaction_id);
    snapshot.transaction_status = Some(summary.status);
    snapshot.backup_path = Some(summary.transaction_backup_path.clone());
    snapshot.codex_home = Some(summary.target_codex_home.clone());
    // Persisted support JSON never grants arbitrary file access. Bind every target to
    // the separately validated transaction journal before a command can recheck it.
    let targets = backup::support_targets(summary.transaction_id).unwrap_or_default();
    let previous_sessions = snapshot.sessions.len();
    snapshot.sessions.retain(|s| {
        targets.contains(&s.path)
            && s.path
                .starts_with(summary.target_codex_home.join("sessions"))
    });
    snapshot.omitted_sessions += previous_sessions - snapshot.sessions.len();
    snapshot.index_path = snapshot.index_path.take().filter(|p| {
        targets.contains(p) && p == &summary.target_codex_home.join("session_index.jsonl")
    });
    snapshot.project_paths.retain(|p| {
        p.parent() == Some(summary.projects_root.as_path())
            && targets.iter().any(|t| t.starts_with(p))
    });
    for path in &summary.restored_project_paths {
        if snapshot.project_paths.len() < MAX_SESSIONS && !snapshot.project_paths.contains(path) {
            snapshot.project_paths.push(path.clone());
        } else if !snapshot.project_paths.contains(path) {
            snapshot.projects_truncated = true;
        }
    }
}
