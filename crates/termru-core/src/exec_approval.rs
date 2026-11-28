//! The part of a host that makes a connection **act**, and the hash a device
//! uses to remember that its user approved it.
//!
//! A host is a synced record, so whoever can write to the sync account can
//! change what every device does when it connects: type a command into the
//! shell, hand the local agent to the server, set environment variables, open
//! a listener that is reachable from the network. Those four things are the
//! "exec-like" fields. Each device keeps, **locally and never synced**, the
//! hash of the exec-like fields its user approved for a host (saving the host
//! in the editor approves them). A connection that finds a different hash
//! connects without them (see `docs/architecture/0010-data-model-v2.md`).
//!
//! What is hashed is exactly what is withheld: the startup command, the agent
//! flag, the environment and the **enabled forwards that do not bind
//! loopback**. Forwards on loopback are not exec-like (they are reachable
//! from this machine only) and always run.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::host::{EnvVar, Forward, Host};
use crate::validate::strip_brackets;

/// Prefixed to what is hashed so the hash of these fields cannot be mistaken
/// for any other hash, and so a future change of the format is a visible one.
const HASH_DOMAIN: &str = "termru-exec-approval-v1\n";

/// The exec-like fields of one host, as hashed, reviewed and withheld.
///
/// The field order of this struct is the order of the hashed JSON: do not
/// reorder or rename without a new [`HASH_DOMAIN`] (a golden test pins it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecFields {
    /// Typed into the shell once it starts.
    pub startup_command: Option<String>,
    /// The server may use the local SSH agent.
    pub agent_forward: bool,
    /// Variables requested from the server.
    pub env: Vec<EnvVar>,
    /// The enabled forwards whose listener is not on loopback only.
    pub forwards: Vec<Forward>,
}

impl ExecFields {
    /// True when the host asks for nothing that needs approval.
    pub fn is_empty(&self) -> bool {
        self.startup_command.is_none()
            && !self.agent_forward
            && self.env.is_empty()
            && self.forwards.is_empty()
    }

    /// The SHA-256 of the fields, as lowercase hex. Stable across versions of
    /// this program and across devices.
    pub fn hash(&self) -> String {
        // Serializing plain strings, numbers and booleans cannot fail.
        let json = serde_json::to_string(self).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(HASH_DOMAIN.as_bytes());
        hasher.update(json.as_bytes());
        hasher
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }
}

impl Forward {
    /// Whether the listener is reachable from this machine (or, for a remote
    /// forward, from the server itself) only: no bind address, `localhost`,
    /// or a loopback IP literal. Anything else, a name that is not
    /// `localhost` included, counts as exposed.
    pub fn binds_loopback(&self) -> bool {
        let Some(address) = self.bind_address.as_deref() else {
            return true;
        };
        let address = strip_brackets(address);
        if address.eq_ignore_ascii_case("localhost") {
            return true;
        }
        let literal = address.split_once('%').map_or(address, |(ip, _zone)| ip);
        literal
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.to_canonical().is_loopback())
    }
}

impl Host {
    /// The fields that need this device's approval before a connection uses
    /// them.
    pub fn exec_fields(&self) -> ExecFields {
        ExecFields {
            startup_command: self.startup_command.clone(),
            agent_forward: self.agent_forward,
            env: self.env.clone(),
            forwards: self
                .forwards
                .iter()
                .filter(|forward| forward.enabled && !forward.binds_loopback())
                .cloned()
                .collect(),
        }
    }

    /// The hash of [`Host::exec_fields`].
    pub fn exec_hash(&self) -> String {
        self.exec_fields().hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::ForwardKind;

    fn host() -> Host {
        Host::new("web".into(), "web.example.com".into(), "deploy".into())
    }

    fn forward(kind: ForwardKind, bind: Option<&str>, port: u16) -> Forward {
        Forward {
            kind,
            bind_address: bind.map(str::to_string),
            bind_port: port,
            target_host: (kind != ForwardKind::Dynamic).then(|| "db".to_string()),
            target_port: (kind != ForwardKind::Dynamic).then_some(5432),
            enabled: true,
        }
    }

    fn rich() -> Host {
        let mut host = host();
        host.startup_command = Some("tmux attach".into());
        host.agent_forward = true;
        host.env = vec![EnvVar {
            name: "LANG".into(),
            value: "C.UTF-8".into(),
        }];
        host.forwards = vec![
            forward(ForwardKind::Local, None, 8080),
            forward(ForwardKind::Local, Some("0.0.0.0"), 9090),
        ];
        host
    }

    #[test]
    fn a_plain_host_has_nothing_to_approve() {
        assert!(host().exec_fields().is_empty());
        let mut with_loopback = host();
        with_loopback.forwards = vec![
            forward(ForwardKind::Local, None, 8080),
            forward(ForwardKind::Dynamic, Some("127.0.0.1"), 1080),
            forward(ForwardKind::Local, Some("localhost"), 8081),
            forward(ForwardKind::Local, Some("[::1]"), 8082),
        ];
        assert!(with_loopback.exec_fields().is_empty());
    }

    #[test]
    fn the_hash_is_stable_across_versions_and_devices() {
        // Pinned: a change here means every device asks for approval again.
        // If the format has to change, bump `HASH_DOMAIN` and say so.
        // That is the SHA-256 of `HASH_DOMAIN` followed by this JSON (checked
        // against an independent implementation when it was pinned):
        // {"startup_command":"tmux attach","agent_forward":true,
        //  "env":[{"name":"LANG","value":"C.UTF-8"}],"forwards":[{"kind":"local",
        //  "bind_address":"0.0.0.0","bind_port":9090,"target_host":"db",
        //  "target_port":5432,"enabled":true}]}
        assert_eq!(
            rich().exec_hash(),
            "7ac2f6de3759c3fd8d3faa9c237f93b9d1284d5746db34e10621c56c5bffa48d",
            "the hash format changed"
        );
        assert_eq!(rich().exec_hash(), rich().exec_hash());
        assert_eq!(rich().exec_hash().len(), 64);
    }

    #[test]
    fn what_is_hashed_is_what_is_reviewed() {
        let fields = rich().exec_fields();
        assert_eq!(fields.startup_command.as_deref(), Some("tmux attach"));
        assert!(fields.agent_forward);
        assert_eq!(fields.env.len(), 1);
        // Only the forward that is not loopback.
        assert_eq!(fields.forwards.len(), 1);
        assert_eq!(fields.forwards[0].bind_port, 9090);
    }

    #[test]
    fn every_exec_field_changes_the_hash() {
        let base = rich().exec_hash();
        let mut changed = rich();
        changed.startup_command = Some("tmux attach -d".into());
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.startup_command = None;
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.agent_forward = false;
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.env[0].value = "C".into();
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.env.push(EnvVar {
            name: "A".into(),
            value: "b".into(),
        });
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.forwards[1].bind_port = 9091;
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.forwards[1].bind_address = Some("192.168.1.5".into());
        assert_ne!(changed.exec_hash(), base);
        let mut changed = rich();
        changed.forwards[1].target_host = Some("other".into());
        assert_ne!(changed.exec_hash(), base);
        // Turning the exposed forward off removes it from the set.
        let mut changed = rich();
        changed.forwards[1].enabled = false;
        assert_ne!(changed.exec_hash(), base);
    }

    #[test]
    fn what_a_connection_never_withholds_does_not_change_the_hash() {
        let base = rich().exec_hash();
        let mut changed = rich();
        changed.name = "renamed".into();
        changed.address = "elsewhere.example.com".into();
        changed.port = 2222;
        changed.favorite = true;
        changed.keepalive_seconds = 99;
        changed.description = Some("note".into());
        // A loopback forward is not exec-like, so adding one needs no approval.
        changed
            .forwards
            .push(forward(ForwardKind::Dynamic, None, 1080));
        // Nor does a disabled exposed one.
        let mut off = forward(ForwardKind::Local, Some("0.0.0.0"), 9999);
        off.enabled = false;
        changed.forwards.push(off);
        assert_eq!(changed.exec_hash(), base);
    }

    #[test]
    fn the_hash_does_not_depend_on_unrelated_serde_details() {
        // Deserializing the same host from sync JSON gives the same hash.
        let json = serde_json::to_string(&rich()).unwrap();
        let back: Host = serde_json::from_str(&json).unwrap();
        assert_eq!(back.exec_hash(), rich().exec_hash());
    }

    #[test]
    fn loopback_is_decided_from_the_address() {
        let bind = |address: Option<&str>| forward(ForwardKind::Local, address, 80);
        for loopback in [
            None,
            Some("localhost"),
            Some("LOCALHOST"),
            Some("127.0.0.1"),
            Some("127.1.2.3"),
            Some("::1"),
            Some("[::1]"),
            Some("::1%lo"),
            Some("::ffff:127.0.0.1"),
        ] {
            assert!(bind(loopback).binds_loopback(), "{loopback:?}");
        }
        for exposed in [
            Some("0.0.0.0"),
            Some("::"),
            Some("192.168.1.5"),
            Some("10.0.0.1"),
            Some("fe80::1%eth0"),
            Some("gateway.example.com"),
            Some("*"),
            Some(""),
        ] {
            assert!(!bind(exposed).binds_loopback(), "{exposed:?}");
        }
    }
}
