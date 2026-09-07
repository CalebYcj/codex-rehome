use crate::core::error::{ErrorCode, RehomeError};
use std::path::{Path, PathBuf};

// Filesystem guards keep canonical paths. Codex project keys and launcher
// arguments must instead match the normal paths returned by its folder picker.
pub fn codex_project_path(path: &Path) -> Result<String, RehomeError> {
    let text = path.to_str().ok_or_else(|| {
        RehomeError::new(
            ErrorCode::RestoreFailed,
            "project path cannot be represented in Codex JSON metadata without loss",
        )
    })?;
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return Ok(format!(r"\\{unc}"));
    }
    if let Some(drive) = text.strip_prefix(r"\\?\") {
        let bytes = drive.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\'
        {
            return Ok(drive.to_owned());
        }
    }
    Ok(text.to_owned())
}

pub(crate) fn agents_skills_root(codex_home: &Path) -> Result<PathBuf, RehomeError> {
    codex_home
        .parent()
        .map(|parent| parent.join(".agents").join("skills"))
        .ok_or_else(|| {
            RehomeError::new(
                ErrorCode::RestoreFailed,
                "target Codex home has no user-home parent",
            )
        })
}

// Shared skills are a third, narrowly scoped root, never the whole user home.
// Bind each external write to its authenticated archive-relative destination.
pub(crate) fn restore_target_root(
    codex_home: &Path,
    projects_root: &Path,
    source: &str,
    target: &Path,
) -> Result<PathBuf, RehomeError> {
    if let Some(relative) = source.strip_prefix("agents/skills/") {
        let relative = normalize_entry(Path::new(relative))?;
        let root = agents_skills_root(codex_home)?;
        let expected = relative
            .split('/')
            .fold(root.clone(), |path, part| path.join(part));
        if target == expected {
            return Ok(root);
        }
    } else if target.starts_with(codex_home) {
        return Ok(codex_home.to_path_buf());
    } else if target.starts_with(projects_root) {
        return Ok(projects_root.to_path_buf());
    }
    Err(RehomeError::new(
        ErrorCode::RestoreFailed,
        format!(
            "restore target escapes the planned roots: {}",
            target.display()
        ),
    ))
}

pub fn normalize_entry(path: &Path) -> Result<String, RehomeError> {
    let raw = path
        .to_str()
        .ok_or_else(|| invalid_entry("entry name is not UTF-8"))?;
    if raw.is_empty() || raw.contains('\0') {
        return Err(invalid_entry("entry name is empty or contains NUL"));
    }
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err(invalid_entry("absolute archive entries are not allowed"));
    }

    let normalized = raw.replace('\\', "/");
    if normalized.ends_with('/') {
        return Err(invalid_entry(
            "archive entry has an empty trailing component",
        ));
    }

    let mut components = Vec::new();
    for component in normalized.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(invalid_entry(
                "archive entry contains an ambiguous component",
            ));
        }
        if component.contains(':') {
            return Err(invalid_entry(
                "drive prefixes and alternate data streams are not allowed",
            ));
        }
        if component.chars().any(|character| {
            matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
                || ('\u{1}'..='\u{1f}').contains(&character)
        }) {
            return Err(invalid_entry(
                "archive entry contains characters forbidden on Windows",
            ));
        }
        if component.trim() != component || component.ends_with('.') {
            return Err(invalid_entry("archive entry is not portable"));
        }
        if is_windows_device_name(component) {
            return Err(invalid_entry("Windows device names are not allowed"));
        }
        components.push(component);
    }

    Ok(components.join("/"))
}

pub fn validate_source_containment(root: &Path, candidate: &Path) -> Result<(), RehomeError> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| invalid_entry("selected source root cannot be resolved"))?;
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|_| invalid_entry("selected source path cannot be resolved"))?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(invalid_entry("selected source path escapes its root"));
    }
    Ok(())
}

fn is_windows_device_name(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || stem
            .strip_prefix("com")
            .or_else(|| stem.strip_prefix("lpt"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn invalid_entry(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_skills_cannot_authorize_other_user_home_files() {
        let home = std::env::temp_dir().join("rehome-root-test");
        let codex = home.join(".codex");
        let projects = home.join("projects");
        let skill = home.join(".agents/skills/example/SKILL.md");
        assert_eq!(
            restore_target_root(&codex, &projects, "agents/skills/example/SKILL.md", &skill)
                .unwrap(),
            home.join(".agents/skills")
        );
        for (source, target) in [
            (
                "agents/skills/example/SKILL.md",
                home.join(".agents/settings.json"),
            ),
            ("agents/skills/example/SKILL.md", codex.join("auth.json")),
            (
                "agents/skills/../settings.json",
                home.join(".agents/settings.json"),
            ),
            ("agents/settings.json", home.join(".agents/settings.json")),
            ("codex/skills/example/SKILL.md", skill.clone()),
            ("agents/skills/other/SKILL.md", skill),
        ] {
            assert!(
                restore_target_root(&codex, &projects, source, &target).is_err(),
                "{source}: {}",
                target.display()
            );
        }
    }

    #[test]
    fn codex_project_paths_use_normal_drive_and_unc_spellings() {
        for (input, expected) in [
            (r"\\?\C:\项目\论文", r"C:\项目\论文"),
            (r"\\?\UNC\server\share\项目", r"\\server\share\项目"),
            (r"C:\Projects\visual", r"C:\Projects\visual"),
            ("/Users/test/项目", "/Users/test/项目"),
            (r"\\?\Volume{test}\project", r"\\?\Volume{test}\project"),
        ] {
            assert_eq!(codex_project_path(Path::new(input)).unwrap(), expected);
        }
    }
}
