use crate::core::{
    discovery::{
        associated_project_id, conversation_classification, discover_codex, StateDatabaseSnapshot,
    },
    error::{ErrorCode, RehomeError},
    exclusions::is_forbidden,
    models::{
        ContentCounts, ConversationEntry, CreatePackageReport, CreatePackageRequest,
        ExclusionSummary, PackageManifest, PackageMode, PackagePreview, ProjectEntry,
    },
    paths::normalize_entry,
    session::{metadata_string, metadata_uuid, session_metadata_from_value, SessionMetadata},
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use tempfile::{Builder, NamedTempFile};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipArchive, ZipWriter};

const FORMAT: &str = "codex-rehome";
const SCHEMA_VERSION: u32 = 1;
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_CONTROL_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INSPECTION_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ARCHIVE_FILE_BYTES: u64 = 2 * MAX_INSPECTION_BYTES;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;
const THREAD_EXPORT_COLUMNS_V1: &[&str] = &[
    "id",
    "cwd",
    "rollout_path",
    "title",
    "created_at",
    "updated_at",
    "source",
    "model_provider",
    "sandbox_policy",
    "approval_mode",
    "archived",
    "has_user_event",
    "preview",
];
const EXCLUSION_RULES: &[&str] = &[
    "credentials and authentication data",
    "environment and private key files",
    "version-control metadata",
    "dependency, cache, build, and runtime data",
    "symbolic links and filesystem redirects",
];

pub fn create_package(request: CreatePackageRequest) -> Result<CreatePackageReport, RehomeError> {
    create_package_with_overwrite(request, false)
}

/// Creates a package after the desktop save dialog has explicitly confirmed replacement.
/// Other callers keep the non-overwrite default above.
pub fn create_package_replacing(
    request: CreatePackageRequest,
) -> Result<CreatePackageReport, RehomeError> {
    create_package_with_overwrite(request, true)
}

fn create_package_with_overwrite(
    request: CreatePackageRequest,
    replace_existing: bool,
) -> Result<CreatePackageReport, RehomeError> {
    validate_output_path(&request.output_path, replace_existing)?;
    let inventory = discover_codex(Some(request.codex_home.clone()))?;
    let output_parent = usable_parent(&request.output_path);
    fs::create_dir_all(output_parent).map_err(|error| {
        package_invalid(format!(
            "could not create package output directory: {error}"
        ))
    })?;

    let staging_root = private_app_temp_root()?;
    validate_staging_location(
        &staging_root,
        output_parent,
        &request.project_paths,
        &request.codex_home,
    )?;
    let staging = Builder::new()
        .prefix(".rehome-stage-")
        .tempdir_in(&staging_root)
        .map_err(|error| package_invalid(format!("could not create private staging: {error}")))?;
    make_staging_private(staging.path())?;

    let mut payloads = PayloadCollection::new()?;
    let mut counts = ContentCounts::default();
    let mut excluded_files = 0_u64;
    let mut excluded_bytes = 0_u64;

    let (projects, project_exclusions) = stage_projects(
        &request.project_paths,
        staging.path(),
        &mut payloads,
        &mut counts,
    )?;
    excluded_files += project_exclusions.0;
    excluded_bytes += project_exclusions.1;

    let index_metadata = read_selected_session_index(
        inventory.session_index_path.as_deref(),
        &request.conversation_ids,
    )?;
    let conversations = stage_conversations(
        ConversationSelection {
            paths: &inventory.conversation_paths,
            codex_home: &request.codex_home,
            selected_ids: &request.conversation_ids,
            index: &index_metadata,
            projects: &projects,
        },
        staging.path(),
        &mut payloads,
        &mut counts,
    )?;

    let complete_index = complete_session_index(&index_metadata, &conversations)?;
    if !complete_index.is_empty() {
        stage_generated(
            staging.path(),
            &mut payloads,
            "codex/session_index.jsonl",
            &complete_index,
        )?;
    }

    if let Some(state_db) = inventory.state_db_path.as_deref() {
        let (metadata, selected_rows) =
            export_selected_threads(state_db, &request.conversation_ids)?;
        if selected_rows > 0 {
            stage_generated(
                staging.path(),
                &mut payloads,
                "codex/metadata/threads.json",
                &metadata,
            )?;
        }
        counts.sqlite_threads = selected_rows;
    }

    if !request.skill_paths.is_empty() {
        counts.skills = stage_discovered_trees(
            &request.skill_paths,
            &request.codex_home.join("skills"),
            "codex/skills",
            staging.path(),
            &mut payloads,
            false,
        )?;
    }
    if !request.plugin_paths.is_empty() {
        counts.plugins = stage_discovered_trees(
            &request.plugin_paths,
            &request.codex_home.join("plugins").join("cache"),
            "codex/plugins/cache",
            staging.path(),
            &mut payloads,
            true,
        )?;
    }
    if !request.generated_image_paths.is_empty() {
        counts.generated_images = stage_discovered_files(
            &request.generated_image_paths,
            &request.codex_home.join("generated_images"),
            "codex/generated_images",
            staging.path(),
            &mut payloads,
        )?;
    }

    let package_id = Uuid::new_v4();
    let manifest = PackageManifest {
        format: FORMAT.to_owned(),
        schema_version: SCHEMA_VERSION,
        package_id,
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        source_os: inventory.source_os,
        source_arch: env::consts::ARCH.to_owned(),
        source_device_id: request.source_device_id,
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: counts.clone(),
        projects,
        conversations,
        exclusions: ExclusionSummary {
            excluded_files,
            excluded_bytes,
            rules: EXCLUSION_RULES
                .iter()
                .map(|rule| (*rule).to_owned())
                .collect(),
        },
    };

    let checksums = render_checksums(&payloads);
    ensure_control_size("checksums.sha256", checksums.len() as u64)?;
    write_staged_bytes(staging.path(), "checksums.sha256", checksums.as_bytes())?;
    // The manifest is deliberately materialized only after every payload and checksum.
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| package_invalid(format!("could not serialize manifest: {error}")))?;
    ensure_control_size("manifest.json", manifest_bytes.len() as u64)?;
    write_staged_bytes(staging.path(), "manifest.json", &manifest_bytes)?;

    write_archive_atomically(
        staging.path(),
        &request.output_path,
        &payloads,
        replace_existing,
    )?;
    let bytes_written = fs::metadata(&request.output_path)
        .map_err(|error| package_invalid(format!("could not inspect finished package: {error}")))?
        .len();

    Ok(CreatePackageReport {
        package_path: request.output_path,
        package_id,
        bytes_written,
        counts,
    })
}

#[derive(Debug)]
pub(crate) struct VerifiedPayload {
    pub content_hash: String,
    pub size_bytes: u64,
    pub archive_name: Option<String>,
    pub inline_bytes: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct VerifiedPackage {
    pub preview: PackagePreview,
    pub payloads: BTreeMap<String, VerifiedPayload>,
    pub planning_payloads: BTreeMap<String, Vec<u8>>,
}

impl VerifiedPackage {
    pub(crate) fn authenticated_planning_payload(
        &self,
        source: &str,
    ) -> Result<&[u8], RehomeError> {
        let bytes = self
            .planning_payloads
            .get(source)
            .ok_or_else(|| package_invalid("verified planning payload bytes are missing"))?;
        if bytes.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(package_invalid(
                "verified planning payload exceeds the inspection limit",
            ));
        }
        let verified = self
            .payloads
            .get(source)
            .ok_or_else(|| package_invalid("verified planning payload metadata is missing"))?;
        authenticate_payload_bytes(bytes, verified)?;
        Ok(bytes)
    }

    pub(crate) fn authenticated_payload(&self, source: &str) -> Result<Vec<u8>, RehomeError> {
        let verified = self
            .payloads
            .get(source)
            .ok_or_else(|| package_invalid("restore operation references a missing payload"))?;
        let mut file = fs::File::open(&self.preview.package_path)
            .map_err(|error| package_invalid(format!("could not reopen package: {error}")))?;
        let archive_hash = hash_archive_file(&mut file)?;
        if !archive_hash.eq_ignore_ascii_case(&self.preview.archive_hash) {
            return Err(package_invalid(
                "package archive changed after restore planning",
            ));
        }
        if let Some(bytes) = verified.inline_bytes.as_ref() {
            authenticate_payload_bytes(bytes, verified)?;
            return Ok(bytes.clone());
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|error| package_invalid(format!("could not rewind package: {error}")))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|error| package_invalid(format!("invalid ZIP container: {error}")))?;
        let archive_name = verified.archive_name.as_deref().unwrap_or(source);
        let mut entry = archive.by_name(archive_name).map_err(|error| {
            package_invalid(format!(
                "could not reopen verified payload {source}: {error}"
            ))
        })?;
        if entry.is_dir() {
            return Err(package_invalid("restore payload is not a regular file"));
        }
        let bytes = read_payload_entry(&mut entry)?;
        authenticate_payload_bytes(&bytes, verified)?;
        Ok(bytes)
    }
}

pub fn inspect_package(path: &Path) -> Result<PackagePreview, RehomeError> {
    Ok(inspect_package_for_planning(path)?.preview)
}

pub(crate) fn inspect_package_for_planning(path: &Path) -> Result<VerifiedPackage, RehomeError> {
    if let Some(package) = crate::core::legacy::inspect_schema_v3(path)? {
        return Ok(package);
    }
    let mut file = fs::File::open(path)
        .map_err(|error| package_invalid(format!("could not open package: {error}")))?;
    let archive_hash = hash_archive_file(&mut file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| package_invalid(format!("could not rewind package: {error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| package_invalid(format!("invalid ZIP container: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(package_invalid(
            "ZIP entry count exceeds the inspection limit",
        ));
    }

    let mut names = Vec::with_capacity(archive.len());
    let mut paths = PortablePathRegistry::default();
    let mut file_paths = HashSet::new();
    let mut payload_hashes = BTreeMap::new();
    let mut payloads = BTreeMap::new();
    let mut manifest_bytes = None;
    let mut checksum_bytes = None;
    let mut forbidden_files_total = 0_u64;
    let mut total_bytes = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| package_invalid(format!("could not read ZIP entry: {error}")))?;
        let raw_name = entry.name().to_owned();
        let normalized = validate_zip_entry_name(&raw_name, entry.is_dir())?;
        paths.insert(
            &normalized,
            if entry.is_dir() {
                ArchivePathKind::Directory
            } else {
                ArchivePathKind::File
            },
        )?;

        let preview_name = if entry.is_dir() {
            format!("{normalized}/")
        } else {
            normalized.clone()
        };
        if package_entry_is_forbidden(&normalized) {
            forbidden_files_total += 1;
        }
        names.push(preview_name);

        if entry.is_dir() {
            continue;
        }
        if entry.size() > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(package_invalid(
                "ZIP entry size exceeds the inspection limit",
            ));
        }
        if matches!(normalized.as_str(), "manifest.json" | "checksums.sha256") {
            ensure_control_size(&normalized, entry.size())?;
        }
        total_bytes = total_bytes
            .checked_add(entry.size())
            .ok_or_else(|| package_invalid("ZIP uncompressed size exceeds the inspection limit"))?;
        if total_bytes > MAX_INSPECTION_BYTES {
            return Err(package_invalid(
                "ZIP uncompressed size exceeds the inspection limit",
            ));
        }
        file_paths.insert(normalized.clone());
        match normalized.as_str() {
            "manifest.json" => manifest_bytes = Some(read_control_entry(&mut entry, &normalized)?),
            "checksums.sha256" => {
                checksum_bytes = Some(read_control_entry(&mut entry, &normalized)?)
            }
            _ => {
                let size_bytes = entry.size();
                let content_hash = hash_reader(&mut entry)?;
                payload_hashes.insert(normalized.clone(), content_hash.clone());
                payloads.insert(
                    normalized,
                    VerifiedPayload {
                        content_hash,
                        size_bytes,
                        archive_name: None,
                        inline_bytes: None,
                    },
                );
            }
        }
    }

    let manifest_bytes = manifest_bytes
        .as_deref()
        .ok_or_else(|| package_invalid("manifest.json is missing"))?;
    let manifest: PackageManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|error| package_invalid(format!("manifest.json is invalid: {error}")))?;
    if manifest.format != FORMAT {
        return Err(package_invalid("manifest format is not codex-rehome"));
    }
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(RehomeError::new(
            ErrorCode::UnsupportedSchema,
            format!("unsupported package schema {}", manifest.schema_version),
        ));
    }
    validate_manifest_archive_paths(&manifest, &file_paths, &payload_hashes)?;

    let checksum_bytes = checksum_bytes
        .as_deref()
        .ok_or_else(|| package_invalid("checksums.sha256 is missing"))?;
    verify_checksums(checksum_bytes, &payload_hashes)?;

    let mut planning_sources = manifest
        .conversations
        .iter()
        .map(|conversation| conversation.archive_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for source in ["codex/session_index.jsonl", "codex/metadata/threads.json"] {
        if payloads.contains_key(source) {
            planning_sources.insert(source.to_owned());
        }
    }
    let mut planning_payloads = BTreeMap::new();
    for source in planning_sources {
        let mut entry = archive.by_name(&source).map_err(|error| {
            package_invalid(format!(
                "could not reopen verified planning payload: {error}"
            ))
        })?;
        let bytes = read_payload_entry(&mut entry)?;
        let payload = payloads
            .get(&source)
            .ok_or_else(|| package_invalid("verified planning payload metadata is missing"))?;
        authenticate_payload_bytes(&bytes, payload)?;
        planning_payloads.insert(source, bytes);
    }

    let mut file = archive.into_inner();
    file.seek(SeekFrom::Start(0))
        .map_err(|error| package_invalid(format!("could not rewind package: {error}")))?;
    let final_archive_hash = hash_archive_file(&mut file)?;
    if final_archive_hash != archive_hash {
        return Err(package_invalid(
            "package changed while it was being inspected",
        ));
    }

    names.sort();
    Ok(VerifiedPackage {
        preview: PackagePreview {
            package_path: path.to_path_buf(),
            archive_hash,
            manifest,
            checksum_valid: true,
            entries: names,
            forbidden_files_total,
        },
        payloads,
        planning_payloads,
    })
}

#[derive(Clone)]
struct Payload {
    hash: String,
    executable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchivePathKind {
    File,
    Directory,
}

#[derive(Default)]
struct PortablePathRegistry {
    entries: BTreeMap<String, ArchivePathKind>,
}

impl PortablePathRegistry {
    fn insert(&mut self, path: &str, kind: ArchivePathKind) -> Result<(), RehomeError> {
        let key = portable_collision_key(path);
        if self.entries.contains_key(&key) {
            return Err(package_invalid("portable archive path collision"));
        }
        for ancestor in portable_ancestors(&key) {
            if self.entries.get(ancestor) == Some(&ArchivePathKind::File) {
                return Err(package_invalid(
                    "portable archive path collision between a file and descendant",
                ));
            }
        }
        if kind == ArchivePathKind::File {
            let descendant_prefix = format!("{key}/");
            if self
                .entries
                .keys()
                .any(|existing| existing.starts_with(&descendant_prefix))
            {
                return Err(package_invalid(
                    "portable archive path collision between a file and descendant",
                ));
            }
        }
        self.entries.insert(key, kind);
        Ok(())
    }
}

struct PayloadCollection {
    entries: BTreeMap<String, Payload>,
    paths: PortablePathRegistry,
}

impl PayloadCollection {
    fn new() -> Result<Self, RehomeError> {
        let mut paths = PortablePathRegistry::default();
        paths.insert("manifest.json", ArchivePathKind::File)?;
        paths.insert("checksums.sha256", ArchivePathKind::File)?;
        Ok(Self {
            entries: BTreeMap::new(),
            paths,
        })
    }

    fn insert(&mut self, path: String, payload: Payload) -> Result<(), RehomeError> {
        self.paths.insert(&path, ArchivePathKind::File)?;
        self.entries.insert(path, payload);
        Ok(())
    }
}

#[derive(Default)]
struct SessionIndexMetadata {
    by_id: BTreeMap<Uuid, Value>,
}

struct ConversationSelection<'a> {
    paths: &'a [PathBuf],
    codex_home: &'a Path,
    selected_ids: &'a [Uuid],
    index: &'a SessionIndexMetadata,
    projects: &'a [ProjectEntry],
}

fn complete_session_index(
    index: &SessionIndexMetadata,
    conversations: &[ConversationEntry],
) -> Result<Vec<u8>, RehomeError> {
    let mut bytes = Vec::new();
    for conversation in conversations {
        let value = index
            .by_id
            .get(&conversation.task_id)
            .cloned()
            .unwrap_or_else(|| {
                serde_json::json!({
                    "id": conversation.task_id,
                    "thread_name": conversation.title,
                    "title": conversation.title,
                    "updated_at": conversation.updated_at,
                    "project_id": conversation.project_id,
                })
            });
        serde_json::to_writer(&mut bytes, &value)
            .map_err(|error| package_invalid(format!("could not encode session index: {error}")))?;
        bytes.push(b'\n');
        ensure_control_size("codex/session_index.jsonl", bytes.len() as u64)?;
    }
    Ok(bytes)
}

fn stage_projects(
    project_paths: &[PathBuf],
    staging_root: &Path,
    payloads: &mut PayloadCollection,
    counts: &mut ContentCounts,
) -> Result<(Vec<ProjectEntry>, (u64, u64)), RehomeError> {
    let mut projects = Vec::new();
    let mut excluded_files = 0_u64;
    let mut excluded_bytes = 0_u64;
    let mut unique_roots = HashSet::new();

    for source_root in project_paths {
        let canonical = source_root.canonicalize().map_err(|error| {
            package_invalid(format!("selected project cannot be resolved: {error}"))
        })?;
        let root_metadata = fs::symlink_metadata(source_root).map_err(io_package_error)?;
        if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
            return Err(package_invalid("selected project must be a real directory"));
        }
        if !unique_roots.insert(canonical.clone()) {
            return Err(package_invalid("selected project is duplicated"));
        }

        let project_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, canonical.to_string_lossy().as_bytes());
        let archive_root = format!("projects/{project_id}/files");
        let mut file_count = 0_u64;
        let mut content_bytes = 0_u64;

        let walker = WalkDir::new(&canonical)
            .follow_links(false)
            .sort_by_file_name();
        for entry in walker {
            let entry = entry.map_err(|error| {
                package_invalid(format!("could not walk selected project: {error}"))
            })?;
            if entry.path() == canonical {
                continue;
            }
            if entry.file_type().is_symlink() {
                let length = fs::symlink_metadata(entry.path())
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                excluded_files += 1;
                excluded_bytes += length;
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&canonical)
                .map_err(|_| package_invalid("selected project entry escapes the project root"))?;
            if is_forbidden(relative) {
                let length = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                excluded_files += 1;
                excluded_bytes += length;
                continue;
            }
            let relative = normalize_entry(relative)?;
            let archive_path = format!("{archive_root}/{relative}");
            stage_source(entry.path(), staging_root, payloads, &archive_path)?;
            let length = entry
                .metadata()
                .map_err(|error| {
                    package_invalid(format!("could not inspect selected project file: {error}"))
                })?
                .len();
            file_count += 1;
            content_bytes += length;
        }

        let name = canonical
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project")
            .to_owned();
        let project = ProjectEntry {
            project_id,
            name,
            source_path: canonical.to_string_lossy().into_owned(),
            archive_path: archive_root,
            file_count,
            content_bytes,
            git_remote: None,
            git_branch: None,
            git_head: None,
        };
        let project_json = serde_json::to_vec_pretty(&project)
            .map_err(|error| package_invalid(format!("could not serialize project: {error}")))?;
        stage_generated(
            staging_root,
            payloads,
            &format!("projects/{project_id}/project.json"),
            &project_json,
        )?;
        counts.projects += 1;
        counts.project_files += file_count;
        projects.push(project);
    }
    projects.sort_by_key(|project| project.project_id);
    Ok((projects, (excluded_files, excluded_bytes)))
}

fn stage_conversations(
    selection: ConversationSelection<'_>,
    staging_root: &Path,
    payloads: &mut PayloadCollection,
    counts: &mut ContentCounts,
) -> Result<Vec<ConversationEntry>, RehomeError> {
    let selected: HashSet<Uuid> = selection.selected_ids.iter().copied().collect();
    if selected.len() != selection.selected_ids.len() {
        return Err(package_invalid("selected conversation ID is duplicated"));
    }
    let mut found = HashSet::new();
    let mut conversations = Vec::new();

    for source in selection.paths {
        let Some(session) = session_identity_from_file(source)? else {
            continue;
        };
        let task_id = session.task_id;
        if !selected.contains(&task_id) {
            continue;
        }
        if !found.insert(task_id) {
            return Err(package_invalid(
                "selected conversation has multiple session files",
            ));
        }

        let relative = source
            .strip_prefix(selection.codex_home)
            .map_err(|_| package_invalid("conversation path escapes the selected Codex home"))?;
        let archive_path = format!("codex/{}", normalize_entry(relative)?);
        let archive_path = normalize_entry(Path::new(&archive_path))?;
        let staged_payload = copy_source_to_staging(source, staging_root, &archive_path)?;
        let session_value = session.fields;
        let content_hash = staged_payload.hash.clone();
        payloads.insert(archive_path.clone(), staged_payload)?;
        let metadata = selection
            .index
            .by_id
            .get(&task_id)
            .unwrap_or(&session_value);
        conversations.push(ConversationEntry {
            task_id,
            project_id: associated_project_id(metadata, &session_value, selection.projects),
            title: metadata_string(metadata, &["title", "thread_name"])
                .or_else(|| metadata_string(&session_value, &["title", "thread_name"]))
                .unwrap_or_else(|| "Imported conversation".to_owned()),
            updated_at: metadata_string(metadata, &["updated_at", "timestamp"])
                .or_else(|| metadata_string(&session_value, &["updated_at", "timestamp"]))
                .unwrap_or_default(),
            content_hash,
            archive_path,
            classification: conversation_classification(&session_value),
        });
    }

    if found != selected {
        return Err(package_invalid(
            "one or more selected conversations were not found",
        ));
    }
    conversations.sort_by_key(|conversation| conversation.task_id);
    counts.conversations = conversations.len() as u64;
    Ok(conversations)
}

fn read_selected_session_index(
    path: Option<&Path>,
    selected_ids: &[Uuid],
) -> Result<SessionIndexMetadata, RehomeError> {
    let Some(path) = path else {
        return Ok(SessionIndexMetadata::default());
    };
    let selected: HashSet<Uuid> = selected_ids.iter().copied().collect();
    let mut result = SessionIndexMetadata::default();
    let before = source_fingerprint(path)?;
    let file = fs::File::open(path).map_err(io_package_error)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    while read_bounded_line(&mut reader, &mut line)? {
        let line = std::str::from_utf8(strip_line_ending(&line))
            .map_err(|_| package_invalid("session index is not UTF-8"))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| package_invalid("session index contains malformed JSONL"))?;
        let Some(id) = metadata_uuid(&value, &["id", "thread_id"]) else {
            continue;
        };
        if selected.contains(&id) {
            // Source order is stable, so the last row is the deterministic winner.
            result.by_id.insert(id, value);
        }
    }
    if before != source_fingerprint(path)? {
        return Err(package_invalid(
            "source file changed in size or modification time while being read",
        ));
    }
    Ok(result)
}

fn export_selected_threads(
    database: &Path,
    selected_ids: &[Uuid],
) -> Result<(Vec<u8>, u64), RehomeError> {
    let snapshot = StateDatabaseSnapshot::create(database).map_err(|error| {
        package_invalid(format!("could not snapshot Codex state metadata: {error}"))
    })?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection =
        Connection::open_with_flags(snapshot.database_path(), flags).map_err(|error| {
            package_invalid(format!("could not read Codex state metadata: {error}"))
        })?;
    let available_columns = thread_table_columns(&connection)?;
    if !available_columns.contains("id") {
        return Err(package_invalid("Codex threads table has no id column"));
    }
    let columns: Vec<&str> = THREAD_EXPORT_COLUMNS_V1
        .iter()
        .copied()
        .filter(|column| available_columns.contains(*column))
        .collect();
    let query = format!("SELECT {} FROM threads ORDER BY rowid", columns.join(", "));
    let mut statement = connection
        .prepare(&query)
        .map_err(|error| package_invalid(format!("could not read Codex threads: {error}")))?;
    let selected: HashSet<String> = selected_ids.iter().map(Uuid::to_string).collect();
    let mut rows = statement
        .query([])
        .map_err(|error| package_invalid(format!("could not query Codex threads: {error}")))?;
    let mut bytes = vec![b'[', b'\n'];
    let mut count = 0_u64;
    while let Some(row) = rows
        .next()
        .map_err(|error| package_invalid(format!("could not read Codex thread row: {error}")))?
    {
        let id = row
            .get_ref(0)
            .ok()
            .and_then(|value| value.as_str().ok())
            .map(str::to_owned);
        if !id.as_ref().is_some_and(|id| selected.contains(id)) {
            continue;
        }
        let mut object = Map::new();
        for (index, column) in columns.iter().enumerate() {
            let value = row.get_ref(index).map_err(|error| {
                package_invalid(format!("could not read Codex thread field: {error}"))
            })?;
            object.insert((*column).to_owned(), sqlite_json_value(value)?);
        }
        let encoded = serde_json::to_vec(&Value::Object(object))
            .map_err(|error| package_invalid(format!("could not encode Codex thread: {error}")))?;
        let separator_bytes = usize::from(count > 0) * 2;
        let projected_size = bytes
            .len()
            .checked_add(separator_bytes)
            .and_then(|size| size.checked_add(encoded.len()))
            .and_then(|size| size.checked_add(3))
            .ok_or_else(|| {
                package_invalid("Codex thread metadata exceeds the control-file limit")
            })?;
        ensure_control_size("codex/metadata/threads.json", projected_size as u64)?;
        if count > 0 {
            bytes.extend_from_slice(b",\n");
        }
        bytes.extend_from_slice(&encoded);
        count += 1;
    }
    bytes.extend_from_slice(b"\n]\n");
    ensure_control_size("codex/metadata/threads.json", bytes.len() as u64)?;
    Ok((bytes, count))
}

fn thread_table_columns(connection: &Connection) -> Result<HashSet<String>, RehomeError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| package_invalid(format!("could not inspect Codex threads: {error}")))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| package_invalid(format!("could not inspect Codex threads: {error}")))?;
    let mut result = HashSet::new();
    for column in columns {
        result.insert(
            column
                .map_err(|error| {
                    package_invalid(format!("could not inspect Codex thread column: {error}"))
                })?
                .to_ascii_lowercase(),
        );
    }
    Ok(result)
}

fn sqlite_json_value(value: ValueRef<'_>) -> Result<Value, RehomeError> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::from(value),
        ValueRef::Real(value) => Value::from(value),
        ValueRef::Text(value) => {
            ensure_control_size("Codex thread text field", value.len() as u64)?;
            Value::String(String::from_utf8_lossy(value).into_owned())
        }
        ValueRef::Blob(value) => {
            if value.len() as u64 > (MAX_CONTROL_FILE_BYTES - 4) / 2 {
                return Err(package_invalid(
                    "Codex thread blob field exceeds the control-file limit",
                ));
            }
            Value::String(format!("hex:{}", hex_bytes(value)))
        }
    })
}

fn stage_discovered_files(
    sources: &[PathBuf],
    source_root: &Path,
    archive_root: &str,
    staging_root: &Path,
    payloads: &mut PayloadCollection,
) -> Result<u64, RehomeError> {
    let mut count = 0_u64;
    for source in sources {
        let relative = source
            .strip_prefix(source_root)
            .map_err(|_| package_invalid("discovered Codex content escapes its expected root"))?;
        if is_forbidden(relative) {
            continue;
        }
        let archive_path = format!("{archive_root}/{}", normalize_entry(relative)?);
        stage_source(source, staging_root, payloads, &archive_path)?;
        count += 1;
    }
    Ok(count)
}

fn stage_discovered_trees(
    marker_files: &[PathBuf],
    source_root: &Path,
    archive_root: &str,
    staging_root: &Path,
    payloads: &mut PayloadCollection,
    expand_plugin_root: bool,
) -> Result<u64, RehomeError> {
    let mut roots = BTreeMap::new();
    for marker in marker_files {
        let marker_parent = marker
            .parent()
            .ok_or_else(|| package_invalid("discovered bundle marker has no parent"))?;
        let bundle_root = if expand_plugin_root
            && marker_parent.file_name().and_then(|name| name.to_str()) == Some(".codex-plugin")
        {
            marker_parent
                .parent()
                .ok_or_else(|| package_invalid("plugin marker has no version root"))?
        } else {
            marker_parent
        };
        let canonical = bundle_root.canonicalize().map_err(io_package_error)?;
        roots.insert(canonical, bundle_root.to_path_buf());
    }

    for bundle_root in roots.values() {
        let bundle_relative = bundle_root
            .strip_prefix(source_root)
            .map_err(|_| package_invalid("discovered Codex bundle escapes its expected root"))?;
        let bundle_archive_root = format!("{archive_root}/{}", normalize_entry(bundle_relative)?);
        for entry in WalkDir::new(bundle_root)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry = entry.map_err(|error| {
                package_invalid(format!("could not walk discovered Codex bundle: {error}"))
            })?;
            if entry.path() == bundle_root {
                continue;
            }
            if entry.file_type().is_symlink() {
                return Err(package_invalid(
                    "symbolic links are not allowed in selected Codex bundles",
                ));
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(bundle_root)
                .map_err(|_| package_invalid("discovered bundle entry escapes its bundle root"))?;
            if is_forbidden(relative) {
                continue;
            }
            let archive_path = format!("{bundle_archive_root}/{}", normalize_entry(relative)?);
            stage_source(entry.path(), staging_root, payloads, &archive_path)?;
        }
    }
    Ok(roots.len() as u64)
}

fn stage_source(
    source: &Path,
    staging_root: &Path,
    payloads: &mut PayloadCollection,
    archive_path: &str,
) -> Result<(), RehomeError> {
    let archive_path = normalize_entry(Path::new(archive_path))?;
    let payload = copy_source_to_staging(source, staging_root, &archive_path)?;
    payloads.insert(archive_path, payload)
}

fn copy_source_to_staging(
    source: &Path,
    staging_root: &Path,
    archive_path: &str,
) -> Result<Payload, RehomeError> {
    let before = source_fingerprint(source)?;
    if before.length > MAX_ARCHIVE_ENTRY_BYTES {
        return Err(package_invalid(
            "package source entry exceeds the size limit",
        ));
    }
    let destination = staging_root.join(Path::new(&archive_path));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_package_error)?;
    }
    let mut reader = fs::File::open(source).map_err(io_package_error)?;
    let mut writer = fs::File::create(&destination).map_err(io_package_error)?;
    let hash = copy_and_hash(&mut reader, &mut writer)?;
    writer.sync_all().map_err(io_package_error)?;
    drop(writer);
    if before != source_fingerprint(source)? {
        return Err(package_invalid(
            "source file changed in size or modification time while being copied",
        ));
    }
    Ok(Payload {
        hash,
        executable: source_is_executable(source),
    })
}

fn stage_generated(
    staging_root: &Path,
    payloads: &mut PayloadCollection,
    archive_path: &str,
    bytes: &[u8],
) -> Result<(), RehomeError> {
    let archive_path = normalize_entry(Path::new(archive_path))?;
    write_staged_bytes(staging_root, &archive_path, bytes)?;
    payloads.insert(
        archive_path,
        Payload {
            hash: sha256_hex(bytes),
            executable: false,
        },
    )
}

fn write_staged_bytes(root: &Path, archive_path: &str, bytes: &[u8]) -> Result<(), RehomeError> {
    let destination = root.join(Path::new(archive_path));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_package_error)?;
    }
    fs::write(destination, bytes).map_err(io_package_error)
}

fn write_archive_atomically(
    staging_root: &Path,
    output_path: &Path,
    payloads: &PayloadCollection,
    replace_existing: bool,
) -> Result<(), RehomeError> {
    let output_parent = usable_parent(output_path);
    let mut temporary = NamedTempFile::new_in(output_parent)
        .map_err(|error| package_invalid(format!("could not create package temp file: {error}")))?;
    {
        let mut writer = ZipWriter::new(temporary.as_file_mut());
        let entries = staged_archive_entries(staging_root, payloads)?;
        for entry in entries {
            let options = stable_options(entry.permissions);
            if entry.is_directory {
                writer
                    .add_directory(entry.name, options)
                    .map_err(zip_package_error)?;
            } else {
                writer
                    .start_file(&entry.name, options)
                    .map_err(zip_package_error)?;
                let mut source = fs::File::open(staging_root.join(Path::new(&entry.name)))
                    .map_err(io_package_error)?;
                io::copy(&mut source, &mut writer).map_err(io_package_error)?;
            }
        }
        writer.finish().map_err(zip_package_error)?;
    }
    temporary.as_file().sync_all().map_err(io_package_error)?;
    publish_archive(temporary.path(), output_path, replace_existing).map_err(|error| {
        package_invalid(format!("could not atomically publish package: {error}"))
    })?;
    drop(temporary);
    Ok(())
}

#[cfg(windows)]
fn publish_archive(source: &Path, destination: &Path, replace_existing: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace_existing {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    let moved = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) };
    if moved == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn publish_archive(source: &Path, destination: &Path, replace_existing: bool) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "target path contains NUL"))?;
    let result = if replace_existing {
        unsafe { libc::rename(source.as_ptr(), destination.as_ptr()) }
    } else {
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) }
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn publish_archive(source: &Path, destination: &Path, replace_existing: bool) -> io::Result<()> {
    if replace_existing {
        fs::rename(source, destination)
    } else {
        fs::hard_link(source, destination)
    }
}

struct ArchiveEntry {
    name: String,
    is_directory: bool,
    permissions: u32,
}

fn staged_archive_entries(
    staging_root: &Path,
    payloads: &PayloadCollection,
) -> Result<Vec<ArchiveEntry>, RehomeError> {
    let mut entries = Vec::new();
    let mut total_bytes = 0_u64;
    for entry in WalkDir::new(staging_root).sort_by_file_name() {
        let entry = entry.map_err(|error| {
            package_invalid(format!("could not enumerate package staging: {error}"))
        })?;
        if entry.path() == staging_root {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(staging_root)
            .map_err(|_| package_invalid("staged package entry escapes staging"))?;
        let name = normalize_entry(relative)?;
        if entry.file_type().is_dir() {
            entries.push(ArchiveEntry {
                name: format!("{name}/"),
                is_directory: true,
                permissions: 0o755,
            });
        } else if entry.file_type().is_file() {
            let size = entry
                .metadata()
                .map_err(|error| {
                    package_invalid(format!("could not inspect package staging: {error}"))
                })?
                .len();
            if size > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(package_invalid(
                    "staged package entry exceeds the size limit",
                ));
            }
            total_bytes = total_bytes
                .checked_add(size)
                .ok_or_else(|| package_invalid("staged package exceeds the size limit"))?;
            if total_bytes > MAX_INSPECTION_BYTES {
                return Err(package_invalid("staged package exceeds the size limit"));
            }
            let executable = payloads
                .entries
                .get(&name)
                .map(|payload| payload.executable)
                .unwrap_or(false);
            entries.push(ArchiveEntry {
                name,
                is_directory: false,
                permissions: if executable { 0o755 } else { 0o644 },
            });
        } else {
            return Err(package_invalid("staging contains a non-regular entry"));
        }
    }
    if entries.len() > MAX_ARCHIVE_ENTRIES {
        return Err(package_invalid(
            "staged package entry count exceeds the limit",
        ));
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn stable_options(permissions: u32) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(permissions)
}

fn render_checksums(payloads: &PayloadCollection) -> String {
    let mut checksums = String::new();
    for (path, payload) in &payloads.entries {
        checksums.push_str(&payload.hash);
        checksums.push_str("  ");
        checksums.push_str(path);
        checksums.push('\n');
    }
    checksums
}

fn verify_checksums(
    checksum_bytes: &[u8],
    payload_hashes: &BTreeMap<String, String>,
) -> Result<(), RehomeError> {
    if checksum_bytes.starts_with(&[0xef, 0xbb, 0xbf]) || checksum_bytes.contains(&b'\r') {
        return Err(package_invalid(
            "checksums.sha256 must be LF UTF-8 without a BOM",
        ));
    }
    let text = std::str::from_utf8(checksum_bytes)
        .map_err(|_| package_invalid("checksums.sha256 is not UTF-8"))?;
    let mut expected = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (hash, path) = line
            .split_once("  ")
            .ok_or_else(|| package_invalid("checksums.sha256 has an invalid line"))?;
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(package_invalid("checksums.sha256 has an invalid hash"));
        }
        let normalized = validate_zip_entry_name(path, false)?;
        if matches!(normalized.as_str(), "manifest.json" | "checksums.sha256") {
            return Err(package_invalid(
                "checksums.sha256 references a control file",
            ));
        }
        if expected
            .insert(normalized, hash.to_ascii_lowercase())
            .is_some()
        {
            return Err(package_invalid("checksums.sha256 has a duplicate path"));
        }
    }

    let payload_paths: HashSet<&str> = payload_hashes.keys().map(String::as_str).collect();
    let checksum_paths: HashSet<&str> = expected.keys().map(String::as_str).collect();
    if payload_paths != checksum_paths {
        return Err(RehomeError::new(
            ErrorCode::ChecksumMismatch,
            "checksum coverage does not match package payloads",
        ));
    }
    for (path, expected_hash) in expected {
        let actual_hash = payload_hashes.get(&path).ok_or_else(|| {
            RehomeError::new(
                ErrorCode::ChecksumMismatch,
                "checksummed payload is missing",
            )
        })?;
        if actual_hash != &expected_hash {
            return Err(RehomeError::new(
                ErrorCode::ChecksumMismatch,
                format!("checksum mismatch for {path}"),
            ));
        }
    }
    Ok(())
}

fn validate_manifest_archive_paths(
    manifest: &PackageManifest,
    file_paths: &HashSet<String>,
    payload_hashes: &BTreeMap<String, String>,
) -> Result<(), RehomeError> {
    let mut referenced_conversations = PortablePathRegistry::default();
    for conversation in &manifest.conversations {
        let path = validate_manifest_archive_path(&conversation.archive_path)?;
        if !path.starts_with("codex/sessions/") && !path.starts_with("codex/archived_sessions/") {
            return Err(package_invalid(
                "manifest conversation path is outside the expected Codex session prefixes",
            ));
        }
        referenced_conversations.insert(&path, ArchivePathKind::File)?;
        let actual_hash = payload_hashes.get(&path).ok_or_else(|| {
            package_invalid("manifest conversation references a missing package payload")
        })?;
        if actual_hash != &conversation.content_hash.to_ascii_lowercase() {
            return Err(package_invalid(
                "manifest conversation content hash does not match its package payload",
            ));
        }
    }

    for project in &manifest.projects {
        let expected_root = format!("projects/{}/files", project.project_id);
        let path = validate_manifest_archive_path(&project.archive_path)?;
        if path != expected_root {
            return Err(package_invalid(
                "manifest project path does not match its expected package prefix",
            ));
        }
        let project_metadata = format!("projects/{}/project.json", project.project_id);
        if !file_paths.contains(&project_metadata) {
            return Err(package_invalid(
                "manifest project references missing project metadata",
            ));
        }
        if project.file_count > 0 {
            let prefix = format!("{path}/");
            if !payload_hashes
                .keys()
                .any(|entry| entry.starts_with(&prefix))
            {
                return Err(package_invalid(
                    "manifest project references missing project payloads",
                ));
            }
        }
    }
    Ok(())
}

fn validate_manifest_archive_path(path: &str) -> Result<String, RehomeError> {
    let normalized = validate_zip_entry_name(path, false)
        .map_err(|error| package_invalid(format!("manifest archive path is invalid: {error}")))?;
    if normalized != path {
        return Err(package_invalid("manifest archive path is not normalized"));
    }
    Ok(normalized)
}

fn read_control_entry<R: Read>(reader: &mut R, name: &str) -> Result<Vec<u8>, RehomeError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_CONTROL_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| package_invalid(format!("could not read ZIP control file: {error}")))?;
    ensure_control_size(name, bytes.len() as u64)?;
    Ok(bytes)
}

fn read_payload_entry<R: Read>(reader: &mut R) -> Result<Vec<u8>, RehomeError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_ARCHIVE_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| package_invalid(format!("could not read ZIP payload: {error}")))?;
    if bytes.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
        return Err(package_invalid(
            "ZIP entry size exceeds the inspection limit",
        ));
    }
    Ok(bytes)
}

fn authenticate_payload_bytes(bytes: &[u8], verified: &VerifiedPayload) -> Result<(), RehomeError> {
    let content_hash = format!("{:x}", Sha256::digest(bytes));
    if bytes.len() as u64 != verified.size_bytes
        || !content_hash.eq_ignore_ascii_case(&verified.content_hash)
    {
        return Err(RehomeError::new(
            ErrorCode::ChecksumMismatch,
            "ZIP payload changed after checksum verification",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod authenticated_payload_tests {
    use super::*;

    #[test]
    fn rejects_consumed_payload_bytes_that_do_not_match_the_verified_hash() {
        let verified = VerifiedPayload {
            content_hash: format!("{:x}", Sha256::digest(b"verified bytes")),
            size_bytes: b"verified bytes".len() as u64,
            archive_name: None,
            inline_bytes: None,
        };

        let error = authenticate_payload_bytes(b"tampered bytes", &verified).unwrap_err();

        assert_eq!(error.code, ErrorCode::ChecksumMismatch);
    }
}

fn hash_archive_file(file: &mut fs::File) -> Result<String, RehomeError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
    let mut bytes_read = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| package_invalid(format!("could not hash package: {error}")))?;
        if count == 0 {
            break;
        }
        bytes_read = bytes_read
            .checked_add(count as u64)
            .ok_or_else(|| package_invalid("package file exceeds the inspection limit"))?;
        if bytes_read > MAX_ARCHIVE_FILE_BYTES {
            return Err(package_invalid(
                "package file size exceeds the inspection limit",
            ));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_bytes(&hasher.finalize()))
}

fn ensure_control_size(name: &str, size: u64) -> Result<(), RehomeError> {
    if size > MAX_CONTROL_FILE_BYTES {
        return Err(package_invalid(format!(
            "ZIP control file size exceeds the inspection limit: {name}"
        )));
    }
    Ok(())
}

fn hash_reader<R: Read>(reader: &mut R) -> Result<String, RehomeError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
    let mut bytes_read = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| package_invalid(format!("could not stream ZIP payload: {error}")))?;
        if count == 0 {
            break;
        }
        bytes_read = bytes_read
            .checked_add(count as u64)
            .ok_or_else(|| package_invalid("ZIP entry size exceeds the inspection limit"))?;
        if bytes_read > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(package_invalid(
                "ZIP entry size exceeds the inspection limit",
            ));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_bytes(&hasher.finalize()))
}

fn copy_and_hash<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<String, RehomeError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
    loop {
        let count = reader.read(&mut buffer).map_err(io_package_error)?;
        if count == 0 {
            break;
        }
        writer
            .write_all(&buffer[..count])
            .map_err(io_package_error)?;
        hasher.update(&buffer[..count]);
    }
    Ok(hex_bytes(&hasher.finalize()))
}

fn portable_collision_key(path: &str) -> String {
    path.split('/')
        .map(|component| {
            component
                .nfc()
                .flat_map(char::to_lowercase)
                .collect::<String>()
                .nfc()
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn portable_ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(|(index, _)| &path[..index])
}

fn validate_zip_entry_name(raw_name: &str, is_directory: bool) -> Result<String, RehomeError> {
    if raw_name.contains('\\') {
        return Err(package_invalid(
            "backslashes are not allowed in ZIP entry names",
        ));
    }
    let candidate = if is_directory {
        raw_name
            .strip_suffix('/')
            .ok_or_else(|| package_invalid("ZIP directory entry has no trailing slash"))?
    } else {
        raw_name
    };
    if candidate.is_empty() || (!is_directory && raw_name.ends_with('/')) {
        return Err(package_invalid(
            "ZIP entry name is empty or has the wrong type",
        ));
    }
    normalize_entry(Path::new(candidate))
}

fn package_entry_is_forbidden(entry: &str) -> bool {
    const PLUGIN_CACHE_ROOT: &str = "codex/plugins/cache";
    if entry == PLUGIN_CACHE_ROOT {
        return false;
    }
    if let Some(relative) = entry.strip_prefix(&format!("{PLUGIN_CACHE_ROOT}/")) {
        return is_forbidden(Path::new(relative));
    }
    is_forbidden(Path::new(entry))
}

fn validate_output_path(path: &Path, replace_existing: bool) -> Result<(), RehomeError> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(package_invalid("package output path is invalid"));
    }
    if path.exists() && !replace_existing {
        return Err(package_invalid("package output path already exists"));
    }
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(io_package_error)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(package_invalid(
                "existing package output is not a regular file and cannot be replaced",
            ));
        }
    }
    Ok(())
}

fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[derive(Debug, PartialEq, Eq)]
struct SourceFingerprint {
    length: u64,
    modified: SystemTime,
}

fn source_fingerprint(path: &Path) -> Result<SourceFingerprint, RehomeError> {
    let metadata = fs::symlink_metadata(path).map_err(io_package_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(package_invalid("package source is not a regular file"));
    }
    Ok(SourceFingerprint {
        length: metadata.len(),
        modified: metadata.modified().map_err(io_package_error)?,
    })
}

fn session_identity_from_file(path: &Path) -> Result<Option<SessionMetadata>, RehomeError> {
    let file = fs::File::open(path).map_err(io_package_error)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    while read_bounded_line(&mut reader, &mut line)? {
        let Ok(line) = std::str::from_utf8(strip_line_ending(&line)) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(metadata) = session_metadata_from_value(value) {
            return Ok(Some(metadata));
        }
    }
    Ok(None)
}

fn read_bounded_line<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> Result<bool, RehomeError> {
    line.clear();
    loop {
        let available = reader.fill_buf().map_err(io_package_error)?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len() + length > MAX_JSONL_LINE_BYTES {
            return Err(package_invalid("JSONL line exceeds the control-file limit"));
        }
        line.extend_from_slice(&available[..length]);
        let complete = available[length - 1] == b'\n';
        reader.consume(length);
        if complete {
            return Ok(true);
        }
    }
}

fn strip_line_ending(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(&Sha256::digest(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(unix)]
fn source_is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn source_is_executable(_path: &Path) -> bool {
    false
}

fn private_app_temp_root() -> Result<PathBuf, RehomeError> {
    let root = private_app_temp_path();
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(package_invalid(
                "private application temp root is not a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(&root).map_err(io_package_error)?;
        }
        Err(error) => return Err(io_package_error(error)),
    }
    make_staging_private(&root)?;
    root.canonicalize().map_err(io_package_error)
}

#[cfg(target_os = "windows")]
fn private_app_temp_path() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("CodexRehome")
        .join("Temp")
}

#[cfg(target_os = "macos")]
fn private_app_temp_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("Library")
        .join("Caches")
        .join("CodexRehome")
        .join("Temp")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn private_app_temp_path() -> PathBuf {
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("codex-rehome");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join(".cache")
        .join("codex-rehome")
        .join("tmp")
}

#[cfg(not(any(unix, target_os = "windows")))]
fn private_app_temp_path() -> PathBuf {
    env::temp_dir().join("codex-rehome")
}

fn validate_staging_location(
    staging_root: &Path,
    output_parent: &Path,
    project_paths: &[PathBuf],
    codex_home: &Path,
) -> Result<(), RehomeError> {
    let mut forbidden_roots = Vec::with_capacity(project_paths.len() + 2);
    forbidden_roots.push(output_parent.canonicalize().map_err(io_package_error)?);
    forbidden_roots.push(codex_home.canonicalize().map_err(io_package_error)?);
    for project in project_paths {
        forbidden_roots.push(project.canonicalize().map_err(io_package_error)?);
    }
    if forbidden_roots
        .iter()
        .any(|root| staging_root.starts_with(root))
    {
        return Err(package_invalid(
            "private staging cannot be inside a source project or package output directory",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn make_staging_private(path: &Path) -> Result<(), RehomeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_package_error)
}

#[cfg(not(unix))]
fn make_staging_private(_path: &Path) -> Result<(), RehomeError> {
    Ok(())
}

fn package_invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}

fn io_package_error(error: io::Error) -> RehomeError {
    package_invalid(format!("package I/O failed: {error}"))
}

fn zip_package_error(error: zip::result::ZipError) -> RehomeError {
    package_invalid(format!("ZIP write failed: {error}"))
}
