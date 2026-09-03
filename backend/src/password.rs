//! Argon2id password hashing - the OWASP-recommended default (memory-hard,
//! resists GPU/ASIC cracking far better than bcrypt/PBKDF2). The stored hash
//! is the full PHC string (algorithm + params + salt + hash all in one
//! self-describing value), so verification never needs to separately know
//! or store the salt or the parameters used.
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

pub const MIN_PASSWORD_LEN: usize = 8;
/// Argon2 is deliberately memory- and CPU-hard - hashing an attacker-
/// supplied multi-megabyte "password" would burn real server resources for
/// every request, a cheap denial-of-service lever with no legitimate use
/// (no real password is anywhere near this long). Callers must enforce
/// this before ever passing user input to hash_password/verify_password.
pub const MAX_PASSWORD_LEN: usize = 256;

/// The alphabet a generated password is drawn from.
///
/// Missing on purpose: `0`, `O`, `o`, `1`, `l`, `I`. A provisioned password
/// is read off one screen and typed into another, sometimes read aloud, so
/// the characters people confuse are worth more as clarity than as the
/// fraction of a bit they add. What is left is 56 symbols, and with 20 of
/// them that is about 116 bits - far past anything that needs defending.
const GENERATED_ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// How many characters a generated password has.
const GENERATED_LEN: usize = 20;

/// A password for an account somebody else is creating.
///
/// Generated rather than chosen, because whoever provisions the account
/// would otherwise pick it - and a password chosen by one person for
/// another is reliably the weakest one either of them uses. It is shown
/// once and never stored in the clear; the account it belongs to is forced
/// to replace it at first sign-in, so its only job is to survive the trip
/// from one person to the other.
///
/// Rejection sampling rather than the modulo of a random byte: 256 is not a
/// multiple of 56, so taking a remainder would make the first few symbols
/// of the alphabet measurably likelier than the rest.
pub fn generate_password() -> String {
    use rand::RngCore;
    let mut rng = rand::rngs::OsRng;
    let mut password = String::with_capacity(GENERATED_LEN);
    let mut buffer = [0u8; 64];
    while password.len() < GENERATED_LEN {
        rng.fill_bytes(&mut buffer);
        for byte in buffer {
            if password.len() == GENERATED_LEN {
                break;
            }
            // 4 * 56 = 224, so bytes below that map evenly onto the 56
            // symbols; the 32 above are discarded rather than folded in,
            // which is what would make the first symbols likelier.
            if (byte as usize) < GENERATED_ALPHABET.len() * 4 {
                password.push(GENERATED_ALPHABET[(byte as usize) % GENERATED_ALPHABET.len()] as char);
            }
        }
    }
    password
}

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
    fn a_generated_password_is_the_right_length_and_uses_only_unambiguous_characters() {
        let password = generate_password();
        assert_eq!(password.chars().count(), GENERATED_LEN);
        for character in password.chars() {
            assert!(GENERATED_ALPHABET.contains(&(character as u8)), "{character} is not in the alphabet");
        }
        // The characters people misread when copying one off a screen.
        for confusable in ["0", "O", "o", "1", "l", "I"] {
            assert!(!password.contains(confusable), "{password} contains the confusable {confusable}");
        }
    }

    /// Not a randomness test - it is a "did somebody wire it to a constant"
    /// test, which is the failure that would actually ship.
    #[test]
    fn generated_passwords_differ() {
        let generated: std::collections::HashSet<String> = (0..64).map(|_| generate_password()).collect();
        assert_eq!(generated.len(), 64);
    }

    #[test]
    fn a_generated_password_is_long_enough_to_be_accepted() {
        assert!(generate_password().len() >= MIN_PASSWORD_LEN);
        assert!(generate_password().len() <= MAX_PASSWORD_LEN);
    }

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
