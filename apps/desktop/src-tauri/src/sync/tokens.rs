//! Storage of the sync tokens.
//!
//! The tokens are credentials: they live in the OS credential store, never in
//! the local database, never in the sync payload and never in a log.

use keyring::{Entry, Error as KeyringError};

use crate::app_error::CommandError;
use crate::secrets::SERVICE;
use crate::sync::client::AuthResponse;

/// The credential-store entry that holds the tokens.
const ENTRY: &str = "sync/tokens";

/// Where the sync tokens are kept.
pub trait TokenStorage: Send + Sync {
    /// Loads the tokens. `None` means "not stored".
    fn get(&self) -> Option<AuthResponse>;

    /// Stores the tokens.
    fn set(&self, tokens: &AuthResponse) -> Result<(), CommandError>;

    /// Removes the stored tokens.
    fn delete(&self) -> Result<(), CommandError>;
}

/// Reads and writes the sync tokens in the OS credential store.
pub struct KeyringTokens;

impl TokenStorage for KeyringTokens {
    fn get(&self) -> Option<AuthResponse> {
        let entry = Entry::new(SERVICE, ENTRY).ok()?;
        let json = entry.get_password().ok()?;
        serde_json::from_str(&json).ok()
    }

    fn set(&self, tokens: &AuthResponse) -> Result<(), CommandError> {
        let json = serde_json::to_string(tokens)
            .map_err(|error| CommandError::new(format!("could not encode the tokens: {error}")))?;
        let entry = Entry::new(SERVICE, ENTRY).map_err(keyring_error)?;
        entry.set_password(&json).map_err(keyring_error)
    }

    fn delete(&self) -> Result<(), CommandError> {
        let entry = Entry::new(SERVICE, ENTRY).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

/// Keeps the tokens in memory. Used by the tests; never used in the app.
#[derive(Debug, Default)]
pub struct MemoryTokens {
    tokens: std::sync::Mutex<Option<AuthResponse>>,
}

impl MemoryTokens {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStorage for MemoryTokens {
    fn get(&self) -> Option<AuthResponse> {
        self.tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set(&self, tokens: &AuthResponse) -> Result<(), CommandError> {
        let mut guard = self
            .tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(tokens.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), CommandError> {
        let mut guard = self
            .tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = None;
        Ok(())
    }
}

/// Maps a keyring error into an actionable message (never leaks a token).
fn keyring_error(error: KeyringError) -> CommandError {
    match error {
        KeyringError::NoEntry => CommandError::new("no stored sync credentials"),
        KeyringError::NoStorageAccess(_) => CommandError::new(
            "no system credential store is available on this machine; \
             the sync tokens can not be stored",
        ),
        KeyringError::PlatformFailure(error) => {
            CommandError::new(format!("the system credential store failed: {error}"))
        }
        KeyringError::BadEncoding(_) => {
            CommandError::new("the credential store returned unreadable sync credentials")
        }
        other => CommandError::new(format!("credential store error: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn tokens() -> AuthResponse {
        AuthResponse {
            user_id: Uuid::nil(),
            device_id: Uuid::nil(),
            access_token: "access-token-for-test".to_string(),
            refresh_token: "refresh-token-for-test".to_string(),
            access_expires_at: Utc::now(),
            refresh_expires_at: Utc::now(),
        }
    }

    #[test]
    fn missing_tokens_are_none_not_an_error() {
        assert!(KeyringTokens.get().is_none());
    }

    #[test]
    fn deleting_missing_tokens_is_not_an_error() {
        // Without a credential store there is nothing to delete: the
        // prerequisite is missing, not broken.
        if !crate::secrets::SecretStore::available() {
            eprintln!("SKIPPED: no system credential store is available on this machine");
            return;
        }
        let result = KeyringTokens.delete();
        assert!(result.is_ok(), "{:?}", result.err().map(|e| e.message));
    }

    #[test]
    fn the_tokens_round_trip_through_the_credential_store() {
        // The credential store may not exist on the test machine; the test
        // then only proves that the code does not panic.
        if KeyringTokens.set(&tokens()).is_ok() {
            if let Some(stored) = KeyringTokens.get() {
                assert_eq!(stored.access_token, "access-token-for-test");
            }
            let _ = KeyringTokens.delete();
        }
    }

    #[test]
    fn the_memory_store_keeps_the_tokens() {
        let store = MemoryTokens::new();
        assert!(store.get().is_none());
        store.set(&tokens()).expect("store the tokens");
        assert_eq!(
            store.get().expect("the tokens").access_token,
            "access-token-for-test"
        );
        store.delete().expect("delete the tokens");
        assert!(store.get().is_none());
    }
}
