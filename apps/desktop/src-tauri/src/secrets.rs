//! OS credential store integration.
//!
//! Connection secrets (passwords, key passphrases) live in the platform
//! credential store — Secret Service on Linux, Credential Manager on
//! Windows, Keychain on macOS — and **never** in the local database, the
//! sync payload or the logs.
//!
//! When no credential store is reachable the app degrades gracefully: the
//! user is asked for the secret on every connection and everything else
//! keeps working.

use keyring::{Entry, Error as KeyringError};

use crate::app_error::CommandError;

/// Service name used in the platform credential store.
pub(crate) const SERVICE: &str = "com.liwyd.termru";

/// The kind of secret a host needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    Password,
    Passphrase,
}

impl SecretKind {
    /// Wire name used by the frontend.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Passphrase => "passphrase",
        }
    }

    /// Parses the wire name; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "password" => Some(Self::Password),
            "passphrase" => Some(Self::Passphrase),
            _ => None,
        }
    }
}

/// Reads and writes connection secrets in the OS credential store.
pub struct SecretStore;

impl SecretStore {
    fn entry(host_id: &str, kind: SecretKind) -> Result<Entry, KeyringError> {
        Entry::new(SERVICE, &format!("{host_id}/{}", kind.as_str()))
    }

    /// Stores a secret for a host.
    pub fn set(host_id: &str, kind: SecretKind, secret: &str) -> Result<(), CommandError> {
        let entry = Self::entry(host_id, kind).map_err(keyring_error)?;
        entry.set_password(secret).map_err(keyring_error)
    }

    /// Loads a stored secret. `None` means "not stored" **or** "no
    /// credential store available" — the caller then asks the user.
    pub fn get(host_id: &str, kind: SecretKind) -> Option<String> {
        let entry = Self::entry(host_id, kind).ok()?;
        match entry.get_password() {
            Ok(secret) => Some(secret),
            Err(KeyringError::NoEntry) => None,
            Err(_) => None,
        }
    }

    /// Removes a stored secret.
    pub fn delete(host_id: &str, kind: SecretKind) -> Result<(), CommandError> {
        let entry = Self::entry(host_id, kind).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            // Already gone: nothing to do.
            Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }

    /// Whether a secret is stored (used by the UI to show "remembered").
    pub fn has(host_id: &str, kind: SecretKind) -> bool {
        Self::get(host_id, kind).is_some()
    }

    /// Whether a credential store is reachable on this machine.
    ///
    /// A machine without one (a CI runner, a headless server) is a missing
    /// prerequisite, not a failure: the app asks for the secret on every
    /// connection and everything else keeps working.
    pub fn available() -> bool {
        let Ok(entry) = Self::entry("termru-probe", SecretKind::Password) else {
            return false;
        };
        match entry.get_password() {
            Ok(_) => true,
            // The store is there, the entry is not: available.
            Err(KeyringError::NoEntry) => true,
            Err(_) => false,
        }
    }
}

/// Maps a keyring error into an actionable message (never leaks the
/// secret).
fn keyring_error(error: KeyringError) -> CommandError {
    match error {
        KeyringError::NoEntry => CommandError::new("no stored secret for this host"),
        KeyringError::NoStorageAccess(_) => CommandError::new(
            "no system credential store is available on this machine; \
             the secret will be asked for on every connection",
        ),
        KeyringError::PlatformFailure(error) => {
            CommandError::new(format!("the system credential store failed: {error}"))
        }
        KeyringError::BadEncoding(_) => {
            CommandError::new("the credential store returned an unreadable secret")
        }
        other => CommandError::new(format!("credential store error: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_kind_round_trips() {
        assert_eq!(SecretKind::parse("password"), Some(SecretKind::Password));
        assert_eq!(
            SecretKind::parse("passphrase"),
            Some(SecretKind::Passphrase)
        );
        assert_eq!(SecretKind::parse("nope"), None);
    }

    #[test]
    fn missing_secret_is_none_not_an_error() {
        // Either the store is unavailable or the entry does not exist —
        // both mean "ask the user".
        assert!(SecretStore::get("termru-test-host", SecretKind::Password).is_none());
        assert!(!SecretStore::has("termru-test-host", SecretKind::Password));
    }

    #[test]
    fn delete_of_a_missing_secret_is_not_an_error() {
        // Without a credential store there is nothing to delete: the
        // prerequisite is missing, not broken.
        if !SecretStore::available() {
            eprintln!("SKIPPED: no system credential store is available on this machine");
            return;
        }
        // Deleting something that is not there must succeed.
        let result = SecretStore::delete("termru-test-host", SecretKind::Password);
        assert!(result.is_ok(), "{:?}", result.err().map(|e| e.message));
    }
}
