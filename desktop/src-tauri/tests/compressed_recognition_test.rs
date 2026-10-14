#[allow(dead_code)]
mod common;
use common::{synthetic_codex_fixture, THREAD_ID};
use rehome_desktop_lib::core::{
    discovery::{discover_codex_with_context, DiscoveryContext},
    models::CreatePackageRequest,
    package::{create_package, create_package_replacing, inspect_package},
};
use rusqlite::{params, Connection};
use std::{error::Error, fs};
use uuid::Uuid;

#[test]
fn unreadable_compressed_rollout_stays_visible_and_preserves_source() -> Result<(), Box<dyn Error>>
{
    let fixture = synthetic_codex_fixture()?;
    let compressed = fixture.session_path.with_extension("jsonl.zst");
    // An invalid frame remains visible and blocked; its source is never changed.
    fs::write(&compressed, b"\x28\xb5\x2f\xfdrecognition-only")?;
    fs::remove_file(&fixture.session_path)?;
    let before = fs::read(&compressed)?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(
        inventory.unsupported_conversation_paths,
        vec![compressed.clone()]
    );
    assert_eq!(inventory.conversations.len(), 1);
    assert_eq!(
        inventory.conversations[0].task_id,
        Uuid::parse_str(THREAD_ID)?
    );
    assert_eq!(
        inventory.conversations[0].title,
        "Synthetic migration thread"
    );
    assert_eq!(
        inventory.conversations[0].project_id,
        Some(inventory.projects[0].project_id)
    );
    assert!(inventory.conversations[0]
        .archive_path
        .ends_with(".jsonl.zst"));
    assert!(inventory
        .warnings
        .iter()
        .any(|w| w.contains("compressed") && w.contains("cannot be exported")));
    assert_eq!(fs::read(&compressed)?, before);
    assert!(!fixture.session_path.exists());
    Ok(())
}

#[test]
fn compressed_sibling_does_not_duplicate_plain_conversation() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(
        fixture.session_path.with_extension("jsonl.zst"),
        b"unread sibling",
    )?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.conversations.len(), 1);
    assert!(inventory.conversations[0].archive_path.ends_with(".jsonl"));
    assert!(inventory.unsupported_conversation_paths.is_empty());
    assert!(!inventory
        .warnings
        .iter()
        .any(|w| w.contains("cannot be exported")));
    Ok(())
}

#[test]
fn compressed_active_rollout_wins_over_older_plain_history() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let older = fixture
        .session_path
        .parent()
        .unwrap()
        .join(format!("rollout-2026-07-21T00-00-00-{THREAD_ID}.jsonl"));
    fs::copy(&fixture.session_path, &older)?;
    fs::write(
        fixture.session_path.with_extension("jsonl.zst"),
        b"compressed active",
    )?;
    fs::remove_file(&fixture.session_path)?;
    Connection::open(&fixture.state_db_path)?.execute(
        "UPDATE threads SET rollout_path = ?1 WHERE id = ?2",
        params![fixture.session_path.to_string_lossy().as_ref(), THREAD_ID],
    )?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.conversations.len(), 1);
    assert!(inventory.conversations[0]
        .archive_path
        .ends_with(".jsonl.zst"));
    Ok(())
}

#[test]
fn compressed_archived_rollout_without_index_still_has_identity() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let archived = fixture.codex_home.join("archived_sessions");
    fs::create_dir_all(&archived)?;
    fs::write(
        archived.join(format!("rollout-2026-07-22T00-00-00-{THREAD_ID}.jsonl.zst")),
        b"unread frame",
    )?;
    fs::remove_file(&fixture.session_path)?;
    fs::remove_file(&fixture.session_index_path)?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.conversations.len(), 1);
    assert_eq!(
        inventory.conversations[0].task_id,
        Uuid::parse_str(THREAD_ID)?
    );
    assert!(inventory.conversations[0]
        .archive_path
        .starts_with("codex/archived_sessions/"));
    Ok(())
}

#[test]
fn compressed_active_rollout_resolves_a_logical_path_alias() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let parent = fixture.session_path.parent().unwrap();
    fs::copy(
        &fixture.session_path,
        parent.join(format!("rollout-2026-07-21T00-00-00-{THREAD_ID}.jsonl")),
    )?;
    fs::write(
        fixture.session_path.with_extension("jsonl.zst"),
        b"unread compressed active",
    )?;
    fs::remove_file(&fixture.session_path)?;
    // Do not use PathBuf::join here: on Windows a verbatim parent can
    // normalize away `..`, leaving this alias test with a direct path.
    let separator = std::path::MAIN_SEPARATOR;
    let parent_text = parent.to_string_lossy();
    let parent_text = parent_text.strip_prefix(r"\\?\").unwrap_or(&parent_text);
    let alias = std::path::PathBuf::from(format!(
        "{parent_text}{separator}..{separator}22{separator}{}",
        fixture.session_path.file_name().unwrap().to_string_lossy()
    ));
    assert!(alias
        .components()
        .any(|component| component == std::path::Component::ParentDir));
    Connection::open(&fixture.state_db_path)?.execute(
        "UPDATE threads SET rollout_path = ?1 WHERE id = ?2",
        params![alias.to_string_lossy().as_ref(), THREAD_ID],
    )?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.conversations.len(), 1);
    assert!(inventory.conversations[0]
        .archive_path
        .ends_with(".jsonl.zst"));
    Ok(())
}

#[test]
fn compressed_invalid_filename_is_warned_not_silently_ignored() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(
        fixture
            .session_path
            .parent()
            .unwrap()
            .join("未知.jsonl.zst"),
        b"unread frame",
    )?;
    let inventory = discover_codex_with_context(
        Some(fixture.codex_home.clone()),
        &DiscoveryContext::default(),
    )?;
    assert_eq!(inventory.conversations.len(), 1);
    assert!(inventory
        .warnings
        .iter()
        .any(|w| w.contains("未知.jsonl.zst")));
    Ok(())
}

#[test]
fn compressed_selection_fails_explicitly_without_replacing_output() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    let compressed = fixture.session_path.with_extension("jsonl.zst");
    fs::write(&compressed, b"recognition fixture")?;
    fs::remove_file(&fixture.session_path)?;
    let output_root = tempfile::tempdir()?;
    let output = output_root.path().join("keep.rehome");
    fs::write(&output, b"previous package")?;
    let error = create_package_replacing(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output.clone(),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })
    .unwrap_err();
    assert!(
        error.message.contains("compressed Codex conversation"),
        "{}",
        error.message
    );
    assert_eq!(fs::read(&output)?, b"previous package");
    assert_eq!(fs::read(compressed)?, b"recognition fixture");
    Ok(())
}

#[test]
fn compressed_sibling_does_not_break_plain_export() -> Result<(), Box<dyn Error>> {
    let fixture = synthetic_codex_fixture()?;
    fs::write(
        fixture.session_path.with_extension("jsonl.zst"),
        b"unread sibling",
    )?;
    let output_root = tempfile::tempdir()?;
    let report = create_package(CreatePackageRequest {
        codex_home: fixture.codex_home.clone(),
        project_paths: vec![],
        conversation_ids: vec![Uuid::parse_str(THREAD_ID)?],
        output_path: output_root.path().join("plain.rehome"),
        source_device_id: Uuid::nil(),
        skill_paths: vec![],
        plugin_paths: vec![],
        generated_image_paths: vec![],
    })?;
    assert_eq!(report.counts.conversations, 1);
    assert!(inspect_package(&report.package_path)?
        .manifest
        .conversations[0]
        .archive_path
        .ends_with(".jsonl"));
    Ok(())
}
