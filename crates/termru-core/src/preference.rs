//! Application preferences.
//!
//! Preferences are split into two scopes:
//!
//! * **Portable** preferences (theme, terminal appearance, shortcuts) are part
//!   of the synced payload so a fresh device inherits the user's look & feel.
//! * **Device-local** preferences (window geometry cache, sync endpoint, device
//!   id, token material) live in the local store only and are never synced.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::CoreError;

pub const DEFAULT_SCROLLBACK: u32 = 10_000;
pub const MAX_SCROLLBACK: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TerminalPreferences {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_scrollback")]
    pub scrollback: u32,
    #[serde(default)]
    pub cursor_blink: bool,
    #[serde(default = "default_cursor_style")]
    pub cursor_style: CursorStyle,
    #[serde(default)]
    pub copy_on_select: bool,
}

fn default_font_family() -> String {
    "monospace".to_string()
}

fn default_font_size() -> u32 {
    14
}

fn default_scrollback() -> u32 {
    DEFAULT_SCROLLBACK
}

fn default_cursor_style() -> CursorStyle {
    CursorStyle::Block
}

impl Default for TerminalPreferences {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            scrollback: default_scrollback(),
            cursor_blink: true,
            cursor_style: default_cursor_style(),
            copy_on_select: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    Block,
    Bar,
    Underline,
}

/// Portable (synced) preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", default)]
pub struct Preferences {
    pub theme: ThemePreference,
    pub terminal: TerminalPreferences,
    /// User-defined keybinding overrides: action id -> accelerator string.
    /// Merged over platform defaults by the frontend; validated there for
    /// conflicts. Bounded to keep payloads small.
    pub keybindings: BTreeMap<String, String>,
}

impl Preferences {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.terminal.font_size < 6 || self.terminal.font_size > 72 {
            return Err(CoreError::validation("font size must be between 6 and 72"));
        }
        if self.terminal.scrollback > MAX_SCROLLBACK {
            return Err(CoreError::validation(format!(
                "scrollback must be at most {MAX_SCROLLBACK} lines"
            )));
        }
        if self.terminal.font_family.chars().count() > 128 {
            return Err(CoreError::validation("font family name is too long"));
        }
        if self.terminal.font_family.chars().any(char::is_control) {
            return Err(CoreError::validation(
                "font family must not contain control characters",
            ));
        }
        if self.keybindings.len() > 512 {
            return Err(CoreError::validation("too many keybinding overrides"));
        }
        for (action, accel) in &self.keybindings {
            if action.is_empty() || action.len() > 64 || action.chars().any(char::is_control) {
                return Err(CoreError::validation("invalid keybinding action id"));
            }
            if accel.is_empty() || accel.len() > 64 || accel.chars().any(char::is_control) {
                return Err(CoreError::validation("invalid keybinding accelerator"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Preferences::default().validate().unwrap();
    }

    #[test]
    fn rejects_out_of_range_font_and_scrollback() {
        let mut p = Preferences::default();
        p.terminal.font_size = 200;
        assert!(p.validate().is_err());

        let mut p = Preferences::default();
        p.terminal.scrollback = MAX_SCROLLBACK + 1;
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_bad_keybinding_override() {
        let mut p = Preferences::default();
        p.keybindings
            .insert("new_tab".to_string(), "Ctrl+Shift+T".to_string());
        p.validate().unwrap();
        p.keybindings.insert("".to_string(), "Ctrl+T".to_string());
        assert!(p.validate().is_err());
    }
}
