//! Opaque access and refresh tokens.
//!
//! The server does not use self-contained tokens (no JWT): every token is a
//! random 256-bit value that is stored **hashed**. A stolen database therefore
//! does not hand out usable credentials, and revoking a session is immediate.

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::error::{ApiError, ApiResult};

/// Number of random bytes in a token.
const TOKEN_BYTES: usize = 32;

/// Generates a new opaque token (base64url, unpadded).
pub fn generate_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Hashes a token for storage. SHA-256 is appropriate here: the input has
/// 256 bits of entropy, so the hash is not brute-forceable and can be used
/// as an index.
pub fn hash_token(token: &str) -> Vec<u8> {
    hash_bytes(token.as_bytes())
}

/// Hashes arbitrary bytes with SHA-256.
pub fn hash_bytes(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

/// Checks that a token has a plausible shape before it is hashed or compared.
pub fn validate_token_shape(token: &str) -> ApiResult<()> {
    if token.len() < 32 || token.len() > 128 {
        return Err(ApiError::Unauthorized("invalid token".to_string()));
    }
    if !token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(ApiError::Unauthorized("invalid token".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_url_safe() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }

    #[test]
    fn hashing_is_stable_and_opaque() {
        let token = generate_token();
        let hash = hash_token(&token);
        assert_eq!(hash.len(), 32);
        assert_eq!(hash, hash_token(&token));
        assert_ne!(hash, hash_token(&generate_token()));
        assert!(!String::from_utf8_lossy(&hash).contains(&token));
    }

    #[test]
    fn token_shape_is_validated() {
        assert!(validate_token_shape(&generate_token()).is_ok());
        assert!(validate_token_shape("short").is_err());
        assert!(validate_token_shape(&"a".repeat(200)).is_err());
        assert!(validate_token_shape(&"a".repeat(40)).is_ok());
        assert!(validate_token_shape(&"!".repeat(40)).is_err());
    }
}
