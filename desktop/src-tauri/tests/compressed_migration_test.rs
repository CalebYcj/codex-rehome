#[allow(dead_code)]
mod common;

use common::{synthetic_codex_fixture, THREAD_ID};
use rehome_desktop_lib::core::{
    discovery::{discover_codex_with_context, DiscoveryContext},
    models::CreatePackageRequest,
    package::{create_package, create_package_replacing, inspect_package},
};
use std::{error::Error, fs, io::Read};
use uuid::Uuid;

#[test]
fn export_uses_the_active_compressed_alias_not_an_older_plain_rollout() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let parent = fixture.session_path.parent().unwrap();
    fs::copy(
        &fixture.session_path,
        parent.join(format!("rollout-2026-07-21-{THREAD_ID}.jsonl")),
    )?;
    let mut bytes = fs::read(&fixture.session_path)?;
    bytes.extend(b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"ACTIVE-COMPRESSED-MARKER\"}}\n");
    fs::write(
        fixture.session_path.with_extension("jsonl.zst"),
        zstd::stream::encode_all(bytes.as_slice(), 3)?,
    )?;
    fs::remove_file(&fixture.session_path)?;
    let parent_text = parent.to_string_lossy();
    let parent_text = parent_text.strip_prefix(r"\\?\").unwrap_or(&parent_text);
    let sep = std::path::MAIN_SEPARATOR;
    let alias = format!(
        "{parent_text}{sep}..{sep}22{sep}{}",
        fixture.session_path.file_name().unwrap().to_string_lossy()
    );
    rusqlite::Connection::open(&fixture.state_db_path)?.execute(
        "UPDATE threads SET rollout_path=?1 WHERE id=?2",
        rusqlite::params![alias, THREAD_ID],
    )?;
    let output = tempfile::tempdir()?;
    let report = create_package(request(&fixture, output.path().join("active.rehome")))?;
    let preview = inspect_package(&report.package_path)?;
    let mut archive = zip::ZipArchive::new(fs::File::open(report.package_path)?)?;
    let mut decoded = Vec::new();
    archive
        .by_name(&preview.manifest.conversations[0].archive_path)?
        .read_to_end(&mut decoded)?;
    assert_eq!(decoded, bytes);
    Ok(())
}

#[test]
fn invalid_checksum_and_trailing_garbage_do_not_publish_a_package() -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let fixture = synthetic_codex_fixture()?;
    let original = fs::read(&fixture.session_path)?;
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3)?;
    encoder.include_checksum(true)?;
    encoder.write_all(&original)?;
    let valid = encoder.finish()?;
    fs::remove_file(&fixture.session_path)?;
    let mut bad_checksum = valid.clone();
    *bad_checksum.last_mut().unwrap() ^= 1;
    let mut garbage = valid;
    garbage.extend(b"not another frame");
    let output = tempfile::tempdir()?;
    let package = output.path().join("keep.rehome");
    for bytes in [bad_checksum, garbage] {
        fs::write(fixture.session_path.with_extension("jsonl.zst"), &bytes)?;
        fs::write(&package, b"previous package")?;
        assert!(create_package_replacing(request(&fixture, package.clone())).is_err());
        assert_eq!(fs::read(&package)?, b"previous package");
        assert_eq!(
            fs::read(fixture.session_path.with_extension("jsonl.zst"))?,
            bytes
        );
    }
    Ok(())
}

fn request(
    fixture: &common::SyntheticCodexFixture,
    output: std::path::PathBuf,
) -> CreatePackageRequest {
    CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID).unwrap()],
        output_path: output,
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    }
}

#[test]
fn compressed_payload_with_a_different_identity_is_blocked() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let different = fs::read_to_string(&fixture.session_path)?
        .replace(THREAD_ID, "22222222-2222-4222-8222-222222222222");
    let compressed = fixture.session_path.with_extension("jsonl.zst");
    fs::write(
        &compressed,
        zstd::stream::encode_all(different.as_bytes(), 3)?,
    )?;
    fs::remove_file(&fixture.session_path)?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.unsupported_conversation_paths, vec![compressed]);
    let output = tempfile::tempdir()?;
    assert!(create_package(request(&fixture, output.path().join("bad.rehome"))).is_err());
    assert!(!output.path().join("bad.rehome").exists());
    Ok(())
}

#[test]
fn compressed_archive_without_index_keeps_subagent_metadata() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let mut header: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.session_path)?)?;
    header["payload"]["thread_source"] = "subagent".into();
    header["payload"]["agent_path"] = "/root/check".into();
    header["payload"]["history_mode"] = "paginated".into();
    let archived = fixture.codex_home.join("archived_sessions");
    fs::create_dir_all(&archived)?;
    fs::write(
        archived.join(format!("rollout-2026-07-22-{THREAD_ID}.jsonl.zst")),
        zstd::stream::encode_all(format!("{header}\n").as_bytes(), 3)?,
    )?;
    fs::remove_file(&fixture.session_path)?;
    fs::remove_file(&fixture.session_index_path)?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(
        inventory.conversations[0]
            .classification
            .as_ref()
            .unwrap()
            .agent_path
            .as_deref(),
        Some("/root/check")
    );
    let output = tempfile::tempdir()?;
    let report = create_package(request(&fixture, output.path().join("archived.rehome")))?;
    let preview = inspect_package(&report.package_path)?;
    assert!(preview.manifest.conversations[0]
        .archive_path
        .starts_with("codex/archived_sessions/"));
    assert!(preview.manifest.conversations[0].classification.is_some());
    Ok(())
}

#[test]
fn compressed_export_retains_a_large_record_and_concatenated_frames() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let header = fs::read(&fixture.session_path)?;
    let record = format!("{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}}}\n", "x".repeat(2 * 1024 * 1024));
    let mut frame = zstd::stream::encode_all(header.as_slice(), 3)?;
    frame.extend(zstd::stream::encode_all(record.as_bytes(), 3)?);
    fs::write(fixture.session_path.with_extension("jsonl.zst"), &frame)?;
    fs::remove_file(&fixture.session_path)?;
    let output = tempfile::tempdir()?;
    let report = create_package(request(&fixture, output.path().join("large.rehome")))?;
    let preview = inspect_package(&report.package_path)?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&report.package_path)?)?;
    let mut recovered = Vec::new();
    archive
        .by_name(&preview.manifest.conversations[0].archive_path)?
        .read_to_end(&mut recovered)?;
    let mut original = header;
    original.extend(record.as_bytes());
    assert_eq!(recovered, original);
    Ok(())
}

#[test]
fn valid_compressed_rollout_exports_original_jsonl_bytes() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let original = fs::read(&fixture.session_path)?;
    let frame = zstd::stream::encode_all(original.as_slice(), 3)?;
    let compressed = fixture.session_path.with_extension("jsonl.zst");
    fs::write(&compressed, &frame)?;
    fs::remove_file(&fixture.session_path)?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.conversations.len(), 1);
    assert!(inventory.unsupported_conversation_paths.is_empty());
    let output = tempfile::tempdir()?;
    let report = create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![fixture.project_path.clone()],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output.path().join("compressed.rehome"),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    let preview = inspect_package(&report.package_path)?;
    let archive_path = &preview.manifest.conversations[0].archive_path;
    assert!(archive_path.ends_with(".jsonl"));
    assert!(!archive_path.ends_with(".zst"));
    let mut archive = zip::ZipArchive::new(fs::File::open(&report.package_path)?)?;
    let mut recovered = Vec::new();
    archive.by_name(archive_path)?.read_to_end(&mut recovered)?;
    assert_eq!(recovered, original);
    assert_eq!(fs::read(&compressed)?, frame);
    assert!(!fixture.session_path.exists());
    Ok(())
}

#[test]
fn truncated_compressed_frame_never_replaces_existing_package() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let mut original = fs::read(&fixture.session_path)?;
    // Enough data for multiple streaming blocks: the metadata can still be
    // discovered even though the decoder fails at the end of the frame.
    for i in 0..20000 {
        original.extend_from_slice(format!("{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"marker-{i}\"}}}}\n").as_bytes());
    }
    let mut frame = zstd::stream::encode_all(original.as_slice(), 3)?;
    frame.truncate(frame.len() - 7);
    fs::write(fixture.session_path.with_extension("jsonl.zst"), &frame)?;
    fs::remove_file(&fixture.session_path)?;
    let output = tempfile::tempdir()?;
    let output_path = output.path().join("keep.rehome");
    fs::write(&output_path, b"previous package")?;
    let error = create_package_replacing(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output_path.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })
    .unwrap_err();
    assert!(error.message.contains("compressed"), "{}", error.message);
    assert_eq!(fs::read(&output_path)?, b"previous package");
    Ok(())
}
