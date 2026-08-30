use chrono::Duration as ChronoDuration;
use rand::Rng;

/// How long a pairing code stays valid after the desktop generates it.
pub const PAIRING_CODE_TTL: ChronoDuration = ChronoDuration::minutes(5);

const PREFIX: &str = "VIBE";
const GROUP_LEN: usize = 4;
const GROUP_COUNT: usize = 2;
/// Crockford-ish base32 with ambiguous characters (0/O, 1/I/L, U) removed,
/// so a code read aloud or hand-typed from one screen to another doesn't
/// produce silently-wrong-but-plausible-looking transcriptions.
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Generates a code like `VIBE-J7K4-92QT` from a CSPRNG. `GROUP_LEN *
/// GROUP_COUNT` = 8 symbols from a 30-character alphabet is ~39 bits of
/// entropy - short enough to type, and combined with the TTL and single-use
/// consumption on the agent side, not realistically brute-forceable within
/// its 5-minute window over a network round trip per guess.
pub fn generate_pairing_code() -> String {
    let mut rng = rand::thread_rng();
    let mut code = String::from(PREFIX);
    for _ in 0..GROUP_COUNT {
        code.push('-');
        for _ in 0..GROUP_LEN {
            let idx = rng.gen_range(0..ALPHABET.len());
            code.push(ALPHABET[idx] as char);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_match_the_expected_shape() {
        let code = generate_pairing_code();
        let parts: Vec<&str> = code.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "VIBE");
        assert_eq!(parts[1].len(), GROUP_LEN);
        assert_eq!(parts[2].len(), GROUP_LEN);
        for c in parts[1].bytes().chain(parts[2].bytes()) {
            assert!(ALPHABET.contains(&c), "unexpected character: {}", c as char);
        }
    }

    #[test]
    fn generated_codes_are_not_all_identical() {
        let a = generate_pairing_code();
        let b = generate_pairing_code();
        assert_ne!(a, b, "two random codes collided - RNG looks broken");
    }
}
