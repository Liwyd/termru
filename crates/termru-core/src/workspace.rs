//! Workspace / window / tab / split layout persistence types.
//!
//! Layouts are **device-local**: they reference hosts (which are synced) but
//! window geometry and pane structure only make sense on the machine that
//! created them. The document is restored on launch; live SSH sessions are
//! never claimed to survive a restart — panes with a host reconnect on demand.
//!
//! Documents are validated defensively: they may later arrive from a sync
//! payload or a corrupted file, so bounds are enforced before rendering.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::ids::{HostId, PaneId, TabId, WindowId};

pub const WORKSPACE_DOC_VERSION: u32 = 1;
pub const MAX_WINDOWS: usize = 16;
pub const MAX_TABS_PER_WINDOW: usize = 64;
pub const MAX_PANES_PER_TAB: usize = 64;
pub const MAX_LAYOUT_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceDoc {
    pub version: u32,
    #[serde(default)]
    pub windows: Vec<WindowLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window: Option<WindowId>,
    /// The page the user was on (`hosts`, `sessions`, …). Additive: a
    /// document written before routing existed has no value and loads fine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_route: Option<String>,
}

impl Default for WorkspaceDoc {
    fn default() -> Self {
        Self {
            version: WORKSPACE_DOC_VERSION,
            windows: Vec::new(),
            active_window: None,
            last_route: None,
        }
    }
}

/// The routes the workspace document may name. Anything else is rejected:
/// the document may arrive from a corrupted file or a future client.
pub const KNOWN_ROUTES: [&str; 7] = [
    "hosts",
    "sessions",
    "folders",
    "identities",
    "sync",
    "preferences",
    "host-edit",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WindowLayout {
    pub id: WindowId,
    #[serde(default)]
    pub tabs: Vec<TabLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab: Option<TabId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<WindowBounds>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TabLayout {
    pub id: TabId,
    /// User-provided tab title; when absent the frontend derives it from the
    /// active pane's host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub root: PaneNode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_pane: Option<PaneId>,
}

/// A pane either displays no session yet (empty) or is bound to a host profile
/// that the frontend reconnects when the layout is restored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneNode {
    Leaf {
        pane: PaneId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_id: Option<HostId>,
    },
    Split {
        direction: SplitDirection,
        /// Fraction of space given to `first`, in (0, 1).
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

/// `Columns` = panes side by side (left/right); `Rows` = stacked (top/bottom).
/// Named unambiguously to avoid the classic "horizontal split" ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Columns,
    Rows,
}

impl WorkspaceDoc {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.version == 0 || self.version > WORKSPACE_DOC_VERSION {
            return Err(CoreError::validation(format!(
                "unsupported workspace document version {}",
                self.version
            )));
        }
        if self.windows.len() > MAX_WINDOWS {
            return Err(CoreError::validation(format!(
                "a workspace may contain at most {MAX_WINDOWS} windows"
            )));
        }
        if let Some(active) = self.active_window
            && !self.windows.iter().any(|w| w.id == active)
        {
            return Err(CoreError::validation("active_window does not exist"));
        }
        if let Some(route) = &self.last_route
            && !KNOWN_ROUTES.contains(&route.as_str())
        {
            return Err(CoreError::validation("unknown last_route"));
        }
        for window in &self.windows {
            if window.tabs.len() > MAX_TABS_PER_WINDOW {
                return Err(CoreError::validation(format!(
                    "a window may contain at most {MAX_TABS_PER_WINDOW} tabs"
                )));
            }
            if let Some(active) = window.active_tab
                && !window.tabs.iter().any(|t| t.id == active)
            {
                return Err(CoreError::validation("active_tab does not exist"));
            }
            for tab in &window.tabs {
                if let Some(title) = &tab.title {
                    if title.chars().count() > 128 || title.chars().any(char::is_control) {
                        return Err(CoreError::validation("tab title is invalid"));
                    }
                }
                let mut pane_count = 0usize;
                validate_node(&tab.root, 0, &mut pane_count)?;
                if pane_count > MAX_PANES_PER_TAB {
                    return Err(CoreError::validation(format!(
                        "a tab may contain at most {MAX_PANES_PER_TAB} panes"
                    )));
                }
                if let Some(active) = tab.active_pane
                    && !node_contains_pane(&tab.root, active)
                {
                    return Err(CoreError::validation("active_pane does not exist"));
                }
            }
        }
        Ok(())
    }
}

fn validate_node(node: &PaneNode, depth: usize, panes: &mut usize) -> Result<(), CoreError> {
    if depth > MAX_LAYOUT_DEPTH {
        return Err(CoreError::validation(format!(
            "split layout exceeds maximum depth of {MAX_LAYOUT_DEPTH}"
        )));
    }
    match node {
        PaneNode::Leaf { .. } => {
            *panes += 1;
            Ok(())
        }
        PaneNode::Split {
            ratio,
            first,
            second,
            ..
        } => {
            if !ratio.is_finite() || *ratio <= 0.05 || *ratio >= 0.95 {
                return Err(CoreError::validation(
                    "split ratio must be between 0.05 and 0.95",
                ));
            }
            validate_node(first, depth + 1, panes)?;
            validate_node(second, depth + 1, panes)
        }
    }
}

fn node_contains_pane(node: &PaneNode, pane: PaneId) -> bool {
    match node {
        PaneNode::Leaf { pane: p, .. } => *p == pane,
        PaneNode::Split { first, second, .. } => {
            node_contains_pane(first, pane) || node_contains_pane(second, pane)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(host: Option<HostId>) -> PaneNode {
        PaneNode::Leaf {
            pane: PaneId::new(),
            host_id: host,
        }
    }

    #[test]
    fn default_document_is_valid() {
        WorkspaceDoc::default().validate().unwrap();
    }

    #[test]
    fn rejects_unknown_active_tab() {
        let mut doc = WorkspaceDoc::default();
        doc.windows.push(WindowLayout {
            id: WindowId::new(),
            tabs: vec![],
            active_tab: Some(TabId::new()),
            bounds: None,
        });
        assert!(doc.validate().is_err());
    }

    #[test]
    fn rejects_bad_ratio_and_deep_nesting() {
        let mut tab = TabLayout {
            id: TabId::new(),
            title: None,
            root: PaneNode::Split {
                direction: SplitDirection::Columns,
                ratio: 0.0,
                first: Box::new(leaf(None)),
                second: Box::new(leaf(None)),
            },
            active_pane: None,
        };
        let mut doc = WorkspaceDoc::default();
        doc.windows.push(WindowLayout {
            id: WindowId::new(),
            tabs: vec![tab.clone()],
            active_tab: None,
            bounds: None,
        });
        assert!(doc.validate().is_err());

        // Deep nesting overflow.
        let mut node = leaf(None);
        for _ in 0..(MAX_LAYOUT_DEPTH + 2) {
            node = PaneNode::Split {
                direction: SplitDirection::Rows,
                ratio: 0.5,
                first: Box::new(node),
                second: Box::new(leaf(None)),
            };
        }
        tab.root = node;
        doc.windows[0].tabs[0] = tab;
        assert!(doc.validate().is_err());
    }

    #[test]
    fn accepts_known_last_route_and_rejects_unknown() {
        let mut doc = WorkspaceDoc::default();
        for route in KNOWN_ROUTES {
            doc.last_route = Some(route.to_string());
            doc.validate().unwrap();
        }
        doc.last_route = Some("nonsense".to_string());
        assert!(doc.validate().is_err());
        // A document without a route (written before routing existed) loads.
        doc.last_route = None;
        doc.validate().unwrap();
    }

    #[test]
    fn roundtrips_through_json() {
        let mut doc = WorkspaceDoc {
            last_route: Some("sessions".to_string()),
            ..WorkspaceDoc::default()
        };
        let tab = TabLayout {
            id: TabId::new(),
            title: Some("web".to_string()),
            root: PaneNode::Split {
                direction: SplitDirection::Columns,
                ratio: 0.3,
                first: Box::new(leaf(Some(HostId::new()))),
                second: Box::new(leaf(None)),
            },
            active_pane: None,
        };
        doc.windows.push(WindowLayout {
            id: WindowId::new(),
            tabs: vec![tab],
            active_tab: None,
            bounds: Some(WindowBounds {
                x: 10,
                y: 20,
                width: 1280,
                height: 800,
            }),
        });
        doc.validate().unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let back: WorkspaceDoc = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
        back.validate().unwrap();
    }
}
