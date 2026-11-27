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
/// Terminal line box as a multiple of the font size (a 14 px font gets an
/// 18.2 px line).
pub const DEFAULT_LINE_HEIGHT: f64 = 1.3;

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
    /// ([`DEFAULT_LINE_HEIGHT`] = `1.3`). Additive: an older record without
    /// the field loads with the default.
    #[serde(default = "default_line_height")]
    pub line_height: f64,
    /// Ask before a paste of several lines reaches the shell. A paste of
    /// many lines runs every one of them as soon as the shell reads it, so
    /// the question is on by default. Additive: an older record without the
    /// field loads with the default.
    #[serde(default = "default_confirm_multiline_paste")]
    pub confirm_multiline_paste: bool,
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

fn default_confirm_multiline_paste() -> bool {
    true
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
            confirm_multiline_paste: default_confirm_multiline_paste(),
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
    /// conflicts. An empty accelerator means "unbound": the command has no
    /// shortcut on any device. Bounded to keep payloads small.
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
            // An empty accelerator is deliberate: the user unbound the command.
            if accel.len() > 64 || accel.chars().any(char::is_control) {
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
                line_height: 1.65, // the previous default stays valid
                ..TerminalPreferences::default()
            },
            ..Preferences::default()
        };
        ok.validate().unwrap();
    }

    #[test]
    fn default_line_height_is_tight_and_consistent() {
        assert_eq!(DEFAULT_LINE_HEIGHT, 1.3);
        assert_eq!(default_line_height(), DEFAULT_LINE_HEIGHT);
        assert_eq!(
            Preferences::default().terminal.line_height,
            DEFAULT_LINE_HEIGHT
        );
        // A record that omits the field gets the same value through serde.
        let parsed: TerminalPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.line_height, DEFAULT_LINE_HEIGHT);
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
        // The paste question is on for a record that never heard of it.
        assert!(parsed.terminal.confirm_multiline_paste);
        assert_eq!(parsed.keybindings.len(), 1);
        parsed.validate().unwrap();
    }

    /// A payload from a sync server (or a backup) that predates the paste
    /// question loads with it on, and one that carries it keeps the value.
    #[test]
    fn confirm_multiline_paste_defaults_on_and_round_trips() {
        assert!(Preferences::default().terminal.confirm_multiline_paste);
        let parsed: TerminalPreferences = serde_json::from_str("{}").unwrap();
        assert!(parsed.confirm_multiline_paste);

        let off: TerminalPreferences =
            serde_json::from_str(r#"{"confirm_multiline_paste":false}"#).unwrap();
        assert!(!off.confirm_multiline_paste);

        let p = Preferences {
            terminal: TerminalPreferences {
                confirm_multiline_paste: false,
                ..TerminalPreferences::default()
            },
            ..Preferences::default()
        };
        let back: Preferences = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert!(!back.terminal.confirm_multiline_paste);
    }

    /// A whole record of the previous release (every field it wrote, none of
    /// the new ones) must still load and validate.
    #[test]
    fn full_previous_release_record_loads() {
        let previous = r#"{"theme":"light",
            "terminal":{"font_family":"IBM Plex Mono, monospace","font_size":14,
                "scrollback":10000,"cursor_blink":true,"cursor_style":"block",
                "copy_on_select":false,"line_height":1.3},
            "keybindings":{},"reduced_motion":"off","focus_visibility":true,
            "routine_notices":false}"#;
        let parsed: Preferences = serde_json::from_str(previous).unwrap();
        assert_eq!(parsed.reduced_motion, MotionPreference::Off);
        assert!(parsed.focus_visibility);
        assert!(!parsed.routine_notices);
        assert!(parsed.terminal.confirm_multiline_paste);
        parsed.validate().unwrap();
    }

    /// The shortcut table can unbind a command: an empty accelerator is a
    /// valid override, an empty action id is not.
    #[test]
    fn an_empty_accelerator_means_unbound() {
        let mut p = Preferences::default();
        p.keybindings.insert("tab.close".to_string(), String::new());
        p.validate().unwrap();
        let back: Preferences = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(
            back.keybindings.get("tab.close").map(String::as_str),
            Some("")
        );

        let mut bad = Preferences::default();
        bad.keybindings.insert(String::new(), String::new());
        assert!(bad.validate().is_err());
        let mut long = Preferences::default();
        long.keybindings
            .insert("tab.close".to_string(), "x".repeat(65));
        assert!(long.validate().is_err());
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
