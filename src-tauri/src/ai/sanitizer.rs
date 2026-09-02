//! Removing secrets from anything on its way to an AI provider.
//!
//! **Why this is its own module, and why it is the first thing written.**
//! Everything else in `ai` is plumbing that can be corrected later if it is
//! wrong. This one cannot: a secret that reaches a third-party inference
//! endpoint has left the machine, and no later change un-sends it. The
//! provider is an arbitrary URL the user typed - OpenRouter, a colleague's
//! box, anything OpenAI-shaped - so "the model provider is trustworthy" is
//! not an assumption this code is allowed to make.
//!
//! **This is the second line of defence, not the first.** The first is that
//! `AiContextBuilder` only ever reads through the repositories, and a
//! secret `EnvironmentVariable` read that way already comes back with an
//! empty value (see `models::EnvironmentVariable::value`'s own doc
//! comment) - the real one lives in the OS keyring and is only ever
//! resolved by a runtime about to start a container. Private keys are
//! likewise referenced by *path* on `Server`, never by content
//! (`models::Server::private_key_path`), and SSH passwords are keyring-only.
//! So the obvious secrets are structurally absent before this runs.
//!
//! What this catches is the rest: a password somebody typed into a *plain*
//! (not secret-flagged) environment variable, a `--requirepass` inside a
//! blueprint's rendered `runtime_config`, a connection string with
//! credentials in the middle of a log line. Those are real - `RedisBlueprint`
//! puts a password in a command array and `NatsBlueprint` puts a token in
//! one, both of which are part of `runtime_config`, which the Application
//! context would otherwise forward verbatim.
//!
//! **The bias is deliberate.** Over-redaction costs the model a little
//! context and is immediately visible to the user in the context preview;
//! under-redaction is a disclosure. Where the two conflict this module
//! over-redacts, which is why `PUBLIC_KEY` is starred out even though a
//! public key is not a secret.

use serde_json::{Map, Value};

/// What a redacted value is replaced with. Deliberately not the empty
/// string: the model should be able to tell "this exists and was withheld"
/// from "this is unset", because those two states have different
/// diagnoses.
pub const REDACTED: &str = "***";

/// Substrings that, appearing anywhere in a normalised key, mean the value
/// is a secret. Matched against the key with every non-alphanumeric
/// character stripped, so `API_KEY`, `api-key` and `apiKey` all normalise
/// to `apikey` and all match.
const SECRET_SUBSTRINGS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "apikey",
    "privatekey",
    "passphrase",
    "requirepass",
    "credential",
    "auth",
    "pwd",
];

/// Whole words that mean the value is a secret. Matched against the key's
/// individual segments rather than as substrings, because these are short
/// enough that a substring match would be absurd - `key` would redact
/// `monkey`, and `pass` would redact `passenger`.
const SECRET_SEGMENTS: &[&str] = &["key", "pass", "auth", "token", "secret", "pwd", "salt", "cookie", "session"];

/// Splits an identifier into lowercase words on both separator characters
/// and camelCase boundaries, so `authToken`, `AUTH_TOKEN` and `auth-token`
/// all yield `["auth", "token"]`.
fn segments(key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for ch in key.chars() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if ch.is_uppercase() && previous_lower && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        previous_lower = ch.is_lowercase() || ch.is_numeric();
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Whether a value stored under this name should never be sent.
///
/// Leading dashes are stripped first so the same judgement applies to a
/// command-line flag (`--requirepass`) as to a map key (`requirePass`) -
/// they are the same secret wearing different syntax, and both appear in
/// blueprint `runtime_config`.
pub fn is_secret_key(key: &str) -> bool {
    let key = key.trim_start_matches('-');
    let normalised: String = key.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    if SECRET_SUBSTRINGS.iter().any(|needle| normalised.contains(needle)) {
        return true;
    }
    segments(key).iter().any(|segment| SECRET_SEGMENTS.contains(&segment.as_str()))
}

/// True for the characters that can make up a key immediately before a
/// `=` or `:` separator.
fn is_key_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.'
}

/// Redacts secrets from free text - a log line, an error message, a
/// rendered command.
///
/// Three passes, in this order, because the later ones would otherwise
/// mangle what the earlier ones are looking for:
///
/// 1. PEM blocks, whole. A private key is the one thing here where a
///    partial redaction is worthless.
/// 2. URL userinfo (`postgres://user:pw@host`), before the `:` rule can
///    misread it.
/// 3. `KEY=VALUE` and `KEY: VALUE`, when `KEY` reads as a secret.
pub fn sanitize_text(text: &str) -> String {
    let text = redact_pem_blocks(text);
    let text = redact_url_userinfo(&text);
    redact_key_value_pairs(&text)
}

/// Replaces everything between a PEM `BEGIN` and its `END` line.
///
/// An unterminated block - a truncated log tail, which is exactly how a key
/// tends to show up in one - redacts to the end of the text rather than
/// being left alone. The half of a private key that made it into the buffer
/// is still the half worth not sending.
fn redact_pem_blocks(text: &str) -> String {
    const BEGIN: &str = "-----BEGIN";
    const END: &str = "-----END";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        out.push_str(&rest[..start]);
        out.push_str(REDACTED);
        let after = &rest[start + BEGIN.len()..];
        match after.find(END) {
            Some(end) => {
                // Past the END marker's own trailing dashes, so no fragment
                // of the armour is left dangling.
                let tail = &after[end + END.len()..];
                rest = match tail.find("-----") {
                    Some(dashes) => &tail[dashes + 5..],
                    None => "",
                };
            }
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Turns `scheme://user:password@host` into `scheme://user:***@host`.
///
/// The username is kept on purpose: which account a failing connection
/// string is using is often the whole diagnosis, and it is not the secret.
fn redact_url_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_end) = rest.find("://") {
        let authority_start = scheme_end + 3;
        out.push_str(&rest[..authority_start]);
        let authority = &rest[authority_start..];
        // The authority ends at the first character that cannot be part of
        // it; an `@` after one of those belongs to something else entirely.
        let authority_end = authority.find(['/', ' ', '"', '\n', '\t', '\'']).unwrap_or(authority.len());
        match authority[..authority_end].find('@') {
            Some(at) => {
                let userinfo = &authority[..at];
                match userinfo.find(':') {
                    Some(colon) => {
                        out.push_str(&userinfo[..=colon]);
                        out.push_str(REDACTED);
                    }
                    None => out.push_str(userinfo),
                }
                out.push('@');
                rest = &authority[at + 1..];
            }
            None => {
                out.push_str(&authority[..authority_end]);
                rest = &authority[authority_end..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Redacts the value half of `KEY=VALUE` and `KEY: VALUE` when the key
/// reads as a secret.
///
/// A quoted value is redacted to its closing quote, so a password
/// containing spaces does not leave its tail behind.
fn redact_key_value_pairs(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch != '=' && ch != ':' {
            out.push(ch);
            i += 1;
            continue;
        }

        // Walk back over the key that was just emitted. One closing quote
        // is stepped over first, so a JSON key (`"authToken":`) is read as
        // the key it is rather than as an empty one.
        let key: String = {
            let mut backwards = out.chars().rev().peekable();
            if matches!(backwards.peek(), Some('"') | Some('\'')) {
                backwards.next();
            }
            let trailing: Vec<char> = backwards.take_while(|c| is_key_char(*c)).collect();
            trailing.into_iter().rev().collect()
        };
        if key.is_empty() || !is_secret_key(&key) {
            out.push(ch);
            i += 1;
            continue;
        }

        out.push(ch);
        i += 1;
        // Any spaces between the separator and the value stay as they were.
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            out.push(chars[i]);
            i += 1;
        }
        if i >= chars.len() || chars[i] == '\n' || chars[i] == '\r' {
            continue;
        }
        let quote = if chars[i] == '"' || chars[i] == '\'' { Some(chars[i]) } else { None };
        out.push_str(REDACTED);
        match quote {
            // A quoted value ends at its closing quote whichever separator
            // introduced it. This is also what keeps JSON-ish text
            // diagnosable: `{"auth": "x", "port": 8080}` loses only the `x`.
            Some(q) => {
                i += 1;
                while i < chars.len() && chars[i] != q && chars[i] != '\n' {
                    i += 1;
                }
                if i < chars.len() && chars[i] == q {
                    i += 1;
                }
            }
            // An unquoted value ends where its syntax says it does, and the
            // two separators disagree. `KEY=VALUE` is shell/env shape: the
            // value is one token. `KEY: VALUE` is header/log shape: the
            // value is the rest of the line. Treating the second like the
            // first is what made `Authorization: Bearer sk-abc` redact only
            // the word `Bearer` and send the token.
            None if ch == ':' => {
                while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                    i += 1;
                }
            }
            None => {
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
            }
        }
    }
    out
}

/// Redacts a JSON structure - a blueprint's `runtime_config`, an
/// Application's `metadata`.
///
/// Three rules, because a secret hides in all three shapes here:
///
/// - **Object values** whose key reads as a secret are replaced wholesale,
///   whatever their type. Recursing into them instead would leak a
///   password that happened to be stored as `{"value": "..."}`.
/// - **Array elements** are checked pairwise for the command-line shape: an
///   element that reads as a secret flag redacts the element *after* it.
///   This is what catches `["--requirepass", "hunter2"]` and
///   `["--auth", "s3cret"]`, which is how `RedisBlueprint` and
///   `NatsBlueprint` actually store theirs.
/// - **Every remaining string** goes through `sanitize_text`, which is what
///   catches an inline `--password=x` or a connection string.
pub fn sanitize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (key, child) in map {
                if is_secret_key(key) {
                    out.insert(key.clone(), Value::String(REDACTED.to_string()));
                } else {
                    out.insert(key.clone(), sanitize_json(child));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            let mut redact_next = false;
            for item in items {
                if std::mem::take(&mut redact_next) {
                    out.push(Value::String(REDACTED.to_string()));
                    continue;
                }
                if let Value::String(text) = item {
                    // A flag with its value attached is one token, and
                    // `sanitize_text` already handles that shape; a bare
                    // flag arms the next element instead.
                    if text.starts_with('-') && !text.contains('=') && is_secret_key(text) {
                        redact_next = true;
                    }
                }
                out.push(sanitize_json(item));
            }
            Value::Array(out)
        }
        Value::String(text) => Value::String(sanitize_text(text)),
        other => other.clone(),
    }
}

/// One environment variable as it will be shown to the model - `(key,
/// value, is_secret)` in, `(key, value)` out.
///
/// A secret-flagged row is reported as present-but-withheld rather than
/// dropped, for the same reason `REDACTED` is not the empty string: "this
/// Application has a `MYSQL_ROOT_PASSWORD` set" and "this Application has
/// no such variable" are different diagnoses, and the second is a common
/// real cause of a container that will not start.
pub fn sanitize_environment(vars: &[(String, String, bool)]) -> Vec<(String, String)> {
    vars.iter()
        .map(|(key, value, is_secret)| {
            if *is_secret || is_secret_key(key) {
                (key.clone(), REDACTED.to_string())
            } else {
                (key.clone(), sanitize_text(value))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_names_are_recognised_in_every_casing_convention() {
        for key in ["DB_PASSWORD", "db-password", "dbPassword", "API_KEY", "apiKey", "api.key", "authToken", "MYSQL_ROOT_PASSWORD", "--requirepass", "PASSPHRASE", "AWS_SECRET_ACCESS_KEY"] {
            assert!(is_secret_key(key), "{key} should be treated as a secret");
        }
    }

    /// The other half of the bargain: a sanitizer that redacts everything
    /// leaves the model with nothing to diagnose from. These are the keys
    /// an operator most needs the model to actually see.
    #[test]
    fn ordinary_configuration_keys_are_left_alone() {
        for key in ["JAVA_HOME", "PORT", "TZ", "MYSQL_DATABASE", "MYSQL_USER", "PMA_HOST", "NODE_ENV", "monkey", "PASSENGER_COUNT"] {
            assert!(!is_secret_key(key), "{key} should not be treated as a secret");
        }
    }

    #[test]
    fn an_inline_assignment_loses_its_value_but_keeps_its_name() {
        assert_eq!(sanitize_text("MYSQL_ROOT_PASSWORD=hunter2"), format!("MYSQL_ROOT_PASSWORD={REDACTED}"));
        assert_eq!(sanitize_text("starting with DB_PASSWORD=abc123 and PORT=8080"), format!("starting with DB_PASSWORD={REDACTED} and PORT=8080"));
    }

    #[test]
    fn a_quoted_value_is_redacted_past_its_spaces() {
        assert_eq!(sanitize_text("password=\"two words\" next"), format!("password={REDACTED} next"));
    }

    /// The bearer token, not just the word `Bearer`. An unquoted value
    /// after a colon runs to the end of the line - see `redact_key_value_pairs`.
    #[test]
    fn a_bearer_header_loses_the_whole_token() {
        assert_eq!(sanitize_text("Authorization: Bearer sk-abcdef"), format!("Authorization: {REDACTED}"));
        let two_lines = sanitize_text("Authorization: Bearer sk-abcdef\nContent-Type: application/json");
        assert!(two_lines.contains("application/json"), "the next line must survive: {two_lines}");
        assert!(!two_lines.contains("sk-abcdef"));
    }

    /// A colon-separated value that is quoted stops at its quote, so a JSON
    /// fragment that arrived as free text keeps everything but the secret.
    #[test]
    fn quoted_json_text_loses_only_the_secret_value() {
        assert_eq!(
            sanitize_text("{\"authToken\": \"s3cret\", \"port\": 4222}"),
            format!("{{\"authToken\": {REDACTED}, \"port\": 4222}}")
        );
    }

    #[test]
    fn a_connection_string_keeps_its_user_and_host_but_not_its_password() {
        assert_eq!(
            sanitize_text("mysql://appuser:s3cr3t@db.internal:3306/appdb"),
            format!("mysql://appuser:{REDACTED}@db.internal:3306/appdb")
        );
    }

    /// A URL with no credentials in it must survive untouched - this is the
    /// common case, and mangling it would break the very context the model
    /// needs to reason about an endpoint.
    #[test]
    fn a_url_without_userinfo_is_unchanged() {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        assert_eq!(sanitize_text(url), url);
    }

    #[test]
    fn a_pem_block_is_replaced_whole() {
        let text = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEA\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let sanitized = sanitize_text(text);
        assert!(!sanitized.contains("b3BlbnNzaC1rZXktdjEA"));
        assert!(sanitized.contains("before"));
        assert!(sanitized.contains("after"));
    }

    /// A log tail cuts wherever the buffer ended, so the `END` line is
    /// frequently missing. Bailing out and leaving the fragment in would be
    /// the worst possible reading of "no terminator found".
    #[test]
    fn a_truncated_pem_block_redacts_to_the_end() {
        let sanitized = sanitize_text("log line\n-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA");
        assert!(!sanitized.contains("MIIEpAIBAAKCAQEA"));
        assert!(sanitized.contains("log line"));
    }

    #[test]
    fn a_redis_command_array_loses_the_password_after_the_flag() {
        let config = serde_json::json!({ "image": "redis:7", "command": ["redis-server", "--appendonly", "yes", "--requirepass", "hunter2"] });
        let sanitized = sanitize_json(&config);
        assert_eq!(sanitized["command"][4], serde_json::json!(REDACTED));
        // The surrounding, non-secret arguments have to survive - they are
        // most of what makes the config diagnosable.
        assert_eq!(sanitized["command"][1], serde_json::json!("--appendonly"));
        assert_eq!(sanitized["command"][2], serde_json::json!("yes"));
        assert_eq!(sanitized["image"], serde_json::json!("redis:7"));
    }

    #[test]
    fn a_nats_command_array_loses_the_token_but_keeps_the_store_dir() {
        let config = serde_json::json!({ "image": "nats:2", "command": ["-m", "8222", "-js", "--store_dir", ".", "--auth", "s3cret"] });
        let sanitized = sanitize_json(&config);
        assert_eq!(sanitized["command"][6], serde_json::json!(REDACTED));
        assert_eq!(sanitized["command"][4], serde_json::json!("."));
    }

    #[test]
    fn a_secret_object_key_is_replaced_whatever_its_type() {
        let config = serde_json::json!({ "authToken": { "value": "s3cret", "rotatedAt": 12 }, "port": 4222 });
        let sanitized = sanitize_json(&config);
        assert_eq!(sanitized["authToken"], serde_json::json!(REDACTED));
        assert_eq!(sanitized["port"], serde_json::json!(4222));
    }

    #[test]
    fn a_secret_flagged_environment_row_is_reported_as_present_not_dropped() {
        let vars = vec![
            ("MYSQL_ROOT_PASSWORD".to_string(), String::new(), true),
            ("MYSQL_DATABASE".to_string(), "appdb".to_string(), false),
            ("SOME_TOKEN".to_string(), "leaked-if-this-fails".to_string(), false),
        ];
        let sanitized = sanitize_environment(&vars);
        assert_eq!(sanitized[0], ("MYSQL_ROOT_PASSWORD".to_string(), REDACTED.to_string()));
        assert_eq!(sanitized[1], ("MYSQL_DATABASE".to_string(), "appdb".to_string()));
        // Not flagged as secret by the user, but named like one - the whole
        // point of checking the name as well as the flag.
        assert_eq!(sanitized[2], ("SOME_TOKEN".to_string(), REDACTED.to_string()));
    }
}
