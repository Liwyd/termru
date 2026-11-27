//! Authentication: an ordered chain of methods, driven by what the server
//! allows.
//!
//! ## Flow
//!
//! 1. A `none` request is sent first. Some servers accept it (guest and
//!    appliance logins), and every server answers with the list of methods it
//!    allows.
//! 2. The configured methods ([`ConnectParams::auth`]) are then tried in
//!    order, **skipping the ones the server does not allow** (offering a
//!    method the server did not list only burns one of its `MaxAuthTries`).
//!    After every rejection the server's list is refreshed.
//! 3. A *partial success* (the server accepted the step but wants another
//!    one, e.g. `publickey,keyboard-interactive`) completes that method and
//!    carries on with the remaining ones — also with methods that had been
//!    skipped earlier because the server only allowed them from now on.
//! 4. A method that cannot be used locally (missing key file, no agent, a
//!    passphrase nobody can supply) is recorded and the chain continues. When
//!    nothing worked the error lists what was tried, why each method did not
//!    work and what the server allows. A **cancelled** question, in contrast,
//!    ends the whole authentication: the user said no.
//!
//! Secrets never appear in the error: it names methods, not credentials.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse, Prompt};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{Certificate, HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{MethodKind, MethodSet};
use tokio::sync::mpsc;

use crate::config::{
    AuthChoice, AuthPrompt, AuthPromptField, AuthPromptKind, AuthPromptRequest, AuthPromptResponse,
    KeyIdentity, MAX_AGENT_KEYS, SecretValue,
};
use crate::error::SshError;
use crate::known_hosts::fingerprint;
use crate::session::ClientHandler;

/// How often a wrong key passphrase may be re-asked before giving up (the
/// same as `ssh`).
const MAX_PASSPHRASE_ATTEMPTS: usize = 3;
/// Keyboard-interactive rounds one attempt may take. Real servers need one
/// to three; the bound stops a server that asks forever.
const MAX_KBD_ROUNDS: usize = 8;

/// Everything the authentication needs besides the connection itself.
pub(crate) struct AuthContext<'a> {
    pub username: &'a str,
    pub methods: &'a [AuthChoice],
    pub prompt: &'a dyn AuthPrompt,
    /// Raised while a human answers, so the authentication deadline (and the
    /// connect budget) stand still.
    pub prompt_active: &'a AtomicBool,
    /// Longest a single question may wait for the user.
    pub prompt_timeout: Duration,
    /// Text the server showed in a round that asked nothing (RFC 4256
    /// allows it); forwarded to the user like the banner.
    pub notices: &'a mpsc::UnboundedSender<String>,
}

/// What one method amounted to, when it did not fail locally.
#[derive(Debug)]
enum MethodResult {
    Success,
    Rejected {
        remaining: MethodSet,
        /// The server accepted this step and wants another method too.
        partial_success: bool,
        /// More specific than "rejected", e.g. how many agent keys were
        /// offered.
        note: Option<String>,
    },
}

impl From<AuthResult> for MethodResult {
    fn from(result: AuthResult) -> Self {
        match result {
            AuthResult::Success => Self::Success,
            AuthResult::Failure {
                remaining_methods,
                partial_success,
            } => Self::Rejected {
                remaining: remaining_methods,
                partial_success,
                note: None,
            },
        }
    }
}

fn kind_of(method: &AuthChoice) -> MethodKind {
    match method {
        AuthChoice::Password { .. } => MethodKind::Password,
        AuthChoice::PrivateKey { .. } | AuthChoice::Agent { .. } => MethodKind::PublicKey,
        AuthChoice::KeyboardInteractive { .. } => MethodKind::KeyboardInteractive,
    }
}

fn kind_name(kind: MethodKind) -> &'static str {
    <&'static str>::from(&kind)
}

/// `true` when the server's list does not rule the method out. An unknown
/// list (`None`) rules nothing out.
fn is_allowed(allowed: Option<&MethodSet>, method: &AuthChoice) -> bool {
    allowed.is_none_or(|set| set.contains(&kind_of(method)))
}

fn describe_set(set: Option<&MethodSet>) -> String {
    match set {
        None => "unknown".to_string(),
        Some(set) if set.is_empty() => "nothing".to_string(),
        Some(set) => set
            .iter()
            .map(|kind| kind_name(*kind))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Maps a russh failure on the handle (the session task is gone) to the
/// error the chain understands.
fn transport_error(error: &russh::Error, what: &str) -> SshError {
    match error {
        russh::Error::SendError
        | russh::Error::RecvError
        | russh::Error::Disconnect
        | russh::Error::HUP => SshError::ConnectionClosed,
        other => SshError::Protocol(format!("{what}: {other}")),
    }
}

/// Runs the chain. See the module docs for the rules.
pub(crate) async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthContext<'_>,
) -> Result<(), SshError> {
    if ctx.methods.is_empty() {
        return Err(SshError::AuthFailed {
            method: "none",
            detail: "no authentication method is configured for this connection".to_string(),
        });
    }

    let mut report = Report::default();
    let mut allowed: Option<MethodSet> = None;

    match handle.authenticate_none(ctx.username).await {
        Ok(AuthResult::Success) => return Ok(()),
        Ok(AuthResult::Failure {
            remaining_methods, ..
        }) => {
            // An empty list means the server did not say; do not skip
            // everything on that.
            if !remaining_methods.is_empty() {
                allowed = Some(remaining_methods);
            }
        }
        // Nothing was attempted yet: a server that hangs up here (a busy
        // `MaxStartups` limit, a firewall) is a connection problem, which may
        // be retried.
        Err(error) => return Err(transport_error(&error, "authentication probe failed")),
    }

    // Set once the server turned a password down: keyboard-interactive must
    // not send the same password again (it would count as a second failed
    // login against the account's lock-out counter).
    let mut password_rejected = false;
    let mut pending: Vec<&AuthChoice> = ctx.methods.iter().collect();
    while let Some(position) = pending
        .iter()
        .position(|method| is_allowed(allowed.as_ref(), method))
    {
        let method = pending.remove(position);
        match run_method(handle, ctx, method, password_rejected).await {
            Ok(MethodResult::Success) => return Ok(()),
            Ok(MethodResult::Rejected {
                remaining,
                partial_success,
                note,
            }) => {
                if matches!(method, AuthChoice::Password { .. }) && !partial_success {
                    password_rejected = true;
                }
                report.record(method, Outcome::Rejected(note, partial_success));
                if !remaining.is_empty() {
                    allowed = Some(remaining);
                }
            }
            Err(SshError::ConnectionClosed) => return Err(report.connection_closed()),
            Err(error) if is_method_local(&error) => {
                report.record(method, Outcome::Unusable(error));
            }
            Err(error) => return Err(error),
        }
    }

    Err(report.into_error(&pending, allowed.as_ref(), ctx.methods))
}

/// Failures of one method that leave the chain intact: the next method may
/// still work.
fn is_method_local(error: &SshError) -> bool {
    matches!(
        error,
        SshError::PassphraseRequired
            | SshError::WrongPassphrase { .. }
            | SshError::KeyLoadFailed { .. }
            | SshError::AgentUnavailable
            | SshError::AgentNoKey
            | SshError::AuthFailed { .. }
    )
}

async fn run_method(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthContext<'_>,
    method: &AuthChoice,
    password_rejected: bool,
) -> Result<MethodResult, SshError> {
    match method {
        AuthChoice::Password { password } => try_password(handle, ctx, password).await,
        AuthChoice::PrivateKey {
            key_path,
            passphrase,
            identity,
        } => {
            try_private_key(
                handle,
                ctx,
                key_path,
                passphrase.as_ref(),
                identity.as_ref(),
            )
            .await
        }
        AuthChoice::Agent {
            socket,
            prefer_fingerprint,
        } => {
            try_agent(
                handle,
                ctx,
                socket.as_deref(),
                prefer_fingerprint.as_deref(),
            )
            .await
        }
        AuthChoice::KeyboardInteractive { password } => {
            let reusable = password.as_ref().filter(|_| !password_rejected);
            try_keyboard_interactive(handle, ctx, reusable).await
        }
    }
}

// ---------- the report ----------

enum Outcome {
    /// The server said no (`note` says more; the flag is "partial success").
    Rejected(Option<String>, bool),
    /// The method could not even be tried.
    Unusable(SshError),
}

#[derive(Default)]
struct Report {
    attempts: Vec<(&'static str, Outcome)>,
}

impl Report {
    fn record(&mut self, method: &AuthChoice, outcome: Outcome) {
        self.attempts.push((method.method_name(), outcome));
    }

    fn tried_summary(&self) -> String {
        self.attempts
            .iter()
            .map(|(name, outcome)| match outcome {
                Outcome::Rejected(Some(note), _) => format!("{name} ({note})"),
                Outcome::Rejected(None, true) => {
                    format!("{name} (accepted, but the server wants more)")
                }
                Outcome::Rejected(None, false) => format!("{name} (rejected by the server)"),
                Outcome::Unusable(error) => format!("{name} ({error})"),
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// The server dropped the connection while we were authenticating, which
    /// is what `sshd` does after too many failed attempts. Reported as an
    /// authentication failure, not as a lost network: retrying would only
    /// repeat the attempts (and the questions).
    fn connection_closed(&self) -> SshError {
        let tried = self.tried_summary();
        SshError::AuthFailed {
            method: "authentication",
            detail: if tried.is_empty() {
                "the server closed the connection during authentication".to_string()
            } else {
                format!(
                    "the server closed the connection during authentication (it may limit the number of attempts); tried: {tried}"
                )
            },
        }
    }

    fn into_error(
        mut self,
        skipped: &[&AuthChoice],
        allowed: Option<&MethodSet>,
        configured: &[AuthChoice],
    ) -> SshError {
        let server_allows = describe_set(allowed);
        let skipped_note = if skipped.is_empty() {
            String::new()
        } else {
            format!(
                "; not tried because the server does not allow them: {}",
                skipped
                    .iter()
                    .map(|method| method.method_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        match self.attempts.len() {
            0 => SshError::AuthFailed {
                method: "none of the configured methods",
                detail: format!(
                    "the server allows {server_allows}, but this connection is set up for {}",
                    configured
                        .iter()
                        .map(AuthChoice::method_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            1 => {
                let (name, outcome) = self.attempts.remove(0);
                match outcome {
                    // A single method that could not be used: its own error
                    // is the most useful one (and drives the UI, e.g. the
                    // passphrase dialog).
                    Outcome::Unusable(error) if skipped.is_empty() => error,
                    Outcome::Unusable(error) => SshError::AuthFailed {
                        method: name,
                        detail: format!("{error}{skipped_note}; the server allows {server_allows}"),
                    },
                    Outcome::Rejected(note, partial) => SshError::AuthFailed {
                        method: name,
                        detail: single_rejection(note, partial, &server_allows, &skipped_note),
                    },
                }
            }
            _ => SshError::AuthFailed {
                method: "multiple methods",
                detail: format!(
                    "tried {}{skipped_note}; the server allows {server_allows}",
                    self.tried_summary()
                ),
            },
        }
    }
}

fn single_rejection(
    note: Option<String>,
    partial_success: bool,
    server_allows: &str,
    skipped_note: &str,
) -> String {
    let head = match (note, partial_success) {
        (_, true) => "the server accepted this step but requires additional authentication that this connection cannot provide".to_string(),
        (Some(note), false) => note,
        (None, false) => "the server rejected the credentials".to_string(),
    };
    format!("{head} (methods the server allows: {server_allows}){skipped_note}")
}

// ---------- asking the user ----------

/// Marks a human as busy for as long as it lives, also when the future that
/// owns it is dropped (the connection was cancelled while a question waited).
struct Busy<'a>(&'a AtomicBool);

impl<'a> Busy<'a> {
    fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self(flag)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn ask_user(
    ctx: &AuthContext<'_>,
    request: AuthPromptRequest,
) -> Result<Vec<SecretValue>, SshError> {
    let expected = request.prompts.len();
    let busy = Busy::new(ctx.prompt_active);
    let outcome = tokio::time::timeout(ctx.prompt_timeout, ctx.prompt.ask(request)).await;
    drop(busy);
    match outcome {
        Ok(AuthPromptResponse::Answers(answers)) if answers.len() == expected => Ok(answers),
        Ok(AuthPromptResponse::Answers(_)) => Err(SshError::Protocol(
            "the authentication prompt returned the wrong number of answers".to_string(),
        )),
        Ok(AuthPromptResponse::Cancelled) => Err(SshError::AuthCancelled),
        Ok(AuthPromptResponse::TimedOut) | Err(_) => Err(SshError::AuthPromptTimeout),
    }
}

// ---------- password ----------

async fn try_password(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthContext<'_>,
    password: &SecretValue,
) -> Result<MethodResult, SshError> {
    handle
        .authenticate_password(ctx.username, password.expose())
        .await
        .map(MethodResult::from)
        .map_err(|error| transport_error(&error, "password authentication failed"))
}

// ---------- key file (+ certificate) ----------

async fn try_private_key(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthContext<'_>,
    key_path: &Path,
    passphrase: Option<&SecretValue>,
    identity: Option<&KeyIdentity>,
) -> Result<MethodResult, SshError> {
    let key = Arc::new(load_key_interactive(ctx, key_path, passphrase, identity).await?);

    // A certificate next to the key (`id_ed25519-cert.pub`, as `ssh-keygen -s`
    // writes it) is offered first, like OpenSSH does. If the server turns it
    // down the plain key still gets its turn.
    if let Some(certificate) = load_certificate(key_path, &key) {
        let result = handle
            .authenticate_openssh_cert(ctx.username, key.clone(), certificate)
            .await
            .map_err(|error| transport_error(&error, "certificate authentication failed"))?;
        match MethodResult::from(result) {
            MethodResult::Rejected {
                partial_success: false,
                ..
            } => tracing::debug!("the server rejected the certificate; trying the plain key"),
            decided => return Ok(decided),
        }
    }

    let hash = if key.algorithm().is_rsa() {
        rsa_hash(handle).await
    } else {
        None
    };
    handle
        .authenticate_publickey(ctx.username, PrivateKeyWithHashAlg::new(key, hash))
        .await
        .map(MethodResult::from)
        .map_err(|error| transport_error(&error, "publickey authentication failed"))
}

/// The signature hash for RSA keys: the best one the server announced
/// (`rsa-sha2-512` before `rsa-sha2-256` before the legacy SHA-1 `ssh-rsa`).
/// A server that announced nothing (older than OpenSSH 7.2, or one that does
/// not send `server-sig-algs`) gets SHA-256, which every server still
/// accepting RSA understands.
async fn rsa_hash(handle: &Handle<ClientHandler>) -> Option<HashAlg> {
    match handle.best_supported_rsa_hash().await {
        Ok(Some(announced)) => announced,
        Ok(None) | Err(_) => Some(HashAlg::Sha256),
    }
}

/// `<key>-cert.pub` when it exists, parses and certifies exactly this key.
/// Anything else is not an error: the plain key is simply used.
fn load_certificate(key_path: &Path, key: &PrivateKey) -> Option<Certificate> {
    let mut name = key_path.as_os_str().to_os_string();
    name.push("-cert.pub");
    let path = PathBuf::from(name);
    if !path.is_file() {
        return None;
    }
    let certificate = match Certificate::read_file(&path) {
        Ok(certificate) => certificate,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring an unreadable certificate");
            return None;
        }
    };
    if certificate.public_key() != key.public_key().key_data() {
        tracing::warn!(path = %path.display(), "ignoring a certificate that belongs to another key");
        return None;
    }
    Some(certificate)
}

/// Loads the key; asks for the passphrase when the key is encrypted and the
/// supplied one is missing or wrong.
async fn load_key_interactive(
    ctx: &AuthContext<'_>,
    path: &Path,
    supplied: Option<&SecretValue>,
    identity: Option<&KeyIdentity>,
) -> Result<PrivateKey, SshError> {
    let mut passphrase = supplied.cloned();
    for _ in 0..MAX_PASSPHRASE_ATTEMPTS {
        let error = match load_key_blocking(path, passphrase.clone()).await {
            Ok(key) => return Ok(key),
            Err(error) => error,
        };
        let retry = match &error {
            SshError::PassphraseRequired => false,
            SshError::WrongPassphrase { .. } => true,
            _ => return Err(error),
        };
        if !ctx.prompt.is_interactive() {
            return Err(error);
        }
        let label = identity.map_or_else(
            || {
                path.file_name().map_or_else(
                    || "the key".to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            },
            |identity| identity.name.clone(),
        );
        let request = AuthPromptRequest {
            kind: AuthPromptKind::Passphrase {
                identity: identity.cloned(),
                retry,
            },
            name: String::new(),
            instructions: String::new(),
            prompts: vec![AuthPromptField {
                text: format!("Passphrase for {label}"),
                echo: false,
            }],
        };
        passphrase = ask_user(ctx, request).await?.pop();
    }
    Err(SshError::WrongPassphrase {
        path: path.display().to_string(),
    })
}

/// Decrypting a key (bcrypt KDF) takes a noticeable fraction of a second of
/// CPU: not on an async worker.
async fn load_key_blocking(
    path: &Path,
    passphrase: Option<SecretValue>,
) -> Result<PrivateKey, SshError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || load_key(&path, passphrase.as_ref()))
        .await
        .map_err(|error| SshError::Protocol(format!("the key could not be loaded: {error}")))?
}

pub(crate) fn load_key(
    path: &Path,
    passphrase: Option<&SecretValue>,
) -> Result<PrivateKey, SshError> {
    let path_str = path.display().to_string();
    let passphrase = passphrase.map(|p| p.expose().to_string());
    match russh::keys::load_secret_key(path, passphrase.as_deref()) {
        Ok(key) => Ok(key),
        Err(err) => Err(classify_key_error(err, &path_str, passphrase.is_some())),
    }
}

pub(crate) fn classify_key_error(
    err: russh::keys::Error,
    path: &str,
    passphrase_given: bool,
) -> SshError {
    use russh::keys::Error as KeyError;
    if matches!(err, KeyError::KeyIsEncrypted) {
        return if passphrase_given {
            SshError::WrongPassphrase {
                path: path.to_string(),
            }
        } else {
            SshError::PassphraseRequired
        };
    }
    let detail = err.to_string();
    let lower = detail.to_lowercase();
    let looks_crypto = lower.contains("decrypt")
        || lower.contains("passphrase")
        || lower.contains("password")
        || lower.contains("crypt");
    if looks_crypto {
        if passphrase_given {
            SshError::WrongPassphrase {
                path: path.to_string(),
            }
        } else {
            SshError::PassphraseRequired
        }
    } else {
        SshError::KeyLoadFailed {
            path: path.to_string(),
            detail,
        }
    }
}

// ---------- SSH agent ----------

async fn try_agent(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthContext<'_>,
    socket: Option<&Path>,
    prefer_fingerprint: Option<&str>,
) -> Result<MethodResult, SshError> {
    let mut agent = connect_agent(socket).await?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| SshError::AuthFailed {
            method: "publickey (agent)",
            detail: format!("the agent could not list its keys: {e}"),
        })?;
    if identities.is_empty() {
        return Err(SshError::AgentNoKey);
    }
    let total = identities.len();
    let ordered = order_agent_identities(identities, prefer_fingerprint);

    let mut rejected = 0usize;
    let mut unusable = 0usize;
    let mut last_remaining: Option<MethodSet> = None;
    for identity in ordered.into_iter().take(MAX_AGENT_KEYS) {
        let hash = if identity.public_key().algorithm().is_rsa() {
            rsa_hash(handle).await
        } else {
            None
        };
        let attempt = match &identity {
            AgentIdentity::PublicKey { key, .. } => {
                handle
                    .authenticate_publickey_with(ctx.username, key.clone(), hash, &mut agent)
                    .await
            }
            AgentIdentity::Certificate { certificate, .. } => {
                handle
                    .authenticate_certificate_with(
                        ctx.username,
                        certificate.clone(),
                        hash,
                        &mut agent,
                    )
                    .await
            }
        };
        match attempt {
            Ok(AuthResult::Success) => return Ok(MethodResult::Success),
            Ok(AuthResult::Failure {
                remaining_methods,
                partial_success,
            }) => {
                if partial_success {
                    return Ok(MethodResult::Rejected {
                        remaining: remaining_methods,
                        partial_success,
                        note: None,
                    });
                }
                rejected += 1;
                let publickey_left = remaining_methods.contains(&MethodKind::PublicKey);
                last_remaining = Some(remaining_methods);
                if !publickey_left {
                    // The server will not take any more keys.
                    break;
                }
            }
            Err(error) => {
                if handle.is_closed() {
                    return Err(SshError::ConnectionClosed);
                }
                // The agent could not sign (it declined, e.g. a confirmation
                // was refused, or it lost the key): the next key may work.
                tracing::warn!(%error, comment = identity_label(&identity), "the agent could not sign; trying the next key");
                unusable += 1;
            }
        }
    }

    if rejected == 0 {
        return Err(SshError::AuthFailed {
            method: "publickey (agent)",
            detail: format!("the agent could not sign with any of its {total} key(s)"),
        });
    }
    let mut note = format!("the server rejected {rejected} of the agent's {total} key(s)");
    if unusable > 0 {
        note.push_str(&format!(", {unusable} could not sign"));
    }
    if total > MAX_AGENT_KEYS {
        note.push_str(&format!(
            "; only the first {MAX_AGENT_KEYS} are offered to stay under the server's attempt limit"
        ));
    }
    Ok(MethodResult::Rejected {
        remaining: last_remaining.unwrap_or_else(MethodSet::empty),
        partial_success: false,
        note: Some(note),
    })
}

fn identity_label(identity: &AgentIdentity) -> &str {
    identity.comment()
}

/// The preferred key (by `SHA256:` fingerprint) first, everything else in the
/// agent's own order.
pub(crate) fn order_agent_identities(
    identities: Vec<AgentIdentity>,
    prefer_fingerprint: Option<&str>,
) -> Vec<AgentIdentity> {
    let Some(wanted) = prefer_fingerprint else {
        return identities;
    };
    let (mut first, rest): (Vec<_>, Vec<_>) = identities
        .into_iter()
        .partition(|identity| fingerprint(&identity.public_key()) == wanted);
    first.extend(rest);
    first
}

type DynAgent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

async fn connect_agent(socket: Option<&Path>) -> Result<DynAgent, SshError> {
    #[cfg(unix)]
    {
        let connected = match socket {
            Some(path) => AgentClient::connect_uds(path).await,
            None => AgentClient::connect_env().await,
        };
        match connected {
            Ok(client) => Ok(client.dynamic()),
            Err(_) => Err(SshError::AgentUnavailable),
        }
    }
    #[cfg(windows)]
    {
        // The OpenSSH agent that ships with Windows listens on a named pipe;
        // Pageant is the fallback. `SSH_AUTH_SOCK` only counts when it names
        // a pipe (Git-for-Windows sets a Unix-style path that is useless
        // here).
        const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
        let pipe = socket
            .filter(|path| path.to_string_lossy().starts_with(r"\\.\pipe\"))
            .map_or_else(
                || std::ffi::OsString::from(OPENSSH_PIPE),
                |path| path.as_os_str().to_os_string(),
            );
        // `connect_named_pipe` retries a busy pipe forever: bound it.
        let pipe_client = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            AgentClient::connect_named_pipe(&pipe),
        )
        .await;
        if let Ok(Ok(client)) = pipe_client {
            return Ok(client.dynamic());
        }
        match AgentClient::connect_pageant().await {
            Ok(client) => Ok(client.dynamic()),
            Err(_) => Err(SshError::AgentUnavailable),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = socket;
        Err(SshError::AgentUnavailable)
    }
}

// ---------- keyboard-interactive ----------

async fn try_keyboard_interactive(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthContext<'_>,
    password: Option<&SecretValue>,
) -> Result<MethodResult, SshError> {
    let lost = |error: russh::Error| transport_error(&error, "keyboard-interactive failed");
    let mut response = handle
        .authenticate_keyboard_interactive_start(ctx.username, None)
        .await
        .map_err(lost)?;
    let mut password_available = password.is_some();
    let mut rounds = 0usize;
    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(MethodResult::Success),
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods,
                partial_success,
            } => {
                return Ok(MethodResult::Rejected {
                    remaining: remaining_methods,
                    partial_success,
                    note: None,
                });
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                rounds += 1;
                if rounds > MAX_KBD_ROUNDS {
                    return Err(SshError::AuthFailed {
                        method: "keyboard-interactive",
                        detail: format!(
                            "the server kept asking questions (more than {MAX_KBD_ROUNDS} rounds)"
                        ),
                    });
                }
                let answers: Vec<String> = if prompts.is_empty() {
                    // An information-only round: show it, answer nothing.
                    let text = [name.trim(), instructions.trim()]
                        .iter()
                        .filter(|part| !part.is_empty())
                        .copied()
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        let _ = ctx.notices.send(text);
                    }
                    Vec::new()
                } else if let Some(stored) =
                    password.filter(|_| password_available && is_lone_password_prompt(&prompts))
                {
                    // Only once: if the server asks again the stored
                    // password was wrong, and repeating it would only feed
                    // the server's lock-out counter.
                    password_available = false;
                    vec![stored.expose().to_string()]
                } else {
                    if !ctx.prompt.is_interactive() {
                        return Err(SshError::AuthFailed {
                            method: "keyboard-interactive",
                            detail:
                                "the server asked a question and no interactive prompt is available"
                                    .to_string(),
                        });
                    }
                    let request = AuthPromptRequest {
                        kind: AuthPromptKind::KeyboardInteractive,
                        name,
                        instructions,
                        prompts: prompts
                            .iter()
                            .map(|prompt| AuthPromptField {
                                text: prompt.prompt.clone(),
                                echo: prompt.echo,
                            })
                            .collect(),
                    };
                    ask_user(ctx, request)
                        .await?
                        .iter()
                        .map(|answer| answer.expose().to_string())
                        .collect()
                };
                response = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(lost)?;
            }
        }
    }
}

/// Words that make "...password" something other than the account password
/// (one-time codes, password changes): the stored password must not be sent
/// to those.
const NOT_THE_ACCOUNT_PASSWORD: &[&str] = &[
    "one-time",
    "one time",
    "otp",
    "token",
    "verification",
    "code",
    "passcode",
    "pin",
    "new",
    "retype",
    "confirm",
    "current",
    "old",
    "expired",
    "reenter",
    "re-enter",
];

/// `true` for the one question a PAM stack asks for a plain password:
/// a single hidden prompt such as `Password:`, `user@host's password:` or
/// `(user@host) Password:`.
pub(crate) fn is_lone_password_prompt(prompts: &[Prompt]) -> bool {
    let [only] = prompts else {
        return false;
    };
    if only.echo {
        return false;
    }
    let text = only
        .prompt
        .trim()
        .trim_end_matches(':')
        .trim()
        .to_lowercase();
    if text.split_whitespace().last() != Some("password") {
        return false;
    }
    !NOT_THE_ACCOUNT_PASSWORD
        .iter()
        .any(|word| text.contains(word))
}

// ---------- the server's banner ----------

/// Most text a server may show us before authentication (a banner can be
/// sent repeatedly).
pub(crate) const MAX_BANNER_BYTES: usize = 32 * 1024;

/// Turns server-controlled text into something safe to print in a terminal.
///
/// The banner arrives before the user has authenticated the server's claims
/// to anything, and a terminal obeys escape sequences: one could rewrite the
/// screen, set the window title or change the colours. Escape sequences (CSI
/// such as `ESC [ 31 m`, OSC such as `ESC ] 0 ; title BEL`, and the shorter
/// forms) are therefore removed **whole**, so a colourful banner reads as
/// plain text instead of leaving `[31m` behind. Everything else that is not
/// plain text goes too — remaining control characters, the C1 range and
/// bidirectional overrides — except newlines and tabs. Line endings become
/// `\n`.
pub(crate) fn sanitize_banner(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_BANNER_BYTES));
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if out.len() >= MAX_BANNER_BYTES {
            break;
        }
        match c {
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push('\n');
                }
            }
            '\n' | '\t' => out.push(c),
            '\x1b' => skip_escape(&mut chars),
            // The 8-bit forms of CSI and OSC.
            '\u{9b}' => skip_csi(&mut chars),
            '\u{9d}' => skip_osc(&mut chars),
            c if c.is_control() => {}
            // Bidirectional embeddings, overrides and isolates: text that
            // displays other than it reads.
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' => {}
            c => out.push(c),
        }
    }
    out
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// After `ESC`: the rest of the sequence.
fn skip_escape(chars: &mut Chars<'_>) {
    match chars.peek().copied() {
        Some('[') => {
            chars.next();
            skip_csi(chars);
        }
        Some(']') => {
            chars.next();
            skip_osc(chars);
        }
        // `ESC ( B` and friends: intermediate bytes, then one final byte.
        Some('\u{20}'..='\u{2f}') => {
            while matches!(chars.peek(), Some('\u{20}'..='\u{2f}')) {
                chars.next();
            }
            if matches!(chars.peek(), Some('\u{30}'..='\u{7e}')) {
                chars.next();
            }
        }
        // `ESC c`, `ESC 7`, ...: a single final byte. A newline stays.
        Some(next) if !next.is_control() => {
            chars.next();
        }
        _ => {}
    }
}

/// After `CSI`: parameter and intermediate bytes, then the final byte.
fn skip_csi(chars: &mut Chars<'_>) {
    while matches!(chars.peek(), Some('\u{20}'..='\u{3f}')) {
        chars.next();
    }
    if matches!(chars.peek(), Some('\u{40}'..='\u{7e}')) {
        chars.next();
    }
}

/// After `OSC`: everything up to the terminator (`BEL` or `ST`). A string
/// that is never terminated swallows the rest of the text.
fn skip_osc(chars: &mut Chars<'_>) {
    while let Some(c) = chars.next() {
        match c {
            '\u{7}' | '\u{9c}' => return,
            '\x1b' if chars.peek() == Some(&'\\') => {
                chars.next();
                return;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use russh::keys::PublicKey;

    use super::*;

    fn prompt(text: &str, echo: bool) -> Prompt {
        Prompt {
            prompt: text.to_string(),
            echo,
        }
    }

    #[test]
    fn a_lone_hidden_password_prompt_is_recognised() {
        for text in [
            "Password:",
            "password: ",
            "Password",
            "deploy@host's password:",
            "(deploy@host) Password:",
        ] {
            assert!(
                is_lone_password_prompt(&[prompt(text, false)]),
                "{text:?} is the account password"
            );
        }
    }

    /// The stored password must never be sent to a question that is not for
    /// the account password: a one-time code, a password change, a second
    /// question.
    #[test]
    fn other_questions_are_not_answered_with_the_password() {
        for text in [
            "One-time password:",
            "Verification code:",
            "Enter your OTP password:",
            "New password:",
            "Retype new password:",
            "(current) UNIX password:",
            "Token:",
            "Username:",
            "",
        ] {
            assert!(
                !is_lone_password_prompt(&[prompt(text, false)]),
                "{text:?} must go to the user"
            );
        }
        assert!(
            !is_lone_password_prompt(&[prompt("Password:", true)]),
            "a visible prompt is not a password prompt"
        );
        assert!(
            !is_lone_password_prompt(&[prompt("Password:", false), prompt("Code:", true)]),
            "two questions are never answered automatically"
        );
        assert!(!is_lone_password_prompt(&[]));
    }

    #[test]
    fn banner_text_loses_everything_a_terminal_would_obey() {
        let hostile = "Welcome\x1b[2J\x1b]0;pwned\x07 to\r\nthe\rhost\u{9b}31m\u{202e}evil\x00";
        let clean = sanitize_banner(hostile);
        for forbidden in ['\x1b', '\x07', '\u{9b}', '\u{202e}', '\0', '\r'] {
            assert!(
                !clean.contains(forbidden),
                "{forbidden:?} survived: {clean:?}"
            );
        }
        // The sequences go whole: no `[2J`, no window title, no `31m`.
        assert_eq!(clean, "Welcome to\nthe\nhostevil");
    }

    /// A colourful banner reads as plain text, not as text with the remains
    /// of its colour codes in it.
    #[test]
    fn colour_codes_leave_no_remains() {
        let coloured = "\x1b[1;31mAuthorized\x1b[0m users \x1b(Bonly\x1b]0;t\x1b\\.\x1bc";
        assert_eq!(sanitize_banner(coloured), "Authorized users only.");
        // An unterminated title swallows the rest instead of leaking it.
        assert_eq!(sanitize_banner("ok\x1b]0;never ends"), "ok");
        // A lone escape at the end is just dropped.
        assert_eq!(sanitize_banner("ok\x1b"), "ok");
        // An escape before a newline does not eat the newline.
        assert_eq!(sanitize_banner("a\x1b\nb"), "a\nb");
    }

    #[test]
    fn banner_text_is_bounded() {
        let huge = "x".repeat(MAX_BANNER_BYTES * 3);
        assert!(sanitize_banner(&huge).len() <= MAX_BANNER_BYTES);
        assert_eq!(sanitize_banner("plain\ttext\nline"), "plain\ttext\nline");
    }

    fn named(comment: &str, key: PublicKey) -> AgentIdentity {
        AgentIdentity::PublicKey {
            key,
            comment: comment.to_string(),
        }
    }

    #[test]
    fn the_preferred_agent_key_goes_first_and_the_rest_keep_their_order() {
        let keys: Vec<PublicKey> = (1..=4u8).map(test_public_key).collect();
        let identities: Vec<AgentIdentity> = keys
            .iter()
            .enumerate()
            .map(|(i, key)| named(&format!("k{i}"), key.clone()))
            .collect();
        let wanted = fingerprint(&keys[2]);

        let ordered = order_agent_identities(identities.clone(), Some(&wanted));
        let comments: Vec<&str> = ordered.iter().map(AgentIdentity::comment).collect();
        assert_eq!(comments, ["k2", "k0", "k1", "k3"]);

        let untouched = order_agent_identities(identities.clone(), None);
        let comments: Vec<&str> = untouched.iter().map(AgentIdentity::comment).collect();
        assert_eq!(comments, ["k0", "k1", "k2", "k3"]);

        let unknown = order_agent_identities(identities, Some("SHA256:nothing"));
        let comments: Vec<&str> = unknown.iter().map(AgentIdentity::comment).collect();
        assert_eq!(comments, ["k0", "k1", "k2", "k3"]);
    }

    fn test_public_key(seed: u8) -> PublicKey {
        crate::known_hosts::tests_support::ed25519_public_key(seed)
    }

    #[test]
    fn a_single_unusable_method_keeps_its_own_error() {
        let mut report = Report::default();
        let method = AuthChoice::PrivateKey {
            key_path: PathBuf::from("/k"),
            passphrase: None,
            identity: None,
        };
        report.record(&method, Outcome::Unusable(SshError::PassphraseRequired));
        let error = report.into_error(&[], None, std::slice::from_ref(&method));
        assert!(matches!(error, SshError::PassphraseRequired), "{error:?}");
    }

    #[test]
    fn several_failures_are_listed_with_what_the_server_allows() {
        let key = AuthChoice::PrivateKey {
            key_path: PathBuf::from("/k"),
            passphrase: None,
            identity: None,
        };
        let kbd = AuthChoice::KeyboardInteractive { password: None };
        let password = AuthChoice::Password {
            password: SecretValue::new("hunter2".to_string()),
        };
        let mut report = Report::default();
        report.record(&key, Outcome::Rejected(None, false));
        report.record(&kbd, Outcome::Rejected(None, false));
        let allowed =
            MethodSet::from(&[MethodKind::PublicKey, MethodKind::KeyboardInteractive][..]);
        let error = report.into_error(&[&password], Some(&allowed), &[key, kbd, password.clone()]);
        let text = error.to_string();
        assert!(
            text.contains("publickey (rejected by the server)"),
            "{text}"
        );
        assert!(text.contains("keyboard-interactive (rejected"), "{text}");
        assert!(text.contains("password"), "{text}");
        assert!(
            text.contains("the server allows publickey, keyboard-interactive"),
            "{text}"
        );
        assert!(!text.contains("hunter2"), "{text}");
        assert!(error.is_auth_failure());
    }

    #[test]
    fn when_every_method_is_ruled_out_the_error_says_so() {
        let password = AuthChoice::Password {
            password: SecretValue::new("hunter2".to_string()),
        };
        let allowed = MethodSet::from(&[MethodKind::PublicKey][..]);
        let error = Report::default().into_error(
            &[&password],
            Some(&allowed),
            std::slice::from_ref(&password),
        );
        let text = error.to_string();
        assert!(text.contains("the server allows publickey"), "{text}");
        assert!(text.contains("set up for password"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }

    #[test]
    fn methods_are_skipped_only_when_the_server_names_others() {
        let password = AuthChoice::Password {
            password: SecretValue::new("x".to_string()),
        };
        let only_keys = MethodSet::from(&[MethodKind::PublicKey][..]);
        assert!(!is_allowed(Some(&only_keys), &password));
        assert!(
            is_allowed(None, &password),
            "an unknown list rules nothing out"
        );
        let agent = AuthChoice::Agent {
            socket: None,
            prefer_fingerprint: None,
        };
        assert!(is_allowed(Some(&only_keys), &agent));
    }
}
