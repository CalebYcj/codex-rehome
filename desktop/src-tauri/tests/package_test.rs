mod common;

use common::{synthetic_codex_fixture, THREAD_ID};
use rehome_desktop_lib::core::{
    error::ErrorCode,
    models::{
        ContentCounts, CreatePackageRequest, ExclusionSummary, PackageManifest, PackageMode,
        SourceOs,
    },
    package::{create_package, inspect_package},
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime},
};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipArchive, ZipWriter};

#[test]
fn packages_selected_fixture_content_without_mutating_sources() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(fixture.codex_home.join("auth.json"), b"fixture-token\n")?;
    fs::write(fixture.codex_home.join("config.toml"), b"model = 'local'\n")?;
    let skill_reference = fixture
        .skill_path
        .parent()
        .unwrap()
        .join("references")
        .join("guide.md");
    fs::create_dir_all(skill_reference.parent().unwrap())?;
    fs::write(&skill_reference, b"# Synthetic guide\n")?;
    let plugin_tool = fixture
        .plugin_manifest_path
        .parent()
        .unwrap()
        .join("bin")
        .join("tool.js");
    fs::create_dir_all(plugin_tool.parent().unwrap())?;
    fs::write(&plugin_tool, b"export const synthetic = true;\n")?;
    fs::write(
        fixture.plugin_manifest_path.parent().unwrap().join(".env"),
        b"PLUGIN_SECRET=excluded\n",
    )?;
    assert!([
        &fixture.session_path,
        &fixture.session_index_path,
        &fixture.state_db_path,
        &fixture.skill_path,
        &fixture.plugin_manifest_path,
        &fixture.generated_image_path,
        &fixture.readme_path,
        &fixture.env_path,
        &fixture.git_config_path,
        &fixture.node_modules_file_path,
    ]
    .iter()
    .all(|path| path.exists()));
    let source_before = snapshot_files(&fixture.root)?;
    let output_directory = tempfile::tempdir()?;
    let output = output_directory.path().join("handoff.rehome");

    let report = create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output.clone(),
        source_device_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333")?,
        include_skills: true,
        include_plugins: true,
        include_generated_images: true,
    })?;

    assert_eq!(report.package_path, output);
    assert_eq!(report.counts.projects, 1);
    assert_eq!(report.counts.project_files, 1);
    assert_eq!(report.counts.conversations, 1);
    assert_eq!(report.counts.skills, 1);
    assert_eq!(report.counts.plugins, 1);
    assert_eq!(report.counts.generated_images, 1);
    assert_eq!(report.counts.sqlite_threads, 1);
    assert_eq!(
        report.bytes_written,
        fs::metadata(&report.package_path)?.len()
    );

    let preview = inspect_package(&report.package_path)?;
    assert_eq!(preview.manifest.format, "codex-rehome");
    assert_eq!(preview.manifest.schema_version, 1);
    assert_eq!(preview.manifest.source_os, SourceOs::Windows);
    assert_eq!(
        preview.manifest.source_device_id,
        Uuid::parse_str("33333333-3333-4333-8333-333333333333")?
    );
    assert_eq!(preview.manifest.counts, report.counts);
    assert_eq!(preview.forbidden_files_total, 0);
    assert!(preview.checksum_valid);
    assert!(preview.entries.iter().all(|entry| !entry.contains('\\')));
    assert!(!preview.entries.iter().any(|entry| entry.contains("/.git/")));
    assert!(!preview.entries.iter().any(|entry| entry.ends_with("/.env")));
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("auth.json")));
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("config.toml")));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/session_index.jsonl"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/metadata/threads.json"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry.ends_with("/files/README.md")));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/skills/synthetic-skill/SKILL.md"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/skills/synthetic-skill/references/guide.md"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/plugins/cache/synthetic-plugin/manifest.json"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/plugins/cache/synthetic-plugin/bin/tool.js"));
    assert!(!preview
        .entries
        .iter()
        .any(|entry| entry == "codex/plugins/cache/synthetic-plugin/.env"));
    assert!(preview
        .entries
        .iter()
        .any(|entry| entry == "codex/generated_images/synthetic-image.png"));
    assert!(preview
        .entries
        .iter()
        .filter(|entry| entry.ends_with(".jsonl"))
        .any(|entry| entry.contains("codex/sessions/")));
    assert_eq!(preview.entries, sorted(preview.entries.clone()));
    assert_eq!(snapshot_files(&fixture.root)?, source_before);
    Ok(())
}

#[test]
fn package_exports_threads_from_a_private_wal_snapshot() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let generator_directory = tempfile::tempdir()?;
    let generator = generator_directory.path().join("generator.sqlite");
    let writer = Connection::open(&generator)?;
    writer.pragma_update(None, "journal_mode", "WAL")?;
    writer.pragma_update(None, "wal_autocheckpoint", 0)?;
    writer.execute(
        "CREATE TABLE threads (\
            id TEXT PRIMARY KEY, cwd TEXT NOT NULL, rollout_path TEXT NOT NULL, \
            title TEXT NOT NULL, updated_at TEXT NOT NULL, archived INTEGER NOT NULL, \
            has_user_event INTEGER NOT NULL, preview TEXT NOT NULL\
        )",
        [],
    )?;
    writer.execute(
        "INSERT INTO threads VALUES (?1, ?2, ?3, ?4, ?5, 0, 1, ?6)",
        params![
            THREAD_ID,
            r"C:\Users\OldUser\Documents\visual",
            r"C:\Users\OldUser\.codex\sessions\thread.jsonl",
            "WAL package thread",
            "2026-07-22T00:00:00Z",
            "WAL package preview",
        ],
    )?;

    let state_database = fixture.codex_home.join("state_9.sqlite");
    fs::copy(&generator, &state_database)?;
    fs::copy(
        sqlite_sidecar(&generator, "-wal"),
        sqlite_sidecar(&state_database, "-wal"),
    )?;
    assert!(!sqlite_sidecar(&state_database, "-shm").exists());
    let source_before = snapshot_files(&fixture.root)?;
    let output_directory = tempfile::tempdir()?;
    let output = output_directory.path().join("wal-handoff.rehome");

    let report = create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: Vec::new(),
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output,
        source_device_id: Uuid::new_v4(),
        include_skills: false,
        include_plugins: false,
        include_generated_images: false,
    })?;

    assert_eq!(report.counts.sqlite_threads, 1);
    assert_eq!(snapshot_files(&fixture.root)?, source_before);
    assert!(!sqlite_sidecar(&state_database, "-shm").exists());
    drop(writer);
    Ok(())
}

#[test]
fn rejects_corrupt_zip_bytes_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("corrupt.rehome");
    fs::write(&package, b"this is not a zip archive")?;

    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_corrupted_payload_bytes_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("payload-corrupt.rehome");
    let manifest = serde_json::to_vec(&test_manifest(1))?;
    let payload = b"ORIGINAL-STORED-PAYLOAD";
    let checksums = format!("{}  codex/sessions/thread.jsonl\n", checksum(payload));
    write_test_zip(
        &package,
        &[
            ("checksums.sha256", checksums.as_bytes()),
            ("codex/sessions/thread.jsonl", payload),
            ("manifest.json", &manifest),
        ],
    )?;
    assert_eq!(
        replace_all(&package, payload, b"CORRUPT!-STORED-PAYLOAD")?,
        1
    );

    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_unsafe_and_duplicate_zip_entry_names() -> Result<(), Box<dyn Error>> {
    let cases: &[&[(&str, &[u8])]] = &[
        &[("../escape", b"payload")],
        &[("/absolute", b"payload")],
        &[("C:/absolute", b"payload")],
        &[("folder\\file", b"payload")],
    ];

    for (index, entries) in cases.iter().enumerate() {
        let directory = tempfile::tempdir()?;
        let package = directory.path().join(format!("unsafe-{index}.rehome"));
        write_test_zip(&package, entries)?;
        assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    }

    let directory = tempfile::tempdir()?;
    let package = directory.path().join("duplicate.rehome");
    write_test_zip(
        &package,
        &[("duplicate-a", b"one"), ("duplicate-b", b"two")],
    )?;
    assert!(replace_all(&package, b"duplicate-b", b"duplicate-a")? >= 2);
    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_missing_manifest_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("missing-manifest.rehome");
    write_test_zip(&package, &[("checksums.sha256", b"")])?;

    assert_error_code(inspect_package(&package), ErrorCode::PackageInvalid);
    Ok(())
}

#[test]
fn rejects_unsupported_schema_before_checksum_validation() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("future.rehome");
    let manifest = serde_json::to_vec(&test_manifest(99))?;
    write_test_zip(&package, &[("manifest.json", &manifest)])?;

    assert_error_code(inspect_package(&package), ErrorCode::UnsupportedSchema);
    Ok(())
}

#[test]
fn rejects_checksum_mismatch_with_a_stable_error_code() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("mismatch.rehome");
    let manifest = serde_json::to_vec(&test_manifest(1))?;
    let checksums = format!("{}  codex/sessions/thread.jsonl\n", "0".repeat(64));
    write_test_zip(
        &package,
        &[
            ("checksums.sha256", checksums.as_bytes()),
            ("codex/sessions/thread.jsonl", b"selected payload"),
            ("manifest.json", &manifest),
        ],
    )?;

    assert_error_code(inspect_package(&package), ErrorCode::ChecksumMismatch);
    Ok(())
}

#[test]
fn writer_uses_portable_deterministic_zip_metadata_and_checksum_text() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("portable.rehome");
    create_package(package_request(&fixture, package.clone()))?;

    let file = fs::File::open(&package)?;
    let mut archive = ZipArchive::new(file)?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        names.push(entry.name().to_owned());
        assert_eq!(entry.last_modified(), Some(DateTime::default()));
        let mode = entry.unix_mode().expect("portable Unix mode") & 0o777;
        assert_eq!(mode, if entry.is_dir() { 0o755 } else { 0o644 });
    }
    assert_eq!(names, sorted(names.clone()));

    let mut checksums = String::new();
    archive
        .by_name("checksums.sha256")?
        .read_to_string(&mut checksums)?;
    assert!(!checksums.starts_with('\u{feff}'));
    assert!(!checksums.contains('\r'));
    assert!(checksums.ends_with('\n'));
    let checksum_paths: Vec<&str> = checksums
        .lines()
        .map(|line| line.split_once("  ").expect("checksum line").1)
        .collect();
    assert_eq!(checksum_paths, sorted_strs(checksum_paths.clone()));

    let payload_paths: Vec<&str> = names
        .iter()
        .filter(|name| !name.ends_with('/'))
        .map(String::as_str)
        .filter(|name| !matches!(*name, "checksums.sha256" | "manifest.json"))
        .collect();
    assert_eq!(checksum_paths, payload_paths);
    Ok(())
}

#[test]
fn create_package_never_clobbers_an_existing_output() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("existing.rehome");
    fs::write(&package, b"keep me")?;

    assert_error_code(
        create_package(package_request(&fixture, package.clone())),
        ErrorCode::PackageInvalid,
    );
    assert_eq!(fs::read(package)?, b"keep me");
    Ok(())
}

#[test]
fn aborts_if_a_source_changes_while_it_is_copied() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let large_source = fixture.project_path.join("large.bin");
    let large_file = fs::File::create(&large_source)?;
    large_file.set_len(64 * 1024 * 1024)?;
    drop(large_file);

    let directory = tempfile::tempdir()?;
    let package = directory.path().join("racing.rehome");
    let staging_parent = directory.path().to_path_buf();
    let source_for_thread = large_source.clone();
    let mutator = thread::spawn(move || -> Result<(), String> {
        let deadline = SystemTime::now() + Duration::from_secs(10);
        loop {
            let copy_started = WalkDir::new(&staging_parent)
                .into_iter()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name() == "large.bin");
            if copy_started {
                OpenOptions::new()
                    .append(true)
                    .open(&source_for_thread)
                    .and_then(|mut file| file.write_all(b"changed"))
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            if SystemTime::now() >= deadline {
                return Err("timed out waiting for staged copy".to_owned());
            }
            thread::sleep(Duration::from_millis(1));
        }
    });

    assert_error_code(
        create_package(package_request(&fixture, package.clone())),
        ErrorCode::PackageInvalid,
    );
    mutator
        .join()
        .map_err(|_| "source mutator panicked")?
        .map_err(|error| format!("source mutator failed: {error}"))?;
    assert!(!package.exists());
    assert!(fs::read_dir(directory.path())?.next().is_none());
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    bytes: Vec<u8>,
    length: u64,
    modified: Option<SystemTime>,
    readonly: bool,
}

fn snapshot_files(root: &Path) -> Result<BTreeMap<PathBuf, FileSnapshot>, Box<dyn Error>> {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            let relative = entry.path().strip_prefix(root)?.to_path_buf();
            let metadata = entry.metadata()?;
            Ok((
                relative,
                FileSnapshot {
                    bytes: fs::read(entry.path())?,
                    length: metadata.len(),
                    modified: metadata.modified().ok(),
                    readonly: metadata.permissions().readonly(),
                },
            ))
        })
        .collect()
}

fn sorted(mut entries: Vec<String>) -> Vec<String> {
    entries.sort();
    entries
}

fn sorted_strs(mut entries: Vec<&str>) -> Vec<&str> {
    entries.sort();
    entries
}

fn package_request(
    fixture: &common::SyntheticCodexFixture,
    output_path: PathBuf,
) -> CreatePackageRequest {
    CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID).unwrap()],
        output_path,
        source_device_id: Uuid::nil(),
        include_skills: true,
        include_plugins: true,
        include_generated_images: true,
    }
}

fn test_manifest(schema_version: u32) -> PackageManifest {
    PackageManifest {
        format: "codex-rehome".into(),
        schema_version,
        package_id: Uuid::nil(),
        created_at: "2026-07-22T00:00:00Z".into(),
        source_os: SourceOs::Windows,
        source_arch: "x86_64".into(),
        source_device_id: Uuid::nil(),
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: ContentCounts::default(),
        projects: vec![],
        conversations: vec![],
        exclusions: ExclusionSummary::default(),
    }
}

fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) -> Result<(), Box<dyn Error>> {
    let file = fs::File::create(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    writer.finish()?;
    Ok(())
}

fn replace_all(path: &Path, from: &[u8], to: &[u8]) -> Result<usize, Box<dyn Error>> {
    assert_eq!(from.len(), to.len());
    let mut bytes = fs::read(path)?;
    let mut replacements = 0;
    for offset in 0..=bytes.len() - from.len() {
        if &bytes[offset..offset + from.len()] == from {
            bytes[offset..offset + to.len()].copy_from_slice(to);
            replacements += 1;
        }
    }
    fs::write(path, bytes)?;
    Ok(replacements)
}

fn assert_error_code<T: std::fmt::Debug>(
    result: Result<T, rehome_desktop_lib::core::error::RehomeError>,
    code: ErrorCode,
) {
    assert_eq!(result.expect_err("operation must fail").code, code);
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sqlite_sidecar(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}
