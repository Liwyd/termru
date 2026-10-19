use serde::Serialize;
use termru_core::error::CoreError;
use termru_ssh::SshError;
use termru_store::StoreError;

/// Error type returned from Tauri commands. Message-first: it is shown
/// directly in the UI, so it must be actionable and must never contain
/// secret material.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub message: String,
}

impl CommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn not_found(what: &str, id: &str) -> Self {
        Self::new(format!("no {what} with id {id}"))
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<CoreError> for CommandError {
    fn from(error: CoreError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<StoreError> for CommandError {
    fn from(error: StoreError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<SshError> for CommandError {
    fn from(error: SshError) -> Self {
        Self::new(error.to_string())
    }
}
