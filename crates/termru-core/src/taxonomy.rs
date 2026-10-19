//! Folders (groups) and tags for organizing hosts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::ids::{GroupId, TagId};
use crate::validate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    /// Parent folder for nesting. Depth is capped by
    /// [`validate::MAX_GROUP_DEPTH`] (enforced by the store when re-parenting).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<GroupId>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip)]
    pub server_rev: Option<i64>,
    #[serde(skip)]
    pub dirty: bool,
}

impl Group {
    pub fn new(name: String) -> Self {
        let now = Utc::now();
        Self {
            id: GroupId::new(),
            name,
            parent_id: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
            deleted: false,
            server_rev: None,
            dirty: false,
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        validate::validate_display_name("group name", &self.name, validate::MAX_NAME_LEN)?;
        if Some(self.id) == self.parent_id {
            return Err(CoreError::validation("a group cannot be its own parent"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    /// Optional UI color (hex `#rrggbb`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip)]
    pub server_rev: Option<i64>,
    #[serde(skip)]
    pub dirty: bool,
}

impl Tag {
    pub fn new(name: String) -> Self {
        let now = Utc::now();
        Self {
            id: TagId::new(),
            name,
            color: None,
            created_at: now,
            updated_at: now,
            deleted: false,
            server_rev: None,
            dirty: false,
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        validate::validate_display_name("tag name", &self.name, validate::MAX_TAG_NAME_LEN)?;
        if let Some(color) = &self.color {
            let ok = color.len() == 7
                && color.starts_with('#')
                && color[1..].chars().all(|c| c.is_ascii_hexdigit());
            if !ok {
                return Err(CoreError::validation(
                    "tag color must be a hex color like '#4f8cff'",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_rejects_self_parent() {
        let mut g = Group::new("prod".to_string());
        g.validate().unwrap();
        g.parent_id = Some(g.id);
        assert!(g.validate().is_err());
    }

    #[test]
    fn tag_color_rules() {
        let mut t = Tag::new("urgent".to_string());
        t.color = Some("#4f8cff".to_string());
        t.validate().unwrap();
        t.color = Some("blue".to_string());
        assert!(t.validate().is_err());
        t.color = Some("#xyzxyz".to_string());
        assert!(t.validate().is_err());
    }
}
