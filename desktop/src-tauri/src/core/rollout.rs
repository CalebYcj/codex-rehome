use super::session::{parse_session_metadata, SessionMetadata};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::Path,
};

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
