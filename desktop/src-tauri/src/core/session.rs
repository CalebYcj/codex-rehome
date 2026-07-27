use serde_json::Value;
use uuid::Uuid;

pub(crate) struct SessionMetadata {
    pub task_id: Uuid,
    pub fields: Value,
}

pub(crate) fn parse_session_metadata(bytes: &[u8]) -> Option<SessionMetadata> {
    bytes.split(|byte| *byte == b'\n').find_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let line = std::str::from_utf8(line).ok()?;
        let value = serde_json::from_str::<Value>(line).ok()?;
        session_metadata_from_value(value)
    })
}

pub(crate) fn session_metadata_from_value(value: Value) -> Option<SessionMetadata> {
    match value.get("type").and_then(Value::as_str) {
        Some("session_meta") => {
            let mut fields = value.get("payload")?.as_object()?.clone();
            if !fields.contains_key("timestamp") {
                if let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) {
                    fields.insert("timestamp".into(), Value::String(timestamp.to_owned()));
                }
            }
            let fields = Value::Object(fields);
            let task_id = metadata_uuid(&fields, &["id"])?;
            Some(SessionMetadata { task_id, fields })
        }
        Some(_) => None,
        None if has_legacy_session_context(&value) => {
            let task_id = metadata_uuid(&value, &["thread_id", "id", "conversation_id"])?;
            Some(SessionMetadata {
                task_id,
                fields: value,
            })
        }
        None => None,
    }
}

pub(crate) fn metadata_uuid(value: &Value, keys: &[&str]) -> Option<Uuid> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
    })
}

pub(crate) fn metadata_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_owned))
}

fn has_legacy_session_context(value: &Value) -> bool {
    [
        "cwd",
        "workspace_root",
        "timestamp",
        "updated_at",
        "project_id",
        "rollout_path",
    ]
    .iter()
    .any(|key| value.get(*key).is_some())
}
