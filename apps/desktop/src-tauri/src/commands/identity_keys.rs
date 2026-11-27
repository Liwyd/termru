//! Identity keys: generating an ed25519 key for an identity, and showing the
//! public half of the key an identity is bound to.
//!
//! A generated key lives in the app's `keys/` folder, never overwrites a
//! file and is readable by its owner only. Its passphrase is optional; it
//! reaches the credential store (under the identity) only when the user asks
//! for that, and a credential store that is not there is a note, not a
//! failure: the key exists and works, the passphrase is asked for on connect.

use std::path::Path;

use serde::Serialize;
use tauri::State;
use termru_core::identity::Identity;
use termru_core::ids::IdentityId;
use termru_store::Store;

use super::secrets::blocking;
use crate::app_error::CommandError;
use crate::keygen::{self, KeyInfo};
use crate::secrets::SecretStore;
use crate::state::AppState;

/// The result of `identity_generate`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct GeneratedIdentity {
    /// The saved identity (with its fingerprint).
    pub identity: Identity,
    /// Where the private key was written.
    pub key_path: String,
    /// The public key as one OpenSSH line.
    pub public_key: String,
    pub fingerprint: String,
    /// The passphrase is now in the credential store.
    pub passphrase_remembered: bool,
    /// Why the passphrase was not remembered, when the user asked for it.
    pub passphrase_note: Option<String>,
}

/// What `identity_public_key` knows about the key an identity is bound to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PublicKeyInfo {
    pub public_key: String,
    pub fingerprint: String,
    pub algorithm: String,
    pub encrypted: bool,
    pub key_path: String,
}

impl PublicKeyInfo {
    fn new(info: KeyInfo, key_path: String) -> Self {
        Self {
            public_key: info.public_key,
            fingerprint: info.fingerprint,
            algorithm: info.algorithm,
            encrypted: info.encrypted,
            key_path,
        }
    }
}

/// The user's request, as the command receives it.
struct Request {
    name: String,
    comment: Option<String>,
    passphrase: Option<String>,
    remember_passphrase: bool,
}

/// Creates the key, the identity and its binding. `remember` stores the
/// passphrase of the identity (the real one talks to the credential store;
/// the tests count its calls).
fn generate_identity(
    store: &Store,
    keys_dir: &Path,
    request: Request,
    remember: impl FnOnce(&str, &str) -> Result<(), CommandError>,
) -> Result<GeneratedIdentity, CommandError> {
    let name = request.name.trim().to_string();
    let mut identity = Identity::new(name.clone());
    identity.comment = request
        .comment
        .map(|comment| comment.trim().to_string())
        .filter(|comment| !comment.is_empty());
    // Everything the record would be refused for is refused before a key is
    // written: a rejected name leaves no file behind.
    identity.validate()?;

    let passphrase = request.passphrase.filter(|value| !value.is_empty());
    let stem = format!("id_ed25519-{}", identity.id);
    let key = keygen::generate_ed25519(keys_dir, &stem, &name, passphrase.as_deref())?;

    identity.fingerprint = Some(key.fingerprint.clone());
    let key_path = key.private_path.to_string_lossy().into_owned();
    let repos = store.repos();
    let saved = repos.identities().save(&mut identity).and_then(|()| {
        repos
            .identities()
            .bind(identity.id, &key_path)
            .inspect_err(|_| {
                // The identity without a key would be a dead entry.
                let _ = repos.identities().soft_delete(identity.id);
            })
    });
    if let Err(error) = saved {
        keygen::remove_generated(&key);
        return Err(error.into());
    }

    let (remembered, note) = match (&passphrase, request.remember_passphrase) {
        (Some(value), true) => match remember(&identity.id.to_string(), value) {
            Ok(()) => (true, None),
            Err(error) => (
                false,
                Some(format!(
                    "The passphrase could not be stored on this device ({error}). \
                     You will be asked for it when you connect."
                )),
            ),
        },
        _ => (false, None),
    };

    Ok(GeneratedIdentity {
        identity,
        key_path,
        public_key: key.public_key,
        fingerprint: key.fingerprint,
        passphrase_remembered: remembered,
        passphrase_note: note,
    })
}

/// The public key of the key an identity is bound to on this device; `None`
/// when no key file is bound.
fn public_key_of(store: &Store, id: IdentityId) -> Result<Option<PublicKeyInfo>, CommandError> {
    let Some(path) = store.repos().identities().binding(id)? else {
        return Ok(None);
    };
    let info = keygen::read_key_info(Path::new(&path))?;
    Ok(Some(PublicKeyInfo::new(info, path)))
}

/// Generates an ed25519 key and an identity for it. A non-empty `passphrase`
/// encrypts the key; `remember_passphrase` keeps it in the credential store.
#[tauri::command]
pub async fn identity_generate(
    state: State<'_, AppState>,
    name: String,
    comment: Option<String>,
    passphrase: Option<String>,
    remember_passphrase: bool,
) -> Result<GeneratedIdentity, CommandError> {
    let store = state.store.clone();
    let keys_dir = state.keys_dir.clone();
    let request = Request {
        name,
        comment,
        passphrase,
        remember_passphrase,
    };
    // Deriving the key from the passphrase (bcrypt-pbkdf) takes a moment.
    blocking(move || {
        generate_identity(&store, &keys_dir, request, |identity, passphrase| {
            SecretStore::set_identity_passphrase(identity, passphrase)
        })
    })
    .await
}

/// The public key, fingerprint and algorithm of the key file an identity is
/// bound to (no passphrase needed). `None` when nothing is bound.
#[tauri::command]
pub async fn identity_public_key(
    state: State<'_, AppState>,
    id: IdentityId,
) -> Result<Option<PublicKeyInfo>, CommandError> {
    let store = state.store.clone();
    blocking(move || public_key_of(&store, id)).await
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use ssh_key::{HashAlg, PrivateKey};

    use super::*;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&dir.path().join("keys.sqlite")).expect("store");
        (dir, store)
    }

    fn request(name: &str, passphrase: Option<&str>, remember: bool) -> Request {
        Request {
            name: name.to_string(),
            comment: Some("  work laptop  ".to_string()),
            passphrase: passphrase.map(str::to_string),
            remember_passphrase: remember,
        }
    }

    #[test]
    fn creates_the_identity_the_key_and_the_binding() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        let made = generate_identity(&store, &keys, request("  laptop  ", None, false), |_, _| {
            panic!("no passphrase, nothing to remember")
        })
        .expect("generated");

        assert_eq!(made.identity.name, "laptop");
        assert_eq!(made.identity.comment.as_deref(), Some("work laptop"));
        assert_eq!(
            made.identity.fingerprint.as_deref(),
            Some(made.fingerprint.as_str())
        );
        assert!(made.fingerprint.starts_with("SHA256:"));
        assert!(made.public_key.starts_with("ssh-ed25519 "));
        assert!(!made.passphrase_remembered);
        assert_eq!(made.passphrase_note, None);

        // The key sits in the key folder, under a name that carries the id.
        assert!(made.key_path.starts_with(keys.to_str().expect("utf-8")));
        assert!(made.key_path.contains(&made.identity.id.to_string()));
        assert!(Path::new(&made.key_path).is_file());

        // The record and the binding are in the store.
        let repos = store.repos();
        let listed = repos.identities().list().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].fingerprint, made.identity.fingerprint);
        assert_eq!(
            repos
                .identities()
                .binding(made.identity.id)
                .expect("binding"),
            Some(made.key_path.clone())
        );
    }

    #[test]
    fn a_rejected_name_leaves_no_key_behind() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        for name in ["", "   ", "bad\nname"] {
            let error = generate_identity(&store, &keys, request(name, None, false), |_, _| Ok(()))
                .expect_err(name);
            assert!(
                error.message.contains("name"),
                "{name:?}: {}",
                error.message
            );
        }
        assert!(!keys.exists(), "nothing was written");
        assert!(store.repos().identities().list().expect("list").is_empty());
    }

    #[test]
    fn two_identities_never_share_a_key_file() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        let a = generate_identity(&store, &keys, request("same", None, false), |_, _| Ok(()))
            .expect("first");
        let b = generate_identity(&store, &keys, request("same", None, false), |_, _| Ok(()))
            .expect("second");
        assert_ne!(a.key_path, b.key_path);
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn the_passphrase_is_remembered_only_when_asked_for() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        let calls: RefCell<Vec<(String, String)>> = RefCell::new(Vec::new());
        let record = |identity: &str, passphrase: &str| {
            calls
                .borrow_mut()
                .push((identity.to_string(), passphrase.to_string()));
            Ok(())
        };

        // A passphrase, not remembered: the key is encrypted, the store untouched.
        let locked = generate_identity(
            &store,
            &keys,
            request("locked", Some("correct horse"), false),
            record,
        )
        .expect("locked");
        assert!(calls.borrow().is_empty());
        assert!(!locked.passphrase_remembered);
        let parsed = PrivateKey::from_openssh(
            std::fs::read_to_string(&locked.key_path).expect("private key"),
        )
        .expect("parses");
        assert!(parsed.is_encrypted());
        assert!(parsed.decrypt("correct horse").is_ok());

        // A passphrase, remembered: stored under the identity.
        let remembered = generate_identity(
            &store,
            &keys,
            request("remembered", Some("correct horse"), true),
            |identity, passphrase| {
                calls
                    .borrow_mut()
                    .push((identity.to_string(), passphrase.to_string()));
                Ok(())
            },
        )
        .expect("remembered");
        assert!(remembered.passphrase_remembered);
        assert_eq!(
            calls.borrow().as_slice(),
            [(
                remembered.identity.id.to_string(),
                "correct horse".to_string()
            )]
        );

        // "Remember" without a passphrase has nothing to remember.
        let open = generate_identity(&store, &keys, request("open", None, true), |_, _| {
            panic!("no passphrase, nothing to remember")
        })
        .expect("open");
        assert!(!open.passphrase_remembered);
        let parsed =
            PrivateKey::from_openssh(std::fs::read_to_string(&open.key_path).expect("private key"))
                .expect("parses");
        assert!(!parsed.is_encrypted());
    }

    #[test]
    fn a_credential_store_that_is_not_there_is_a_note_not_a_failure() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        let made = generate_identity(
            &store,
            &keys,
            request("no-store", Some("correct horse"), true),
            |_, _| Err(CommandError::new("no system credential store is available")),
        )
        .expect("the key is made all the same");
        assert!(!made.passphrase_remembered);
        let note = made.passphrase_note.expect("a note");
        assert!(note.contains("asked for it when you connect"), "{note}");
        assert!(Path::new(&made.key_path).is_file());
        assert_eq!(store.repos().identities().list().expect("list").len(), 1);
    }

    #[test]
    fn a_too_short_passphrase_creates_nothing() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        let error = generate_identity(
            &store,
            &keys,
            request("short", Some("abc"), true),
            |_, _| Ok(()),
        )
        .expect_err("refused");
        assert!(error.message.contains("passphrase"), "{}", error.message);
        assert!(store.repos().identities().list().expect("list").is_empty());
        assert!(!keys.exists());
    }

    #[test]
    fn shows_the_public_key_of_the_bound_file() {
        let (dir, store) = temp_store();
        let keys = dir.path().join("keys");
        let made = generate_identity(
            &store,
            &keys,
            request("shown", Some("correct horse"), false),
            |_, _| Ok(()),
        )
        .expect("generated");

        let info = public_key_of(&store, made.identity.id)
            .expect("read")
            .expect("bound");
        assert_eq!(info.public_key, made.public_key);
        assert_eq!(info.fingerprint, made.fingerprint);
        assert_eq!(info.algorithm, "ssh-ed25519");
        assert!(info.encrypted);
        assert_eq!(info.key_path, made.key_path);

        // The fingerprint is the one of the key itself.
        let private =
            PrivateKey::from_openssh(std::fs::read_to_string(&made.key_path).expect("private key"))
                .expect("parses");
        assert_eq!(
            info.fingerprint,
            private.fingerprint(HashAlg::Sha256).to_string()
        );
    }

    #[test]
    fn an_identity_without_a_bound_key_has_no_public_key() {
        let (_dir, store) = temp_store();
        let mut identity = Identity::new("unbound".to_string());
        store
            .repos()
            .identities()
            .save(&mut identity)
            .expect("save");
        assert!(public_key_of(&store, identity.id).expect("read").is_none());

        // Bound to a file that is gone: an actionable error, not a panic.
        store
            .repos()
            .identities()
            .bind(identity.id, "/nonexistent/termru-test-key")
            .expect("bind");
        let error = public_key_of(&store, identity.id).expect_err("missing file");
        assert!(
            error.message.contains("does not exist"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_wire_shapes_are_snake_case_like_the_other_dtos() {
        let (dir, store) = temp_store();
        let made = generate_identity(
            &store,
            &dir.path().join("keys"),
            request("wire", None, false),
            |_, _| Ok(()),
        )
        .expect("generated");
        let json = serde_json::to_value(&made).expect("json");
        for field in [
            "identity",
            "key_path",
            "public_key",
            "fingerprint",
            "passphrase_remembered",
            "passphrase_note",
        ] {
            assert!(json.get(field).is_some(), "missing {field}: {json}");
        }
        let info = serde_json::to_value(
            public_key_of(&store, made.identity.id)
                .expect("read")
                .expect("bound"),
        )
        .expect("json");
        for field in [
            "public_key",
            "fingerprint",
            "algorithm",
            "encrypted",
            "key_path",
        ] {
            assert!(info.get(field).is_some(), "missing {field}: {info}");
        }
    }
}
