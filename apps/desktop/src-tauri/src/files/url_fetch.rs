//! "Download from a link" in an Application's files - a plugin straight from
//! Modrinth, a world from a release page - fetched by the Node itself, so a
//! large file never makes the round trip through this computer.
//!
//! **What a link may be.** `http` or `https`, nothing else curl speaks. No
//! `user:password@` - the URL is a command-line argument on the Node, visible
//! in `ps` while it downloads (AGENTS.md 2), so a link that carries a login is
//! refused rather than leaked. And no address that is obviously internal -
//! loopback, private ranges, link-local (the cloud metadata service lives at
//! 169.254.169.254), `localhost`: a team member with file access could
//! otherwise have the Node fetch its own internal services into a folder
//! they can read. That check is on the literal host only; a public name that
//! resolves to a private address still gets through, and the threat model
//! says so rather than claiming otherwise.

use std::net::IpAddr;

use crate::errors::{AppError, AppResult};

/// The largest download accepted: 2 GiB. `curl` enforces it up front when
/// the server announces a size, and during the transfer when it does not.
pub(crate) const MAX_FETCH_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// A POSIX `sh` function, `fetch_to <url> <file>`, shared by every provider
/// that fetches on the Node so the flags cannot drift apart. `curl` where it
/// exists, `wget` otherwise, exit 11 when neither does. Redirects are
/// followed, but only to `http`/`https`, at most five of them.
pub(crate) fn fetch_function() -> String {
    format!(
        "fetch_to() {{\n\
         \x20   if command -v curl >/dev/null 2>&1; then\n\
         \x20       curl -fsSL --proto =http,https --proto-redir =http,https --max-redirs 5 --max-filesize {MAX_FETCH_BYTES} --max-time 590 -o \"$2\" -- \"$1\"\n\
         \x20   elif command -v wget >/dev/null 2>&1; then\n\
         \x20       wget -q --max-redirect=5 -T 60 -O \"$2\" -- \"$1\"\n\
         \x20   else\n\
         \x20       echo \"neither curl nor wget is installed on this Node\" >&2\n\
         \x20       return 11\n\
         \x20   fi\n\
         }}\n"
    )
}

/// A friendlier sentence for the exit codes that have an obvious cause.
pub(crate) fn describe_failure(exit_code: i32, stderr: &str) -> String {
    match exit_code {
        11 => "neither curl nor wget is installed on this Node - install one (sudo apt install curl) and try again".into(),
        63 => "the file is larger than the 2 GB a download from a link may be".into(),
        6 => "the address in the link couldn't be found".into(),
        28 => "the download took too long and was stopped".into(),
        22 => format!("the server refused the download: {}", stderr.trim()),
        _ if stderr.trim().is_empty() => format!("the download failed (exit {exit_code})"),
        _ => format!("the download failed: {}", stderr.trim()),
    }
}

fn is_internal(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast() || v4.octets()[0] == 0,
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7, unique local
                || (first & 0xfe00) == 0xfc00
                // fe80::/10, link-local
                || (first & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| is_internal(IpAddr::V4(v4)))
        }
    }
}

/// Checks a link against the rules in the module comment and returns it as
/// it will be handed to `curl`.
pub(crate) fn validate_fetch_url(input: &str) -> AppResult<String> {
    let text = input.trim();
    if text.is_empty() {
        return Err(AppError::InvalidInput("paste the link to download".into()));
    }
    if text.len() > 2048 {
        return Err(AppError::InvalidInput("that link is too long".into()));
    }
    let url = reqwest::Url::parse(text).map_err(|_| AppError::InvalidInput(format!("'{text}' isn't a valid link")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::InvalidInput("only http and https links can be downloaded".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidInput(
            "a link with a login in it can't be used - it would be visible to every account on the Node while it downloads".into(),
        ));
    }
    let host = url.host_str().filter(|host| !host.is_empty()).ok_or_else(|| AppError::InvalidInput("the link has no address".into()))?;
    // An IPv6 host comes back in brackets; the parser has already turned any
    // spelling of an IPv4 address (hex, octal, dotless) into dotted decimal.
    let internal = match host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        Ok(address) => is_internal(address),
        Err(_) => {
            let domain = host.trim_end_matches('.').to_ascii_lowercase();
            domain == "localhost" || domain.ends_with(".localhost")
        }
    };
    if internal {
        return Err(AppError::InvalidInput("links to this Node itself or its private network can't be downloaded".into()));
    }
    Ok(url.to_string())
}

/// The name a server gives a download, for the "Save as" field.
///
/// A link's last path segment is often not the file - \`/download\`,
/// \`/v2/resources/123/download\`, a version id - and the real name only
/// arrives with the response: in \`Content-Disposition\`, or as the path the
/// redirects end on. Asked from this computer with a \`HEAD\` (or, for a
/// server that refuses one, a \`GET\` dropped after its headers), under the same
/// rules as the download itself. \`None\` when nothing better than the link
/// itself is on offer.
pub(crate) async fn suggest_file_name(url: &str) -> AppResult<Option<String>> {
    let url = validate_fetch_url(url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|err| AppError::Internal(format!("couldn't start the request: {err}")))?;
    let mut response = client.head(&url).send().await.map_err(|err| AppError::Connection(format!("couldn't reach the link: {err}")))?;
    if !response.status().is_success() {
        response = client.get(&url).send().await.map_err(|err| AppError::Connection(format!("couldn't reach the link: {err}")))?;
    }
    let from_header = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(file_name_from_disposition);
    let from_final_url = response.url().path_segments().and_then(|mut segments| segments.next_back()).and_then(|segment| {
        let decoded = percent_decode(segment);
        // Only a segment that looks like a file name - "download" is not one.
        decoded.contains('.').then_some(decoded)
    });
    Ok(from_header.or(from_final_url).and_then(|name| safe_file_name(&name)))
}

/// The file name in a \`Content-Disposition\` header: the RFC 5987
/// \`filename*=UTF-8''...\` form when there is one, the plain \`filename=\` otherwise.
fn file_name_from_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        let Some((key, value)) = part.split_once('=') else { continue };
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => {
                let value = value.trim().trim_matches('"');
                let encoded = value.split_once("''").map_or(value, |(_, rest)| rest);
                return Some(percent_decode(encoded));
            }
            "filename" => plain = Some(value.trim().trim_matches('"').to_string()),
            _ => {}
        }
    }
    plain
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(high * 16 + low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A name, never a path: separators and anything that could step out of the
/// folder go, as do leading dots.
fn safe_file_name(name: &str) -> Option<String> {
    let cleaned: String = name.chars().filter(|c| !matches!(c, '/' | '\\') && !c.is_control()).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').chars().take(200).collect::<String>();
    (!cleaned.is_empty()).then_some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_content_disposition_name_is_read_in_either_form() {
        assert_eq!(file_name_from_disposition("attachment; filename=\"LuckPerms-Bukkit-5.4.jar\"").as_deref(), Some("LuckPerms-Bukkit-5.4.jar"));
        assert_eq!(file_name_from_disposition("attachment; filename=plugin.jar").as_deref(), Some("plugin.jar"));
        assert_eq!(
            file_name_from_disposition("attachment; filename=\"fallback.jar\"; filename*=UTF-8''Mój%20plugin.jar").as_deref(),
            Some("Mój plugin.jar")
        );
        assert_eq!(file_name_from_disposition("inline"), None);
    }

    #[test]
    fn a_suggested_name_is_a_name_and_never_a_path() {
        assert_eq!(safe_file_name("../../etc/passwd").as_deref(), Some("etcpasswd"));
        assert_eq!(safe_file_name("..\\x.jar").as_deref(), Some("x.jar"));
        assert_eq!(safe_file_name("   "), None);
        assert_eq!(percent_decode("a%20b%2Ejar"), "a b.jar");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn an_ordinary_public_link_is_accepted() {
        assert_eq!(
            validate_fetch_url(" https://cdn.modrinth.com/data/abc/versions/1.0/plugin.jar ").unwrap(),
            "https://cdn.modrinth.com/data/abc/versions/1.0/plugin.jar"
        );
        assert!(validate_fetch_url("http://example.com/world.zip").is_ok());
        assert!(validate_fetch_url("https://1.1.1.1/x").is_ok());
    }

    #[test]
    fn other_schemes_are_refused() {
        for link in ["file:///etc/passwd", "ftp://example.com/x", "gopher://example.com", "scp://host/x", "javascript:alert(1)", "not a link"] {
            assert!(validate_fetch_url(link).is_err(), "{link}");
        }
    }

    #[test]
    fn a_login_in_the_link_is_refused_rather_than_put_on_a_command_line() {
        assert!(validate_fetch_url("https://user:secret@example.com/x.jar").is_err());
        assert!(validate_fetch_url("https://token@example.com/x.jar").is_err());
    }

    #[test]
    fn internal_addresses_are_refused() {
        for link in [
            "http://127.0.0.1/",
            "http://localhost:8080/",
            "http://LOCALHOST./",
            "http://api.localhost/",
            "http://10.0.0.5/",
            "http://192.168.1.1/",
            "http://172.16.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fd00::1]/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            assert!(validate_fetch_url(link).is_err(), "{link}");
        }
    }

    #[test]
    fn the_shell_function_restricts_protocols_and_caps_the_size() {
        let function = fetch_function();
        assert!(function.contains("--proto =http,https --proto-redir =http,https"));
        assert!(function.contains(&format!("--max-filesize {MAX_FETCH_BYTES}")));
        assert!(function.contains("-- \"$1\""), "the url can never be read as an option");
    }
}
