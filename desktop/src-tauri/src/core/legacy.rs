use crate::core::{
    discovery::associated_project_id,
    error::{ErrorCode, RehomeError},
    exclusions::is_forbidden,
    models::{
        ContentCounts, ConversationEntry, ExclusionSummary, PackageManifest, PackageMode,
        PackagePreview, ProjectEntry, SourceOs,
    },
    package::{VerifiedPackage, VerifiedPayload},
    paths::normalize_entry,
    session::{metadata_string, parse_session_metadata},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use uuid::Uuid;
use zip::ZipArchive;

const LEGACY_SCHEMA: u32 = 3;
const MAX_ENTRIES: usize = 10_000;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const THREAD_COLUMNS: &[&str] = &[
    "id",
    "cwd",
    "rollout_path",
    "title",
    "updated_at",
    "archived",
    "has_user_event",
    "preview",
];

#[derive(Deserialize)]
struct LegacyManifest {
    package_schema_version: u32,
    source_os: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    source_home: String,
    #[serde(default)]
    exclude_strategy: Value,
}

#[derive(Clone)]
struct LegacyFile {
    logical_name: String,
    archive_name: String,
    hash: String,
    size: u64,
}

#[derive(Default)]
struct ProjectSource {
    source_path: String,
    package_name: String,
}

pub(crate) fn inspect_schema_v3(path: &Path) -> Result<Option<VerifiedPackage>, RehomeError> {
    let mut file = fs::File::open(path)
        .map_err(|error| invalid(format!("could not open package: {error}")))?;
    let archive_hash = hash_file(&mut file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| invalid(format!("could not rewind package: {error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| invalid(format!("invalid ZIP container: {error}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(invalid("ZIP entry count exceeds the inspection limit"));
    }

    let manifest_candidates = (0..archive.len())
        .filter_map(|index| {
            let entry = archive.by_index(index).ok()?;
            let name = normalize_zip_name(entry.name(), entry.is_dir()).ok()?;
            (!entry.is_dir() && (name == "MANIFEST.json" || name.ends_with("/MANIFEST.json")))
                .then_some(name)
        })
        .collect::<Vec<_>>();
    if manifest_candidates.is_empty() {
        return Ok(None);
    }
    if manifest_candidates.len() != 1 {
        return Err(invalid(
            "legacy package contains multiple MANIFEST.json files",
        ));
    }
    let manifest_name = &manifest_candidates[0];
    let prefix = manifest_name
        .strip_suffix("MANIFEST.json")
        .unwrap_or_default();
    let manifest_bytes = read_archive_entry(&mut archive, manifest_name)?;
    let manifest: LegacyManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| invalid(format!("legacy MANIFEST.json is invalid: {error}")))?;
    if manifest.package_schema_version != LEGACY_SCHEMA {
        return Ok(None);
    }

    let mut files = BTreeMap::new();
    let mut total_bytes = 0_u64;
    let mut forbidden_files_total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| invalid(format!("could not read ZIP entry: {error}")))?;
        let archive_name = normalize_zip_name(entry.name(), entry.is_dir())?;
        let Some(logical_name) = archive_name.strip_prefix(prefix) else {
            return Err(invalid("legacy ZIP has entries outside its package root"));
        };
        if logical_name.is_empty() || entry.is_dir() {
            continue;
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(invalid("ZIP entry size exceeds the inspection limit"));
        }
        total_bytes = total_bytes
            .checked_add(entry.size())
            .ok_or_else(|| invalid("ZIP uncompressed size exceeds the inspection limit"))?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(invalid(
                "ZIP uncompressed size exceeds the inspection limit",
            ));
        }
        let logical_name = normalize_entry(Path::new(logical_name))?;
        if legacy_entry_is_forbidden(&logical_name) {
            forbidden_files_total += 1;
        }
        let size = entry.size();
        let hash = hash_reader(&mut entry)?;
        if files
            .insert(
                logical_name.clone(),
                LegacyFile {
                    logical_name,
                    archive_name,
                    hash,
                    size,
                },
            )
            .is_some()
        {
            return Err(invalid("legacy package contains duplicate portable paths"));
        }
    }
    verify_legacy_checksums(&mut archive, prefix, &files)?;

    let source_os = match manifest.source_os.to_ascii_lowercase().as_str() {
        "windows" => SourceOs::Windows,
        "mac" | "macos" => SourceOs::Macos,
        _ => return Err(invalid("legacy manifest has an unsupported source OS")),
    };
    let project_sources = read_project_sources(&mut archive, prefix, &files)?;
    let mut payloads = BTreeMap::new();
    let mut planning_payloads = BTreeMap::new();
    let mut projects = build_projects(&project_sources, &files, &mut payloads)?;
    projects.sort_by(|left, right| left.name.cmp(&right.name));

    let mut session_candidates = Vec::new();
    for file in files.values() {
        if let Some(modern) = map_codex_payload(&file.logical_name) {
            if modern.ends_with(".sqlite") || modern.starts_with("codex/state_") {
                continue;
            }
            if adapted_payload_is_forbidden(&modern) {
                continue;
            }
            insert_archive_payload(&mut payloads, modern.clone(), file)?;
            if modern.starts_with("codex/sessions/")
                || modern.starts_with("codex/archived_sessions/")
            {
                session_candidates.push((modern, file.archive_name.clone()));
            }
        }
    }

    if session_candidates.is_empty() {
        for file in files.values().filter(|file| {
            file.logical_name.starts_with("selected_chats/")
                && file.logical_name.ends_with(".jsonl")
        }) {
            let modern = format!(
                "codex/sessions/imported/{}",
                Path::new(&file.logical_name)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| invalid("selected chat filename is invalid"))?
            );
            insert_archive_payload(&mut payloads, modern.clone(), file)?;
            session_candidates.push((modern, file.archive_name.clone()));
        }
    }

    let mut conversations = Vec::new();
    let mut seen_sessions = HashSet::new();
    for (modern, archive_name) in session_candidates {
        let bytes = read_archive_entry(&mut archive, &archive_name)?;
        let Some(session) = parse_session_metadata(&bytes) else {
            continue;
        };
        if !seen_sessions.insert(session.task_id) {
            return Err(invalid(
                "legacy package contains duplicate conversation IDs",
            ));
        }
        let payload = payloads
            .get(&modern)
            .ok_or_else(|| invalid("legacy session payload mapping is missing"))?;
        conversations.push(ConversationEntry {
            task_id: session.task_id,
            project_id: associated_project_id(&session.fields, &session.fields, &projects),
            title: metadata_string(&session.fields, &["title", "thread_name"])
                .unwrap_or_else(|| "Imported conversation".to_owned()),
            updated_at: metadata_string(&session.fields, &["updated_at", "timestamp"])
                .unwrap_or_default(),
            content_hash: payload.content_hash.clone(),
            archive_path: modern.clone(),
            classification: None,
        });
        planning_payloads.insert(modern, bytes);
    }
    conversations.sort_by_key(|conversation| conversation.task_id);

    let index_bytes = build_session_index(&mut archive, &files, &conversations)?;
    if !index_bytes.is_empty() {
        insert_inline_payload(
            &mut payloads,
            &mut planning_payloads,
            "codex/session_index.jsonl",
            index_bytes,
        );
    }
    if let Some(thread_bytes) = build_thread_metadata(&mut archive, &files, &conversations)? {
        insert_inline_payload(
            &mut payloads,
            &mut planning_payloads,
            "codex/metadata/threads.json",
            thread_bytes,
        );
    }

    let counts = ContentCounts {
        projects: projects.len() as u64,
        project_files: projects.iter().map(|project| project.file_count).sum(),
        conversations: conversations.len() as u64,
        skills: payloads
            .keys()
            .filter(|path| path.starts_with("codex/skills/") && path.ends_with("/SKILL.md"))
            .count() as u64,
        plugins: payloads
            .keys()
            .filter(|path| {
                path.starts_with("codex/plugins/cache/") && path.ends_with("plugin.json")
            })
            .count() as u64,
        generated_images: payloads
            .keys()
            .filter(|path| path.starts_with("codex/generated_images/"))
            .count() as u64,
        sqlite_threads: conversations
            .iter()
            .filter(|conversation| {
                planning_payloads.contains_key("codex/metadata/threads.json")
                    && !conversation.task_id.is_nil()
            })
            .count() as u64,
    };
    let package_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, archive_hash.as_bytes());
    let source_device_id = Uuid::new_v5(&Uuid::NAMESPACE_DNS, manifest.source_home.as_bytes());
    let exclusion_text = match manifest.exclude_strategy {
        Value::String(value) => value,
        Value::Null => "legacy schema v3 exclusions".to_owned(),
        value => value.to_string(),
    };
    let package_manifest = PackageManifest {
        format: "codex-rehome".to_owned(),
        schema_version: 1,
        package_id,
        created_at: manifest.created_at,
        source_os,
        source_arch: "legacy".to_owned(),
        source_device_id,
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts,
        projects,
        conversations,
        exclusions: ExclusionSummary {
            excluded_files: 0,
            excluded_bytes: 0,
            rules: vec![exclusion_text],
        },
    };
    let mut entries = payloads.keys().cloned().collect::<Vec<_>>();
    entries.sort();
    Ok(Some(VerifiedPackage {
        preview: PackagePreview {
            package_path: path.to_path_buf(),
            archive_hash,
            manifest: package_manifest,
            checksum_valid: true,
            entries,
            forbidden_files_total,
        },
        payloads,
        planning_payloads,
    }))
}

fn build_projects(
    project_sources: &[ProjectSource],
    files: &BTreeMap<String, LegacyFile>,
    payloads: &mut BTreeMap<String, VerifiedPayload>,
) -> Result<Vec<ProjectEntry>, RehomeError> {
    let mut by_name = project_sources
        .iter()
        .map(|source| (source.package_name.clone(), source.source_path.clone()))
        .collect::<HashMap<_, _>>();
    for file in files.values() {
        let Some(relative) = file.logical_name.strip_prefix("projects/") else {
            continue;
        };
        let Some((name, _)) = relative.split_once('/') else {
            continue;
        };
        by_name.entry(name.to_owned()).or_default();
    }
    let mut projects = Vec::new();
    for (name, source_path) in by_name {
        let project_id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("legacy-project:{source_path}:{name}").as_bytes(),
        );
        let legacy_prefix = format!("projects/{name}/");
        let modern_root = format!("projects/{project_id}/files");
        let mut file_count = 0_u64;
        let mut content_bytes = 0_u64;
        for file in files
            .values()
            .filter(|file| file.logical_name.starts_with(&legacy_prefix))
        {
            let relative = file
                .logical_name
                .strip_prefix(&legacy_prefix)
                .ok_or_else(|| invalid("legacy project path mapping failed"))?;
            if relative.is_empty() || is_forbidden(Path::new(relative)) {
                continue;
            }
            let modern = format!("{modern_root}/{relative}");
            insert_archive_payload(payloads, modern, file)?;
            file_count += 1;
            content_bytes = content_bytes.saturating_add(file.size);
        }
        let project = ProjectEntry {
            project_id,
            name: name.clone(),
            source_path,
            archive_path: modern_root,
            file_count,
            content_bytes,
            git_remote: None,
            git_branch: None,
            git_head: None,
        };
        let metadata = serde_json::to_vec_pretty(&project).map_err(|error| {
            invalid(format!("could not adapt legacy project metadata: {error}"))
        })?;
        let metadata_path = format!("projects/{project_id}/project.json");
        let hash = format!("{:x}", Sha256::digest(&metadata));
        payloads.insert(
            metadata_path,
            VerifiedPayload {
                content_hash: hash,
                size_bytes: metadata.len() as u64,
                archive_name: None,
                inline_bytes: Some(metadata),
            },
        );
        projects.push(project);
    }
    Ok(projects)
}

fn read_project_sources(
    archive: &mut ZipArchive<fs::File>,
    _prefix: &str,
    files: &BTreeMap<String, LegacyFile>,
) -> Result<Vec<ProjectSource>, RehomeError> {
    let Some(file) = files.get("metadata/path_map.json") else {
        return Ok(Vec::new());
    };
    let value: Value = serde_json::from_slice(&read_archive_entry(archive, &file.archive_name)?)
        .map_err(|error| invalid(format!("legacy path_map.json is invalid: {error}")))?;
    Ok(value
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|project| {
            let package_name = project
                .get("package_project_name")
                .and_then(Value::as_str)?;
            Some(ProjectSource {
                source_path: project
                    .get("source_path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                package_name: package_name.to_owned(),
            })
        })
        .collect())
}

fn build_session_index(
    archive: &mut ZipArchive<fs::File>,
    files: &BTreeMap<String, LegacyFile>,
    conversations: &[ConversationEntry],
) -> Result<Vec<u8>, RehomeError> {
    let selected = conversations
        .iter()
        .map(|conversation| conversation.task_id)
        .collect::<HashSet<_>>();
    let mut rows = BTreeMap::<Uuid, Value>::new();
    if let Some(file) = files.get("home/.codex/session_index.jsonl") {
        let bytes = read_archive_entry(archive, &file.archive_name)?;
        for line in bytes.split(|byte| *byte == b'\n') {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            let Some(id) = value
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
            else {
                continue;
            };
            if selected.contains(&id) {
                rows.insert(id, value);
            }
        }
    }
    for conversation in conversations {
        rows.entry(conversation.task_id).or_insert_with(|| {
            serde_json::json!({
                "id": conversation.task_id,
                "thread_name": conversation.title,
                "updated_at": conversation.updated_at
            })
        });
    }
    let mut bytes = Vec::new();
    for row in rows.into_values() {
        serde_json::to_writer(&mut bytes, &row)
            .map_err(|error| invalid(format!("could not adapt legacy session index: {error}")))?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn build_thread_metadata(
    archive: &mut ZipArchive<fs::File>,
    files: &BTreeMap<String, LegacyFile>,
    conversations: &[ConversationEntry],
) -> Result<Option<Vec<u8>>, RehomeError> {
    let Some(file) = files.get("metadata/thread_index_export.json") else {
        return Ok(None);
    };
    let value: Value = serde_json::from_slice(&read_archive_entry(archive, &file.archive_name)?)
        .map_err(|error| {
            invalid(format!(
                "legacy thread_index_export.json is invalid: {error}"
            ))
        })?;
    let selected = conversations
        .iter()
        .map(|conversation| conversation.task_id.to_string())
        .collect::<HashSet<_>>();
    let rows = value
        .get("threads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| selected.contains(id))
        })
        .map(|row| {
            let mut output = Map::new();
            for column in THREAD_COLUMNS {
                if let Some(value) = row.get(*column) {
                    output.insert((*column).to_owned(), value.clone());
                }
            }
            Value::Object(output)
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(None);
    }
    serde_json::to_vec_pretty(&rows)
        .map(Some)
        .map_err(|error| invalid(format!("could not adapt legacy thread metadata: {error}")))
}

fn map_codex_payload(path: &str) -> Option<String> {
    path.strip_prefix("home/.codex/")
        .map(|relative| format!("codex/{relative}"))
}

fn insert_archive_payload(
    payloads: &mut BTreeMap<String, VerifiedPayload>,
    modern: String,
    file: &LegacyFile,
) -> Result<(), RehomeError> {
    if payloads
        .insert(
            modern,
            VerifiedPayload {
                content_hash: file.hash.clone(),
                size_bytes: file.size,
                archive_name: Some(file.archive_name.clone()),
                inline_bytes: None,
            },
        )
        .is_some()
    {
        return Err(invalid("legacy payloads collide after path adaptation"));
    }
    Ok(())
}

fn insert_inline_payload(
    payloads: &mut BTreeMap<String, VerifiedPayload>,
    planning_payloads: &mut BTreeMap<String, Vec<u8>>,
    name: &str,
    bytes: Vec<u8>,
) {
    let hash = format!("{:x}", Sha256::digest(&bytes));
    payloads.insert(
        name.to_owned(),
        VerifiedPayload {
            content_hash: hash,
            size_bytes: bytes.len() as u64,
            archive_name: None,
            inline_bytes: Some(bytes.clone()),
        },
    );
    planning_payloads.insert(name.to_owned(), bytes);
}

fn verify_legacy_checksums(
    archive: &mut ZipArchive<fs::File>,
    prefix: &str,
    files: &BTreeMap<String, LegacyFile>,
) -> Result<(), RehomeError> {
    let checksum = files
        .get("SHA256SUMS.txt")
        .ok_or_else(|| invalid("legacy SHA256SUMS.txt is missing"))?;
    let bytes = read_archive_entry(archive, &checksum.archive_name)?;
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) || bytes.contains(&b'\r') {
        return Err(RehomeError::new(
            ErrorCode::ChecksumMismatch,
            "legacy SHA256SUMS.txt cannot be verified directly; regenerate it as LF UTF-8 without BOM",
        ));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("legacy SHA256SUMS.txt is not UTF-8"))?;
    let mut expected = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (hash, raw_path) = line
            .split_once("  ")
            .ok_or_else(|| invalid("legacy SHA256SUMS.txt has an invalid line"))?;
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("legacy SHA256SUMS.txt has an invalid hash"));
        }
        let raw_path = raw_path.strip_prefix("./").unwrap_or(raw_path);
        let logical = raw_path.strip_prefix(prefix).unwrap_or(raw_path);
        let logical = normalize_entry(Path::new(logical))?;
        if expected
            .insert(logical, hash.to_ascii_lowercase())
            .is_some()
        {
            return Err(invalid("legacy SHA256SUMS.txt has a duplicate path"));
        }
    }
    let actual = files
        .iter()
        .filter(|(name, _)| name.as_str() != "SHA256SUMS.txt")
        .map(|(name, file)| (name.clone(), file.hash.clone()))
        .collect::<BTreeMap<_, _>>();
    if expected != actual {
        return Err(RehomeError::new(
            ErrorCode::ChecksumMismatch,
            "legacy package checksum verification failed",
        ));
    }
    Ok(())
}

fn legacy_entry_is_forbidden(path: &str) -> bool {
    if matches!(path, "MANIFEST.txt" | "MANIFEST.json" | "SHA256SUMS.txt")
        || path.starts_with("metadata/")
        || path.starts_with("docs/")
    {
        return false;
    }
    if let Some(relative) = path.strip_prefix("home/.codex/plugins/cache/") {
        return is_forbidden(Path::new(relative));
    }
    is_forbidden(Path::new(path))
}

fn adapted_payload_is_forbidden(path: &str) -> bool {
    if let Some(relative) = path.strip_prefix("codex/plugins/cache/") {
        return is_forbidden(Path::new(relative));
    }
    is_forbidden(Path::new(path))
}

fn normalize_zip_name(raw: &str, directory: bool) -> Result<String, RehomeError> {
    let candidate = if directory {
        raw.strip_suffix('/')
            .ok_or_else(|| invalid("ZIP directory entry has no trailing slash"))?
    } else {
        raw
    };
    if candidate.is_empty() {
        return Err(invalid("ZIP entry name is empty"));
    }
    normalize_entry(Path::new(candidate))
}

fn read_archive_entry(
    archive: &mut ZipArchive<fs::File>,
    name: &str,
) -> Result<Vec<u8>, RehomeError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| invalid(format!("could not read legacy ZIP entry {name}: {error}")))?;
    if entry.is_dir() || entry.size() > MAX_ENTRY_BYTES {
        return Err(invalid("legacy ZIP entry is not a supported file"));
    }
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(format!("could not read legacy ZIP entry: {error}")))?;
    Ok(bytes)
}

fn hash_file(file: &mut fs::File) -> Result<String, RehomeError> {
    let metadata = file
        .metadata()
        .map_err(|error| invalid(format!("could not inspect package: {error}")))?;
    if metadata.len() > 2 * MAX_TOTAL_BYTES {
        return Err(invalid("package file exceeds the inspection limit"));
    }
    hash_reader(file)
}

fn hash_reader(reader: &mut impl Read) -> Result<String, RehomeError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| invalid(format!("could not hash package data: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}
