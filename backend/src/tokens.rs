//! Opaque random secret tokens (32 bytes, URL-safe base64) - the shared
//! shape behind both refresh tokens and invitation tokens: generate a raw
//! value to hand to a client once, store only its hash, compare by hashing
//! whatever the client presents later. Factored out here once a second
//! caller (invitations.rs) needed the identical pattern refresh_token.rs
//! already had.
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_not_all_identical() {
        assert_ne!(generate(), generate());
    }

    #[test]
    fn hashing_is_deterministic_but_not_reversible_looking() {
        let raw = generate();
        assert_eq!(hash(&raw), hash(&raw));
        assert_ne!(hash(&raw), raw);
    }
}
