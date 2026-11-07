use super::session::{parse_session_metadata, SessionMetadata};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::Path,
};

/// The first delimited UUID is the session ID; later UUIDs identify segments.
/// Return its position too so conflict copies retain only the timestamp prefix.
pub(crate) fn filename_identity(stem: &str) -> Option<(usize, uuid::Uuid)> {
    let rest = stem.strip_prefix("rollout-")?;
    for (offset, _) in rest.char_indices() {
        let index = "rollout-".len() + offset;
        if offset != 0 && rest.as_bytes()[offset - 1] != b'-' {
            continue;
        }
        let end = index.checked_add(36)?;
        let Some(text) = stem.get(index..end) else {
            continue;
        };
        if end != stem.len() && !matches!(stem.as_bytes().get(end), Some(b'-' | b'.')) {
            continue;
        }
        if let Ok(id) = uuid::Uuid::parse_str(text) {
            return Some((index, id));
        }
    }
    None
}

pub(crate) fn is_compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".jsonl.zst"))
}

pub(crate) fn open(path: &Path) -> io::Result<Box<dyn Read>> {
    let file = File::open(path)?;
    if !is_compressed(path) {
        return Ok(Box::new(file));
    }
    let mut decoder = zstd::stream::read::Decoder::new(file)?;
    // Bound the decoder's memory window, not the file or transcript size.
    // Codex level-3 streams fit comfortably within this 128 MiB ceiling.
    decoder.window_log_max(27)?;
    Ok(Box::new(decoder))
}

pub(crate) fn metadata(path: &Path) -> io::Result<Option<SessionMetadata>> {
    let mut reader = BufReader::new(open(path)?);
    let mut line = Vec::new();
    loop {
        line.clear();
        // Metadata/control records retain their existing 1 MiB parser bound.
        let length = reader
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)?;
        if length == 0 {
            return Ok(None);
        }
        if length > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rollout metadata record exceeds the control-file limit",
            ));
        }
        if let Some(metadata) = parse_session_metadata(&line) {
            return Ok(Some(metadata));
        }
    }
}

#[cfg(test)]
mod filename_tests {
    use super::*;

    #[test]
    fn first_delimited_uuid_identifies_plain_or_segmented_rollouts() {
        let id = uuid::Uuid::from_u128(1);
        let segment = uuid::Uuid::from_u128(2);
        for stem in [
            format!("rollout-{id}"),
            format!("rollout-2026-11-07T01-02-03-{id}-{segment}"),
            format!("rollout-2026-11-07T01-02-03-{id}.segment-{segment}"),
        ] {
            let (offset, actual) = filename_identity(&stem).unwrap();
            assert_eq!(actual, id);
            assert_eq!(&stem[offset..offset + 36], id.to_string());
        }
        for stem in [
            format!("other-{id}"),
            format!("rollout-x{id}"),
            format!("rollout-{id}x"),
            "rollout-坏文件名".into(),
        ] {
            assert!(filename_identity(&stem).is_none());
        }
    }
}
