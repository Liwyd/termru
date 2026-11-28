//! Wire types for the revision-based sync protocol.
//!
//! Design summary (see `docs/architecture/0005-sync-protocol.md`):
//!
//! * Every user has a monotonically increasing integer `rev` assigned by the
//!   server for each accepted mutation.
//! * Pull: `GET /v1/sync/changes?since=<rev>` returns records (including
//!   tombstones) with `rev > since`, cursor-paginated, bounded per page.
//! * Push: `POST /v1/sync/changes` submits ordered mutations, each carrying a
//!   client-generated `mutation_id` (idempotency) and `base_rev` (the server
//!   revision the client last observed for that record).
//! * A conflict exists when `base_rev` is stale — i.e. someone else mutated the
//!   record since the client last synced. Conflicts are returned explicitly and
//!   **never** silently resolved by the server.
//! * Payloads are opaque to the server except for size/kind/id validation; the
//!   client re-validates every pulled record before applying it, so a malicious
//!   peer device cannot inject invalid data into another device.
//!
//! Secrets (passwords, passphrases, private keys) are never part of this
//! protocol.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::CoreError;
use crate::host::Host;
use crate::identity::Identity;
use crate::preference::Preferences;
use crate::taxonomy::{Group, Tag};

/// Version of the sync payload schema. Bump on incompatible payload changes
/// and provide a migration path before shipping.
pub const CLIENT_SCHEMA_VERSION: u32 = 1;

/// Hard limits enforced by both client and server.
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_BATCH_MUTATIONS: usize = 500;
pub const MAX_PULL_RECORDS: usize = 500;

/// Sentinel record id for the singleton portable-preferences record.
pub const PREFERENCES_RECORD_ID: &str = "global";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Host,
    Identity,
    Group,
    Tag,
    Preferences,
}

impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Identity => "identity",
            Self::Group => "group",
            Self::Tag => "tag",
            Self::Preferences => "preferences",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "host" => Some(Self::Host),
            "identity" => Some(Self::Identity),
            "group" => Some(Self::Group),
            "tag" => Some(Self::Tag),
            "preferences" => Some(Self::Preferences),
            _ => None,
        }
    }
}

/// Typed view over a record payload. Encoding/decoding always validates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum RecordPayload {
    /// Boxed: a host carries the v2 lists and is by far the largest record.
    Host(Box<Host>),
    Identity(Identity),
    Group(Group),
    Tag(Tag),
    Preferences(Box<Preferences>),
}

impl RecordPayload {
    pub fn kind(&self) -> RecordKind {
        match self {
            Self::Host(_) => RecordKind::Host,
            Self::Identity(_) => RecordKind::Identity,
            Self::Group(_) => RecordKind::Group,
            Self::Tag(_) => RecordKind::Tag,
            Self::Preferences(_) => RecordKind::Preferences,
        }
    }

    /// The record id this payload belongs to.
    pub fn record_id(&self) -> String {
        match self {
            Self::Host(h) => h.id.to_string(),
            Self::Identity(i) => i.id.to_string(),
            Self::Group(g) => g.id.to_string(),
            Self::Tag(t) => t.id.to_string(),
            Self::Preferences(_) => PREFERENCES_RECORD_ID.to_string(),
        }
    }

    /// Entity-level validation (same rules as local writes).
    pub fn validate_entity(&self) -> Result<(), CoreError> {
        match self {
            Self::Host(h) => h.validate(),
            Self::Identity(i) => i.validate(),
            Self::Group(g) => g.validate(),
            Self::Tag(t) => t.validate(),
            Self::Preferences(p) => p.validate(),
        }
    }

    /// Decodes `value` according to `kind` and validates it. Used by the
    /// client on every pulled record and by the client before every push.
    pub fn decode_and_validate(kind: RecordKind, value: &Value) -> Result<Self, CoreError> {
        let payload: RecordPayload = match kind {
            RecordKind::Host => RecordPayload::Host(
                serde_json::from_value(value.clone())
                    .map_err(|e| CoreError::validation(format!("invalid host payload: {e}")))?,
            ),
            RecordKind::Identity => RecordPayload::Identity(
                serde_json::from_value(value.clone())
                    .map_err(|e| CoreError::validation(format!("invalid identity payload: {e}")))?,
            ),
            RecordKind::Group => RecordPayload::Group(
                serde_json::from_value(value.clone())
                    .map_err(|e| CoreError::validation(format!("invalid group payload: {e}")))?,
            ),
            RecordKind::Tag => RecordPayload::Tag(
                serde_json::from_value(value.clone())
                    .map_err(|e| CoreError::validation(format!("invalid tag payload: {e}")))?,
            ),
            RecordKind::Preferences => RecordPayload::Preferences(Box::new(
                serde_json::from_value(value.clone()).map_err(|e| {
                    CoreError::validation(format!("invalid preferences payload: {e}"))
                })?,
            )),
        };
        // The payload's own kind must match the envelope; record-id matching
        // against the envelope id is enforced by `PushMutation::validate_structure`
        // (push) and by the sync client (pull).
        if payload.kind() != kind {
            return Err(CoreError::validation(
                "payload kind does not match envelope",
            ));
        }
        payload.validate_entity()?;
        Ok(payload)
    }
}

/// Record id format check shared by client and server.
pub fn validate_record_id(kind: RecordKind, id: &str) -> Result<(), CoreError> {
    if id.len() > 64 || id.chars().any(char::is_control) {
        return Err(CoreError::validation("record id is malformed"));
    }
    match kind {
        RecordKind::Preferences => {
            if id != PREFERENCES_RECORD_ID {
                return Err(CoreError::validation(
                    "the preferences record must have id 'global'",
                ));
            }
        }
        _ => {
            id.parse::<uuid::Uuid>()
                .map_err(|_| CoreError::validation("record id must be a UUID"))?;
        }
    }
    Ok(())
}

/// A record as returned by a pull. `payload` is `None` for tombstones whose
/// content the server no longer retains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WireRecord {
    pub id: String,
    pub kind: RecordKind,
    pub rev: i64,
    pub updated_at: DateTime<Utc>,
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PullResponse {
    pub records: Vec<WireRecord>,
    /// Cursor to pass as `since` on the next request. Equal to the highest
    /// `rev` included in this page (unchanged when the page is empty).
    pub next_rev: i64,
    pub has_more: bool,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PushRequest {
    pub mutations: Vec<PushMutation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PushMutation {
    pub mutation_id: uuid::Uuid,
    pub id: String,
    pub kind: RecordKind,
    /// Server revision this mutation is based on. `None` = record never seen
    /// from the server (create). Create conflicts when the record already
    /// exists server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_rev: Option<i64>,
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    /// Client wall-clock time of the edit (display only; ordering uses `rev`).
    pub updated_at: DateTime<Utc>,
}

impl PushMutation {
    /// Structural validation shared by client (before push) and server
    /// (before applying). Payload byte limit protects the server.
    pub fn validate_structure(&self) -> Result<(), CoreError> {
        validate_record_id(self.kind, &self.id)?;
        if let Some(base_rev) = self.base_rev
            && base_rev < 0
        {
            return Err(CoreError::validation("base_rev must be non-negative"));
        }
        if self.deleted {
            if self.payload.is_some() {
                return Err(CoreError::validation(
                    "a delete mutation must not carry a payload",
                ));
            }
        } else {
            let payload = self
                .payload
                .as_ref()
                .ok_or_else(|| CoreError::validation("an upsert mutation requires a payload"))?;
            let encoded = serde_json::to_vec(payload)
                .map_err(|_| CoreError::validation("payload is not serializable"))?;
            if encoded.len() > MAX_PAYLOAD_BYTES {
                return Err(CoreError::validation(format!(
                    "payload exceeds {} bytes",
                    MAX_PAYLOAD_BYTES
                )));
            }
            let decoded = RecordPayload::decode_and_validate(self.kind, payload)?;
            if decoded.record_id() != self.id {
                return Err(CoreError::validation(
                    "payload record id does not match mutation id",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PushResponse {
    pub accepted: Vec<AcceptedMutation>,
    pub conflicts: Vec<ConflictEntry>,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AcceptedMutation {
    pub mutation_id: uuid::Uuid,
    pub id: String,
    pub rev: i64,
}

/// The server's copy of a record that conflicted with a pushed mutation.
/// `server_record` is `None` when the server has no record (e.g. it was
/// deleted remotely) — the client must then treat it as a remote deletion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConflictEntry {
    pub mutation_id: uuid::Uuid,
    pub id: String,
    pub kind: RecordKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_record: Option<WireRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::IdentityId;

    fn sample_host() -> Host {
        Host::new("web-1".into(), "web-1.example.com".into(), "deploy".into())
    }

    #[test]
    fn push_upsert_mutation_requires_matching_valid_payload() {
        let host = sample_host();
        let id = host.id.to_string();
        let good = PushMutation {
            mutation_id: uuid::Uuid::new_v4(),
            id: id.clone(),
            kind: RecordKind::Host,
            base_rev: None,
            deleted: false,
            payload: Some(serde_json::to_value(&host).unwrap()),
            updated_at: Utc::now(),
        };
        good.validate_structure().unwrap();

        let mut bad = good.clone();
        bad.id = uuid::Uuid::new_v4().to_string(); // mismatched id
        assert!(bad.validate_structure().is_err());

        let mut bad = good.clone();
        bad.payload = Some(serde_json::json!({"name": "too short"}));
        assert!(bad.validate_structure().is_err());
    }

    #[test]
    fn delete_mutation_must_not_carry_payload() {
        let mut m = PushMutation {
            mutation_id: uuid::Uuid::new_v4(),
            id: sample_host().id.to_string(),
            kind: RecordKind::Host,
            base_rev: Some(3),
            deleted: true,
            payload: None,
            updated_at: Utc::now(),
        };
        m.validate_structure().unwrap();
        m.payload = Some(serde_json::to_value(sample_host()).unwrap());
        assert!(m.validate_structure().is_err());
    }

    #[test]
    fn payload_size_limit_enforced() {
        let mut host = sample_host();
        host.description = Some("x".repeat(MAX_PAYLOAD_BYTES));
        let m = PushMutation {
            mutation_id: uuid::Uuid::new_v4(),
            id: host.id.to_string(),
            kind: RecordKind::Host,
            base_rev: None,
            deleted: false,
            payload: Some(serde_json::to_value(host).unwrap()),
            updated_at: Utc::now(),
        };
        assert!(m.validate_structure().is_err());
    }

    #[test]
    fn preferences_record_id_is_singleton() {
        validate_record_id(RecordKind::Preferences, "global").unwrap();
        assert!(
            validate_record_id(
                RecordKind::Preferences,
                uuid::Uuid::new_v4().to_string().as_str()
            )
            .is_err()
        );
        validate_record_id(RecordKind::Identity, &IdentityId::new().to_string()).unwrap();
        assert!(validate_record_id(RecordKind::Identity, "not-a-uuid").is_err());
    }

    #[test]
    fn payload_decode_rejects_invalid_payloads() {
        // An envelope is not a bare entity payload.
        let envelope = serde_json::json!({"kind": "host", "payload": {}});
        assert!(RecordPayload::decode_and_validate(RecordKind::Host, &envelope).is_err());

        // Entity-level validation runs after decoding.
        let invalid_tag = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": ""
        });
        assert!(RecordPayload::decode_and_validate(RecordKind::Tag, &invalid_tag).is_err());

        let host = sample_host();
        let value = serde_json::to_value(&host).unwrap();
        RecordPayload::decode_and_validate(RecordKind::Host, &value).unwrap();
    }

    /// The new host fields travel through the sync protocol unchanged, and the
    /// server-side structural validation (which runs the same entity rules)
    /// accepts them.
    #[test]
    fn v2_host_fields_roundtrip_through_a_push_mutation() {
        use crate::host::{AuthKind, EnvVar, Forward, ForwardKind};
        let mut host = sample_host();
        host.jump_chain = vec![crate::ids::HostId::new()];
        host.forwards = vec![Forward {
            kind: ForwardKind::Remote,
            bind_address: None,
            bind_port: 9000,
            target_host: Some("127.0.0.1".into()),
            target_port: Some(3000),
            enabled: true,
        }];
        host.env = vec![EnvVar {
            name: "TERMRU_PROFILE".into(),
            value: "ops".into(),
        }];
        host.startup_command = Some("uptime".into());
        host.agent_forward = true;
        host.auth_order = Some(vec![AuthKind::Agent, AuthKind::Password]);
        let mutation = PushMutation {
            mutation_id: uuid::Uuid::new_v4(),
            id: host.id.to_string(),
            kind: RecordKind::Host,
            base_rev: None,
            deleted: false,
            payload: Some(serde_json::to_value(&host).unwrap()),
            updated_at: Utc::now(),
        };
        mutation.validate_structure().unwrap();
        let wire = serde_json::to_string(&mutation).unwrap();
        let back: PushMutation = serde_json::from_str(&wire).unwrap();
        let decoded =
            RecordPayload::decode_and_validate(RecordKind::Host, back.payload.as_ref().unwrap())
                .unwrap();
        let RecordPayload::Host(decoded) = decoded else {
            panic!("a host payload decodes to a host");
        };
        assert_eq!(decoded.jump_chain, host.jump_chain);
        assert_eq!(decoded.forwards, host.forwards);
        assert_eq!(decoded.env, host.env);
        assert_eq!(decoded.startup_command, host.startup_command);
        assert!(decoded.agent_forward);
        assert_eq!(decoded.auth_order, host.auth_order);
    }

    /// The payload an older device pushes (no v2 fields) is accepted by the
    /// current validation, and an invalid v2 payload is refused on the way in.
    #[test]
    fn old_host_payloads_are_accepted_and_bad_v2_ones_are_not() {
        let old = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": "legacy",
            "address": "legacy.example.com",
            "port": 22,
            "username": "root",
            "auth": {"type": "agent"},
            "tags": [],
            "favorite": false,
            "term": "xterm-256color",
            "keepalive_seconds": 30,
            "sort_order": 0,
            "created_at": "2025-01-01T00:00:00Z",
            "updated_at": "2025-01-01T00:00:00Z",
        });
        RecordPayload::decode_and_validate(RecordKind::Host, &old).unwrap();

        let mut bad = old.clone();
        bad["forwards"] = serde_json::json!([
            {"kind": "dynamic", "bind_port": 1080, "target_host": "x", "target_port": 1}
        ]);
        assert!(RecordPayload::decode_and_validate(RecordKind::Host, &bad).is_err());
        let mut bad = old;
        bad["env"] = serde_json::json!([{"name": "A B", "value": "x"}]);
        assert!(RecordPayload::decode_and_validate(RecordKind::Host, &bad).is_err());
    }
}
