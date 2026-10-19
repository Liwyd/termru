//! OpenSSH-compatible `known_hosts` handling.
//!
//! Supports the format real OpenSSH clients read and write, including:
//! * plain and hashed (`|1|salt|HMAC-SHA1`) host patterns
//! * `[host]:port` addressing for non-default ports
//! * comma-separated pattern lists, `*`/`?` wildcards, and `!` negation
//! * `@revoked` markers (hard refusal) and `@cert-authority` (ignored; Termru
//!   does not validate host certificates in this version)
//! * multiple key lines per host, matched by key type
//!
//! Policy (documented in `docs/architecture/0002-ssh-transport.md`):
//! * same host + same key type + different key ⇒ **Changed** (hard warning,
//!   never auto-accepted)
//! * same host + same key type + same key ⇒ Known
//! * host present only under *other* key types ⇒ Unknown (trust prompt notes
//!   existing entries), so algorithm migrations remain possible
//! * `@revoked` entry matching the presented key ⇒ Revoked (hard refusal)
//!
//! New trust decisions are written **hashed** (privacy: the file does not
//! reveal which hosts you connect to) using a single atomic append.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use russh::keys::ssh_key::HashAlg;
use russh::keys::{PublicKey, ssh_key};

use crate::error::SshError;

/// What a known-hosts evaluation decided about the presented key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evaluation {
    Known {
        fingerprint: String,
    },
    Unknown {
        fingerprint: String,
        key_type: String,
    },
    Changed {
        old: HostKeyEntry,
        new: HostKeyEntry,
    },
    Revoked {
        fingerprint: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostKeyEntry {
    pub key_type: String,
    pub fingerprint: String,
}

impl HostKeyEntry {
    pub fn from_key(key: &PublicKey) -> Self {
        Self {
            key_type: algorithm_name(key),
            fingerprint: fingerprint(key),
        }
    }
}

/// `SHA256:<base64>` fingerprint, identical to `ssh-keygen -lf` output.
pub fn fingerprint(key: &PublicKey) -> String {
    format!("{}", key.fingerprint(HashAlg::Sha256))
}

/// SSH wire name of the key algorithm (e.g. `ssh-ed25519`).
pub fn algorithm_name(key: &PublicKey) -> String {
    key.algorithm().to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum KnownHostsError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("no home directory found; cannot locate known_hosts")]
    NoHome,
}

impl From<KnownHostsError> for SshError {
    fn from(value: KnownHostsError) -> Self {
        SshError::Protocol(value.to_string())
    }
}

/// Handle to a known_hosts file. Reads are performed per evaluation so
/// external edits (e.g. `ssh-keygen -R`) are picked up immediately.
pub struct KnownHosts {
    path: PathBuf,
}

enum LineKind {
    /// Regular entry: positive patterns, optional negations.
    Entry {
        positive: Vec<String>,
        negated: Vec<String>,
    },
    Revoked {
        positive: Vec<String>,
    },
    /// `@cert-authority` (ignored by Termru) or unparseable marker.
    IgnoredMarker,
    CommentOrBlank,
}

struct ParsedLine {
    kind: LineKind,
    key_type: Option<String>,
    key_base64: Option<String>,
}

fn parse_line(raw: &str) -> ParsedLine {
    let trimmed = raw.trim_end_matches(['\n', '\r']);
    if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
        return ParsedLine {
            kind: LineKind::CommentOrBlank,
            key_type: None,
            key_base64: None,
        };
    }
    let mut parts = trimmed.split_whitespace();
    let first = parts.next().unwrap_or("");
    let marker = first.strip_prefix('@');
    let (kind_first, skip_marker) = match marker {
        Some("revoked") => (parts.next().unwrap_or(""), true),
        Some("cert-authority") => {
            return ParsedLine {
                kind: LineKind::IgnoredMarker,
                key_type: None,
                key_base64: None,
            };
        }
        Some(_) => {
            return ParsedLine {
                kind: LineKind::IgnoredMarker,
                key_type: None,
                key_base64: None,
            };
        }
        None => (first, false),
    };
    let _ = skip_marker;
    let key_type = parts.next().map(str::to_string);
    let key_base64 = parts.next().map(str::to_string);

    let (positive, negated) = split_patterns(kind_first);
    let kind = match marker {
        Some("revoked") => LineKind::Revoked { positive },
        _ => LineKind::Entry { positive, negated },
    };
    ParsedLine {
        kind,
        key_type,
        key_base64,
    }
}

fn split_patterns(field: &str) -> (Vec<String>, Vec<String>) {
    let mut positive = Vec::new();
    let mut negated = Vec::new();
    for pat in field.split(',') {
        if pat.is_empty() {
            continue;
        }
        if let Some(rest) = pat.strip_prefix('!') {
            negated.push(rest.to_string());
        } else {
            positive.push(pat.to_string());
        }
    }
    (positive, negated)
}

/// Glob matcher supporting `*` and `?` (OpenSSH hostname wildcards).
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Iterative backtracking matcher.
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Evaluates a single (possibly hashed) host pattern against the target.
fn pattern_matches(pattern: &str, host_port: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix("|1|") {
        let mut it = rest.split('|');
        let (Some(salt_b64), Some(hash_b64), None) = (it.next(), it.next(), it.next()) else {
            return false;
        };
        let (Ok(salt), Ok(expected)) = (base64_decode(salt_b64), base64_decode(hash_b64)) else {
            return false;
        };
        let mac = hmac_sha1(&salt, host_port.as_bytes());
        return mac == expected;
    }
    wildcard_match(pattern, host_port)
}

/// Full OpenSSH matching semantics: at least one positive pattern must match
/// and no negated pattern may match. Returns `None` when the line does not
/// apply (or only negations matched).
fn line_matches_host(positive: &[String], negated: &[String], host_port: &str) -> bool {
    let pos_hit = positive.iter().any(|p| pattern_matches(p, host_port));
    if !pos_hit {
        return false;
    }
    !negated.iter().any(|n| wildcard_match(n, host_port))
}

fn host_port_string(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

// Minimal base64 (standard alphabet, with padding) and HMAC-SHA1 helpers so
// known_hosts handling does not pull in extra dependencies beyond sha1/hmac.
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    if let Ok(bytes) = engine.decode(input) {
        return Ok(bytes);
    }
    // Tolerate unpadded input (some tools omit '=' padding).
    let mut padded = input.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    engine.decode(padded).map_err(|_| ())
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::Mac;
    // HMAC accepts keys of any length; this constructor cannot fail for
    // SHA-1. An empty result is treated as a non-match by callers.
    let Ok(mut mac) = hmac::Hmac::<sha1::Sha1>::new_from_slice(key) else {
        return Vec::new();
    };
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    // Padded, matching what OpenSSH itself writes into hashed known_hosts.
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn random_salt() -> [u8; 16] {
    use rand::RngCore;
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    salt
}

fn hashed_pattern(host_port: &str) -> String {
    let salt = random_salt();
    let mac = hmac_sha1(&salt, host_port.as_bytes());
    format!("|1|{}|{}", base64_encode(&salt), base64_encode(&mac))
}

impl KnownHosts {
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `~/.ssh/known_hosts` — shared with the OpenSSH client on purpose, so
    /// trusting a host in Termru also trusts it for `ssh` and vice versa.
    pub fn default_path() -> Result<PathBuf, KnownHostsError> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| !h.as_os_str().is_empty())
            .or_else(|| {
                // Windows fallback
                std::env::var_os("USERPROFILE").map(PathBuf::from)
            })
            .ok_or(KnownHostsError::NoHome)?;
        Ok(home.join(".ssh").join("known_hosts"))
    }

    pub fn open_default() -> Result<Self, KnownHostsError> {
        Ok(Self::with_path(Self::default_path()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Evaluates the presented key against the file. Never writes.
    pub fn evaluate(&self, host: &str, port: u16, key: &PublicKey) -> Result<Evaluation, SshError> {
        let hp = host_port_string(host, port);
        let presented_type = algorithm_name(key);
        let presented_fp = fingerprint(key);

        let contents = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Evaluation::Unknown {
                    fingerprint: presented_fp,
                    key_type: presented_type,
                });
            }
            Err(e) => {
                return Err(KnownHostsError::Io {
                    path: self.path.clone(),
                    source: e,
                }
                .into());
            }
        };

        let mut same_type_differs = false;
        for raw in contents.lines() {
            let line = parse_line(raw);
            match line.kind {
                LineKind::CommentOrBlank | LineKind::IgnoredMarker => continue,
                LineKind::Revoked { ref positive } => {
                    if !line_matches_host(positive, &[], &hp) {
                        continue;
                    }
                    if let (Some(kt), Some(kb)) = (&line.key_type, &line.key_base64)
                        && kt == &presented_type
                        && let Ok(stored) = russh::keys::parse_public_key_base64(kb)
                        && stored == *key
                    {
                        return Ok(Evaluation::Revoked {
                            fingerprint: presented_fp,
                        });
                    }
                }
                LineKind::Entry {
                    ref positive,
                    ref negated,
                } => {
                    if !line_matches_host(positive, negated, &hp) {
                        continue;
                    }
                    let (Some(kt), Some(kb)) = (&line.key_type, &line.key_base64) else {
                        tracing::warn!("skipping unparseable known_hosts line for {hp}");
                        continue;
                    };
                    if kt != &presented_type {
                        // Host known under a different key type: does not
                        // certify or contradict the presented key.
                        continue;
                    }
                    let Ok(stored) = russh::keys::parse_public_key_base64(kb) else {
                        tracing::warn!("skipping corrupt key material in known_hosts for {hp}");
                        continue;
                    };
                    if stored == *key {
                        return Ok(Evaluation::Known {
                            fingerprint: presented_fp,
                        });
                    }
                    same_type_differs = true;
                }
            }
        }

        if same_type_differs {
            // Report the first stored same-type entry as "old" for the warning.
            for raw in contents.lines() {
                let line = parse_line(raw);
                if let LineKind::Entry {
                    ref positive,
                    ref negated,
                } = line.kind
                    && line_matches_host(positive, negated, &hp)
                    && line.key_type.as_deref() == Some(presented_type.as_str())
                    && let Some(kb) = &line.key_base64
                    && let Ok(stored) = russh::keys::parse_public_key_base64(kb)
                {
                    return Ok(Evaluation::Changed {
                        old: HostKeyEntry::from_key(&stored),
                        new: HostKeyEntry {
                            key_type: presented_type,
                            fingerprint: presented_fp,
                        },
                    });
                }
            }
            return Ok(Evaluation::Changed {
                old: HostKeyEntry {
                    key_type: presented_type.clone(),
                    fingerprint: "unknown (unparseable stored key)".to_string(),
                },
                new: HostKeyEntry {
                    key_type: presented_type,
                    fingerprint: presented_fp,
                },
            });
        }

        Ok(Evaluation::Unknown {
            fingerprint: presented_fp,
            key_type: presented_type,
        })
    }

    /// Records trust for `key`. With `replace_existing`, removes all existing
    /// entries for this host first (the "accept changed key" flow) — the user
    /// is always shown the warning before this is called with `true`.
    pub fn trust(
        &self,
        host: &str,
        port: u16,
        key: &PublicKey,
        replace_existing: bool,
    ) -> Result<(), SshError> {
        let hp = host_port_string(host, port);
        if replace_existing {
            self.remove_entries(&hp)?;
        } else {
            // Refuse to silently add a second same-type key (would mask a
            // changed-key situation for future evaluations).
            if let Evaluation::Known { .. } = self.evaluate(host, port, key)? {
                return Ok(());
            }
            if let Evaluation::Changed { .. } = self.evaluate(host, port, key)? {
                return Err(SshError::Protocol(
                    "host key differs from the stored entry; explicit replacement required"
                        .to_string(),
                ));
            }
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| KnownHostsError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }

        // `to_openssh()` yields "alg base64 [comment]"; known_hosts stores the
        // base64 blob (algorithm + key data) separately from the type column.
        let openssh = key
            .to_openssh()
            .map_err(|e| SshError::Protocol(format!("key encode failed: {e}")))?;
        let key_b64 = openssh
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| SshError::Protocol("public key encoding is malformed".to_string()))?;
        let line = format!(
            "{} {} {}\n",
            hashed_pattern(&hp),
            algorithm_name(key),
            key_b64,
        );

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| KnownHostsError::Io {
                path: self.path.clone(),
                source: e,
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        // Single append write: atomic for O_APPEND on POSIX, so concurrent
        // Termru instances cannot interleave partial lines.
        file.write_all(line.as_bytes())
            .map_err(|e| KnownHostsError::Io {
                path: self.path.clone(),
                source: e,
            })?;
        file.flush().map_err(|e| KnownHostsError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(())
    }

    /// Number of entries removed for this host (used for logging/UX).
    pub fn remove_entries(&self, host_port: &str) -> Result<usize, SshError> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => {
                return Err(KnownHostsError::Io {
                    path: self.path.clone(),
                    source: e,
                }
                .into());
            }
        };
        let mut kept = String::new();
        let mut removed = 0usize;
        for raw in contents.lines() {
            let line = parse_line(raw);
            let matches = match line.kind {
                LineKind::CommentOrBlank | LineKind::IgnoredMarker => false,
                LineKind::Revoked { ref positive } => line_matches_host(positive, &[], host_port),
                LineKind::Entry {
                    ref positive,
                    ref negated,
                } => line_matches_host(positive, negated, host_port),
            };
            if matches {
                removed += 1;
            } else {
                kept.push_str(raw);
                kept.push('\n');
            }
        }
        if removed > 0 {
            self.atomic_write(&kept)?;
        }
        Ok(removed)
    }

    fn atomic_write(&self, contents: &str) -> Result<(), SshError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let tmp = parent.join(format!(".known_hosts.{}.tmp", std::process::id()));
        std::fs::write(&tmp, contents).map_err(|e| KnownHostsError::Io {
            path: tmp.clone(),
            source: e,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| KnownHostsError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(())
    }
}

#[allow(unused_imports)]
use ssh_key as _ssh_key_alias;

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::PrivateKey;

    /// Deterministic xorshift64* RNG implementing rand_core 0.10's
    /// `CryptoRng` (the bound `PrivateKey::random` requires). Test-only:
    /// keys are reproducible from the seed, which keeps failures debuggable.
    struct TestRng(u64);

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }

        fn next_u64_raw(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    impl ssh_key::rand_core::TryCryptoRng for TestRng {}

    impl ssh_key::rand_core::TryRng for TestRng {
        type Error = core::convert::Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(self.next_u64_raw() as u32)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(self.next_u64_raw())
        }

        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            for chunk in dst.chunks_mut(8) {
                let word = self.next_u64_raw().to_le_bytes();
                let n = chunk.len();
                chunk.copy_from_slice(&word[..n]);
            }
            Ok(())
        }
    }

    fn ed25519_key(seed: u8) -> PrivateKey {
        let mut rng = TestRng::new(u64::from(seed));
        PrivateKey::random(&mut rng, ssh_key::Algorithm::Ed25519).expect("test key generation")
    }

    /// Base64 blob column as OpenSSH stores it (second field of the line).
    fn openssh_b64(key: &PublicKey) -> String {
        key.to_openssh()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string()
    }

    fn setup() -> (tempfile::TempDir, KnownHosts) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::with_path(path);
        (dir, kh)
    }

    #[test]
    fn unknown_then_trusted_then_known() {
        let (_dir, kh) = setup();
        let key = ed25519_key(1).public_key().clone();
        let eval = kh.evaluate("example.com", 22, &key).unwrap();
        assert!(matches!(eval, Evaluation::Unknown { .. }));

        kh.trust("example.com", 22, &key, false).unwrap();
        let eval = kh.evaluate("example.com", 22, &key).unwrap();
        assert!(matches!(eval, Evaluation::Known { .. }), "got {eval:?}");

        // Written hashed: the plaintext host must not appear in the file.
        let raw = std::fs::read_to_string(kh.path()).unwrap();
        assert!(!raw.contains("example.com"), "host leaked: {raw}");
        assert!(raw.contains("|1|"), "expected hashed entry: {raw}");
    }

    #[test]
    fn changed_key_is_detected_and_requires_replacement() {
        let (_dir, kh) = setup();
        let old_key = ed25519_key(1).public_key().clone();
        let new_key = ed25519_key(2).public_key().clone();
        kh.trust("example.com", 22, &old_key, false).unwrap();

        let eval = kh.evaluate("example.com", 22, &new_key).unwrap();
        match eval {
            Evaluation::Changed { old, new } => {
                assert_ne!(old.fingerprint, new.fingerprint);
                assert!(new.fingerprint.starts_with("SHA256:"));
            }
            other => panic!("expected Changed, got {other:?}"),
        }

        // Refusing to append a second key silently.
        assert!(kh.trust("example.com", 22, &new_key, false).is_err());
        // Explicit replacement succeeds and old key becomes Unknown/Changed.
        kh.trust("example.com", 22, &new_key, true).unwrap();
        assert!(matches!(
            kh.evaluate("example.com", 22, &new_key).unwrap(),
            Evaluation::Known { .. }
        ));
        let eval = kh.evaluate("example.com", 22, &old_key).unwrap();
        assert!(matches!(eval, Evaluation::Changed { .. }), "got {eval:?}");
    }

    #[test]
    fn port_specific_entries_do_not_collide() {
        let (_dir, kh) = setup();
        let key = ed25519_key(3).public_key().clone();
        kh.trust("example.com", 2222, &key, false).unwrap();
        assert!(matches!(
            kh.evaluate("example.com", 2222, &key).unwrap(),
            Evaluation::Known { .. }
        ));
        // Same host, different port: still unknown.
        assert!(matches!(
            kh.evaluate("example.com", 22, &key).unwrap(),
            Evaluation::Unknown { .. }
        ));
    }

    #[test]
    fn plaintext_openssh_entries_are_honored() {
        let (_dir, kh) = setup();
        let key = ed25519_key(4).public_key().clone();
        let b64 = openssh_b64(&key);
        // OpenSSH writes "<host> <type> <base64> [comment]"
        std::fs::write(
            kh.path(),
            format!("plain.example.com ssh-ed25519 {b64} test-comment\n"),
        )
        .unwrap();
        assert!(matches!(
            kh.evaluate("plain.example.com", 22, &key).unwrap(),
            Evaluation::Known { .. }
        ));
    }

    #[test]
    fn wildcard_and_negation_semantics() {
        let (_dir, kh) = setup();
        let key = ed25519_key(5).public_key().clone();
        let b64 = openssh_b64(&key);
        std::fs::write(
            kh.path(),
            format!("*.example.com,!*dev.example.com ssh-ed25519 {b64}\n"),
        )
        .unwrap();
        assert!(matches!(
            kh.evaluate("www.example.com", 22, &key).unwrap(),
            Evaluation::Known { .. }
        ));
        assert!(matches!(
            kh.evaluate("wwwdev.example.com", 22, &key).unwrap(),
            Evaluation::Unknown { .. }
        ));
    }

    #[test]
    fn revoked_entries_are_hard_refusals() {
        let (_dir, kh) = setup();
        let key = ed25519_key(6).public_key().clone();
        let b64 = openssh_b64(&key);
        std::fs::write(
            kh.path(),
            format!("@revoked revoked.example.com ssh-ed25519 {b64}\n"),
        )
        .unwrap();
        assert!(matches!(
            kh.evaluate("revoked.example.com", 22, &key).unwrap(),
            Evaluation::Revoked { .. }
        ));
    }

    #[test]
    fn remove_entries_only_touches_matching_host() {
        let (_dir, kh) = setup();
        let key = ed25519_key(7).public_key().clone();
        kh.trust("one.example.com", 22, &key, false).unwrap();
        kh.trust("two.example.com", 22, &key, false).unwrap();
        let hp = host_port_string("one.example.com", 22);
        let removed = kh.remove_entries(&hp).unwrap();
        assert_eq!(removed, 1);
        assert!(matches!(
            kh.evaluate("two.example.com", 22, &key).unwrap(),
            Evaluation::Known { .. }
        ));
        assert!(matches!(
            kh.evaluate("one.example.com", 22, &key).unwrap(),
            Evaluation::Unknown { .. }
        ));
    }
}
