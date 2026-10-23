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
/// Terminal line box as a multiple of the font size (14 px / 23.1 px).
pub const DEFAULT_LINE_HEIGHT: f64 = 1.65;

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
    /// Multiplier of the font size, the line box of the terminal
    /// (`1.65` = the design's 14 px / 23.1 px). Additive: an older record
    /// without the field loads with the default.
    #[serde(default = "default_line_height")]
    pub line_height: f64,
}

fn default_font_family() -> String {
    "IBM Plex Mono, monospace".to_string()
}

fn default_font_size() -> u32 {
    14
}

fn default_scrollback() -> u32 {
    DEFAULT_SCROLLBACK
}

fn default_line_height() -> f64 {
    DEFAULT_LINE_HEIGHT
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
            line_height: default_line_height(),
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

/// Motion preference: follow the operating system, force the reduced set or
/// force full motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionPreference {
    #[default]
    System,
    On,
    Off,
}

/// Portable (synced) preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct Preferences {
    pub theme: ThemePreference,
    pub terminal: TerminalPreferences,
    /// User-defined keybinding overrides: action id -> accelerator string.
    /// Merged over platform defaults by the frontend; validated there for
    /// conflicts. Bounded to keep payloads small.
    pub keybindings: BTreeMap<String, String>,
    /// Reduced motion: `system` follows the OS, `on` forces it, `off`
    /// overrides an OS that asks for it.
    pub reduced_motion: MotionPreference,
    /// Thicker focus ring for users who need a more visible one.
    pub focus_visibility: bool,
    /// Whether routine (non-error) notices are shown at all.
    pub routine_notices: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            terminal: TerminalPreferences::default(),
            keybindings: BTreeMap::new(),
            reduced_motion: MotionPreference::default(),
            focus_visibility: false,
            routine_notices: true,
        }
    }
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
        if !(1.0..=3.0).contains(&self.terminal.line_height) {
            return Err(CoreError::validation(
                "line height must be between 1.0 and 3.0",
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

    #[test]
    fn rejects_out_of_range_line_height() {
        for line_height in [0.5, 4.0, f64::NAN, f64::INFINITY] {
            let p = Preferences {
                terminal: TerminalPreferences {
                    line_height,
                    ..TerminalPreferences::default()
                },
                ..Preferences::default()
            };
            assert!(p.validate().is_err(), "line height {line_height} must fail");
        }
        let ok = Preferences {
            terminal: TerminalPreferences {
                line_height: 1.65,
                ..TerminalPreferences::default()
            },
            ..Preferences::default()
        };
        ok.validate().unwrap();
    }

    /// A record written before the redesign phase exists in every install
    /// that synced or stored it: it must keep loading, with the new defaults.
    #[test]
    fn record_without_the_new_fields_loads_with_defaults() {
        let old = r#"{"theme":"dark","terminal":{"font_family":"monospace",
            "font_size":16,"scrollback":5000,"cursor_blink":false,
            "cursor_style":"bar","copy_on_select":true},
            "keybindings":{"palette.open":"Ctrl+K"}}"#;
        let parsed: Preferences = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.theme, ThemePreference::Dark);
        assert_eq!(parsed.terminal.font_size, 16);
        assert_eq!(parsed.terminal.line_height, DEFAULT_LINE_HEIGHT);
        assert_eq!(parsed.reduced_motion, MotionPreference::System);
        assert!(!parsed.focus_visibility);
        assert!(parsed.routine_notices);
        assert_eq!(parsed.keybindings.len(), 1);
        parsed.validate().unwrap();
    }

    #[test]
    fn new_fields_round_trip() {
        let p = Preferences {
            reduced_motion: MotionPreference::On,
            focus_visibility: true,
            routine_notices: false,
            terminal: TerminalPreferences {
                line_height: 1.5,
                ..TerminalPreferences::default()
            },
            ..Preferences::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Preferences = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reduced_motion, MotionPreference::On);
        assert!(back.focus_visibility);
        assert!(!back.routine_notices);
        assert_eq!(back.terminal.line_height, 1.5);
    }
}
