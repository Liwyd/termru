//! Password hashing (argon2id).
//!
//! Passwords are stored as PHC strings. Verification always runs the argon2
//! KDF, also for unknown accounts, so that the response time of a login does
//! not reveal whether the account exists.

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use std::sync::OnceLock;

use crate::error::{ApiError, ApiResult};

/// Minimum accepted password length.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Maximum accepted password length (argon2 is not meant for very long
/// inputs; the limit also bounds the cost of hashing).
pub const MAX_PASSWORD_LEN: usize = 256;

/// A hash of a random value, used to keep the timing of a failed login
/// constant when the account does not exist.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        // Hashing cannot realistically fail for a fixed 32-byte input.
        hash_password("dummy-password-for-constant-time").unwrap_or_default()
    })
}

/// Hashes a password with argon2id.
pub fn hash_password(password: &str) -> ApiResult<String> {
    validate_password(password)?;
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| ApiError::Internal(anyhow::Error::new(e)))
}

/// Verifies a password against a stored PHC hash.
///
/// Returns `false` for malformed hashes instead of an error: a corrupt row
/// must not become a distinguishable failure mode.
pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Runs a verification against a dummy hash, so that an unknown account
/// costs the same as a known one.
pub fn verify_against_dummy(password: &str) {
    let hash = dummy_hash();
    if !hash.is_empty() {
        let _ = verify_password(password, hash);
    }
}

/// Checks the password policy.
///
/// Only length and "not entirely whitespace" are enforced. Composition rules
/// (one upper case letter, one digit, ...) push people towards predictable
/// passwords, so they are deliberately not required.
pub fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::BadRequest(format!(
            "the password must have at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if password.chars().count() > MAX_PASSWORD_LEN {
        return Err(ApiError::BadRequest(format!(
            "the password must have at most {MAX_PASSWORD_LEN} characters"
        )));
    }
    if password.chars().all(char::is_whitespace) {
        return Err(ApiError::BadRequest(
            "the password must not consist of whitespace only".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_round_trips() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong horse battery staple", &hash));
    }

    #[test]
    fn two_hashes_of_the_same_password_differ() {
        let a = hash_password("correct horse battery staple").unwrap();
        let b = hash_password("correct horse battery staple").unwrap();
        assert_ne!(a, b, "each hash must use a fresh salt");
    }

    #[test]
    fn the_password_policy_is_enforced() {
        assert!(validate_password("short").is_err());
        assert!(validate_password(&"x".repeat(MAX_PASSWORD_LEN + 1)).is_err());
        assert!(validate_password("               ").is_err());
        assert!(validate_password("longenoughpassword").is_ok());
        assert!(validate_password("a passphrase with spaces").is_ok());
    }

    #[test]
    fn a_malformed_hash_fails_closed() {
        assert!(!verify_password("whatever", "not-a-phc-string"));
        assert!(!verify_password("whatever", ""));
    }
}
