use rehome_desktop_lib::core::{
    error::{ErrorCode, RehomeError},
    models::{
        CodexInventory, ContentCounts, ConversationEntry, CreatePackageReport,
        CreatePackageRequest, ExclusionSummary, PackageManifest, PackageMode, PackagePreview,
        PendingRecovery, ProjectEntry, RestoreOptions, RestorePlan, RestoreReport, RollbackReport,
        SourceOs, TargetInventory, VerificationReport,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
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
    assert_contract::<RestorePlan>();
    assert_contract::<RestoreOptions>();
    assert_contract::<RestoreReport>();
    assert_contract::<RollbackReport>();
    assert_contract::<PendingRecovery>();
    assert_contract::<VerificationReport>();
    assert_contract::<ErrorCode>();
    assert_contract::<RehomeError>();
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
