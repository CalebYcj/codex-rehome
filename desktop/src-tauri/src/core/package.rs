use crate::core::{
    discovery::{discover_codex, StateDatabaseSnapshot},
    error::{ErrorCode, RehomeError},
    exclusions::is_forbidden,
    models::{
        ContentCounts, ConversationEntry, CreatePackageReport, CreatePackageRequest,
        ExclusionSummary, PackageManifest, PackageMode, PackagePreview, ProjectEntry,
    },
    paths::normalize_entry,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};
use tempfile::{Builder, NamedTempFile};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipArchive, ZipWriter};

const FORMAT: &str = "codex-rehome";
const SCHEMA_VERSION: u32 = 1;
const MAX_INSPECTION_BYTES: u64 = 1024 * 1024 * 1024;
const EXCLUSION_RULES: &[&str] = &[
    "credentials and authentication data",
    "environment and private key files",
    "version-control metadata",
    "dependency, cache, build, and runtime data",
];

pub fn create_package(request: CreatePackageRequest) -> Result<CreatePackageReport, RehomeError> {
    validate_output_path(&request.output_path)?;
    let inventory = discover_codex(Some(request.codex_home.clone()))?;
    let output_parent = usable_parent(&request.output_path);
    fs::create_dir_all(output_parent).map_err(|error| {
        package_invalid(format!(
            "could not create package output directory: {error}"
        ))
    })?;

    let staging = Builder::new()
        .prefix(".rehome-stage-")
        .tempdir_in(output_parent)
        .map_err(|error| package_invalid(format!("could not create private staging: {error}")))?;
    make_staging_private(staging.path())?;

    let mut payloads = BTreeMap::new();
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
        &inventory.conversation_paths,
        &request.codex_home,
        &request.conversation_ids,
        &index_metadata,
        staging.path(),
        &mut payloads,
        &mut counts,
    )?;

    if !index_metadata.bytes.is_empty() {
        stage_generated(
            staging.path(),
            &mut payloads,
            "codex/session_index.jsonl",
            &index_metadata.bytes,
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

    if request.include_skills {
        counts.skills = stage_discovered_trees(
            &inventory.skill_paths,
            &request.codex_home.join("skills"),
            "codex/skills",
            staging.path(),
            &mut payloads,
        )?;
    }
    if request.include_plugins {
        counts.plugins = stage_discovered_trees(
            &inventory.plugin_paths,
            &request.codex_home.join("plugins").join("cache"),
            "codex/plugins/cache",
            staging.path(),
            &mut payloads,
        )?;
    }
    if request.include_generated_images {
        counts.generated_images = stage_discovered_files(
            &inventory.generated_image_paths,
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
    write_staged_bytes(staging.path(), "checksums.sha256", checksums.as_bytes())?;
    // The manifest is deliberately materialized only after every payload and checksum.
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| package_invalid(format!("could not serialize manifest: {error}")))?;
    write_staged_bytes(staging.path(), "manifest.json", &manifest_bytes)?;

    write_archive_atomically(staging.path(), &request.output_path, &payloads)?;
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

pub fn inspect_package(path: &Path) -> Result<PackagePreview, RehomeError> {
    let file = fs::File::open(path)
        .map_err(|error| package_invalid(format!("could not open package: {error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| package_invalid(format!("invalid ZIP container: {error}")))?;
    let mut names = Vec::with_capacity(archive.len());
    let mut seen = HashSet::new();
    let mut files = BTreeMap::new();
    let mut forbidden_files_total = 0_u64;
    let mut total_bytes = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| package_invalid(format!("could not read ZIP entry: {error}")))?;
        let raw_name = entry.name().to_owned();
        let normalized = validate_zip_entry_name(&raw_name, entry.is_dir())?;
        let duplicate_key = normalized.to_ascii_lowercase();
        if !seen.insert(duplicate_key) {
            return Err(package_invalid("duplicate ZIP entry name"));
        }

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
        total_bytes = total_bytes
            .checked_add(entry.size())
            .ok_or_else(|| package_invalid("ZIP uncompressed size exceeds the inspection limit"))?;
        if total_bytes > MAX_INSPECTION_BYTES {
            return Err(package_invalid(
                "ZIP uncompressed size exceeds the inspection limit",
            ));
        }
        let mut bytes = Vec::with_capacity(entry.size().min(usize::MAX as u64) as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| package_invalid(format!("could not read ZIP payload: {error}")))?;
        files.insert(normalized, bytes);
    }

    let manifest_bytes = files
        .get("manifest.json")
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

    let checksum_bytes = files
        .get("checksums.sha256")
        .ok_or_else(|| package_invalid("checksums.sha256 is missing"))?;
    verify_checksums(checksum_bytes, &files)?;

    names.sort();
    Ok(PackagePreview {
        package_path: path.to_path_buf(),
        manifest,
        checksum_valid: true,
        entries: names,
        forbidden_files_total,
    })
}

#[derive(Clone)]
struct Payload {
    hash: String,
    executable: bool,
}

#[derive(Default)]
struct SessionIndexMetadata {
    bytes: Vec<u8>,
    by_id: HashMap<Uuid, Value>,
}

fn stage_projects(
    project_paths: &[PathBuf],
    staging_root: &Path,
    payloads: &mut BTreeMap<String, Payload>,
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
                return Err(package_invalid(
                    "symbolic links are not allowed in selected projects",
                ));
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
    paths: &[PathBuf],
    codex_home: &Path,
    selected_ids: &[Uuid],
    index: &SessionIndexMetadata,
    staging_root: &Path,
    payloads: &mut BTreeMap<String, Payload>,
    counts: &mut ContentCounts,
) -> Result<Vec<ConversationEntry>, RehomeError> {
    let selected: HashSet<Uuid> = selected_ids.iter().copied().collect();
    if selected.len() != selected_ids.len() {
        return Err(package_invalid("selected conversation ID is duplicated"));
    }
    let mut found = HashSet::new();
    let mut conversations = Vec::new();

    for source in paths {
        let bytes = read_stable_source(source)?;
        let Some((task_id, session_value)) = session_identity(&bytes) else {
            continue;
        };
        if !selected.contains(&task_id) {
            continue;
        }
        if !found.insert(task_id) {
            return Err(package_invalid(
                "selected conversation has multiple session files",
            ));
        }
        let relative = source
            .strip_prefix(codex_home)
            .map_err(|_| package_invalid("conversation path escapes the selected Codex home"))?;
        let archive_path = format!("codex/{}", normalize_entry(relative)?);
        stage_source(source, staging_root, payloads, &archive_path)?;
        let metadata = index.by_id.get(&task_id).unwrap_or(&session_value);
        conversations.push(ConversationEntry {
            task_id,
            project_id: json_uuid(metadata, &["project_id"])
                .or_else(|| json_uuid(&session_value, &["project_id"])),
            title: json_string(metadata, &["title"])
                .unwrap_or_else(|| "Imported conversation".to_owned()),
            updated_at: json_string(metadata, &["updated_at", "timestamp"])
                .or_else(|| json_string(&session_value, &["updated_at", "timestamp"]))
                .unwrap_or_default(),
            content_hash: sha256_hex(&bytes),
            archive_path,
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
    let bytes = read_stable_source(path)?;
    let selected: HashSet<Uuid> = selected_ids.iter().copied().collect();
    let mut result = SessionIndexMetadata::default();
    for line in String::from_utf8(bytes)
        .map_err(|_| package_invalid("session index is not UTF-8"))?
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| package_invalid("session index contains malformed JSONL"))?;
        let Some(id) = json_uuid(&value, &["id", "thread_id"]) else {
            continue;
        };
        if selected.contains(&id) {
            let encoded = serde_json::to_vec(&value).map_err(|error| {
                package_invalid(format!("could not encode session index: {error}"))
            })?;
            result.bytes.extend_from_slice(&encoded);
            result.bytes.push(b'\n');
            result.by_id.insert(id, value);
        }
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
    let mut statement = connection
        .prepare("SELECT * FROM threads ORDER BY rowid")
        .map_err(|error| package_invalid(format!("could not read Codex threads: {error}")))?;
    let columns: Vec<String> = statement
        .column_names()
        .iter()
        .map(|column| (*column).to_owned())
        .collect();
    let id_column = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("id"))
        .ok_or_else(|| package_invalid("Codex threads table has no id column"))?;
    let selected: HashSet<String> = selected_ids.iter().map(Uuid::to_string).collect();
    let mut rows = statement
        .query([])
        .map_err(|error| package_invalid(format!("could not query Codex threads: {error}")))?;
    let mut exported = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| package_invalid(format!("could not read Codex thread row: {error}")))?
    {
        let id = row
            .get_ref(id_column)
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
            object.insert(column.clone(), sqlite_json_value(value));
        }
        exported.push(Value::Object(object));
    }
    let count = exported.len() as u64;
    let mut bytes = serde_json::to_vec_pretty(&exported)
        .map_err(|error| package_invalid(format!("could not encode Codex threads: {error}")))?;
    bytes.push(b'\n');
    Ok((bytes, count))
}

fn sqlite_json_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::from(value),
        ValueRef::Real(value) => Value::from(value),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::String(format!("hex:{}", hex_bytes(value))),
    }
}

fn stage_discovered_files(
    sources: &[PathBuf],
    source_root: &Path,
    archive_root: &str,
    staging_root: &Path,
    payloads: &mut BTreeMap<String, Payload>,
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
    payloads: &mut BTreeMap<String, Payload>,
) -> Result<u64, RehomeError> {
    let mut roots = BTreeMap::new();
    for marker in marker_files {
        let bundle_root = marker
            .parent()
            .ok_or_else(|| package_invalid("discovered bundle marker has no parent"))?;
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
    payloads: &mut BTreeMap<String, Payload>,
    archive_path: &str,
) -> Result<(), RehomeError> {
    let archive_path = normalize_entry(Path::new(archive_path))?;
    if payloads.contains_key(&archive_path) {
        return Err(package_invalid(
            "multiple sources map to the same package entry",
        ));
    }
    let before = source_fingerprint(source)?;
    let destination = staging_root.join(Path::new(&archive_path));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_package_error)?;
    }
    let mut reader = fs::File::open(source).map_err(io_package_error)?;
    let mut writer = fs::File::create(&destination).map_err(io_package_error)?;
    io::copy(&mut reader, &mut writer).map_err(io_package_error)?;
    writer.sync_all().map_err(io_package_error)?;
    drop(writer);
    if before != source_fingerprint(source)? {
        return Err(package_invalid(
            "source file changed in size or modification time while being copied",
        ));
    }
    let bytes = fs::read(&destination).map_err(io_package_error)?;
    payloads.insert(
        archive_path,
        Payload {
            hash: sha256_hex(&bytes),
            executable: source_is_executable(source),
        },
    );
    Ok(())
}

fn stage_generated(
    staging_root: &Path,
    payloads: &mut BTreeMap<String, Payload>,
    archive_path: &str,
    bytes: &[u8],
) -> Result<(), RehomeError> {
    let archive_path = normalize_entry(Path::new(archive_path))?;
    if payloads.contains_key(&archive_path) {
        return Err(package_invalid("duplicate generated package entry"));
    }
    write_staged_bytes(staging_root, &archive_path, bytes)?;
    payloads.insert(
        archive_path,
        Payload {
            hash: sha256_hex(bytes),
            executable: false,
        },
    );
    Ok(())
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
    payloads: &BTreeMap<String, Payload>,
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
    temporary.persist_noclobber(output_path).map_err(|error| {
        package_invalid(format!(
            "could not atomically publish package: {}",
            error.error
        ))
    })?;
    Ok(())
}

struct ArchiveEntry {
    name: String,
    is_directory: bool,
    permissions: u32,
}

fn staged_archive_entries(
    staging_root: &Path,
    payloads: &BTreeMap<String, Payload>,
) -> Result<Vec<ArchiveEntry>, RehomeError> {
    let mut entries = Vec::new();
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
            let executable = payloads
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
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn stable_options(permissions: u32) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(permissions)
}

fn render_checksums(payloads: &BTreeMap<String, Payload>) -> String {
    let mut checksums = String::new();
    for (path, payload) in payloads {
        checksums.push_str(&payload.hash);
        checksums.push_str("  ");
        checksums.push_str(path);
        checksums.push('\n');
    }
    checksums
}

fn verify_checksums(
    checksum_bytes: &[u8],
    files: &BTreeMap<String, Vec<u8>>,
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

    let payload_paths: HashSet<&str> = files
        .keys()
        .filter(|path| !matches!(path.as_str(), "manifest.json" | "checksums.sha256"))
        .map(String::as_str)
        .collect();
    let checksum_paths: HashSet<&str> = expected.keys().map(String::as_str).collect();
    if payload_paths != checksum_paths {
        return Err(RehomeError::new(
            ErrorCode::ChecksumMismatch,
            "checksum coverage does not match package payloads",
        ));
    }
    for (path, expected_hash) in expected {
        let bytes = files.get(&path).ok_or_else(|| {
            RehomeError::new(
                ErrorCode::ChecksumMismatch,
                "checksummed payload is missing",
            )
        })?;
        if sha256_hex(bytes) != expected_hash {
            return Err(RehomeError::new(
                ErrorCode::ChecksumMismatch,
                format!("checksum mismatch for {path}"),
            ));
        }
    }
    Ok(())
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

fn validate_output_path(path: &Path) -> Result<(), RehomeError> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(package_invalid("package output path is invalid"));
    }
    if path.exists() {
        return Err(package_invalid("package output path already exists"));
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

fn read_stable_source(path: &Path) -> Result<Vec<u8>, RehomeError> {
    let before = source_fingerprint(path)?;
    let bytes = fs::read(path).map_err(io_package_error)?;
    if before != source_fingerprint(path)? {
        return Err(package_invalid(
            "source file changed in size or modification time while being read",
        ));
    }
    Ok(bytes)
}

fn session_identity(bytes: &[u8]) -> Option<(Uuid, Value)> {
    std::str::from_utf8(bytes).ok()?.lines().find_map(|line| {
        let value: Value = serde_json::from_str(line).ok()?;
        let id = json_uuid(&value, &["thread_id", "id", "conversation_id"])?;
        Some((id, value))
    })
}

fn json_uuid(value: &Value, keys: &[&str]) -> Option<Uuid> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
    })
}

fn json_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_owned))
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
