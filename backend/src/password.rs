//! Argon2id password hashing - the OWASP-recommended default (memory-hard,
//! resists GPU/ASIC cracking far better than bcrypt/PBKDF2). The stored hash
//! is the full PHC string (algorithm + params + salt + hash all in one
//! self-describing value), so verification never needs to separately know
//! or store the salt or the parameters used.
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| format!("failed to hash password: {err}"))
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hashed_password_verifies_against_the_original_but_not_a_wrong_one() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn hashing_the_same_password_twice_produces_different_hashes() {
        // Different random salt each time - proves the salt is actually
        // being randomized, not reused or hardcoded.
        let a = hash_password("same password").unwrap();
        let b = hash_password("same password").unwrap();
        assert_ne!(a, b);
        assert!(verify_password("same password", &a));
        assert!(verify_password("same password", &b));
    }

    #[test]
    fn verifying_against_garbage_stored_hash_is_a_clean_false_not_a_panic() {
        assert!(!verify_password("anything", "not a real phc hash"));
    }
}
