//! Turning arbitrary user text into names other systems will accept.
//!
//! A Node called "Web Server (EU)" has to become something DNS and Docker
//! will take, and both want the same thing: an RFC 1123 label. That
//! conversion was written twice - `dns_service::slugify` and
//! `runtime::docker::network_alias` - with the same rules and no shared
//! definition, which is how two names that are supposed to agree drift
//! apart.

/// The longest a single DNS label may be, per RFC 1123. Docker's own
/// network aliases are resolved through its embedded DNS, so the same limit
/// applies to both callers.
const MAX_LABEL_LENGTH: usize = 63;

/// Lowercases, replaces every run of non-alphanumeric characters with a
/// single `-`, trims leading and trailing `-`, and truncates to
/// `MAX_LABEL_LENGTH`.
///
/// Returns `None` for input that contains nothing usable at all ("!!!"),
/// rather than inventing a name: the two callers want different fallbacks -
/// DNS uses a literal `"node"`, Docker uses the container's own name - and
/// picking one here would force the other to detect and undo it.
///
/// Every character this emits is single-byte ASCII, so a byte length is a
/// safe stand-in for a character count when applying the cap.
pub(crate) fn dns_label(input: &str) -> Option<String> {
    let mut result = String::with_capacity(input.len().min(MAX_LABEL_LENGTH));
    let mut last_was_dash = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
        if result.len() >= MAX_LABEL_LENGTH {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            result.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !result.is_empty() {
            result.push('-');
            last_was_dash = true;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_replaces_runs_of_punctuation_with_one_dash() {
        assert_eq!(dns_label("Web Server (EU)").unwrap(), "web-server-eu");
        assert_eq!(dns_label("Paper   Survival!!").unwrap(), "paper-survival");
        assert_eq!(dns_label("db01").unwrap(), "db01");
    }

    #[test]
    fn never_starts_or_ends_with_a_dash() {
        assert_eq!(dns_label("  leading").unwrap(), "leading");
        assert_eq!(dns_label("trailing!!").unwrap(), "trailing");
        assert_eq!(dns_label("--both--").unwrap(), "both");
    }

    #[test]
    fn truncates_to_the_rfc1123_label_limit_without_leaving_a_trailing_dash() {
        assert_eq!(dns_label(&"a".repeat(80)).unwrap().len(), MAX_LABEL_LENGTH);
        // A truncation landing exactly on a dash must not leave one behind.
        let awkward = format!("{}-tail", "a".repeat(MAX_LABEL_LENGTH - 1));
        assert!(!dns_label(&awkward).unwrap().ends_with('-'));
    }

    /// `None`, not a made-up name: the DNS and Docker callers want
    /// different fallbacks, and choosing one here would force the other to
    /// detect and undo it.
    #[test]
    fn input_with_nothing_usable_produces_no_label() {
        assert!(dns_label("!!!").is_none());
        assert!(dns_label("").is_none());
        assert!(dns_label("   ").is_none());
    }
}
