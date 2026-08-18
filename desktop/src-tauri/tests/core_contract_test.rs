use rehome_desktop_lib::core::{
    error::{ErrorCode, RehomeError},
    models::{
        ChangeKind, CodexInventory, ContentCounts, ConversationEntry, CreatePackageReport,
        CreatePackageRequest, ExclusionSummary, FileConflictResolution, PackageManifest,
        PackageMode, PackagePreview, PendingRecovery, PlannedOperation, PlannedSession,
        ProjectEntry, RecoveryStatus, ReferenceRewrite, ReferenceRewriteKind, RestoreOptions,
        RestorePlan, RestoreReport, RollbackReport, SessionAction, SourceOs, TargetInventory,
        VerificationReport,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt::Debug, path::PathBuf};
use uuid::Uuid;

#[test]
fn manifest_round_trip() {
    let manifest = PackageManifest {
        format: "codex-rehome".into(),
        schema_version: 1,
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
    };

    assert_eq!(
        serde_json::from_str::<PackageManifest>(&serde_json::to_string(&manifest).unwrap())
            .unwrap(),
        manifest
    );
}

#[test]
fn populated_manifest_preserves_source_syntax_and_portable_archive_paths() {
    let project_id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let task_id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let manifest = PackageManifest {
        format: "codex-rehome".into(),
        schema_version: 1,
        package_id: Uuid::nil(),
        created_at: "2026-07-22T00:00:00Z".into(),
        source_os: SourceOs::Windows,
        source_arch: "x86_64".into(),
        source_device_id: Uuid::nil(),
        mode: PackageMode::Full,
        parent_checkpoint: None,
        counts: ContentCounts {
            projects: 1,
            project_files: 1,
            conversations: 1,
            ..ContentCounts::default()
        },
        projects: vec![ProjectEntry {
            project_id,
            name: "visual".into(),
            source_path: r"C:\Users\OldUser\Documents\visual".into(),
            source_available: true,
            archive_path: "projects/22222222-2222-4222-8222-222222222222/files".into(),
            file_count: 1,
            content_bytes: 18,
            git_remote: None,
            git_branch: None,
            git_head: None,
        }],
        conversations: vec![ConversationEntry {
            task_id,
            project_id: Some(project_id),
            title: "Synthetic migration thread".into(),
            updated_at: "2026-07-22T00:00:00Z".into(),
            content_hash: "fixed-content-hash".into(),
            archive_path: "codex/sessions/2026/07/22/thread.jsonl".into(),
            classification: None,
        }],
        exclusions: ExclusionSummary::default(),
    };

    let source_path: &String = &manifest.projects[0].source_path;
    assert_eq!(source_path, r"C:\Users\OldUser\Documents\visual");

    let json = serde_json::to_value(manifest).unwrap();
    assert_eq!(json["source_os"], "windows");
    assert_eq!(json["mode"], "full");
    assert_eq!(
        json["projects"][0]["source_path"],
        r"C:\Users\OldUser\Documents\visual"
    );
    assert_eq!(
        json["projects"][0]["archive_path"],
        "projects/22222222-2222-4222-8222-222222222222/files"
    );
    assert_eq!(
        json["conversations"][0]["archive_path"],
        "codex/sessions/2026/07/22/thread.jsonl"
    );
}

#[test]
fn older_project_entries_default_to_an_available_source() {
    let project = serde_json::from_value::<ProjectEntry>(serde_json::json!({
        "project_id": "22222222-2222-4222-8222-222222222222",
        "name": "visual",
        "source_path": "C:\\Users\\OldUser\\Documents\\visual",
        "archive_path": "projects/22222222-2222-4222-8222-222222222222/files",
        "file_count": 1,
        "content_bytes": 18,
        "git_remote": null,
        "git_branch": null,
        "git_head": null
    }))
    .unwrap();

    assert!(project.source_available);
}

#[test]
fn public_models_support_the_core_contract_traits() {
    fn assert_contract<T>()
    where
        T: Debug + Clone + Serialize + DeserializeOwned + PartialEq,
    {
    }

    assert_contract::<PackageManifest>();
    assert_contract::<SourceOs>();
    assert_contract::<PackageMode>();
    assert_contract::<ContentCounts>();
    assert_contract::<ProjectEntry>();
    assert_contract::<ConversationEntry>();
    assert_contract::<ExclusionSummary>();
    assert_contract::<CodexInventory>();
    assert_contract::<TargetInventory>();
    assert_contract::<CreatePackageRequest>();
    assert_contract::<CreatePackageReport>();
    assert_contract::<PackagePreview>();
    assert_contract::<ChangeKind>();
    assert_contract::<FileConflictResolution>();
    assert_contract::<SessionAction>();
    assert_contract::<ReferenceRewriteKind>();
    assert_contract::<ReferenceRewrite>();
    assert_contract::<PlannedSession>();
    assert_contract::<PlannedOperation>();
    assert_contract::<RestorePlan>();
    assert_contract::<RestoreOptions>();
    assert_contract::<RestoreReport>();
    assert_contract::<RollbackReport>();
    assert_contract::<PendingRecovery>();
    assert_contract::<RecoveryStatus>();
    assert_contract::<VerificationReport>();
    assert_contract::<ErrorCode>();
    assert_contract::<RehomeError>();
}

#[test]
fn planning_and_recovery_enums_serialize_as_stable_snake_case_values() {
    let change_kinds = [
        (ChangeKind::Add, "add"),
        (ChangeKind::Update, "update"),
        (ChangeKind::Unchanged, "unchanged"),
        (ChangeKind::Preserve, "preserve"),
        (ChangeKind::Conflict, "conflict"),
    ];
    let session_actions = [
        (SessionAction::Skip, "skip"),
        (SessionAction::Import, "import"),
        (SessionAction::ImportAsBranch, "import_as_branch"),
    ];
    let conflict_resolutions = [
        (FileConflictResolution::KeepExisting, "keep_existing"),
        (FileConflictResolution::UsePackage, "use_package"),
    ];
    let rewrite_kinds = [
        (ReferenceRewriteKind::ConversationId, "conversation_id"),
        (
            ReferenceRewriteKind::ConversationTitle,
            "conversation_title",
        ),
        (ReferenceRewriteKind::ProjectPath, "project_path"),
        (ReferenceRewriteKind::SessionPath, "session_path"),
    ];
    let recovery_statuses = [
        (RecoveryStatus::Prepared, "prepared"),
        (RecoveryStatus::Applying, "applying"),
        (RecoveryStatus::Verifying, "verifying"),
        (RecoveryStatus::Committed, "committed"),
        (RecoveryStatus::RollingBack, "rolling_back"),
        (RecoveryStatus::RolledBack, "rolled_back"),
        (RecoveryStatus::RollbackFailed, "rollback_failed"),
    ];

    for (value, expected) in change_kinds {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }
    for (value, expected) in session_actions {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }
    for (value, expected) in conflict_resolutions {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }
    for (value, expected) in rewrite_kinds {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }
    for (value, expected) in recovery_statuses {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }
}

#[test]
fn restore_and_recovery_contracts_use_typed_state() {
    let operation = PlannedOperation {
        package_source: "projects/project-id/files/README.md".into(),
        target: PathBuf::from(r"C:\Users\NewUser\Documents\visual\README.md"),
        expected_previous_hash: None,
        action: ChangeKind::Add,
        rollback_required: true,
    };
    let plan = RestorePlan {
        plan_id: Uuid::nil(),
        package_path: PathBuf::from("handoff.rehome"),
        package_id: Uuid::nil(),
        archive_hash: "archive-sha256".into(),
        target_codex_home: PathBuf::from(r"C:\Users\NewUser\.codex"),
        projects_root: PathBuf::from(r"C:\Users\NewUser\Documents"),
        operations: vec![operation.clone()],
        sessions: vec![],
        reference_rewrites: vec![],
        bridge_verification: Default::default(),
        conflict_count: 0,
        required_bytes: 18,
    };
    let recovery = PendingRecovery {
        transaction_id: Uuid::nil(),
        package_id: Uuid::nil(),
        created_at: "2026-07-22T00:00:00Z".into(),
        status: RecoveryStatus::Prepared,
        backup_root: PathBuf::from(r"C:\Users\NewUser\AppData\Local\ReHome\backups"),
    };

    assert_eq!(plan.operations, vec![operation]);
    assert_eq!(recovery.status, RecoveryStatus::Prepared);
}

#[test]
fn error_codes_serialize_as_stable_snake_case_values() {
    let cases = [
        (ErrorCode::CodexNotFound, "codex_not_found"),
        (ErrorCode::PackageInvalid, "package_invalid"),
        (ErrorCode::ChecksumMismatch, "checksum_mismatch"),
        (ErrorCode::UnsupportedSchema, "unsupported_schema"),
        (ErrorCode::CodexRunning, "codex_running"),
        (ErrorCode::DiskSpaceInsufficient, "disk_space_insufficient"),
        (ErrorCode::ProjectConflict, "project_conflict"),
        (ErrorCode::RestoreFailed, "restore_failed"),
        (ErrorCode::RollbackFailed, "rollback_failed"),
        (ErrorCode::RegistrationIncomplete, "registration_incomplete"),
    ];

    for (code, expected) in cases {
        assert_eq!(
            serde_json::to_string(&code).unwrap(),
            format!(r#""{expected}""#)
        );
    }
}

#[test]
fn rehome_error_is_human_readable_and_has_a_stable_payload() {
    let error = RehomeError::new(ErrorCode::PackageInvalid, "manifest.json is missing");

    assert_eq!(error.to_string(), "manifest.json is missing");
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({
            "code": "package_invalid",
            "message": "manifest.json is missing"
        })
    );
}
