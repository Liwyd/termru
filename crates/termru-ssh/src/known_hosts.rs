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
use russh::keys::{Algorithm, PublicKey, ssh_key};

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

/// The host-key algorithms to offer in the handshake: russh's defaults, with
/// the families this host is already known under (`known_types`, as in
/// `known_hosts`: `ssh-ed25519`, `ssh-rsa`, ...) moved to the front. A server
/// that owns several host keys signs with the first type both sides support,
/// so this makes it present one we can verify instead of its own favourite.
/// Nothing is removed: a server that lacks the known type still connects (and
/// is then asked about, see `HostKeyPromptRequest::Unknown`).
pub(crate) fn preferred_host_key_algorithms(known_types: &[String]) -> Vec<Algorithm> {
    let family = |algorithm: &Algorithm| match algorithm {
        // `rsa-sha2-256`, `rsa-sha2-512` and `ssh-rsa` sign with the same
        // key, which known_hosts stores as `ssh-rsa`.
        Algorithm::Rsa { .. } => "ssh-rsa".to_string(),
        other => other.to_string(),
    };
    let (mut first, rest): (Vec<Algorithm>, Vec<Algorithm>) = russh::Preferred::DEFAULT
        .key
        .iter()
        .cloned()
        .partition(|algorithm| known_types.contains(&family(algorithm)));
    first.extend(rest);
    first
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
    /// Extra files that are only ever read (the user's `~/.ssh/known_hosts`).
    readonly: Vec<PathBuf>,
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

/// The verdict over the writable file (`own`) and the read-only ones
/// (`theirs`): see [`KnownHosts::with_readonly`] for the rules.
fn combine(own: Evaluation, theirs: Vec<Evaluation>) -> Evaluation {
    if matches!(own, Evaluation::Revoked { .. }) {
        return own;
    }
    if let Some(revoked) = theirs
        .iter()
        .find(|evaluation| matches!(evaluation, Evaluation::Revoked { .. }))
    {
        return revoked.clone();
    }
    if !matches!(own, Evaluation::Unknown { .. }) {
        return own;
    }
    if let Some(known) = theirs
        .iter()
        .find(|evaluation| matches!(evaluation, Evaluation::Known { .. }))
    {
        return known.clone();
    }
    if let Some(changed) = theirs
        .iter()
        .find(|evaluation| matches!(evaluation, Evaluation::Changed { .. }))
    {
        return changed.clone();
    }
    own
}

impl KnownHosts {
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            readonly: Vec::new(),
        }
    }

    /// Adds known_hosts files that are consulted but **never written**.
    ///
    /// How they combine with the writable file (see [`Evaluation`]):
    /// * `@revoked` in any file refuses the key;
    /// * what the writable file says wins once it says anything: it holds
    ///   what the user decided in this app, so a key it knows is known and a
    ///   key that contradicts it is a changed key, whatever the read-only
    ///   files hold;
    /// * a host the writable file has no entry for is judged by the
    ///   read-only files: the same key is trusted without a prompt, a
    ///   different key of the same type is a changed key (the prompt's
    ///   "accept" records the new key in the writable file only);
    /// * a read-only file that cannot be read is skipped: it is a bonus, not
    ///   a requirement.
    #[must_use]
    pub fn with_readonly(mut self, paths: Vec<PathBuf>) -> Self {
        self.readonly = paths;
        self
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

    /// Evaluates the presented key against the writable file and the
    /// read-only ones (see [`KnownHosts::with_readonly`]). Never writes.
    pub fn evaluate(&self, host: &str, port: u16, key: &PublicKey) -> Result<Evaluation, SshError> {
        let own = self.evaluate_file(&self.path, host, port, key)?;
        if self.readonly.is_empty() {
            return Ok(own);
        }
        let theirs: Vec<Evaluation> = self
            .readonly
            .iter()
            .filter_map(
                |path| match self.evaluate_file(path, host, port, key) {
                    Ok(evaluation) => Some(evaluation),
                    Err(error) => {
                        tracing::warn!(path = %path.display(), %error, "skipping an unreadable known_hosts file");
                        None
                    }
                },
            )
            .collect();
        Ok(combine(own, theirs))
    }

    /// Evaluates against one file. Never writes.
    fn evaluate_file(
        &self,
        path: &Path,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<Evaluation, SshError> {
        let hp = host_port_string(host, port);
        let presented_type = algorithm_name(key);
        let presented_fp = fingerprint(key);

        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Evaluation::Unknown {
                    fingerprint: presented_fp,
                    key_type: presented_type,
                });
            }
            Err(e) => {
                return Err(KnownHostsError::Io {
                    path: path.to_path_buf(),
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

    /// The key types this host is already known under (`ssh-ed25519`, ...),
    /// in file order and without duplicates. `@revoked` and
    /// `@cert-authority` lines do not count: they do not vouch for a key.
    ///
    /// Used for two things: the trust prompt says so when the server presents
    /// a type we have not seen (see `HostKeyPromptRequest::Unknown`), and the
    /// handshake asks the server for these types first, so a server that has
    /// several host keys presents one we can actually verify.
    pub fn known_key_types(&self, host: &str, port: u16) -> Result<Vec<String>, SshError> {
        let mut types = self.key_types_in(&self.path, host, port)?;
        for path in &self.readonly {
            match self.key_types_in(path, host, port) {
                Ok(more) => {
                    for key_type in more {
                        if !types.contains(&key_type) {
                            types.push(key_type);
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "skipping an unreadable known_hosts file");
                }
            }
        }
        Ok(types)
    }

    fn key_types_in(&self, path: &Path, host: &str, port: u16) -> Result<Vec<String>, SshError> {
        let hp = host_port_string(host, port);
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(KnownHostsError::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
                .into());
            }
        };
        let mut types: Vec<String> = Vec::new();
        for raw in contents.lines() {
            let line = parse_line(raw);
            let LineKind::Entry {
                ref positive,
                ref negated,
            } = line.kind
            else {
                continue;
            };
            if !line_matches_host(positive, negated, &hp) {
                continue;
            }
            if let Some(key_type) = line.key_type
                && !types.contains(&key_type)
            {
                types.push(key_type);
            }
        }
        Ok(types)
    }

    /// Records trust for `key`. With `replace_existing` (the "accept changed
    /// key" flow — the user is always shown the warning first) the stale
    /// entries are removed **narrowly**: only lines that name exactly this
    /// host and carry the *same key type* as `key`. Other key types of the
    /// host, `@revoked`/`@cert-authority` lines, and wildcard or negated
    /// lines (which cover other hosts too) are never touched; a line listing
    /// several hosts only loses this host.
    pub fn trust(
        &self,
        host: &str,
        port: u16,
        key: &PublicKey,
        replace_existing: bool,
    ) -> Result<(), SshError> {
        let hp = host_port_string(host, port);
        if replace_existing {
            self.rewrite_without_host(&hp, Some(&algorithm_name(key)))?;
        } else {
            // Refuse to silently add a second same-type key (would mask a
            // changed-key situation for future evaluations).
            // The writable file only: a key the read-only files vouch for is
            // still written here when the user chose to trust it.
            let current = self.evaluate_file(&self.path, host, port, key)?;
            if let Evaluation::Known { .. } = current {
                return Ok(());
            }
            if let Evaluation::Changed { .. } = current {
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
        let mut line = format!(
            "{} {} {}\n",
            hashed_pattern(&hp),
            algorithm_name(key),
            key_b64,
        );
        if self.lacks_trailing_newline() {
            // A hand-edited file may end without a newline; appending would
            // glue the new entry onto the last line and corrupt both.
            line.insert(0, '\n');
        }

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

    /// Removes every entry that names exactly this host (any key type) and
    /// returns how many lines were removed or rewritten.
    ///
    /// Like `ssh-keygen -R`, this never touches `@revoked` or
    /// `@cert-authority` lines, nor wildcard/negated patterns (they cover
    /// other hosts); a multi-host line only loses this host.
    pub fn remove_entries(&self, host_port: &str) -> Result<usize, SshError> {
        self.rewrite_without_host(host_port, None)
    }

    /// Rewrites the file without the entries that name exactly `host_port`
    /// (restricted to `key_type` when given). Returns the number of lines
    /// removed or rewritten.
    fn rewrite_without_host(
        &self,
        host_port: &str,
        key_type: Option<&str>,
    ) -> Result<usize, SshError> {
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
        let mut out = String::with_capacity(contents.len());
        let mut changed = 0usize;
        // `split_inclusive` keeps every untouched line byte-for-byte,
        // including its line ending and a missing final newline.
        for raw in contents.split_inclusive('\n') {
            match strip_host_from_line(raw, host_port, key_type) {
                LineEdit::Keep => out.push_str(raw),
                LineEdit::Drop => changed += 1,
                LineEdit::Rewrite(line) => {
                    out.push_str(&line);
                    changed += 1;
                }
            }
        }
        if changed > 0 {
            self.atomic_write(&out)?;
        }
        Ok(changed)
    }

    /// `true` when the file exists, is non-empty and does not end in `\n`.
    fn lacks_trailing_newline(&self) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return false;
        };
        let Ok(len) = file.seek(SeekFrom::End(0)) else {
            return false;
        };
        if len == 0 || file.seek(SeekFrom::Start(len - 1)).is_err() {
            return false;
        }
        let mut last = [0u8; 1];
        file.read_exact(&mut last).is_ok() && last[0] != b'\n'
    }

    /// Replaces the file atomically (write a sibling temp file, then rename).
    ///
    /// * the temp name is unique per call (process id + counter, created with
    ///   `create_new`), so concurrent writers — threads of one process or two
    ///   Termru instances — never clobber each other's temp file;
    /// * the permissions of the file being replaced are preserved (a user who
    ///   made it group-readable keeps it so); a new file starts at `0600`.
    fn atomic_write(&self, contents: &str) -> Result<(), SshError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let tmp = parent.join(format!(
            ".known_hosts.{}.{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let original_permissions = std::fs::metadata(&self.path).ok().map(|m| m.permissions());

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = options.open(&tmp).and_then(|mut file| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        });
        if let Err(source) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(KnownHostsError::Io { path: tmp, source }.into());
        }
        if let Some(permissions) = original_permissions {
            let _ = std::fs::set_permissions(&tmp, permissions);
        }
        if let Err(source) = std::fs::rename(&tmp, &self.path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(KnownHostsError::Io {
                path: self.path.clone(),
                source,
            }
            .into());
        }
        Ok(())
    }
}

/// What to do with one line of the file when forgetting a host.
enum LineEdit {
    Keep,
    Drop,
    Rewrite(String),
}

/// Decides what forgetting `host_port` (optionally only for `key_type`) means
/// for one raw line. Conservative on purpose: anything that is not an
/// unambiguous plain entry for exactly this host is kept.
fn strip_host_from_line(raw: &str, host_port: &str, key_type: Option<&str>) -> LineEdit {
    let body = raw.trim_end_matches(['\n', '\r']);
    let eol = &raw[body.len()..];
    let trimmed = body.trim_start();
    // Comments, blanks, and `@revoked` / `@cert-authority` markers.
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
        return LineEdit::Keep;
    }
    let indent = &body[..body.len() - trimmed.len()];
    let Some(split_at) = trimmed.find(char::is_whitespace) else {
        return LineEdit::Keep;
    };
    let (patterns, rest) = trimmed.split_at(split_at);
    let mut fields = rest.split_whitespace();
    let (Some(line_type), Some(_key)) = (fields.next(), fields.next()) else {
        return LineEdit::Keep;
    };
    if key_type.is_some_and(|wanted| wanted != line_type) {
        return LineEdit::Keep;
    }

    let tokens: Vec<&str> = patterns.split(',').collect();
    if tokens.iter().any(|token| token.starts_with('!')) {
        return LineEdit::Keep;
    }
    let mut remaining: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut named_here = false;
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        let wildcard = !token.starts_with("|1|") && token.contains(['*', '?']);
        if pattern_matches(token, host_port) {
            if wildcard {
                // Covers other hosts as well: not ours to delete.
                return LineEdit::Keep;
            }
            // Plain or hashed pattern that identifies exactly this host.
            named_here = true;
        } else {
            remaining.push(token);
        }
    }
    if !named_here {
        return LineEdit::Keep;
    }
    if remaining.is_empty() {
        LineEdit::Drop
    } else {
        LineEdit::Rewrite(format!("{indent}{}{rest}{eol}", remaining.join(",")))
    }
}

#[allow(unused_imports)]
use ssh_key as _ssh_key_alias;

/// Deterministic test keys, shared by the unit tests of this crate.
#[cfg(test)]
pub(crate) mod tests_support {
    use russh::keys::{PrivateKey, PublicKey, ssh_key};

    /// Deterministic xorshift64* RNG implementing rand_core 0.10's
    /// `CryptoRng` (the bound `PrivateKey::random` requires). Test-only:
    /// keys are reproducible from the seed, which keeps failures debuggable.
    pub(crate) struct TestRng(pub(crate) u64);

    impl TestRng {
        pub(crate) fn new(seed: u64) -> Self {
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

    pub(crate) fn ed25519_key(seed: u8) -> PrivateKey {
        let mut rng = TestRng::new(u64::from(seed));
        PrivateKey::random(&mut rng, ssh_key::Algorithm::Ed25519).expect("test key generation")
    }

    pub(crate) fn ed25519_public_key(seed: u8) -> PublicKey {
        ed25519_key(seed).public_key().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::ed25519_key;
    use super::*;

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

    // ---- replacement scope (regression tests for the over-eager removal) ----

    fn write_file(kh: &KnownHosts, contents: &str) {
        std::fs::write(kh.path(), contents).unwrap();
    }

    fn read_file(kh: &KnownHosts) -> String {
        std::fs::read_to_string(kh.path()).unwrap()
    }

    #[test]
    fn the_key_types_known_for_a_host_come_from_its_own_entries_only() {
        let (_dir, kh) = setup();
        write_file(
            &kh,
            "# a comment\n\
             other.test ssh-rsa AAAAB3NzaC1yc2E\n\
             host.test,alias.test ssh-rsa AAAAB3NzaC1yc2E\n\
             [host.test]:2222 ecdsa-sha2-nistp256 AAAAE2VjZHNh\n\
             *.wild.test ssh-dss AAAAB3NzaC1kc3M\n\
             @revoked host.test ssh-dss AAAAB3NzaC1kc3M\n\
             @cert-authority host.test ssh-ed25519 AAAAC3NzaC1lZDI1NTE5\n",
        );
        assert_eq!(kh.known_key_types("host.test", 22).unwrap(), ["ssh-rsa"]);
        assert_eq!(
            kh.known_key_types("host.test", 2222).unwrap(),
            ["ecdsa-sha2-nistp256"],
            "the port is part of the identity"
        );
        assert_eq!(kh.known_key_types("x.wild.test", 22).unwrap(), ["ssh-dss"]);
        assert!(kh.known_key_types("unknown.test", 22).unwrap().is_empty());

        // A hashed entry written by `trust` counts too, once per type, in
        // file order.
        let key = ed25519_key(3).public_key().clone();
        kh.trust("host.test", 22, &key, false).unwrap();
        assert_eq!(
            kh.known_key_types("host.test", 22).unwrap(),
            ["ssh-rsa", "ssh-ed25519"]
        );
    }

    #[test]
    fn a_missing_file_knows_no_key_types() {
        let (_dir, kh) = setup();
        assert!(kh.known_key_types("host.test", 22).unwrap().is_empty());
    }

    #[test]
    fn known_key_types_move_to_the_front_of_the_handshake_offer() {
        let default: Vec<Algorithm> = russh::Preferred::DEFAULT.key.to_vec();
        assert_eq!(preferred_host_key_algorithms(&[]), default);
        assert_eq!(
            preferred_host_key_algorithms(&["not-a-real-type".to_string()]),
            default,
            "an unknown type changes nothing"
        );

        let ecdsa = preferred_host_key_algorithms(&["ecdsa-sha2-nistp384".to_string()]);
        assert_eq!(ecdsa.len(), default.len(), "nothing is removed");
        assert_eq!(ecdsa[0].to_string(), "ecdsa-sha2-nistp384");
        assert_eq!(ecdsa[1], Algorithm::Ed25519, "the rest keeps its order");

        // `ssh-rsa` in known_hosts covers every RSA signature algorithm.
        let rsa = preferred_host_key_algorithms(&["ssh-rsa".to_string()]);
        assert!(
            rsa[..3]
                .iter()
                .all(|algorithm| matches!(algorithm, Algorithm::Rsa { .. })),
            "{rsa:?}"
        );
        assert_eq!(rsa[3], Algorithm::Ed25519);
    }

    #[test]
    fn replacing_a_key_keeps_the_hosts_other_key_types() {
        let (_dir, kh) = setup();
        let old = ed25519_key(10).public_key().clone();
        let new = ed25519_key(11).public_key().clone();
        // The host also has an RSA entry; only the ed25519 one may go.
        let rsa_line = "host.example.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDfake rsa-comment\n";
        write_file(
            &kh,
            &format!(
                "{rsa_line}host.example.com ssh-ed25519 {}\n",
                openssh_b64(&old)
            ),
        );
        kh.trust("host.example.com", 22, &new, true).unwrap();
        let contents = read_file(&kh);
        assert!(contents.contains(rsa_line), "RSA entry lost: {contents}");
        assert!(
            !contents.contains(&openssh_b64(&old)),
            "stale ed25519 entry survived: {contents}"
        );
        assert!(matches!(
            kh.evaluate("host.example.com", 22, &new).unwrap(),
            Evaluation::Known { .. }
        ));
    }

    #[test]
    fn replacing_a_key_never_touches_revoked_or_cert_authority_lines() {
        let (_dir, kh) = setup();
        let old = ed25519_key(12).public_key().clone();
        let revoked = ed25519_key(13).public_key().clone();
        let new = ed25519_key(14).public_key().clone();
        let revoked_line = format!(
            "@revoked host.example.com ssh-ed25519 {}\n",
            openssh_b64(&revoked)
        );
        let ca_line =
            "@cert-authority host.example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIfake\n";
        write_file(
            &kh,
            &format!(
                "{revoked_line}{ca_line}host.example.com ssh-ed25519 {}\n",
                openssh_b64(&old)
            ),
        );
        kh.trust("host.example.com", 22, &new, true).unwrap();
        let contents = read_file(&kh);
        assert!(
            contents.contains(&revoked_line),
            "@revoked line lost: {contents}"
        );
        assert!(
            contents.contains(ca_line),
            "@cert-authority line lost: {contents}"
        );
        assert!(
            matches!(
                kh.evaluate("host.example.com", 22, &revoked).unwrap(),
                Evaluation::Revoked { .. }
            ),
            "the revoked key must stay refused"
        );
    }

    #[test]
    fn replacing_a_key_leaves_wildcard_and_negated_lines_alone() {
        let (_dir, kh) = setup();
        let shared = ed25519_key(15).public_key().clone();
        let new = ed25519_key(16).public_key().clone();
        let wildcard = format!("*.example.com ssh-ed25519 {}\n", openssh_b64(&shared));
        let negated = format!(
            "www.example.com,!staging.example.com ssh-ed25519 {}\n",
            openssh_b64(&shared)
        );
        write_file(&kh, &format!("{wildcard}{negated}"));
        kh.trust("www.example.com", 22, &new, true).unwrap();
        let contents = read_file(&kh);
        assert!(
            contents.contains(&wildcard),
            "wildcard line lost: {contents}"
        );
        assert!(contents.contains(&negated), "negated line lost: {contents}");
        // Other hosts under the wildcard are unaffected.
        assert!(matches!(
            kh.evaluate("db.example.com", 22, &shared).unwrap(),
            Evaluation::Known { .. }
        ));
        // The new key is trusted for this host, whatever the wildcard says.
        assert!(matches!(
            kh.evaluate("www.example.com", 22, &new).unwrap(),
            Evaluation::Known { .. }
        ));
    }

    #[test]
    fn replacing_a_key_only_removes_this_host_from_a_multi_host_line() {
        let (_dir, kh) = setup();
        let old = ed25519_key(17).public_key().clone();
        let new = ed25519_key(18).public_key().clone();
        write_file(
            &kh,
            &format!(
                "  a.example.com,b.example.com,[a.example.com]:2222 ssh-ed25519 {} my comment\n",
                openssh_b64(&old)
            ),
        );
        kh.trust("a.example.com", 22, &new, true).unwrap();
        let contents = read_file(&kh);
        assert!(
            contents.starts_with(&format!(
                "  b.example.com,[a.example.com]:2222 ssh-ed25519 {} my comment\n",
                openssh_b64(&old)
            )),
            "only the host itself leaves the list (indent/comment kept): {contents}"
        );
        // The sibling host keeps its old key; this host has the new one.
        assert!(matches!(
            kh.evaluate("b.example.com", 22, &old).unwrap(),
            Evaluation::Known { .. }
        ));
        assert!(matches!(
            kh.evaluate("a.example.com", 22, &new).unwrap(),
            Evaluation::Known { .. }
        ));
        // The same host on another port is a different entry and survives.
        assert!(matches!(
            kh.evaluate("a.example.com", 2222, &old).unwrap(),
            Evaluation::Known { .. }
        ));
    }

    #[test]
    fn replacing_a_key_removes_matching_hashed_lines_of_the_same_type_only() {
        let (_dir, kh) = setup();
        let old = ed25519_key(19).public_key().clone();
        let other_host_key = ed25519_key(20).public_key().clone();
        let new = ed25519_key(21).public_key().clone();
        kh.trust("target.example.com", 22, &old, false).unwrap();
        kh.trust("bystander.example.com", 22, &other_host_key, false)
            .unwrap();
        kh.trust("target.example.com", 22, &new, true).unwrap();
        assert!(matches!(
            kh.evaluate("target.example.com", 22, &new).unwrap(),
            Evaluation::Known { .. }
        ));
        assert!(
            matches!(
                kh.evaluate("target.example.com", 22, &old).unwrap(),
                Evaluation::Changed { .. }
            ),
            "the stale hashed entry must be gone"
        );
        assert!(matches!(
            kh.evaluate("bystander.example.com", 22, &other_host_key)
                .unwrap(),
            Evaluation::Known { .. }
        ));
        assert_eq!(read_file(&kh).lines().count(), 2);
    }

    #[test]
    fn comments_blank_lines_and_crlf_survive_a_replacement() {
        let (_dir, kh) = setup();
        let old = ed25519_key(22).public_key().clone();
        let other = ed25519_key(23).public_key().clone();
        let new = ed25519_key(24).public_key().clone();
        let keep = format!(
            "# my hosts\r\n\r\nkeep.example.com ssh-ed25519 {}\r\n",
            openssh_b64(&other)
        );
        write_file(
            &kh,
            &format!("{keep}gone.example.com ssh-ed25519 {}", openssh_b64(&old)),
        );
        kh.trust("gone.example.com", 22, &new, true).unwrap();
        assert!(read_file(&kh).starts_with(&keep));
    }

    /// Regression test: appending to a file without a final newline used to
    /// glue the new entry onto the last line.
    #[test]
    fn appending_to_a_file_without_trailing_newline_starts_a_new_line() {
        let (_dir, kh) = setup();
        let first = ed25519_key(25).public_key().clone();
        let second = ed25519_key(26).public_key().clone();
        write_file(
            &kh,
            &format!("one.example.com ssh-ed25519 {}", openssh_b64(&first)),
        );
        kh.trust("two.example.com", 22, &second, false).unwrap();
        assert!(matches!(
            kh.evaluate("one.example.com", 22, &first).unwrap(),
            Evaluation::Known { .. }
        ));
        assert!(matches!(
            kh.evaluate("two.example.com", 22, &second).unwrap(),
            Evaluation::Known { .. }
        ));
        assert_eq!(read_file(&kh).lines().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_the_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, kh) = setup();
        let old = ed25519_key(27).public_key().clone();
        let new = ed25519_key(28).public_key().clone();
        kh.trust("perm.example.com", 22, &old, false).unwrap();
        std::fs::set_permissions(kh.path(), std::fs::Permissions::from_mode(0o640)).unwrap();
        // Rewriting via the temp file must not reset the mode to 0600.
        kh.remove_entries("perm.example.com").unwrap();
        let mode = std::fs::metadata(kh.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640, "mode after rewrite: {mode:o}");
        kh.trust("perm.example.com", 22, &new, false).unwrap();
    }

    /// Regression test: the temp file used to be named after the process id
    /// only, so two concurrent rewrites raced on the same path.
    #[test]
    fn concurrent_rewrites_use_distinct_temp_files() {
        let (dir, kh) = setup();
        let kh = std::sync::Arc::new(kh);
        write_file(&kh, "seed ssh-ed25519 AAAA\n");
        let mut workers = Vec::new();
        for worker in 0..8 {
            let kh = kh.clone();
            workers.push(std::thread::spawn(move || {
                for round in 0..25 {
                    kh.atomic_write(&format!("w{worker}-{round} ssh-ed25519 AAAA\n"))
                        .expect("atomic write");
                }
            }));
        }
        for worker in workers {
            worker.join().expect("worker finished");
        }
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        assert!(read_file(&kh).starts_with('w'));
    }

    // ---- read-only extra files (the user's ~/.ssh/known_hosts) ----

    /// A system known_hosts file with the given (host-pattern, key) lines.
    fn system_file(dir: &Path, name: &str, lines: &[(&str, &str, &PublicKey)]) -> PathBuf {
        let path = dir.join(name);
        let mut text = String::new();
        for (marker, host, key) in lines {
            if !marker.is_empty() {
                text.push_str(marker);
                text.push(' ');
            }
            text.push_str(&format!(
                "{host} {} {}\n",
                algorithm_name(key),
                openssh_b64(key)
            ));
        }
        std::fs::write(&path, text).unwrap();
        path
    }

    fn with_system(dir: &tempfile::TempDir, system: Vec<PathBuf>) -> KnownHosts {
        KnownHosts::with_path(dir.path().join("known_hosts")).with_readonly(system)
    }

    #[test]
    fn a_key_only_the_system_file_knows_is_trusted_without_a_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let key = ed25519_key(1).public_key().clone();
        let system = system_file(dir.path(), "system", &[("", "example.com", &key)]);
        let kh = with_system(&dir, vec![system]);
        assert!(matches!(
            kh.evaluate("example.com", 22, &key).unwrap(),
            Evaluation::Known { .. }
        ));
        // Other hosts are still unknown.
        assert!(matches!(
            kh.evaluate("other.example.com", 22, &key).unwrap(),
            Evaluation::Unknown { .. }
        ));
        // Nothing was written to the writable file by looking.
        assert!(!kh.path().exists());
    }

    #[test]
    fn a_different_key_in_the_system_file_is_a_changed_key() {
        let dir = tempfile::tempdir().unwrap();
        let system_key = ed25519_key(1).public_key().clone();
        let presented = ed25519_key(2).public_key().clone();
        let system = system_file(dir.path(), "system", &[("", "example.com", &system_key)]);
        let kh = with_system(&dir, vec![system.clone()]);
        let Evaluation::Changed { old, new } = kh.evaluate("example.com", 22, &presented).unwrap()
        else {
            panic!("a contradicting system entry must block");
        };
        assert_eq!(old.fingerprint, fingerprint(&system_key));
        assert_eq!(new.fingerprint, fingerprint(&presented));

        // Accepting the warning records the new key in the writable file
        // only; the system file is not touched.
        let before = std::fs::read(&system).unwrap();
        kh.trust("example.com", 22, &presented, true).unwrap();
        assert_eq!(std::fs::read(&system).unwrap(), before);
        assert!(matches!(
            kh.evaluate("example.com", 22, &presented).unwrap(),
            Evaluation::Known { .. }
        ));
    }

    /// What the user decided in this app wins over a stale system entry, and
    /// a key that contradicts the app's own entry still blocks even when the
    /// system file knows it.
    #[test]
    fn the_writable_file_wins_once_it_has_an_entry() {
        let dir = tempfile::tempdir().unwrap();
        let old = ed25519_key(1).public_key().clone();
        let new = ed25519_key(2).public_key().clone();
        let system = system_file(dir.path(), "system", &[("", "example.com", &old)]);
        let kh = with_system(&dir, vec![system]);
        kh.trust("example.com", 22, &new, true).unwrap();
        assert!(matches!(
            kh.evaluate("example.com", 22, &new).unwrap(),
            Evaluation::Known { .. }
        ));
        // The system file vouches for `old`, but the app pinned `new`.
        assert!(matches!(
            kh.evaluate("example.com", 22, &old).unwrap(),
            Evaluation::Changed { .. }
        ));
    }

    #[test]
    fn a_revoked_key_in_the_system_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let key = ed25519_key(1).public_key().clone();
        let system = system_file(dir.path(), "system", &[("@revoked", "example.com", &key)]);
        let kh = with_system(&dir, vec![system]);
        assert!(matches!(
            kh.evaluate("example.com", 22, &key).unwrap(),
            Evaluation::Revoked { .. }
        ));
        // Even after the app trusted it: revocation is final.
        let (_other, own) = setup();
        own.trust("example.com", 22, &key, false).unwrap();
        let both = KnownHosts::with_path(own.path()).with_readonly(vec![dir.path().join("system")]);
        assert!(matches!(
            both.evaluate("example.com", 22, &key).unwrap(),
            Evaluation::Revoked { .. }
        ));
    }

    #[test]
    fn system_files_use_the_same_host_syntax() {
        let dir = tempfile::tempdir().unwrap();
        let key = ed25519_key(1).public_key().clone();
        let system = system_file(
            dir.path(),
            "system",
            &[
                ("", "[example.com]:2222", &key),
                ("", "*.corp.example", &key),
                ("", "fe80::1", &key),
            ],
        );
        let kh = with_system(&dir, vec![system]);
        for (host, port) in [
            ("example.com", 2222),
            ("db.corp.example", 22),
            ("fe80::1", 22),
        ] {
            assert!(
                matches!(
                    kh.evaluate(host, port, &key).unwrap(),
                    Evaluation::Known { .. }
                ),
                "{host}:{port}"
            );
        }
        assert!(matches!(
            kh.evaluate("example.com", 22, &key).unwrap(),
            Evaluation::Unknown { .. }
        ));
    }

    #[test]
    fn a_missing_or_unreadable_system_file_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let key = ed25519_key(1).public_key().clone();
        // Missing, and a directory (cannot be read as a file).
        let kh = with_system(
            &dir,
            vec![dir.path().join("absent"), dir.path().to_path_buf()],
        );
        assert!(matches!(
            kh.evaluate("example.com", 22, &key).unwrap(),
            Evaluation::Unknown { .. }
        ));
        assert!(kh.known_key_types("example.com", 22).unwrap().is_empty());
    }

    #[test]
    fn known_key_types_include_the_system_files() {
        let dir = tempfile::tempdir().unwrap();
        let key = ed25519_key(1).public_key().clone();
        let system = system_file(dir.path(), "system", &[("", "example.com", &key)]);
        let kh = with_system(&dir, vec![system]);
        assert_eq!(
            kh.known_key_types("example.com", 22).unwrap(),
            vec!["ssh-ed25519".to_string()]
        );
    }
}
