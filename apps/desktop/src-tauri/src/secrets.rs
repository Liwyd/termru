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
//!
//! ## Whose secret it is
//!
//! A **password** belongs to a host (`<host id>/password`). A key
//! **passphrase** belongs to the key, that is to its *identity*
//! (`identity:<identity id>/passphrase`): every host that authenticates with
//! the identity shares one stored passphrase, and changing the host's other
//! settings never orphans it. Earlier versions stored the passphrase per host
//! (`<host id>/passphrase`); such an entry is still *read* (and removed the
//! next time the passphrase is saved under the identity).

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

/// The keyring user name of a secret.
fn entry_name(owner: &str, kind: SecretKind) -> String {
    format!("{owner}/{}", kind.as_str())
}

/// The owner of a key passphrase: derived from the identity.
fn passphrase_owner(identity_id: &str) -> String {
    format!("identity:{identity_id}")
}

/// A passphrase lookup that falls back to the pre-identity per-host entry.
fn passphrase_with_fallback(
    get: impl Fn(&str) -> Option<String>,
    identity_id: &str,
    legacy_host_id: &str,
) -> Option<String> {
    get(&passphrase_owner(identity_id)).or_else(|| get(legacy_host_id))
}

impl SecretStore {
    fn entry(owner: &str, kind: SecretKind) -> Result<Entry, KeyringError> {
        Entry::new(SERVICE, &entry_name(owner, kind))
    }

    /// The stored passphrase of an identity's key. `legacy_host_id` is the
    /// host the connection is for: an entry an older version stored under
    /// that host is used when the identity has none yet.
    pub fn get_passphrase(identity_id: &str, legacy_host_id: &str) -> Option<String> {
        passphrase_with_fallback(
            |owner| Self::get(owner, SecretKind::Passphrase),
            identity_id,
            legacy_host_id,
        )
    }

    /// Whether a passphrase is stored for the identity (or, from an older
    /// version, for the host).
    pub fn has_passphrase(identity_id: &str, legacy_host_id: &str) -> bool {
        Self::get_passphrase(identity_id, legacy_host_id).is_some()
    }

    /// Stores the passphrase under the identity. A leftover per-host entry is
    /// removed: it would be stale the day the passphrase changes.
    pub fn set_passphrase(
        identity_id: &str,
        legacy_host_id: &str,
        secret: &str,
    ) -> Result<(), CommandError> {
        Self::set(
            &passphrase_owner(identity_id),
            SecretKind::Passphrase,
            secret,
        )?;
        // Best effort: the new entry is what counts.
        let _ = Self::delete(legacy_host_id, SecretKind::Passphrase);
        Ok(())
    }

    /// Removes the passphrase of the identity and the host's older entry.
    pub fn delete_passphrase(identity_id: &str, legacy_host_id: &str) -> Result<(), CommandError> {
        Self::delete(&passphrase_owner(identity_id), SecretKind::Passphrase)?;
        Self::delete(legacy_host_id, SecretKind::Passphrase)
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
    #[cfg(test)]
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
    fn passphrases_are_keyed_by_identity_not_by_host() {
        let owner = passphrase_owner("11111111-2222-4333-8444-555555555555");
        assert_eq!(owner, "identity:11111111-2222-4333-8444-555555555555");
        assert_eq!(
            entry_name(&owner, SecretKind::Passphrase),
            "identity:11111111-2222-4333-8444-555555555555/passphrase"
        );
        // A password stays under its host.
        assert_eq!(
            entry_name("host-1", SecretKind::Password),
            "host-1/password"
        );
        // Two hosts, one identity: the same entry.
        assert_eq!(passphrase_owner("i1"), passphrase_owner("i1"));
        assert_ne!(passphrase_owner("i1"), passphrase_owner("i2"));
    }

    /// Migration: an older version stored the passphrase under the host. It
    /// is still found, and the identity's own entry wins once there is one.
    #[test]
    fn an_older_per_host_passphrase_is_still_read() {
        let stored = |entries: &'static [(&'static str, &'static str)]| {
            move |owner: &str| {
                entries
                    .iter()
                    .find(|(key, _)| *key == owner)
                    .map(|(_, value)| (*value).to_string())
            }
        };
        let legacy_only = stored(&[("host-1", "old-passphrase")]);
        assert_eq!(
            passphrase_with_fallback(legacy_only, "id-1", "host-1").as_deref(),
            Some("old-passphrase")
        );
        let both = stored(&[("host-1", "old"), ("identity:id-1", "new")]);
        assert_eq!(
            passphrase_with_fallback(both, "id-1", "host-1").as_deref(),
            Some("new")
        );
        let other_host = stored(&[("host-2", "other")]);
        assert_eq!(passphrase_with_fallback(other_host, "id-1", "host-1"), None);
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
