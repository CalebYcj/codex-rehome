use crate::core::{
    error::{ErrorCode, RehomeError},
    models::ReferenceRewrite,
    package::{private_app_temp_root, stream_authenticated_payload, VerifiedPayload},
    planner::{collect_session_project_paths, rewrite_jsonl_payload},
    session::parse_session_metadata,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
};
use tempfile::{Builder, NamedTempFile, TempPath};

// This bounds one JSON record, not a conversation or migration package. Records
// are released after processing; the transcript itself stays on disk.
pub(crate) const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;
const MAX_SUMMARY_PATHS: usize = 100_000;
const MAX_SUMMARY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct SessionPayload {
    file: TempPath,
    verified: VerifiedPayload,
    pub summary: Vec<u8>,
    parse_error: Option<String>,
}

impl SessionPayload {
    pub(crate) fn capture(
        reader: &mut impl Read,
        verified: &VerifiedPayload,
    ) -> Result<Self, RehomeError> {
        let mut file = Builder::new()
            .prefix(".rehome-session-")
            .tempfile_in(private_app_temp_root()?)
            .map_err(io_error)?;
        stream_authenticated_payload(reader, file.as_file_mut(), verified)?;
        file.as_file().sync_all().map_err(io_error)?;
        let mut metadata = None;
        let mut paths = BTreeSet::new();
        let mut path_bytes = 0usize;
        let scan = visit_jsonl(file.reopen().map_err(io_error)?, |line, value| {
            if metadata.is_none() {
                metadata = parse_session_metadata(line).map(|record| {
                    let mut fields = record.fields.as_object().cloned().unwrap_or_default();
                    fields.retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "id" | "thread_id"
                                | "conversation_id"
                                | "title"
                                | "thread_name"
                                | "timestamp"
                                | "updated_at"
                                | "cwd"
                                | "workspace_root"
                                | "project"
                                | "project_path"
                                | "project_id"
                                | "rollout_path"
                                | "history_mode"
                        )
                    });
                    fields.insert("id".into(), Value::String(record.task_id.to_string()));
                    json!({"type":"session_meta", "payload":fields})
                });
            }
            let mut record_paths = BTreeSet::new();
            let task_id = metadata
                .as_ref()
                .and_then(|m| m.get("payload"))
                .and_then(|m| m.get("id"))
                .and_then(Value::as_str)
                .and_then(|id| uuid::Uuid::parse_str(id).ok());
            collect_session_project_paths(value, task_id, &mut record_paths);
            for path in record_paths {
                if !paths.contains(&path) {
                    path_bytes = path_bytes.saturating_add(path.len());
                    if path_bytes > MAX_SUMMARY_BYTES {
                        return Err(invalid(
                            "session project-path metadata exceeds the metadata limit",
                        ));
                    }
                    paths.insert(path);
                }
            }
            if paths.len() > MAX_SUMMARY_PATHS {
                return Err(invalid("session contains too many distinct project paths"));
            }
            Ok(())
        });
        let mut summary = Vec::new();
        if let Some(metadata) = metadata {
            serde_json::to_writer(&mut summary, &metadata).map_err(json_error)?;
            summary.push(b'\n');
            if summary.len() > MAX_SUMMARY_BYTES {
                return Err(invalid(
                    "session planning metadata exceeds the metadata limit",
                ));
            }
        }
        for path in paths {
            serde_json::to_writer(
                &mut summary,
                &json!({"type":"turn_context","payload":{"cwd":path}}),
            )
            .map_err(json_error)?;
            summary.push(b'\n');
            if summary.len() > MAX_SUMMARY_BYTES {
                return Err(invalid(
                    "session planning metadata exceeds the metadata limit",
                ));
            }
        }
        Ok(Self {
            file: file.into_temp_path(),
            verified: verified.clone(),
            summary,
            parse_error: scan.err().map(|error| error.message),
        })
    }

    pub(crate) fn rewritten_hash(
        &self,
        rewrites: &[ReferenceRewrite],
        source: &str,
    ) -> Result<String, RehomeError> {
        self.write_rewritten(&mut io::sink(), rewrites, source)
    }

    pub(crate) fn write_rewritten(
        &self,
        writer: &mut impl Write,
        rewrites: &[ReferenceRewrite],
        source: &str,
    ) -> Result<String, RehomeError> {
        if let Some(error) = &self.parse_error {
            return Err(invalid(error));
        }
        let mut input = HashReader {
            reader: File::open(&self.file).map_err(io_error)?,
            hash: Sha256::new(),
            size: 0,
        };
        let mut output = HashWriter {
            writer,
            hash: Sha256::new(),
        };
        rewrite_stream(&mut input, &mut output, rewrites, source)?;
        if input.size != self.verified.size_bytes
            || format!("{:x}", input.hash.finalize()) != self.verified.content_hash
        {
            return Err(RehomeError::new(
                ErrorCode::ChecksumMismatch,
                "cached session changed after checksum verification",
            ));
        }
        Ok(format!("{:x}", output.hash.finalize()))
    }

    pub(crate) fn rewritten_file(
        &self,
        rewrites: &[ReferenceRewrite],
        source: &str,
    ) -> Result<(NamedTempFile, String), RehomeError> {
        let mut file = Builder::new()
            .prefix(".rehome-rewrite-")
            .tempfile_in(private_app_temp_root()?)
            .map_err(io_error)?;
        let hash = self.write_rewritten(file.as_file_mut(), rewrites, source)?;
        file.as_file().sync_all().map_err(io_error)?;
        Ok((file, hash))
    }
}

pub(crate) fn visit_jsonl(
    reader: impl Read,
    mut visit: impl FnMut(&[u8], &Value) -> Result<(), RehomeError>,
) -> Result<(), RehomeError> {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut line = Vec::new();
    let mut first = true;
    while read_record(&mut reader, &mut line)? {
        let mut bytes = line.strip_suffix(b"\n").unwrap_or(&line);
        bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        if first {
            bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            first = false;
        }
        if bytes.is_empty() {
            continue;
        }
        let value = serde_json::from_slice(bytes).map_err(json_error)?;
        visit(bytes, &value)?;
    }
    Ok(())
}

fn rewrite_stream(
    reader: impl Read,
    writer: &mut impl Write,
    rewrites: &[ReferenceRewrite],
    source: &str,
) -> Result<(), RehomeError> {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut line = Vec::new();
    let mut first = true;
    let changes = rewrites
        .iter()
        .any(|rewrite| rewrite.package_source == source);
    while read_record(&mut reader, &mut line)? {
        let mut bytes = line.as_slice();
        if first {
            bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            first = false;
        }
        if changes {
            // The existing scoped metadata transform remains authoritative. Its
            // allocation is now bounded to this record instead of the transcript.
            let rewritten = rewrite_jsonl_payload(bytes, rewrites, source)?;
            writer.write_all(&rewritten).map_err(io_error)?;
        } else {
            writer.write_all(bytes).map_err(io_error)?;
        }
    }
    Ok(())
}

fn read_record(reader: &mut impl BufRead, line: &mut Vec<u8>) -> Result<bool, RehomeError> {
    line.clear();
    loop {
        let available = reader.fill_buf().map_err(io_error)?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(length) > MAX_RECORD_BYTES {
            return Err(invalid("session JSON record exceeds the 64 MiB parsing limit; the migration package and conversation have no fixed total-size limit"));
        }
        line.extend_from_slice(&available[..length]);
        let complete = available[length - 1] == b'\n';
        reader.consume(length);
        if complete {
            return Ok(true);
        }
    }
}

struct HashReader {
    reader: File,
    hash: Sha256,
    size: u64,
}
impl Read for HashReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(buffer)?;
        self.hash.update(&buffer[..count]);
        self.size = self
            .size
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("session size overflow"))?;
        Ok(count)
    }
}
struct HashWriter<'a, W> {
    writer: &'a mut W,
    hash: Sha256,
}
impl<W: Write> Write for HashWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.writer.write(buffer)?;
        self.hash.update(&buffer[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
fn io_error(error: io::Error) -> RehomeError {
    invalid(format!("could not process session stream: {error}"))
}
fn json_error(error: serde_json::Error) -> RehomeError {
    invalid(format!("session JSONL is invalid: {error}"))
}
fn invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::ReferenceRewriteKind;
    use std::io::Cursor;
    use uuid::Uuid;

    fn captured(bytes: &[u8]) -> SessionPayload {
        SessionPayload::capture(
            &mut Cursor::new(bytes),
            &VerifiedPayload {
                content_hash: format!("{:x}", Sha256::digest(bytes)),
                size_bytes: bytes.len() as u64,
                archive_name: None,
                inline_bytes: None,
            },
        )
        .unwrap()
    }

    fn project_rewrite() -> ReferenceRewrite {
        ReferenceRewrite {
            source_task_id: Uuid::nil(),
            package_source: "codex/sessions/test.jsonl".into(),
            kind: ReferenceRewriteKind::ProjectPath,
            from: "C:/old".into(),
            to: "/Users/new/project".into(),
        }
    }

    #[test]
    fn a_single_unterminated_record_is_bounded_before_json_allocation() {
        let error = visit_jsonl(
            io::repeat(b'x').take(MAX_RECORD_BYTES as u64 + 1),
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.message.contains("64 MiB parsing limit"));
    }

    #[test]
    fn streamed_rewrite_matches_existing_scoped_transform_and_bom_rules() {
        for bytes in [
            b"\xef\xbb\xbf{\"type\":\"session_meta\",\"payload\":{\"id\":\"11111111-1111-4111-8111-111111111111\",\"cwd\":\"C:/old\",\"history_mode\":\"paginated\"}}\r\n\r\n{\"text\":\"C:/old stays in messages\"}".as_slice(),
            b"{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"C:/old\"}}\n".as_slice(),
        ] {
            let payload = captured(bytes);
            for rewrites in [vec![], vec![project_rewrite()]] {
                let expected = rewrite_jsonl_payload(bytes, &rewrites, "codex/sessions/test.jsonl").unwrap();
                let mut actual = Vec::new();
                let hash = payload.write_rewritten(&mut actual, &rewrites, "codex/sessions/test.jsonl").unwrap();
                assert_eq!(actual, expected);
                assert_eq!(hash, format!("{:x}", Sha256::digest(&actual)));
            }
        }
    }

    #[test]
    fn capture_and_rewrite_reject_changed_source_bytes() {
        let payload = captured(b"{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"C:/old\"}}\n");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&payload.file)
            .unwrap()
            .write_all(b" ")
            .unwrap();
        let error = payload
            .rewritten_hash(&[], "codex/sessions/test.jsonl")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ChecksumMismatch);
        let expected = VerifiedPayload {
            content_hash: "bad".into(),
            size_bytes: 3,
            archive_name: None,
            inline_bytes: None,
        };
        assert_eq!(
            SessionPayload::capture(&mut Cursor::new(b"{}\n"), &expected)
                .unwrap_err()
                .code,
            ErrorCode::ChecksumMismatch
        );
    }

    #[test]
    fn planning_keeps_metadata_only_and_accepts_large_individual_records() {
        let mut bytes = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"11111111-1111-4111-8111-111111111111\",\"cwd\":\"C:/old\"}}\n{\"type\":\"response_item\",\"payload\":{\"text\":\"".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 2 * 1024 * 1024));
        bytes.extend_from_slice(b"\"}}\n");
        let payload = captured(&bytes);
        assert!(payload.summary.len() < 1024);
        let mut output = Vec::new();
        payload
            .write_rewritten(
                &mut output,
                &[project_rewrite()],
                "codex/sessions/test.jsonl",
            )
            .unwrap();
        assert!(output.len() > 2 * 1024 * 1024);
        assert!(output
            .windows(18)
            .any(|chunk| chunk == b"/Users/new/project"));
    }

    struct RepeatedReader {
        pattern: Vec<u8>,
        position: usize,
        remaining: usize,
    }
    impl Read for RepeatedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(self.remaining);
            for byte in &mut buffer[..count] {
                *byte = self.pattern[self.position];
                self.position = (self.position + 1) % self.pattern.len();
            }
            self.remaining -= count;
            Ok(count)
        }
    }

    #[test]
    fn transcript_body_does_not_accumulate_in_planning_metadata() {
        let header = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"11111111-1111-4111-8111-111111111111\",\"cwd\":\"C:/old\"}}\n";
        let pattern = format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"text\":\"{}\"}}}}\n",
            "x".repeat(1024)
        )
        .into_bytes();
        let records = 32_768;
        let mut hash = Sha256::new();
        hash.update(header);
        for _ in 0..records {
            hash.update(&pattern);
        }
        let verified = VerifiedPayload {
            content_hash: format!("{:x}", hash.finalize()),
            size_bytes: (header.len() + pattern.len() * records) as u64,
            archive_name: None,
            inline_bytes: None,
        };
        let tail = RepeatedReader {
            remaining: pattern.len() * records,
            pattern,
            position: 0,
        };
        let payload =
            SessionPayload::capture(&mut Cursor::new(header).chain(tail), &verified).unwrap();
        assert!(verified.size_bytes > 32 * 1024 * 1024);
        assert!(payload.summary.len() < 1024);
        assert_eq!(
            std::fs::metadata(&payload.file).unwrap().len(),
            verified.size_bytes
        );
        let hash = payload
            .rewritten_hash(&[], "codex/sessions/test.jsonl")
            .unwrap();
        assert_eq!(hash, verified.content_hash);
    }
}
