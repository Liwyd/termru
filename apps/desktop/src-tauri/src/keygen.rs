//! SSH key files for identities: generating a new ed25519 key and reading
//! the public half of a key that is already on disk.
//!
//! The private key never leaves this module as text: it is written straight
//! to a file that only the current user can read, and what the rest of the
//! app sees is the path, the public key and the fingerprint. Nothing here
//! overwrites an existing file; a name clash is an error, not a replacement.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use ssh_key::getrandom::SysRng;
use ssh_key::rand_core::UnwrapErr;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};

use crate::app_error::CommandError;

/// The shortest passphrase accepted for a new key. `ssh-keygen` refuses
/// anything shorter than five characters, and a passphrase that short is
/// hardly protection anyway; an empty one means "no passphrase".
pub const MIN_PASSPHRASE_LEN: usize = 5;

/// The largest file read as a key. A private key is a few kilobytes; a file
/// much larger than that is not a key, and reading it whole would waste memory.
const MAX_KEY_FILE_BYTES: u64 = 64 * 1024;

/// A key that was just written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedKey {
    pub private_path: PathBuf,
    pub public_path: PathBuf,
    /// The public key as one OpenSSH line (`ssh-ed25519 AAAA… comment`).
    pub public_key: String,
    /// `SHA256:…`, as `ssh-keygen -l` prints it.
    pub fingerprint: String,
}

/// What is known about a key file without its passphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    pub public_key: String,
    pub fingerprint: String,
    /// The algorithm name (`ssh-ed25519`, `ssh-rsa`, …).
    pub algorithm: String,
    /// The private key is protected by a passphrase.
    pub encrypted: bool,
}

/// Keeps a key comment to one printable line: a comment ends up in the
/// public key line, where a line break would start a second entry.
fn clean_comment(comment: &str) -> String {
    comment
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

fn io_error(action: &str, path: &Path, error: &std::io::Error) -> CommandError {
    CommandError::new(format!("could not {action} {}: {error}", path.display()))
}

/// Creates the directory for the keys. On Unix it is readable by its owner
/// only; on Windows it inherits the access rules of the user's profile.
fn create_keys_dir(dir: &Path) -> Result<(), CommandError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|error| io_error("create the key folder", dir, &error))
}

/// Writes a new file, failing when the path is taken. A private key is
/// created with mode 0600 from the first byte (no window in which it is
/// readable by others).
fn write_new(path: &Path, contents: &[u8], private: bool) -> Result<(), CommandError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            CommandError::new(format!(
                "{} already exists; Termru never overwrites a key file",
                path.display()
            ))
        } else {
            io_error("create", path, &error)
        }
    })?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| io_error("write", path, &error))
}

/// Generates an ed25519 key pair in `keys_dir` as `<file_stem>` and
/// `<file_stem>.pub`. A non-empty `passphrase` encrypts the private key
/// (aes256-ctr with a bcrypt-pbkdf key, the OpenSSH default).
pub fn generate_ed25519(
    keys_dir: &Path,
    file_stem: &str,
    comment: &str,
    passphrase: Option<&str>,
) -> Result<GeneratedKey, CommandError> {
    if file_stem.is_empty() || file_stem.contains(['/', '\\']) || file_stem.contains("..") {
        return Err(CommandError::new("the key file name is not valid"));
    }
    let passphrase = passphrase.filter(|value| !value.is_empty());
    if let Some(value) = passphrase {
        if value.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(CommandError::new(format!(
                "a passphrase needs at least {MIN_PASSPHRASE_LEN} characters; \
                 leave it empty for a key without one"
            )));
        }
    }

    let mut rng = UnwrapErr(SysRng);
    let mut key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|error| CommandError::new(format!("could not generate the key: {error}")))?;
    key.set_comment(clean_comment(comment));

    let public_key = key
        .public_key()
        .to_openssh()
        .map_err(|error| CommandError::new(format!("could not encode the public key: {error}")))?;
    let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

    let stored = match passphrase {
        Some(value) => key
            .encrypt(&mut SysRng, value)
            .map_err(|error| CommandError::new(format!("could not encrypt the key: {error}")))?,
        None => key,
    };
    let private_text = stored
        .to_openssh(LineEnding::LF)
        .map_err(|error| CommandError::new(format!("could not encode the key: {error}")))?;

    create_keys_dir(keys_dir)?;
    let private_path = keys_dir.join(file_stem);
    let public_path = keys_dir.join(format!("{file_stem}.pub"));
    write_new(&private_path, private_text.as_bytes(), true)?;
    if let Err(error) = write_new(&public_path, format!("{public_key}\n").as_bytes(), false) {
        // A private key without its public half would be an orphan.
        let _ = fs::remove_file(&private_path);
        return Err(error);
    }
    Ok(GeneratedKey {
        private_path,
        public_path,
        public_key,
        fingerprint,
    })
}

/// Removes the files of a generated key (the cleanup of a failed save).
pub fn remove_generated(key: &GeneratedKey) {
    let _ = fs::remove_file(&key.private_path);
    let _ = fs::remove_file(&key.public_path);
}

fn read_limited(path: &Path) -> Result<Vec<u8>, CommandError> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CommandError::new(format!("the key file {} does not exist", path.display()))
        } else {
            io_error("read", path, &error)
        }
    })?;
    if !metadata.is_file() {
        return Err(CommandError::new(format!(
            "{} is not a file",
            path.display()
        )));
    }
    if metadata.len() > MAX_KEY_FILE_BYTES {
        return Err(CommandError::new(format!(
            "{} is too large to be a key file",
            path.display()
        )));
    }
    fs::read(path).map_err(|error| io_error("read", path, &error))
}

fn info_of(public: &PublicKey, encrypted: bool) -> Result<KeyInfo, CommandError> {
    Ok(KeyInfo {
        public_key: public.to_openssh().map_err(|error| {
            CommandError::new(format!("could not encode the public key: {error}"))
        })?,
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: public.algorithm().to_string(),
        encrypted,
    })
}

/// The `.pub` file next to a key file, when it is a public key.
fn sibling_public_key(path: &Path) -> Option<PublicKey> {
    let mut sibling = path.as_os_str().to_owned();
    sibling.push(".pub");
    let text = fs::read_to_string(PathBuf::from(sibling)).ok()?;
    PublicKey::from_openssh(text.trim()).ok()
}

/// The public key of the key file at `path`, without asking for a passphrase.
///
/// An OpenSSH private key carries its public half in the clear, so an
/// encrypted key answers too. The `.pub` file next to it is preferred when
/// it holds the same key, because only that file still has the key's
/// comment (an encrypted key keeps its comment inside the encrypted part).
/// A key in an older format (`BEGIN RSA PRIVATE KEY`, PuTTY) falls back to
/// the `.pub` file alone.
pub fn read_key_info(path: &Path) -> Result<KeyInfo, CommandError> {
    let bytes = read_limited(path)?;
    if let Ok(private) = PrivateKey::from_openssh(&bytes) {
        let derived = private.public_key();
        let public = sibling_public_key(path)
            .filter(|public| public.key_data() == derived.key_data())
            .unwrap_or_else(|| derived.clone());
        return info_of(&public, private.is_encrypted());
    }
    if let Some(public) = sibling_public_key(path) {
        // The private key is in a format this module does not read, so it
        // cannot say whether a passphrase protects it.
        return info_of(&public, false);
    }
    Err(CommandError::new(format!(
        "{} is not an OpenSSH key file, so its public key cannot be shown",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn writes_an_openssh_key_pair() {
        let dir = dir();
        let keys = dir.path().join("keys");
        let key = generate_ed25519(&keys, "id_ed25519-test", "work laptop", None).expect("key");

        assert_eq!(key.private_path, keys.join("id_ed25519-test"));
        assert_eq!(key.public_path, keys.join("id_ed25519-test.pub"));

        let private = fs::read_to_string(&key.private_path).expect("private key");
        // Built from parts so secret scanners do not flag the test source.
        assert!(private.starts_with(concat!("-----BEGIN OPENSSH ", "PRIVATE", " KEY-----")));
        let parsed = PrivateKey::from_openssh(&private).expect("parses");
        assert!(!parsed.is_encrypted());
        assert_eq!(parsed.algorithm(), Algorithm::Ed25519);

        // The public key is one OpenSSH line with the comment, and the file
        // next to the private key holds the same line.
        assert!(
            key.public_key.starts_with("ssh-ed25519 "),
            "{}",
            key.public_key
        );
        assert!(
            key.public_key.ends_with(" work laptop"),
            "{}",
            key.public_key
        );
        assert!(!key.public_key.contains('\n'));
        let on_disk = fs::read_to_string(&key.public_path).expect("public key");
        assert_eq!(on_disk, format!("{}\n", key.public_key));
        let public = PublicKey::from_openssh(on_disk.trim()).expect("public key parses");
        assert_eq!(public.key_data(), parsed.public_key().key_data());

        // The fingerprint is the SHA-256 one `ssh-keygen -l` shows.
        assert!(
            key.fingerprint.starts_with("SHA256:"),
            "{}",
            key.fingerprint
        );
        assert_eq!(key.fingerprint.len(), "SHA256:".len() + 43);
        assert_eq!(
            key.fingerprint,
            parsed.fingerprint(HashAlg::Sha256).to_string()
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_private_key_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir();
        let keys = dir.path().join("keys");
        let key = generate_ed25519(&keys, "id", "k", None).expect("key");
        let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&key.private_path), 0o600);
        assert_eq!(mode(&key.public_path), 0o644);
        assert_eq!(mode(&keys), 0o700);
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let dir = dir();
        let keys = dir.path().join("keys");
        fs::create_dir_all(&keys).expect("dir");

        // The private key's name is taken.
        fs::write(keys.join("id"), "precious").expect("existing");
        let error = generate_ed25519(&keys, "id", "k", None).expect_err("refused");
        assert!(
            error.message.contains("never overwrites"),
            "{}",
            error.message
        );
        assert_eq!(
            fs::read_to_string(keys.join("id")).expect("read"),
            "precious"
        );
        assert!(!keys.join("id.pub").exists());

        // Only the public key's name is taken: nothing is left behind.
        fs::write(keys.join("other.pub"), "mine").expect("existing public");
        let error = generate_ed25519(&keys, "other", "k", None).expect_err("refused");
        assert!(
            error.message.contains("never overwrites"),
            "{}",
            error.message
        );
        assert_eq!(
            fs::read_to_string(keys.join("other.pub")).expect("read"),
            "mine"
        );
        assert!(!keys.join("other").exists(), "no orphaned private key");
    }

    #[test]
    fn a_passphrase_encrypts_the_key_and_the_key_loads_with_it() {
        let dir = dir();
        let keys = dir.path().join("keys");
        let key = generate_ed25519(&keys, "id", "locked", Some("correct horse")).expect("key");

        let text = fs::read_to_string(&key.private_path).expect("private key");
        let parsed = PrivateKey::from_openssh(&text).expect("parses");
        assert!(parsed.is_encrypted());
        // The public half is readable without the passphrase and matches.
        let public = PublicKey::from_openssh(&key.public_key).expect("public key");
        assert_eq!(parsed.public_key().key_data(), public.key_data());

        let unlocked = parsed.decrypt("correct horse").expect("right passphrase");
        assert!(!unlocked.is_encrypted());
        assert_eq!(
            unlocked.fingerprint(HashAlg::Sha256).to_string(),
            key.fingerprint
        );
        assert!(
            parsed.decrypt("wrong passphrase").is_err(),
            "a wrong passphrase must not unlock the key"
        );
    }

    /// OpenSSH itself reads what this module writes: `ssh-keygen -y` derives
    /// the public key from the private file, with and without a passphrase.
    /// Skipped where OpenSSH is not installed (a Windows runner).
    #[test]
    fn openssh_reads_the_generated_keys() {
        let probe = std::process::Command::new("ssh-keygen").arg("-?").output();
        if probe.is_err() {
            eprintln!("SKIPPED: ssh-keygen is not installed on this machine");
            return;
        }
        let dir = dir();
        for passphrase in [None, Some("correct horse")] {
            let stem = if passphrase.is_some() {
                "locked"
            } else {
                "open"
            };
            let key = generate_ed25519(dir.path(), stem, "interop", passphrase).expect("key");
            let output = std::process::Command::new("ssh-keygen")
                .args(["-y", "-P", passphrase.unwrap_or(""), "-f"])
                .arg(&key.private_path)
                .output()
                .expect("ssh-keygen runs");
            assert!(
                output.status.success(),
                "ssh-keygen refused the key: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let derived = String::from_utf8_lossy(&output.stdout);
            let ours = PublicKey::from_openssh(&key.public_key).expect("ours");
            let theirs = PublicKey::from_openssh(derived.trim()).expect("theirs");
            assert_eq!(ours.key_data(), theirs.key_data());
        }
    }

    #[test]
    fn a_too_short_passphrase_is_refused_before_anything_is_written() {
        let dir = dir();
        let keys = dir.path().join("keys");
        let error = generate_ed25519(&keys, "id", "k", Some("abcd")).expect_err("refused");
        assert!(error.message.contains("at least 5"), "{}", error.message);
        assert!(!keys.exists(), "nothing was created");

        // An empty passphrase means no passphrase.
        let key = generate_ed25519(&keys, "id", "k", Some("")).expect("key");
        let parsed = PrivateKey::from_openssh(fs::read_to_string(&key.private_path).expect("read"))
            .expect("parses");
        assert!(!parsed.is_encrypted());
    }

    #[test]
    fn a_file_name_cannot_leave_the_key_folder() {
        let dir = dir();
        for stem in ["", "../id", "a/b", "a\\b", ".."] {
            let error = generate_ed25519(dir.path(), stem, "k", None).expect_err(stem);
            assert!(
                error.message.contains("not valid"),
                "{stem}: {}",
                error.message
            );
        }
    }

    #[test]
    fn a_comment_stays_on_one_line() {
        let dir = dir();
        let key = generate_ed25519(dir.path(), "id", "evil\nssh-rsa AAAA\r\n", None).expect("key");
        assert_eq!(key.public_key.lines().count(), 1, "{}", key.public_key);
    }

    #[test]
    fn two_keys_are_never_the_same() {
        let dir = dir();
        let first = generate_ed25519(dir.path(), "a", "k", None).expect("first");
        let second = generate_ed25519(dir.path(), "b", "k", None).expect("second");
        assert_ne!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn the_public_key_of_a_file_comes_without_the_passphrase() {
        let dir = dir();
        let key = generate_ed25519(dir.path(), "locked", "k", Some("secret pass")).expect("key");
        let info = read_key_info(&key.private_path).expect("info");
        assert_eq!(info.public_key, key.public_key);
        assert_eq!(info.fingerprint, key.fingerprint);
        assert_eq!(info.algorithm, "ssh-ed25519");
        assert!(info.encrypted);

        let open = generate_ed25519(dir.path(), "open", "k", None).expect("key");
        assert!(!read_key_info(&open.private_path).expect("info").encrypted);
    }

    #[test]
    fn a_key_in_another_format_falls_back_to_its_public_file() {
        let dir = dir();
        let key = generate_ed25519(dir.path(), "real", "k", None).expect("key");
        // A "private key" this module cannot parse, with a public file next to it.
        let legacy = dir.path().join("legacy");
        fs::write(
            &legacy,
            concat!("-----BEGIN RSA ", "PRIVATE", " KEY-----\nnot parsed\n"),
        )
        .expect("legacy");
        fs::write(
            dir.path().join("legacy.pub"),
            format!("{}\n", key.public_key),
        )
        .expect("pub");
        let info = read_key_info(&legacy).expect("info");
        assert_eq!(info.fingerprint, key.fingerprint);

        // Without the public file there is nothing to show.
        let lonely = dir.path().join("lonely");
        fs::write(
            &lonely,
            concat!("-----BEGIN RSA ", "PRIVATE", " KEY-----\nnot parsed\n"),
        )
        .expect("lonely");
        let error = read_key_info(&lonely).expect_err("no public key");
        assert!(
            error.message.contains("not an OpenSSH key"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_missing_or_huge_or_wrong_kind_of_file_is_reported() {
        let dir = dir();
        let error = read_key_info(&dir.path().join("missing")).expect_err("missing");
        assert!(
            error.message.contains("does not exist"),
            "{}",
            error.message
        );

        let error = read_key_info(dir.path()).expect_err("a folder");
        assert!(error.message.contains("not a file"), "{}", error.message);

        let huge = dir.path().join("huge");
        fs::write(&huge, vec![b'x'; (MAX_KEY_FILE_BYTES + 1) as usize]).expect("huge");
        let error = read_key_info(&huge).expect_err("huge");
        assert!(error.message.contains("too large"), "{}", error.message);
    }
}
