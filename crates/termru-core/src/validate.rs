//! Input validation for user-supplied domain fields.
//!
//! All validation is centralized here so the desktop UI, the local store, and
//! the sync payload builder enforce identical rules. Limits are deliberately
//! conservative: they bound attacker-controlled sizes in synced payloads and
//! keep UI rendering safe (no control characters in names that end up in logs,
//! menus, or terminal titles).

use crate::error::CoreError;

pub const MAX_NAME_LEN: usize = 128;
pub const MAX_ADDRESS_LEN: usize = 253;
pub const MAX_USERNAME_LEN: usize = 128;
pub const MAX_DESCRIPTION_LEN: usize = 2_000;
pub const MAX_TAG_NAME_LEN: usize = 64;
pub const MAX_TERM_LEN: usize = 64;
pub const MAX_SNIPPET_BODY_LEN: usize = 16_384;
pub const MAX_TAGS_PER_HOST: usize = 32;
pub const MAX_GROUP_DEPTH: usize = 8;

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
pub fn validate_address(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation("address must not be empty"));
    }
    if value.chars().count() > MAX_ADDRESS_LEN {
        return Err(CoreError::validation(format!(
            "address must be at most {MAX_ADDRESS_LEN} characters"
        )));
    }
    let ok = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    if !ok {
        return Err(CoreError::validation(
            "address contains invalid characters (allowed: letters, digits, '.', '-', '_', ':')",
        ));
    }
    if value.starts_with('-') || value.ends_with('-') {
        return Err(CoreError::validation(
            "address must not start or end with '-'",
        ));
    }
    Ok(())
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

/// Validates a keepalive interval in seconds (0 disables keepalives).
pub fn validate_keepalive(seconds: u32) -> Result<()> {
    const MAX_KEEPALIVE: u32 = 3_600;
    if seconds > MAX_KEEPALIVE {
        return Err(CoreError::validation(format!(
            "keepalive interval must be 0 (disabled) or at most {MAX_KEEPALIVE} seconds"
        )));
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
    fn like_escaping() {
        assert_eq!(escape_like_pattern("100%_x"), r"100\%\_x");
        assert_eq!(escape_like_pattern("plain"), "plain");
    }
}
