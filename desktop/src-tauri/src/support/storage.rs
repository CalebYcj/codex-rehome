use super::models::SupportSnapshot;
use crate::core::stable_fs::PinnedParent;
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub const MAX_BYTES: usize = 256 * 1024;
const MAX_REPORTS: usize = 50;

pub(crate) fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "support path is unavailable or unsafe",
    )
}

pub(crate) fn check_ancestry(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(unsafe_path());
    }
    #[cfg(windows)]
    if path.components().any(|c| matches!(c, Component::Prefix(p) if matches!(p.kind(), std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..) | std::path::Prefix::DeviceNS(..)))) { return Err(unsafe_path()); }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(unsafe_path());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(unsafe_path());
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn open_read(path: &Path) -> io::Result<fs::File> {
    check_ancestry(path)?;
    let parent = PinnedParent::open(path.parent().ok_or_else(unsafe_path)?)?;
    let file = parent.open_file(path.file_name().ok_or_else(unsafe_path)?)?;
    if !file.metadata()?.is_file() {
        return Err(unsafe_path());
    }
    Ok(file)
}

pub fn save(root: &Path, snapshot: &SupportSnapshot) -> io::Result<PathBuf> {
    let bytes = serde_json::to_vec_pretty(snapshot)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("support snapshot size limit reached"));
    }
    check_ancestry(root)?;
    fs::create_dir_all(root)?;
    check_ancestry(root)?;
    let name = format!("{}.json", snapshot.support_id);
    let parent = PinnedParent::open(root)?;
    if !parent.child_exists(OsStr::new(&name))?
        && fs::read_dir(root)?
            .filter_map(Result::ok)
            .filter(|entry| {
                let path = entry.path();
                path.extension() == Some(OsStr::new("json"))
                    && path
                        .file_stem()
                        .and_then(OsStr::to_str)
                        .is_some_and(|stem| Uuid::parse_str(stem).is_ok())
            })
            .take(MAX_REPORTS)
            .count()
            >= MAX_REPORTS
    {
        return Err(io::Error::other("support report count limit reached"));
    }
    parent.replace_bytes(OsStr::new(&name), &bytes)?;
    Ok(root.join(name))
}

pub fn load(root: &Path, id: Uuid) -> io::Result<SupportSnapshot> {
    let mut bytes = Vec::new();
    open_read(&root.join(format!("{id}.json")))?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(unsafe_path());
    }
    let snapshot: SupportSnapshot = serde_json::from_slice(&bytes)?;
    if snapshot.schema_version != 1
        || snapshot.knowledge_revision != super::models::KNOWLEDGE_REVISION
        || snapshot.support_id != id
        || snapshot.sessions.len() > 100
        || snapshot.project_paths.len() > 100
    {
        return Err(unsafe_path());
    }
    Ok(snapshot)
}

pub(crate) fn save_guide(root: &Path, locale: super::models::Locale) -> io::Result<PathBuf> {
    check_ancestry(root)?;
    fs::create_dir_all(root)?;
    let name = if locale.is_zh() {
        "agent-recovery-guide.zh-CN.md"
    } else {
        "agent-recovery-guide.en.md"
    };
    PinnedParent::open(root)?
        .replace_bytes(OsStr::new(name), super::render::guide(locale).as_bytes())?;
    Ok(root.join(name))
}
