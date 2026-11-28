//! Input validation for user-supplied domain fields.
//!
//! All validation is centralized here so the desktop UI, the local store, and
//! the sync payload builder enforce identical rules. Limits are deliberately
//! conservative: they bound attacker-controlled sizes in synced payloads and
//! keep UI rendering safe (no control characters in names that end up in logs,
//! menus, or terminal titles).

use crate::error::CoreError;
use crate::host::{AuthKind, EnvVar, Forward, ForwardKind};
use crate::ids::HostId;

pub const MAX_NAME_LEN: usize = 128;
pub const MAX_ADDRESS_LEN: usize = 253;
pub const MAX_USERNAME_LEN: usize = 128;
pub const MAX_DESCRIPTION_LEN: usize = 2_000;
pub const MAX_TAG_NAME_LEN: usize = 64;
pub const MAX_TERM_LEN: usize = 64;
pub const MAX_SNIPPET_BODY_LEN: usize = 16_384;
pub const MAX_TAGS_PER_HOST: usize = 32;
pub const MAX_GROUP_DEPTH: usize = 8;
/// Hops a connection may pass through before it reaches the host.
pub const MAX_JUMP_HOPS: usize = 8;
pub const MAX_FORWARDS_PER_HOST: usize = 32;
pub const MAX_ENV_VARS: usize = 32;
pub const MAX_ENV_NAME_LEN: usize = 64;
pub const MAX_ENV_VALUE_LEN: usize = 1_024;
pub const MAX_STARTUP_COMMAND_LEN: usize = 1_024;
/// The four authentication kinds: an order may name each at most once.
pub const MAX_AUTH_ORDER_LEN: usize = 4;
/// Longest interface name Linux accepts is 15 bytes; Windows zone ids are
/// numbers. The bound keeps a hostile record from carrying a novel.
const MAX_ZONE_ID_LEN: usize = 32;

type Result<T> = std::result::Result<T, CoreError>;

/// Rejects strings containing control characters (including newline/NUL) and
/// requires a non-empty, non-whitespace-only value within `max_len`.
pub fn validate_display_name(field: &str, value: &str, max_len: usize) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation(format!("{field} must not be empty")));
    }
    if value.chars().all(char::is_whitespace) {
        return Err(CoreError::validation(format!(
            "{field} must contain non-whitespace characters"
        )));
    }
    if value.chars().count() > max_len {
        return Err(CoreError::validation(format!(
            "{field} must be at most {max_len} characters"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(CoreError::validation(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(())
}

/// Validates a hostname or IP address literal used for SSH connections.
///
/// Intentionally conservative: letters, digits, dots, hyphens, underscores
/// (common in internal DNS), and IPv6 colons. Rejects spaces, slashes, quotes,
/// and anything else that has no business in a connection target. This is
/// defense in depth against argument/URL injection if the value is ever used
/// to build strings passed to other components.
///
/// IPv6 literals are accepted bare (`fe80::1`, the canonical form) or in
/// brackets (`[fe80::1]`, as typed in URLs and `ssh_config`), and may carry a
/// zone id (`fe80::1%eth0`, `fe80::1%3`) — link-local addresses cannot be
/// reached without one. Use [`strip_brackets`] before handing the address to
/// the network stack.
pub fn validate_address(value: &str) -> Result<()> {
    check_host("address", value, true)
}

/// Like [`validate_address`] for the other host names a record carries
/// (forward targets, bind addresses): a value with a colon must be a real
/// IPv6 literal, because these fields are new and need no leniency towards
/// older data.
fn validate_host_field(field: &str, value: &str) -> Result<()> {
    check_host(field, value, false)
}

/// `legacy_colons`: keep accepting strings with colons that merely use the
/// characters of the old rule (host addresses saved before IPv6 literals were
/// parsed). Such a value never connected anywhere, but refusing it now would
/// make an old synced record fail validation on another device.
fn check_host(field: &str, value: &str, legacy_colons: bool) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation(format!("{field} must not be empty")));
    }
    if value.chars().count() > MAX_ADDRESS_LEN + 2 {
        return Err(CoreError::validation(format!(
            "{field} must be at most {MAX_ADDRESS_LEN} characters"
        )));
    }
    if let Some(inner) = value.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or_else(|| {
            CoreError::validation(format!("{field} has an opening '[' without a closing ']'"))
        })?;
        return check_ipv6(field, inner);
    }
    if value.contains('%') || (value.contains(':') && !legacy_colons) {
        return check_ipv6(field, value);
    }
    if value.chars().count() > MAX_ADDRESS_LEN {
        return Err(CoreError::validation(format!(
            "{field} must be at most {MAX_ADDRESS_LEN} characters"
        )));
    }
    let ok = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    if !ok {
        return Err(CoreError::validation(format!(
            "{field} contains invalid characters (allowed: letters, digits, '.', '-', '_', ':')"
        )));
    }
    if value.starts_with('-') || value.ends_with('-') {
        return Err(CoreError::validation(format!(
            "{field} must not start or end with '-'"
        )));
    }
    Ok(())
}

/// An IPv6 literal with an optional `%zone` (no brackets).
fn check_ipv6(field: &str, value: &str) -> Result<()> {
    let (address, zone) = match value.split_once('%') {
        Some((address, zone)) => (address, Some(zone)),
        None => (value, None),
    };
    if address.parse::<std::net::Ipv6Addr>().is_err() {
        return Err(CoreError::validation(format!(
            "{field} is not a valid IPv6 address"
        )));
    }
    if let Some(zone) = zone {
        let ok = !zone.is_empty()
            && zone.len() <= MAX_ZONE_ID_LEN
            && zone
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !ok {
            return Err(CoreError::validation(format!(
                "{field} has an invalid IPv6 zone id (letters, digits, '-', '_' and '.' only)"
            )));
        }
    }
    Ok(())
}

/// `[::1]` → `::1`; anything else is returned unchanged. The canonical,
/// connectable form of an address.
pub fn strip_brackets(value: &str) -> &str {
    value
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(value)
}

pub fn validate_username(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation("username must not be empty"));
    }
    if value.chars().count() > MAX_USERNAME_LEN {
        return Err(CoreError::validation(format!(
            "username must be at most {MAX_USERNAME_LEN} characters"
        )));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@'))
    {
        return Err(CoreError::validation(
            "username contains invalid characters (allowed: letters, digits, '.', '-', '_', '@')",
        ));
    }
    Ok(())
}

pub fn validate_port(port: u16) -> Result<()> {
    if port == 0 {
        return Err(CoreError::validation("port must be between 1 and 65535"));
    }
    Ok(())
}

pub fn validate_description(value: &str) -> Result<()> {
    if value.chars().count() > MAX_DESCRIPTION_LEN {
        return Err(CoreError::validation(format!(
            "description must be at most {MAX_DESCRIPTION_LEN} characters"
        )));
    }
    if value.chars().any(|c| c == '\0') {
        return Err(CoreError::validation(
            "description must not contain NUL characters",
        ));
    }
    Ok(())
}

/// Validates a terminal type string (e.g. `xterm-256color`).
pub fn validate_term(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation("term must not be empty"));
    }
    if value.chars().count() > MAX_TERM_LEN {
        return Err(CoreError::validation(format!(
            "term must be at most {MAX_TERM_LEN} characters"
        )));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(CoreError::validation(
            "term contains invalid characters (allowed: letters, digits, '-', '_', '.')",
        ));
    }
    Ok(())
}

/// Validates a keepalive interval in seconds (0 = automatic default).
pub fn validate_keepalive(seconds: u32) -> Result<()> {
    const MAX_KEEPALIVE: u32 = 3_600;
    if seconds > MAX_KEEPALIVE {
        return Err(CoreError::validation(format!(
            "keepalive interval must be 0 (automatic) or at most {MAX_KEEPALIVE} seconds"
        )));
    }
    Ok(())
}

/// Validates a startup command: one line of text the shell receives as if it
/// had been typed. It travels through sync, so it is kept small and free of
/// anything that could smuggle a second line or terminal control sequences
/// into the session (tab is allowed).
pub fn validate_startup_command(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(CoreError::validation(
            "startup command must not be empty (leave it unset instead)",
        ));
    }
    if value.chars().count() > MAX_STARTUP_COMMAND_LEN {
        return Err(CoreError::validation(format!(
            "startup command must be at most {MAX_STARTUP_COMMAND_LEN} characters"
        )));
    }
    if value.chars().any(|c| c.is_control() && c != '\t') {
        return Err(CoreError::validation(
            "startup command must be a single line without control characters",
        ));
    }
    Ok(())
}

/// Validates the environment variables sent to the server (`SetEnv`).
///
/// Names follow the POSIX rule `[A-Za-z_][A-Za-z0-9_]*`. Values are plain
/// text: they are synced, so they are no place for secrets (documented in the
/// UI and in `docs/architecture/0010-data-model-v2.md`).
pub fn validate_env(env: &[EnvVar]) -> Result<()> {
    if env.len() > MAX_ENV_VARS {
        return Err(CoreError::validation(format!(
            "a host may have at most {MAX_ENV_VARS} environment variables"
        )));
    }
    let mut seen: Vec<&str> = Vec::with_capacity(env.len());
    for var in env {
        validate_env_name(&var.name)?;
        if var.value.chars().count() > MAX_ENV_VALUE_LEN {
            return Err(CoreError::validation(format!(
                "the value of {} must be at most {MAX_ENV_VALUE_LEN} characters",
                var.name
            )));
        }
        if var.value.chars().any(char::is_control) {
            return Err(CoreError::validation(format!(
                "the value of {} must not contain control characters",
                var.name
            )));
        }
        if seen.contains(&var.name.as_str()) {
            return Err(CoreError::validation(format!(
                "environment variable {} is listed twice",
                var.name
            )));
        }
        seen.push(&var.name);
    }
    Ok(())
}

pub fn validate_env_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !first_ok || !rest_ok {
        return Err(CoreError::validation(format!(
            "environment variable name {name:?} is invalid \
             (use letters, digits and '_', not starting with a digit)"
        )));
    }
    if name.len() > MAX_ENV_NAME_LEN {
        return Err(CoreError::validation(format!(
            "environment variable names must be at most {MAX_ENV_NAME_LEN} characters"
        )));
    }
    Ok(())
}

/// Validates an explicit authentication order: at least one kind, none twice.
pub fn validate_auth_order(order: &[AuthKind]) -> Result<()> {
    if order.is_empty() {
        return Err(CoreError::validation(
            "the authentication order must name at least one method (leave it unset for the default)",
        ));
    }
    if order.len() > MAX_AUTH_ORDER_LEN {
        return Err(CoreError::validation(
            "the authentication order names too many methods",
        ));
    }
    for (index, kind) in order.iter().enumerate() {
        if order[..index].contains(kind) {
            return Err(CoreError::validation(format!(
                "the authentication order names {} twice",
                kind.label()
            )));
        }
    }
    Ok(())
}

/// A local-side listener binds an address, not a name: an IP literal (with an
/// optional zone) or `localhost`.
fn is_bindable_literal(value: &str) -> bool {
    let value = strip_brackets(value);
    if value.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let address = value.split_once('%').map_or(value, |(address, _)| address);
    address.parse::<std::net::IpAddr>().is_ok()
}

/// Validates one port forward against the rules of its kind.
///
/// * `Local` / `Remote`: need a target host and port;
/// * `Dynamic` (SOCKS): has no target at all;
/// * `Local` / `Dynamic` listen on this machine, so their bind address is an
///   IP literal or `localhost`; a `Remote` one names an address on the
///   server (a host name is fine there).
///
/// No bind address means loopback: exposing a forward to the network is
/// always an explicit choice.
pub fn validate_forward(forward: &Forward) -> Result<()> {
    if forward.bind_port == 0 {
        return Err(CoreError::validation(
            "forward bind port must be between 1 and 65535",
        ));
    }
    if let Some(address) = &forward.bind_address {
        validate_host_field("bind address", address)?;
        if forward.kind != ForwardKind::Remote && !is_bindable_literal(address) {
            return Err(CoreError::validation(
                "a local listener needs an IP address (or localhost) as its bind address",
            ));
        }
    }
    match forward.kind {
        ForwardKind::Local | ForwardKind::Remote => {
            let host = forward.target_host.as_deref().ok_or_else(|| {
                CoreError::validation(format!(
                    "a {} forward needs a target host",
                    forward.kind.label()
                ))
            })?;
            validate_host_field("target host", host)?;
            match forward.target_port {
                Some(port) if port != 0 => {}
                _ => {
                    return Err(CoreError::validation(format!(
                        "a {} forward needs a target port between 1 and 65535",
                        forward.kind.label()
                    )));
                }
            }
        }
        ForwardKind::Dynamic => {
            if forward.target_host.is_some() || forward.target_port.is_some() {
                return Err(CoreError::validation(
                    "a dynamic (SOCKS) forward has no target: the client of the proxy names it",
                ));
            }
        }
    }
    Ok(())
}

/// Validates the forwards of one host: each one, their number, and that no
/// two of them claim the same listening address and port.
pub fn validate_forwards(forwards: &[Forward]) -> Result<()> {
    if forwards.len() > MAX_FORWARDS_PER_HOST {
        return Err(CoreError::validation(format!(
            "a host may have at most {MAX_FORWARDS_PER_HOST} port forwards"
        )));
    }
    let mut claimed: Vec<(bool, String, u16)> = Vec::with_capacity(forwards.len());
    for forward in forwards {
        validate_forward(forward)?;
        // Local and dynamic forwards listen here (one port space); remote
        // ones listen on the server (another one).
        let remote = forward.kind == ForwardKind::Remote;
        let address = forward.bind_address.as_deref().map_or_else(
            || if remote { "localhost" } else { "127.0.0.1" }.to_string(),
            |address| {
                let address = strip_brackets(address).to_ascii_lowercase();
                if !remote && address == "localhost" {
                    "127.0.0.1".to_string()
                } else {
                    address
                }
            },
        );
        let key = (remote, address, forward.bind_port);
        if claimed.contains(&key) {
            return Err(CoreError::validation(format!(
                "two forwards listen on {}:{}",
                key.1, key.2
            )));
        }
        claimed.push(key);
    }
    Ok(())
}

/// The part of a host the jump-chain check needs from a lookup.
#[derive(Debug, Clone)]
pub struct JumpNode {
    pub name: String,
    pub jump_chain: Vec<HostId>,
}

/// Lookup-free checks of a jump chain: its length, that the host does not
/// route through itself and that no hop is listed twice. [`Host::validate`]
/// runs this, so sync can enforce it without knowing the other hosts.
///
/// [`Host::validate`]: crate::host::Host::validate
pub fn validate_jump_chain(host_id: HostId, chain: &[HostId]) -> Result<()> {
    if chain.len() > MAX_JUMP_HOPS {
        return Err(CoreError::validation(format!(
            "a jump chain may have at most {MAX_JUMP_HOPS} hops"
        )));
    }
    for (index, hop) in chain.iter().enumerate() {
        if *hop == host_id {
            return Err(CoreError::validation("a host cannot be its own jump host"));
        }
        if chain[..index].contains(hop) {
            return Err(CoreError::validation(
                "a jump chain lists the same host twice",
            ));
        }
    }
    Ok(())
}

/// The full path a connection takes: every hop, in order, from this machine
/// to the host (the host itself not included).
///
/// The first hop is reached from here, and **its own jump chain is followed
/// first** (as OpenSSH does for `ProxyJump`): reaching `app` through
/// `bastion`, where `bastion` sits behind `gateway`, goes
/// `gateway → bastion → app`. The later hops of a chain are reached from the
/// hop before them, so their own chains are not used.
///
/// Fails when the chain is longer than [`MAX_JUMP_HOPS`], a hop does not
/// exist (`lookup` returns `None`: deleted, or not synced yet), the chain
/// leads back to the host or to a hop it already passed (a cycle).
///
/// Used wherever a host is saved locally and wherever a connection is built.
/// It is deliberately not part of [`crate::host::Host::validate`]: sync
/// delivers records in any order, so a hop may legitimately arrive after the
/// host that names it.
pub fn resolve_jump_chain(
    host_id: HostId,
    host_name: &str,
    chain: &[HostId],
    lookup: &dyn Fn(HostId) -> Option<JumpNode>,
) -> Result<Vec<HostId>> {
    validate_jump_chain(host_id, chain)?;
    let mut path: Vec<HostId> = Vec::new();
    let mut visiting: Vec<HostId> = vec![host_id];
    expand(
        host_name,
        chain,
        lookup,
        &mut visiting,
        &mut path,
        MAX_JUMP_HOPS,
    )?;
    if path.len() > MAX_JUMP_HOPS {
        return Err(CoreError::validation(format!(
            "the route to {host_name} has {} hops; at most {MAX_JUMP_HOPS} are allowed",
            path.len()
        )));
    }
    for (index, hop) in path.iter().enumerate() {
        if *hop == host_id {
            return Err(CoreError::validation(format!(
                "the route to {host_name} leads back to {host_name}"
            )));
        }
        if path[..index].contains(hop) {
            return Err(CoreError::validation(format!(
                "the route to {host_name} passes the same jump host twice"
            )));
        }
    }
    Ok(path)
}

fn expand(
    owner: &str,
    chain: &[HostId],
    lookup: &dyn Fn(HostId) -> Option<JumpNode>,
    visiting: &mut Vec<HostId>,
    path: &mut Vec<HostId>,
    budget: usize,
) -> Result<()> {
    let Some((first, rest)) = chain.split_first() else {
        return Ok(());
    };
    // A cycle never leaves the chain of first hops, so that is the walk
    // that needs the visited set; the budget bounds the depth of a chain
    // that is long but not a cycle.
    if visiting.contains(first) {
        return Err(CoreError::validation(format!(
            "the jump chain of {owner} is a cycle"
        )));
    }
    if budget == 0 {
        return Err(CoreError::validation(format!(
            "the route to {owner} has more than {MAX_JUMP_HOPS} hops"
        )));
    }
    let node = lookup(*first).ok_or_else(|| {
        CoreError::validation(format!(
            "a jump host of {owner} does not exist (was it deleted?)"
        ))
    })?;
    visiting.push(*first);
    expand(
        &node.name,
        &node.jump_chain,
        lookup,
        visiting,
        path,
        budget - 1,
    )?;
    path.push(*first);
    for hop in rest {
        lookup(*hop).ok_or_else(|| {
            CoreError::validation(format!(
                "a jump host of {owner} does not exist (was it deleted?)"
            ))
        })?;
        path.push(*hop);
    }
    Ok(())
}

/// Escapes `%`, `_`, and `\` before using a string in a SQL `LIKE` pattern.
/// Callers must still parameterize the escape character (`ESCAPE '\'`).
pub fn escape_like_pattern(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_rules() {
        assert!(validate_display_name("name", "prod-db-1", MAX_NAME_LEN).is_ok());
        assert!(validate_display_name("name", "", MAX_NAME_LEN).is_err());
        assert!(validate_display_name("name", "   ", MAX_NAME_LEN).is_err());
        assert!(validate_display_name("name", "bad\nname", MAX_NAME_LEN).is_err());
        assert!(validate_display_name("name", "a\0b", MAX_NAME_LEN).is_err());
        assert!(
            validate_display_name("name", &"x".repeat(MAX_NAME_LEN + 1), MAX_NAME_LEN).is_err()
        );
    }

    #[test]
    fn address_rules() {
        assert!(validate_address("example.com").is_ok());
        assert!(validate_address("192.168.1.10").is_ok());
        assert!(validate_address("fe80::1").is_ok());
        assert!(validate_address("internal_host-1.corp").is_ok());
        assert!(validate_address("").is_err());
        assert!(validate_address("host name").is_err());
        assert!(validate_address("host/name").is_err());
        assert!(validate_address("-bad.com").is_err());
        assert!(validate_address("host;rm -rf").is_err());
        assert!(validate_address("host$(x)").is_err());
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("deploy").is_ok());
        assert!(validate_username("svc-account").is_ok());
        assert!(validate_username("").is_err());
        assert!(validate_username("user name").is_err());
        assert!(validate_username("user;id").is_err());
    }

    #[test]
    fn port_rules() {
        assert!(validate_port(22).is_ok());
        assert!(validate_port(65535).is_ok());
        assert!(validate_port(0).is_err());
    }

    #[test]
    fn term_rules() {
        assert!(validate_term("xterm-256color").is_ok());
        assert!(validate_term("screen").is_ok());
        assert!(validate_term("xterm;rm").is_err());
        assert!(validate_term("").is_err());
    }

    #[test]
    fn keepalive_rules() {
        assert!(validate_keepalive(0).is_ok());
        assert!(validate_keepalive(30).is_ok());
        assert!(validate_keepalive(3_601).is_err());
    }

    #[test]
    fn ipv6_literals_with_zone_ids_are_addresses() {
        for ok in [
            "::1",
            "fe80::1",
            "fe80::1%eth0",
            "fe80::1%3",
            "fe80::1%enp3s0.100",
            "2001:db8::ff00:42:8329",
            "::ffff:192.0.2.1",
            "[::1]",
            "[fe80::1%eth0]",
        ] {
            assert!(validate_address(ok).is_ok(), "{ok} must be valid");
        }
        for bad in [
            "fe80::1%",
            "fe80::1%eth 0",
            "fe80::1%eth0%1",
            "fe80::1%e/th0",
            "fe80::1%eth0;id",
            "host%eth0",
            "[fe80::1",
            "fe80::1]",
            "[host.example]",
            "[::1]:22",
            "%eth0",
            "fe80::zz%eth0",
        ] {
            assert!(validate_address(bad).is_err(), "{bad} must be refused");
        }
        let long_zone = format!("fe80::1%{}", "z".repeat(MAX_ZONE_ID_LEN + 1));
        assert!(validate_address(&long_zone).is_err());
    }

    /// Hosts saved before IPv6 parsing existed keep validating: a synced old
    /// record must not fail on another device.
    #[test]
    fn addresses_that_were_valid_stay_valid() {
        for old in ["host:22", "a:b:c", "web-1.example.com", "10.0.0.1"] {
            assert!(validate_address(old).is_ok(), "{old}");
        }
        // The new forward fields are strict about colons.
        assert!(validate_host_field("target host", "host:22").is_err());
        assert!(validate_host_field("target host", "::1").is_ok());
        assert!(validate_host_field("target host", "db.internal").is_ok());
    }

    #[test]
    fn brackets_are_stripped_only_when_they_wrap_the_whole_value() {
        assert_eq!(strip_brackets("[::1]"), "::1");
        assert_eq!(strip_brackets("::1"), "::1");
        assert_eq!(strip_brackets("[::1"), "[::1");
        assert_eq!(strip_brackets("host"), "host");
    }

    fn fwd(kind: ForwardKind, bind_port: u16) -> Forward {
        Forward {
            kind,
            bind_address: None,
            bind_port,
            target_host: None,
            target_port: None,
            enabled: true,
        }
    }

    fn local(bind_port: u16, host: &str, port: u16) -> Forward {
        Forward {
            target_host: Some(host.to_string()),
            target_port: Some(port),
            ..fwd(ForwardKind::Local, bind_port)
        }
    }

    #[test]
    fn forward_rules_depend_on_the_kind() {
        assert!(validate_forward(&local(8080, "db.internal", 5432)).is_ok());
        assert!(validate_forward(&fwd(ForwardKind::Dynamic, 1080)).is_ok());
        let remote = Forward {
            target_host: Some("127.0.0.1".into()),
            target_port: Some(3000),
            ..fwd(ForwardKind::Remote, 9000)
        };
        assert!(validate_forward(&remote).is_ok());

        // Local and remote need a complete target.
        assert!(validate_forward(&fwd(ForwardKind::Local, 8080)).is_err());
        assert!(validate_forward(&fwd(ForwardKind::Remote, 8080)).is_err());
        let mut no_port = local(8080, "db", 5432);
        no_port.target_port = None;
        assert!(validate_forward(&no_port).is_err());
        no_port.target_port = Some(0);
        assert!(validate_forward(&no_port).is_err());

        // A dynamic forward has no target.
        let mut dynamic = fwd(ForwardKind::Dynamic, 1080);
        dynamic.target_host = Some("example.com".into());
        assert!(validate_forward(&dynamic).is_err());
        let mut dynamic = fwd(ForwardKind::Dynamic, 1080);
        dynamic.target_port = Some(80);
        assert!(validate_forward(&dynamic).is_err());

        // Ports.
        assert!(validate_forward(&local(0, "db", 5432)).is_err());
        assert!(validate_forward(&local(65535, "db", 65535)).is_ok());
    }

    #[test]
    fn forward_hosts_are_checked() {
        for bad in ["", "host name", "db;id", "host:22", "[db]", "-db"] {
            assert!(
                validate_forward(&local(8080, bad, 22)).is_err(),
                "target {bad:?}"
            );
        }
        for good in ["db", "::1", "[::1]", "fe80::1%eth0", "10.0.0.5"] {
            assert!(
                validate_forward(&local(8080, good, 22)).is_ok(),
                "target {good:?}"
            );
        }
    }

    #[test]
    fn local_listeners_bind_addresses_but_remote_ones_may_name_a_host() {
        let mut forward = local(8080, "db", 22);
        for bind in [
            "127.0.0.1",
            "::1",
            "[::1]",
            "0.0.0.0",
            "::",
            "localhost",
            "fe80::1%eth0",
        ] {
            forward.bind_address = Some(bind.into());
            assert!(validate_forward(&forward).is_ok(), "bind {bind}");
        }
        for bind in ["", "example.com", "127.0.0.1;id", "*"] {
            forward.bind_address = Some(bind.into());
            assert!(validate_forward(&forward).is_err(), "bind {bind:?}");
        }
        let mut dynamic = fwd(ForwardKind::Dynamic, 1080);
        dynamic.bind_address = Some("example.com".into());
        assert!(validate_forward(&dynamic).is_err());

        let mut remote = Forward {
            target_host: Some("127.0.0.1".into()),
            target_port: Some(3000),
            ..fwd(ForwardKind::Remote, 9000)
        };
        remote.bind_address = Some("gateway.internal".into());
        assert!(validate_forward(&remote).is_ok());
    }

    #[test]
    fn forwards_may_not_share_a_listener() {
        let a = local(8080, "db", 5432);
        let b = local(8081, "db", 5432);
        assert!(validate_forwards(&[a.clone(), b]).is_ok());
        assert!(validate_forwards(&[a.clone(), a.clone()]).is_err());
        // `localhost` is the default loopback address.
        let mut named = local(8080, "other", 1);
        named.bind_address = Some("localhost".into());
        assert!(validate_forwards(&[a.clone(), named]).is_err());
        // Local and dynamic share one port space.
        assert!(validate_forwards(&[a, fwd(ForwardKind::Dynamic, 8080)]).is_err());
        // A remote listener lives on the server: no clash with a local one.
        let remote = Forward {
            target_host: Some("127.0.0.1".into()),
            target_port: Some(3000),
            ..fwd(ForwardKind::Remote, 8080)
        };
        assert!(validate_forwards(&[local(8080, "db", 5432), remote]).is_ok());
    }

    #[test]
    fn forward_count_is_bounded() {
        let many: Vec<Forward> = (0..=MAX_FORWARDS_PER_HOST)
            .map(|index| local(1024 + index as u16, "db", 22))
            .collect();
        assert!(validate_forwards(&many[..MAX_FORWARDS_PER_HOST]).is_ok());
        assert!(validate_forwards(&many).is_err());
    }

    fn var(name: &str, value: &str) -> EnvVar {
        EnvVar {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn env_names_follow_the_posix_rule() {
        for ok in ["LANG", "_X", "a1", "MY_VAR_2", "x"] {
            assert!(validate_env(&[var(ok, "v")]).is_ok(), "{ok}");
        }
        for bad in ["", "1A", "A-B", "A B", "A=B", "ÄB", "A\n", "a.b", "A$"] {
            assert!(validate_env(&[var(bad, "v")]).is_err(), "{bad:?}");
        }
        assert!(validate_env(&[var(&"A".repeat(MAX_ENV_NAME_LEN), "v")]).is_ok());
        assert!(validate_env(&[var(&"A".repeat(MAX_ENV_NAME_LEN + 1), "v")]).is_err());
    }

    #[test]
    fn env_values_and_counts_are_bounded() {
        assert!(validate_env(&[var("A", "")]).is_ok());
        assert!(
            validate_env(&[var("A", "tab\tis fine")]).is_err(),
            "tabs are control characters"
        );
        assert!(validate_env(&[var("A", "line\nbreak")]).is_err());
        assert!(validate_env(&[var("A", "nul\0")]).is_err());
        assert!(validate_env(&[var("A", &"x".repeat(MAX_ENV_VALUE_LEN))]).is_ok());
        assert!(validate_env(&[var("A", &"x".repeat(MAX_ENV_VALUE_LEN + 1))]).is_err());
        assert!(validate_env(&[var("A", "1"), var("A", "2")]).is_err());
        let many: Vec<EnvVar> = (0..=MAX_ENV_VARS)
            .map(|n| var(&format!("V{n}"), "x"))
            .collect();
        assert!(validate_env(&many[..MAX_ENV_VARS]).is_ok());
        assert!(validate_env(&many).is_err());
    }

    #[test]
    fn startup_command_is_one_line() {
        assert!(validate_startup_command("cd /srv/app && ls -la").is_ok());
        assert!(validate_startup_command("echo\thello").is_ok());
        assert!(validate_startup_command("").is_err());
        assert!(validate_startup_command("   ").is_err());
        assert!(validate_startup_command("ls\nrm -rf /").is_err());
        assert!(validate_startup_command("ls\r").is_err());
        assert!(validate_startup_command("echo \u{1b}[2J").is_err());
        assert!(validate_startup_command("a\0b").is_err());
        assert!(validate_startup_command(&"x".repeat(MAX_STARTUP_COMMAND_LEN)).is_ok());
        assert!(validate_startup_command(&"x".repeat(MAX_STARTUP_COMMAND_LEN + 1)).is_err());
    }

    #[test]
    fn auth_order_names_each_method_once() {
        use AuthKind::*;
        assert!(validate_auth_order(&[Agent, Password]).is_ok());
        assert!(validate_auth_order(&[Agent, PrivateKey, Password, KeyboardInteractive]).is_ok());
        assert!(validate_auth_order(&[]).is_err());
        assert!(validate_auth_order(&[Agent, Agent]).is_err());
    }

    fn node(name: &str, chain: &[HostId]) -> JumpNode {
        JumpNode {
            name: name.into(),
            jump_chain: chain.to_vec(),
        }
    }

    struct Graph(std::collections::HashMap<HostId, JumpNode>);

    impl Graph {
        fn new(nodes: &[(HostId, &str, &[HostId])]) -> Self {
            Self(
                nodes
                    .iter()
                    .map(|(id, name, chain)| (*id, node(name, chain)))
                    .collect(),
            )
        }

        fn resolve(&self, id: HostId, chain: &[HostId]) -> Result<Vec<HostId>> {
            resolve_jump_chain(id, "target", chain, &|hop| self.0.get(&hop).cloned())
        }
    }

    #[test]
    fn jump_chain_structure_is_checked_without_a_lookup() {
        let (me, a, b) = (HostId::new(), HostId::new(), HostId::new());
        assert!(validate_jump_chain(me, &[]).is_ok());
        assert!(validate_jump_chain(me, &[a, b]).is_ok());
        assert!(validate_jump_chain(me, &[me]).is_err(), "self reference");
        assert!(
            validate_jump_chain(me, &[a, me]).is_err(),
            "self reference mid-chain"
        );
        assert!(
            validate_jump_chain(me, &[a, b, a]).is_err(),
            "duplicate hop"
        );
        let long: Vec<HostId> = (0..=MAX_JUMP_HOPS).map(|_| HostId::new()).collect();
        assert!(validate_jump_chain(me, &long[..MAX_JUMP_HOPS]).is_ok());
        assert!(validate_jump_chain(me, &long).is_err(), "too many hops");
    }

    #[test]
    fn a_route_is_the_first_hops_own_route_then_the_hop_then_the_rest() {
        let (target, gateway, bastion, db) =
            (HostId::new(), HostId::new(), HostId::new(), HostId::new());
        // bastion sits behind gateway; db is reached from bastion.
        let graph = Graph::new(&[
            (gateway, "gateway", &[]),
            (bastion, "bastion", &[gateway]),
            (db, "db", &[]),
        ]);
        assert_eq!(graph.resolve(target, &[]).unwrap(), Vec::<HostId>::new());
        assert_eq!(graph.resolve(target, &[gateway]).unwrap(), vec![gateway]);
        assert_eq!(
            graph.resolve(target, &[bastion]).unwrap(),
            vec![gateway, bastion]
        );
        assert_eq!(
            graph.resolve(target, &[bastion, db]).unwrap(),
            vec![gateway, bastion, db]
        );
    }

    #[test]
    fn a_hops_own_route_is_only_followed_for_the_first_hop() {
        let (target, a, b, c) = (HostId::new(), HostId::new(), HostId::new(), HostId::new());
        // b would need c, but b is reached from a, which is already inside.
        let graph = Graph::new(&[(a, "a", &[]), (b, "b", &[c]), (c, "c", &[])]);
        assert_eq!(graph.resolve(target, &[a, b]).unwrap(), vec![a, b]);
    }

    #[test]
    fn jump_cycles_are_refused() {
        let (target, a, b) = (HostId::new(), HostId::new(), HostId::new());
        // a -> b -> a
        let graph = Graph::new(&[(a, "a", &[b]), (b, "b", &[a])]);
        let error = graph.resolve(target, &[a]).unwrap_err().to_string();
        assert!(error.contains("cycle"), "{error}");
        // The host being saved closes the loop: target -> a -> target.
        let graph = Graph::new(&[(a, "a", &[target])]);
        assert!(graph.resolve(target, &[a]).is_err());
        // Longer loop through three hosts.
        let c = HostId::new();
        let graph = Graph::new(&[(a, "a", &[b]), (b, "b", &[c]), (c, "c", &[a])]);
        assert!(graph.resolve(target, &[a]).is_err());
    }

    #[test]
    fn a_route_may_not_pass_a_host_twice_or_grow_too_long() {
        let (target, a, b) = (HostId::new(), HostId::new(), HostId::new());
        // a is behind b, and the chain lists both: b, a -> route b, a ... fine,
        // but a behind b and chain [b, b-again] is caught by the structure.
        let graph = Graph::new(&[(a, "a", &[b]), (b, "b", &[])]);
        // chain [b, a]: route is (b's route: none) b, then a => b, a; a's own
        // route (b) is not used for a later hop.
        assert_eq!(graph.resolve(target, &[b, a]).unwrap(), vec![b, a]);
        // chain [a, b]: route is b (a's route), a, b => b twice.
        let error = graph.resolve(target, &[a, b]).unwrap_err().to_string();
        assert!(error.contains("twice"), "{error}");

        // A chain of eight hosts, each behind the next: the route is too long
        // once one more hop is added to it.
        let hosts: Vec<HostId> = (0..=MAX_JUMP_HOPS).map(|_| HostId::new()).collect();
        let entries: Vec<(HostId, &str, Vec<HostId>)> = hosts
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let behind = if index == 0 {
                    vec![]
                } else {
                    vec![hosts[index - 1]]
                };
                (*id, "hop", behind)
            })
            .collect();
        let graph = Graph(
            entries
                .iter()
                .map(|(id, name, chain)| (*id, node(name, chain)))
                .collect(),
        );
        // hosts[7] is behind 7 others: route of 8 hops including itself.
        assert_eq!(
            graph
                .resolve(target, &[hosts[MAX_JUMP_HOPS - 1]])
                .unwrap()
                .len(),
            MAX_JUMP_HOPS
        );
        let error = graph
            .resolve(target, &[hosts[MAX_JUMP_HOPS]])
            .unwrap_err()
            .to_string();
        assert!(error.contains("hops"), "{error}");
    }

    #[test]
    fn a_missing_or_deleted_hop_is_named_as_such() {
        let (target, a, ghost) = (HostId::new(), HostId::new(), HostId::new());
        let graph = Graph::new(&[(a, "a", &[ghost])]);
        for chain in [vec![ghost], vec![a], vec![a, ghost]] {
            let error = graph.resolve(target, &chain).unwrap_err().to_string();
            assert!(error.contains("does not exist"), "{chain:?}: {error}");
        }
    }

    #[test]
    fn like_escaping() {
        assert_eq!(escape_like_pattern("100%_x"), r"100\%\_x");
        assert_eq!(escape_like_pattern("plain"), "plain");
    }
}
