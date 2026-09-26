//! Two-factor sign-in with a time-based code (TOTP, RFC 6238) - the six
//! digits Google Authenticator, Aegis or 1Password show.
//!
//! **The secret is encrypted at rest**, with AES-256-GCM under a key that is
//! not the JWT secret (`TOTP_ENCRYPTION_KEY`, 32 bytes, base64). A copy of the
//! database alone - a backup, a leaked dump - then does not hand over every
//! account's second factor with it. The user's id is the associated data, so
//! a ciphertext moved onto another row does not decrypt. Without the key the
//! service still runs; turning two-factor on is refused with a code that says
//! why, and accounts that already have it are refused at sign-in rather than
//! let through on a password alone.
//!
//! **Recovery codes** are the way back from a lost phone, since this service
//! sends no email. Ten, each usable once, shown once, stored as SHA-256 like
//! refresh tokens: they are random enough that a slow hash would only slow
//! down the check that has to try each of them.
//!
//! **A code is accepted once.** The time step of the last accepted code is
//! kept, and a code for that step or an earlier one is refused - so a code
//! read over somebody's shoulder cannot be replayed within its minute.

use std::sync::OnceLock;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;
use uuid::Uuid;

/// Seconds per code, digits per code, and how many steps either side of now
/// are accepted - one, so a phone a little out of sync still works.
const STEP_SECONDS: i64 = 30;
const DIGITS: u32 = 6;
const SKEW_STEPS: i64 = 1;

/// 160 bits, the size RFC 4226 recommends for an HMAC-SHA1 key.
const SECRET_BYTES: usize = 20;

pub const RECOVERY_CODE_COUNT: usize = 10;

/// The encryption key, read once from `TOTP_ENCRYPTION_KEY`. `None` when it is
/// missing or not 32 bytes of base64 - reported once at startup by `main`.
pub fn cipher() -> Option<&'static Aes256Gcm> {
    static CIPHER: OnceLock<Option<Aes256Gcm>> = OnceLock::new();
    CIPHER
        .get_or_init(|| {
            let encoded = std::env::var("TOTP_ENCRYPTION_KEY").ok()?;
            let key = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
            (key.len() == 32).then(|| Aes256Gcm::new_from_slice(&key).expect("a 32-byte key is a valid AES-256 key"))
        })
        .as_ref()
}

/// A fresh random secret.
pub fn generate_secret() -> Vec<u8> {
    let mut secret = vec![0u8; SECRET_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    secret
}

/// `nonce || ciphertext`, with the user id bound in as associated data.
pub fn encrypt(cipher: &Aes256Gcm, user_id: Uuid, secret: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: secret, aad: user_id.as_bytes() })
        .map_err(|_| "couldn't encrypt the two-factor secret".to_string())?;
    Ok([nonce.as_slice(), &ciphertext].concat())
}

pub fn decrypt(cipher: &Aes256Gcm, user_id: Uuid, stored: &[u8]) -> Result<Vec<u8>, String> {
    if stored.len() < 12 {
        return Err("the stored two-factor secret is malformed".into());
    }
    let (nonce, ciphertext) = stored.split_at(12);
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ciphertext, aad: user_id.as_bytes() })
        .map_err(|_| "couldn't decrypt the two-factor secret - is TOTP_ENCRYPTION_KEY the one it was stored with?".to_string())
}

/// RFC 4226's HOTP value for one counter.
fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(secret).expect("HMAC accepts a key of any length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let value = u32::from_be_bytes([digest[offset] & 0x7f, digest[offset + 1], digest[offset + 2], digest[offset + 3]]);
    value % 10u32.pow(DIGITS)
}

/// The time step a code is accepted at, or `None`.
///
/// `after_step` is the step of the last code this account used; nothing at
/// or before it is accepted again. Compared in constant time per candidate,
/// so the check does not reveal how close a guess was.
pub fn verify(secret: &[u8], code: &str, now_unix: i64, after_step: Option<i64>) -> Option<i64> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != DIGITS as usize || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let current = now_unix.div_euclid(STEP_SECONDS);
    (current - SKEW_STEPS..=current + SKEW_STEPS)
        .filter(|step| after_step.is_none_or(|last| *step > last) && *step >= 0)
        .find(|step| {
            let expected = format!("{:0width$}", hotp(secret, *step as u64), width = DIGITS as usize);
            constant_time_eq(expected.as_bytes(), digits.as_bytes())
        })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The code an authenticator shows at `unix` - what the integration tests
/// type in, the way a person would.
pub fn code_at(secret: &[u8], unix: i64) -> String {
    format!("{:0width$}", hotp(secret, unix.div_euclid(STEP_SECONDS) as u64), width = DIGITS as usize)
}

/// Base32 back to bytes, for a secret as an app would have been given it.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.chars().filter(|c| *c != '=') {
        let value = match c.to_ascii_uppercase() {
            letter @ 'A'..='Z' => letter as u32 - 'A' as u32,
            digit @ '2'..='7' => digit as u32 - '2' as u32 + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// RFC 4648 base32, no padding - how authenticator apps take a secret.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// The `otpauth://` link an authenticator app reads from the QR code.
pub fn otpauth_uri(email: &str, secret: &[u8]) -> String {
    let label = percent_encode(&format!("VibeSSH:{email}"));
    format!("otpauth://totp/{label}?secret={}&issuer=VibeSSH&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}", base32(secret))
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' | b'@' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Ten recovery codes, `xxxxx-xxxxx` from an alphabet without look-alikes -
/// about 50 bits each, typed by a person who has lost their phone.
pub fn generate_recovery_codes() -> Vec<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    (0..RECOVERY_CODE_COUNT)
        .map(|_| {
            let mut bytes = [0u8; 10];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            let chars: String = bytes.iter().map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char).collect();
            format!("{}-{}", &chars[..5], &chars[5..])
        })
        .collect()
}

/// A recovery code as typed - any case, with or without the hyphen or
/// spaces - reduced to the form that was hashed.
pub fn normalize_recovery_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA1, secret "12345678901234567890", 8 digits
    /// there - the last six of each are what a six-digit code shows.
    #[test]
    fn codes_match_the_rfc_6238_test_vectors() {
        let secret = b"12345678901234567890";
        for (time, eight_digits) in [(59i64, "94287082"), (1111111109, "07081804"), (1111111111, "14050471"), (1234567890, "89005924"), (2000000000, "69279037")] {
            let six = &eight_digits[2..];
            assert_eq!(verify(secret, six, time, None), Some(time / 30), "t={time}");
        }
    }

    #[test]
    fn a_code_is_accepted_one_step_either_side_and_not_further() {
        let secret = b"12345678901234567890";
        let code = format!("{:06}", hotp(secret, 100));
        assert_eq!(verify(secret, &code, 100 * 30, None), Some(100));
        assert_eq!(verify(secret, &code, 101 * 30, None), Some(100));
        assert_eq!(verify(secret, &code, 99 * 30, None), Some(100));
        assert_eq!(verify(secret, &code, 102 * 30, None), None);
    }

    #[test]
    fn a_used_code_is_not_accepted_again() {
        let secret = b"12345678901234567890";
        let code = format!("{:06}", hotp(secret, 100));
        assert_eq!(verify(secret, &code, 100 * 30, Some(100)), None);
        assert_eq!(verify(secret, &code, 100 * 30, Some(99)), Some(100));
    }

    #[test]
    fn malformed_codes_are_refused() {
        let secret = b"12345678901234567890";
        for code in ["", "12345", "1234567", "abcdef", "12 34 5"] {
            assert_eq!(verify(secret, code, 0, None), None, "{code:?}");
        }
    }

    #[test]
    fn base32_matches_rfc_4648() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"fooba"), "MZXW6YTB");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn the_secret_only_decrypts_for_the_account_it_was_stored_for() {
        let cipher = Aes256Gcm::new_from_slice(&[7u8; 32]).unwrap();
        let alice = Uuid::new_v4();
        let secret = generate_secret();
        let stored = encrypt(&cipher, alice, &secret).unwrap();
        assert_eq!(decrypt(&cipher, alice, &stored).unwrap(), secret);
        assert!(decrypt(&cipher, Uuid::new_v4(), &stored).is_err());
    }

    #[test]
    fn recovery_codes_are_distinct_and_typed_back_in_any_form() {
        let codes = generate_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODE_COUNT);
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), RECOVERY_CODE_COUNT);
        let code = &codes[0];
        assert_eq!(normalize_recovery_code(&code.to_uppercase().replace('-', " ")), normalize_recovery_code(code));
    }

    #[test]
    fn the_otpauth_link_names_the_account_and_carries_the_secret() {
        let uri = otpauth_uri("ana@example.com", b"12345678901234567890");
        assert!(uri.starts_with("otpauth://totp/VibeSSH:ana@example.com?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"));
        assert!(uri.contains("issuer=VibeSSH"));
    }
}
