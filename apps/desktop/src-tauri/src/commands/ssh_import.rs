//! Importing hosts from the user's OpenSSH configuration (`~/.ssh/config`).
//!
//! `ssh_config_preview` reads the file and lists what could be imported, with
//! the hosts the library already has (same address, user and port) marked.
//! `ssh_config_import` creates the chosen hosts, one identity per distinct
//! `IdentityFile` (the path is **bound** on this device; the key file itself
//! is never opened, read or copied) and the jump chains between them, all in
//! one store transaction.
//!
//! The parser lives in `termru_ssh::ssh_config` and does no I/O. Everything
//! that touches the disk is here: the file read, `Include` resolution
//! ([`FsIncludeLoader`]) and the existence check of key files. The planning
//! ([`plan`]) is pure, so it is tested without a window or a file system.
//!
//! The frontend only names aliases. The import reads the file again and
//! builds everything from it: nothing the webview sends is trusted to
//! describe a host.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use termru_core::host::{AuthMethod, Host};
use termru_core::identity::Identity;
use termru_core::ids::{HostId, IdentityId};
use termru_core::validate::{JumpNode, resolve_jump_chain, strip_brackets};
use termru_ssh::ssh_config::{
    self, ImportCandidate, IncludeLoader, JumpRef, LoadedFile, ParsedConfig,
};
use termru_store::{ImportBatch, ImportedIdentity, Store};

use crate::app_error::CommandError;
use crate::state::AppState;

/// A config file larger than this is refused: nobody writes one by hand.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Files one `Include` pattern may expand to.
const MAX_GLOB_MATCHES: usize = 128;

// ---------------------------------------------------------------- DTOs ----

/// One host of the configuration, as the import dialog shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SshConfigCandidate {
    /// The `Host` alias; also the name the imported host gets.
    pub alias: String,
    pub host: String,
    /// The user the import would use (the configured one, else this
    /// machine's user name); `None` when there is neither.
    pub user: Option<String>,
    pub port: u16,
    /// The key files, `~` expanded (those that could be).
    pub identity_files: Vec<String>,
    /// The route, first hop first, as written (`bastion`, `ops@gw:2200`).
    pub jump_chain: Vec<String>,
    pub warnings: Vec<String>,
    /// The library already has a host with this address, user and port.
    pub existing_host_id: Option<String>,
    /// Why this host cannot be imported, when it cannot.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshConfigPreview {
    /// `false` when there is no `~/.ssh/config` (not an error).
    pub found: bool,
    pub path: String,
    pub candidates: Vec<SshConfigCandidate>,
    /// Things about the file as a whole: skipped `Match` blocks, includes
    /// that could not be followed.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportedHost {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkippedHost {
    pub alias: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshConfigImportReport {
    /// Every host that was created: the ones asked for and the jump hosts
    /// they needed.
    pub imported: Vec<ImportedHost>,
    pub identities_created: usize,
    /// Asked for, but not imported (with the reason).
    pub skipped: Vec<SkippedHost>,
    /// Asked for, but the library already has the host.
    pub already_present: Vec<String>,
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------ the plan ----

/// What the library holds that the import must not duplicate.
#[derive(Debug, Default, Clone)]
pub(crate) struct Library {
    pub hosts: Vec<Host>,
    /// Key path → the identity bound to it.
    pub identity_paths: HashMap<String, IdentityId>,
}

pub(crate) struct Context<'a> {
    pub home: Option<&'a Path>,
    /// This machine's user name: the user of a host that names none.
    pub user: Option<&'a str>,
    pub key_exists: &'a dyn Fn(&Path) -> bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Create(HostId),
    Exists(HostId),
    Skipped(String),
}

pub(crate) struct Plan {
    pub batch: ImportBatch,
    /// One entry per requested alias, in the order requested.
    pub outcomes: Vec<(String, Outcome)>,
    pub warnings: Vec<String>,
}

type HostKey = (String, String, u16);

/// Hosts and identities the import would create, so far. Cloned before each
/// requested host and put back when that host fails: a half-built route is
/// never left behind.
#[derive(Clone, Default)]
struct Draft {
    hosts: Vec<Host>,
    identities: Vec<ImportedIdentity>,
    keys: HashMap<HostKey, HostId>,
    paths: HashMap<String, IdentityId>,
    warnings: Vec<String>,
}

struct Planner<'a> {
    config: &'a ParsedConfig,
    library: &'a Library,
    ctx: &'a Context<'a>,
    draft: Draft,
    /// Names being resolved right now (the route being walked).
    stack: Vec<String>,
}

fn host_key(address: &str, user: &str, port: u16) -> HostKey {
    (
        strip_brackets(address).to_ascii_lowercase(),
        user.to_string(),
        port,
    )
}

impl Planner<'_> {
    /// The id of the host `name` (an alias, or any name a `ProxyJump` uses)
    /// stands for, creating it when the library and the draft lack it.
    fn resolve(
        &mut self,
        name: &str,
        user: Option<&str>,
        port: Option<u16>,
    ) -> Result<(HostId, bool), String> {
        let candidate = self.config.describe(name);
        let user = user
            .map(str::to_string)
            .or_else(|| candidate.user.clone())
            .or_else(|| self.ctx.user.map(str::to_string))
            .ok_or_else(|| {
                "no user name: the configuration sets none and this machine's is unknown"
                    .to_string()
            })?;
        let port = port.unwrap_or(candidate.port);
        let key = host_key(&candidate.host, &user, port);

        if let Some(existing) = self
            .library
            .hosts
            .iter()
            .find(|host| host_key(&host.address, &host.username, host.port) == key)
        {
            return Ok((existing.id, false));
        }
        if let Some(id) = self.draft.keys.get(&key) {
            return Ok((*id, false));
        }
        let marker = format!("{}|{}|{}", key.0, key.1, key.2);
        if self.stack.contains(&marker) {
            return Err(format!("the jump hosts of {name} lead back to it"));
        }
        self.stack.push(marker);
        let route = self.route(&candidate);
        self.stack.pop();
        let route = route?;

        let mut host = Host::new(
            name.to_string(),
            strip_brackets(&candidate.host).to_string(),
            user,
        );
        host.port = port;
        host.jump_chain = route;
        self.attach_identity(&candidate, &mut host);
        for warning in &candidate.warnings {
            self.draft.warnings.push(format!("{name}: {warning}"));
        }
        host.validate().map_err(|error| error.to_string())?;
        self.draft.keys.insert(key, host.id);
        let id = host.id;
        self.draft.hosts.push(host);
        Ok((id, true))
    }

    fn route(&mut self, candidate: &ImportCandidate) -> Result<Vec<HostId>, String> {
        let mut route = Vec::with_capacity(candidate.jump_chain.len());
        for JumpRef { host, user, port } in &candidate.jump_chain {
            let (id, _) = self
                .resolve(host, user.as_deref(), *port)
                .map_err(|reason| format!("jump host {host}: {reason}"))?;
            route.push(id);
        }
        Ok(route)
    }

    /// The first key file that can be located becomes the host's identity
    /// (a host uses one key); without any, the host authenticates through the
    /// agent, which is what `ssh` falls back to.
    fn attach_identity(&mut self, candidate: &ImportCandidate, host: &mut Host) {
        let mut unusable = Vec::new();
        let mut chosen: Option<PathBuf> = None;
        for file in &candidate.identity_files {
            match expand_key_path(file, self.ctx) {
                Ok(path) => {
                    chosen = Some(path);
                    break;
                }
                Err(reason) => unusable.push(format!("{file}: {reason}")),
            }
        }
        for note in unusable {
            self.draft
                .warnings
                .push(format!("{}: IdentityFile {note}", host.name));
        }
        let Some(path) = chosen else {
            host.auth = AuthMethod::Agent;
            return;
        };
        let used = candidate
            .identity_files
            .iter()
            .filter(|file| expand_key_path(file, self.ctx).is_ok())
            .count();
        if used > 1 {
            self.draft.warnings.push(format!(
                "{}: only the first of its {used} IdentityFile entries was imported (a host uses one key)",
                host.name
            ));
        }
        let text = path.to_string_lossy().into_owned();
        let identity_id = if let Some(id) = self.library.identity_paths.get(&text) {
            *id
        } else if let Some(id) = self.draft.paths.get(&text) {
            *id
        } else {
            let name = path
                .file_name()
                .map_or_else(|| text.clone(), |name| name.to_string_lossy().into_owned());
            let identity = Identity::new(name);
            let id = identity.id;
            if !(self.ctx.key_exists)(&path) {
                self.draft.warnings.push(format!(
                    "{}: the key file {text} does not exist (the identity was created anyway)",
                    host.name
                ));
            }
            self.draft.identities.push(ImportedIdentity {
                identity,
                key_path: text.clone(),
            });
            self.draft.paths.insert(text, id);
            id
        };
        host.auth = AuthMethod::PrivateKey { identity_id };
    }

    /// Every route of the draft must be walkable.
    fn check_routes(&self) -> Result<(), String> {
        let lookup = |id: HostId| {
            self.draft
                .hosts
                .iter()
                .chain(self.library.hosts.iter())
                .find(|host| host.id == id)
                .map(|host| JumpNode {
                    name: host.name.clone(),
                    jump_chain: host.jump_chain.clone(),
                })
        };
        for host in &self.draft.hosts {
            resolve_jump_chain(host.id, &host.name, &host.jump_chain, &lookup)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

/// Plans the import of `selection` (aliases of `config`, in order). Never
/// fails as a whole: a host that cannot be imported is `Skipped` with the
/// reason, and what it needed is not imported either.
pub(crate) fn plan(
    config: &ParsedConfig,
    selection: &[String],
    library: &Library,
    ctx: &Context<'_>,
) -> Plan {
    let declared: HashSet<String> = config
        .candidates()
        .into_iter()
        .map(|candidate| candidate.alias)
        .collect();
    let mut planner = Planner {
        config,
        library,
        ctx,
        draft: Draft::default(),
        stack: Vec::new(),
    };
    let mut outcomes = Vec::with_capacity(selection.len());
    let mut seen: HashSet<&str> = HashSet::new();
    for alias in selection {
        if !seen.insert(alias.as_str()) {
            continue;
        }
        if !declared.contains(alias) {
            outcomes.push((
                alias.clone(),
                Outcome::Skipped("the configuration declares no such host".to_string()),
            ));
            continue;
        }
        let snapshot = planner.draft.clone();
        let result = planner
            .resolve(alias, None, None)
            .and_then(|(id, created)| planner.check_routes().map(|()| (id, created)));
        match result {
            Ok((id, true)) => outcomes.push((alias.clone(), Outcome::Create(id))),
            Ok((id, false)) => outcomes.push((alias.clone(), Outcome::Exists(id))),
            Err(reason) => {
                planner.draft = snapshot;
                outcomes.push((alias.clone(), Outcome::Skipped(reason)));
            }
        }
    }
    let draft = planner.draft;
    Plan {
        batch: ImportBatch {
            identities: draft.identities,
            hosts: draft.hosts,
        },
        outcomes,
        warnings: draft.warnings,
    }
}

/// `~`, `%d` (home), `%u` (user name) and `%%` are expanded; a relative path
/// is taken relative to `~/.ssh`. Anything else Termru cannot know is an
/// error, and the entry is not used.
pub(crate) fn expand_key_path(raw: &str, ctx: &Context<'_>) -> Result<PathBuf, String> {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => text.push_str(&home(ctx)?.to_string_lossy()),
            Some('u') => text.push_str(
                ctx.user
                    .ok_or_else(|| "the user name of this machine is unknown".to_string())?,
            ),
            Some('%') => text.push('%'),
            Some(other) => return Err(format!("the token %{other} cannot be expanded")),
            None => text.push('%'),
        }
    }
    let path = if text == "~" {
        home(ctx)?.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home(ctx)?.join(rest)
    } else if text.starts_with('~') {
        return Err("~user paths are not supported".to_string());
    } else {
        PathBuf::from(&text)
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(home(ctx)?.join(".ssh").join(path))
    }
}

fn home<'a>(ctx: &'a Context<'_>) -> Result<&'a Path, String> {
    ctx.home
        .ok_or_else(|| "the home directory is unknown".to_string())
}

// ------------------------------------------------------- the file system --

/// Resolves `Include` patterns on disk: `~` is the home directory, a relative
/// pattern is anchored in `~/.ssh`, wildcards expand like `glob(3)` (sorted,
/// hidden files only when the pattern names them).
pub(crate) struct FsIncludeLoader {
    home: PathBuf,
    /// What could not be read, for the user.
    pub notes: Vec<String>,
}

impl FsIncludeLoader {
    pub(crate) fn new(home: PathBuf) -> Self {
        Self {
            home,
            notes: Vec::new(),
        }
    }

    fn anchor(&self, pattern: &str) -> PathBuf {
        let path = if pattern == "~" {
            self.home.clone()
        } else if let Some(rest) = pattern.strip_prefix("~/") {
            self.home.join(rest)
        } else {
            PathBuf::from(pattern)
        };
        if path.is_absolute() {
            path
        } else {
            self.home.join(".ssh").join(path)
        }
    }
}

impl IncludeLoader for FsIncludeLoader {
    fn load(&mut self, pattern: &str) -> Vec<LoadedFile> {
        let mut files = Vec::new();
        for path in expand_glob(&self.anchor(pattern)) {
            if !path.is_file() {
                continue;
            }
            match read_limited(&path) {
                Ok(contents) => files.push(LoadedFile {
                    name: path.display().to_string(),
                    contents,
                }),
                Err(reason) => self
                    .notes
                    .push(format!("Include {pattern}: {}: {reason}", path.display())),
            }
        }
        files
    }
}

fn read_limited(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    if size > MAX_CONFIG_BYTES {
        return Err(format!("larger than {MAX_CONFIG_BYTES} bytes; skipped"));
    }
    std::fs::read_to_string(path).map_err(|error| error.to_string())
}

fn has_wildcard(component: &str) -> bool {
    component.contains(['*', '?'])
}

/// Expands the wildcards of `pattern`, component by component. A path
/// without wildcards is returned as it is (whether it exists is the
/// caller's question).
pub(crate) fn expand_glob(pattern: &Path) -> Vec<PathBuf> {
    let mut results: Vec<PathBuf> = vec![PathBuf::new()];
    for component in pattern.components() {
        let name = component.as_os_str().to_string_lossy().into_owned();
        let mut next = Vec::new();
        for base in &results {
            if !has_wildcard(&name) {
                next.push(base.join(component.as_os_str()));
                continue;
            }
            let Ok(entries) = std::fs::read_dir(base) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|entry| !entry.starts_with('.') || name.starts_with('.'))
                .filter(|entry| ssh_config::pattern_matches(&name, entry))
                .collect();
            names.sort();
            next.extend(names.into_iter().map(|entry| base.join(entry)));
        }
        results = next;
        results.truncate(MAX_GLOB_MATCHES);
    }
    results
}

// ----------------------------------------------------------- commands -----

/// This machine's user name, for hosts whose configuration names no `User`.
fn local_user() -> Option<String> {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .filter(|user| termru_core::validate::validate_username(user).is_ok())
}

fn library(store: &Store) -> Result<Library, CommandError> {
    let repos = store.repos();
    let hosts = repos.hosts().list()?;
    let identity_paths = repos
        .identities()
        .bindings()?
        .into_iter()
        .map(|(id, path)| (path, id))
        .collect();
    Ok(Library {
        hosts,
        identity_paths,
    })
}

/// What reading `~/.ssh/config` produced.
struct Loaded {
    config: ParsedConfig,
    path: PathBuf,
    notes: Vec<String>,
}

/// `Ok(None)`: there is no such file.
fn load_config(home: &Path) -> Result<Option<Loaded>, CommandError> {
    let path = home.join(".ssh").join("config");
    if !path.is_file() {
        return Ok(None);
    }
    let text = read_limited(&path)
        .map_err(|reason| CommandError::new(format!("{}: {reason}", path.display())))?;
    let mut loader = FsIncludeLoader::new(home.to_path_buf());
    let config = ssh_config::parse(&path.display().to_string(), &text, &mut loader);
    Ok(Some(Loaded {
        config,
        path,
        notes: loader.notes,
    }))
}

fn describe_jump(hop: &JumpRef) -> String {
    let mut out = String::new();
    if let Some(user) = &hop.user {
        out.push_str(user);
        out.push('@');
    }
    out.push_str(&hop.host);
    if let Some(port) = hop.port {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out
}

/// The preview of `config` against `library`: every declared host, whether
/// the library has it, and whether it could be imported.
pub(crate) fn preview(
    config: &ParsedConfig,
    library: &Library,
    ctx: &Context<'_>,
) -> Vec<SshConfigCandidate> {
    let candidates = config.candidates();
    let aliases: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.alias.clone())
        .collect();
    let outcomes: HashMap<String, Outcome> = plan(config, &aliases, library, ctx)
        .outcomes
        .into_iter()
        .collect();
    candidates
        .into_iter()
        .map(|candidate| {
            let user = candidate
                .user
                .clone()
                .or_else(|| ctx.user.map(str::to_string));
            let (existing_host_id, problem) = match outcomes.get(&candidate.alias) {
                Some(Outcome::Exists(id)) => (Some(id.to_string()), None),
                Some(Outcome::Skipped(reason)) => (None, Some(reason.clone())),
                _ => (None, None),
            };
            SshConfigCandidate {
                identity_files: candidate
                    .identity_files
                    .iter()
                    .map(|file| {
                        expand_key_path(file, ctx)
                            .map_or_else(|_| file.clone(), |path| path.display().to_string())
                    })
                    .collect(),
                jump_chain: candidate.jump_chain.iter().map(describe_jump).collect(),
                alias: candidate.alias,
                host: candidate.host,
                user,
                port: candidate.port,
                warnings: candidate.warnings,
                existing_host_id,
                problem,
            }
        })
        .collect()
}

/// Lists what `~/.ssh/config` could contribute.
#[tauri::command]
pub async fn ssh_config_preview(
    state: State<'_, AppState>,
) -> Result<SshConfigPreview, CommandError> {
    let store = state.store.clone();
    let home = state.home_dir.clone();
    // File reads and SQLite: not on an async worker.
    tauri::async_runtime::spawn_blocking(move || {
        preview_blocking(&store, home.as_deref(), local_user().as_deref())
    })
    .await
    .map_err(|error| CommandError::new(format!("the import could not be prepared: {error}")))?
}

fn preview_blocking(
    store: &Store,
    home: Option<&Path>,
    user: Option<&str>,
) -> Result<SshConfigPreview, CommandError> {
    let Some(home) = home else {
        return Err(CommandError::new("the home directory is unknown"));
    };
    let path = home.join(".ssh").join("config");
    let Some(loaded) = load_config(home)? else {
        return Ok(SshConfigPreview {
            found: false,
            path: path.display().to_string(),
            candidates: Vec::new(),
            warnings: Vec::new(),
        });
    };
    let ctx = Context {
        home: Some(home),
        user,
        key_exists: &|path| path.is_file(),
    };
    let candidates = preview(&loaded.config, &library(store)?, &ctx);
    let mut warnings = loaded.config.warnings.clone();
    warnings.extend(loaded.notes);
    Ok(SshConfigPreview {
        found: true,
        path: loaded.path.display().to_string(),
        candidates,
        warnings,
    })
}

/// Imports the hosts named by `selection` (aliases from the preview), with
/// the jump hosts they route through and the identities of their key files.
#[tauri::command]
pub async fn ssh_config_import(
    state: State<'_, AppState>,
    selection: Vec<String>,
) -> Result<SshConfigImportReport, CommandError> {
    let store = state.store.clone();
    let home = state.home_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        import_blocking(&store, home.as_deref(), local_user().as_deref(), &selection)
    })
    .await
    .map_err(|error| CommandError::new(format!("the import failed: {error}")))?
}

pub(crate) fn import_blocking(
    store: &Store,
    home: Option<&Path>,
    user: Option<&str>,
    selection: &[String],
) -> Result<SshConfigImportReport, CommandError> {
    let Some(home) = home else {
        return Err(CommandError::new("the home directory is unknown"));
    };
    let loaded = load_config(home)?.ok_or_else(|| {
        CommandError::new(format!(
            "{} does not exist",
            home.join(".ssh").join("config").display()
        ))
    })?;
    let ctx = Context {
        home: Some(home),
        user,
        key_exists: &|path| path.is_file(),
    };
    let mut plan = plan(&loaded.config, selection, &library(store)?, &ctx);
    let imported: Vec<ImportedHost> = plan
        .batch
        .hosts
        .iter()
        .map(|host| ImportedHost {
            id: host.id.to_string(),
            name: host.name.clone(),
        })
        .collect();
    let identities_created = plan.batch.identities.len();
    if !plan.batch.hosts.is_empty() || !plan.batch.identities.is_empty() {
        store.repos().hosts().import(&mut plan.batch)?;
    }
    let mut skipped = Vec::new();
    let mut already_present = Vec::new();
    for (alias, outcome) in plan.outcomes {
        match outcome {
            Outcome::Skipped(reason) => skipped.push(SkippedHost { alias, reason }),
            Outcome::Exists(_) => already_present.push(alias),
            Outcome::Create(_) => {}
        }
    }
    Ok(SshConfigImportReport {
        imported,
        identities_created,
        skipped,
        already_present,
        warnings: plan.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        home: &'a Path,
        user: Option<&'a str>,
        exists: &'a dyn Fn(&Path) -> bool,
    ) -> Context<'a> {
        Context {
            home: Some(home),
            user,
            key_exists: exists,
        }
    }

    fn parse(text: &str) -> ParsedConfig {
        ssh_config::parse("config", text, &mut ssh_config::NoIncludes)
    }

    fn aliases(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    fn created<'a>(plan: &'a Plan, name: &str) -> &'a Host {
        plan.batch
            .hosts
            .iter()
            .find(|host| host.name == name)
            .unwrap_or_else(|| panic!("no created host {name}: {:?}", plan.outcomes))
    }

    const CONFIG: &str = "\
Host bastion
  HostName bastion.corp.example
  User ops
  Port 2222
  IdentityFile ~/.ssh/work_ed25519

Host app
  HostName app.corp.example
  User deploy
  ProxyJump bastion
  IdentityFile ~/.ssh/work_ed25519

Host db
  HostName 10.0.0.5
  ProxyJump ops@gw.example.com:2200,bastion

Host plain
";

    fn run(text: &str, selection: &[&str], library: &Library) -> Plan {
        let home = Path::new("/home/me");
        let yes = |_: &Path| true;
        plan(
            &parse(text),
            &aliases(selection),
            library,
            &ctx(home, Some("me"), &yes),
        )
    }

    #[test]
    fn a_host_with_a_key_gets_one_identity_bound_to_its_path() {
        let plan = run(CONFIG, &["bastion"], &Library::default());
        let host = created(&plan, "bastion");
        assert_eq!(host.address, "bastion.corp.example");
        assert_eq!((host.username.as_str(), host.port), ("ops", 2222));
        assert_eq!(plan.batch.identities.len(), 1);
        let imported = &plan.batch.identities[0];
        assert_eq!(imported.key_path, "/home/me/.ssh/work_ed25519");
        assert_eq!(imported.identity.name, "work_ed25519");
        assert_eq!(
            host.auth,
            AuthMethod::PrivateKey {
                identity_id: imported.identity.id
            }
        );
    }

    #[test]
    fn hosts_sharing_a_key_file_share_its_identity() {
        let plan = run(CONFIG, &["bastion", "app"], &Library::default());
        assert_eq!(plan.batch.identities.len(), 1);
        let (bastion, app) = (created(&plan, "bastion"), created(&plan, "app"));
        assert_eq!(bastion.auth, app.auth);
    }

    #[test]
    fn a_key_path_the_library_already_binds_is_reused() {
        let identity = IdentityId::new();
        let library = Library {
            hosts: Vec::new(),
            identity_paths: HashMap::from([("/home/me/.ssh/work_ed25519".to_string(), identity)]),
        };
        let plan = run(CONFIG, &["bastion"], &library);
        assert!(plan.batch.identities.is_empty());
        assert_eq!(
            created(&plan, "bastion").auth,
            AuthMethod::PrivateKey {
                identity_id: identity
            }
        );
    }

    #[test]
    fn a_proxy_jump_by_alias_becomes_a_jump_chain_and_imports_the_jump_host() {
        let plan = run(CONFIG, &["app"], &Library::default());
        let (app, bastion) = (created(&plan, "app"), created(&plan, "bastion"));
        assert_eq!(app.jump_chain, vec![bastion.id]);
        assert_eq!(plan.batch.hosts.len(), 2, "the jump host comes along");
        assert_eq!(
            plan.outcomes,
            vec![("app".to_string(), Outcome::Create(app.id))]
        );
    }

    #[test]
    fn a_hop_the_file_does_not_declare_is_created_from_what_the_list_says() {
        let plan = run(CONFIG, &["db"], &Library::default());
        let db = created(&plan, "db");
        assert_eq!(db.jump_chain.len(), 2);
        let gateway = plan
            .batch
            .hosts
            .iter()
            .find(|host| host.id == db.jump_chain[0])
            .expect("the first hop");
        assert_eq!(gateway.address, "gw.example.com");
        assert_eq!((gateway.username.as_str(), gateway.port), ("ops", 2200));
        assert!(matches!(gateway.auth, AuthMethod::Agent));
        assert_eq!(db.jump_chain[1], created(&plan, "bastion").id);
    }

    #[test]
    fn a_host_without_a_key_uses_the_agent_and_the_local_user() {
        let plan = run(CONFIG, &["plain"], &Library::default());
        let plain = created(&plan, "plain");
        assert!(matches!(plain.auth, AuthMethod::Agent));
        assert_eq!(plain.username, "me");
        assert_eq!((plain.address.as_str(), plain.port), ("plain", 22));
        assert!(plan.batch.identities.is_empty());
    }

    #[test]
    fn a_host_that_exists_is_recognised_by_address_user_and_port() {
        let mut existing = Host::new(
            "My Bastion".into(),
            "BASTION.corp.example".into(),
            "ops".into(),
        );
        existing.port = 2222;
        let library = Library {
            hosts: vec![existing.clone()],
            identity_paths: HashMap::new(),
        };
        let plan = run(CONFIG, &["bastion", "app"], &library);
        assert_eq!(
            plan.outcomes[0],
            ("bastion".to_string(), Outcome::Exists(existing.id))
        );
        // The existing host serves as the jump host of what is imported.
        let app = created(&plan, "app");
        assert_eq!(app.jump_chain, vec![existing.id]);
        assert_eq!(plan.batch.hosts.len(), 1);

        // A different port is a different host.
        existing.port = 22;
        let library = Library {
            hosts: vec![existing],
            identity_paths: HashMap::new(),
        };
        let plan = run(CONFIG, &["bastion"], &library);
        assert!(matches!(plan.outcomes[0].1, Outcome::Create(_)));
    }

    #[test]
    fn unknown_and_repeated_aliases_are_reported_or_ignored() {
        let plan = run(CONFIG, &["ghost", "plain", "plain"], &Library::default());
        assert!(
            matches!(&plan.outcomes[0].1, Outcome::Skipped(reason) if reason.contains("no such host"))
        );
        assert_eq!(plan.outcomes.len(), 2, "the repeat is ignored");
        assert_eq!(plan.batch.hosts.len(), 1);
    }

    #[test]
    fn a_cycle_of_jump_hosts_skips_the_hosts_in_it_and_nothing_else() {
        let text = "Host a\n  ProxyJump b\nHost b\n  ProxyJump a\nHost fine\n  User x\n";
        let plan = run(text, &["a", "b", "fine"], &Library::default());
        for alias in ["a", "b"] {
            let (_, outcome) = plan
                .outcomes
                .iter()
                .find(|(name, _)| name == alias)
                .expect(alias);
            assert!(
                matches!(outcome, Outcome::Skipped(reason) if reason.contains("lead back")),
                "{alias}: {outcome:?}"
            );
        }
        assert_eq!(
            plan.batch.hosts.len(),
            1,
            "only `fine` is created: {:?}",
            plan.batch.hosts
        );
        assert_eq!(plan.batch.hosts[0].name, "fine");
    }

    #[test]
    fn a_host_that_cannot_be_valid_is_skipped_with_the_reason() {
        let text = "Host odd\n  HostName \"bad host name\"\nHost ok\n";
        let plan = run(text, &["odd", "ok"], &Library::default());
        assert!(
            matches!(&plan.outcomes[0].1, Outcome::Skipped(reason) if reason.contains("address"))
        );
        assert!(matches!(plan.outcomes[1].1, Outcome::Create(_)));
    }

    #[test]
    fn a_failing_jump_host_takes_its_dependants_with_it_and_leaves_no_orphans() {
        let text = "Host app\n  ProxyJump broken\nHost broken\n  HostName \"bad host\"\nHost ok\n";
        let plan = run(text, &["app", "ok"], &Library::default());
        assert!(
            matches!(&plan.outcomes[0].1, Outcome::Skipped(reason) if reason.contains("jump host broken"))
        );
        let names: Vec<&str> = plan
            .batch
            .hosts
            .iter()
            .map(|host| host.name.as_str())
            .collect();
        assert_eq!(names, vec!["ok"]);
    }

    #[test]
    fn without_any_user_a_host_is_skipped() {
        let home = Path::new("/home/me");
        let yes = |_: &Path| true;
        let plan = plan(
            &parse("Host a\n"),
            &aliases(&["a"]),
            &Library::default(),
            &ctx(home, None, &yes),
        );
        assert!(
            matches!(&plan.outcomes[0].1, Outcome::Skipped(reason) if reason.contains("no user"))
        );
    }

    #[test]
    fn key_paths_expand_the_way_ssh_does() {
        let home = Path::new("/home/me");
        let yes = |_: &Path| true;
        let ctx = ctx(home, Some("me"), &yes);
        for (raw, expected) in [
            ("~/.ssh/id", "/home/me/.ssh/id"),
            ("/abs/key", "/abs/key"),
            ("%d/.ssh/%u_key", "/home/me/.ssh/me_key"),
            ("rel_key", "/home/me/.ssh/rel_key"),
            ("~", "/home/me"),
            ("100%%", "/home/me/.ssh/100%"),
        ] {
            assert_eq!(
                expand_key_path(raw, &ctx).expect(raw),
                PathBuf::from(expected),
                "{raw}"
            );
        }
        for raw in ["~other/key", "%h_key"] {
            assert!(expand_key_path(raw, &ctx).is_err(), "{raw}");
        }
        let no_home = Context {
            home: None,
            user: None,
            key_exists: &yes,
        };
        assert!(expand_key_path("~/x", &no_home).is_err());
        assert!(expand_key_path("/abs", &no_home).is_ok());
    }

    #[test]
    fn extra_key_files_and_missing_keys_are_reported() {
        let text = "Host a\n  IdentityFile ~/.ssh/one\n  IdentityFile ~/.ssh/two\n";
        let home = Path::new("/home/me");
        let no = |_: &Path| false;
        let plan = plan(
            &parse(text),
            &aliases(&["a"]),
            &Library::default(),
            &ctx(home, Some("me"), &no),
        );
        assert_eq!(plan.batch.identities.len(), 1, "one key per host");
        let joined = plan.warnings.join("\n");
        assert!(joined.contains("only the first"), "{joined}");
        assert!(joined.contains("does not exist"), "{joined}");
    }

    #[test]
    fn the_preview_marks_existing_and_impossible_hosts() {
        let existing = Host::new("b".into(), "bastion.corp.example".into(), "ops".into());
        let mut existing = existing;
        existing.port = 2222;
        let library = Library {
            hosts: vec![existing.clone()],
            identity_paths: HashMap::new(),
        };
        let home = Path::new("/home/me");
        let yes = |_: &Path| true;
        let text = format!("{CONFIG}\nHost odd\n  HostName \"bad host\"\n");
        let rows = preview(&parse(&text), &library, &ctx(home, Some("me"), &yes));
        let row = |alias: &str| rows.iter().find(|row| row.alias == alias).expect(alias);
        assert_eq!(
            row("bastion").existing_host_id,
            Some(existing.id.to_string())
        );
        assert!(row("app").existing_host_id.is_none() && row("app").problem.is_none());
        assert_eq!(row("app").jump_chain, vec!["bastion"]);
        assert_eq!(
            row("db").jump_chain,
            vec!["ops@gw.example.com:2200", "bastion"]
        );
        assert_eq!(
            row("app").identity_files,
            vec!["/home/me/.ssh/work_ed25519"]
        );
        assert!(row("odd").problem.is_some());
        assert_eq!(row("plain").user.as_deref(), Some("me"));
    }

    // ------------------------------------------------------ file system --

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    #[test]
    fn includes_resolve_relative_to_ssh_dir_with_wildcards_sorted_and_without_hidden_files() {
        let home = tempfile::tempdir().expect("home");
        let ssh = home.path().join(".ssh");
        write(&ssh.join("conf.d/b.conf"), "Host from-b\n");
        write(&ssh.join("conf.d/a.conf"), "Host from-a\n");
        write(&ssh.join("conf.d/.hidden.conf"), "Host hidden\n");
        write(&ssh.join("conf.d/notes.txt"), "Host not-config\n");
        write(&ssh.join("extra"), "Host extra\n");
        write(
            &ssh.join("config"),
            "Include conf.d/*.conf\nInclude ~/.ssh/extra\nInclude missing/*\nHost root\n",
        );
        let loaded = load_config(home.path()).expect("loads").expect("exists");
        let names: Vec<String> = loaded
            .config
            .candidates()
            .into_iter()
            .map(|candidate| candidate.alias)
            .collect();
        assert_eq!(names, vec!["from-a", "from-b", "extra", "root"]);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    }

    #[test]
    fn a_missing_config_is_not_an_error_and_an_oversized_one_is() {
        let home = tempfile::tempdir().expect("home");
        assert!(load_config(home.path()).expect("no error").is_none());
        let path = home.path().join(".ssh/config");
        write(
            &path,
            &"#".repeat(usize::try_from(MAX_CONFIG_BYTES).expect("fits") + 1),
        );
        let error = load_config(home.path()).err().expect("too large");
        assert!(error.message.contains("larger than"), "{}", error.message);
    }

    #[test]
    fn glob_expansion_handles_wildcards_in_directories_too() {
        let dir = tempfile::tempdir().expect("dir");
        write(&dir.path().join("one/x.conf"), "");
        write(&dir.path().join("two/y.conf"), "");
        write(&dir.path().join("two/z.txt"), "");
        let found = expand_glob(&dir.path().join("*/*.conf"));
        let names: Vec<String> = found
            .iter()
            .map(|path| {
                path.strip_prefix(dir.path())
                    .expect("inside")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["one/x.conf", "two/y.conf"]);
        // No wildcard: the path comes back whether or not it exists.
        assert_eq!(
            expand_glob(&dir.path().join("nope")),
            vec![dir.path().join("nope")]
        );
    }

    #[test]
    fn importing_creates_hosts_identities_and_jump_chains_in_one_go() {
        let home = tempfile::tempdir().expect("home");
        write(&home.path().join(".ssh/config"), CONFIG);
        write(
            &home.path().join(".ssh/work_ed25519"),
            "not read by the import",
        );
        let dir = tempfile::tempdir().expect("db dir");
        let store = Store::open(&dir.path().join("t.sqlite")).expect("store");

        let report = import_blocking(
            &store,
            Some(home.path()),
            Some("me"),
            &aliases(&["app", "plain"]),
        )
        .expect("imports");
        let names: Vec<&str> = report
            .imported
            .iter()
            .map(|host| host.name.as_str())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        for name in ["app", "bastion", "plain"] {
            assert!(names.contains(&name), "{names:?}");
        }
        assert_eq!(report.identities_created, 1);
        assert!(report.skipped.is_empty() && report.already_present.is_empty());

        let hosts = store.repos().hosts().list().expect("list");
        let app = hosts.iter().find(|host| host.name == "app").expect("app");
        let bastion = hosts
            .iter()
            .find(|host| host.name == "bastion")
            .expect("bastion");
        assert_eq!(app.jump_chain, vec![bastion.id]);
        let AuthMethod::PrivateKey { identity_id } = &app.auth else {
            panic!("app uses a key: {:?}", app.auth);
        };
        let bound = store
            .repos()
            .identities()
            .binding(*identity_id)
            .expect("binding");
        assert_eq!(
            bound.as_deref(),
            Some(
                home.path()
                    .join(".ssh/work_ed25519")
                    .to_str()
                    .expect("utf-8")
            )
        );
        // Everything is queued for sync.
        assert_eq!(store.repos().sync().pending().expect("pending").len(), 4);

        // Importing again changes nothing and says so.
        let again = import_blocking(
            &store,
            Some(home.path()),
            Some("me"),
            &aliases(&["app", "plain", "ghost"]),
        )
        .expect("imports");
        assert!(again.imported.is_empty() && again.identities_created == 0);
        assert_eq!(again.already_present, vec!["app", "plain"]);
        assert_eq!(again.skipped.len(), 1);
        assert_eq!(store.repos().hosts().list().expect("list").len(), 3);
    }

    #[test]
    fn importing_without_a_config_says_so() {
        let home = tempfile::tempdir().expect("home");
        let dir = tempfile::tempdir().expect("db dir");
        let store = Store::open(&dir.path().join("t.sqlite")).expect("store");
        let error = import_blocking(&store, Some(home.path()), Some("me"), &aliases(&["a"]))
            .expect_err("no config");
        assert!(
            error.message.contains("does not exist"),
            "{}",
            error.message
        );
        let error =
            import_blocking(&store, None, Some("me"), &aliases(&["a"])).expect_err("no home");
        assert!(
            error.message.contains("home directory"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_preview_of_a_machine_without_a_config() {
        let home = tempfile::tempdir().expect("home");
        let dir = tempfile::tempdir().expect("db dir");
        let store = Store::open(&dir.path().join("t.sqlite")).expect("store");
        let preview = preview_blocking(&store, Some(home.path()), Some("me")).expect("no error");
        assert!(!preview.found && preview.candidates.is_empty());
        assert!(preview.path.ends_with("config"));
    }
}
