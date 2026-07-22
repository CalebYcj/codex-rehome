use crate::core::error::{ErrorCode, RehomeError};
use std::path::Path;

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
