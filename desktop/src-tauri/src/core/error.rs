use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    CodexNotFound,
    PackageInvalid,
    ChecksumMismatch,
    UnsupportedSchema,
    CodexRunning,
    DiskSpaceInsufficient,
    ProjectConflict,
    RestoreFailed,
    RollbackFailed,
    RegistrationIncomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RehomeError {
    pub code: ErrorCode,
    pub message: String,
}

impl RehomeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RehomeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RehomeError {}
