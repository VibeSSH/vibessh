//! Live health of a Paper or Purpur server, read the way spark never could:
//! over the SSH tunnel VibeSSH already holds, never a public link.
//!
//! spark uploads a snapshot to a bytebin URL anyone with the link can open.
//! This asks the running server three questions - `/tps`, `/mspt`, `/list` -
//! over RCON bound to the server's loopback and reached through a forwarded
//! channel, so the numbers render in the desktop app and nothing leaves the
//! tunnel. The password lives in the OS keyring, never in a config file.
//!
//! Two halves, both pure and both tested here without a socket: the RCON wire
//! codec (`encode_packet` / `decode_packet`), and the parsers that turn a
//! server's coloured console text into numbers. TPS and MSPT live inside the
//! JVM, so there is no plugin-free way to read them other than asking the
//! server in its own language and reading the reply - which is exactly what
//! the existing `SshSession::get_metrics` does for the host, one layer down.

// ---- RCON wire codec ---------------------------------------------------
//
// Packet: i32 LE length (of everything after it) | i32 LE request id |
//         i32 LE type | body bytes | 0x00 | 0x00.
// Types: 3 = auth (login), 2 = run a command, 0 = a response line.
// On a good login the server echoes the request id; on a bad one it sends -1.

pub const RCON_TYPE_AUTH: i32 = 3;
pub const RCON_TYPE_COMMAND: i32 = 2;
// Named for completeness of the protocol vocabulary; only the tests below
// reference it, since a reply body is read by content, not by type tag.
#[allow(dead_code)]
pub const RCON_TYPE_RESPONSE: i32 = 0;

/// The id an auth response carries when the password was wrong.
pub const RCON_AUTH_FAILED: i32 = -1;

/// Frames one packet ready for the wire. Bodies are ASCII commands, so the
/// two trailing nulls are the body terminator plus the packet's empty-string
/// terminator, per the protocol.
pub fn encode_packet(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let body = body.as_bytes();
    // id (4) + type (4) + body + two nulls.
    let length = (4 + 4 + body.len() + 2) as i32;
    let mut out = Vec::with_capacity(4 + length as usize);
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(body);
    out.push(0);
    out.push(0);
    out
}

/// Reads one packet from the front of `buf`, returning it and how many bytes
/// it consumed, or `None` if `buf` does not yet hold a whole packet. A caller
/// loops on this because one command's reply can arrive as several packets.
pub fn decode_packet(buf: &[u8]) -> Option<(i32, i32, String, usize)> {
    if buf.len() < 4 {
        return None;
    }
    let length = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if length < 10 {
        // Below id + type + two nulls: not a packet this side ever sends.
        return None;
    }
    let total = 4 + length as usize;
    if buf.len() < total {
        return None;
    }
    let id = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let kind = i32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    // Body is everything between the header and the two terminating nulls.
    let body = String::from_utf8_lossy(&buf[12..total - 2]).into_owned();
    Some((id, kind, body, total))
}

/// Drops Minecraft's section-sign colour codes, so a parser sees `20.0`, not
/// the coloured form. The section sign is U+00A7 - one `char`, two UTF-8
/// bytes - so this works on the decoded `String`, not the raw bytes.
pub fn strip_color(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{00A7}' {
            chars.next(); // the code character after the section sign
        } else {
            out.push(c);
        }
    }
    out
}

// ---- Console-text parsers ---------------------------------------------

/// `TPS from last 1m, 5m, 15m: *20.0, 19.98, 18.7` -> the three numbers.
/// The `*` Paper prints when a figure is capped at 20 is dropped; a reader
/// wants the number, and the cap is already visible as "20.0".
pub fn parse_tps(text: &str) -> Option<(f32, f32, f32)> {
    let clean = strip_color(text);
    let after = clean.split(':').nth(1)?;
    let mut nums = after
        .split(',')
        .filter_map(|piece| piece.trim().trim_start_matches('*').trim().parse::<f32>().ok());
    let one = nums.next()?;
    let five = nums.next()?;
    let fifteen = nums.next()?;
    Some((one, five, fifteen))
}

/// The MSPT block is two lines:
///   `Server tick times (avg/min/max) from last 5s, 10s, 1m:`
///   `0.7/0.4/4.6, 0.8/0.4/10.2, 0.9/0.4/40.1`
/// Returns avg and max of the *first* (shortest, most recent) window, because
/// that is the one that answers "is it stuttering right now".
pub fn parse_mspt(text: &str) -> Option<(f32, f32)> {
    let clean = strip_color(text);
    // The data line is the one holding `/`-separated triples; the header does
    // not. Take the first triple on it.
    for line in clean.lines() {
        let Some(first_group) = line.split(',').next() else {
            continue;
        };
        let triple: Vec<f32> = first_group
            .split('/')
            .filter_map(|p| {
                p.trim()
                    .trim_start_matches(|c: char| !c.is_ascii_digit() && c != '.')
                    .parse::<f32>()
                    .ok()
            })
            .collect();
        if triple.len() == 3 {
            return Some((triple[0], triple[2]));
        }
    }
    None
}

/// `There are 3 of a max of 20 players online: Alice, Bob, Carol`
/// Also handles the zero case, where there is no `:` and no names.
pub fn parse_list(text: &str) -> Option<(u32, u32, Vec<String>)> {
    let clean = strip_color(text);
    let online_at = clean.find(" of a max of")?;
    let online: u32 = clean[..online_at]
        .rsplit(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())?
        .parse()
        .ok()?;
    let rest = &clean[online_at + " of a max of".len()..];
    let max: u32 = rest.split_whitespace().next()?.parse().ok()?;
    let names = match clean.split_once(':') {
        Some((_, list)) => list
            .split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect(),
        None => Vec::new(),
    };
    Some((online, max, names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_round_trips() {
        let frame = encode_packet(7, RCON_TYPE_COMMAND, "tps");
        assert_eq!(
            i32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize,
            frame.len() - 4
        );
        let (id, kind, body, used) = decode_packet(&frame).unwrap();
        assert_eq!((id, kind, body.as_str(), used), (7, RCON_TYPE_COMMAND, "tps", frame.len()));
    }

    #[test]
    fn decode_waits_for_a_whole_packet() {
        let frame = encode_packet(1, RCON_TYPE_RESPONSE, "hello");
        assert!(decode_packet(&frame[..frame.len() - 3]).is_none(), "half a packet must not decode");
        let mut stream = frame.clone();
        stream.extend(encode_packet(2, RCON_TYPE_RESPONSE, "world"));
        let (_, _, first, used) = decode_packet(&stream).unwrap();
        assert_eq!(first, "hello");
        let (_, _, second, _) = decode_packet(&stream[used..]).unwrap();
        assert_eq!(second, "world");
    }

    #[test]
    fn auth_failure_is_minus_one() {
        let reply = encode_packet(RCON_AUTH_FAILED, RCON_TYPE_RESPONSE, "");
        let (id, _, _, _) = decode_packet(&reply).unwrap();
        assert_eq!(id, RCON_AUTH_FAILED);
    }

    #[test]
    fn strips_section_colour_codes() {
        assert_eq!(strip_color("\u{00A7}a20.0\u{00A7}r"), "20.0");
        assert_eq!(strip_color("no codes here"), "no codes here");
    }

    #[test]
    fn parses_paper_tps_with_colour_and_cap() {
        let line = "\u{00A7}6TPS from last 1m, 5m, 15m: \u{00A7}a*20.0, \u{00A7}a19.98, \u{00A7}e18.71";
        assert_eq!(parse_tps(line), Some((20.0, 19.98, 18.71)));
    }

    #[test]
    fn parses_purpur_tps_plain() {
        assert_eq!(parse_tps("TPS from last 1m, 5m, 15m: 20.0, 20.0, 20.0"), Some((20.0, 20.0, 20.0)));
    }

    #[test]
    fn parses_mspt_first_window() {
        let block = "\u{00A7}6Server tick times (avg/min/max) from last 5s, 10s, 1m:\n\u{00A7}a0.7/0.4/4.6, \u{00A7}a0.8/0.4/10.2, \u{00A7}e0.9/0.4/40.1";
        assert_eq!(parse_mspt(block), Some((0.7, 4.6)));
    }

    #[test]
    fn parses_list_with_names() {
        let line = "There are \u{00A7}a3\u{00A7}r of a max of \u{00A7}a20\u{00A7}r players online: Alice, Bob, Carol";
        assert_eq!(parse_list(line), Some((3, 20, vec!["Alice".into(), "Bob".into(), "Carol".into()])));
    }

    #[test]
    fn parses_empty_server() {
        let line = "There are 0 of a max of 20 players online";
        assert_eq!(parse_list(line), Some((0, 20, vec![])));
    }

    #[test]
    fn rejects_junk_instead_of_inventing_numbers() {
        assert_eq!(parse_tps("Unknown command."), None);
        assert_eq!(parse_mspt("Unknown command."), None);
        assert_eq!(parse_list("Unknown command."), None);
    }
}
