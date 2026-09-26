use super::{models::*, storage};
use crate::core::models::RecoveryStatus;
use chrono::Utc;
use serde_json::Value;
use std::{
    fs::Metadata,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    time::{Duration, Instant},
};

const LINE_LIMIT: u64 = 1024 * 1024;
const INDEX_LIMIT: u64 = 32 * 1024 * 1024;

fn check(subject: impl Into<String>, code: &str, status: CheckStatus) -> Check {
    Check {
        subject: subject.into(),
        code: code.into(),
        status,
    }
}
fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}
fn read_header(path: &Path) -> io::Result<Value> {
    let file = storage::open_read(path)?;
    let before = file.metadata()?;
    let mut line = Vec::new();
    BufReader::new((&file).take(LINE_LIMIT + 1)).read_until(b'\n', &mut line)?;
    if line.len() as u64 > LINE_LIMIT || !unchanged(&before, &file.metadata()?) {
        return Err(io::Error::other("limit_or_changed"));
    }
    let line = line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&line);
    Ok(serde_json::from_slice(line)?)
}

pub fn run(snapshot: &SupportSnapshot) -> RecheckReport {
    let started = Instant::now();
    let mut report = RecheckReport {
        checked_at: Utc::now().to_rfc3339(),
        checks: vec![],
        omitted_sessions: snapshot.omitted_sessions,
    };
    if snapshot.transaction_status != Some(RecoveryStatus::Committed) {
        report.checks.push(check(
            "transaction",
            "transaction_not_committed",
            CheckStatus::Unknown,
        ));
    } else {
        for (i, path) in snapshot.project_paths.iter().take(MAX_SESSIONS).enumerate() {
            let (code, status) =
                match storage::check_ancestry(path).and_then(|_| std::fs::metadata(path)) {
                    Ok(meta) if meta.is_dir() => ("project_exists", CheckStatus::Pass),
                    Ok(_) => ("project_not_directory", CheckStatus::Fail),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        ("project_missing", CheckStatus::Fail)
                    }
                    Err(_) => ("path_unavailable", CheckStatus::Unknown),
                };
            report
                .checks
                .push(check(format!("project-{}", i + 1), code, status));
        }
        if snapshot.sessions.is_empty() {
            report.checks.push(check(
                "sessions",
                "mapping_unavailable",
                CheckStatus::Unknown,
            ));
        }
        for (i, session) in snapshot.sessions.iter().take(MAX_SESSIONS).enumerate() {
            if started.elapsed() > Duration::from_secs(10) {
                report
                    .checks
                    .push(check("sessions", "time_limit", CheckStatus::Unknown));
                break;
            }
            let (code, status) = match read_header(&session.path) {
                Ok(value) => {
                    let payload = &value["payload"];
                    if value["type"] != "session_meta" {
                        ("session_header_unknown", CheckStatus::Unknown)
                    } else if payload["id"].as_str() != Some(session.id.to_string().as_str()) {
                        ("session_id_mismatch", CheckStatus::Fail)
                    } else if session
                        .cwd
                        .as_ref()
                        .is_some_and(|cwd| payload["cwd"].as_str() != Some(cwd))
                    {
                        ("session_path_mismatch", CheckStatus::Fail)
                    } else {
                        ("session_header_valid", CheckStatus::Pass)
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    ("session_missing", CheckStatus::Fail)
                }
                Err(_) => ("session_unavailable", CheckStatus::Unknown),
            };
            report
                .checks
                .push(check(format!("conversation-{}", i + 1), code, status));
        }
        check_index(snapshot, &mut report, started);
    }
    report.checks.push(check(
        "database",
        "database_not_checked",
        CheckStatus::Unknown,
    ));
    report.checks.push(check(
        "Codex",
        "conversation_not_verified",
        CheckStatus::Unknown,
    ));
    if snapshot.omitted_sessions > 0 {
        report
            .checks
            .push(check("sessions", "coverage_limited", CheckStatus::Unknown));
    }
    if snapshot.projects_truncated {
        report.checks.push(check(
            "projects",
            "project_coverage_limited",
            CheckStatus::Unknown,
        ));
    }
    report
}

fn check_index(snapshot: &SupportSnapshot, report: &mut RecheckReport, started: Instant) {
    let Some(path) = snapshot.index_path.as_ref() else {
        report
            .checks
            .push(check("index", "index_not_available", CheckStatus::Unknown));
        return;
    };
    let result = (|| -> io::Result<Vec<Check>> {
        let file = storage::open_read(path)?;
        let before = file.metadata()?;
        if before.len() > INDEX_LIMIT {
            return Err(io::Error::other("index_limit"));
        }
        let mut reader = BufReader::new((&file).take(INDEX_LIMIT + 1));
        let mut found = std::collections::HashMap::new();
        let mut scanned = 0;
        loop {
            if started.elapsed() > Duration::from_secs(10) {
                return Err(io::Error::other("time_limit"));
            }
            let mut line = Vec::new();
            let read = reader
                .by_ref()
                .take(LINE_LIMIT + 1)
                .read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            scanned += read as u64;
            if line.len() as u64 > LINE_LIMIT || scanned > INDEX_LIMIT {
                return Err(io::Error::other("index_limit"));
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let row: Value = serde_json::from_slice(&line)?;
            if let Some(session) = snapshot
                .sessions
                .iter()
                .find(|s| row["id"].as_str() == Some(s.id.to_string().as_str()))
            {
                let rollout_ok = row
                    .get("rollout_path")
                    .and_then(Value::as_str)
                    .is_none_or(|p| Path::new(p) == session.path);
                let cwd_ok = row
                    .get("cwd")
                    .and_then(Value::as_str)
                    .is_none_or(|cwd| session.cwd.as_ref().is_none_or(|expected| cwd == expected));
                found.insert(session.id, rollout_ok && cwd_ok);
            }
        }
        if !unchanged(&before, &file.metadata()?) {
            return Err(io::Error::other("index_changed"));
        }
        Ok(snapshot
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (code, status) = match found.get(&s.id) {
                    Some(true) => ("index_entry_valid", CheckStatus::Pass),
                    Some(false) => ("index_path_mismatch", CheckStatus::Fail),
                    None => ("index_entry_missing", CheckStatus::Fail),
                };
                check(format!("index/conversation-{}", i + 1), code, status)
            })
            .collect())
    })();
    match result {
        Ok(checks) => report.checks.extend(checks),
        Err(_) => report.checks.push(check(
            "index",
            "index_unavailable_or_changed",
            CheckStatus::Unknown,
        )),
    }
}
