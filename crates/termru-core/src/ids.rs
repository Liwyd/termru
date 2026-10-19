//! Typed identifiers for all domain entities.
//!
//! IDs are UUIDv4 values serialized as lowercase canonical strings. The
//! newtype wrappers prevent accidentally passing a `HostId` where a
//! `GroupId` is expected.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<Uuid>().map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                s.parse::<Uuid>().map(Self).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(
    /// Identifies a connection host profile.
    HostId
);
define_id!(
    /// Identifies a folder/group in the host tree.
    GroupId
);
define_id!(
    /// Identifies a logical SSH identity (key reference).
    IdentityId
);
define_id!(
    /// Identifies a tag.
    TagId
);
define_id!(
    /// Identifies a saved workspace layout.
    WorkspaceId
);
define_id!(
    /// Identifies a live terminal session (never persisted).
    SessionId
);
define_id!(
    /// Identifies an OS window label (frontend-assigned).
    WindowId
);
define_id!(
    /// Identifies a pane inside a tab split layout.
    PaneId
);
define_id!(
    /// Identifies a tab inside a window.
    TabId
);
define_id!(
    /// Identifies an enrolled device for cloud sync.
    DeviceId
);
define_id!(
    /// Identifies a client-generated mutation for idempotent sync pushes.
    MutationId
);
define_id!(
    /// Identifies a sync conflict surfaced to the user.
    ConflictId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_roundtrips_through_serde_as_string() {
        let id = HostId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.starts_with('"') && json.ends_with('"'));
        let back: HostId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn id_rejects_invalid_string() {
        assert!("not-a-uuid".parse::<HostId>().is_err());
        assert!(serde_json::from_str::<HostId>("\"not-a-uuid\"").is_err());
    }
}
